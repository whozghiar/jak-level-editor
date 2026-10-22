// What the menus, the toolbar and the keys do besides calling the engine: the interface's own state
// (the left panel's tab...), the confirmation before losing unsaved changes, the file dialogs.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { engine } from "./engine.svelte";
import { setLanguage } from "./i18n";
import { call } from "./rpc";
import { windows } from "./windows.svelte";

export type LeftTab = "levels" | "build" | "catalog" | "prefabs" | "groups";

export const ui = $state({
  leftTab: "levels" as LeftTab,
  // Level > Replace a level of the game: the inspector shows its section (C++ m_scroll_to_replace)
  scrollToReplace: false,
  // what waits for the user to agree to lose the unsaved changes (the discard window)
  pendingDiscard: null as (() => void) | null,
});

/** Runs `action` now, or after the user agreed to lose the unsaved changes (confirm_discard). */
export function confirmDiscard(action: () => void): void {
  const d = engine.state.document;
  if (!d.unsaved || !d.open) return action();
  ui.pendingDiscard = action;
  windows.open("discard");
}

/**
 * Saves the project (save in actions.cpp): the engine writes it, or answers {ask_name} the first time
 * and with Save as, and the Save window names it. Undefined when it failed.
 */
export async function save(
  saveAs: boolean,
): Promise<{ saved?: boolean; ask_name?: string } | undefined> {
  const r = await call<{ saved?: boolean; ask_name?: string }>("save", {
    save_as: saveAs,
  });
  if (r?.ask_name !== undefined) windows.open("save_as", { name: r.ask_name });
  return r;
}

export function newLevel(): void {
  confirmDiscard(() => void call("new_level"));
}

export function openProjectFile(path: string): void {
  confirmDiscard(() => void call("open_project_file", { path }));
}

/** File > Open project: a .glb of the library's projects. */
export function openProject(): void {
  confirmDiscard(async () => {
    const path = await open({
      filters: [{ name: "glTF (.glb)", extensions: ["glb"] }],
      defaultPath: engine.state.projects_root || undefined,
    });
    if (typeof path === "string") call("open_project_file", { path });
  });
}

/** File > Import a model: a .glb made elsewhere (Blender...), placed like a model of the catalog. */
export async function importModel(): Promise<void> {
  const path = await open({
    filters: [{ name: "glTF (.glb, .gltf)", extensions: ["glb", "gltf"] }],
    defaultPath: engine.state.projects_root || undefined,
  });
  if (typeof path !== "string") return;
  const name = path.replace(/^.*[\\/]/, "").replace(/\.[^.]*$/, "");
  call("start_model_placement", { name, path, once: false });
}

/** File > Quit, the title bar's cross: the window's close, which asks first (LevelEditor). */
export function quit(): void {
  if ("__TAURI_INTERNALS__" in window) getCurrentWindow().close();
}

/**
 * View > Frame the selection (F), Frame the level (Home): the camera is the viewport's, which listens
 * to this window event (detail "selection" or "all").
 */
export function frameView(what: "selection" | "all"): void {
  window.dispatchEvent(new CustomEvent("jle:frame", { detail: what }));
}

/** A setting of the View menu turned on or off. */
export function toggleView(key: string): void {
  call("set_view", { [key]: !engine.state.view[key] });
}

/** View > Language: the interface's and the engine's (its log, the assistant's answers). */
export function changeLanguage(code: string): void {
  call("set_language", { code: setLanguage(code) });
}

/** Level > Play in the game (the toolbar's Play): the Play window, and the game runtime started on
 * the level unless it works already or cannot play (the window says why, or offers to install
 * OpenGOAL). */
export function playLevel(): void {
  windows.open("play");
  if (!engine.jobs.play && engine.state.runtime?.available) call("play_level");
}
