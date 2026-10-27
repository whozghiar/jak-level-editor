// The engine's state as the interface shows it: call("get_state"), again after each jle:changed whose
// payload has `state` or `selection` (or no payload), at most once per animation frame. Until the
// engine answers (or when it fails), the defaults below.
//
// What get_state answers (the fields the frame reads; panels and windows may read more):
//   document: { open, title, project_path, unsaved }
//       open: a level or a new one is being edited (document_open()); title: the project's file
//       name, else the level's name, else "" (shown "untitled"); project_path: "" before the first
//       save; unsaved: history.revision() != saved_revision
//   projects_root: <library>/projects (where File > Open starts)
//   recent_projects: string[] (File > Recent projects, Prefs::recent_projects)
//   history: { can_undo, can_redo, undo_label, redo_label, revision }
//   selection: { count, primary, hash, group, level, topology_editable }
//       group: group_of(primary), 0 none; level: level_of_node(primary); topology_editable(primary)
//   clipboard_empty: nothing copied yet
//   tool: "select" | "place" | "topology" | "terrain" | "collision"
//   gizmo: { mode: "translate" | "rotate" | "scale", local, snap: { enabled, translate, rotate_deg, scale } }
//   brush: { magnet, size } (the terrain brush: M and [ ] change them)
//   view: { game_view, game_view_ready, game_view_error, effects, actors, ocean, fog, navmesh,
//           labels, grid, textures, select_terrain } (the View menu; game_view_error: why it is not ready)
//   story: { available, ... } (the story bar: the game's data has a story and the document has nodes)
//   loading: a level is being read or added, or the asset store is busy (status bar "loading...")
//   remote_clients: assistants connected through the MCP server
//   library_empty: the game was not extracted
//   language: the engine's (set_language)
//   log: { level: "info" | "warn" | "error", text }[]: the log's last lines (then jle:log, one line each)
//   jobs: { id, label, done, total }[]: the work going on (then jle:progress, the same object; a job
//       is over when done >= total or { finished: true })

import { call, onEngine } from "./rpc";

export type LogLine = { level: "info" | "warn" | "error"; text: string };
export type Job = {
  id: string;
  label: string;
  done: number;
  total: number;
  finished?: boolean;
};
export type Tool = "select" | "place" | "topology" | "terrain" | "collision";
export type GizmoMode = "translate" | "rotate" | "scale";

const defaults = () => ({
  // level: the level opened (settings.level); replaces: the game's level it replaces (Level > Replace)
  document: {
    open: false,
    title: "",
    project_path: "",
    unsaved: false,
    level: "",
    replaces: "",
  },
  projects_root: "",
  recent_projects: [] as string[],
  history: {
    can_undo: false,
    can_redo: false,
    undo_label: "",
    redo_label: "",
    revision: 0,
  },
  selection: {
    count: 0,
    primary: 0,
    hash: 0,
    group: 0,
    level: 0,
    topology_editable: false,
  },
  clipboard_empty: true,
  tool: "select" as Tool,
  gizmo: {
    mode: "translate" as GizmoMode,
    local: false,
    snap: { enabled: false, translate: 0.5, rotate_deg: 15, scale: 0.1 },
  },
  brush: { magnet: false, size: 4 } as {
    magnet: boolean;
    size: number;
    [k: string]: any;
  },
  view: {
    game_view: false,
    game_view_ready: false,
    game_view_error: "",
    effects: true,
    actors: true,
    ocean: true,
    fog: true,
    navmesh: true,
    collision: false,
    labels: true,
    grid: true,
    textures: true,
    select_terrain: false,
  } as Record<string, any>,
  story: { available: false } as Record<string, any>,
  loading: false,
  remote_clients: 0,
  library_empty: false,
  language: "en",
  log: [] as LogLine[],
  jobs: [] as Job[],
});

export type EngineState = ReturnType<typeof defaults> & Record<string, any>;

// the engine's answer over the defaults, object by object (a field it does not give keeps its default)
function merge(base: any, over: any): any {
  if (
    !over ||
    typeof over !== "object" ||
    Array.isArray(over) ||
    !base ||
    typeof base !== "object" ||
    Array.isArray(base)
  )
    return over ?? base;
  const out = { ...base, ...over };
  for (const k in base) out[k] = merge(base[k], over[k]);
  return out;
}

export const engine = $state({
  state: defaults() as EngineState,
  ready: false, // get_state answered once
  log: [] as LogLine[],
  jobs: {} as Record<string, Job>,
});

const jobOver = (j: Job) => j.finished || (j.total > 0 && j.done >= j.total);

/** Follows the engine (LevelEditor mounts it); returns what stops it. */
export function startEngine(): () => void {
  let queued = false;
  let busy = false;
  const refresh = async () => {
    if (busy) return void requestAnimationFrame(refresh); // the change is read after the call going on
    queued = false;
    busy = true;
    const s = await call<Partial<EngineState>>("get_state", {}, true);
    busy = false;
    if (!s) return;
    engine.state = merge(defaults(), s);
    if (!engine.ready && s.log) engine.log = s.log;
    if (s.jobs)
      engine.jobs = Object.fromEntries(
        s.jobs.filter((j) => !jobOver(j)).map((j) => [j.id, j]),
      );
    engine.ready = true;
  };
  const schedule = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(refresh);
  };
  schedule();
  const offs = [
    onEngine<{ state?: boolean; selection?: boolean } | null>(
      "jle:changed",
      (p) => {
        if (!p || p.state || p.selection) schedule();
      },
    ),
    onEngine<LogLine>("jle:log", (l) => {
      engine.log.push(l);
      if (engine.log.length > 2000)
        engine.log.splice(0, engine.log.length - 2000);
    }),
    onEngine<Job>("jle:progress", (j) => {
      if (jobOver(j)) delete engine.jobs[j.id];
      else engine.jobs[j.id] = j;
    }),
  ];
  return () => offs.forEach((off) => off());
}
