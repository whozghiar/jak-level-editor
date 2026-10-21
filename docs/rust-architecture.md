# Jak Level Editor: architecture of the Rust version

The editor (formerly *OpenGOAL Level Editor*, C++/ImGui/OpenGL) is rewritten in Rust with the same
approach as the [OpenGOAL Launcher](https://github.com/open-goal/launcher): **Tauri 2** for the
native side, **Svelte 5 + Tailwind CSS 4 + Flowbite** for the interface. It is split so that the
editor is a self-contained module: it runs as its own application (*Jak Level Editor*) and plugs
into the launcher (a *Level Editor* button) with two lines of Rust and one Svelte import.

## 1. Layout

```text
Cargo.toml                        the Rust workspace
crates/
  core/                           jak-level-editor-core (lib jle_core): the engine, no Tauri
    src/math.rs ...               vectors, quaternions, matrices, boxes, rays (core/math.h)
    src/scene/ ...                the document: nodes, meshes, materials, BVH, picking, gaps,
                                  terrain, topology, shapes, story, relight... (src/scene/*)
    src/io/ ...                   glTF, the library, the catalog, prefabs, the game's data, the
                                  export for the game and its nav meshes (src/io/*)
    src/game/ ...                 the game's mood: hour, weather, fog, lights (src/game_view/*)
    src/navmesh.rs                nav mesh generation (rerecast, a Rust port of Recast)
    src/editor/ ...               the application without its interface: the document open,
                                  the selection, the history, the tools, the jobs, the API
                                  (src/app/actions.cpp, groups.cpp, extract.cpp...)
  tauri-plugin/                   tauri-plugin-jak-level-editor: the engine inside a Tauri app
                                  (commands, events, the jle:// protocol, the remote control)
  mcp/                            jak-level-editor-mcp: the MCP server (stdio) for AI assistants
ui/                               the interface module: <LevelEditor/> and everything under it
  LevelEditor.svelte              the root component the standalone app and the launcher mount
  lib/                            rpc (calls to the engine), i18n, stores
  render/                         the WebGL2 viewport: renderer, game view, gizmo, overlays
  components/                     menus, toolbar, panels, inspector, windows
src/, index.html, package.json    the standalone app's frontend (mounts ui/LevelEditor.svelte)
src-tauri/                        the standalone app (binary jak-level-editor): Tauri + the plugin
extractor/                        ogle-extract (C++, from OpenGOAL's decompiler): unchanged
data/, lang/                      the game's tables, the interface's texts (en, fr)
```

`extractor/` stays a native tool the engine runs, as the launcher runs OpenGOAL's own extractor,
compiler and game: it is OpenGOAL's decompiler, not the editor.

## 2. Who does what

- **The engine (Rust, `jle_core`)** owns the document and everything computed from it: nodes,
  meshes, materials, textures, history, selection, tools, picking (BVH ray casts), the catalog,
  prefabs, levels, story, nav meshes, holes, export, extraction, the remote tools of the MCP
  server. It never draws. It is tested with `cargo test -p jak-level-editor-core` (the C++
  `--selftest`).
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

`call` dispatches on `name` (snake_case, the names of the C++ functions where there is one). Errors
are translation keys with their arguments when the interface shows them (`"error.no_library"`), or
English text. `get_state` answers what the interface shows besides the scene (the document, the
selection, the tool, the history's labels, the jobs, the log's last lines, the view's settings).

The engine tells the interface something changed with **one event**, `jle:changed`, whose
payload says what (`{"revision": 12, "scene": true, "selection": true, "state": true}`), plus
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
| `tod/<node id>` | the node's game lighting: its palette (8 RGBA per color) and the color of each vertex |
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
emits `jle:changed`. The remote control (MCP) listens on `127.0.0.1:47821` and runs its tools
under the same lock: what an assistant does shows at once and can be undone.

## 6. In the launcher

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
<LevelEditor embedded />
```

with the capability `jak-level-editor:default` and the alias `$level-editor` to
`level-editor/ui`. Its sidebar's *Level Editor* button opens the editor in a window of its own.

The standalone app gives the plugin its command line instead:
`tauri_plugin_jak_level_editor::Builder::new().args(std::env::args()).build()`, which reads
`[project.glb] --library <folder> --lang <code> --remote-port <port> --hidden --open-level <name>`
(`--hidden`: no window, the preferences left unchanged; `--open-level`: a level of the library
opened at the start). The plugin takes the translations and the game's tables from the app's
resources (`lang/`, `data/`: `bundle.resources` of src-tauri/tauri.conf.json), else next to the
executable, else the repository's. ogle-extract is found next to the executable, in the resources
or in the repository's `build/bin/`: to ship it with the app, add `"../build/bin/ogle-extract.exe":
"ogle-extract.exe"` and `"../build/bin/ogle-extract-data/": "ogle-extract-data/"` to
`bundle.resources` once it is built (a missing path there fails `tauri build`).

## 7. Porting conventions

- The Rust code follows the C++ one file for file (`src/scene/terrain.cpp` -> `crates/core/src/scene/terrain.rs`),
  with the same names in snake_case, the same comments (the C++ comments explain the game: keep
  them), and the same behavior: the editor must not lose a feature.
- Shared data: `Arc<Mesh>` (copy on write with `Arc::make_mut`), meshes' derived data (bounds,
  BVH) computed lazily behind `OnceLock`/`RwLock`.
- JSON: `serde_json::Value` where the C++ uses `nlohmann::json` (extras, game data).
- Interface texts: `lang/<code>.json`, the same keys as the C++ editor; printf arguments (`%s`,
  `%d`, `%zu`, `%.1f`) are formatted by `ui/lib/i18n.ts` (interface) and `jle_core::i18n` (log,
  MCP answers).
- The C++ editor's sources stay on `main` (and in this branch's history): `git show main:src/...`.
