// The viewport alone on the dev mock: http://localhost:3001/ui/render/dev/index.html (npx vite).
// Click selects (the gizmo moves it), W/E/R: move/rotate/scale, X: world/local, G: wireframe.

import { mount } from "svelte";
import Viewport from "../Viewport.svelte";
import { call } from "../engine";
import { GizmoOp } from "../gizmo";
import { viewport } from "../viewport_api";

mount(Viewport, { target: document.getElementById("app")! });

let hover_seq = 0;
addEventListener("pointermove", async (e) => {
  if (
    !viewport.canvas ||
    e.target !== viewport.canvas ||
    viewport.controls?.busy ||
    viewport.gizmo?.gizmo.active()
  )
    return;
  const seq = ++hover_seq;
  const [x, y] = viewport.mouse(e);
  const hit = await viewport.pick(x, y);
  if (seq !== hover_seq) return;
  viewport.hovered = hit?.node ?? 0;
  viewport.requestRedraw();
});
addEventListener("pointerdown", async (e) => {
  if (e.target !== viewport.canvas || e.button !== 0 || e.altKey) return;
  const [x, y] = viewport.mouse(e);
  const hit = await viewport.pick(x, y);
  await call("select", { ids: hit ? [hit.node] : [] });
});
addEventListener("keydown", (e) => {
  const g = viewport.gizmo?.gizmo;
  if (!g || viewport.controls?.looking) return;
  if (e.code === "KeyW") g.op = GizmoOp.Translate;
  else if (e.code === "KeyE") g.op = GizmoOp.Rotate;
  else if (e.code === "KeyR") g.op = GizmoOp.Scale;
  else if (e.code === "KeyX") g.space = 1 - g.space;
  else if (e.code === "KeyG")
    viewport.setSettings({ wireframe: !viewport.settings.wireframe });
  viewport.requestRedraw();
});
(window as any).viewport = viewport;
