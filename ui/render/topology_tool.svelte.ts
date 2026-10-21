// The topology tool in the view (topology_tool.cpp topology_tool, draw_topology_overlay): the
// element's edges, its points (or its faces picked) drawn over it; a click picks the point nearest
// the cursor or the face under it (Shift or Ctrl: in or out), a drag a box; the gizmo on what is
// picked moves, turns or scales it (the mesh changes as it goes, one undo step when let go). The
// engine keeps what is picked and edits the mesh (editor/topology_tool.rs); the view draws what
// `topology_view` answers.
// Viewport.svelte attaches it (`attachTopologyTool`).

import { engine } from "../lib/engine.svelte";
import { callQueue } from "./call_queue";
import { engineReady, onEngine } from "./engine";
import { GizmoOp, GizmoSpace, type GizmoCallbacks } from "./gizmo";
import { rgba } from "./labels";
import {
  col3,
  dot,
  inverse,
  matMul,
  point,
  setCol3,
  translate,
  type Mat4,
  type Vec2,
  type Vec3,
} from "./math";
import type { Renderer } from "./renderer";
import { store } from "./scene_store";
import { viewport } from "./viewport_api";

/** topology_view's answer */
interface TopoView {
  node: number;
  mesh: number;
  version: number;
  faces: boolean;
  selected_points: number[];
  selected_faces: number[]; // primitive, triangle pairs
  center: Vec3 | null;
  count: number;
  points: number[] | null; // the element's space, x y z each
}

const kEdges = 0x90f0e0c0; // the C++ IM_COL32s
const kFace = 0x8028a0ff;
const kManyTriangles = 60000;

export function attachTopologyTool(): () => void {
  const canvas = viewport.canvas!,
    controls = viewport.controls!,
    gizmo = viewport.gizmo!;
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
  const active = () => engine.state.tool === "topology";

  let view: TopoView | null = null;
  let points: Float32Array | null = null;
  let pointsKey = "";
  let box: { a: Vec2; b: Vec2; selecting: boolean; add: boolean } | null = null;
  // a gizmo drag: the element's world and the gizmo's frame when it began, whether it moved yet
  let drag: { start: Mat4; frame: Mat4; sent: boolean } | null = null;

  const queue = callQueue(
    (_, v) => {
      if (v === undefined) return;
      view = v as TopoView | null;
      const key = view ? `${view.mesh}:${view.version}` : "";
      if (view?.points) {
        points = new Float32Array(view.points);
        pointsKey = key;
      } else if (key !== pointsKey) {
        points = null;
        pointsKey = "";
        // the points of the mesh as it is now
        if (view && view.count <= 40000)
          queue.send("topology_view", { points: true });
      }
      viewport.requestRedraw();
    },
    ["topology_view", "topology_drag"],
  );
  const refresh = () => active() && queue.send("topology_view", {});
  const world = (): Mat4 | null => {
    const n = view && store.nodes.get(view.node);
    return n ? store.world(n) : null;
  };
  const viewArgs = () => {
    const [w, h] = viewport.size();
    return {
      view_proj: Array.from(viewport.renderer!.viewProj),
      width: w,
      height: h,
    };
  };

  // ---- the gizmo on what is picked (its middle, turned as the element when local)
  const frame = (): Mat4 | null => {
    const w = world();
    if (!w || !view?.center) return null;
    if (gizmo.gizmo.space === GizmoSpace.World) return translate(view.center);
    const f = new Float32Array(w) as Mat4;
    setCol3(f, 3, view.center);
    return f;
  };
  const callbacks: GizmoCallbacks = {
    targets: () => {
      const f = frame();
      return f && view ? { ids: [view.node], frame: f } : null;
    },
    onStart: () => {
      const w = world(),
        f = frame();
      drag =
        w && f
          ? { start: new Float32Array(w) as Mat4, frame: f, sent: false }
          : null;
    },
    onDrag: (worlds) => {
      const w = view && worlds.get(view.node);
      if (!drag || !w) return;
      const s = drag.start;
      if (!drag.sent && w.every((v, i) => v === s[i])) return;
      drag.sent = true;
      const frame = Array.from(drag.frame);
      if (gizmo.gizmo.op === GizmoOp.Scale) {
        // the factors along the gizmo's axes: w = start * scale(k)
        const k = [0, 1, 2].map((c) => {
          const a = col3(s, c);
          return dot(col3(w, c), a) / Math.max(dot(a, a), 1e-12);
        });
        queue.send("topology_drag", { frame, scale: k });
      } else {
        // the move or the turn: w = delta * start
        queue.send("topology_drag", {
          frame,
          delta: Array.from(matMul(w, inverse(s))),
        });
      }
    },
    onEnd: async (_ids, _worlds, label) => {
      drag = null;
      const op =
        label === "undo.rotate"
          ? "rotate"
          : label === "undo.scale"
            ? "scale"
            : "translate";
      queue.send("topology_drag_end", { op });
      await queue.done();
    },
  };
  let saved: GizmoCallbacks | null = null;

  // ---- picking: a click, Shift adds (or takes out), a box
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
      queue.send("topology_box", { ...viewArgs(), a: b.a, b: b.b, add: b.add });
    } else {
      const r = viewport.rayFromMouse(b.b[0], b.b[1]);
      queue.send("topology_pick", {
        ...viewArgs(),
        origin: r.origin,
        dir: r.dir,
        mouse: b.b,
        add: b.add,
      });
    }
  });

  // ---- what the engine changes: the view again (once the engine is there: the dev page's mock)
  let attached = true;
  off.push(() => (attached = false));
  void engineReady.then(() => {
    if (!attached) return;
    off.push(
      onEngine("jle:changed", (p) => {
        if (!p || p.scene || p.selection || p.state || p.document) refresh();
      }),
    );
  });
  off.push(
    $effect.root(() => {
      $effect(() => {
        const now = engine.state.tool === "topology";
        void engine.state.gizmo;
        if (now && !saved) {
          saved = gizmo.callbacks;
          gizmo.callbacks = callbacks;
          refresh();
        } else if (!now && saved) {
          gizmo.callbacks = saved;
          saved = null;
          view = null;
          points = null;
          pointsKey = "";
          box = drag = null;
        }
        // the gizmo on what is picked (the select tool turns it off for the other tools)
        if (now) queueMicrotask(() => active() && (gizmo.enabled = true));
        viewport.requestRedraw();
      });
    }),
  );

  // ---- drawing: its edges (a mesh of many triangles: its picked faces only), the faces picked
  off.push(
    viewport.addOverlay((r) => {
      if (!active() || !view) return;
      edges(r);
    }),
  );
  function edges(r: Renderer) {
    const w = world();
    const n = view && store.nodes.get(view.node);
    const d = n ? store.meshOf(n) : null;
    if (!w || !d) return;
    const tris = d.prims.reduce((s, p) => s + p.indices / 3, 0);
    const pos = d.prims.map(
      (p) => new Float32Array(d.buffer, p.pos, p.vertices * 3),
    );
    const idx = d.prims.map((p) => new Uint32Array(d.buffer, p.idx, p.indices));
    const v = (pi: number, i: number): Vec3 =>
      point(w, [pos[pi][i * 3], pos[pi][i * 3 + 1], pos[pi][i * 3 + 2]]);
    if (tris <= kManyTriangles)
      for (let pi = 0; pi < d.prims.length; pi++)
        for (let t = 0; t + 2 < idx[pi].length; t += 3) {
          const a = v(pi, idx[pi][t]),
            b = v(pi, idx[pi][t + 1]),
            c = v(pi, idx[pi][t + 2]);
          r.line(a, b, kEdges);
          r.line(b, c, kEdges);
          r.line(c, a, kEdges);
        }
    const f = view!.selected_faces;
    for (let k = 0; k + 1 < f.length; k += 2) {
      const [pi, t] = [f[k], f[k + 1]];
      if (pi >= idx.length || t * 3 + 2 >= idx[pi].length) continue;
      r.tri(
        v(pi, idx[pi][t * 3]),
        v(pi, idx[pi][t * 3 + 1]),
        v(pi, idx[pi][t * 3 + 2]),
        kFace,
      );
    }
    r.flush(true, true);
  }
  // the points over the view, the box being drawn
  off.push(
    viewport.addOverlay2d((ctx) => {
      if (!active()) return;
      const w = world();
      if (view && !view.faces && points && w) {
        const on = new Set(view.selected_points);
        const dots = (selected: boolean, radius: number, color: string) => {
          ctx.beginPath();
          for (let i = 0; i * 3 < points!.length; i++) {
            if (on.has(i) !== selected) continue;
            const s = viewport.project(
              point(w, [
                points![i * 3],
                points![i * 3 + 1],
                points![i * 3 + 2],
              ]),
            );
            if (!s) continue;
            ctx.moveTo(s[0] + radius, s[1]);
            ctx.arc(s[0], s[1], radius, 0, 2 * Math.PI);
          }
          ctx.fillStyle = color;
          ctx.fill();
        };
        dots(false, 2.5, rgba(220, 230, 240, 200));
        dots(true, 4.5, rgba(255, 160, 40));
      }
      if (box?.selecting) {
        const x = Math.min(box.a[0], box.b[0]),
          y = Math.min(box.a[1], box.b[1]);
        const bw = Math.abs(box.b[0] - box.a[0]),
          bh = Math.abs(box.b[1] - box.a[1]);
        ctx.fillStyle = rgba(255, 160, 40, 30);
        ctx.fillRect(x, y, bw, bh);
        ctx.strokeStyle = rgba(255, 160, 40, 200);
        ctx.lineWidth = 1;
        ctx.strokeRect(x + 0.5, y + 0.5, bw, bh);
      }
    }),
  );

  return () => {
    for (const f of off) f();
    if (saved) gizmo.callbacks = saved;
  };
}
