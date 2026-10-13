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
open-goal-level-editor [project.glb] [--library <folder>] [--lang <code>] [--remote-port <port>] [--hidden]
open-goal-level-editor --screenshot <in> <image.png> [setup] [--size WxH] [--library <folder>] [--lang <code>]
                       [--camera x,y,z,yaw,pitch] [--story off|<step>|<node>] [--select a,b,...]
                       [--add a,b,...] [--game <hour>]
open-goal-level-editor --mcp [--remote-port <port>]
open-goal-level-editor --selftest
```

- `--library` chooses the folder of the extracted games (instead of `library/`), `--lang` the
  language, for one session.
- `--screenshot` runs the whole editor in a hidden window and saves a picture of it
  (`tools/capture_docs.py` makes the documentation's this way). Setups: `levels`, `select`,
  `scale`, `prefab`, `catalog`, `models`, `decor`, `decor-all`, `materials`, `effects` (the Effects
  of the catalog; `--select` searches them), `actors` (the level with its actors: its effects
  running), `navmesh`, `export` (exported to a temporary folder), `extract` (`<in>` is a disc,
  extracted to a temporary folder), `guide`, `tools`, `help`, `build`, `textures`, `project`
  (`<in>` is a project; `select`, `scale`, `prefab` and `navmesh` take one too). `--story` shows
  every state (`off`), a step or the state just after a task node; `--select` names the elements
  used (a level added: selected whole); `--add` adds levels; `--game` shows the game rendering at
  that hour.
- `--mcp` is the MCP server; `--remote-port` the port of the editor's remote control (47821; `0`
  turns it off); `--hidden` runs the editor without a window and without touching the preferences.
- The command line modes never change the preferences. The preferences, the thumbnail cache and
  what the catalog read of the levels are in `%APPDATA%/OpenGOAL/open-goal-level-editor` on Windows
  (`~/.local/share/OpenGOAL/open-goal-level-editor` on Linux).

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
library/export/<project>/                          a level exported for the game
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

Read a checkout of the game's own code (read only), then rebuild: the editor reads the copies in
`build/bin/data/`.

## 5. The game rendering

- **The renderer**: OpenGOAL's background renderer (`TFragment` and its shaders, vendored in
  `extractor/opengoal`) draws the decor as the level builder makes it of the export, with the
  game's camera. The level is made in chunks (the pieces sharing a palette in a cell of 128 m); an
  edit makes again the chunks it touched.
- **The time of day and the weather** (`src/game_view/game_mood.cpp`): Jak 3's mood code
  (`init-weather!` and `update-mood-exterior`): the 9 fog tables (no cloud: haze, clear, smog;
  overcast: fog, clear, rain; storm: fog, clear, rain), the 3 color tables and the 3 channel groups
  of `mood-tables.gc` mixed for the weather's `cloud` and `fog`, then the two snapshots of the hour
  (`*mood-interp-table*`) mixed: the 8 palettes' weights, the fog, the sky color, the models' lights.
  The level's lamps follow the city's (palette 5 at night).
- **The sky** (`src/game_view/sky.cpp`): drawn behind the level: the mood's sky color high up, its
  fog color at the horizon, the sun and the moon moving along an arc through the day (from the
  morning's light direction, +x, to the evening's), stars at night, procedural clouds drifting,
  covering as much of the sky as the weather's `mood-clouds` (`cloud-min` .. `cloud-max`), lit by
  the ambient light and the sun as `sky-tng` lights its clouds, darker in a storm; a lightning flash
  lights them. The Game rendering window shows half a turn of it in a texture.
- **Rain and lightning**: rain streaks fall around the camera (`rain` 0..1); a storm (cloud above
  0.75) flashes every 5 to 10 seconds (`gen-lightning-and-thunder!`), brightening the light from
  above.
- **Effects**: the particle effects run by the game's rules (`data/jak3/particles.json`); some
  parts launch only at some hours; in the game rendering they fade in the fog by their distance.

## 6. The export for the game

What **Export for the game...** and `export_level` write, in `library/export/<project>/`:

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
replacement), build with `(mi)`, then `(start 'play (get-continue-by-name *game-info* "<name>-start"))`.

## 7. The remote control and the MCP server

The editor listens on 127.0.0.1:47821 (one JSON line per request: `{"id", "tool", "args"}`, one
per answer: `{"id", "ok", "result" | "error", "image"}`). `--mcp` forwards MCP tool calls to it, and
starts the editor when none runs. Besides the MCP tools, the remote control has `ui_input` (mouse and
key events one frame after the other, pictures of the whole window: the interface's tests and the
documentation's screenshots). The tools, their arguments and the rules of an agent:
[the guide for AI agents](mcp-agent-guide.md).

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
src/core/      math, log, preferences, translations, undo history
src/scene/     the document, picking, the story, the particles, the light of what is placed (relight),
               shapes, topology, skeletons, a level's style, assemblies, the terrain, holes in the
               decor (gaps)
src/io/        glTF, the library, prefabs and the game's prefabs, game data, the catalog, nav meshes and
               the export for the game, behaviors, Jak II's structures
src/render/    camera, renderer, thumbnails, offscreen captures, the palette of materials, the guide's
               animations
src/game_view/ the game rendering: the game's renderer (game_view), the time of day and the weather
               (game_mood), the sky (sky)
src/tools/     the gizmo, the nav mesh generator (Recast, then the game's limits)
src/app/       the window and its loop, document actions, panels, groups, the Game rendering window
               (game_window), building tools (holes, small pieces), the guide, behaviors,
               the preview, the topology and terrain tools, extraction, the remote control and the MCP
               server, the self-test
lang/          translations (en.json, fr.json)
data/          what the editor knows of the games (made by tools/gen_*.py)
tools/         gen_game_data.py, gen_particles.py, gen_jak2_creatures.py, gen_prefab_recipes.py: data/;
               capture_docs.py: the documentation's screenshots
docs/          reference.md (this page), mcp-agent-guide.md, behaviors.md, captures/
extractor/     ogle-extract, built from OpenGOAL's decompiler (opengoal/, ISC), and the game's renderer
third-party/   Dear ImGui, glad, tinygltf, nlohmann json, stb, Recast
.claude/skills/ skills for Claude Code working on this repository
.mcp.json      the MCP server, for Claude Code started in this folder
```
