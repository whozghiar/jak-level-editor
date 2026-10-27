// The collision tool in the view (editor/collision_tool.rs), as OpenRAC's collision dock: the
// levels' collision drawn over them, a color per surface, while the tool is on (or View > Collision);
// a click picks the face under the cursor (Shift: in or out), a double click the faces of its
// surface touching it, a drag a box (every face whose middle is in it, seen or not). The faces
// picked are drawn orange, where the window's Move would put them while it is being set. The
// engine keeps what is picked and changes the collision; the view draws what `collision_overlay`
// answers (the triangles again only when the collision changed).
// Viewport.svelte attaches it (`attachCollisionTool`).

import { engine } from "../lib/engine.svelte";
import { callQueue } from "./call_queue";
import { call } from "./engine";
import { rgba } from "./labels";
import { add, col32, type Vec2, type Vec3 } from "./math";
import type { Renderer } from "./renderer";
import { viewport } from "./viewport_api";

/** The window's settings the view shows: the move being set (drawn before it is done) */
export const collisionUi = $state({ move: [0, 0, 0] as Vec3 });

function bytes(b64: string): ArrayBuffer {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out.buffer;
}

/** The edges of the triangles of `data` (x, y, z, rgba each): two vertices each, darker */
function edges(data: ArrayBuffer): ArrayBuffer {
  const src = new Uint8Array(data);
  const n = Math.floor(src.length / 48); // triangles
  const out = new Uint8Array(n * 96);
  for (let t = 0; t < n; t++) {
    for (let k = 0; k < 3; k++) {
      for (const [j, v] of [k, (k + 1) % 3].entries()) {
        const at = t * 96 + (k * 2 + j) * 16;
        out.set(src.subarray(t * 48 + v * 16, t * 48 + v * 16 + 16), at);
        for (let c = 12; c < 15; c++) out[at + c] = out[at + c] >> 1;
        out[at + 15] = 220;
      }
    }
  }
  return out.buffer;
}

/** `data` (x, y, z, rgba each) moved by `d` */
function moved(data: ArrayBuffer, d: Vec3): ArrayBuffer {
  if (!d[0] && !d[1] && !d[2]) return data;
  const out = data.slice(0);
  const f = new Float32Array(out);
  for (let i = 0; i + 3 < f.length; i += 4) {
    f[i] += d[0];
    f[i + 1] += d[1];
    f[i + 2] += d[2];
  }
  return out;
}

export function attachCollisionTool(): () => void {
  const canvas = viewport.canvas!,
    controls = viewport.controls!;
  const off: (() => void)[] = [];
  const on = <E extends Event>(
    t: EventTarget,
    type: string,
    fn: (e: E) => void,
  ) => {
    t.addEventListener(type, fn as EventListener);
    off.push(() => t.removeEventListener(type, fn as EventListener));
  };
  const active = () => engine.state.tool === "collision";
  const shown = () => active() || !!engine.state.view.collision;

  // the triangles drawn (their key: what they were made of), those picked, those moved
  let key = -1;
  let faces: ArrayBuffer | null = null;
  let lines: ArrayBuffer | null = null;
  let picked: ArrayBuffer | null = null;
  let pickedMoved: { data: ArrayBuffer; by: string } | null = null;
  let asked = "";
  let box: { a: Vec2; b: Vec2; selecting: boolean; add: boolean } | null = null;
  let lastDown = { time: 0, at: [0, 0] as Vec2 };

  const queue = callQueue(() => viewport.requestRedraw(), []);

  // the overlay again when the collision or the faces picked changed
  async function refresh() {
    const c = engine.state.collision;
    if (!shown() || !c) {
      faces = lines = picked = pickedMoved = null;
      key = -1;
      asked = "";
      return;
    }
    const want = `${c.key}:${c.picked_key}`;
    if (want === asked) return;
    asked = want;
    const r = await call("collision_overlay", { key }).catch(() => null);
    if (!r) return;
    if (r.triangles !== undefined) {
      faces = bytes(r.triangles);
      lines = edges(faces);
      key = r.key;
    }
    picked = bytes(r.picked);
    pickedMoved = null;
    viewport.requestRedraw();
  }
  off.push(
    $effect.root(() => {
      $effect(() => {
        void engine.state.collision?.key;
        void engine.state.collision?.picked_key;
        void engine.state.tool;
        void engine.state.view.collision;
        void refresh();
      });
      $effect(() => {
        void collisionUi.move[0];
        void collisionUi.move[1];
        void collisionUi.move[2];
        viewport.requestRedraw();
      });
    }),
  );

  // ---- picking: a click, Shift adds (or takes out), a double click its surface, a box
  on(canvas, "pointerdown", (e: PointerEvent) => {
    if (!active() || e.button !== 0 || e.altKey || controls.busy) return;
    const m = viewport.mouse(e);
    box = { a: m, b: m, selecting: false, add: e.shiftKey || e.ctrlKey };
    canvas.setPointerCapture(e.pointerId);
  });
  on(canvas, "pointermove", (e: PointerEvent) => {
    if (!box) return;
    box.b = viewport.mouse(e);
    if (
      !box.selecting &&
      Math.hypot(box.b[0] - box.a[0], box.b[1] - box.a[1]) > 6
    )
      box.selecting = true;
    viewport.requestRedraw();
  });
  on(window, "pointerup", (e: PointerEvent) => {
    if (e.button !== 0 || !box) return;
    const b = box;
    box = null;
    viewport.requestRedraw();
    if (!active()) return;
    if (b.selecting) {
      const [w, h] = viewport.size();
      queue.send("collision_box", {
        view_proj: Array.from(viewport.renderer!.viewProj),
        width: w,
        height: h,
        a: b.a,
        b: b.b,
        add: b.add,
      });
      return;
    }
    const now = performance.now();
    const double =
      now - lastDown.time < 400 &&
      Math.hypot(b.b[0] - lastDown.at[0], b.b[1] - lastDown.at[1]) < 6;
    lastDown = { time: double ? 0 : now, at: b.b };
    const r = viewport.rayFromMouse(b.b[0], b.b[1]);
    queue.send("collision_pick", {
      origin: r.origin,
      dir: r.dir,
      add: b.add,
      connected: double,
    });
  });

  // ---- drawing: the faces, those picked (where the move puts them), the last click, the places
  // Jak may fall through
  off.push(
    viewport.addOverlay(
      (r: Renderer) => {
        if (!shown()) return;
        if (faces) r.drawTris(0, faces);
        if (lines) r.drawTris(2, lines, true, true);
        const c = engine.state.collision;
        if (!active() || !c) return;
        if (picked) {
          const d = collisionUi.move;
          const by = d.join(",");
          if (pickedMoved?.by !== by)
            pickedMoved = { data: moved(picked, d), by };
          r.drawTris(1, pickedMoved.data);
        }
        const mark = col32(255, 220, 60, 255);
        if (c.last_point) {
          const p = c.last_point as Vec3;
          r.line(add(p, [-0.4, 0, 0]), add(p, [0.4, 0, 0]), mark);
          r.line(add(p, [0, 0, -0.4]), add(p, [0, 0, 0.4]), mark);
          r.line(p, add(p, [0, 0.8, 0]), mark);
        }
        const hot = col32(255, 40, 40, 255);
        for (const h of c.hot ?? []) {
          const p = h.at as Vec3;
          r.circle(add(p, [0, 0.1, 0]), [1, 0, 0], [0, 0, 1], 1.5, hot, 24);
          r.line(p, add(p, [0, 3, 0]), hot);
        }
        r.flush(false);
      },
      { capture: true },
    ),
  );
  // the box being drawn
  off.push(
    viewport.addOverlay2d((ctx) => {
      if (!active() || !box?.selecting) return;
      const x = Math.min(box.a[0], box.b[0]),
        y = Math.min(box.a[1], box.b[1]);
      const bw = Math.abs(box.b[0] - box.a[0]),
        bh = Math.abs(box.b[1] - box.a[1]);
      ctx.fillStyle = rgba(255, 160, 40, 30);
      ctx.fillRect(x, y, bw, bh);
      ctx.strokeStyle = rgba(255, 160, 40, 200);
      ctx.lineWidth = 1;
      ctx.strokeRect(x + 0.5, y + 0.5, bw, bh);
    }),
  );

  return () => off.forEach((f) => f());
}
