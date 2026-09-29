# OpenGOAL Level Editor

A simple level editor for the levels of [OpenGOAL](https://github.com/open-goal/jak-project)
(Jak and Daxter, Jak II, Jak 3). It extracts the levels from **your own copy of the game**, loads
one, and lets you select its elements to move, rotate, stretch, copy or delete them, and save
groups of elements as prefabs to place again anywhere.

A level opens with its decor, its objects (crates, doors, platforms...), its water and ocean, and
without its creatures; the levels around it in the game can be added to it, each at its place, to
work on the junction between two levels or to build a new one next to the game's. A story bar
shows each level as the game shows it at any point of the story (Jak II and Jak 3): the palace
square before and after Mar's tomb opens, Dead Town before and after its tower falls... A catalog
holds every object of the game and the decor of each level, to put in a level or to build a new
one, and an AI assistant (Claude Code...) can do the building for you through the editor's MCP
server. The editor also generates the nav meshes the creatures of Jak II and Jak 3 walk on, and
**exports the level for the game**: the files of OpenGOAL's level builder, for Jak and Daxter,
Jak II or Jak 3, the level adapted to the game chosen.

- No game data comes with the editor: everything is read from the user's disc.
- It needs nothing but this repository and your disc: it does not use jak-project to build, to
  extract or to run. The repository holds the editor and `ogle-extract`, the extractor it runs,
  and the extracted levels go to `library/` in the repository (ignored by git).
- The interface is translated with JSON files (`lang/`): English and French for now.
- It is built on the work of the OpenGOAL team (its extractor comes from OpenGOAL's decompiler,
  its game data from their decompiled code), with which it is not affiliated: see
  [License](#13-license).

![A level loaded: the Levels panel on the left, the 3D view, the inspector on the right](docs/captures/01-overview.png)

**[A guided tour, with screenshots](docs/captures/README.md)** shows how the editor works, step by
step: from your disc to a level built by hand or by an assistant.

## Contents

1. [Requirements](#1-requirements)
2. [Building](#2-building)
3. [Extracting your game's levels](#3-extracting-your-games-levels)
4. [Using the editor](#4-using-the-editor)
5. [Building levels with an assistant (MCP)](#5-building-levels-with-an-assistant-mcp)
6. [Shortcuts](#6-shortcuts)
7. [Languages](#7-languages)
8. [Files](#8-files)
9. [Command line](#9-command-line)
10. [What was tested](#10-what-was-tested)
11. [Known limitations](#11-known-limitations)
12. [Repository layout](#12-repository-layout)
13. [License](#13-license)

## 1. Requirements

**To use the editor**

- Windows 10/11 or Linux, 64-bit, with an OpenGL 3.3 graphics card.
- Your PS2 copy of Jak and Daxter: The Precursor Legacy, Jak II or Jak 3: the disc image (`.iso`),
  or the folder of a disc already extracted. `iso_data/` in the editor's own folder is there for
  them (put yours in `iso_data/jak1`, `iso_data/jak2`, `iso_data/jak3`, or however you like): it is
  ignored by git, and where **File > Extract game assets...** looks first.
- Disk space for all the levels of a game: 0.7 GB for Jak and Daxter (24 levels), 2.6 GB for
  Jak II (147), 3.7 GB for Jak 3 (272); and about 5 GB of free memory while they are extracted.

**To build it**

- CMake 3.20 or newer, Ninja, a C++20 compiler (tested with Clang 19 on Windows).
- git or network access on the first `cmake`: SDL3 is downloaded when it is not installed.

## 2. Building

```bash
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
./build/bin/open-goal-level-editor --selftest   # internal tests: "OK (0 failures)"
./build/bin/open-goal-level-editor              # starts the editor
```

`build/bin/` then holds:

| File | What it is |
|---|---|
| `open-goal-level-editor` | the editor |
| `ogle-extract` | the level extractor, run by the editor (or alone, see [3](#from-the-command-line)) |
| `ogle-extract-data/` | the games' tables the extractor reads (types, disc files, level names) |
| `data/` | what the editor knows of each game (actor roles, water, oceans, story: see [8](#game-data)) |
| `lang/` | the translations of the interface |

Keep them together if you move the editor.

| CMake option | Default | Effect |
|---|---|---|
| `OGLE_BUILD_EXTRACTOR` | `ON` | builds `ogle-extract` |
| `OGLE_SDL3_SOURCE_DIR` | empty | builds SDL3 from this folder instead of downloading it |

SDL3 comes from an installed SDL3 package when CMake finds one, else from `OGLE_SDL3_SOURCE_DIR`,
else it is downloaded (tag `release-3.4.10`); the first build takes a few minutes. Dear ImGui,
glad, tinygltf, nlohmann json, stb and Recast (the nav mesh generator) are in `third-party/`.

## 3. Extracting your game's levels

### From the editor

1. **File > Extract game assets...** (or the button of the Levels panel on the first start).
2. **The game**: choose Jak and Daxter, Jak II or Jak 3.
3. **Your disc**: choose your `.iso` file (**.iso file...**) or the folder of your disc
   (**Folder...**). The editor reads the disc and shows its game, version and serial (for
   example *Jak II: Renegade - pal - SCES-51608*); it tells you if the disc is another game.
   The disc can be anywhere: nothing has to be copied to a particular folder, but both dialogs
   open on `iso_data/` in the editor's own folder by default, ready if that is where you put yours.
4. **Where to write the levels**: by default `library/` in the editor's own folder (the
   repository it was built from; `library/` next to the executable when it is moved elsewhere).
   `library/` is ignored by git. Each game goes to its own subfolder, so the three games share it.
5. **Extract all the levels**: the progress bar shows each level written; **Cancel** stops.
   When it is done, the Levels panel shows the game's levels.

![The extraction window after the extraction of Jak II's 147 levels](docs/captures/02-extraction.png)

An `.iso` is read in place: the extractor copies only the files it reads (the `DGO` and `CGO`
folders, about 400 MB of a 4 GB disc) to a work folder of the destination, and deletes it when
done. The extractor reads nothing but your disc and its own tables (`ogle-extract-data/`), and
writes nothing outside the destination.

Measured with Jak II PAL on a 16-core processor and an SSD: the 147 levels are extracted in less
than a minute and take 2.6 GB. Jak and Daxter (NTSC, *Black Label*) and Jak 3 (NTSC) were extracted
the same way, from disc folders of their own.

![Jak and Daxter: the village, its sea and the far sea to the horizon](docs/captures/19-jak1.png)

![Jak 3: Spargus](docs/captures/20-jak3.png)

### What is written

```text
library/<game>/library.json                             the game, its version, the levels extracted
library/<game>/common/models/*.glb                      models every level uses (collectables, Jak...)
library/<game>/<level>/decor/<level>-background.glb     the level: decor (one node per element),
                                                        collision and actors
library/<game>/<level>/models/<model>-lodN.glb          the models of its actors (crates, doors...)
library/<game>/<level>/level.json                       what the folder holds (DGO, actors, models)
library/<game>/prefabs/*.glb                            your prefabs for that game
```

These files come from your copy of the game: keep them on your computer, do not share them.

### From the command line

```text
ogle-extract <game.iso | disc folder> <output folder> [--levels ATO,CIB] [--textures]
ogle-extract --detect <game.iso | folder>   # game, version and serial (json)
ogle-extract --list <game.iso | folder>     # the levels of the disc (json)
```

Without `--levels`, every level is extracted; `--textures` also writes each level's textures as
PNG files. The details (options, messages read by the editor, origin of the code) are in
[`extractor/README.md`](extractor/README.md).

## 4. Using the editor

```text
+----------------------------------------------------------------------+
| File Edit View Tools Help                                            |
| [Move W] [Rotate E] [Snap] | [Copy] [Paste] [Delete] | [Create a prefab] | [Undo] [Redo] |
+----------------+-----------------------------------+-----------------+
| Levels|Catalog |                                   | Inspector       |
| |Prefabs       |              3D view              | (the selection  |
|                |                                   |  or the level)  |
|                | [Story] < ======o========= >      |                 |
+----------------+-----------------------------------+-----------------+
| hints                   |  last message                              |
+----------------------------------------------------------------------+
```

### Loading a level

In the **Levels** panel, choose the game at the top if you extracted several, then a level of the
list (the filter finds `cty` for the Haven City districts, a DGO name, etc.). A picture of the
level appears under the list: **Load the level** (or double-click it). The level comes with its
decor, its actors (each one drawn with its model when the extracted files have one) and its
collision (kept, not shown). With **With its actors** unchecked, it comes without any actor: its
decor alone (the same for **Add to the document**).

The list only has the game's levels: the parts of a disc that are only actors (the actors a
mission adds to a level, cutscenes) and the copies of a level found in another DGO are left out.
When a level opens:

- **Creatures are left out**: enemies, characters, animals, the Titan suit, the characters of the
  cutscenes. So are the logic actors that the game does not draw (particle spawners, cameras,
  triggers, managers). The objects stay: crates, doors, platforms, props, turrets...
- **Water** (pools, fountains, falls) is drawn translucent, **dark eco** dark purple, **lava**
  with its own texture: the water actors get the model of their "look". The **ocean** is drawn at
  its height where its map covers, and, for the levels the game draws it with (the atoll, the
  port, Jak and Daxter's coast...), the **far ocean** to the horizon, behind everything as the game
  draws it. The ocean is the one of any level shown that has one, the level opened first (View >
  Ocean hides it).
- Jak and Daxter's levels are lit as at noon (the game changes their light with the time of day).
- The levels the game shows with it come too (**Shown with this level** in the inspector, for
  example Samos' hut with Dead Town: uncheck one to hide it), and so do the actors the missions
  add to it (the **mission layers**), shown by the story bar when their mission is under way.

### The story

Some levels change along the game. The bar at the bottom of the view shows the level at a point
of the story (Jak II and Jak 3):

- The slider goes through the **states of the level**: the start of the game, then each step of
  the missions that changes it, in the order of the game; the steps that change nothing there are
  skipped. The palace square has three states among the 324 steps of Jak II's story (Mar's tomb
  opens after `canyon-insert-items-resolution`), Dead Town four (its tower falls at the end of the
  first visit, then the Titan suit missions change it). The text says the last task node done.
- The arrows go to the previous and next state.
- With several levels in the document, the list next to **Story** chooses the level the slider
  moves: each level is at its own point of the story.
- **Story** unchecked shows every state at once.

What changes: the actors (the game keeps an actor while its kill mask and the level's task mask,
set by the missions of that level, share no bit), the decor prototypes some missions show or hide,
the actors whose type appears or disappears with a mission, the mission layers, the height of the
sewer's water. Nothing is removed from the document: hidden elements are only not drawn, and a
saved project keeps every state.

![The palace square after canyon-insert-items-resolution: the broken statue and its rubble](docs/captures/05-story.png)

### Several levels side by side

The levels of a game share its world: each extracted level keeps the game's coordinates, so levels
that are side by side in the game meet in the editor too, with nothing to align. With a level open
(or a new one), choose another in the Levels panel and click **Add to the document**: it is loaded
at its place, with the levels the game shows with it and the layers of its missions, and it can be
undone. What for:

- **The junction between two levels**: open `ctyinda`, add `ctyindb`, and the streets of the
  industrial zone run from one level into the other; both are edited like one.
- **A new level next to the game's**: **File > New level**, then add the levels of the zone it
  will join: build at their side, at the right place in the game's world.

The inspector (nothing selected) lists the document's **levels**: show or hide each level added,
click its name to frame it, **Junction** frames where it meets the level opened (the strip where
their boxes overlap), **Remove** takes it out of the document. **Around it in the game** lists the
levels the game displays together with those of the document (read from the game's continue
points, the places where it starts the player): click one to add it. A saved project keeps its
levels, each under its group.

**Select** (next to each level of the inspector), or **Edit > Select its level** (Ctrl+L) with an
element of a level selected, selects the **whole level**: the gizmo stands in its middle, and
moving or turning it moves the level with everything it holds (decor, collision, actors). For the
level opened, its root elements are selected (without the levels added nor the nav meshes).

Each level added is shown along the story as when the player is there: its own task mask, the
layers its missions lend it, and its own point of the story (the list of the story bar).

![ctyindb selected whole, next to ctyinda: the gizmo in its middle](docs/captures/17-level-selected.png)

![ctyinda with ctyindb and ctywide added: the industrial zone, the city walls, the palace, and the inspector's levels](docs/captures/16-levels.png)

### Moving around

| Action | Input |
|---|---|
| Fly | right button held + WASD (ZQSD on AZERTY); Q / E (A / E on AZERTY): down / up |
| Speed | Shift: faster, Ctrl: slower; the wheel while flying changes the speed |
| Forward, pan, turn around | wheel; middle button; Alt + left button |
| Frame | F: the selection; Home: the level |

### Selecting and editing

- **Click** an element to select it: each decor element is its own object (a click on a pipe
  selects that pipe, not every copy of it). **Shift or Ctrl + click** adds to the selection;
  **dragging** in an empty spot selects everything in the rectangle.
- The level's shell is split in pieces: a wall, a staircase or a bridge is selected on its own.
  Only the large pieces, the terrain, are locked, so that clicks go to what is on them.
- **Move (W)** and **Rotate (E)**: drag an arrow, a square or a ring of the gizmo. **Snap** moves
  by 0.5 m and turns by 15 degrees. The inspector gives the exact position and rotation.
- **Scale (R)**: drag the square at the end of an axis to stretch the selection along it (a
  catwalk of the industrial zone made longer, a wall made higher), or the square in the middle to
  make it bigger or smaller. The axes are the element's own, and each selected element scales
  around its origin. **Snap** scales by steps of 0.1. The inspector's **Scale** field gives the
  exact values: with **Keep the proportions** checked, changing one changes the three.
  Decor (pieces, prototypes, models) keeps its scale in the game; most objects do not (the game
  gives its actors their own size), and the inspector says so.
- **Copy** (Ctrl+C) then **Paste** (Ctrl+V) puts copies where the originals were, selected: move
  them with the gizmo. **Duplicate** (Ctrl+D) does both at once.
- **Delete** (Del) removes the selection. **Undo** (Ctrl+Z) and **Redo** (Ctrl+Y) go back and forth.

![A flight of steps of the palace selected, with the gizmo and the inspector](docs/captures/03-selection.png)

![A canal bridge stretched two and a half times along its length: the scale gizmo and the inspector's Scale field](docs/captures/07-scale.png)

### Prefabs

1. Select the elements that go together (a building and its props, a group of crates...).
2. **Create a prefab** (toolbar, inspector or Edit menu), give it a name: it is saved in the
   game's `prefabs/` folder and appears in the **Prefabs** panel, with a picture.
3. Click a prefab, then click in the view to place copies where the cursor points (R or
   Ctrl + wheel turns it, Esc stops), or drag it into the view to place one. A prefab sits on the
   surface it is placed on.

Right-click a prefab to delete it.

![A prefab made of barrels and a crate, and a copy of it placed in front](docs/captures/04-prefabs.png)

### The catalog

The **Catalog** panel lists what else can be put in a level, placed like prefabs:

- **Objects** (1,171 for Jak II), by family, with a filter:
  - every object of the game (531): crates, collectables, platforms and elevators, doors,
    buttons, jump pads, hazards, breakables, lights, turrets, vehicles, the water, dark eco and
    lava of each level (with the type of that level's water: its ripples and color)... one entry
    per model for the objects that choose theirs by name. The few the game draws without a model
    (electric gates, fire, the managers of a level's objects) are markers in the editor;
  - the **particle effects** of the levels (636): one per particle group their actors use, the
    lights, neon signs, steam, sparks, drips... drawn by the game, markers in the editor;
  - the **logic** of the levels that makes sense in another (4): the neon Baron sign, the boats
    of the port, a parking spot, the electric belt of the fortress. The rest is left out: what
    does nothing in Jak II, the fights, formations and spawners of enemies, cutscenes.

  A new object takes the data of an object of its type found in the game's levels (the pickup of
  a crate, the particle group of an effect...), without what ties that one to its level (its
  name, its story, the actors and paths it refers to).
- **Models** (182 for Jak II): every other model of the game but the characters (parts of
  objects, debris, props), placed as decor: the game draws them, they do nothing.
- **Decor**: the decor of the levels, to reuse in another: their prototypes (a pillar, an arch, a
  catwalk, a lamp post) and the pieces of their shell (a wall, a bridge). **All levels** searches
  the 31,552 parts of Jak II's levels by name (`catwalk` finds the 23 catwalks of the industrial
  zone, the slums, the drill platform and `caspad`), each prototype once; choosing a level shows
  its whole decor. A part sits on the point it is placed at.

The first time a library is shown, the editor reads what the catalog needs of every level (their
decor and the data of their actors, not their geometry) in the background, a few seconds, and
keeps it in its preferences folder: the next times the catalog is complete at once.

Click an element, then click in the view to place copies (R or Ctrl + wheel turns it, Esc stops),
or drag it into the view.

![The objects of the catalog, by family](docs/captures/06-catalog.png)

![The decor of every level searched for "catwalk"](docs/captures/08-decor-search.png)

### A new level

**File > New level** (Ctrl+N) starts an empty level of the game shown in the Levels panel: build
it from the decor and objects of the catalog and from your prefabs. There is no ground at first:
elements placed in the void go to height 0.

![A new level built from the catalog: two decks, a stretched bridge over a pool, lamps, crates and orbs](docs/captures/10-new-level.png)

### Nav meshes

The enemies and citizens of Jak II and Jak 3 walk on **nav meshes**: flat polygons over the ground
of a level, on which the game finds their way. **Tools > Nav meshes** makes them from the level's
walkable surfaces:

1. Set **the creature**: its radius (how far it keeps from the walls), its height (the room it
   needs above the floor), the highest step it climbs and the steepest slope it walks.
   **Precision** holds the voxel size, the longest edge, the outline tolerance and the smallest
   walkable island kept.
2. **Generate for the whole level**, or **Generate around the selection** (only there): the nav
   meshes of that region are replaced. With **Only the ground under the selection** checked, only
   the pieces of walkable ground that the selected elements stand on are kept (select something in
   a street): the roofs, ledges and closed rooms where no creature goes are left out.
3. The new nav meshes are selected and drawn over the level, one color per mesh, their borders
   brighter (**View > Nav meshes** hides them). The inspector gives a mesh's polygons, corners and
   walkable area, and says whether it suits the game; **Generate this area again** makes it again
   with the window's parameters.

The walkable surfaces are those of the decor shown, without the water, dark eco, glows and light
shafts, nor the objects (their models move, open or break). Recast (from recastnavigation) builds
the polygons; the editor then fits them to what the game reads: convex polygons of 3 or 4 corners
facing up, neighbors sharing their edges exactly, at most 255 polygons per mesh (a larger piece of
ground is cut in several meshes, and the small pieces a cut leaves join a neighbor), each mesh in
one piece. Each mesh gets the route table the game follows (the first step of the shortest way
from every polygon to every other) and the polygon grid it uses to find the polygon under a point.
A nav mesh is an element of the document: it can be deleted, undone and redone, and it is saved
with the project.

**Export the nav meshes...** writes them as the game's levels hold them, an `entity-nav-mesh` per
mesh (polygons, route table, polygon grid, tags), in a `nav_data` file (`.json`) for a custom
level builder; **First id** is the entity id of the first one, the next ones follow.

![The nav meshes of a district: the streets, the yard and the catwalks, one color per mesh, the Nav meshes window and the inspector](docs/captures/15-navmesh.png)

For the 448 x 448 m district of [the next part](#5-building-levels-with-an-assistant-mcp), keeping
the ground connected to a few street points: 8 nav meshes, 1,323 polygons and 96,800 m2 in 0.6
seconds; 151 pieces of ground away from the streets (the insides of the blocks, the facades'
ledges, the tops of props) were left out.

### Saving

**File > Save** (Ctrl+S) writes the level as a project: a standard `.glb` file that Blender also
opens. Backups are kept next to it (`<project>.glb.backups/`), and **File > Open project**
(Ctrl+O) opens it again.

### Exporting for the game

**File > Export for the game...** writes what the view shows (the levels and elements hidden, or
not there at the point of the story shown, are left out) as a **custom level of OpenGOAL**: the
files its level builder (`goalc/build_level`) takes, for the game chosen.

- **Destination game**: Jak and Daxter, Jak II or Jak 3; for another game than the level's own,
  the level is adapted to it (below).
- **Name** (the level's name in the game, 3 to 10 lower case letters, digits or `-`, not a level of
  the game), **Nickname** (3 letters: its DGO file), **ISO name** (8 letters at most), **First
  actor id** (above the game's own actors' ids: 40,000 for Jak and Daxter, 50,000 for Jak II,
  90,000 for Jak 3 by default).
- **With the actors**, and the collision: **The game's collision** (the levels' own collision,
  exact, where they have it; the builder makes the rest from the shape of the decor: the elements
  placed, the new levels) or **Made from the decor** (all of it from the decor's shape, with
  automatic walls: for levels whose own decor was moved or deleted).
- **Start**: where the player starts, a continue point of the level opened by default (moved with
  it).
- **Written in**: the level goes to a folder of its name there; by default `exports/<game>/` next
  to the extracted games, then the last folder used for that game. OpenGOAL reads its custom levels
  from `custom_assets/<game>/levels/` of its checkout.

![The export window after the export of the slums of Jak II for Jak 3, and its report](docs/captures/18-export.png)

| File | What it is |
|---|---|
| `<name>.glb` | the decor in the game's space, one node per element with the builder's extras: `set_collision` + `ignore` on the decor covered by the game's collision (and on the see-through surfaces: water, glass), none on the rest (the builder makes their collision); the game's collision, one node per surface (`set_collision`, `set_invisible`, `collide_material`, `collide_event`, `collide_mode`, `noedge`, `nocamera`, `noentity`, `nolineofsight`) with the game's normals |
| `<name>.jsonc` | the level for the builder: its names, `gltf_file`, `base_id`, the destination's own keys (Jak and Daxter's `sky`, `tex_remap`, `ambients`; Jak II and 3's `base_region_id`), the art groups of the actors' models, and the actors (`trans`, `etype`, `game_task`, `quat`, `bsphere`, `lump`: their data typed as the builder reads it) |
| `<name>.gd` | its DGO: the art groups and the level |
| `<name>-nav.json` | Jak 3: the document's nav meshes (`nav_data`) |
| `level-info.gc` | the level's definition for `goal_src/<game>/engine/level/level-info.gc`, made from the test-zone's of that game, with its start |
| `INSTALL.md` | how to add it to the game, and what the export changed |

What is adapted to the destination game:

- **Collision surfaces**: the builders take Jak and Daxter's fields and convert them for Jak II and
  Jak 3. The surfaces of Jak II and 3 are written with them: their materials that Jak and Daxter
  has keep theirs (stone too), the others take the nearest (`hdwood`: wood, `carpet`: straw, the
  metals: metal...); their events beyond Jak and Daxter's (rails, slides, shocks) and their own
  flags (`noboard`, `nopilot`, `nojak`...) are lost, and counted in the report. `nogrind`, on
  nearly every surface of Jak II and 3, is off on every surface a builder makes: not counted.
- **Actor types**: the types the destination game does not have are left out (and its
  placeholders: types Jak 3 keeps for Jak II's entities, which spawn nothing of theirs, such as
  `cty-fruit-stand`); Jak II's doors become Jak 3's (`hide-door-a` becomes `cty-door`...). The art
  groups are the models the destination's code gives each type.
- **What crates and pickups hold**, and what a security wall asks for: the destination's pickup of
  the same name (`pass-red` is 29 in Jak II, 39 in Jak 3); a pickup it does not have empties the
  crate. Jak and Daxter's power cells and scout flies, whose amount is a task, are left out.
- **The story**: an actor's kill mask belongs to its level's story (the new level has its own), so
  every actor is there all along; for another game, the task of an actor (`game_task`, `task`,
  `task-actor`) and its scripts (`pair` data: `on-activate`...) are left out.
- **Links between actors** by their id become links by name (the builder numbers the actors
  again); a link to an actor that is not in the level is left out. The integers of 8 and 16 bits
  are written on 32 (the builder has no smaller ones), and each actor's visibility box is left
  out.

The report of the window (and `INSTALL.md`) says each change and how many elements it concerns.
To add the level to the game (also in `INSTALL.md`):

1. Copy the folder to `custom_assets/<game>/levels/<name>/` of the jak-project checkout.
2. In `goal_src/<game>/game.gp`, next to the test-zone's lines: `(build-custom-level "<name>")`
   and `(custom-level-cgo "<NICK>.DGO" "<name>/<name>.gd")`.
3. Add the definition of `level-info.gc` to `goal_src/<game>/engine/level/level-info.gc`, with an
   `:index` no other level has.
4. Build the game (`(mi)` in the REPL) and go there: `(bg-custom '<name>-vis)`.

## 5. Building levels with an assistant (MCP)

The editor can be driven by an AI assistant such as Claude Code, through the
[Model Context Protocol](https://modelcontextprotocol.io): you describe the level you want, the
assistant opens a level or starts a new one, places decor, objects and prefabs, looks at the result
through the camera and saves the project. Everything happens in the editor window you see, and
every change can be undone (Ctrl+Z).

`open-goal-level-editor --mcp` is the MCP server: it speaks the protocol on stdin and stdout,
forwards each tool call to the editor running on this computer, and starts the editor when none
is running.

**With Claude Code**: the repository has a `.mcp.json` for it. Build the editor, start Claude Code
in the repository's folder and accept the server `open-goal-level-editor`. From another folder,
add it once:

```bash
claude mcp add open-goal-level-editor -- "<repository>/build/bin/open-goal-level-editor.exe" --mcp
```

Then ask, for example: *"Open ctypal, put three crates in front of the fountain and show me a
picture"*, or *"Start a new level with a catwalk of the industrial zone three times as long"*.

![The view the assistant gets from capture_view: the demo level of the tour](docs/captures/11-assistant-view.png)

The demo level of [the tour](docs/captures/README.md#9-let-an-assistant-build-it) was built with
these tools by `tools/build_demo_level.py`, through the editor's remote control; the picture above
is what `capture_view` returned.

A larger example: **Ironworks**, a district in the style of the industrial zone and as large (448 x
448 m, 15,465 elements), built in about 100 seconds by `tools/build_industrial_district.py`, a small
MCP client of `--mcp` calling the tools of the table below. It reuses the zone's modules as the
game places them: 15 closed blocks, each made of four of the zone's facades with everything the
game hangs on them (pipes, lamps, windows, stains, red lights and their glow), roofs, the streets
closed at the edge by wall buildings, catwalks with the zone's ramp, a yard with floor vents,
steam stacks and a pipe bridge, and the district's nav meshes. Nothing is left open, even where the
game's camera never goes (the roofs, the ends of the streets, under the ground). It needs a Jak II
library, from which it reads where the game placed each piece:

```bash
python tools/build_industrial_district.py --out build/demo/ironworks.glb --views build/demo
```

![Ironworks from the air, as capture_view returned it](docs/captures/13-district.png)

| Tool | What it does |
|---|---|
| `get_status` | the games extracted, the document (its levels too), the story view (each level's states), the camera, the selection |
| `list_levels`, `open_level`, `new_level` | the levels of a game (`game`: another game extracted; `near`: the ones the game displays with a level); open one (`actors: false`: its decor alone); start an empty one |
| `add_level`, `remove_level`, `get_level_info` | several levels side by side: add one at its place in the game's world (`actors` too), take it out; a level's neighbors, continue points (positions in meters), role, bounds and root elements in the document |
| `list_catalog`, `list_models`, `list_decor`, `list_prefabs` | what can be placed: objects and effects, models, the decor of every level (searched by name) or of one, prefabs |
| `place`, `place_many` | puts an object, a model, a decor part or a prefab at a position (meters, Y up), turned (`yaw_deg`, or `rotation_deg` on the three axes) and scaled, dropped to the ground if asked; `place_many` places up to 2,000 at once, undone at once |
| `find_nodes`, `get_node`, `raycast` | what the level holds and where: by name, type, kind (nav meshes too) or distance; the ground under a point |
| `transform_nodes`, `duplicate_nodes`, `delete_nodes`, `select`, `set_visibility` | move, turn, scale or stretch (`scale`, `scale_by`), copy, delete, show to the user (`level`: a whole level, to move it), show or hide (a level's group: the whole level) |
| `set_camera`, `capture_view` | look somewhere, or frame elements, a level or the junction of two levels; a picture of the view, given to the assistant |
| `set_story` | the story view of Jak II and Jak 3 levels, each level at its own point: `level`, then `state` (1 to its number of states), `step` or `after_node` |
| `export_level` | the document as a custom level of OpenGOAL for a game (`game`, `name`, `folder`...), adapted to it: the files written and what changed (`notes`) |
| `generate_navmesh`, `export_navmesh` | nav meshes for the whole level, the selection or a box, with the creature's size; only the ground connected to given points (`from`); written for the game |
| `create_prefab`, `save_project`, `open_project`, `undo`, `redo` | the other actions of the editor |

The editor listens on port 47821 of this computer only (127.0.0.1): other computers cannot reach
it. `--remote-port <port>` changes the port (give the same to the editor and to `--mcp`), `0`
turns it off. The status bar shows **Assistant connected** while the MCP server is connected.

## 6. Shortcuts

| Key | Action |
|---|---|
| W, E, R | Move, rotate, scale |
| Click, Shift/Ctrl + click, drag | Select, add to the selection, box selection |
| Ctrl+C, Ctrl+V, Ctrl+D, Del | Copy, paste, duplicate, delete |
| Ctrl+L | Select the whole level of the selection |
| Ctrl+Z, Ctrl+Y | Undo, redo |
| F, Home | Frame the selection, frame the level |
| R, Ctrl + wheel (while placing) | Turn what is being placed |
| Esc | Stop placing, then clear the selection |
| Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S | New level, open, save, save as |
| F1 | Getting started |

## 7. Languages

**View > Language** switches the interface language; the choice is remembered. On the first
start the editor takes the system's language when it has a translation for it, else English.

Each language is a file of `lang/` mapping keys to texts:

```json
{
  "_language": "Français",
  "menu.file": "Fichier",
  "levels.count": "%zu niveaux sur %zu"
}
```

To add a language, copy `lang/en.json` to `lang/<code>.json` (`de.json`, `es.json`...), set
`_language` to the language's name and translate the values. Keep the `%s`, `%zu` and `%d` of
each text, in the same order: `--selftest` checks that every language has every key of English
with the same arguments. A missing key shows its English text.

## 8. Files

### Project (`.glb`)

A regular glTF binary. Each node carries its kind in its extras (`og_kind`: `render`,
`collision`, `actor`, `group`, `navmesh`); instancing is kept (the copies of an element share its
mesh). The levels added to the level opened are groups marked `og_level` (with the level's name),
their companions and mission layers `og_companion` and `og_layer` under them. A nav mesh keeps its
polygons in `og_navmesh` (`verts` relative to the node's position,
`polys`: 3 or 4 corner indices each, `adj`: the polygon across each edge, -1 at a border), and a
fan of triangles as its mesh. An
actor keeps the data of the game (`etype`, `aid`, `game_task`, `bsphere`, `lump`, and the type of
each tag of its lump in `lump_types`), and its model is a `render` child marked `og_actor_visual`. The surface of each collision triangle is kept
(`og_tri_pat_views` in the mesh extras). The level the project comes from is in
`asset.extras.ogle.settings`.

### Nav meshes for the game (`.json`)

What **Export the nav meshes...** and `export_navmesh` write: the `entity-nav-mesh` objects as they
are in the game's memory, in the data blob format of OpenGOAL's level tools (`words`, `pointers`,
`types`, `symbols`, `roots`), the `nav_data` of a level for a custom level builder. Each has its
entity id, its `nav-mesh` (polygons with their vertices relative to the mesh's origin, the route
table, the polygon grid, the bounds) and its tags (`name`, `nearest-y-threshold`, `nav-max-users`).
The polygon grid follows the game's own rule: the grids the editor makes for the game's nav meshes
are the ones the game has, byte for byte.

### Level for the game

What **Export for the game...** and `export_level` write: see
[Exporting for the game](#exporting-for-the-game).

### Prefab (`.glb`)

The same format, holding only the prefab's elements, around a pivot at the bottom center of their
bounds.

### Game data

`data/<game>/game-data.json` is what the editor knows of a game beyond the extracted files. It
holds no asset of the game, only names and rules read in OpenGOAL's decompiled code:

| Part | What it is |
|---|---|
| `actors.roles` | the actor types that are creatures or logic actors (the others are objects) |
| `actors.objects`, `actors.models`, `actors.skeletons` | the objects' families, the models each type draws, the skeleton groups the actors choose by name |
| `actors.effects`, `actors.unlisted` | the particle effects (part-spawner and its subtypes), the logic the catalog does not offer |
| `actors.placeholders` | the types the game's type table keeps for an older game's entities, which spawn nothing of theirs (Jak 3's `cty-fruit-stand`...) |
| `actors.water_looks` | the model of each look of the water actors |
| `oceans` | each ocean map: its corner, height, the 96 m cells it covers, the color of its far ocean |
| `levels` | per level: task level, base task mask, ocean and far ocean, layer or not, companion levels, neighbors (the levels the game displays with it) and continue points (name, position in meters, the levels shown there) |
| `story` | the task nodes in the order of the game (task masks, borrowed layers), and the decor prototypes, actor types and ocean heights that depend on them |
| `enums` | the enums the export for another game maps by name (`pickup-type`) |

`tools/gen_game_data.py` writes it from a checkout of jak-project (read only), for example:

```bash
python tools/gen_game_data.py <jak-project>/goal_src            # the three games
python tools/gen_game_data.py <jak-project>/goal_src jak2       # one game
```

The roles come from the types' ancestry (`enemy`, `process-taskable`... are creatures) with a few
choices made in the script (turrets are objects). Run it again when the decompiled code changes,
then rebuild (the files are copied next to the executable).

### Extracted level

`<level>-background.glb` holds the level's shell (tfrag) in pieces (the large ones, the terrain,
marked `og_terrain`), its decor (one group per prototype whose instances share the mesh), its collision (one node per surface: `collide_pat`, the exact
32-bit value, and `collide_game`) and its actors (nodes with `kind: "actor"` and the actor's
data, with the type of each tag of its lump in `lump_types`). Materials that the PS2 blends additively (glows, light beams) keep their blend in
`extras.og_blend` (`add`, `add_alpha`, `subtract`, `half`). `level.json` lists the actors by type
with the data of one of each (the catalog's templates) and the art-names they choose.

A library extracted before this version of the editor still opens, but extract the game again to
get the lit interiors, the shell in pieces and the catalog's templates.

## 9. Command line

```text
open-goal-level-editor [project.glb] [--library <folder>] [--lang <code>] [--game <game>]
                       [--remote-port <port>]
open-goal-level-editor --screenshot <in> <image.png> [setup] [--size WxH] [--library <folder>]
                       [--lang <code>] [--game <game>] [--camera x,y,z,yaw,pitch]
                       [--story off|<step>|<node>] [--select a,b,...] [--add a,b,...]
open-goal-level-editor --mcp [--remote-port <port>]
open-goal-level-editor --selftest
```

- `--library` chooses the folder of the extracted games (instead of `library/`), `--lang` the
  language, `--game` the game shown (`jak1`, `jak2`, `jak3`), for one session.
- `--screenshot` runs the whole editor in a hidden window and saves a picture of it (the
  screenshots of [the tour](docs/captures/README.md) were made this way). Setups: `levels`,
  `select`, `scale`, `prefab`, `catalog`, `models`, `decor`, `decor-all` (`<in>` is a level name of
  the library), `project` (`<in>` is a project; `select`, `scale`, `prefab` and `navmesh` take one
  too), `navmesh` (the nav meshes and their window; made for the whole level when there are none),
  `export` (the level exported for another game to a temporary folder: the window and its report),
  `extract` (`<in>` is an `.iso` or a disc folder: it is extracted to a temporary folder), `help`.
  `--story` shows the level with every state (`off`), at a step, or just after a task node
  (`canyon-insert-items-resolution`); `--select` names the elements `select`, `scale` and `prefab`
  use (a level added: `select` selects it whole); `--add` adds levels to the one loaded, one after
  the other.
- `--mcp` is the MCP server of [5](#5-building-levels-with-an-assistant-mcp); `--remote-port` the
  port of the editor's remote control (47821, `0` turns it off); `--hidden` runs the editor without
  a window and without changing the preferences (tests of the MCP server).
- The command line modes never change the preferences.

The editor uses the library folder of the last extraction while it holds an extracted game, and
`library/` otherwise. The preferences (library folder, discs, language, recent projects), the
thumbnail cache and what the catalog read of the levels (`index/`) are in
`%APPDATA%/OpenGOAL/open-goal-level-editor` on Windows (`~/.local/share/OpenGOAL/open-goal-level-editor`
on Linux).

## 10. What was tested

- `--selftest`: 67 checks (math, a library written like the extractor writes one, opening its
  level: locked terrain, decor instances, actor with its model, collision surfaces; copy and paste
  with undo; prefab file names, saving, pivot, placing twice with shared meshes, the actors of a
  prefab named apart; project round trip, an empty project too, a stretched element's scale kept,
  the alpha test of a blended material kept; the scale gizmo driven like the
  mouse would (an axis, the center, snapped); a level opened with game data:
  creatures, logic actors and cutscene
  characters left out, the model of a water actor, a companion level and a mission layer, the
  story at two steps and the steps where the level changes, the ocean; the shell in pieces (the
  terrain locked, a piece around its base); the catalog (objects with their model, family and
  data, water by look), the decor parts of a level, the same parts and the art-names of its actors
  read without loading it, a particle effect per group, the decor of every level; an actor copy
  named like the game names them;
  a request and its answer over the remote control; the game data of Jak II; the translations'
  keys and arguments; nav meshes: a floor around a block in the game's limits, the route table
  leading everywhere, the levels' blended decor walkable but not water and glows, only the ground
  next to a point kept, a ground of more than 255 polygons cut in meshes that suit the game, a
  mesh kept by its node and by a project, the blob for the game read back; several levels: one
  added at its place with its companion and mission layer under its group, shown along the story
  as when the player is there, each level at its own point of the story and the steps where an
  added level changes, a level opened without its actors, a level appended to a document with its
  textures and materials shared or renumbered, the neighbors and continue points of the game data;
  the export for the game: collision surfaces as the builder takes them and what is lost, the
  data of actors typed as the builder reads it, the types of the destination (a door changed, a
  placeholder left out), pickups by name, a small level of Jak II exported for Jak 3 (its files, a
  pickup renumbered, a link by name, the story's data left out, the game's collision one node per
  surface, the shell not colliding twice, a placed box colliding by its shape) and for Jak II
  (its types and scripts kept)).
- Jak and Daxter NTSC (*Black Label*, SCUS-97124) and Jak 3 NTSC (SCUS-97330), from disc folders
  of their own: 24 and 272 levels extracted (0.7 and 3.7 GB), 24 and
  105 levels listed; the village of Jak and Daxter with its sea and far sea, Spargus and the
  desert of Jak 3 open.
- The export for the game, through `--mcp` against a hidden editor: the slums of Jak II
  (`ctysluma`, 15,819 meshes, 33,700 collision triangles) for Jak 3 (4 actors kept, 12 of types
  Jak 3 does not have or only as placeholders, a door changed, a pass renumbered), for Jak II;
  Jak and Daxter's village for Jak and Daxter (73 actors) and for Jak II (47 actors, the scout
  flies of its crates left out); Jak 3's desert for Jak II. The files read back: the `.jsonc` parsed, the `.glb` with its
  collision nodes and extras.
- The story of the levels one by one, through `--mcp`: `ctywide` has 68 states among the 324 steps
  (the burning bush missions change it one after the other), `ctysluma` 3, `ctyport` without its
  actors 1; a level at its state while the others stay at theirs.
- Several levels of Jak II: `ctyinda` with `ctyindb` and `ctywide` added (27,160 elements, the
  capture of the tour in 6 seconds; `ctyindb` added in 0.3 s); the streets of the industrial zone
  continue from one level into the other (hidden, `ctyindb` shows where `ctyinda` stops); through
  `--mcp`: `add_level`, `get_level_info`, `list_levels near`, `set_visibility`, the junction
  framed, the adding undone and redone, a second adding refused, the project saved and opened
  again with its levels, `remove_level`, a new level with `ctyinda` added.
- The nav meshes against the game's: the polygon grid the editor makes for each of the 61 nav
  meshes of Jak II's levels (decompiled by OpenGOAL) is the game's, byte for byte. With
  `OGLE_NAV_REFERENCE` set to one of them (`<level>-nav.json`), `--selftest` also checks that the
  game's mesh reads back, passes the editor's checks, that its route leads everywhere and that
  the grid is the same (`ctyinda`, `ctyindb`, `atoll`, `ctygena`).
- The district of `tools/build_industrial_district.py`, built through `--mcp` against a hidden
  editor, then opened in the editor: 15,465 elements, the corners of every block closed from the
  streets, the roofs, the street ends and the underside closed; its nav meshes (8 meshes, 1,323
  polygons, every one suiting the game, none inside a block) and their export for the game.
- Jak II extracted again with this version: the 84 levels of the list open; the insides that were
  black (Hip Hog, garage, oracle, Vin's room, prison...) are lit; the shell of the palace square
  is 274 pieces (26 of terrain), Dead Town's 132; Dead Town's decor has 244 parts.
- The catalog of Jak II against every actor of the 147 extracted levels: every type the game can
  place is there but the creatures and cutscene characters (left out), the types the game does not
  know (Jak and Daxter's village left in `village1`, `city-race-ring`, `dark-jump`) and the 87
  particle actors of `nestb`, which name no particle group (the game shows nothing for them
  either); every model is an object, a model of the Models tab or a character's. The
  first reading of the levels takes about 4 seconds, 0.5 s from the cache.
- The MCP server, driven by a test client through `--mcp` against a hidden editor: every tool,
  from opening `ctypal` to placing a crate on the ground and an arch of Dead Town, moving and
  copying them, a capture of the view, the story view, undo and redo, saving and opening the
  project, a prefab, a new level; an unknown tool gives an error. Then a new level with the
  industrial zone's catwalk found by name among every level's decor, stretched three times along
  its length (48 m instead of 16), scaled to half, both undone, and a particle effect placed.
- The screenshots of [the tour](docs/captures/README.md), made with `--screenshot` and the demo
  level built by `tools/build_demo_level.py` through the remote control (the water of a project
  opened again, which blended materials lost before, is there).
- The game data of Jak II with the extracted levels: the list shows 84 of the 147 levels; the
  palace square (`ctypal`) keeps 29 of its 183 actors and shows the broken statue, the rubble and
  the door of Mar's tomb only after `canyon-insert-items-resolution`; Dead Town (`ruins`) has four
  states (its tower stands until `ruins-tower-exit`), Samos' hut and the ocean; the dark eco pools
  of `nest`, the pools of `ctypal` and the falls of `stadium` are drawn.
- Jak II PAL (SCES-51608), from the folder of the disc: `ogle-extract` writes the 147 levels in
  less than a minute. The editor lists them, loads `atoll` (13,550 nodes, 1.2 million triangles,
  314 actors) and `ctyindb`, selects one decor instance, creates a prefab from six elements and
  places it, in English and in French.
- The extraction window from start to end (disc read, extraction, the Levels panel showing the
  levels), with a small `.iso` made from the same disc.

Not tested yet: the full `.iso` of a disc; the MCP server from Claude Code itself (it was driven by
a test client speaking the same protocol); a level exported by the editor, built by OpenGOAL's
level builder and played in the game (the files follow the builders' code, read for this), and so
nav meshes followed by creatures in the game.

## 11. Known limitations

- **Reflections (envmap) are not drawn**, and the additive blends of the PS2 are approximated:
  water is a translucent blue surface, dark eco a dark purple one.
- **The light is still**: each surface takes the brightest of its level's light layers (Jak and
  Daxter's: noon), where the game mixes them as time goes (the sun, lamps, flickering), and very
  bright surfaces are softened. **View > Fog** only hazes the low-detail backdrop scenery meshes
  (made to be seen through the game's own fog): nothing nearer is fogged.
- **The catalog's objects** come from the extracted levels and the game's code. A new object
  takes the data of one of its type: check it before building the level for the game. The
  particle effects are markers: their particles are not drawn.
- **Scale**: the game draws decor at any scale, but gives most of its actors (objects) their own
  size whatever the level says; the inspector warns when an object is scaled. Collision is not
  scaled either (see below).
- **The story is a line**: the task nodes in the order of the game. Missions the player can do in
  another order are shown in that order, and what the game changes from its scripts (cutscenes,
  some doors and elevators moving) is not shown. The companion levels (Samos' hut in Dead Town)
  are shown at every step: the game only draws the hut in the cutscenes played there.
- **Jak and Daxter has no story view**: its levels change in another way (the game's task
  states), not read yet. Its creatures are left out from a list of types made without testing.
- **Actor models are found by file name** (the type's name, `<type>-...` or `...-<type>`); the
  actors without one are drawn as a marker.
- **Collision does not follow moved elements**: it is kept as the level had it (the export's
  **Made from the decor** makes it from the decor as it is).
- **Several levels**: each is loaded whole (the city of Jak II is many levels: add the ones you
  work on); an added level is shown along the story as when the player is in it, not as the game
  mixes the levels it has loaded; one ocean map is drawn (the level opened's, else the first level
  shown with one); the game's load boundaries (where it loads and unloads levels) are not shown.
- **Nav meshes**: the meshes a large piece of ground is cut into are not linked (the game's
  creatures do not go from one to the next); they do not follow the decor when it moves (generate
  them again); the levels' own nav meshes are not read (the extraction does not take them). The
  file for the game is the `nav_data` a level builder reads: OpenGOAL's Jak II level builder does
  not take nav meshes yet.
- **Animated models** are shown in their rest pose.
- **The export for the game** has not been built with OpenGOAL's level builder nor played yet.
  What it does not do: the code of the objects that live in a level's own files (particle effects,
  a level's logic) is not added to the DGO (`INSTALL.md` says where to find it); the positions
  inside the actors' data (paths, targets) stay the game's when an actor or a level is moved; the
  collision of the levels' own decor is the game's as extracted (a piece moved or deleted keeps its
  collision: export with **Made from the decor**); the builders cannot write Jak II and 3's own
  surfaces (see [Exporting for the game](#exporting-for-the-game)); the scripts kept for the same
  game (`pair` data) need a level builder that reads them; the level's definition keeps the
  test-zone's `:index`, to change; the regions, cameras and ambient sounds of the levels are not
  exported; Jak and Daxter's levels take village1's sky.

## 12. Repository layout

```text
src/core/      math, log, preferences, translations, undo history
src/scene/     the document, picking (BVH), the story view
src/io/        glTF import and export, the library of extracted levels, prefabs, game data, the
               catalog, loading threads, the nav meshes for the game (nav_export), the export of a
               level for the game (game_export)
src/render/    camera, OpenGL renderer, thumbnails, offscreen captures
src/tools/     the gizmo, the nav mesh generator (navmesh: Recast, then the game's limits)
src/app/       window and main loop, document actions, interface panels, extraction window, the
               remote control and MCP server (remote*, mcp.cpp), tests
lang/          translations of the interface (en.json, fr.json)
data/          what the editor knows of each game (game-data.json, made by tools/gen_game_data.py)
tools/         gen_game_data.py: writes data/ from OpenGOAL's decompiled code; build_demo_level.py:
               builds the tour's demo level through the editor's remote control;
               build_industrial_district.py: builds the tour's district through the MCP server
library/       the levels extracted from your game (created by the extraction, ignored by git);
               library/projects/ is where a new project is saved by default
iso_data/      put your own disc images or disc folders here for the extraction to read (ignored
               by git, only a .gitignore of its own is kept)
extractor/     ogle-extract: main.cpp, the games' tables (data/) and the data readers of OpenGOAL's
               decompiler it is built from (opengoal/, ISC license)
third-party/   Dear ImGui, glad, tinygltf, nlohmann json, stb, Recast
docs/captures/ the guided tour and its screenshots, and how to make them again
.mcp.json      the MCP server, for Claude Code started in this folder
```

Neither OpenGOAL's game runtime, nor its compiler, nor its REPL are in the repository: only the
editor and the extractor it needs.

## 13. License

ISC (see [`LICENSE`](LICENSE)), the license of OpenGOAL.

This editor is built on the work of the [OpenGOAL](https://github.com/open-goal/jak-project) team:
its extractor is made of the data readers of OpenGOAL's decompiler (`extractor/opengoal/`, OpenGOAL
code under the ISC license: `extractor/opengoal/LICENSE`), its game data is generated from
OpenGOAL's decompiled game code, and what it knows of the games' formats comes from their research.
It is an independent project: it is not affiliated with the OpenGOAL team, nor endorsed by it, and
it reuses their license, the ISC license.

Third-party code keeps its own license, listed in `third-party/README.md`. Jak and Daxter, Jak II
and Jak 3 belong to Sony Interactive Entertainment and were developed by Naughty Dog: no data of
the games is in the repository; the screenshots of `docs/captures/` show levels of Jak II, Jak and
Daxter and Jak 3 extracted from copies of the games.
