/**
 * A drag from a panel into the viewport (the C++ panels' BeginDragDropSource): pointer events, as
 * Tauri's native drag-drop blocks HTML5's on Windows. From a pointerdown: after 4 px with the left
 * button held, `start` begins the placement (start_*_placement {once: true}) and the window gets
 * the event "jle:panel-drag" {detail: "start"}; the button released anywhere, "jle:panel-drag"
 * {detail: "end"}: the place tool drops it there when the pointer is over the viewport (C++
 * m_drag_from_panel). The click on the element let go of after a drag is not a click.
 */
export function panelDrag(e: PointerEvent, start: () => void): void {
  if (e.button !== 0) return;
  const x = e.clientX;
  const y = e.clientY;
  let on = false;
  const move = (m: PointerEvent) => {
    if (on || Math.hypot(m.clientX - x, m.clientY - y) < 4) return;
    on = true;
    start();
    window.dispatchEvent(new CustomEvent("jle:panel-drag", { detail: "start" }));
  };
  const up = () => {
    removeEventListener("pointermove", move);
    removeEventListener("pointerup", up);
    if (!on) return;
    window.dispatchEvent(new CustomEvent("jle:panel-drag", { detail: "end" }));
    // a click follows the pointerup in the same task, when it is on the element pressed
    const swallow = (c: MouseEvent) => c.stopPropagation();
    addEventListener("click", swallow, { capture: true });
    setTimeout(() => removeEventListener("click", swallow, { capture: true }));
  };
  addEventListener("pointermove", move);
  addEventListener("pointerup", up);
}
