// The viewport alone on the dev mock: http://localhost:3001/ui/render/dev/index.html (npx vite).
// The select tool (select_tool.ts) clicks, boxes, drags and turns; W/E/R: move/rotate/scale, X: world/local, G: wireframe.

import { mount } from "svelte";
import Viewport from "../Viewport.svelte";
import { GizmoOp } from "../gizmo";
import { viewport } from "../viewport_api";

mount(Viewport, { target: document.getElementById("app")! });

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
