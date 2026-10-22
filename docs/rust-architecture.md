# Jak Level Editor: architecture

The editor (version 1, *OpenGOAL Level Editor*, was C++/ImGui/OpenGL) is written in Rust with the
same approach as the [OpenGOAL Launcher](https://github.com/open-goal/launcher): **Tauri 2** for
the native side, **Svelte 5 + Tailwind CSS 4 + Flowbite** for the interface. The editor is a
self-contained module: it runs as its own application (*Jak Level Editor*) and plugs into the
launcher (a *Level Editor* button) with two lines of Rust and one Svelte import.

## 1. Layout

```text
Cargo.toml                        the Rust workspace: crates/core, crates/tauri-plugin, crates/mcp, src-tauri
crates/
  core/                           jak-level-editor-core (lib jle_core): the engine, no Tauri
    src/math.rs                   vectors, quaternions, matrices, boxes, rays
    src/history.rs, log.rs,       the undo history, the log, the translations (printf arguments),
      i18n.rs, prefs.rs           the preferences
    src/scene/                    the document: nodes, meshes, materials, the BVH and picking, holes
                                  (gaps), the ground, the terrain, topology, shapes, the story,
                                  assemblies, the light of what is placed (relight), particles,
                                  skeletons, a level's style
    src/io/                       glTF, the library and its index, the catalog, prefabs and the
                                  game's prefab recipes, the game's data, the texture palette,
                                  behaviors, the export for the game and its nav meshes, Jak II's
                                  structures, the level package's installer (level_package.rs)
    src/game/                     the game view's chunks (view.rs), the game's mood: hour, weather,
                                  fog, lights (mood.rs)
    src/navmesh.rs, navmesh/      nav mesh generation (rerecast, a Rust port of Recast; recast.rs:
                                  Recast's own steps where rerecast differs)
    src/runtime.rs                the game runtimes: an OpenGOAL the editor drives, the standalone
                                  app's (section 6)
    src/editor/                   the application without its interface: the Editor (mod.rs), the
                                  dispatch of `call` (api.rs), get_state (state.rs), levels,
                                  project, selection, groups, placement, the tools (terrain_tool,
                                  topology_tool, preview), inspector, panels, catalog, thumbnails,
                                  export, extraction, effects, ocean, surfaces, behaviors, the
                                  jle:// encoders (protocol.rs), the remote control and its tools
                                  (remote.rs, remote_tools.rs)
    tests/                        tests of the whole engine
  tauri-plugin/                   tauri-plugin-jak-level-editor: the engine inside a Tauri app (its
                                  command, its events, the jle:// protocol, the remote control, the
                                  command line)
  mcp/                            jak-level-editor-mcp: the MCP server (stdio) for AI assistants
ui/                               the interface module: <LevelEditor/> and everything under it
  LevelEditor.svelte              the root component the standalone app and the launcher mount
  lib/                            rpc.ts (calls to the engine, jleUrl), engine.svelte.ts (its state
                                  and events), i18n.ts, actions, shortcuts, windows
  render/                         the WebGL2 viewport: renderer, camera and controls, the game view,
                                  the sky, particles, the gizmo, labels, overlays, the tools (select,
                                  place, terrain, topology, preview), the panels' pictures; dev/: the
                                  viewport alone on a mock engine (http://localhost:3001/ui/render/dev/
                                  with `npx vite`)
  components/                     frame/ (title bar, menus, toolbar, story bar), panels/, inspector/,
                                  windows/, guide/, common/
src/, index.html, package.json    the standalone app's frontend (mounts ui/LevelEditor.svelte), its
                                  fonts
src-tauri/                        the standalone app (binary jak-level-editor): Tauri + the plugin,
                                  tauri.conf.json, its capability, its icons
extractor/                        ogle-extract (C++, from OpenGOAL's decompiler), built by the root
                                  CMakeLists.txt (nothing else is C++)
data/, lang/                      the game's tables, the interface's texts (en, fr)
tools/                            Python: data/ made again from the game's code, the documentation's
                                  pictures
```

`extractor/` stays a native tool the engine runs (`crates/core/src/editor/extract.rs`), as the
launcher runs OpenGOAL's own extractor, compiler and game: it is OpenGOAL's decompiler, not the
editor. `cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release && cmake --build build` writes
`build/bin/ogle-extract(.exe)` and its tables `build/bin/ogle-extract-data/`, where the engine
looks for it (after the program's own folder and its resources; `$JLE_EXTRACTOR` last).

## 2. Who does what

- **The engine (Rust, `jle_core`)** owns the document and everything computed from it: nodes,
  meshes, materials, textures, history, selection, tools, picking (BVH ray casts), the catalog,
  prefabs, levels, story, nav meshes, holes, export, extraction, the remote tools of the MCP
  server. It never draws. It is tested with `cargo test -p jak-level-editor-core`.
- **The interface (Svelte + TypeScript)** shows the engine's state and sends the user's actions.
  Its viewport draws with WebGL2 what the engine gives it (the meshes, their world matrices, the
  materials and textures, the game's palettes), and handles the mouse: the camera, the gizmo,
  the box, the previews while a button is held. What it decides goes to the engine as one call
  (a move: the matrices at the end of the drag, one undo step).

## 3. The engine's API

One Tauri command carries every call, so that the API is the engine's, testable without Tauri:

```ts
invoke("plugin:jak-level-editor|call", { name: "open_level", args: { name: "ctyinda" } })
```

```rust
// crates/core/src/editor/api.rs
impl Editor {
    pub fn call(&mut self, name: &str, args: &serde_json::Value) -> Result<serde_json::Value, String>;
}
```

`call` dispatches on `name` (snake_case, the names of the C++ editor's functions). Errors
are translation keys with their arguments when the interface shows them (`"levels.no_library"`), or
English text. `get_state` answers what the interface shows besides the scene (the document, the
selection, the tool, the history's labels, the jobs, the log's last lines, the view's settings).

The engine tells the interface something changed with **one event**, `jle:changed`, whose
payload says what (`{"revision": 12, "scene": true, "selection": true, "state": true}`; also
`document`, `materials`, `textures`, `gameview`): `call` compares the engine before and after
(the history's revision, the scene, the selection, the state the interface shows) and emits it
once, the queries (`get_state`, `node`, `pick`...) never. Besides it,
`jle:log` (a line of the log), `jle:progress` (a job) and `jle:request` (the engine needs the
interface: a capture of the view for the MCP server, a camera move; the interface answers with
`call("frontend_reply", {id, ...})`).

## 4. The `jle://` protocol (binary data for the viewport)

The plugin serves the viewport's data on a URI scheme (`jle://localhost/<path>` on Linux/macOS,
`http://jle.localhost/<path>` on Windows; `ui/lib/rpc.ts` has `jleUrl(path)`). Little-endian.

| Path | Answer |
|---|---|
| `scene?full=1` | the render state: every node (else only what changed since the last answer, and the ids removed) |
| `mesh/<uid>` | a mesh's primitives: positions, normals, uvs, colors, indices, material |
| `texture/<index>` | a texture of the document: `u32 width, u32 height`, RGBA bytes |
| `materials` | JSON: the document's materials (texture, color, alpha, blend, cutoff, two sides) |
| `thumb/<name>` | a thumbnail the interface drew before (PNG of <preferences>/thumbs), 404 until then |
| `preview/<kind>?...` | what a thumbnail draws: a mesh with its own materials and textures (`JLEP`) |
| `palette?game=&background=&name=` | the image of a texture of the games, as `texture/<index>` |
| `asset?path=` | a file of the library (a model's .glb for a preview, a sprite) |

`scene` (format `JLES` 1): `u32 magic "JLES"`, `u32 version`, `u32 full (1) or delta (0)`,
`u32 count`, then per node `u32 id, u32 parent, u8 kind, u8 flags, u16 reserved, u32 mesh uid
(low 32 bits, 0 none), u32 mesh version, f32 world[16]` (column-major), then `u32 removed`, then
the ids removed. Flags: 1 visible (with its parents, and the story), 2 locked, 4 reference level,
8 under an actor (its model), 16 has game lighting, 32 selected, 64 water. For an actor, the u16
is the low 16 bits of the FNV-1a hash of its etype (the color of its marker).

`mesh/<uid>` (format `JLEM` 1): `u32 magic "JLEM"`, `u32 version`, `u32 prims`, then per primitive
`u32 vertices, u32 indices, i32 material, u32 flags (1 uv, 2 colors, 4 backdrop: fogged by
distance)` followed by its arrays `f32 pos[3n]`, `f32 nrm[3n]`, `f32 uv[2n]` (if any),
`u8 col[4n]` (if any), `u32 idx[m]`.

`materials`: `{"materials": [...], "textures": [{"version": n}, ...]}`, each material `{name,
texture, base_color, alpha: "opaque"|"mask"|"blend", blend: ""|"add"|"add_alpha"|"subtract"|"half",
cutoff, double_sided, translucent, water}`.

### The panels' pictures

The catalog's and the prefabs' pictures are drawn by the interface (`ui/render/thumbnails.ts`, 128
pixels, the C++ framing: a 3/4 view) in a WebGL2 context of its own, from `preview/<kind>`:
`model?path=` (a model or a prefab of the library), `part?path=<a level's background>&node=<the node
whose mesh it is>`, `level?path=<a level's background>` (all of it, from above, its textures
shrunk), `icon?etype=&family=` (an object of the game without a model) or `shape?preset=`. The
effects are drawn from the game's particles (`particle_thumb.ts`). A picture drawn is kept with
`call("thumb_store", {name, png})` (base64), `name` being the FNV-1a 64 of its key in hexadecimal
(`m:<path>@<stamp>`, `p:<path>#<node>@<stamp>`, `b:<path>@<stamp>` a level,
`i1:<etype>#<family>`, `e2:<group>@0`, `s:<preset>`), and asked first as `thumb/<name>`.

`preview/<kind>` (format `JLEP` 1): `u32 magic "JLEP"`, `u32 version`, `u32 json bytes`, the JSON
(padded with spaces to 4 bytes: `{materials, textures: [{version}], bounds: {lo, hi}, draws:
[{mesh, world}]}`, the materials as `materials` gives them, `bounds` what the picture frames,
`draws` where each mesh is drawn), `u32 meshes`, then each `u32 bytes` and the mesh (`JLEM`, its
materials indexing the package's), `u32 textures`, then each texture as `texture/<index>`: `u32
width, u32 height`, RGBA bytes.

`palette?...` waits while the engine reads the texture (a level's file once for all the textures
asked of it); 404 when that level has no such texture.

### Game view

View > Game rendering (G) draws the decor as the game does (`ui/render/game_view.ts`, the game's
TFragment shader): in chunks, the pieces sharing a palette (a tree of an extracted level, or the
new geometry) in a cell of 128 m, made by `jle_core::game::view::GameViewChunks` from the document
and made again only where an edit touched them. The mood (the palettes' weights, the fog, the sky)
comes from the call `game_mood`.

| Path | Answer |
|---|---|
| `gameview` | `GameViewChunks::update(&scene)`, then `encode_chunk_list()`: the chunks |
| `gameview/chunk/<id>` | `encode_chunk(id)`: a chunk's vertices, indices and draws |
| `gameview/palette/<key>` | `encode_palette(key)`: a palette the chunks use |

`gameview` (format `JLGL` 1): `u32 magic "JLGL"`, `u32 version`, `u32 count`, then per chunk (40
bytes) `u32 id, u32 version, u32 palette, u32 triangles, f32 lo[3], f32 hi[3]` (its box, meters).
A chunk's version and a palette's key are new each time it is made (unique for the life of the
program): the interface keeps chunks by (id, version) and palettes by key.

`gameview/chunk/<id>` (format `JLGC` 1): `u32 magic "JLGC"`, `u32 version`, `u32 id, u32 chunk
version, u32 palette, u32 vertices, u32 indices, u32 draws, f32 lo[3], f32 hi[3]` (56 bytes),
then per draw (16 bytes) `u32 first, u32 count` (into the indices), `i32 texture` (the document's,
-1 a white one), `u8 flags` (1 translucent tree, 2 depth write, 4 alpha test, 8 a texture of the
editor's: its alpha halved), `u8 aref` (alpha test: what is under aref / 127 is not drawn), `u8
blend` (0 none, 1 alpha, 2 add alpha, 3 add, 4 subtract, 5 half: the PS2's), `u8 0`; then per
vertex (24 bytes) `f32 x, y, z` (game units: 4096 per meter), `f32 s, t`, `u16 color` (in the
palette), `u16 0`; then `u32 idx[indices]`. The normal tree's draws come first.

`gameview/palette/<key>` (format `JLGP` 1): `u32 magic "JLGP"`, `u32 version`, `u32 colors`, then
32 bytes per color: its 8 palettes' RGBA (128 is 1.0), palette k at k * 4.

## 5. Threads

The plugin keeps the engine in an `Arc<Mutex<Editor>>`. Long work (a level read, the catalog's
index, the game's prefabs, a nav mesh, an export, the extraction) runs on a worker thread that
holds what it reads (`Arc`s of the library) and applies its result under the lock when done, then
emits `jle:progress` while it goes and `jle:changed` when done (`Editor::run_job`). The remote
control (MCP) listens on `127.0.0.1:47821` and runs its tools under the same lock: what an
assistant does shows at once and can be undone; a tool that waits (a level loading, a job, a
picture of the view) is asked again every few milliseconds, the lock let go in between. A timer of
the plugin calls `Editor::tick` every 50 ms (the game view's hour passing, a storm's lightning).

## 6. What the host gives the editor

The application that hosts the editor (its own app, the OpenGOAL Launcher) gives it a `Host`
(`crates/core/src/host.rs`, `tauri_plugin_jak_level_editor::Builder::host`). The editor never asks
which application hosts it: it uses what it is given and hides what it is not.

| Field | Standalone app | OpenGOAL Launcher |
|---|---|---|
| `default_library` | `Documents/Jak Level Editor` | `<install>/jak-level-editor` |
| `extract_source` | none (the user's disc) | `<install>/active/jak3/data/iso_data/jak3`: the disc it already extracted |
| `extractor`, `extractor_provider` | ogle-extract installed with the app | downloaded on demand from the editor's release of its version |
| `runtime` | `StandaloneRuntime`: the jak-project checkout chosen in the settings (with the builder's fixes), else the OpenGOAL Launcher's game for the editor when the launcher's active version has the builder | the launcher's isolated game `<install>/features/jak3/level_edited/`, run with the programs of the launcher's active tooling version (`<install>/versions/official/<version>/`, from the release of `whozghiar/jak-project`; nothing else is downloaded) |

**The library** (`crates/core/src/editor/library.rs`): until the user chose one (the preference
`library_chosen`; preferences of before it whose library holds an extracted game count as chosen),
the interface shows the first-run assistant (`library_setup`, `choose_library {path}`) offering
`default_library`; File > Settings changes it later (another folder is used as it is: nothing is
moved or deleted). `default_library` falls back to `library/` of the editor's repository only when
the program runs from it (a development build), never to the program's folder. The Extract
window's disc is `extract_source` while the user has chosen none; ogle-extract is `extractor`
first, and when it is missing the window offers `extractor_provider`'s download (`extract_fetch`,
a job: `jle:progress` "extract_fetch").

**The game runtime** (`GameRuntime`): `install_level` (copies a level exported for the game into the
game's sources, registers it in `game.gp` and `level-info.gc`, builds the game), `remove_level`,
`launch` (starts the game on the level's continue point), and `play` (both; a host may hand it to
its own interface: the launcher runs it as one of its jobs, with its progress bar). The export
writes `jle-level.json` (`LevelManifest`) next to its README: what the README tells a person to do,
for a runtime to do it. Without a runtime the editor has no Play button: it exports and points to
the README.

**The level package** (`crates/core/src/io/level_package.rs`, used by every runtime, the launcher's
too): `apply(data_dir, &LevelPackage)` and `remove(data_dir, game, name)` on an OpenGOAL data folder
(the one holding `goal_src/<game>` and `custom_assets/<game>`: a jak-project checkout's root, an
OpenGOAL distribution's `data/`). `apply` copies the export's files, adds its `game.gp` lines (next
to the test-zone's) and its definition to `engine/level/level-info.gc` (with an `:index` no other
level has), or, for a level replacing one of the game's, changes that level's `:nickname`; each
inside `;; jak-level-editor: begin <name>` / `;; jak-level-editor: end <name>` lines, a line it
replaces kept there as `;; jak-level-editor: was <line>`. Applying again changes nothing; another
version of the level replaces its blocks in place. Each file edited is backed up once
(`<file>.jle-backup`, deleted when the file is back as it was), and `<data_dir>/jle-installed.json`
records what is installed (name, game, replaces, version: a hash of its files, files, continue
point, index), so that `remove` takes out exactly what `apply` put. One level at a time may replace
a given level. The manifest is checked first (names, files only under the level's folders, only the
`game.gp` lines an export writes).

**The runtimes** (`crates/core/src/runtime.rs`): `OpenGoal` is an OpenGOAL the editor drives, and a
`GameRuntime`: `OpenGoal::checkout(root, bin, game)` (a jak-project checkout and its build's
programs, its settings in `out/jle-settings`, the game's files from its `iso_data/jak3`, else the
ones the launcher extracted) or `OpenGoal::distribution(kind, folder, iso_data)` (`extractor`, `gk`,
`goalc` and `data/`, as the launcher installs them, its settings in `<folder>/_settings`), which
`with_tooling(Tooling {version, dir})` runs with the programs of another distribution.
`launcher_level_edited(install, version, game)` is the launcher's isolated game: its own `data/`,
`_settings/` (its saves) and `packages/` in `<install>/features/<game>/level_edited/`, the programs
of the launcher's tooling version `<install>/versions/official/<version>/`, the disc the launcher
extracted. `prepare()` makes its `data/` again from that version's `data/` whenever it was made from
another version (`jle-runtime.json`: `{version, decompiled}`; `_settings/` is kept), then installs
again the levels whose copies `apply_level` keeps in `packages/<name>/` (`remove_level_files`
deletes the copy), and leaves it to decompile again (`decompile_command`: `extractor <iso_data>
--folder --decompile --game jak3 --proj-path <data>`). It plays only when the version's `data/`
has the builder (`runtime.launcher_version` otherwise) and the launcher extracted the game
(`runtime.no_game_files`). Its `install_level`, `remove_level` and `launch` prepare, decompile and
build it first when needed: the standalone app and the launcher use the same code. Both
build with `extractor <iso_data> --folder --compile --game jak3 --proj-path <data>`
(`compile_command`: goalc's `(mi)`, what the launcher runs for mods; `goalc --cmd` ignores
`--iso-path`). Every program gets `--proj-path`: without it they take the project from the last
"jak-project" in their own path. Jak 3's kernel cannot start at a continue point, so `launch`
writes `goal_src/user/jle/startup-jak3.gc` (`startup_code`: `(make-group "all-code")`, `(lt)`,
then, run once it listens, `(start 'play (get-continue-by-name *game-info* "<continue>"))`), starts
`gk -v --game jak3 --proj-path <data> --config-path <settings> -- -boot -fakeiso -debug`
(`launch_command`; `-debug` opens its listener), then goalc's REPL as that user on free ports with
its stdin held open (`repl_command`: a REPL that ends resets the game), and kills goalc once the
game's output says `Displaying level <name>` (killed, it leaves the game as it is); a second launch
stops the game started before (`stop_game`), as a build does. `run_logged` runs a tool, its lines
and the make system's `[ 42%]` as `HostEvent`s. The launcher can use the same pieces. The builder a data folder has is its
`custom_assets/<game>/jle-builder.json` (`{"builder": "jle-1"}`, written by the editor's
jak-project branch), or, for a checkout of before it, the fixes' code. `StandaloneRuntime`, the
standalone app's, reads the preferences at each call: the jak-project checkout chosen in File >
Settings (`runtime_folder`, its programs in `runtime_bin` or its build's `out/build/Release/bin`),
else the OpenGOAL Launcher's game for the editor (`installationDir` and `activeVersion` of
`%APPDATA%/OpenGOAL-Launcher/settings.json`, then `features/jak3/level_edited/` with that version's
programs, when its data has the builder). `GameRuntime::settings` (a default method answering null) is what the Settings window
shows besides `info`. `info` is asked at each `get_state`: a runtime keeps it cheap.

**Play** (`crates/core/src/editor/runtime.rs`): `get_state.runtime` is the runtime's `RuntimeInfo`
(null without one). `play_level {options?, wait?}` exports the document with the Export window's
options into `<library>/exports/<project>/`, then calls `play` on the job `play` (`jle:progress` at
each step); `play_state` answers `{busy, action, name, steps: [{key, args}], done, total, log,
outcome: "started" | "handed_over" | null, error, seconds, generation}`; `runtime_levels` (the
levels installed), `runtime_remove {name, wait?}`, `runtime_launch {name, wait?}`,
`runtime_settings` (`{info, settings}`). The interface shows the toolbar's Play, Level > Play in the
game and the Play window only when `runtime.available`; without it the Export window points to the
README and to File > Settings.

**The level builder**: OpenGOAL's builder needs fixes for the editor's exports (a level without
regions, nav meshes, flat collision fragments, translucent surfaces, collision on both sides per
mesh, the list of everything visible). A runtime declares the builder it has; the manifest says
`"builder": "jle-1"` when the level needs those fixes.

## 7. In the launcher

The launcher keeps the editor as a git submodule in `level-editor/`, and:

```toml
# src-tauri/Cargo.toml
tauri-plugin-jak-level-editor = { path = "../level-editor/crates/tauri-plugin" }
```

```rust
// src-tauri/src/main.rs
.plugin(tauri_plugin_jak_level_editor::init())
```

```svelte
<!-- a route of the launcher -->
<script> import LevelEditor from "$level-editor/LevelEditor.svelte"; </script>
<LevelEditor embedded locale="fr-FR" />
```

with the capability `jak-level-editor:default` (the plugin's one command, `call`) and the alias
`$level-editor` to `level-editor/ui` (vite's `resolve.alias` and tsconfig's `paths`, as this
repository's own). Its sidebar's *Level Editor* button opens the editor in a window of its own.
`embedded` leaves out the editor's own title bar (the launcher's window has its frame); `locale`
is the launcher's language. `init()` takes no command line: the library and the language are the
preferences', and the remote control listens on its default port. The editor's translations and
tables go in the launcher's resources under `jak-level-editor/` (`lang/`, `data/`), apart from
its own.

The standalone app gives the plugin its command line instead:
`tauri_plugin_jak_level_editor::Builder::new().args(std::env::args()).build()`, which reads
`[project.glb] --library <folder> --lang <code> --remote-port <port> --hidden --open-level <name>`
(`--hidden`: no window, the preferences left unchanged; `--open-level`: a level of the library
opened at the start). The plugin takes the translations and the game's tables from the app's
resources (`lang/`, `data/`: `bundle.resources` of src-tauri/tauri.conf.json; a build copies them
into `target/<profile>/`), else the repository's. ogle-extract is found next to the executable,
in the resources or in the repository's `build/bin/`: to ship it with the app, add
`"../build/bin/ogle-extract.exe": "ogle-extract.exe"` and `"../build/bin/ogle-extract-data/":
"ogle-extract-data/"` to `bundle.resources` once it is built (a missing path there fails
`tauri build`).

## 8. Conventions

- The Rust code follows the C++ editor's file for file (`src/scene/terrain.cpp` ->
  `crates/core/src/scene/terrain.rs`), with the same names in snake_case, the same comments (they
  explain the game) and the same behavior. The comments still name the C++ files they come from:
  read them with `git show <commit>:src/...`, `<commit>` being the one before they were removed
  (`git log -1 --format=%H --diff-filter=D -- src/main.cpp`, then its parent: `<hash>^`).
- Shared data: `Arc<Mesh>` (copy on write with `Arc::make_mut`), meshes' derived data (bounds,
  BVH) computed lazily behind `OnceLock`/`RwLock`.
- JSON: `serde_json::Value` for what the C++ kept in `nlohmann::json` (extras, game data).
- Interface texts: `lang/<code>.json`, the same keys as the C++ editor; printf arguments (`%s`,
  `%d`, `%zu`, `%.1f`) are formatted by `ui/lib/i18n.ts` (interface: `t("key")`) and
  `jle_core::i18n` (log, MCP answers: `tr("key")`). A test checks every key of `en.json` in every
  language, with the same arguments.
- Tests: `cargo test -p jak-level-editor-core` (the C++ `--selftest`'s checks, as `#[cfg(test)]`
  modules and `crates/core/tests/`; those needing the extracted game skip themselves when
  `library/jak3/library.json` is missing), `cargo test -p jak-level-editor-mcp`, and for the
  interface `npx svelte-check --tsconfig ./tsconfig.json` and `npx vite build`.
- Formatting: `rustfmt.toml` (2 spaces), prettier for the interface (`npx yarn format`).
