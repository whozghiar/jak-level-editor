/**
 * A drag from a panel into the viewport (the C++ panels' BeginDragDropSource): pointer events, as
 * Tauri's native drag-drop blocks HTML5's on Windows. From a pointerdown: after 4 px with the left
 * button held, `start` begins the placement (start_*_placement {once: true}), `name` follows the
 * cursor (ImGui's drag-drop tooltip) and the window gets the event "jle:panel-drag" {detail:
 * "start"}; the button released, "jle:panel-drag" {detail: "end"}: the place tool drops it there
 * when the pointer is over the viewport (C++ m_drag_from_panel). Released outside the window (its
 * pointerup lost: the pointer back with no button, the window left), {detail: "cancel"}: nothing
 * placed. The click on the element let go of after a drag is not a click.
 */
export function panelDrag(
  e: PointerEvent,
  name: string,
  start: () => void,
): void {
  if (e.button !== 0) return;
  const x = e.clientX;
  const y = e.clientY;
  let label: HTMLElement | null = null;
  const follow = (m: PointerEvent) =>
    label &&
    (label.style.transform = `translate(${m.clientX + 14}px, ${m.clientY + 12}px)`);
  const move = (m: PointerEvent) => {
    if (!(m.buttons & 1)) return end("cancel");
    if (label) return void follow(m);
    if (Math.hypot(m.clientX - x, m.clientY - y) < 4) return;
    start();
    label = document.body.appendChild(document.createElement("div"));
    label.className =
      "pointer-events-none fixed top-0 left-0 z-[100] rounded-md border border-white/10 bg-[#0b0b0b] px-2 py-1 text-xs text-neutral-200 shadow-xl shadow-black/60";
    label.textContent = name;
    follow(m);
    window.dispatchEvent(
      new CustomEvent("jle:panel-drag", { detail: "start" }),
    );
  };
  const up = () => end("end");
  const cancel = () => end("cancel");
  function end(how: "end" | "cancel") {
    removeEventListener("pointermove", move);
    removeEventListener("pointerup", up);
    removeEventListener("pointercancel", cancel);
    removeEventListener("blur", cancel);
    if (!label) return;
    label.remove();
    window.dispatchEvent(new CustomEvent("jle:panel-drag", { detail: how }));
    if (how === "cancel") return;
    // a click follows the pointerup in the same task, when it is on the element pressed
    const swallow = (c: MouseEvent) => c.stopPropagation();
    addEventListener("click", swallow, { capture: true });
    setTimeout(() => removeEventListener("click", swallow, { capture: true }));
  }
  addEventListener("pointermove", move);
  addEventListener("pointerup", up);
  addEventListener("pointercancel", cancel);
  addEventListener("blur", cancel);
}
