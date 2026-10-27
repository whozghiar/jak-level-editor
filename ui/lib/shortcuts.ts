// The editor's keys (handle_shortcuts in app.cpp), in the same order: the first that matches wins.

import {
  frameView,
  newLevel,
  openProject,
  save,
  toggleView,
} from "./actions.svelte";
import { engine } from "./engine.svelte";
import { call } from "./rpc";
import { windows } from "./windows.svelte";

/**
 * Set by the viewport while the mouse flies the camera or turns the selection (C++ m_looking,
 * m_free_rotating): the keys are its own then (W A S D...).
 */
export const shortcuts = { paused: false };

// typing in a field: its keys are the field's
function typing(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  if (
    el.isContentEditable ||
    el.tagName === "TEXTAREA" ||
    el.tagName === "SELECT"
  )
    return true;
  return (
    el instanceof HTMLInputElement &&
    !["checkbox", "radio", "button", "range", "color"].includes(el.type)
  );
}

/** The window's keydown: runs the editor's action for the key, if any. */
export function handleShortcut(e: KeyboardEvent): void {
  if (
    e.defaultPrevented ||
    e.repeat ||
    shortcuts.paused ||
    document.pointerLockElement ||
    windows.modal
  )
    return;
  if (typing(e.target)) return;
  const s = engine.state;
  const ctrl = e.ctrlKey || e.metaKey;
  const shift = e.shiftKey;
  // a letter of another alphabet (Cyrillic...): the key's Latin letter, as SDL's keycodes give it
  const k =
    e.key.length === 1 && e.key.charCodeAt(0) > 127 && /^Key[A-Z]$/.test(e.code)
      ? e.code.slice(3).toLowerCase()
      : e.key.toLowerCase();
  // [ and ]: the keys right of P too (ImGui reads them by their place: ^ and $ on AZERTY)
  const bracket =
    k === "[" || e.code === "BracketLeft"
      ? "["
      : k === "]" || e.code === "BracketRight"
        ? "]"
        : "";
  const sel = s.selection.count > 0;
  const tool = s.tool;
  if (ctrl && k === "z") call(shift ? "redo" : "undo");
  else if (ctrl && k === "y") call("redo");
  else if (ctrl && k === "s") save(shift);
  else if (ctrl && k === "o") openProject();
  else if (ctrl && k === "n") newLevel();
  else if (ctrl && k === "c") call("copy_selection");
  else if (ctrl && k === "v") call("paste");
  else if (ctrl && k === "d") call("duplicate_selection");
  else if (ctrl && k === "l")
    sel && call("select_level", { group: s.selection.level });
  else if (ctrl && shift && k === "g") sel && windows.open("new_group");
  else if (ctrl && k === "g")
    s.selection.primary && call("select_assembly", { id: s.selection.primary });
  // stops placing, the topology, a terrain stroke shown, the brush, else selects nothing
  else if (k === "escape") call("escape");
  else if (!ctrl && k === "b")
    call(tool === "terrain" ? "stop_terrain_brush" : "start_terrain_brush");
  else if (tool === "terrain" && !ctrl && k === "m")
    call("set_brush", { magnet: !s.brush.magnet });
  else if (tool === "terrain" && bracket)
    call("set_brush", {
      size: Math.min(
        64,
        Math.max(0.5, s.brush.size * (bracket === "]" ? 1.25 : 0.8)),
      ),
    });
  else if (!ctrl && k === "t")
    call(tool === "topology" ? "stop_topology" : "start_topology");
  else if (!ctrl && k === "c")
    call(tool === "collision" ? "stop_collision_tool" : "start_collision_tool");
  else if (tool === "place" && !ctrl && k === "r")
    call("turn_placement", { degrees: shift ? -45 : 45 });
  else if (k === "delete")
    call(
      tool === "topology"
        ? "topology_delete"
        : tool === "collision"
          ? "collision_delete"
          : "delete_selection",
    );
  else if (!ctrl && k === "f") frameView("selection");
  else if (k === "home") frameView("all");
  else if (!ctrl && k === "w") call("set_gizmo", { mode: "translate" });
  else if (!ctrl && k === "e") call("set_gizmo", { mode: "rotate" });
  else if (!ctrl && k === "r") call("set_gizmo", { mode: "scale" });
  else if (k === "f1") windows.toggle("guide", { tab: 0 });
  else if (!ctrl && k === "g" && s.view.game_view_ready)
    toggleView("game_view");
  else return;
  e.preventDefault();
}
