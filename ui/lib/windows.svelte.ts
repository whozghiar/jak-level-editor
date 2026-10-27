// The floating windows (the C++ editor's ImGui windows and popups), opened by id:
// windows.open("export"), windows.open("guide", { tab: 2 }). The props given to open() are given to
// the window's component. A window of one's own: its component in ui/components/windows/, an entry in
// `registry`.

import type { Component } from "svelte";
import { call } from "./rpc";
import DiscardWindow from "../components/windows/DiscardWindow.svelte";
import ExportWindow from "../components/windows/ExportWindow.svelte";
import ExtractWindow from "../components/windows/ExtractWindow.svelte";
import GameWindow from "../components/windows/GameWindow.svelte";
import GuideWindow from "../components/windows/GuideWindow.svelte";
import LogWindow from "../components/windows/LogWindow.svelte";
import NavMeshWindow from "../components/windows/NavMeshWindow.svelte";
import NewGroupWindow from "../components/windows/NewGroupWindow.svelte";
import NewPrefabWindow from "../components/windows/NewPrefabWindow.svelte";
import SaveAsWindow from "../components/windows/SaveAsWindow.svelte";
import SettingsWindow from "../components/windows/SettingsWindow.svelte";
import TerrainWindow from "../components/windows/TerrainWindow.svelte";
import TexturesWindow from "../components/windows/TexturesWindow.svelte";
import WelcomeWindow from "../components/windows/WelcomeWindow.svelte";

export interface WindowDef {
  title: string; // its translation key
  component: Component<any>;
  width?: number; // pixels at first (then the user's)
  height?: number; // else as tall as its content
  modal?: boolean; // over a dimmed editor, Esc closes it (the C++ popups)
  anchor?: "center" | "top-right"; // where it opens
  onclose?: () => void; // closed by the user (its cross, Esc)
  fresh?: boolean; // opened while open: its content made again (the C++ open_export_window)
}

export const registry: Record<string, WindowDef> = {
  extract: { title: "extract.title", component: ExtractWindow, width: 640 }, // File > Extract game assets (extract.cpp)
  export: {
    title: "export.title",
    component: ExportWindow,
    width: 560,
    fresh: true,
  }, // Level > Export for the game
  navmesh: { title: "navmesh.title", component: NavMeshWindow, width: 430 }, // Level > Nav meshes
  game: {
    title: "game.title",
    component: GameWindow,
    width: 440,
    anchor: "top-right",
  }, // View > Sky, hour and weather (game_window.cpp)
  guide: {
    title: "guide.title",
    component: GuideWindow,
    width: 980,
    height: 700,
  }, // Help, F1 (guide_panel.cpp)
  // the terrain brush's: open while the tool is (terrain_tool.cpp); closed, the brush ends
  terrain: {
    title: "terrain.title",
    component: TerrainWindow,
    width: 270,
    anchor: "top-right",
    onclose: () => void call("stop_terrain_brush"),
  },
  textures: {
    title: "textures.title",
    component: TexturesWindow,
    width: 640,
    height: 520,
  }, // the inspector's Game textures...
  save_as: {
    title: "save.title",
    component: SaveAsWindow,
    width: 460,
    modal: true,
  },
  new_prefab: {
    title: "prefab_popup.title",
    component: NewPrefabWindow,
    width: 420,
    modal: true,
  },
  new_group: {
    title: "group.new_title",
    component: NewGroupWindow,
    width: 400,
    modal: true,
  }, // groups.cpp
  discard: { title: "discard.title", component: DiscardWindow, modal: true }, // confirm_discard
  log: { title: "log.title", component: LogWindow, width: 760, height: 360 }, // View > Log
  settings: { title: "settings.title", component: SettingsWindow, width: 520 }, // File > Settings
  welcome: {
    title: "welcome.title",
    component: WelcomeWindow,
    width: 600,
    modal: true,
  }, // the first start: the library
};

export type OpenWindow = {
  id: string;
  props: Record<string, any>;
  z: number;
  opened: number;
};

class Windows {
  list = $state<OpenWindow[]>([]);
  #top = 0;

  /** Opens it (in front; its props replaced when it is open already). */
  open(id: string, props: Record<string, any> = {}): void {
    if (!registry[id]) return console.error(`no window ${id}`);
    const w = this.list.find((w) => w.id === id);
    if (w) {
      w.props = props;
      w.z = ++this.#top;
      if (registry[id].fresh) w.opened++;
    } else {
      this.list.push({ id, props, z: ++this.#top, opened: 0 });
    }
  }

  /** Closes it; `byUser`: its cross or Esc (its onclose runs). */
  close(id: string, byUser = false): void {
    const i = this.list.findIndex((w) => w.id === id);
    if (i < 0) return;
    this.list.splice(i, 1);
    if (byUser) registry[id].onclose?.();
  }

  toggle(id: string, props: Record<string, any> = {}): void {
    if (this.isOpen(id)) this.close(id, true);
    else this.open(id, props);
  }

  isOpen(id: string): boolean {
    return this.list.some((w) => w.id === id);
  }

  focus(id: string): void {
    const w = this.list.find((w) => w.id === id);
    if (w && w.z !== this.#top) w.z = ++this.#top;
  }

  /** A modal window is open: the editor's keys wait. */
  get modal(): boolean {
    return this.list.some((w) => registry[w.id].modal);
  }
}

export const windows = new Windows();
