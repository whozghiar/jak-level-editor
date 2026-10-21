// The Place tool in the view (app.cpp place_tool, draw_tool_overlays, start_free_rotation): what
// is placed drawn see-through where the cursor points (the engine's placement_matrix, from its
// pick), a click places it; the left button held raises it (up and down, Ctrl: forward and back)
// and moves it along x (left and right, Shift finer), placed when it is let go (Shift held then:
// copies again); R and Ctrl + wheel turn it, the right button held tilts it every way (free
// rotation). A drag from a panel ("jle:panel-drag" start ... end, the C++ OGLE_PREFAB payload) places
// one where it is dropped over the view, nothing elsewhere. The gizmo is off while placing (the
// select tool's: it is on with it only).
// Viewport.svelte attaches it (`attachPlaceTool`).

import { engine } from "../lib/engine.svelte";
import { shortcuts } from "../lib/shortcuts";
import { call } from "./engine";
import {
  add,
  axisAngle,
  col3,
  matMul,
  quatMul,
  radians,
  rotate,
  sub,
  translate,
  type Mat4,
  type Quat,
  type Vec2,
  type Vec3,
} from "./math";
import { lockPointer } from "./controls";
import { col32 } from "./renderer";
import type { MeshEntry } from "./scene_store";
import { dragOffset } from "./select_tool";
import { viewport } from "./viewport_api";

const Y: Vec3 = [0, 1, 0];
// std::round: halves away from zero
const round = (v: number) => Math.sign(v) * Math.round(Math.abs(v));
const qnorm = (q: Quat): Quat => {
  const l = Math.hypot(...q) || 1;
  return [q[0] / l, q[1] / l, q[2] / l, q[3] / l];
};

export function attachPlaceTool(): () => void {
  const canvas = viewport.canvas!;
  const controls = viewport.controls!;
  const off: (() => void)[] = [];
  const on = <K extends keyof WindowEventMap>(t: EventTarget, type: K, fn: (e: WindowEventMap[K]) => void, capture = false) => {
    t.addEventListener(type, fn as EventListener, capture);
    off.push(() => t.removeEventListener(type, fn as EventListener, capture));
  };

  // where it would go (m_place_matrix), valid when over the view
  let matrix: Mat4 | null = null;
  let mouse: Vec2 = [0, 0];
  let over = false;
  let ctrl = false;
  // the left button held where it is placed (m_place_drag)
  let drag: { anchor: Mat4; px: Vec2 } | null = null;
  // the right button held: free rotation (m_free_rotating_placement)
  let turning: { right: Vec3; yaw: number; pitch: number; tilt0: Quat; tilt: Quat } | null = null;
  // a drag from a panel going on (m_drag_from_panel)
  let panel = false;
  let preview: MeshEntry | null = null;
  let previewUid = 0;

  const placing = () => engine.state.tool === "place";
  const p = () => engine.state.placement as { yaw: number; tilt: Quat; mesh: number; version: number } | null;

  // the engine's placement_matrix for the cursor (one question at a time, the last position asked
  // again when it moved meanwhile)
  let asking = false;
  let again = false;
  async function ask() {
    if (asking) return void (again = true);
    asking = true;
    do {
      again = false;
      const { origin, dir } = viewport.rayFromMouse(mouse[0], mouse[1]);
      const r = await call("placement_preview", { origin, dir }).catch(() => null);
      if (!drag && !turning) matrix = r && over ? (r.matrix as Mat4) : null;
    } while (again);
    asking = false;
    viewport.requestRedraw();
  }

  function moved(e: PointerEvent | MouseEvent) {
    const r = canvas.getBoundingClientRect();
    mouse = [e.clientX - r.left, e.clientY - r.top];
    ctrl = e.ctrlKey;
    // over the view, no window over it (a panel's drag: over it too)
    over = document.elementFromPoint(e.clientX, e.clientY) === canvas;
  }

  on(window, "pointermove", (e) => {
    if (!placing() && !panel) return;
    moved(e);
    viewport.hovered = 0; // nothing is pointed at but where it goes
    if (turning) return;
    if (drag) {
      const k = e.shiftKey ? 0.25 : 1;
      drag.px = [drag.px[0] + e.movementX * k, drag.px[1] + e.movementY * k];
      const at = col3(drag.anchor, 3);
      const o = dragOffset(viewport.camera, viewport.size()[1], drag.px, at, ctrl, engine.state.gizmo.snap);
      matrix = matMul(translate(o), drag.anchor);
      viewport.requestRedraw();
      return;
    }
    if (over) void ask();
    else if (matrix) {
      matrix = null;
      viewport.requestRedraw();
    }
  });

  // a click places (the left button held first: raised, moved); in the capture phase, before the
  // select tool
  on(
    canvas,
    "pointerdown",
    (e) => {
      if (!placing() || panel || e.button !== 0 || e.altKey) return;
      e.stopImmediatePropagation();
      moved(e);
      if (!matrix) return;
      drag = { anchor: matrix, px: [0, 0] };
      canvas.setPointerCapture(e.pointerId);
    },
    true,
  );
  on(window, "pointerup", (e) => {
    if (e.button !== 0 || !drag) return;
    const m = matrix;
    drag = null;
    // placed: selected (a prefab all of it) with the gizmo on it; Shift: copies again
    if (m)
      call("place_at", { matrix: Array.from(m), keep: e.shiftKey })
        .then(() => void ask())
        .catch((err) => console.warn("[place]", err));
  });

  // a panel's drag: dropped over the view, placed there, else nothing
  on(window, "jle:panel-drag" as keyof WindowEventMap, (e) => {
    const what = (e as CustomEvent).detail;
    if (what === "start") {
      panel = true;
      return;
    }
    if (what !== "end" || !panel) return;
    panel = false;
    // where the pointer is let go (the preview's answer may not be there yet after a quick drag)
    if (over) call("place_at", { ...viewport.rayFromMouse(mouse[0], mouse[1]) }).catch((err) => console.warn("[place]", err));
    else void call("stop_placement");
    matrix = null;
    viewport.requestRedraw();
  });

  // the right button on what is placed tilts it, every way (start_free_rotation)
  const prevRight = controls.hooks.rightPress;
  controls.hooks.rightPress = (x, y, e) => {
    if (!placing()) return prevRight?.(x, y, e) ?? false;
    const pl = p();
    if (!matrix || !pl) return false;
    const tilt0: Quat = pl.tilt ?? [0, 0, 0, 1];
    turning = { right: viewport.camera.right(), yaw: 0, pitch: 0, tilt0, tilt: tilt0 };
    shortcuts.paused = true;
    // the cursor stays where it is: the mouse turns it as far as wanted
    lockPointer(canvas, () => !!turning);
    return true;
  };
  on(window, "mousemove", (e) => {
    if (!turning || !matrix) return;
    const k = e.shiftKey ? 0.002 : 0.008; // radians per pixel
    turning.yaw += e.movementX * k;
    turning.pitch += e.movementY * k;
    let yaw = turning.yaw,
      pitch = turning.pitch;
    const snap = engine.state.gizmo.snap;
    if (snap.enabled !== e.ctrlKey && snap.rotate_deg > 0) {
      const s = radians(snap.rotate_deg);
      yaw = round(yaw / s) * s;
      pitch = round(pitch / s) * s;
    }
    const r = qnorm(quatMul(axisAngle(Y, yaw), axisAngle(turning.right, pitch)));
    turning.tilt = qnorm(quatMul(r, turning.tilt0));
    // the preview where it was, turned (the cursor does not move it while it turns)
    matrix = matMul(matMul(translate(col3(matrix, 3)), rotate(turning.tilt)), rotate(axisAngle(Y, p()?.yaw ?? 0)));
    viewport.requestRedraw();
  });
  on(window, "mouseup", (e) => {
    if (e.button !== 2 || !turning) return;
    const tilt = turning.tilt;
    turning = null;
    shortcuts.paused = false;
    if (document.pointerLockElement === canvas) document.exitPointerLock();
    void call("set_placement", { tilt: Array.from(tilt) });
  });

  // Ctrl + wheel turns it
  const prevWheel = controls.hooks.wheel;
  controls.hooks.wheel = (wheel, e) => {
    if (!placing() || !e.ctrlKey) return prevWheel?.(wheel, e) ?? false;
    void call("turn_placement", { degrees: wheel * 15 });
    return true;
  };

  // what is placed, see-through, where it would go; a cross while its file loads
  off.push(
    viewport.addOverlay((r, rs) => {
      if (!placing() || !matrix) return;
      const c = col3(matrix, 3);
      if (preview?.data) {
        r.drawMesh(preview, matrix, viewport.store.materials, viewport.store, rs, [0.35, 0.75, 1, 0.25], 0.7);
      } else {
        const grey = col32(0x80, 0x80, 0x80);
        r.line(sub(c, [1, 0, 0]), add(c, [1, 0, 0]), grey);
        r.line(sub(c, [0, 0, 1]), add(c, [0, 0, 1]), grey);
        r.line(c, add(c, [0, 1.5, 0]), grey);
      }
      r.circle(add(c, [0, 0.03, 0]), [1, 0, 0], [0, 0, 1], 0.5, col32(0xff, 0xd0, 0x40), 24);
      if (drag) {
        // where it was clicked, and the way to where it goes
        const a = col3(drag.anchor, 3);
        r.circle(add(a, [0, 0.03, 0]), [1, 0, 0], [0, 0, 1], 0.3, col32(0x80, 0x80, 0x80), 16);
        r.line(a, [c[0], a[1], c[2]], col32(0xff, 0x40, 0x40));
        r.line([c[0], a[1], c[2]], c, col32(0x40, 0xff, 0x40));
      }
      r.flush(true);
    }),
  );

  // the tool's state: the preview's mesh kept, the preview asked again when it turns (R, Ctrl +
  // wheel)
  off.push(
    $effect.root(() => {
      let turn = "";
      $effect(() => {
        const now = engine.state.tool === "place";
        const pl = p();
        const uid = now && pl?.mesh ? pl.mesh : 0;
        if (uid !== previewUid) {
          if (previewUid) viewport.store.releaseMesh(previewUid);
          preview = uid ? viewport.store.retainMesh(uid, pl!.version) : null;
          previewUid = uid;
        }
        const key = JSON.stringify([pl?.yaw, pl?.tilt]);
        if (!now) {
          matrix = drag = turning = null;
        } else if (over && key !== turn) {
          void ask();
        }
        turn = key;
        viewport.requestRedraw();
      });
    }),
  );

  return () => {
    for (const f of off) f();
    controls.hooks.rightPress = prevRight;
    controls.hooks.wheel = prevWheel;
    if (previewUid) viewport.store.releaseMesh(previewUid);
  };
}
