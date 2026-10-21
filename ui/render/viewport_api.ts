// The viewport for the rest of the interface: `viewport`, one per window. The camera, the
// settings, what the cursor points at, picking, passes and overlays drawn each frame, the capture
// of the view. Viewport.svelte attaches it to its canvas; it draws on demand (requestRedraw),
// every frame while the camera moves or something animates.

import { Camera } from "./camera";
import { CameraControls } from "./controls";
import { call, engineReady, onEngine } from "./engine";
import { GizmoController } from "./gizmo";
import {
  aabbValid,
  col3,
  radians,
  translate,
  type AABB,
  type Mat4,
  type Ray,
  type Vec2,
  type Vec3,
} from "./math";
import { Renderer } from "./renderer";
import { store, type MeshEntry } from "./scene_store";
import {
  defaultRenderSettings,
  defaultWaterLook,
  NodeFlag,
  NodeKind,
  type PickHit,
  type RenderSettings,
  type WaterLook,
} from "./types";

// where a pass draws in the frame (app.cpp frame()): "opaque" after the grid and before the scene
// (the game view's decor), "translucent" after the scene (the game view's translucent decor),
// "overlay" after the ocean (effects, nav meshes, holes, the tools' overlays, previews)
export type PassStage = "opaque" | "translucent" | "overlay";
export type Pass = (r: Renderer, rs: RenderSettings) => void;

// the ocean (drawWater): its mesh (the engine's, mesh/<uid>), its height, its look; far: the sea
// to the horizon behind everything
export interface Ocean {
  mesh: number;
  version: number;
  height: number;
  look?: WaterLook;
  far?: boolean;
  far_color?: [number, number, number];
}

export interface CameraState {
  pos: Vec3;
  yaw: number; // radians
  pitch: number;
  fov: number; // degrees
}

class ViewportApi {
  readonly camera = new Camera();
  readonly store = store;
  settings: RenderSettings = defaultRenderSettings();
  canvas: HTMLCanvasElement | null = null;
  renderer: Renderer | null = null;
  controls: CameraControls | null = null;
  gizmo: GizmoController | null = null;
  // what the cursor points at (a tool sets it): tinted, outlined with what is under it
  hovered = 0;
  // the outlines a tool asks for: what a box would select (null: the hovered element's subtree),
  // the rest of the hovered element's group
  outlineHover: number[] | null = null;
  outlineGroup: number[] = [];
  // the ocean's look as drawn: the game view gives it the hour's (game_view.ts oceanLook)
  waterLook: ((look: WaterLook, rs: RenderSettings) => WaterLook) | null = null;

  private overlay: HTMLCanvasElement | null = null;
  private passes: { stage: PassStage; fn: Pass; capture: boolean }[] = [];
  private overlays_2d = new Set<(ctx: CanvasRenderingContext2D) => void>();
  private animators = 0;
  private raf = 0;
  private last_t = 0;
  private continuous = false;
  private time = 0;
  private css: Vec2 = [1, 1];
  private dpr = 1;
  private ocean: Ocean | null = null;
  private ocean_entry: MeshEntry | null = null;
  private outline_key = -1;
  private outlineSelection: number[] = [];
  private sent_camera = "";
  private camera_timer = 0;
  private off: (() => void)[] = [];

  // ---------------------------------------------------------------------------------------------
  // lifetime (Viewport.svelte)

  async attach(canvas: HTMLCanvasElement, overlay: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", {
      antialias: true,
      depth: true,
      stencil: false,
      alpha: false,
    });
    if (!gl) throw new Error("WebGL2 is not available");
    this.canvas = canvas;
    this.overlay = overlay;
    this.renderer = new Renderer(gl);
    store.freeGpu = (g) => this.renderer?.free(g);
    this.controls = new CameraControls(
      this.camera,
      canvas,
      store,
      () => this.cameraChanged(),
      async (x, y) => (await this.pick(x, y))?.point ?? null,
    );
    this.gizmo = new GizmoController({
      camera: this.camera,
      canvas,
      store,
      project: this.project,
      size: () => this.size(),
      requestRedraw: () => this.requestRedraw(),
      addOverlay2d: (fn) => this.addOverlay2d(fn),
    });
    await engineReady;
    this.off.push(store.subscribe(() => this.requestRedraw()));
    this.off.push(store.start());
    this.off.push(onEngine("jle:request", (p) => void this.engineRequest(p)));
    this.off.push(
      onEngine(
        "jle:changed",
        (p) => p?.document && void this.documentChanged(),
      ),
    );
    void this.documentChanged();
    // View > Frame the selection (F), Frame the level (Home): lib/actions frameView()
    const frame = (e: Event) =>
      (e as CustomEvent).detail === "all"
        ? this.frameAll()
        : this.frameSelection();
    window.addEventListener("jle:frame", frame);
    this.off.push(() => window.removeEventListener("jle:frame", frame));
    this.requestRedraw();
  }

  detach() {
    for (const f of this.off) f();
    this.off = [];
    this.controls?.dispose();
    this.gizmo?.dispose();
    cancelAnimationFrame(this.raf);
    this.raf = 0;
    store.reset();
    store.freeGpu = () => {};
    this.renderer = this.controls = this.gizmo = null;
    this.canvas = this.overlay = null;
  }

  // the canvas's size in CSS pixels and the screen's pixel ratio
  resize(w: number, h: number, dpr: number) {
    this.css = [Math.max(w, 1), Math.max(h, 1)];
    this.dpr = dpr;
    for (const c of [this.canvas, this.overlay]) {
      if (!c) continue;
      c.width = Math.max(1, Math.round(w * dpr));
      c.height = Math.max(1, Math.round(h * dpr));
    }
    this.requestRedraw();
  }

  size(): Vec2 {
    return [...this.css];
  }

  // ---------------------------------------------------------------------------------------------
  // drawing

  requestRedraw() {
    if (!this.raf && this.renderer)
      this.raf = requestAnimationFrame((t) => this.tick(t));
  }

  // something animates (a tool's preview): the view draws every frame until the returned function
  animate(): () => void {
    this.animators++;
    this.requestRedraw();
    let done = false;
    return () => {
      if (!done) this.animators--;
      done = true;
    };
  }

  setSettings(s: Partial<RenderSettings>) {
    Object.assign(this.settings, s);
    this.requestRedraw();
  }

  setOcean(o: Ocean | null) {
    if (this.ocean_entry) store.releaseMesh(this.ocean_entry.uid);
    this.ocean = o;
    this.ocean_entry = o ? store.retainMesh(o.mesh, o.version) : null;
    this.requestRedraw();
  }

  // a pass of the frame: `capture` false leaves it out of capture() (the C++ capture draws the
  // effects, nav meshes and holes, not the tools)
  addPass(
    stage: PassStage,
    fn: Pass,
    opts: { capture?: boolean } = {},
  ): () => void {
    const p = { stage, fn, capture: opts.capture ?? false };
    this.passes.push(p);
    this.requestRedraw();
    return () => {
      this.passes = this.passes.filter((x) => x !== p);
      this.requestRedraw();
    };
  }
  // shorthand: the overlays drawn after everything (the immediate batch: renderer.line(), tri(),
  // boxLines(), circle(), then flush(depth_test, depth_offset))
  addOverlay(fn: Pass, opts: { capture?: boolean } = {}) {
    return this.addPass("overlay", fn, opts);
  }
  // 2D over the view (CSS pixels): the gizmo, the readouts, the box being drawn
  addOverlay2d(fn: (ctx: CanvasRenderingContext2D) => void): () => void {
    this.overlays_2d.add(fn);
    this.requestRedraw();
    return () => {
      this.overlays_2d.delete(fn);
      this.requestRedraw();
    };
  }

  private tick(t: number) {
    this.raf = 0;
    // the time between frames when drawing every frame, else a frame's
    const dt = this.continuous
      ? Math.min((t - this.last_t) / 1000, 0.1)
      : 1 / 60;
    this.last_t = t;
    this.controls?.tick(dt);
    this.time += dt;
    this.draw(
      this.css[0] * this.dpr,
      this.css[1] * this.dpr,
      this.css[0],
      this.css[1],
      false,
    );
    const ctx = this.overlay?.getContext("2d");
    if (ctx) {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, this.overlay!.width, this.overlay!.height);
      ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
      for (const fn of this.overlays_2d) fn(ctx);
    }
    this.continuous =
      !!this.controls?.busy ||
      this.animators > 0 ||
      store.animated ||
      !!(this.ocean && this.settings.show_ocean);
    if (this.continuous) this.requestRedraw();
  }

  private updateOutlines() {
    const r = this.renderer!;
    // the selection with what is under it (a group of a whole level: its first thousands)
    if (this.outline_key !== store.revision) {
      this.outline_key = store.revision;
      this.outlineSelection = [];
      for (const id of store.selectionRoots())
        this.renderSubtree(id, this.outlineSelection, 6000);
      if (!this.outlineSelection.length)
        this.outlineSelection = store.selection;
    }
    r.outlineSelection = this.outlineSelection;
    r.outlineGroup = this.outlineGroup;
    if (this.outlineHover) {
      r.outlineHover = this.outlineHover;
    } else {
      r.outlineHover = [];
      const h = this.controls?.busy ? undefined : store.nodes.get(this.hovered);
      if (h && !(h.flags & NodeFlag.Selected))
        this.renderSubtree(h.id, r.outlineHover, 4000);
    }
  }
  private renderSubtree(id: number, out: number[], cap: number) {
    for (const i of store.subtree(id)) {
      if (out.length >= cap) return;
      if (store.nodes.get(i)!.kind === NodeKind.Render) out.push(i);
    }
  }

  // One frame into the bound framebuffer (app.cpp frame()): w x h pixels, lw x lh CSS pixels
  private draw(w: number, h: number, lw: number, lh: number, capture: boolean) {
    const r = this.renderer;
    if (!r) return;
    const rs: RenderSettings = { ...this.settings, time: this.time };
    // the far ocean: the sea to the horizon (set_ocean_backdrop)
    if (this.ocean) {
      rs.far_ocean = !!this.ocean.far && store.nodes.size > 0;
      rs.far_ocean_y = this.ocean.height;
      if (this.ocean.far_color) rs.far_ocean_color = this.ocean.far_color;
    }
    const passes = this.passes.filter((p) => !capture || p.capture);
    r.begin(this.camera, w, h, rs, lw, lh);
    for (const p of passes) if (p.stage === "opaque") p.fn(r, rs);
    this.updateOutlines();
    if (capture) {
      // what the cursor points at is not in the picture
      r.outlineHover = [];
      r.outlineGroup = [];
    }
    // nothing is pointed at while the camera moves
    r.drawScene(store, rs, capture || this.controls?.busy ? 0 : this.hovered);
    for (const p of passes) if (p.stage === "translucent") p.fn(r, rs);
    if (
      rs.show_ocean &&
      this.ocean &&
      this.ocean_entry &&
      store.nodes.size > 0
    ) {
      const look = this.ocean.look ?? defaultWaterLook();
      r.drawWater(
        this.ocean_entry,
        translate([0, this.ocean.height, 0]),
        this.waterLook?.(look, rs) ?? look,
        rs,
      );
    }
    for (const p of passes) if (p.stage === "overlay") p.fn(r, rs);
    r.setGameDepth(null);
  }

  // A picture of the view at that size (the MCP server's capture_view): PNG, base64
  async capture(w: number, h: number): Promise<string> {
    const r = this.renderer;
    if (!r) throw new Error("no viewport");
    const gl = r.gl;
    const fbo = gl.createFramebuffer()!;
    const color = gl.createRenderbuffer()!,
      depth = gl.createRenderbuffer()!;
    gl.bindRenderbuffer(gl.RENDERBUFFER, color);
    gl.renderbufferStorage(gl.RENDERBUFFER, gl.RGBA8, w, h);
    gl.bindRenderbuffer(gl.RENDERBUFFER, depth);
    gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT24, w, h);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferRenderbuffer(
      gl.FRAMEBUFFER,
      gl.COLOR_ATTACHMENT0,
      gl.RENDERBUFFER,
      color,
    );
    gl.framebufferRenderbuffer(
      gl.FRAMEBUFFER,
      gl.DEPTH_ATTACHMENT,
      gl.RENDERBUFFER,
      depth,
    );
    const pixels = new Uint8Array(w * h * 4);
    try {
      this.draw(w, h, w, h, true);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    } finally {
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.deleteFramebuffer(fbo);
      gl.deleteRenderbuffer(color);
      gl.deleteRenderbuffer(depth);
      // the view's own projection back
      r.setView(
        this.camera,
        this.css[0] * this.dpr,
        this.css[1] * this.dpr,
        this.css[0],
        this.css[1],
      );
      this.requestRedraw();
    }
    // bottom-up rows to a PNG (the browser's encoder)
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const ctx = c.getContext("2d")!;
    const img = ctx.createImageData(w, h);
    for (let y = 0; y < h; y++)
      img.data.set(
        pixels.subarray((h - 1 - y) * w * 4, (h - y) * w * 4),
        y * w * 4,
      );
    ctx.putImageData(img, 0, 0);
    return c.toDataURL("image/png").slice("data:image/png;base64,".length);
  }

  // ---------------------------------------------------------------------------------------------
  // the camera

  // another document (app.cpp new_document, then poll_level_opening / open_project_file): the
  // camera as it starts, then everything framed once the meshes are read
  private async documentChanged() {
    Object.assign(this.camera, {
      pos: [0, 10, 30],
      yaw: 0,
      pitch: -0.3,
      speed: 12,
    });
    this.cameraChanged();
    await store.refresh();
    await store.meshesLoaded();
    if (store.nodes.size) this.frameAll();
  }

  getCamera(): CameraState {
    const c = this.camera;
    return { pos: [...c.pos], yaw: c.yaw, pitch: c.pitch, fov: c.fov_deg };
  }
  setCamera(s: Partial<CameraState> & { look_at?: Vec3 }) {
    const c = this.camera;
    if (s.pos) c.pos = [...s.pos];
    if (s.yaw !== undefined) c.yaw = s.yaw;
    if (s.pitch !== undefined)
      c.pitch = Math.min(Math.max(s.pitch, -1.55), 1.55);
    if (s.fov !== undefined) c.fov_deg = s.fov;
    if (s.look_at) c.lookAt(s.look_at);
    this.cameraChanged();
  }
  // frames a box: keeps the orientation, backs off so the box fits
  frame(box: AABB) {
    if (!aabbValid(box)) return;
    this.camera.frame(box);
    this.cameraChanged();
  }
  frameAll() {
    this.controls?.frameAll();
  }
  frameSelection() {
    this.controls?.frameSelection();
  }

  private cameraChanged() {
    this.requestRedraw();
    // the engine knows where the camera is once it stops moving (the MCP server's get_status)
    clearTimeout(this.camera_timer);
    this.camera_timer = window.setTimeout(() => {
      const s = this.getCamera();
      const key = JSON.stringify(s);
      if (key === this.sent_camera) return;
      this.sent_camera = key;
      call("set_camera_state", { ...s }).catch(() => {});
    }, 300);
  }

  // the engine asks (jle:request): {kind: "camera", pos | position, yaw, pitch (radians) or
  // yaw_deg, pitch_deg, fov, look_at, frame: "all" | "selection" | {lo, hi}}, {kind: "capture",
  // width, height}; answered with call("frontend_reply", {id, ...}) when it has an id
  private async engineRequest(p: any) {
    if (!p || typeof p !== "object") return;
    let answer: Record<string, unknown> | null = null;
    try {
      if (p.kind === "camera") {
        if (p.frame === "all") this.frameAll();
        else if (p.frame === "selection") this.frameSelection();
        else if (p.frame?.lo && p.frame?.hi)
          this.frame({ lo: p.frame.lo, hi: p.frame.hi });
        this.setCamera({
          pos: p.pos ?? p.position,
          yaw:
            p.yaw ?? (p.yaw_deg !== undefined ? radians(p.yaw_deg) : undefined),
          pitch:
            p.pitch ??
            (p.pitch_deg !== undefined ? radians(p.pitch_deg) : undefined),
          fov: p.fov,
          look_at: p.look_at,
        });
        answer = { camera: this.getCamera() };
      } else if (p.kind === "capture") {
        const w = Math.min(Math.max(p.width ?? 960, 64), 2048),
          h = Math.min(Math.max(p.height ?? 600, 64), 2048);
        answer = {
          png: await this.capture(w, h),
          width: w,
          height: h,
          camera: this.getCamera(),
        };
      } else {
        return;
      }
    } catch (e) {
      answer = { error: String(e) };
    }
    if (p.id !== undefined)
      call("frontend_reply", { id: p.id, ...answer }).catch(() => {});
  }

  // ---------------------------------------------------------------------------------------------
  // picking

  // the ray through a point of the view (CSS pixels, origin top-left)
  rayFromMouse(x: number, y: number): Ray {
    return this.camera.ray(x, y, this.css[0], this.css[1]);
  }
  // world -> view CSS pixels, null behind the camera (the view as last drawn)
  project = (world: Vec3): Vec2 | null => this.renderer?.project(world) ?? null;
  // a mouse event's point in the view
  mouse(e: MouseEvent): Vec2 {
    const r = this.canvas!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }
  // the first surface under a point of the view (the engine's BVH): extra goes with the call
  // (a filter: {actors, navmesh, include_locked...})
  async pick(
    x: number,
    y: number,
    extra: Record<string, unknown> = {},
  ): Promise<PickHit | null> {
    const { origin, dir } = this.rayFromMouse(x, y);
    return (
      ((await call("pick", { origin, dir, ...extra })) as PickHit | null) ??
      null
    );
  }
  // the gizmo's frame for the selection (its middle, turned as the primary element)
  selectionPivot(primary = 0): Vec3 {
    return col3(store.selectionFrame(primary), 3);
  }
}

export type { Mat4 };
export type Viewport = ViewportApi;
export const viewport = new ViewportApi();
