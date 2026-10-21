// The select tool (src/app/app.cpp viewport_interaction, select_tool, drag_offset, update_outlines,
// draw_cursor_info, draw_move_preview, start/update/finish_free_rotation): a click selects what the
// cursor points at (Shift or Ctrl: in or out), a click again at the same place the next element
// under it, a double click or the button held still its whole group, a drag on empty space a box,
// a drag on the selection moves it, the right button held on the selection turns it. The view shows
// what the mouse does while it moves (SceneStore.setPreview); what it decides goes to the engine as
// one call (one undo step). It acts only while the tool is "select" (engine.state.tool): the other
// tools take the mouse then, and the gizmo is off.

import { engine } from "../lib/engine.svelte";
import { t } from "../lib/i18n";
import { shortcuts } from "../lib/shortcuts";
import type { Camera } from "./camera";
import { call, engineReady, onEngine } from "./engine";
import { GizmoOp, GizmoSpace, type GizmoSnap } from "./gizmo";
import { lockPointer } from "./controls";
import { attachLabels, rgba, textBox } from "./labels";
import {
  add,
  axisAngle,
  col3,
  col32,
  dot,
  length,
  length2,
  matMul,
  mul,
  normalize,
  quatMul,
  radians,
  rotate,
  sub,
  translate,
  type Mat4,
  type Vec2,
  type Vec3,
} from "./math";
import type { Renderer } from "./renderer";
import { store } from "./scene_store";
import { NodeFlag, NodeKind, type PickHit, type RenderSettings } from "./types";
import { viewport } from "./viewport_api";

// pick's answer: `selectable` is what a click selects (the actor of a model, 0 on the locked ground)
type Hit = PickHit & { selectable: number; locked?: boolean };

const now = () => performance.now() / 1000;
const dist = (a: Vec2, b: Vec2) => Math.hypot(a[0] - b[0], a[1] - b[1]);
const selected = (id: number) =>
  ((store.nodes.get(id)?.flags ?? 0) & NodeFlag.Selected) !== 0;
const camKey = (c: Camera) => `${c.pos}|${c.yaw}|${c.pitch}`;
// ImGui's double click: io.MouseDoubleClickTime, io.MouseDoubleClickMaxDist
const kDoubleClickTime = 0.3;
const kDoubleClickDist = 6;

// the render nodes under these (what an outline draws), at most `cap`
function renderNodes(ids: Iterable<number>, cap: number): number[] {
  const out: number[] = [];
  for (const id of ids)
    for (const i of store.subtree(id)) {
      if (out.length >= cap) return out;
      if (store.nodes.get(i)!.kind === NodeKind.Render) out.push(i);
    }
  return out;
}

/**
 * How far the mouse moved something, `px` pixels of it (drag_offset): a pixel of the mouse is a
 * pixel of the view where `at` stands; left and right along the view's horizontal, up and down
 * higher and lower (`depth`, Ctrl: forward and back instead), on the snap's steps. The place tool
 * moves what it places with it too.
 */
export function dragOffset(
  cam: Camera,
  viewHeight: number,
  px: Vec2,
  at: Vec3,
  depth: boolean,
  snap: GizmoSnap,
): Vec3 {
  // a pixel of the mouse, a pixel of the view where it stands
  const mpp =
    (2 * length(sub(at, cam.pos)) * Math.tan(radians(cam.fov_deg) * 0.5)) /
    Math.max(1, viewHeight);
  const r = cam.right();
  let side: Vec3 = [r[0], 0, r[2]];
  side = length2(side) > 1e-6 ? normalize(side) : [1, 0, 0];
  let ahead: Vec3 = [-side[2], 0, side[0]]; // the view's forward, level
  if (dot(ahead, cam.forward()) < 0) ahead = mul(ahead, -1);
  let off = add(
    mul(side, px[0] * mpp),
    mul(depth ? ahead : [0, 1, 0], -px[1] * mpp),
  );
  if (snap.enabled && snap.translate > 0) {
    const s = snap.translate;
    off = off.map((v) => Math.round(v / s) * s) as Vec3;
  }
  return off;
}

class SelectTool {
  // what the cursor points at (selectable), since then
  hovered = 0;
  private hover_since = 0;
  private hover_want: Vec2 | null = null;
  private hover_busy = false;
  private hover_cam = "";
  private hover_sent = 0;
  private hover_queued = false;
  // the hovered element's name and group (node), for the words next to the cursor
  private info: {
    id: number;
    name: string;
    group: { name: string; prefab: boolean; roots: number[] } | null;
  } | null = null;
  private info_busy = false;
  private mouse: Vec2 = [-1, -1];
  private over = false; // the cursor over the view, nothing above it (m_mouse_over_vp)
  // the topology tool: what the cursor points at, for the right button (no hover there)
  private under = 0;
  private shift = false;
  private ctrl = false;
  private last_press = { time: -1, at: [0, 0] as Vec2, count: 0 };
  // the left button pressed in the view, for the tool (m_lmb_in_vp; not when the gizmo took it)
  private lmb = false;
  private skip_release = false;
  private press_ready = false; // what was under the cursor at the press is known
  private press_time = 0;
  private press_target = 0;
  private press_on_selection = false;
  private press_point: Vec3 | null = null;
  private long_pressed = false;
  private ring = 0; // the button held still: the ring around the cursor, 0..1
  private stop_animating: (() => void) | null = null;
  // the box
  private box_start: Vec2 = [0, 0];
  private box_selecting = false;
  private box_preview: number[] = [];
  private box_preview_end: Vec2 = [-1, -1];
  private box_preview_time = 0;
  private box_busy = false;
  private box_outline = { key: null as unknown, ids: [] as number[] };
  private group_outline = { key: null as unknown, ids: [] as number[] };
  // the selection dragged, turned, or moved by the gizmo: where its roots were
  private moving = false;
  private move_px: Vec2 = [0, 0];
  private move_anchor: Vec3 = [0, 0, 0];
  private drag_start = new Map<number, Mat4>();
  private drag_pivot0: Vec3 = [0, 0, 0];
  private preview: Map<number, Mat4> | null = null;
  // a click again at the same place, the camera still (m_last_click)
  private last_click: Vec2 = [-1e9, -1e9];
  private last_click_cam = "";
  // the right button held on the selection
  private free_rotating = false;
  private free_right: Vec3 = [1, 0, 0];
  private free_yaw = 0;
  private free_pitch = 0;
  private free_pivot: Vec3 = [0, 0, 0];
  private was_active = true; // the first sync sets the gizmo either way
  // the button's events in order (each may wait for a pick)
  private chain: Promise<unknown> = Promise.resolve();

  private active(): boolean {
    return engine.state.tool === "select";
  }

  // the topology tool: the right button on the selection turns it too (start_free_rotation)
  private topology(): boolean {
    return engine.state.tool === "topology";
  }

  /** On the viewport attached (Viewport.svelte); returns what detaches it. */
  attach(): () => void {
    const canvas = viewport.canvas!,
      controls = viewport.controls!,
      gizmo = viewport.gizmo!;
    const offs: (() => void)[] = [];
    const on = <E extends Event>(
      target: EventTarget,
      type: string,
      fn: (e: E) => void,
      capture = false,
    ) => {
      target.addEventListener(type, fn as EventListener, capture);
      offs.push(() =>
        target.removeEventListener(type, fn as EventListener, capture),
      );
    };
    // the window's, in the capture phase: before the gizmo (a double click on what is under it)
    on(window, "pointerdown", (e: PointerEvent) => this.press(e), true);
    on(window, "pointermove", (e: PointerEvent) => this.move(e), true);
    on(window, "pointerup", (e: PointerEvent) => this.up(e), true);
    // the presses the gizmo did not take
    on(canvas, "pointerdown", (e: PointerEvent) => this.down(e));
    on(canvas, "pointerleave", () => {
      if (this.lmb) return;
      this.over = false;
      this.setHovered(0);
    });
    on(window, "mousemove", (e: MouseEvent) => this.freeMove(e));
    on(window, "mouseup", (e: MouseEvent) => {
      if (e.button === 2 && this.free_rotating) this.finishFreeRotation();
    });
    const key = (e: KeyboardEvent) => {
      this.mods(e);
      // the mouse still: the drag's or the turn's Ctrl and Shift at once (the C++ reads them every
      // frame)
      if (this.moving)
        this.run(() => this.drag(this.mouse, [0, 0], e.shiftKey, e.ctrlKey));
      if (this.free_rotating)
        this.freeMove(
          new MouseEvent("mousemove", {
            shiftKey: e.shiftKey,
            ctrlKey: e.ctrlKey,
          }),
        );
      if (this.lmb || this.hovered) viewport.requestRedraw();
    };
    on(window, "keydown", key, true);
    on(window, "keyup", key, true);
    // on the selection the right button turns it, elsewhere it looks around (or a tool's hook)
    const prev = controls.hooks.rightPress;
    controls.hooks.rightPress = (x, y, e) =>
      this.startFreeRotation() || !!prev?.(x, y, e);
    offs.push(() => (controls.hooks.rightPress = prev));
    // the gizmo's moves: where they began (the move preview, the distance next to the cursor)
    gizmo.callbacks.onStart = (ids) => this.dragBegin(ids);
    gizmo.callbacks.primary = () => engine.state.selection.primary;
    offs.push(viewport.addPass("opaque", () => this.updateOutlines()));
    offs.push(viewport.addOverlay((r, rs) => this.drawMovePreview(r, rs)));
    // the engine's events once it is there (the dev page's mock is installed then)
    let attached = true;
    offs.push(() => (attached = false));
    void engineReady.then(() => {
      if (!attached) return;
      offs.push(attachLabels());
      offs.push(viewport.addOverlay2d((ctx) => this.frame(ctx)));
      offs.push(
        onEngine("jle:changed", (p) => {
          if (!p || p.scene || p.document) this.info = null;
        }),
      );
    });
    this.sync();
    return () => offs.forEach((f) => f());
  }

  /**
   * The engine's state into the view: the gizmo's mode, space, snap (the frame sends W/E/R and the
   * toolbar's to the engine), the tool. Viewport.svelte runs it in an effect.
   */
  sync() {
    const s = engine.state;
    const g = viewport.gizmo?.gizmo;
    if (g) {
      g.op =
        s.gizmo.mode === "rotate"
          ? GizmoOp.Rotate
          : s.gizmo.mode === "scale"
            ? GizmoOp.Scale
            : GizmoOp.Translate;
      g.space = s.gizmo.local ? GizmoSpace.Local : GizmoSpace.World;
      g.snap = { ...s.gizmo.snap };
    }
    this.syncGizmo();
    if (!this.active()) {
      // another tool takes the mouse
      if (this.moving) store.setPreview(null);
      this.lmb = this.moving = this.box_selecting = false;
      this.stop_animating?.();
      this.stop_animating = null;
      this.setHovered(0);
    }
    // the hovered element's group, View > Labels, the document
    void [s.hovered_group, s.view.labels, s.document.open, s.selection.primary];
    viewport.requestRedraw();
  }

  // the gizmo with this tool, not while the mouse moves or turns the selection itself; off when
  // another tool takes over (the topology tool turns it on again for its own points)
  private syncGizmo() {
    const c = viewport.gizmo;
    const active = this.active();
    if (c && (active || this.was_active))
      c.enabled = active && !this.moving && !this.free_rotating;
    // the topology tool's, on its points: not while the element turns
    else if (c && this.topology()) c.enabled = !this.free_rotating;
    this.was_active = active;
  }

  private mods(e: { shiftKey: boolean; ctrlKey: boolean }) {
    this.shift = e.shiftKey;
    this.ctrl = e.ctrlKey;
  }

  private run(fn: () => unknown) {
    this.chain = this.chain.then(fn).catch((e) => console.warn("[select]", e));
  }

  // ---------------------------------------------------------------------------------------------
  // hover

  private setHovered(id: number) {
    if (id === this.hovered) return;
    this.hovered = viewport.hovered = id;
    this.hover_since = now();
    viewport.requestRedraw();
    // its name next to the cursor after a moment
    if (id) setTimeout(() => viewport.requestRedraw(), 360);
    // to the engine (get_state's hovered_group: its group outlined with it), once a frame at most
    if (this.hover_queued) return;
    this.hover_queued = true;
    requestAnimationFrame(() => {
      this.hover_queued = false;
      if (this.hovered === this.hover_sent) return;
      this.hover_sent = this.hovered;
      call("hover", { id: this.hovered }).catch(() => {});
    });
  }

  private canHover(): boolean {
    return (
      this.over &&
      this.active() &&
      !viewport.controls?.busy &&
      !this.free_rotating &&
      !viewport.gizmo?.gizmo.active() &&
      !this.box_selecting &&
      !this.moving
    );
  }

  // what the cursor points at, picked again (one pick at a time, the last position wanted)
  private hover(m: Vec2) {
    this.hover_want = m;
    if (this.hover_busy) return;
    this.hover_busy = true;
    void (async () => {
      try {
        while (this.hover_want) {
          const at = this.hover_want;
          this.hover_want = null;
          this.hover_cam = camKey(viewport.camera);
          const hit = await this.pick(at);
          this.under = hit?.selectable ?? 0;
          if (this.canHover()) this.setHovered(this.under);
        }
      } finally {
        this.hover_busy = false;
      }
    })();
  }

  private async pick(m: Vec2): Promise<Hit | null> {
    return ((await viewport.pick(m[0], m[1]).catch(() => null)) ??
      null) as Hit | null;
  }

  // ---------------------------------------------------------------------------------------------
  // the left button

  private press(e: PointerEvent) {
    if (
      e.target !== viewport.canvas ||
      e.button !== 0 ||
      e.altKey ||
      !this.active() ||
      viewport.controls?.busy ||
      this.free_rotating
    )
      return;
    const m = viewport.mouse(e),
      time = now(),
      p = this.last_press;
    p.count =
      time - p.time < kDoubleClickTime && dist(m, p.at) < kDoubleClickDist
        ? p.count + 1
        : 1;
    p.time = time;
    p.at = m;
    // a double click: the whole prefab or group (else the assembly) of the element under the
    // cursor, before the gizmo (after the first click, the element's gizmo is often right under it)
    if (p.count === 2 && this.hovered && !e.shiftKey && !e.ctrlKey) {
      e.preventDefault();
      e.stopImmediatePropagation();
      this.skip_release = true;
      const id = this.hovered;
      this.run(() => call("select_whole", { id }));
    }
  }

  // the button pressed (not on the gizmo): a click, a box, the selection dragged, or held still
  // (its whole group)
  private down(e: PointerEvent) {
    if (
      e.button !== 0 ||
      e.altKey ||
      !this.active() ||
      viewport.controls?.busy ||
      this.skip_release ||
      this.free_rotating
    )
      return;
    viewport.canvas!.setPointerCapture(e.pointerId);
    this.mods(e);
    const m = viewport.mouse(e),
      time = now();
    this.lmb = true;
    this.press_ready = false;
    this.stop_animating?.();
    // the ring and the box are drawn every frame while it is held
    this.stop_animating = viewport.animate();
    this.run(async () => {
      const hit = await this.pick(m);
      const h = hit?.selectable ?? 0;
      if (this.canHover()) this.setHovered(h);
      this.box_start = m;
      this.box_selecting = false;
      this.long_pressed = false;
      this.press_time = time;
      this.press_target = h;
      this.press_on_selection = h !== 0 && selected(h);
      this.press_point = hit?.point ?? null;
      this.box_preview = [];
      this.press_ready = true;
    });
  }

  private move(e: PointerEvent) {
    if (this.free_rotating) return;
    const m = viewport.mouse(e),
      [w, h] = viewport.size();
    const delta: Vec2 = [m[0] - this.mouse[0], m[1] - this.mouse[1]];
    this.mouse = m;
    this.over =
      m[0] >= 0 &&
      m[1] >= 0 &&
      m[0] < w &&
      m[1] < h &&
      document.elementFromPoint(e.clientX, e.clientY) === viewport.canvas;
    this.mods(e);
    if (this.topology()) {
      if (
        this.over &&
        !viewport.controls?.busy &&
        !viewport.gizmo?.gizmo.active()
      )
        this.hover(m);
      else this.under = 0;
    }
    if (!this.active()) return;
    if (viewport.controls?.busy) return this.setHovered(0);
    // released while another button is held: no pointerup
    if ((this.lmb || this.skip_release) && !(e.buttons & 1)) return this.up(e);
    if (this.lmb) {
      const { shiftKey, ctrlKey } = e;
      this.run(() => this.drag(m, delta, shiftKey, ctrlKey));
    }
    if (this.canHover()) this.hover(m);
    else if (!this.over) this.setHovered(0);
    // the words next to the cursor follow it
    if (this.hovered || this.lmb || viewport.gizmo?.gizmo.active())
      viewport.requestRedraw();
  }

  private drag(m: Vec2, delta: Vec2, shift: boolean, ctrl: boolean) {
    if (
      !this.lmb ||
      !this.press_ready ||
      this.long_pressed ||
      viewport.gizmo?.gizmo.active()
    )
      return;
    if (!this.box_selecting && !this.moving && dist(m, this.box_start) > 6) {
      if (this.press_on_selection) {
        // the selection dragged: left and right, up and down (Ctrl: forward and back)
        this.moving = true;
        this.move_px = [0, 0];
        this.dragBegin(store.selectionRoots());
        this.move_anchor = this.press_point ?? this.drag_pivot0;
        this.syncGizmo();
      } else {
        this.box_selecting = true;
      }
    }
    if (!this.moving) return;
    const k = shift ? 0.25 : 1;
    this.move_px = [
      this.move_px[0] + delta[0] * k,
      this.move_px[1] + delta[1] * k,
    ];
    const t = translate(
      dragOffset(
        viewport.camera,
        viewport.size()[1],
        this.move_px,
        this.move_anchor,
        ctrl,
        viewport.gizmo!.gizmo.snap,
      ),
    );
    this.preview = new Map(
      [...this.drag_start].map(([id, s]) => [id, matMul(t, s)]),
    );
    store.setPreview(this.preview);
  }

  private up(e: PointerEvent) {
    if (e.buttons & 1) return;
    if (this.skip_release) {
      this.skip_release = false;
      return;
    }
    if (!this.lmb) return;
    this.lmb = false;
    this.stop_animating?.();
    this.stop_animating = null;
    const m = viewport.mouse(e),
      [w, h] = viewport.size();
    const over =
      m[0] >= 0 &&
      m[1] >= 0 &&
      m[0] < w &&
      m[1] < h &&
      document.elementFromPoint(e.clientX, e.clientY) === viewport.canvas;
    const { shiftKey, ctrlKey } = e;
    this.run(() => this.release(m, over, shiftKey, ctrlKey));
  }

  private async release(m: Vec2, over: boolean, shift: boolean, ctrl: boolean) {
    try {
      if (this.moving) {
        this.moving = false;
        this.syncGizmo();
        await this.commit("undo.move");
      } else if (this.long_pressed) {
        // done while it was held
      } else if (this.box_selecting) {
        // Ctrl: taken out of the selection, Shift: added to it
        const mode = ctrl ? "remove" : shift ? "add" : "replace";
        await call("select_box", { ...this.boxArgs(m), mode });
      } else if (over) {
        await this.click(m, shift, ctrl);
      }
    } finally {
      this.box_selecting = false;
      this.box_preview = [];
      this.box_preview_end = [-1, -1];
      viewport.requestRedraw();
    }
  }

  private async click(m: Vec2, shift: boolean, ctrl: boolean) {
    const hit = await this.pick(m);
    let target = hit?.selectable ?? 0;
    if (this.canHover()) this.setHovered(target);
    const sel = store.selection;
    // a click again where the last one was, the camera still: the next element under the cursor
    // (what stands behind, inside or under the one selected)
    const cam = camKey(viewport.camera);
    const again = dist(m, this.last_click) < 4 && cam === this.last_click_cam;
    if (again && !shift && !ctrl && sel.length === 1) {
      const { origin, dir } = viewport.rayFromMouse(m[0], m[1]);
      const hits = ((await call("pick_all", { origin, dir }).catch(
        () => null,
      )) ?? []) as Hit[];
      const under: number[] = [];
      for (const h of hits) {
        // the ground hides what is under it
        if (h.locked && !engine.state.view.select_terrain) break;
        if (h.selectable && !under.includes(h.selectable))
          under.push(h.selectable);
      }
      const i = under.indexOf(sel[0]);
      if (i >= 0 && under.length > 1) target = under[(i + 1) % under.length];
    }
    this.last_click = m;
    this.last_click_cam = cam;
    if (target) {
      if (shift || ctrl) await call("toggle_select", { id: target });
      // a click on an element of a selection of several keeps them all (a drag would move them);
      // a click again there takes that element alone
      else if (selected(target) && sel.length > 1 && !again)
        await call("select", { ids: [target], mode: "add" }); // made the primary one
      else await call("select_only", { id: target });
    } else if (!shift && !ctrl) {
      await call("select_only", { id: 0 });
    }
  }

  // box_nodes / select_box: the view's matrix and size, the box from where the button went down
  private boxArgs(b: Vec2) {
    const [width, height] = viewport.size();
    return {
      view_proj: Array.from(viewport.renderer!.viewProj),
      width,
      height,
      a: this.box_start,
      b,
    };
  }

  // ---------------------------------------------------------------------------------------------
  // moves and turns

  private dragBegin(ids: number[]) {
    this.drag_start.clear();
    for (const id of ids) {
      const n = store.nodes.get(id);
      if (n) this.drag_start.set(id, new Float32Array(store.world(n)));
    }
    this.drag_pivot0 = viewport.selectionPivot(engine.state.selection.primary);
  }

  // the end of a drag: one transform_nodes (one undo step), the preview kept until the scene's
  // answer comes back
  private async commit(label: string) {
    const worlds = this.preview;
    this.preview = null;
    const moved =
      worlds &&
      [...worlds].some(([id, w]) =>
        w.some((v, i) => v !== this.drag_start.get(id)?.[i]),
      );
    if (worlds && moved) {
      await call("transform_nodes", {
        ids: [...worlds.keys()],
        worlds: [...worlds.values()].map((w) => Array.from(w)),
        label,
      });
      await store.refresh();
    }
    store.setPreview(null);
  }

  // the right button pressed on the selection (controls.hooks.rightPress): true when it turns it
  private startFreeRotation(): boolean {
    const under = this.topology() ? this.under : this.hovered;
    if (
      !(this.active() || this.topology()) ||
      viewport.gizmo?.gizmo.active() ||
      this.lmb ||
      !under ||
      !selected(under)
    )
      return false;
    const roots = store.selectionRoots();
    if (!roots.length) return false;
    this.dragBegin(roots);
    this.free_pivot = this.drag_pivot0;
    this.free_rotating = true;
    shortcuts.paused = true;
    this.free_right = viewport.camera.right();
    this.free_yaw = this.free_pitch = 0;
    this.preview = null;
    this.setHovered(0);
    this.syncGizmo();
    // the cursor stays where it is: the mouse turns it as far as wanted
    lockPointer(viewport.canvas!, () => this.free_rotating);
    return true;
  }

  // across: around the vertical, up and down: around the camera's horizontal; Shift finer, on the
  // snap's steps (Ctrl: the snap the other way)
  private freeMove(e: MouseEvent) {
    if (!this.free_rotating) return;
    const k = e.shiftKey ? 0.002 : 0.008; // radians per pixel
    this.free_yaw += e.movementX * k;
    this.free_pitch += e.movementY * k;
    let yaw = this.free_yaw,
      pitch = this.free_pitch;
    const snap = viewport.gizmo!.gizmo.snap;
    if (snap.enabled !== e.ctrlKey && snap.rotate_deg > 0) {
      const s = radians(snap.rotate_deg);
      yaw = Math.round(yaw / s) * s;
      pitch = Math.round(pitch / s) * s;
    }
    const r = quatMul(
      axisAngle([0, 1, 0], yaw),
      axisAngle(this.free_right, pitch),
    );
    const p = this.free_pivot;
    const turn = matMul(matMul(translate(p), rotate(r)), translate(mul(p, -1)));
    this.preview = new Map(
      [...this.drag_start].map(([id, s]) => [id, matMul(turn, s)]),
    );
    store.setPreview(this.preview);
  }

  private finishFreeRotation() {
    this.free_rotating = false;
    shortcuts.paused = false;
    if (document.pointerLockElement === viewport.canvas)
      document.exitPointerLock();
    this.syncGizmo();
    this.run(() => this.commit("undo.rotate"));
  }

  // how far the selection went (dragged, or by the gizmo's arrows), whatever the way
  private moveDelta(): Vec3 | null {
    const g = viewport.gizmo?.gizmo;
    const gizmo_move = !!g?.active() && g.op === GizmoOp.Translate;
    if (!this.drag_start.size || !(this.moving || gizmo_move)) return null;
    const [id, start] = this.drag_start.entries().next().value!;
    const n = store.nodes.get(id);
    return n ? sub(col3(store.world(n), 3), col3(start, 3)) : null;
  }

  // ---------------------------------------------------------------------------------------------
  // drawing

  // update_outlines (before the scene is drawn): the box's elements, else the hovered element's
  // group; the selection's and the hovered element's are the viewport's own
  private updateOutlines() {
    if (!this.active()) {
      viewport.outlineHover = null;
      viewport.outlineGroup = [];
      return;
    }
    if (this.box_selecting) {
      if (this.box_outline.key !== this.box_preview)
        this.box_outline = {
          key: this.box_preview,
          ids: renderNodes(this.box_preview, 4000),
        };
      viewport.outlineHover = this.box_outline.ids;
      viewport.outlineGroup = [];
      return;
    }
    viewport.outlineHover = null;
    const g = engine.state.hovered_group as { roots: number[] } | null;
    if (
      !this.hovered ||
      selected(this.hovered) ||
      !g ||
      engine.state.hovered !== this.hovered
    ) {
      viewport.outlineGroup = [];
      return;
    }
    if (this.group_outline.key !== g.roots)
      this.group_outline = { key: g.roots, ids: renderNodes(g.roots, 4000) };
    viewport.outlineGroup = this.group_outline.ids;
  }

  // draw_move_preview: where it was, see-through, and the way from there: along x (red), z (blue),
  // y (green)
  private drawMovePreview(r: Renderer, rs: RenderSettings) {
    if (!this.active()) return;
    const d = this.moveDelta();
    if (!d || length(d) < 1e-3) return;
    const back = translate(mul(d, -1));
    let drawn = 0;
    for (const id of this.drag_start.keys())
      for (const k of store.subtree(id)) {
        const n = store.nodes.get(k)!;
        // ponytail: 3000 pieces at most (a whole level dragged: its first ones)
        if (n.kind !== NodeKind.Render || !n.mesh || ++drawn > 3000) continue;
        const mesh = store.meshes.get(n.mesh);
        if (mesh)
          r.drawMesh(
            mesh,
            matMul(back, store.world(n)),
            store.materials,
            store,
            rs,
            [0.35, 0.75, 1, 0.3],
            0.35,
          );
      }
    const a = this.drag_pivot0,
      b: Vec3 = [a[0] + d[0], a[1], a[2]],
      c: Vec3 = [a[0] + d[0], a[1], a[2] + d[2]],
      e = add(a, d);
    r.line(a, b, col32(235, 80, 70, 255));
    r.line(b, c, col32(80, 140, 255, 255));
    r.line(c, e, col32(110, 220, 90, 255));
    r.line(a, e, col32(255, 255, 255, 220));
    r.flush(false);
  }

  // each frame drawn (after the 3D): the button held still, the box's elements found again, what
  // the cursor points at when the camera moved under it; the box and the words next to the cursor
  private frame(ctx: CanvasRenderingContext2D) {
    if (!this.active()) return;
    if (viewport.controls?.busy) this.setHovered(0);
    const time = now();
    this.ring = 0;
    if (
      this.lmb &&
      this.press_ready &&
      !this.long_pressed &&
      !viewport.gizmo?.gizmo.active()
    ) {
      if (this.box_selecting) {
        // what it would select, outlined as it grows (found again when the box changed, 20 times
        // a second)
        if (
          dist(this.mouse, this.box_preview_end) > 1 &&
          time - this.box_preview_time > 0.05 &&
          !this.box_busy
        ) {
          this.box_busy = true;
          this.box_preview_end = [...this.mouse];
          this.box_preview_time = time;
          call("box_nodes", this.boxArgs(this.box_preview_end))
            .then((r) => {
              if (this.box_selecting) this.box_preview = r?.ids ?? [];
            })
            .catch(() => {})
            .finally(() => {
              this.box_busy = false;
              viewport.requestRedraw();
            });
        }
      } else if (
        !this.moving &&
        this.press_target &&
        dist(this.mouse, this.box_start) <= 6
      ) {
        // held still on an element: after a moment, its whole group (or assembly)
        const held = time - this.press_time;
        if (held > 0.15) this.ring = Math.min(1, (held - 0.15) / 0.45);
        if (held > 0.6) {
          this.long_pressed = true;
          const id = this.press_target;
          this.run(() => call("select_whole", { id }));
        }
      }
    }
    if (this.canHover() && camKey(viewport.camera) !== this.hover_cam)
      this.hover(this.mouse);

    if (this.box_selecting) {
      const [x0, y0] = this.box_start,
        [x1, y1] = this.mouse;
      const x = Math.min(x0, x1),
        y = Math.min(y0, y1),
        w = Math.abs(x1 - x0),
        h = Math.abs(y1 - y0);
      ctx.fillStyle = rgba(80, 190, 210, 40);
      ctx.fillRect(x, y, w, h);
      ctx.strokeStyle = rgba(80, 190, 210, 200);
      ctx.lineWidth = 1;
      ctx.strokeRect(x + 0.5, y + 0.5, w, h);
    }
    if (this.over) this.cursorInfo(ctx);
  }

  // draw_cursor_info: what the cursor points at, next to it: the element, its group, what a click
  // does
  private cursorInfo(ctx: CanvasRenderingContext2D) {
    const [x, y] = this.mouse;
    let text = "";
    let d: Vec3 | null;
    if (this.box_selecting) {
      text = t(
        this.ctrl
          ? "cursor.box_remove"
          : this.shift
            ? "cursor.box_add"
            : "cursor.box",
        this.box_preview.length,
      );
    } else if (this.ring > 0) {
      // the button held still: a ring filling up, then the whole group is selected
      ctx.lineWidth = 3;
      ctx.strokeStyle = rgba(255, 255, 255, 60);
      ctx.beginPath();
      ctx.arc(x, y, 16, 0, 2 * Math.PI);
      ctx.stroke();
      ctx.strokeStyle = rgba(255, 170, 60, 230);
      ctx.beginPath();
      ctx.arc(x, y, 16, -Math.PI / 2, -Math.PI / 2 + 2 * Math.PI * this.ring);
      ctx.stroke();
      text = t("cursor.hold");
    } else if ((d = this.moveDelta())) {
      // how far it went, whatever the way
      text = t("cursor.move_delta", d[0], d[1], d[2], length(d));
      if (this.moving)
        text += "\n" + t(this.ctrl ? "cursor.moving_depth" : "cursor.moving");
    } else if (this.hovered && now() - this.hover_since > 0.35) {
      const info = this.hoverInfo();
      if (!info) return;
      text = info.name;
      const g = info.group;
      if (g) {
        text +=
          "\n" +
          t(
            g.prefab ? "cursor.prefab" : "cursor.group",
            g.name,
            g.roots.length,
          );
        if (!g.roots.every(selected)) text += "\n" + t("cursor.select_group");
      }
    }
    if (!text) return;
    textBox(
      ctx,
      text,
      [x + 18, y + 14],
      [5, 3],
      4,
      rgba(20, 22, 26, 210),
      rgba(235, 235, 225),
    );
  }

  // the hovered element's name and group (node), asked once per element
  private hoverInfo() {
    if (this.info?.id === this.hovered) return this.info;
    if (!this.info_busy) {
      this.info_busy = true;
      const id = this.hovered;
      call("node", { id })
        .then((n) => {
          if (n) this.info = { id, name: n.name ?? "", group: n.group ?? null };
        })
        .catch(() => {})
        .finally(() => {
          this.info_busy = false;
          viewport.requestRedraw();
        });
    }
    return null;
  }
}

export const selectTool = new SelectTool();
