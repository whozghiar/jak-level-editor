// What the inspector's sections share: what they show asked again after each change of the document
// (not while a field is dragged: it shows its own value), and their edits sent in order (the engine
// runs calls on a pool of threads: a drag's last value must arrive last).

import { call, onEngine } from "../../lib/rpc";

export const inspector = $state({
  // counts the changes of the document, the selection, the materials (jle:changed)
  tick: 0,
  // a field is being dragged: its live edits are not shown back
  dragging: false,
});

/** Follows the engine's changes (the inspector mounts it); returns what stops it. */
export function followChanges(): () => void {
  return onEngine<{
    scene?: boolean;
    selection?: boolean;
    materials?: boolean;
    textures?: boolean;
  } | null>("jle:changed", (p) => {
    if (
      !inspector.dragging &&
      (!p || p.scene || p.selection || p.materials || p.textures)
    )
      inspector.tick++;
  });
}

/** The engine's answer to `name` with the arguments `args()` gives (null: nothing asked), asked again
 * when they change and after each change of the document. In a component's script. */
export function query<T = any>(
  name: string,
  args: () => Record<string, unknown> | null,
): { value: T | null } {
  const r = $state({ value: null as T | null });
  $effect(() => {
    const a = args();
    void inspector.tick;
    if (!a) {
      r.value = null;
      return;
    }
    let alive = true;
    call<T>(name, a, true).then((v) => {
      if (alive) r.value = v ?? null;
    });
    return () => {
      alive = false;
    };
  });
  return r;
}

let queue: Promise<unknown> = Promise.resolve();

/** A call after the ones sent before it. */
export function send<T = any>(
  name: string,
  args: Record<string, unknown>,
): Promise<T | undefined> {
  const p = queue.then(() => call<T>(name, args));
  queue = p;
  return p;
}

// the last value of the field dragged not sent yet (one live call at a time: the others are skipped)
let pending: { name: string; args: Record<string, unknown> } | null = null;
let pumping = false;

/** A field being dragged: its value shown at once in the view, not yet an undo step. */
export function live(name: string, args: Record<string, unknown>): void {
  inspector.dragging = true;
  pending = { name, args };
  if (pumping) return;
  pumping = true;
  const pump = () => {
    const l = pending;
    pending = null;
    if (!l) return void (pumping = false);
    send(l.name, { ...l.args, live: true }).then(pump);
  };
  pump();
}

/** The change made (the drag let go, a value typed, a choice): one undo step. */
export function commit(
  name: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  pending = null;
  inspector.dragging = false;
  return send(name, args);
}

// the looks of the sections: the frame's, and a field as small as ImGui's
export { button, check, heading, muted, small } from "../common/classes";
export const field =
  "min-w-0 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-1.5 py-0.5 text-sm text-neutral-200 outline-none focus:border-orange-600";

/** use:escapeReverts on a field applied by its change event (Enter, leaving it): Escape gives it back
 * the value it had when it was entered, nothing applied (ImGui's InputText). */
export function escapeReverts(el: HTMLInputElement) {
  let at = el.value;
  const focus = () => (at = el.value);
  const key = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    el.value = at; // as it was when focused: leaving it fires no change
    el.blur();
  };
  el.addEventListener("focus", focus);
  el.addEventListener("keydown", key);
  return {
    destroy() {
      el.removeEventListener("focus", focus);
      el.removeEventListener("keydown", key);
    },
  };
}
