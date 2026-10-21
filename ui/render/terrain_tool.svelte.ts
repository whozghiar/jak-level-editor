// The terrain brush in the view (terrain_tool.cpp terrain_tool, terrain_right_button,
// draw_terrain_overlay): the square over the ground under the cursor, what it would do drawn
// see-through before it is done; the left button held paints (or walls, erases, smooths,
// flattens), shown until it is let go; the right button held and the mouse moved up raises the
// ground there, down digs it (its height by the cursor, done when it is let go; moved sideways
// first, or with W A S D: it looks around); Ctrl + wheel sizes the square. The engine decides
// everything (editor/terrain_tool.rs): each call answers what to draw. One call at a time, the
// mouse's moves merged while one is out.
// Viewport.svelte attaches it (`attachTerrainTool`).

import { engine } from "../lib/engine.svelte";
import { shortcuts } from "../lib/shortcuts";
import { callQueue } from "./call_queue";
import { textBox, rgba } from "./labels";
import { add, col32, point, sub, type Mat4, type Vec3 } from "./math";
import type { Renderer } from "./renderer";
import { viewport } from "./viewport_api";

/** What the engine answers each brush call (brush_overlay) */
interface Overlay {
  valid: boolean;
  stroke?: boolean;
  point?: Vec3;
  world?: number[];
  center?: Vec3;
  size?: number;
  mode?: string;
  steps?: number;
  heights?: number[] | null; // (steps + 1)^2, row by row along x; null: flat at center.y
  cross?: number[];
  magnet?: { raw: Vec3; edge: Vec3; name: string; text: string } | null;
  ghost?: { cell: number; mode: string; cells: number[] } | null;
  sculpt?: number | null;
}

const kFly = ["KeyW", "KeyA", "KeyS", "KeyD", "KeyQ", "KeyE", "KeyC", "Space"];

export function attachTerrainTool(): () => void {
  const canvas = viewport.canvas!,
    controls = viewport.controls!;
  const off: (() => void)[] = [];
  const on = <E extends Event>(
    t: EventTarget,
    type: string,
    fn: (e: E) => void,
    capture = false,
  ) => {
    t.addEventListener(type, fn as EventListener, capture);
    off.push(() => t.removeEventListener(type, fn as EventListener, capture));
  };
  const active = () => engine.state.tool === "terrain";

  let overlay: Overlay | null = null;
  let mouse: [number, number] | null = null; // in the view (CSS pixels), null: out of it
  let shift = false;
  let held = false; // the left button
  let stopAnimating: (() => void) | null = null;
  let lastDrag = 0;
  // the right button held, the mouse not moved far yet (pixels since); then raising or digging
  let rmb: { dx: number; dy: number } | null = null;
  let sculptPx: number | null = null;
  const keys = new Set<string>();

  // ---- the calls, one at a time; the moves of the mouse merged while one is out
  const { send } = callQueue(
    (_, a) => {
      if (a && typeof a === "object" && "valid" in a) overlay = a as Overlay;
      viewport.requestRedraw();
    },
    ["brush_hover", "brush_drag", "brush_sculpt"],
  );
  const ray = () => {
    if (!mouse) return {};
    const r = viewport.rayFromMouse(mouse[0], mouse[1]);
    return { origin: r.origin, dir: r.dir };
  };
  const hover = () => {
    if (!active() || held || rmb || sculptPx !== null || controls.busy) return;
    send("brush_hover", { ...ray(), shift });
  };
  const sculpt = (release: boolean) =>
    send("brush_sculpt", {
      px: sculptPx ?? 0,
      fov: viewport.camera.fov_deg,
      view_height: viewport.size()[1],
      release,
    });

  // ---- the mouse
  on(canvas, "pointermove", (e: PointerEvent) => {
    mouse = viewport.mouse(e);
    if (!held) hover();
  });
  on(canvas, "pointerleave", () => {
    if (held || !active()) return;
    mouse = null;
    send("brush_hover", { shift });
  });
  on(canvas, "pointerdown", (e: PointerEvent) => {
    if (
      !active() ||
      e.button !== 0 ||
      e.altKey ||
      controls.busy ||
      rmb ||
      sculptPx !== null
    )
      return;
    mouse = viewport.mouse(e);
    held = true;
    canvas.setPointerCapture(e.pointerId);
    lastDrag = performance.now();
    send("brush_press", { ...ray(), shift, dt: 1 / 60 });
    stopAnimating = viewport.animate();
  });
  on(window, "pointerup", (e: PointerEvent) => {
    if (e.button !== 0 || !held) return;
    held = false;
    stopAnimating?.();
    stopAnimating = null;
    send("brush_release");
    hover();
  });
  // the right button: raise or dig, else look around (terrain_right_button)
  const prevRight = controls.hooks.rightPress;
  controls.hooks.rightPress = (x, y, e) => {
    if (!active()) return prevRight?.(x, y, e) ?? false;
    // (not during a stroke of the left button: it looks around)
    if (held || !overlay?.valid) return prevRight?.(x, y, e) ?? false;
    rmb = { dx: 0, dy: 0 };
    viewport.requestRedraw();
    return true;
  };
  on(window, "mousemove", (e: MouseEvent) => {
    if (sculptPx !== null) {
      sculptPx -= e.movementY;
      sculpt(false);
      return;
    }
    if (!rmb) return;
    // not moved far yet: up or down raises or digs, sideways (or a key to fly) looks around
    rmb.dx += e.movementX;
    rmb.dy += e.movementY;
    const flying = kFly.some((k) => keys.has(k));
    const ax = Math.abs(rmb.dx),
      ay = Math.abs(rmb.dy);
    if (!flying && ax + ay <= 6) return;
    const lock = () =>
      Promise.resolve((canvas as any).requestPointerLock?.()).catch(() => {});
    if (!flying && ay > 2 * ax) {
      sculptPx = -rmb.dy;
      rmb = null;
      shortcuts.paused = true;
      void lock();
      sculpt(false);
      return;
    }
    rmb = null;
    controls.looking = true;
    void lock();
    viewport.requestRedraw();
  });
  on(window, "mouseup", (e: MouseEvent) => {
    if (e.button !== 2) return;
    rmb = null;
    if (sculptPx === null) return;
    // released: the ground raised or dug as shown
    sculpt(true);
    sculptPx = null;
    shortcuts.paused = false;
    if (document.pointerLockElement === canvas) document.exitPointerLock();
    hover();
  });
  // Ctrl + wheel: the square's size
  const prevWheel = controls.hooks.wheel;
  controls.hooks.wheel = (wheel, e) => {
    if (!active() || !e.ctrlKey) return prevWheel?.(wheel, e) ?? false;
    const size = Math.min(
      64,
      Math.max(0.5, (engine.state.brush.size ?? 4) * Math.pow(1.15, wheel)),
    );
    send("set_brush", { size });
    hover();
    return true;
  };
  const key = (e: KeyboardEvent, down: boolean) => {
    if (down) keys.add(e.code);
    else keys.delete(e.code);
    if (e.shiftKey !== shift) {
      shift = e.shiftKey; // Shift: the other way
      hover();
    }
  };
  on(window, "keydown", (e: KeyboardEvent) => key(e, true), true);
  on(window, "keyup", (e: KeyboardEvent) => key(e, false), true);
  on(window, "blur", () => keys.clear());

  // ---- the frame: the brush held applied along the way, the brush again when the camera moved
  let camKey = "";
  off.push(
    viewport.addOverlay((r) => {
      if (!active()) return;
      const c = viewport.camera;
      const k = `${c.pos}|${c.yaw}|${c.pitch}`;
      if (held) {
        const now = performance.now();
        send("brush_drag", { ...ray(), shift, dt: (now - lastDrag) / 1000 });
        lastDrag = now;
      } else if (k !== camKey && mouse) {
        queueMicrotask(hover);
      }
      camKey = k;
      draw(r);
    }),
  );
  off.push(viewport.addOverlay2d((ctx) => active() && texts(ctx)));

  // ---- another tool: the brush's state dropped
  off.push(
    $effect.root(() => {
      $effect(() => {
        const on = engine.state.tool === "terrain";
        void engine.state.brush;
        if (!on) {
          overlay = null;
          held = false;
          rmb = null;
          stopAnimating?.();
          stopAnimating = null;
          if (sculptPx !== null && document.pointerLockElement === canvas)
            document.exitPointerLock();
          sculptPx = null;
        } else {
          hover(); // its settings changed: the square and its preview again
        }
        viewport.requestRedraw();
      });
    }),
  );

  // ---- drawing (draw_terrain_overlay, draw_terrain_ghost)
  function draw(r: Renderer) {
    const o = overlay;
    if (!o?.valid || !o.world || !o.center) return;
    const w = new Float32Array(o.world) as Mat4;
    const c = o.center,
      size = o.size!,
      half = size * 0.5,
      steps = o.steps!;
    const right = rmb !== null || sculptPx !== null;
    // the right button held: green, the mouse up or down raises or digs there
    const [cr, cg, cb] = right
      ? [120, 240, 120]
      : o.mode === "erase"
        ? [255, 115, 75]
        : o.mode === "wall"
          ? [240, 205, 90]
          : [90, 230, 255];
    const color = col32(cr, cg, cb);
    // the square, on the ground under it (the terrain's where it has some)
    const at = (i: number, k: number): Vec3 => {
      const y = o.heights ? o.heights[k * (steps + 1) + i] : c[1];
      return point(w, [
        c[0] - half + (size * i) / steps,
        y + 0.08,
        c[2] - half + (size * k) / steps,
      ]);
    };
    for (let s = 0; s < steps; s++) {
      r.line(at(s, 0), at(s + 1, 0), color);
      r.line(at(steps, s), at(steps, s + 1), color);
      r.line(at(s + 1, steps), at(s, steps), color);
      r.line(at(0, s + 1), at(0, s), color);
    }
    // filled, lightly, over the ground it covers
    const fill = col32(cr, cg, cb, 50);
    const stride = steps > 32 ? 2 : 1;
    for (let i = 0; i < steps; i += stride)
      for (let k = 0; k < steps; k += stride) {
        const i1 = Math.min(i + stride, steps),
          k1 = Math.min(k + stride, steps);
        r.tri(at(i, k), at(i1, k), at(i1, k1), fill);
        r.tri(at(i, k), at(i1, k1), at(i, k1), fill);
      }
    // its middle
    const x = o.cross ?? [c[1], c[1], c[1], c[1]];
    const p = (dx: number, y: number, dz: number) =>
      point(w, [c[0] + dx, y + 0.08, c[2] + dz]);
    r.line(p(-0.3, x[0], 0), p(0.3, x[1], 0), color);
    r.line(p(0, x[2], -0.3), p(0, x[3], 0.3), color);
    r.flush(true, true);
    // the magnet: from the cursor to the edge it sticks to
    if (o.magnet) {
      const white = col32(255, 255, 255, 230);
      const e = o.magnet.edge;
      r.line(o.magnet.raw, e, white);
      r.line(sub(e, [0.4, 0, 0]), add(e, [0.4, 0, 0]), white);
      r.line(sub(e, [0, 0, 0.4]), add(e, [0, 0, 0.4]), white);
      r.line(e, add(e, [0, 0.8, 0]), white);
      r.flush(false);
    }
    // what it would do there, before it is done (not while the right button decides)
    if (o.ghost && rmb === null) ghost(r, w, o.ghost);
  }

  // the cells changed, seen where they are in front, and faintly through the ground (a hollow dug
  // under it)
  function ghost(r: Renderer, w: Mat4, g: NonNullable<Overlay["ghost"]>) {
    const cl = g.cell,
      d = g.cells;
    const [mr, mg, mb] =
      g.mode === "wall"
        ? [240, 205, 90]
        : g.mode === "dig"
          ? [255, 170, 60]
          : [90, 230, 255];
    for (const alpha of [110, 45]) {
      for (let n = 0; n + 7 < d.length; n += 8) {
        const [i, k, flags] = [d[n], d[n + 1], d[n + 2]];
        const h = [d[n + 3], d[n + 4], d[n + 5], d[n + 6]];
        const col =
          flags & 1 ? col32(255, 90, 60, alpha) : col32(mr, mg, mb, alpha);
        const x = i * cl,
          z = k * cl;
        const xz: [number, number][] = [
          [x, z],
          [x + cl, z],
          [x + cl, z + cl],
          [x, z + cl],
        ];
        if (!(flags & 2)) {
          const q = xz.map(([a, b], v) => point(w, [a, h[v] + 0.05, b]));
          r.tri(q[0], q[1], q[2], col);
          r.tri(q[0], q[2], q[3], col);
          continue;
        }
        // a wall: its column, from the ground to its top
        const top = Math.max(...h) + d[n + 7],
          base = Math.min(...h);
        const lo = xz.map(([a, b]) => point(w, [a, base, b]));
        const hi = xz.map(([a, b]) => point(w, [a, top, b]));
        r.tri(hi[0], hi[1], hi[2], col);
        r.tri(hi[0], hi[2], hi[3], col);
        for (let e = 0; e < 4; e++) {
          r.tri(lo[e], lo[(e + 1) % 4], hi[(e + 1) % 4], col);
          r.tri(lo[e], hi[(e + 1) % 4], hi[e], col);
        }
      }
      r.flush(alpha > 100, true);
    }
  }

  // the magnet's piece named next to it; the right button's height next to the square
  function texts(ctx: CanvasRenderingContext2D) {
    const o = overlay;
    if (!o?.valid) return;
    const bg = rgba(20, 22, 26, 210),
      fg = rgba(235, 235, 225);
    if (o.magnet) {
      const s = viewport.project(add(o.magnet.edge, [0, 0.05, 0]));
      if (s)
        textBox(ctx, o.magnet.text, [s[0] + 12, s[1] + 6], [5, 3], 4, bg, fg);
    }
    if (sculptPx !== null && o.sculpt != null && o.point) {
      const s = viewport.project(o.point);
      const h = o.sculpt;
      if (s)
        textBox(
          ctx,
          `${h >= 0 ? "+" : ""}${h.toFixed(2)} m`,
          [s[0] + 16, s[1] - 8],
          [5, 3],
          4,
          bg,
          fg,
        );
    }
  }

  return () => {
    for (const f of off) f();
    controls.hooks.rightPress = prevRight;
    controls.hooks.wheel = prevWheel;
    stopAnimating?.();
  };
}
