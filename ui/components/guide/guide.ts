// What the guide explains (guide_panel.cpp): every tool with its animation, the sections of Every
// tool, the tutorials (F1: Getting started, then one per tool or task, each named, a few steps each).

// The keys guide.<id>.title, .text, .example and .keys (optional); its animation (demos.ts), or none
export const guideDemos: Record<string, string | null> = {
  camera: "camera",
  select: "select",
  box_select: "box_select",
  move: "move",
  rotate: "rotate",
  scale: "scale",
  snap: "snap",
  local: "local",
  free_rotate: "free_rotate",
  game_view: null,
  build: "build",
  palette: "build",
  catalog: "place",
  interactive: null,
  models: "place",
  decor: "place",
  jak2: null,
  prefab: "prefab",
  game_prefabs: "prefab",
  bounce: "bounce",
  launch: "launch",
  collision: null,
  ingame: null,
  surface: null,
  event: null,
  levels: "story",
  story: "story",
  replace: "replace",
  export: null,
  place_drag: "place",
  topology: null,
  shape_edit: "build",
  style: null,
  preview: null,
  jak2_level: null,
  terrain: null,
  assembly: null,
  groups: null,
  effects: null,
};

// the sections of Every tool
export const guideSections: { key: string; ids: string[] }[] = [
  {
    key: "guide.section.view",
    ids: ["camera", "select", "box_select", "assembly", "groups", "game_view"],
  },
  {
    key: "guide.section.transform",
    ids: [
      "move",
      "rotate",
      "free_rotate",
      "scale",
      "snap",
      "local",
      "place_drag",
      "topology",
    ],
  },
  {
    key: "guide.section.build",
    ids: [
      "build",
      "shape_edit",
      "style",
      "terrain",
      "palette",
      "catalog",
      "interactive",
      "effects",
      "models",
      "decor",
      "jak2",
      "prefab",
      "game_prefabs",
    ],
  },
  {
    key: "guide.section.behaviors",
    ids: [
      "ingame",
      "collision",
      "surface",
      "event",
      "bounce",
      "launch",
      "preview",
    ],
  },
  {
    key: "guide.section.level",
    ids: ["levels", "jak2_level", "story", "replace", "export"],
  },
];

// What a step's button shows: a window, a tab of the left panel, a section of the inspector...
export type TourAction =
  | "extract"
  | "levels_tab"
  | "build_tab"
  | "catalog_tab"
  | "prefabs_tab"
  | "replace"
  | "export"
  | "game_view"
  | "save"
  | "navmeshes"
  | "settings"
  | "game_window"
  | "manual";

// A step: its id, its animations, its button (the key <prefix><id>.action)
export type TourStep = [id: string, demos: string[], action?: TourAction];

// The tutorials: tuto.<id>.title names one; a step's keys are <prefix><step>.title, .text and .action
// prettier-ignore
export const tutorials: { id: string; prefix: string; steps: TourStep[] }[] = [
  {
    id: "start",
    prefix: "tour.",
    steps: [
      ["extract", [], "extract"],
      ["levels", ["story"], "levels_tab"],
      ["camera", ["camera"]],
      ["select", ["select", "box_select"]],
      ["gizmo", ["move", "rotate", "scale", "snap"]],
      ["build", ["build"], "build_tab"],
      ["catalog", ["place"], "catalog_tab"],
      ["prefabs", ["prefab"], "prefabs_tab"],
      ["behaviors", ["bounce", "launch"]],
      ["replace", ["replace"], "replace"],
      ["export", [], "export"],
      ["play", [], "manual"],
    ],
  },
  {
    id: "first_start",
    prefix: "tuto.first_start.",
    steps: [["library", [], "settings"], ["extractor", [], "extract"], ["disc", [], "extract"], ["jak2", []], ["open", [], "levels_tab"]],
  },
  { id: "move", prefix: "tuto.move.", steps: [["select", ["select", "box_select"]], ["drag", ["move"]], ["fine", ["snap"]], ["exact", []]] },
  { id: "rotate", prefix: "tuto.rotate.", steps: [["rings", ["rotate"]], ["free", ["free_rotate"]], ["exact", []]] },
  { id: "scale", prefix: "tuto.scale.", steps: [["axes", ["scale"]], ["exact", []], ["shapes", ["build"], "build_tab"]] },
  { id: "local", prefix: "tuto.local.", steps: [["what", ["local"]], ["when", ["local"]]] },
  { id: "snap", prefix: "tuto.snap.", steps: [["on", ["snap"]], ["steps", ["snap"]], ["place", ["place"]]] },
  { id: "game_view", prefix: "tuto.game_view.", steps: [["what", [], "game_view"], ["hour", [], "game_window"], ["live", []], ["limits", []]] },
  {
    id: "ingame",
    prefix: "tuto.ingame.",
    steps: [["section", []], ["collision", []], ["surface", []], ["event", []], ["more", []], ["jumps", ["bounce", "launch"]]],
  },
  { id: "materials", prefix: "tuto.materials.", steps: [["window", []], ["catalog", ["build"], "catalog_tab"], ["light", []]] },
  { id: "catalog", prefix: "tuto.catalog.", steps: [["game", [], "catalog_tab"], ["objects", ["place"]], ["decor", []], ["jak2", []]] },
  { id: "prefabs", prefix: "tuto.prefabs.", steps: [["game", ["prefab"], "prefabs_tab"], ["mine", ["prefab"]]] },
  { id: "levels", prefix: "tuto.levels.", steps: [["open", ["story"], "levels_tab"], ["add", []], ["story", ["story"]]] },
  { id: "replace", prefix: "tuto.replace.", steps: [["choose", ["replace"], "replace"], ["align", ["replace"]], ["export", [], "export"]] },
  { id: "save", prefix: "tuto.save.", steps: [["save", [], "save"], ["export", [], "export"], ["navmesh", [], "navmeshes"]] },
  {
    id: "manual",
    prefix: "tuto.manual.",
    steps: [["export", [], "export"], ["jakproject", []], ["copy", []], ["register", []], ["build", []], ["start", []]],
  },
  { id: "place", prefix: "tuto.place.", steps: [["click", ["place"], "catalog_tab"], ["hold", ["place"]], ["turn", ["free_rotate"]]] },
  { id: "topology", prefix: "tuto.topology.", steps: [["enter", []], ["points", []], ["faces", []], ["undo", []]] },
  { id: "shapes", prefix: "tuto.shapes.", steps: [["size", ["build"], "build_tab"], ["bend", []], ["ground", []]] },
  { id: "style", prefix: "tuto.style.", steps: [["choose", [], "build_tab"], ["floor", []], ["walls", []]] },
  { id: "preview", prefix: "tuto.preview.", steps: [["play", [], "catalog_tab"], ["motion", []], ["break", []]] },
  { id: "jak2level", prefix: "tuto.jak2level.", steps: [["choose", [], "levels_tab"], ["what", []], ["export", [], "export"]] },
  {
    id: "terrain",
    prefix: "tuto.terrain.",
    steps: [["start", [], "build_tab"], ["paint", []], ["shape", []], ["walls", []], ["generate", []]],
  },
  { id: "assembly", prefix: "tuto.assembly.", steps: [["select", ["select"]], ["prefab", ["prefab"]], ["game", ["prefab"], "prefabs_tab"]] },
];
