# Technical reference

The details behind the [README](../README.md): the command line, the files the editor reads and
writes, the game data, what the export does, and what the editor does not do (yet).

## Contents

1. [Command line](#1-command-line)
2. [The extraction](#2-the-extraction)
3. [Files](#3-files)
4. [Game data](#4-game-data)
5. [The game rendering](#5-the-game-rendering)
6. [The export for the game](#6-the-export-for-the-game)
7. [The remote control and the MCP server](#7-the-remote-control-and-the-mcp-server)
8. [Known limitations](#8-known-limitations)
9. [Repository layout](#9-repository-layout)

## 1. Command line

```text
jak-level-editor [project.glb] [--library <folder>] [--lang <code>] [--remote-port <port>] [--hidden]
                 [--open-level <name>]
jak-level-editor-mcp [--remote-port <port>]
```

- `project.glb` is opened at the start; `--open-level` opens a level of the library instead
  (`--open-level ctyinda`).
- `--library` chooses the library folder (instead of the one chosen at the first start, else the
  host's default: `Documents/Jak Level Editor`, `library/` of the repository when the program runs
  from it; no first-run assistant then), `--lang` the language, for one session.
- `--remote-port` starts the editor's remote control on that port for the session (what the MCP
  server does when it starts the editor; without it, the remote control listens on 47821 only when
  File > Settings lets an assistant control the editor; `0`: as the preferences say);
  `--hidden` runs the editor without a window and without touching the preferences (scripts).
- `JLE_PREFS_DIR` (environment) puts the preferences in another folder (tests with fresh ones; for
  the MCP server, the folder of the editor's `remote.json`).
- `jak-level-editor-mcp` is the MCP server ([7](#7-the-remote-control-and-the-mcp-server)).
- Opened from the OpenGOAL Launcher, the editor has no command line: the library and the language
  are the preferences'.
- The C++ editor's other modes have their equivalents: `--selftest` is
  `cargo test -p jak-level-editor-core`, `--mcp` is `jak-level-editor-mcp`, and the pictures of
  `--screenshot` are taken through the remote control and the web view's DevTools
  ([captures](captures/README.md)).
- The preferences, the panels' pictures (`thumbs/`), what the catalog read of the levels
  (`index/`) and, while the remote control listens, its `remote.json` are in
  `%APPDATA%/OpenGOAL/jak-level-editor` on Windows (`~/.local/share/OpenGOAL/jak-level-editor` on
  Linux, `~/Library/Application Support/...` on macOS); the OpenGOAL Launcher's editor keeps its own
  in `<launcher's config folder>/jak-level-editor`. While the standalone app's folder has no
  preferences, the C++ editor's (`OpenGOAL/open-goal-level-editor`) are read, and saved into the
  new folder the next time they change.
- The translations (`lang/`) and the game's tables (`data/`) are read from the application's
  resources (`bundle.resources` of `src-tauri/tauri.conf.json`; a build copies them next to the
  program in `target/<profile>/`: build again after changing them), else from the repository.

## 2. The extraction

```text
ogle-extract <game.iso | disc folder> <output folder> [--levels CIA,CIB] [--textures]
ogle-extract --detect <game.iso | folder>   # game, version and serial (json)
ogle-extract --list <game.iso | folder>     # the levels of the disc (json)
```

An `.iso` is read in place: only the files read (the `DGO` and `CGO` folders, about 400 MB) are
copied to a work folder of the destination, deleted when done. Measured with Jak 3 NTSC
(SCUS-97330) on a 16-core processor and an SSD: 272 levels in 1 minute 10 to 1 minute 30, 3.7 GB.
[`extractor/README.md`](../extractor/README.md) has the details.

ogle-extract is a C++ program of its own, built with CMake from the repository's root (which
builds nothing else):

```bash
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build      # build/bin/ogle-extract(.exe) and its tables, build/bin/ogle-extract-data/
```

The editor runs the first it finds: the one its host gives (the OpenGOAL Launcher downloads it on
demand: **Download ogle-extract** in the Extract window), next to its own program, in its
resources, in `build/bin/` of the repository it was built from, else the file `$JLE_EXTRACTOR` names; its tables
(`ogle-extract-data/`) next to it. The installers ship it when they are built with
`src-tauri/tauri.release.conf.json`, which adds `build-release/bin/` (ogle-extract and its tables)
to the resources, as the releases do:

```bash
cmake -S . -B build-release -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=clang \
      -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded
cmake --build build-release
npx yarn tauri build --config src-tauri/tauri.release.conf.json
```

(`MultiThreaded`: the C runtime inside the program, which then runs on a Windows without the VC++
redistributable; the releases build the editor the same way, `RUSTFLAGS=-C
target-feature=+crt-static`.) Without that file, `tauri build` makes installers without the
extractor (a build for working on the editor).

The releases (`.github/workflows/release.yaml`, made from a tag `vX.Y.Z`:
[RELEASING.md](../RELEASING.md)) hold, per platform (`windows-x64`, `linux-x64`):

| Asset | Content |
|---|---|
| `*_x64-setup.exe`, `*_x64_en-US.msi` | the Windows installers (NSIS, for the current user; MSI): the editor, `lang/`, `data/`, `ogle-extract.exe` and `ogle-extract-data/` in its folder |
| `*_amd64.deb`, `*_amd64.AppImage` | the Linux packages, the same resources |
| `jak-level-editor-portable-<platform>.zip` | at its root: `jak-level-editor(.exe)`, `lang/`, `data/`, `ogle-extract(.exe)`, `ogle-extract-data/` (the editor finds them next to itself) |
| `ogle-extract-<platform>.zip` | at its root: `ogle-extract(.exe)` and `ogle-extract-data/`; the OpenGOAL Launcher downloads `https://github.com/whozghiar/open-goal-level-builder/releases/download/v<version>/ogle-extract-<platform>.zip` |
| `jak-level-editor-mcp-<platform>.zip` | `jak-level-editor-mcp(.exe)`, the MCP server ([7](#7-the-remote-control-and-the-mcp-server)): it starts the editor found next to it |

`.github/workflows/build.yaml` checks every push: the engine's and the MCP server's tests, the
interface (types, Prettier on `ui/`, `src/` and the configuration files, build), ogle-extract and
the editor, on Windows and Linux. There is no macOS build: ogle-extract is compiled with `-mavx`
(x86 only) and the editor was never tried there.

```text
library/jak3/library.json                          the game, its version, the levels extracted
library/jak3/common/models/*.glb                   the models every level uses
library/jak3/common/sprites/<page>/<texture>.png   the textures of the particles
library/jak3/<level>/decor/<level>-background.glb  a level: decor, collision and actors
library/jak3/<level>/models/<model>-lodN.glb       the models of its actors
library/jak3/<level>/level.json                    what the folder holds (DGO, actors, models, regions)
library/jak2/...                                   Jak II's levels, the same way
library/prefabricated/*.glb                        the user's prefabs
library/prefabricated/game/*.glb                   the game's prefabs (game/jak2/: Jak II's)
library/projects/<name>/                           a saved project
library/exports/<project>/                         a level exported for the game
```

## 3. Files

### Project (`.glb`)

A regular glTF binary. Each node carries its kind in its extras (`og_kind`: `render`, `collision`,
`actor`, `group`, `navmesh`); instancing is kept (the copies of an element share its mesh).

| Extra | On | What it is |
|---|---|---|
| `og_level`, `og_companion`, `og_layer`, `og_reference` | groups | a level added, a level shown with it, a mission layer, the original a level replaces |
| `og_group` | root elements | the group they are in: `{id, name, prefab}` (a prefab placed: `prefab` true) |
| `og_navmesh` | nav meshes | `verts` (relative to the node), `polys` (3 or 4 corners), `adj` (the polygon across each edge, -1 a border) |
| `etype`, `aid`, `game_task`, `bsphere`, `lump`, `lump_types` | actors | the game's data of the actor; its model is a `render` child marked `og_actor_visual` |
| `og_behavior` | actors | what it does: `open` (approach, locked, always, when), `when`, `distance`, `loads`, `active`, `leave`, `present` (conditions: `{when: always|never|task|pressed|gone, arg}`) |
| `og_decor` | decor | its collision, ground material, effect on Jak, bounce, launch |
| `og_shape` | shapes | kind, size, texture repeat, steps, bend, corners, bumps |
| `og_import` | Jak II's | the game, level and part it comes from, its checks |
| `og_tod` | decor | the game's lighting: `palette` (8 RGBA per color) and `index` (per primitive, per vertex); `local`: lit like the level around it (`og_lit_here`) |
| `og_translucent`, `og_blend` | materials | drawn without its depth; the PS2 blend (`add`, `add_alpha`, `subtract`, `half`) |

The level the project comes from and the level it replaces are in `asset.extras.ogle.settings`.

### Prefab (`.glb`)

The same format, holding only the prefab's elements, around a pivot at the bottom center of their
bounds. Placed, its roots become a group named after it (the groups it was made of are forgotten).

### Nav meshes for the game (`.json`)

The `entity-nav-mesh` objects as they are in the game's memory, in the data blob format of
OpenGOAL's level tools (`words`, `pointers`, `types`, `symbols`, `roots`): the `nav_data` of a level
for a custom level builder. The polygon grid follows the game's own rule: the grids the editor makes
for the game's nav meshes are the game's, byte for byte.

### Extracted level

`<level>-background.glb` holds the level's shell (tfrag) in pieces (the large ones, the terrain,
marked `og_terrain`, with `og_measure`), its decor (a group per prototype whose instances share the
mesh), its collision (a node per surface: `collide_pat`) and its actors. Every piece keeps the
game's lighting in `og_tod`. `level.json` lists the actors by type with the data of one of each
(the catalog's templates) and the level's regions (`region_trees`).

### Ground and decor

When a level opens, its ground is locked (clicks and boxes go to what stands on it; **Select the
ground** reaches it). The shell's pieces are cut by texture, so a piece named `city-port-grnd-cobl`
holds every triangle of that cobblestone, the walls built with it included. A piece marked
`og_terrain` is ground when:

1. it is large and flat: 60 m across at least, half of its surface flat (`ground_piece`);
2. its texture is not a structure's: `roof`, `bridge`, `pillar`, `palace`, `tower-side`, `monitor`,
   `beam`, `pipe`, `brace` are never ground;
3. its texture is natural ground (`sand`, `grass`, `rock`, `dirt`, `mud`, `snow`, `beach`, `mount`,
   `dune`, `cliff`, `lava`, `ground`), or nothing lies under it: down rays from 64 points of its flat
   surfaces, whichever way their triangles face, meet another piece (not a backdrop) more than 1 m
   below for less than half of them. A roof, a ceiling, a bridge, a slab of debris lying on the
   ground, a water surface over its bed have something under them;
4. it is not a long narrow piece (under 12 m wide, 8 times as long) standing more than 10 m over the
   level's ground (the floor under a quarter of the ground's area): a wall's top, a walkway.

A ground keeping walls more than 3 m above its floor (40 % of what stands there upright) has them
moved to a piece of their own, `<name>-walls`, selected like the rest. Projects saved before get the
same when they are opened.

Over the 94 levels of Jak 3 with a shell (9 130 large pieces), this leaves 446 grounds of the 647 the
first rule alone gives: 201 pieces are selected like decor again (59 of the far views of the city,
53 floors, plates and rubble lying on others, 34 roofs and ceilings, 28 pieces of the palace, pillars,
beams, braces and bridges, 24 water surfaces, the 3 tops of the farms' walls), and 33 grounds had
walls split off. In the city's levels, what stays ground is the ground of the streets (5 to 9 m:
cobblestones, pavement, the slums, the ditches, the outposts' yards, the farms' grass). The upper
floors of interiors (caves, the Precursor levels) stay ground too: they are walked on.

## 4. Game data

`data/jak3/game-data.json` is what the editor knows of the game beyond the extracted files: names
and rules read in OpenGOAL's decompiled code, no asset.

| Part | What it is |
|---|---|
| `actors.roles` | the types that are creatures or logic (the others are objects) |
| `actors.objects`, `models`, `skeletons` | the objects' families, the models each type draws |
| `actors.effects`, `unlisted`, `placeholders` | the particle effects, the logic the catalog does not offer, the types that spawn nothing |
| `actors.water_looks` | the model of each look of the water actors |
| `actors.params`, `parents`, `code`, `code_needs` | what each type's code reads (tag, kind, default), its parent, its code file and what it needs |
| `oceans` | each ocean map: corner, height, cells, near ocean, far ocean |
| `levels` | per level: task mask, ocean, layer, companions, neighbors, continue points |
| `story` | the task nodes in the order of the game, and what depends on them |
| `enums` | the enums of the parameters |

| File | Made by |
|---|---|
| `data/jak3/game-data.json` | `python tools/gen_game_data.py <jak-project>/goal_src` (with `tools/object_categories.json`) |
| `data/jak3/particles.json` | `python tools/gen_particles.py <jak-project>/goal_src` |
| `data/jak2/creatures.json` | `python tools/gen_jak2_creatures.py` |
| `data/jak3/prefabs.json`, `data/jak2/prefabs.json` | `python tools/gen_prefab_recipes.py library --game jak3` (`jak2`); `tools/prefab_review.json` keeps the review |

Read a checkout of the game's own code (read only), then build the editor again: it reads the
copy of `data/` a build puts next to its program (the engine's tests read `data/` itself).

## 5. The game rendering

- **The renderer**: the shader of OpenGOAL's background renderer (`TFragment`'s `tfrag3`, ported
  to WebGL2 in `ui/render/game_view.ts`) draws the decor as the level builder makes it of the export
  (`crates/core/src/game/view.rs`), with the game's camera. The level is made in chunks (the pieces
  sharing a palette in a cell of 128 m); an edit makes again the chunks it touched.
- **The time of day and the weather** (`crates/core/src/game/mood.rs`): Jak 3's mood code
  (`init-weather!` and `update-mood-exterior`): the 9 fog tables (no cloud: haze, clear, smog;
  overcast: fog, clear, rain; storm: fog, clear, rain), the 3 color tables and the 3 channel groups
  of `mood-tables.gc` mixed for the weather's `cloud` and `fog`, then the two snapshots of the hour
  (`*mood-interp-table*`) mixed: the 8 palettes' weights, the fog, the sky color, the models' lights.
  The level's lamps follow the city's (palette 5 at night).
- **The sky** (`ui/render/sky.ts`): drawn behind the level: the mood's sky color high up, its
  fog color at the horizon, the sun and the moon moving along an arc through the day (from the
  morning's light direction, +x, to the evening's), stars at night, procedural clouds drifting,
  covering as much of the sky as the weather's `mood-clouds` (`cloud-min` .. `cloud-max`), lit by
  the ambient light and the sun as `sky-tng` lights its clouds, darker in a storm; a lightning flash
  lights them. The Game rendering window shows half a turn of it in a texture.
- **Rain and lightning**: rain streaks fall around the camera (`rain` 0..1); a storm (cloud above
  0.75) flashes every 5 to 10 seconds (`gen-lightning-and-thunder!`), brightening the light from
  above.
- **Effects**: the particle effects run by the game's rules (`data/jak3/particles.json`,
  `ui/render/particles.ts`; `crates/core/src/scene/particles.rs` is the reference its tests prove);
  some parts launch only at some hours; in the game rendering they fade in the fog by their
  distance. A particle mixes with what is behind it as the game sets it (`sp-cpuinfo-flag-3` and
  glows added: fire, sparks, lights; `-4` subtracted; the others mixed), and fire, flames and lava
  take their color, alpha and size over their life from the game's curves (`birth-func-curve`,
  `live-func-curve`: the `curves` of `particles.json`).

## 6. The export for the game

What **Export for the game...** and `export_level` write, in `library/exports/<project>/`:

| File | What it is |
|---|---|
| `custom_assets/jak3/levels/<name>/<name>.glb` | the decor in the game's space, lit with the 8 palettes, the builder's extras (collision); the game's collision, a node per surface |
| `.../<name>.jsonc` | the level for the builder: names, `base_id`, art groups, actors (typed lumps), regions of a level replaced |
| `goal_src/jak3/levels/<name>/<name>-obs.gc` | the level's code: the behaviors set in the editor |
| `.../<name>.gd`, `<name>-nav.json`, `level-info.gc` | its DGO, its nav meshes, its definition for `level-info.gc` |
| `README.md` | how to bring it into the game, what the export changed |

**Collision.** Jak 3 gathers the collision around Jak in a cache of 461 triangles; the decor's
shape is far denser than the game's collision. So the export writes the game's collision where the
levels' own decor is still in place, none where a piece was deleted or moved (the collision nearer
to a piece taken out goes with it), and a collision made from the shape for the pieces placed (4 m
across or more, and every shape of the editor); the report lists the places still too dense.

**Lighting.** Each piece's 8 palettes as the game has them; the pieces made in the editor get
palettes made from their color, in the proportions of the level's.

**What changes**: Jak 3's surfaces are written with the first game's fields the builder reads
(the nearest material; rails, slides and shocks lost and counted); the actor types the game lacks
are left out; actors' scripts are left out (their behavior is the level's code); paths move with
their actor; links between actors by id become links by name.

**Into the game**: copy `custom_assets` and `goal_src` into jak-project, add the lines of
`README.md` to `goal_src/jak3/game.gp` and `level-info.gc` (or the one `:nickname` to change for a
replacement), build with `(mi)`, then start the game there with
`gk -g jak3 -- -boot -fakeiso -debug -level <name>-start` (a gk of whozghiar/jak-project newer than
`v0.3.9-whozghiar.1`), or in the REPL `(start 'play (get-continue-by-name *game-info* "<name>-start"))`.
In the OpenGOAL Launcher, the editor's Play does all of it.

## 7. The remote control and the MCP server

The editor listens on 127.0.0.1:47821 when File > Settings lets an assistant control it, or on the
port the command line gives for the session (`--remote-port`); one JSON line per request:
`{"id", "tool", "args", "token"}`, one per answer: `{"id", "ok", "result" | "error", "image"}`
(`crates/core/src/editor/remote.rs`). `token` is the one the editor wrote into `remote.json`
(`{"port", "token"}`, in its preferences' folder) when it started listening: a request without it is
refused and its connection closed, and so is a line that is not a JSON object. Its tools are the MCP server's, carried out under the
engine's lock like the user's actions: they show at once and can be undone. `capture_view` asks the
interface for a picture of its 3D view (the event `jle:request`, answered with `frontend_reply`).

`jak-level-editor-mcp` (`crates/mcp`) speaks MCP on its standard input and output and forwards each
tool call to the editor with the token of its `remote.json` (`$JLE_PREFS_DIR`, else the standalone
app's preferences' folder), and starts `jak-level-editor --remote-port <port>` (found next to it,
else in `target/release/` or `target/debug/`) when none listens. The tools, their arguments and the rules of
an agent: [the guide for AI agents](mcp-agent-guide.md).

The C++ editor's `ui_input` (mouse and key events, pictures of the whole window) is gone: the
interface is a web page now. Its tests and the documentation's pictures drive it through the
DevTools protocol of its web view: start the editor with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` (Windows), then
`Page.captureScreenshot`, `Input.dispatchMouseEvent`, and
`window.__TAURI_INTERNALS__.invoke("plugin:jak-level-editor|call", {name, args})` for any call of
the engine ([captures](captures/README.md)).

## 8. Known limitations

- **The editor's own view** lights every surface with the brightest of its palettes; the game
  rendering lights as the game does.
- **The game rendering** draws the background with the game's renderer; the actors' models, the
  ocean (waves of its own), the effects and the sky are the editor's: the sky imitates the game's
  colors and clouds, not its textures, its sun's exact orbit nor its cloud mesh. The weather stays
  where it is set (the game moves it slowly on its own); the desert's sandstorms are not offered.
- **Holes** are looked for from points of walkable ground every few meters, at one height: a hole
  seen only from a ledge, or narrower than the rays' spacing far away, can be missed; a surface
  seen through (glass, water, glows) counts as a hole.
- **Effects**: what a particle's own code does (`birth-func`, `func`) is not done; some effects of
  the game come from an object's code rather than from a part-spawner.
- **Scale**: the game draws decor at any scale, but gives most of its objects their own size.
- **The story is a line**: the task nodes in the order of the game.
- **Assemblies** are found by touch (boxes, not shapes); a structure wider than 60 m is cut.
- **The terrain brush** draws a height field: no overhangs, caves or arches.
- **Collision**: the game's collision of a piece moved goes away with it; small pieces placed
  collide only when told to.
- **Several levels**: each is loaded whole; one ocean map is drawn.
- **The ocean** is left out where the ground of the levels around (the game's neighbors, the city
  seen from afar) stands above it, in squares of 3 m: land thinner than 27 m (a sea wall) keeps its
  water, water closed in by that land (not reaching the map's edge nor the document's own levels)
  goes too; inside the box of the document's own levels, their pieces alone hide it.
- **Nav meshes**: the meshes of a large ground are not linked; they do not follow the decor.
- **Behaviors**: the generated code compiles and the level builds; "the object is there" is
  checked when the level loads.
- **Replacing a level**: the game is told through one line of its code, changed by hand.
- **Jak II's structures** have no collision of their own (made from their shape) and no behavior.
- **The light of what you place** is copied from the nearest lit surfaces facing the same way, not
  baked like the artists' (no shadows).
- **The export**: the code of objects that live in a level's own files is not added to the DGO;
  the cameras and ambient sounds of levels are not exported.

## 9. Repository layout

```text
crates/core/          the engine (jak-level-editor-core, lib jle_core), no Tauri:
  src/scene/          the document, picking (BVH), the story, the particles, the light of what is placed
                      (relight), shapes, topology, skeletons, a level's style, assemblies, the ground,
                      the terrain, holes in the decor (gaps)
  src/io/             glTF, the library and its index, prefabs and the game's prefabs, game data, the
                      catalog, behaviors, the export for the game and its nav meshes, Jak II's structures
  src/game/           the game rendering's data: the chunks (view), the time of day and the weather (mood)
  src/editor/         the application without its interface: the document open, the selection, the
                      history, the tools, the jobs, the extraction, the API (api.rs), the jle:// encoders
                      (protocol.rs), the remote control and its tools
  src/navmesh.rs      the nav mesh generator (rerecast, then the game's limits)
  tests/              tests of the whole engine
crates/tauri-plugin/  the engine in a Tauri app: its command, its events, the jle:// protocol, the
                      remote control started
crates/mcp/           jak-level-editor-mcp, the MCP server
ui/                   the interface (Svelte 5, Tailwind, Flowbite): LevelEditor.svelte, lib/ (calls,
                      translations, stores), components/ (menus, panels, inspector, windows, guide),
                      render/ (the WebGL2 viewport, the game view, the sky, the gizmo, the tools)
src/, index.html      the standalone app's page (mounts ui/LevelEditor.svelte)
src-tauri/            the standalone app (jak-level-editor): Tauri, the plugin, its command line
lang/                 translations (en.json, fr.json)
data/                 what the editor knows of the games (made by tools/gen_*.py)
tools/                gen_game_data.py, gen_particles.py, gen_jak2_creatures.py, gen_prefab_recipes.py:
                      data/; capture_docs.py: the documentation's screenshots
docs/                 reference.md (this page), rust-architecture.md, mcp-agent-guide.md, behaviors.md,
                      captures/
extractor/            ogle-extract, built from OpenGOAL's decompiler (opengoal/, ISC); CMakeLists.txt at
                      the root builds it
third-party/          where the third-party code is, and its licenses
.claude/skills/       skills for Claude Code working on this repository (.agents/skills/: the same, for
                      other agents)
.mcp.json             the MCP server, for Claude Code started in this folder
.github/workflows/    build.yaml (the checks of every push), release.yaml (the releases, RELEASING.md)
```
