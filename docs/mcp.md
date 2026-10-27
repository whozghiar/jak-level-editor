# The MCP server: building levels with an AI assistant

An AI assistant can use Jak Level Editor for you: you tell it what you want, in your own words, and
it opens the level, places the buildings and the decor, looks at the result and shows it to you. It works in the editor's window, under your eyes, and every change it makes can be
undone like yours.

This document has two parts:

- **[Part 1, for you](#part-1-for-you)**: what it is, how to set it up, what to ask, what it can do.
- **[Part 2, for the assistant](#part-2-for-the-assistant)**: what an AI agent needs to know to build
  well (what a level is made of, the four rules, recipes, every tool). Claude Code reads it through
  the skill `level-building-mcp` of the editor's repository (in the knowledge base mounted at
  `.agents/skills/`: `jak-level-editor-skills/level-building-mcp/`); you can read it too, to know what
  to expect.

![What an assistant built and saw, at the height of the player's eyes: a street of Jak II's Main Town, a fountain, trees](captures/mcp/assistant-view.png)

## Contents

- [Part 1, for you](#part-1-for-you)
  1. [What it is, in plain words](#1-what-it-is-in-plain-words)
  2. [Setting it up](#2-setting-it-up)
  3. [Your first requests](#3-your-first-requests)
  4. [What it can do](#4-what-it-can-do)
  5. [How it works](#5-how-it-works)
- [Part 2, for the assistant](#part-2-for-the-assistant)
  6. [What you need to know about a level](#6-what-you-need-to-know-about-a-level)
  7. [The four rules](#7-the-four-rules)
  8. [Recipes](#8-recipes)
  9. [Every tool](#9-every-tool)
  10. [When something goes wrong](#10-when-something-goes-wrong)

# Part 1, for you

## 1. What it is, in plain words

**MCP** (Model Context Protocol) is a common language that lets an AI assistant use the tools of a
program. **jak-level-editor-mcp**, the editor's MCP server, passes the assistant's requests to the
editor: the assistant says *"place the prefab small-fountain at 0, 0, 18"*, the
server passes it to the editor, the fountain appears in your window.

Who does what:

- **you** talk to the assistant (Claude Code, or another MCP client) in your own words;
- **the assistant** chooses the editor's tools and their settings, and calls them one after the
  other;
- **the editor** does each one in its window, through the same calls as your clicks: you see it happen, you can
  stop the assistant at any moment, and **Edit > Undo** (Ctrl+Z) takes back any step.

It never touches the game itself nor your files without being asked: it saves or exports only when
you ask it to.

## 2. Setting it up

1. Get the editor and its MCP server: a release's `jak-level-editor-mcp-<platform>.zip`, unzipped
   next to the editor's program; or build both ([Build it yourself](architecture.md#10-build-it-yourself-and-test-it):
   `npx yarn install` once, `npx vite build`, then `cargo build --release -p jak-level-editor -p
   jak-level-editor-mcp`: both land in `target/release/`). Extract the game
   ([user guide, Extract your game](user-guide.md#extract-your-game)): without the extracted levels,
   there is nothing to build with. The MCP server starts the editor found next to it (else in
   `target/debug/`), in its own window, when none listens; one already open is used once **File >
   Settings > Assistant (MCP) > Let an assistant control the editor** is ticked (else close it and
   let the server start one).
2. **Claude Code**: the repository has a `.mcp.json`. Start Claude Code in the repository's folder
   and accept the server `jak-level-editor` (its tools are then named
   `mcp__jak-level-editor__<tool>`). From another folder, add it once:

   ```bash
   # Windows, the repository in D:\levels\jak-level-editor
   claude mcp add jak-level-editor -- "D:/levels/jak-level-editor/target/release/jak-level-editor-mcp.exe"
   # Linux, the repository in ~/jak-level-editor
   claude mcp add jak-level-editor -- ~/jak-level-editor/target/release/jak-level-editor-mcp
   ```

3. **Another client**: run `<repository>/target/release/jak-level-editor-mcp` as a stdio server
   (`--remote-port <port>` if the editor was started with another port than 47821). For the editor
   of the OpenGOAL Launcher, give the server the launcher's editor settings folder:
   `JLE_PREFS_DIR=%APPDATA%/OpenGOAL-Launcher/jak-level-editor` (its `remote.json`).
4. Check: ask the assistant to call `get_status` (*"call get_status of the level editor"*). Its
   answer gives the library and the document, for example
   `{"levels": 272, "document": {"open": true, "level": "ctysluma", "elements": 13193, ...}, ...}`. The
   editor's status bar says **Assistant connected**. If a tool of this guide is missing from the
   list your client shows, the editor is older than the guide: build it again and restart the
   client.

The repository also has a skill for AI agents, `level-building-mcp` (in the knowledge base mounted
at `.agents/skills/`, `jak-level-editor-skills/level-building-mcp/`; `git submodule update --init`,
then `npx yarn skills:link` for Claude Code), which reminds an agent of these rules whenever it
builds.

## 3. Your first requests

Talk to the assistant as you would to a person who knows Jak 3 and the editor. A few examples, and
what it does with them:

| You ask | What the assistant does |
|---|---|
| *"Open the slums and show me a picture of the tower at dusk."* | `open_level ctysluma`, `set_camera` on the tower, `capture_view` at 18:00 with the game rendering: you get the picture in the conversation |
| *"Build a little square with a fountain and two trees in front of the tower."* | finds the ground with `raycast`, places the prefabs `small-fountain` and `haven-tree` with `place`, looks at them from where the player walks with `capture_view`, then shows you a picture |
| *"Make this door open only once the crate in front of it is broken."* | finds the door and the crate with `find_nodes`, sets the door's behavior with `set_actor` |
| *"Put a street of Haven City's Main Town in the desert."* | opens a desert level, lists Jak II's prefabs, places the buildings so that their backs face away from the player, matches their light with `match_lighting` |
| *"Build a house of the slums on the empty square, two floors, worn."* | `build_house` with `level: "ctysluma"`: walls of the slums' plaster with relief, its wooden windows and door, a pipe, stains and holes, a roof of its planks and metal plates; then pictures from the street |
| *"Make it a jumping course: Jak starts on the first platform, a checkpoint halfway, falling kills him."* | `place` with `kind: "gameplay"`: a `start`, a `checkpoint`, a `death_zone` under the platforms |
| *"Close this street with a wall of the industrial zone, 60 m long, then make it twice as high."* | `build_structure` with `kind: "wall"`, `level: "ctyinda"`, `size: [60, 0, 0]`: a solid wall of the zone's metal, its joints, bulges and pipes on both faces; then `set_structure` with `height`: built again, not stretched |
| *"Give this platform a route to the other roof."* | `set_route` with its points: in the game it goes along them and back |
| *"Put two guards on the roofs."* | `generate_navmesh` (the ground they walk on), then `place` with `kind: "gameplay"`, `name: "creature"`, `creature: "kg-grunt"` at each spot |
| *"Put a row of eight lamp posts along the street."* | one lamp post placed, then `repeat` in a `row` of 7 copies, and `arrange` `ground` so that none floats |
| *"Save it as my-square, then export it for the game."* | `save_project my-square`, then `export_level`: the files to copy into a jak-project, and their README |

Good habits:

- **Say where.** "In front of the tower", "on the roof of the hip-hop bar", "100 m north of the
  palace": the assistant finds the place in the level.
- **Ask for pictures.** It looks at the result itself before telling you it is done (one of its
  rules), and can send you the picture.
- **Keep an eye on the window.** If something goes wrong, stop it and undo: every step is one undo.
- **It does not save on its own.** Ask it to save (and to export) when you are happy with the level.

## 4. What it can do

| Family | Examples |
|---|---|
| **Look around** | list the levels, open one or several side by side, show the level at a point of the story, find elements by name or near a point, read an element's settings |
| **Place** | the game's objects (doors, crates, lamps, effects), its models, its decor parts, the prefabs (yours and the game's whole buildings), up to 2,000 at once |
| **Build** | whole houses in the style of a level (its own windows, doors, pipes, wear and roof plates), any structure from a level's building kit, shapes (floors, walls, doorways, stairs, roofs, pillars), natural ground with the terrain brush, textures |
| **Make it playable** | where Jak starts, checkpoints he continues from, death zones he falls into: written by the export |
| **Change** | move, turn, scale, copy, delete, group; put in order (drop onto the ground, line up), repeat in a row, a circle or scattered; what an object does (doors, jump pads, elevators, crates); what the game does with decor (collision, ground material, bounce, launch) |
| **Check** | look from any point, take pictures at any hour and weather with the game rendering |
| **Save and export** | save the project, export it for the game, make nav meshes, undo and redo |

The complete list, tool by tool, is [Every tool](#9-every-tool).

## 5. How it works

```text
 you (the agent)  --MCP, stdio-->  jak-level-editor-mcp  --TCP 127.0.0.1:47821-->  the editor window
```

- `jak-level-editor-mcp` is the MCP server. It speaks the protocol on its standard input
  and output and forwards every tool call to **the editor running on the user's computer**. When
  no editor runs, it starts one (it takes a few seconds the first time).
- Everything you do happens **in the window the user sees**: the user watches you build, and can
  take over at any moment. Every change is one step of the editor's history: the user (or you,
  with `undo`) can take it back.
- The editor only listens on the computer itself (127.0.0.1): nothing is reachable from the
  network. It listens only when the user lets an assistant control it (**File > Settings >
  Assistant (MCP)**), or when the MCP server started it itself; every request carries the key the
  editor writes into its settings folder (`remote.json`), which the server reads there.

The server tells your client its rules when it starts (its `instructions`): they are the
[four rules](#7-the-four-rules) of this guide in short.

# Part 2, for the assistant

This part is written for an AI agent that builds or changes levels of Jak 3 (the game the editor
edits today; Jak and Daxter and Jak II are planned) with the editor's tools:
the ideas behind them, the rules that make a level look right in the game, recipes, and every tool.
You do not need to know level editors to follow it: the notions a level needs are explained when
they first appear; the user guide of the editor is [user-guide.md](user-guide.md).

## 6. What you need to know about a level

### Coordinates

- Positions are in **meters**, `[x, y, z]`, **Y is up**. A Jak 3 level is a few hundred meters
  across; Jak is a little under 2 m tall: look from 1.6 m above the ground to see what the player
  sees around him.
- All the levels of the game share **one world**: each level is where the game puts it (the
  slums of Haven City are around x = 0..400, z = 800..1300). Two levels side by side in the game
  meet in the editor too. A new level (`new_level`) starts empty, around `[0, 0, 0]`.
- Angles are in degrees: `yaw_deg` turns around the vertical axis.

Example: to put a crate on the ground under the point `[120, 50, 900]`, either `raycast` from there
straight down to find the ground, or `place` with `on_ground: true`, which drops it on the first
surface below the position.

### The document

What the editor has open is the **document**: one level of the game (`open_level`), maybe other
levels added next to it (`add_level`), or a new empty level (`new_level`). It is made of
**elements** (nodes), each with an **id** (a number):

| Kind | What it is | Example |
|---|---|---|
| `render` | a piece of **decor**: something drawn, that does nothing by itself | a wall, a pipe, a roof, a lamp post |
| `actor` | an **object** of the game: its code makes it do something | a door, a crate, an elevator, a steam vent |
| `group` | an element that holds others | a whole level added, a prototype of decor and its copies |
| `navmesh` | where the game's creatures walk | |

The **ground** of a level is **locked**: clicks and searches skip it (`include_locked` finds it),
so that you work on what stands on it.

### Prefabs, groups and assemblies

- A **prefab** is a set of elements saved together to be placed again anywhere: a house, a market
  stall, a fountain. The user's prefabs are in `library/prefabricated/`; the **game's prefabs** are
  whole structures taken from the levels (houses of the slums, towers of Spargus, bridges...).
  `list_prefabs` lists both; `place` with `kind: "prefab"` puts one.
- A prefab placed becomes a **group**: its elements stay together (`list_groups`, `select` with
  `group`). You can make groups of your own (`group`). Each element in a group says so (`group` in
  its description).
- An **assembly** is a structure the editor finds by itself in the level: the elements touching
  each other, without the ground (`list_assemblies`). A house of the game with its facades, roof
  and windows is an assembly: the way to take a whole building out of a level.

### Facades: what the player sees, and what he must not

The game's buildings are made to be seen **from where the player can go**. Most of them are only
**a front** (a facade): a wall full of windows, signs and pipes, with nothing behind, or a few
panels open at the back. In the game this never shows, because the player can never get behind.
Placed in a new level where the player can walk around, the same facade shows **its back**: the
void, the inside of its panels, pieces floating. That is the most common mistake of a level built
from the game's pieces. [Rule 2](#rule-2-facades-are-not-buildings) is about it.

| From the street: what it was made for | From behind: what the player must never see |
|---|---|
| ![front](captures/mcp/facade-front.png) | ![back](captures/mcp/facade-back.png) |

### Light, hour and weather

The game lights its levels with colors baked into every corner of their triangles (each corner
has 8 colors, the game's *palettes*: one for the ambient light, four for the sun coming from four
directions, three for the level's own lamps), mixed according to the hour and the weather. What you
place **takes the light of the level around it** automatically; a new level is lit as Haven City
is (its lamps on at night). The **game rendering** draws the
level with the game's own renderer: what the level will look like in the game. Set its hour and
weather with `set_view`, and see it with `capture_view`.

## 7. The four rules

### Rule 1: prefabs hold structures, not crumbs

A prefab is **a structure**: a building, a bridge, a fountain. Small loose pieces (bolts, debris,
stains, papers, tiny props) clutter it and make it look careless once placed elsewhere.
`create_prefab` leaves out the elements smaller than **0.5 m** by default (`min_size`) and answers
their names (`left_out`). Keep that default; give a smaller `min_size` only for a prefab of small
things (a shelf of bottles), and check what was left out.

```json
{"tool": "create_prefab", "args": {"ids": [512, 513, 514], "name": "slums-house-corner"}}
```

```json
{"name": "slums-house-corner", "left_out": ["city-trash-01.mb-3", "city-bolt-02.mb-7"]}
```

The objects of the game (actors: a door, a crate) are always kept, whatever their size.

### Rule 2: facades are not buildings

When a building comes from the game (a prefab of the game, an assembly, decor parts), it is most
often a front only. Decide **where the player will see it from** before placing it:

- Put it where **only its front can be seen**: against a wall or a cliff, at the edge of the level,
  between other buildings, back to back with another facade.
- **Never in the middle of a place the player can walk around**: its back would show.
- Check from every place the player can reach: `set_camera` behind and beside it, at 1.6 m above
  the ground, then `capture_view`. If its back shows, move it, turn it, or hide its back behind
  other pieces (another building, a wall in the level's style: `place` a `wall` shape with
  `style`), touching it.

Example: a shop front of the industrial zone (the left picture of
[the notions above](#facades-what-the-player-sees-and-what-he-must-not)) goes at the end of a
street, its back against the level's outer wall; a building of Main Town, closed all round, can
stand in the middle of a square.

### Rule 3: look before you report

Never say a level is done without looking at it. At least:

1. `set_view` with `game_view: true` (the game's renderer) and a plain hour (`hour: 12`,
   `weather: "clear"`), then the hour and weather the user wants.
2. `capture_view` from **the player's height**: `set_camera` 1.6 m above the ground, in the
   streets, in front of and behind what you built.
3. `capture_view` from above, to see that nothing floats and nothing is missing.
4. Describe what you see to the user, problems included.

### Rule 4: build like the level

A structure must look like it belongs to the level it is in: its materials, its colors, its kind of
windows, its wear. Never a bare flat wall of one texture.

- **A house, a wall, a floor**: `build_structure` makes them whole and solid from the level's own
  pieces (`build_house` for a house; see [the recipe](#build-a-house-like-a-levels)). The game's
  facades are flat behind: prefer these wherever the player could get behind. To resize one,
  `set_structure` (never `transform_nodes` with a scale: it would stretch it).
- **Any other structure** (a wall, a tower, a gate, a floor, a ceiling): `building_kit` gives the
  level's parts by role (window, glass, shade, door, doorframe, pipe, pipe_ring, wear, roof,
  chimney, awning, sign, lamp, vine) with their size and the rotation that turns them to face +z,
  and the textures of its buildings. Make the masses with shapes in the level's style
  (`style` + `surface`: `building` for a building's walls, `roof`, `trim` for frames and bands,
  `wall` and `floor` for the level's own walls and ground), with **relief**: a plinth, bands
  between the floors and pillars at the corners a little out of the wall, overhangs under the
  roof. Then put the level's own parts on them: windows in openings their size, a door in a
  doorway, wear on the walls between the openings, pipes down them, plates on the roofs (see
  [the recipe](#build-any-structure-from-a-levels-kit)).
- **Its scale**: the game's buildings are large. A floor is about as high as two of the level's
  windows (11 m in the slums); size the masses from the kit, not from real-life houses.

## 8. Recipes

Each recipe gives the calls in order, with their arguments. Answers are shortened.

### Look at a level before changing it

```text
get_status                                   -> what is open, the camera, the selection
open_level {"name": "ctysluma", "actors": true}
get_level_info {"name": "ctysluma"}          -> its neighbors, its continue points (where the game
                                                starts the player: good places to look from)
find_nodes {"near": [210, 12, 1010], "radius": 25}
raycast {"origin": [210, 60, 1010], "direction": [0, -1, 0]}   -> the ground: its height
set_camera {"position": [230, 14, 1040], "look_at": [210, 10, 1010]}
capture_view {"game_hour": 12}
```

### Build a small street from prefabs

The example the first picture of this guide comes from: a street of Jak II's Main Town on a
pavement in the style of Haven's slums, a fountain and two trees. The buildings are the user's own
prefabs made from Jak II's Main Town (`main-town-building-a`, `-c`, `-d`: Jak II extracted, prefabs
made with `create_prefab`); use any prefab `list_prefabs` gives. They are whole buildings, closed all
round: they can stand where the player walks around them (rule 2).

```text
new_level
place {"kind": "shape", "name": "ground", "style": "ctysluma", "position": [0, 0, 0],
       "size": [160, 0.5, 120], "bumps": 0}                  -> the pavement, 0.5 m thick, flat
                                                                (bumps: how much a ground goes up and down)
list_prefabs                                                 -> the user's and the game's
place {"kind": "prefab", "name": "main-town-building-a", "position": [-45, 0.5, -20], "on_ground": true}
place {"kind": "prefab", "name": "main-town-building-c", "position": [-3, 0.5, -20], "on_ground": true}
place {"kind": "prefab", "name": "main-town-building-d", "position": [39, 0.5, -20], "on_ground": true}
list_groups                                                  -> {"groups": [{"group": 1, "name":
                                                                "main-town-building-a", "prefab": true,
                                                                "ids": [12, 13, ...]}, ...]}
place {"kind": "prefab", "name": "small-fountain", "position": [0, 0.5, 18], "on_ground": true}
place {"kind": "prefab", "name": "haven-tree", "position": [-25, 0.5, 30], "on_ground": true}
place {"kind": "prefab", "name": "haven-tree", "position": [25, 0.5, 30], "on_ground": true}
set_view {"game_view": true, "hour": 16, "weather": "few_clouds"}
set_camera {"position": [8, 2.1, 48], "look_at": [-4, 10, -10]}
capture_view                                                 (rule 3)
```

![The street from above](captures/mcp/street-above.png)


### Take a building out of a level as a prefab

```text
open_level {"name": "wascitya", "actors": false}
list_assemblies {"min_elements": 40}          -> whole structures, the largest first:
                                                 {"assemblies": [{"ids": [4210, 4211, ..., 4302],
                                                  "box": [[2391, 18, -120], [2428, 41, -84]], ...}, ...]}
select {"assembly": 4210}                     -> shows it to the user
set_camera {"frame": [4210], "yaw_deg": 30, "pitch_deg": -25}
capture_view                                  -> check it is the whole house, nothing else
create_prefab {"ids": [4210, 4211, ..., 4302], "name": "spargus-house"}
                                              -> {"name": "spargus-house", "left_out": [...]}
```

### Set the mood

```text
set_view {"game_view": true, "hour": 18.5, "weather": "few_clouds"}   -> a sunset
set_view {"hour": 22, "weather": "clear"}                              -> night, lamps on, stars
set_view {"hour": 15, "weather": "storm_rain"}                         -> rain and lightning
capture_view
```

The weathers are the game's: `clear`, `few_clouds` (the game's start), `haze`, `smog`, `overcast`,
`overcast_fog`, `rain`, `storm`, `storm_rain`, `storm_fog`; or the two numbers the game itself
uses: `cloud` (0 a clear sky, 0.5 overcast, 1 a storm) and `fog`, a scale whose middle is a clear
air (0 hazy, 0.5 clear, 1 rainy under clouds, smoggy under a clear sky); plus `rain` (0 to 1), the
rain drawn falling, apart from them. They change the
sun's light, the ambient light, the fog and the sky (clouds, sun, moon, stars) of the game
rendering; `level_lights` turns the level's lamps on or off at night.

### Make an object do something

```text
find_nodes {"etype": "com-airlock-outer"}   -> a door: its id
find_nodes {"etype": "crate"}               -> a crate: its name
set_actor {"id": 655, "behavior": {"open": "when", "when": {"when": "gone", "arg": "crate-163"}, "distance": 10}}
```

The door then opens when Jak comes within 10 m, once crate-163 is broken. The export writes it as
the level's own code.

### Draw natural ground

```text
terrain {"mode": "generate", "position": [0, 0, 0], "area": [96, 96], "hills": 14, "style": "desertb"}
  -> {"id": 88}
terrain {"id": 88, "mode": "dig", "position": [-40, 0, -10], "to": [40, 0, 15], "size": 10, "amount": 3}
terrain {"id": 88, "mode": "flatten", "position": [20, 6, -25], "size": 22, "height": 6}
```

### Build a house like a level's

```text
build_house {"level": "ctysluma", "position": [1142, 12, 198], "yaw_deg": 0,
             "floors": 2, "wear": 0.5}
  -> {"group": "house-ctysluma", "elements": 88, "size": [28.4, 26, 19.8],
      "parts": {"window": "city-slum-window-01.mb", "door": "city-slum-door-01.mb",
                "roof": "city-slum-roof-metalplates-damaged-02.mb", "pipe": "...", "wear": "12"}}
set_camera {"position": [1124, 12.6, 229], "look_at": [1142, 20, 198]}
capture_view {"game_hour": 11}
```

It stands on the highest ground under it (its foundation goes down where the ground is lower); its
door faces `yaw_deg` (0: +z). `roof`: `auto` (the level's: flat in Spargus, sloped elsewhere),
`gable`, `hip` or `flat`; `width` and `depth` in meters; another `seed`, other windows and wear. A
level without windows or doors of its own (the industrial zone) borrows Haven City's.

![The same call in three levels: Spargus, the slums, Haven City](captures/building/houses.png)

### Walls and floors that grow

```text
build_structure {"kind": "wall", "level": "ctyinda", "position": [0, 0, -20], "size": [36, 0, 0]}
  -> {"group": "wall-ctyinda", "elements": 53, "size": [...],
      "parts": {"column": "city-ind-joint-BOT-01.mb", "relief": "...", "pipe": "...", "wear": "8"},
      "ids": [412, ...]}
set_structure {"id": 412, "length": 60, "height": 22}   -> built again: more bays, two floors
build_structure {"kind": "floor", "level": "ctysluma", "position": [0, 0, 10], "size": [32, 0, 24]}
```

A wall runs along x before its turn (`yaw_deg`), its front towards +z; it is solid (the level's
texture on both faces and its top), its bays and their details drawn from their number, so a longer
wall keeps the bays it had. `size: 0` takes the level's: a floor as high as two of its windows.
`windows: true` puts the level's windows in some bays. `create_prefab_set {"level": "wascitya"}`
saves nine of them as the user's prefabs (only when asked): placed, they still grow.

![Walls and floors of four levels: the industrial zone, the slums, Spargus, Haven City](captures/building/walls.png)

### Build any structure from a level's kit

A wall of the slums with a window, relief and wear (its front towards +z; for a wall turned by a
yaw, turn every position around it and add the yaw to each part's `rotation_deg` y):

```text
building_kit {"level": "ctysluma"}
  -> {"kit": {"wall": {"texture": "city-slumwall-06", "tile": 5.93}, "flat_roofs": false,
      "parts": {"window": [{"name": "city-slum-window-01", "size": [3.83, 4.95, 1.0],
                            "rotation_deg": [0, 180, 0]}],
                "wear": [{"name": "city-ind-bullet-hole-01", "size": [2.12, 1.96, 0], ...}], ...}}}
place_many {"items": [
  {"kind": "shape", "name": "wall", "style": "ctysluma", "surface": "building",
   "position": [-8, 0.5, 0], "size": [8, 11, 0.6]},
  {"kind": "shape", "name": "window", "style": "ctysluma", "surface": "building",
   "position": [0, 0.5, 0], "size": [8, 11, 0.6], "opening": [3.5, 4.55]},
  {"kind": "shape", "name": "wall", "style": "ctysluma", "surface": "building",
   "position": [8, 0.5, 0], "size": [8, 11, 0.6]},
  {"kind": "shape", "name": "box", "style": "ctysluma", "surface": "trim",
   "position": [0, 0.5, 0], "size": [25, 0.8, 1.2]},                       <- the plinth
  {"kind": "shape", "name": "box", "style": "ctysluma", "surface": "trim",
   "position": [0, 11, 0], "size": [25, 0.5, 1.4]},                        <- the band under the roof
  {"kind": "decor", "level": "ctysluma", "name": "city-slum-window-01",
   "position": [0, 3.5, 0.15], "rotation_deg": [0, 180, 0]},               <- in its opening
  {"kind": "decor", "level": "ctysluma", "name": "city-ind-bullet-hole-01",
   "position": [-8, 3.5, 0.32], "rotation_deg": [0, 0, 0]}]}               <- on the wall's face
```

- A window shape's opening is in the middle of its height: the window part goes at
  `(height - window height) / 2` above the wall's base, its opening about 0.92 of its size (its frame
  covers the edges).
- A part whose `rotation_deg` is a yaw alone (all but some pipes) has its `position` at the middle
  of its bottom. Decals (`size` depth 0) go 2 cm in front of the wall's face.
- A part a little larger than the room for it: `scale` it down, never stretch one way only.

### Make it playable

```text
place {"kind": "gameplay", "name": "start", "position": [0, 1, 0], "yaw_deg": 90}
place {"kind": "gameplay", "name": "checkpoint", "position": [16, 3, 0]}
place {"kind": "gameplay", "name": "death_zone", "position": [16, -12, 0], "size": [60, 1, 30]}
```

The start (one per level: a second moves it) and the checkpoints become the level's continue points
at the export, a death zone an invisible box Jak falls to his death in, back at his last checkpoint.
Put the death zone below every edge Jak can fall from.

### Routes of moving objects

```text
place {"kind": "object", "name": "plat", "position": [8, 2.5, 6]}       -> {"nodes": [{"id": 77}]}
set_route {"id": 77, "points": [[8, 2.5, 6], [16, 4, 6], [24, 5, 6]]}
  -> {"tag": "path", "points": [[8, 2.5, 6], [16, 4, 6], [24, 5, 6]]}
```

A platform or an elevator (an object whose type has a path) goes along its route in the game, and
back. Its own place first; moved later, it takes its route with it; `points: []` takes it away.

### Repeat and arrange

```text
place {"kind": "decor", "level": "ctygenb", "name": "city-street-lamp-overhead-01",
       "position": [0, 0.5, 0]}                       -> {"nodes": [{"id": 412, ...}]}
repeat {"ids": [412], "mode": "row", "count": 7, "axis": "x", "spacing": 8}
                                                      -> {"copies": 7, "ids": [...]}
arrange {"ids": [412, ...], "how": "ground"}          -> each on the ground under it
```

`repeat` `circle` puts the copies around a circle of `radius` the elements are on, `scatter` at
random within `radius` on the ground, each turned and sized a little differently (`vary`).

### Save and export (only when the user asks)

```text
save_project {"name": "main-town-street"}     -> library/projects/main-town-street/
export_level {"name": "mtstreet", "nickname": "mts"}
                                              -> library/exports/main-town-street/: the folder takes
                                                 the project's name, the level in the game "mtstreet";
                                                 its README.md says how to add it to the game
```

## 9. Every tool

**Status and levels**

| Tool | What it does |
|---|---|
| `get_status` | the library, the document (its levels, bounds, unsaved changes), the story view, the camera, the selection, the view (game rendering, hour, weather) |
| `list_levels` | the levels that can be opened (`filter`, `near`: a level's neighbors, `game: "jak2"`: Jak II's) |
| `open_level`, `new_level` | open a level (`actors: false`: its decor alone; `game: "jak2"`: a level of Jak II as decor of Jak 3) or start an empty one |
| `add_level`, `remove_level`, `merge_level` | add a level next to the one open, at its place in the world; take it out; make it part of the document's own level |
| `get_level_info` | a level's neighbors, continue points, role and bounds |
| `replace_level` | make the document a replacement of a level of the game (the original shown in blue, aligned, turned) |
| `set_story` | show the levels at a point of the story |

**Finding things**

| Tool | What it does |
|---|---|
| `find_nodes` | elements by name, type (`etype`), kind, near a point, in a box, in a group; their id, position, size, box, group |
| `get_node` | everything about one element (an actor's data and what it can do) |
| `raycast` | the first surface along a ray: the ground under a point, a wall in front |
| `list_assemblies` | the whole structures of the decor (a house, a bridge) |
| `list_groups` | the groups: the prefabs placed and the groups made, with their elements and box |

**Choosing what to place**

| Tool | What it does |
|---|---|
| `list_prefabs` | the user's prefabs and the game's (`library/prefabricated/`) |
| `list_catalog` | the objects of the game: `group` `objects` (props, doors, water), `effects` (smoke, fire, lights, sparks, water: `effect_kind`), `interactive` (crates, platforms, buttons) |
| `list_models`, `list_decor` | the models of the game (placed as decor), the decor parts of the levels |
| `list_textures` | textures for `set_material` |

**Building**

| Tool | What it does |
|---|---|
| `place`, `place_many` | put a `prefab`, an `object`, a `model`, a `decor` part, a new `shape` (box, floor, wall, doorway, window, pillar, ramp, stairs, roof, roof-hip, cylinder, ground; `style` and `surface` a level's look) or a `gameplay` element (`start`, `checkpoint`, `death_zone`) at a position, turned, scaled, `on_ground`; `place_many` up to 2,000 at once, one undo |
| `build_house` | a whole house in the style of a level, from its own pieces (rule 4) |
| `build_structure`, `set_structure` | a solid wall, floor or house of a level's pieces; built again at another size or look (more pieces, not stretched) |
| `create_prefab_set` | nine structures of a level saved as the user's prefabs, still growing once placed |
| `set_route` | the route a moving platform or an elevator goes along |
| `building_kit` | a level's building parts by role, their size and turn, its buildings' textures |
| `transform_nodes`, `duplicate_nodes`, `delete_nodes` | move, turn, scale; copy; delete |
| `arrange`, `repeat` | put elements in order (`ground`, `height`, `line`, `face`); copies in a `row`, a `circle` or `scatter`ed |
| `set_shape`, `set_material` | a shape's size, bends, corners; a texture for elements |
| `terrain` | natural ground: paint, raise, dig, smooth, flatten, walls, generated hills, in a level's style |
| `group`, `ungroup` | name a group of elements, or undo one |
| `create_prefab` | save elements as a prefab (`min_size`: [rule 1](#rule-1-prefabs-hold-structures-not-crumbs)) |
| `match_lighting` | bring a structure of Jak II to the brightness of the level |
| `set_actor` | what an object does (doors, bouncers, elevators, crates, presence) and its parameters |
| `set_decor` | what the game does with decor: collision, ground material, bounce, launch |
| `restore_effects` | put back the level's particle effects |
| `set_visibility`, `select` | hide or show; show elements, a level, an assembly or a group to the user |

**Checking**

| Tool | What it does |
|---|---|
| `set_camera` | look from a position at a point, or frame elements |
| `set_view` | the game rendering, the hour, the weather, the rain, the lightning, the lamps, the effects |
| `capture_view` | a picture of the view (`game_hour`: the game rendering at that hour; `only`: some elements alone) |

**Saving and exporting**

| Tool | What it does |
|---|---|
| `save_project`, `open_project` | save the document in `library/projects/<name>/`, open one |
| `export_level` | write the level for OpenGOAL's level builder (`library/exports/<project>/`) |
| `generate_navmesh`, `export_navmesh` | where the game's creatures walk |
| `undo`, `redo` | take back, do again |

The complete description of each argument is what your client receives from `tools/list`.

## 10. When something goes wrong

| Problem | What to do |
|---|---|
| `the editor did not start` | the editor is not built, or the library is missing: build it, start it once by hand |
| A call waits for a long time | opening a level, listing the decor or prefabs the first time reads the levels (seconds to a minute): it answers when ready |
| `no document` | `open_level` or `new_level` first |
| An element's id is not found | ids change when elements are deleted and undone; find them again with `find_nodes` or `list_groups` |
| A tool of this guide is missing | the editor is older than the guide: build it again, then restart your client |
| A building shows its back | move it or turn it so that its back faces a wall or the edge of the level, or hide its back with other pieces ([rule 2](#rule-2-facades-are-not-buildings)) |
| A piece is too dark or too bright | it took the light of the level around it; a structure of Jak II: `match_lighting` |
