# OpenGOAL Level Editor

A level editor for **Jak 3**, for the [OpenGOAL](https://github.com/open-goal/jak-project) port of
the game. It reads the levels from **your own copy of the game**, shows them as the game draws them,
and lets you change them or build new ones: move a wall, add a bridge, put houses of Haven City in
the desert, make a door open only once a crate is broken, then export the result as a level you can
play in the game.

You do not need to have used a level editor before. This guide explains each idea the first time it
appears, with an example, and shows each tool in a picture.

![The editor: the levels on the left, the level in the middle as the game draws it, the inspector on the right](docs/captures/getting-started/window.png)

- **Nothing of the game is in this repository.** Everything comes from your disc: you extract it once.
- **It needs nothing else**: not jak-project, not the game, to build or to run. You only need
  jak-project to *play* what you exported.
- **An AI assistant can build for you** (Claude Code or any MCP client): see
  [the guide for AI agents](docs/mcp-agent-guide.md).
- The interface is in **English and French** (View > Language).

## Contents

1. [The ideas in five minutes](#1-the-ideas-in-five-minutes)
2. [Install](#2-install)
3. [First steps](#3-first-steps)
4. [Selecting and moving things](#4-selecting-and-moving-things)
5. [Groups](#5-groups)
6. [The catalog: everything you can place](#6-the-catalog-everything-you-can-place)
7. [Prefabs](#7-prefabs)
8. [Building your own pieces](#8-building-your-own-pieces)
9. [Closing buildings](#9-closing-buildings)
10. [Holes in the decor](#10-holes-in-the-decor)
11. [Seeing it as the game does: sky, hour, weather](#11-seeing-it-as-the-game-does-sky-hour-weather)
12. [Levels side by side, and the story](#12-levels-side-by-side-and-the-story)
13. [What objects do](#13-what-objects-do)
14. [Where creatures walk: nav meshes](#14-where-creatures-walk-nav-meshes)
15. [Saving](#15-saving)
16. [Playing it in the game: export](#16-playing-it-in-the-game-export)
17. [Building with an AI assistant](#17-building-with-an-ai-assistant)
18. [Keys and mouse](#18-keys-and-mouse)
19. [Files and folders](#19-files-and-folders)
20. [More](#20-more)

## 1. The ideas in five minutes

**A level** is a part of the game's world that the game loads as one piece: a district of Haven
City (`ctyinda`, the industrial zone), the slums (`ctysluma`), Spargus (`wascitya`), a part of the
desert... Jak 3 has about a hundred of them. They all live in **one world**: each level is at its
place, so two levels side by side in the game also meet in the editor.

A level is made of **elements**. There are two main kinds:

| | What it is | Examples | What it does in the game |
|---|---|---|---|
| **Decor** | something drawn: the level's scenery | a wall, a roof, a pipe, a lamp post, the ground | nothing by itself: Jak walks on it, bumps into it |
| **Objects** (the game calls them *actors*) | something with its own code | a door, a crate, an elevator, a steam vent, a jump pad | they move, open, break, throw Jak up |

The creatures (enemies, characters, animals) are left out: the editor builds places, not
missions.

**The light** of a level is painted into its decor by the game's artists: every corner of every
piece carries its colors for the sun from four directions, the ambient light and the lamps. The
game mixes them according to the **hour of the day** and the **weather**. The editor's **game
rendering** draws the level exactly so: what you see is what the game will show.

**A prefab** is a set of elements saved together to be placed again anywhere, like a stamp: a
house, a fountain, a market stall. The editor also makes **the game's prefabs**: whole buildings,
towers and bridges taken from the game's levels.

**A facade** is how most buildings of the game are made: only a front, the side the player sees,
with nothing behind. In the game the player never gets behind. In a level of your own he might: the
editor **closes** such buildings for you ([9](#9-closing-buildings)).

**The void** is what is behind everything: nothing. A gap between two pieces lets the player see
it, a visual bug. The editor **finds those holes** ([10](#10-holes-in-the-decor)).

**Exporting** writes your level in the files OpenGOAL's level builder reads: you copy them into
jak-project, build, and play ([16](#16-playing-it-in-the-game-export)).

## 2. Install

### What you need

- Windows 10/11 or Linux, 64-bit, a graphics card with OpenGL 3.3 (4.1 for the game rendering).
- **Your copy of Jak 3** for the PlayStation 2: the disc image (`.iso`) or the folder of the disc.
  Put it in `iso_data/` of this folder if you like (git ignores it), or anywhere.
- About 4 GB of disk for the extracted levels, 5 GB of free memory while they are extracted.
- Optional: your copy of **Jak II**, for its buildings (Haven City as it was, the palace).

### Build it

You need CMake 3.20+, Ninja and a C++20 compiler (tested with Clang 19 on Windows); the first build
downloads SDL3 (a few minutes).

```bash
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
./build/bin/open-goal-level-editor --selftest   # the editor's own tests: "OK (0 failures)"
./build/bin/open-goal-level-editor              # start it
```

Keep the files of `build/bin/` together (the editor, `ogle-extract`, `data/`, `lang/`,
`game-shaders/`).

### Extract your game

The first time, the editor asks for your game: **File > Extract game assets...**

1. **Your Jak 3 disc**: choose the `.iso` (**.iso file...**) or the folder (**Folder...**). The editor
   tells which game and version it is (for example *Jak 3 - ntsc_v1 - SCUS-97330*).
2. **Where to write the levels**: `library/` in this folder by default (git ignores it).
3. **Extract all the levels**: about a minute and a half for the 272 levels of Jak 3.

![The extraction window after Jak 3's 272 levels were extracted](docs/captures/getting-started/extraction.png)

A disc of Jak II is extracted the same way (to `library/jak2/`). The extracted files come from your
game: keep them on your computer, do not share them.

## 3. First steps

### The window

```text
+-----------------------------------------------------------------------------+
| File  Edit  View  Build  Level  Help                                        |
| [Move] [Rotate] [Scale] [Topology] [Terrain] [Local] [Snap] | [Copy] ...   |
+------------------+--------------------------------------+-------------------+
| Levels | Build | |                                      | Inspector         |
| Catalog |        |              the 3D view             | (what is selected,|
| Prefabs | Groups |                                      |  or the level)    |
|                  | [Story] < ======o======== >          |                   |
+------------------+--------------------------------------+-------------------+
| what the mouse does here            |  the last message                      |
+-----------------------------------------------------------------------------+
```

- The **left panel** has the steps of the work: **Levels** to open one, **Build** to make shapes,
  **Catalog** and **Prefabs** to place things, **Groups** to find the groups you made.
- The **inspector** (right) shows what is selected and lets you change it precisely; with nothing
  selected, it shows the level.
- The **status bar** says what the mouse does where it is, and the last thing that happened.

### The guide

**F1** (Help > Tutorials) opens the guide: named tutorials, from *Getting started* (from your disc to
the game, step by step) to one per tool, each with a short animation. Hovering any tool also shows
what it does, its keys and an example.

![The guide: Getting started at its fifth step, the animations of the move, rotate and scale tools](docs/captures/getting-started/guide.png)

### Open a level

In **Levels**, choose a level (type `cty` for Haven City's districts, `des` for the desert), then
**Load the level** (or double-click it). **With its actors** brings its objects too (doors, crates);
unchecked, you get its decor alone.

![The Levels panel: the slums of Haven City loaded](docs/captures/levels/levels-panel.png)

What comes with a level: its decor, its objects, its water (pools, fountains, dark eco, lava drawn
as such), the ocean where the level has one, and the levels the game always shows with it.

### Move around

| To | Do |
|---|---|
| Fly | hold the **right button** and use **W A S D** (Z Q S D on AZERTY), **Q / E** down and up |
| Go faster / slower | **Shift** / **Ctrl** while flying; the **wheel** while flying sets the speed |
| Move forward | the **wheel** |
| Slide the view | hold the **middle button** |
| Turn around a point | **Alt + left button** |
| Frame | **F**: the selection; **Home**: the whole level |

### A new level

**File > New level** (Ctrl+N) starts an empty level, with no ground at first. Build it from the
catalog, the prefabs and the shapes: a floor first (a **Ground** shape, or the terrain brush), then
the rest on it.

## 4. Selecting and moving things

### Select

- **Click** an element to select it. Each piece of decor is its own element: clicking a pipe
  selects that pipe, not every copy of it.
- **Before you click, the cursor tells you** what it points at: the element is outlined, and after a
  moment a label gives its name and, when it is part of one, its prefab or group with how many
  elements it has.
- **Shift + click** or **Ctrl + click** adds an element to the selection (or takes it out).
- **Click again at the same place** to reach what is behind or under: the second click selects
  the next element under the cursor.
- The **ground** is locked, so that clicks go to what stands on it (View > Select the ground
  unlocks it).

![The cursor over a building: its piece outlined, the whole prefab it belongs to outlined faintly, and the label](docs/captures/selecting/hover.png)

### Select many at once

- **Drag a box** from an empty spot: what it would select is **outlined while you drag**, and the
  label next to the cursor counts it. **Shift** while dragging adds it to the selection, **Ctrl**
  takes it out.
- **Double-click** an element, or **hold the left button still on it** (a ring fills around the
  cursor): its **whole prefab or group** is selected. An element in no group gives its **assembly**:
  the whole structure it is part of (a house with its facades, roof and windows).
- **Ctrl+G** selects the assembly, **Ctrl+L** the whole level.

| A box being drawn: 156 elements, outlined | The button held on a prefab: in a moment, all of it |
|---|---|
| ![box](docs/captures/selecting/box.png) | ![hold](docs/captures/selecting/hold.png) |

Example: you placed a house made of 155 pieces; click it once and you get one window frame;
double-click it, and you have the whole house, ready to move.

### Move, turn, stretch

The **gizmo** appears on the selection: arrows to move it along an axis, squares to move it on a
plane, rings to turn it, squares at the end of the axes to stretch it.

- **W** move, **E** rotate, **R** scale. While dragging, the value is written next to the cursor;
  **Shift** is ten times finer, **Ctrl** snaps.
- **Snap** (toolbar) moves by 0.5 m, turns by 15 degrees, scales by 0.1: walls placed side by side
  meet exactly. The arrow next to it changes the steps.
- **Local** moves along the element's own axes (a catwalk set diagonally slides along its length).
- **Drag the selection itself**: press on an element of the selection (not on the gizmo) and move
  the mouse: left and right slide it across the view, up and down raise or lower it (**Ctrl**:
  forward and back instead), Shift finer. A click on an element of a selection of several keeps
  them all selected; click it again to take it alone.
- **Hold the right button on the selection** and move the mouse to turn it on every axis.
- The inspector gives the exact position, rotation and scale.

| Moving a selected prefab with the mouse | A bridge stretched along its length with the scale gizmo |
|---|---|
| ![moving](docs/captures/selecting/moving.png) | ![scale](docs/captures/selecting/scale.png) |

Example: a platform that Jak cannot quite reach: select it, **W**, drag its up arrow by 2 m, and
the player needs a double jump.

### Copy, delete, undo

**Ctrl+C / Ctrl+V** copy and paste (the copies appear where the originals were, selected: move
them), **Ctrl+D** duplicates, **Del** deletes, **Ctrl+Z / Ctrl+Y** undo and redo. Everything can be
undone.

## 5. Groups

A **group** keeps elements together under a name. Every prefab you place is one (named after the
prefab); you can make your own.

- **Make one**: select the elements, then **Ctrl+Shift+G** (Edit > Group the selection...), and
  name it.
- **Use it**: a double click (or the button held) on any of its elements selects it all; it moves,
  turns and copies as one. Copying a group makes a new group.
- **The Groups panel** lists them ([P] a prefab placed, [G] a group of yours): click to select,
  double-click to frame, right-click to rename or **Ungroup**.
- The inspector shows the group of the selection, with **Select it all** and **Ungroup**.

| The Groups panel, a group selected | Naming a new group |
|---|---|
| ![panel](docs/captures/groups/panel.png) | ![new](docs/captures/groups/new-group.png) |

Example: a little square made of a fountain and two trees, grouped as "Little square", copied to
the other end of the street in two clicks.

## 6. The catalog: everything you can place

The **Catalog** panel lists what you can put in a level. Click an entry, then click in the view to
place it (or drag it into the view). Hovering an entry shows what it is.

- **Objects**: what makes up a place: props that just stand there (flags, windmills, a parked
  hellcat), doors and airlocks, water, dark eco and lava.
- **Interactive**: the objects that do something: crates, platforms and elevators, jump pads and
  board vents, buttons, breakable walls and fences, hazards. Their behavior is set in the
  inspector ([13](#13-what-objects-do)), and its **Preview** shows them moving in place (a door
  opening, a fence breaking).
- **Effects**: the game's **visual effects** in a category of their own, sorted by kind: **fire and
  flames**, **smoke, steam and dust**, **lights and glows**, **sparks and electricity**, **water**
  (drips, splashes, falls), and the others (explosions, trails). Each runs in the view as the game
  makes it.
- **Models**: the other models of the game (parts of objects, debris, props), placed as decor.
- **Decor**: the pieces of the levels' scenery (a pillar, an arch, a catwalk), searched by name
  across every level, or by level.
- **Materials**: every texture of the levels: select shapes or pieces, click a texture, they take
  it.

| Interactive objects | The visual effects and their kinds |
|---|---|
| ![interactive](docs/captures/catalog/interactive.png) | ![effects](docs/captures/catalog/effects.png) |

| The decor of every level searched for "catwalk" | Materials |
|---|---|
| ![decor](docs/captures/catalog/decor-search.png) | ![materials](docs/captures/catalog/materials.png) |

Placing: a click puts it where the cursor points, on what is under it. **Keep the left button
down** and move the mouse to move it before it lands: left and right across the view, up and down
to raise or lower it (**Ctrl**: forward and back), Shift finer; it lands when you let go. Once
placed it **stays selected**, the gizmo on it (all of a prefab), so you can adjust it at once; hold
**Shift** when you let go to go on placing copies. **R** or **Ctrl + wheel** turns it, the right
button held turns it every way, **Esc** stops.

Example: steam over a vent of your industrial yard: Catalog > Effects > *Smoke, steam and dust*,
click `ctyindb-steam-drift`, click on the vent.

Never offered: the creatures, the collectibles (orbs, gems, ammo) and the technical objects
(cameras, triggers).

## 7. Prefabs

### Place one

The **Prefabs** panel has **the game's prefabs** (top) and **yours** (bottom). Click one, then click
in the view; it sits on the surface it is placed on.

**The game's prefabs** are whole structures found in the game's levels, by game, level and
category: houses and buildings of the slums, Spargus and Haven, towers, temple gates and walls,
bridges, platforms, machines of the factories, palms and rocks of the desert, statues, and Jak II's
(Haven City's buildings, its palace) when Jak II is extracted. They are made from your levels the
first time you show them.

![The game's prefabs searched for "gate"](docs/captures/prefabs/search.png)

### Make one

1. Select what goes together. For a building of the game, click one of its walls and press
   **Ctrl+G**: its whole assembly is selected.
2. **Create a prefab** (toolbar, inspector or Edit menu) and name it.
3. **Leave out the pieces smaller than 0.5 m** is checked: a prefab is a structure, and loose
   bolts, debris, stains and tiny props would only clutter it. The window says how many will be left
   out. The objects of the game (a door, a crate) are always kept.

It is saved in `library/prefabricated/` and appears under **My prefabs**.

| A house of Spargus selected by its assembly (Ctrl+G) | Making it a prefab: the small pieces left out |
|---|---|
| ![assembly](docs/captures/prefabs/assembly.png) | ![create](docs/captures/prefabs/create.png) |

A prefab of a building the player will walk around should be **closed** first
([9](#9-closing-buildings)).

## 8. Building your own pieces

### Shapes

The **Build** panel makes what the game does not have: a block, a floor, a wall, a **wall with a
door**, a **wall with a window**, a pillar, a ramp, stairs, roofs, a cylinder, a **ground** (a floor
whose top goes up and down). Click one, then click in the view. In the inspector: its **size** in
meters, how often its texture repeats (**Texture every**: a larger wall repeats its texture instead
of stretching it), **Bend** (a wall bent 90 degrees is a quarter of a round tower, stairs bent 180 a
spiral staircase) and rounded **Corners**.

**In the style of a level**: choose a level, and **Place a floor** or **Place a wall** gives a
floor or a wall that looks like it (its textures, their size, its light, how uneven its ground is).

| The Build panel | A yard in the style of the port: its cobbles, a bent wall, a round pillar, a spiral staircase |
|---|---|
| ![shapes](docs/captures/building/shapes.png) | ![style](docs/captures/building/style.png) |

### The terrain brush

**Terrain (B)** draws natural ground freehand, in its own small window: a square follows the mouse;
hold the left button to **paint** ground, **raise** or **dig** it, **smooth** or **flatten** it,
raise **walls**; **Generate hills** makes hilly ground at once. **Right button + wheel** raises or
digs under the square. The ground takes the textures of a level's style automatically (the
desert's sand and rock, the slums' cobbles).

![The terrain brush in the style of the desert: hills, a riverbed dug, a walled yard, a hill raised](docs/captures/building/terrain.png)

### The shape of one piece: topology

**Topology (T)** shows the points and faces of one element: move points with the gizmo, **Extrude**
faces (a step out of a floor), **Subdivide**, **Flip**, **Delete**.

![A floor's faces picked and pulled out 1 m with their walls](docs/captures/building/topology.png)

### See what an object does: the preview

The inspector's **Preview** plays an object's animation where it stands (a door opening), its
movement (an elevator), or shows it **breaking** (a fence of the factory thrown apart).

![The preview of a breakable fence](docs/captures/building/preview.png)

### Jak II's buildings

With Jak II extracted, choose **Jak II** at the top of the Catalog or the Levels panel: its models,
its decor (Haven City as it was), its prefabs, or a whole level of Jak II opened as a level of Jak 3
(its collision kept). What could go wrong is checked when it comes in (textures renamed
`jak2-<name>`, light compared with the level's: **Match the lighting** in the inspector).

![The atoll of Jak II opened as a level of Jak 3](docs/captures/building/jak2-level.png)

### The light of what you place

What you place, paste or move **takes the light of the level around it**: each of its corners
borrows the light of the level's nearby surfaces facing the same way. A piece from a dark cave
placed in a sunny street does not stay dark. **Light it like here** (inspector) does it again.

## 9. Closing buildings

**Why.** Most buildings of the game are **facades**: a front made to be seen from the street, with
nothing or a few open panels behind. In the game the player never sees their back. In your level,
if the player can walk around one, he sees the inside of its panels, pieces floating, or the void:
ugly, and obviously unfinished.

**What closing does.** Select the building (a prefab placed: double-click it), **look at its front
the way the player will** (the camera in the street), then **Edit > Close the building (fill its
back)**. The editor adds one element that **fills it**:

- it reads the building's footprint and height on a grid aligned with its walls;
- it fills what is **hidden behind and inside the building as seen from the camera**: what you see
  from there (a shop's alcove, a courtyard facing the street) stays open;
- it goes as deep behind a facade as a building of its width, as high as the building where it is,
  **lower terrace by terrace going back**, with **pilasters and a cornice** on its new walls: it
  follows the building's shape instead of being a plain box;
- it uses the building's own textures and light, and joins its group (they move together).

| | From the street | From behind | From the side |
|---|---|---|---|
| Before | ![front](docs/captures/closing-buildings/before-front.png) | ![back](docs/captures/closing-buildings/before-back.png) | ![side](docs/captures/closing-buildings/before-side.png) |
| After | ![front](docs/captures/closing-buildings/after-front.png) | ![back](docs/captures/closing-buildings/after-back.png) | ![side](docs/captures/closing-buildings/after-side.png) |

Example: the facade above (a prefab of the industrial zone) is a shop front with a sign on a
column. From the street nothing changes; from behind, the open frame became a building of its
height, with a setback halfway back.

When **not** to close: a facade the player can never get behind (against the edge of the level,
backed onto another building). Then just make sure, by walking around in the view, that its back
never shows.

![Edit > Close the building](docs/captures/closing-buildings/menu.png)

## 10. Holes in the decor

**Why.** Behind the decor there is nothing. A floor that stops 30 cm short of a wall, two slabs not
quite touching, a wall that misses the corner: through each gap the player sees the void, the
screen's background. It is the most common visual bug of a level made by hand.

**Find them.** **Level > Find holes in the decor...** looks from the ground every few meters, at the
height of Jak's eyes, all around and downward (the whole level, or around the selection), and lists
what it found, drawn in the view:

- **red**: a **gap** between pieces, to fill;
- **orange**: an **open edge**, where the ground ends in the void: close it (a wall, a railing,
  buildings) unless the player can never get near it.

Click a line of the list: the camera goes where the player would see it from.

![A yard: the slit between two floor slabs and the corner the side wall misses (red), its open front (orange)](docs/captures/holes/window.png)

**Fix them**: stretch a piece over the gap (scale), move it to overlap its neighbor by a few
centimeters, or place a floor or a wall over it, then look again.

## 11. Seeing it as the game does: sky, hour, weather

**The game rendering** (View > Game rendering, **G**), on from the start, draws the level with the
game's own renderer: its light at the hour and in the weather you choose, its fog, its sky. **G**
switches to the editor's own view, where every light is on (handy in dark corners).

**View > Sky, hour and weather...** opens a small window of its own:

- **The sky**: half a turn around where the camera looks (the whole turn makes you lose your
  bearings), with its horizon and the four directions: the sun or the moon, the clouds, the stars.
- **The hour**: a slider, and **Dawn**, **Noon**, **Dusk**, **Night**. **Time passes** lets it run.
- **The weather**, as the game has them: clear sky, a few clouds (the game's start), haze, smog,
  overcast, overcast and foggy, rain, storm, storm with rain, storm and foggy; or your own with the
  **Clouds**, **Fog / rain** and **Rain** sliders. **Lightning in a storm** flashes the sky and the
  level.
- **What it changes**: the sun dims and the light greys under clouds, the fog closes in with haze
  or rain, the lamps of the level light up at night, some effects only show at some hours, and all
  of them fade in the fog far away. The window sums it up (*the sun is up, its light 0.92; the fog
  closes in at 1000 m; the lamps are off*).

| Noon, clear sky | Dusk, a few clouds: the lamps on |
|---|---|
| ![noon](docs/captures/game-rendering/noon-clear.png) | ![dusk](docs/captures/game-rendering/dusk.png) |

| Storm with rain | Night: stars and moon |
|---|---|
| ![storm](docs/captures/game-rendering/storm.png) | ![night](docs/captures/game-rendering/night.png) |

The **effects** (steam, sparks, smoke, fire, lights) run in the view (View > Effects), and the
**ocean** moves, its waves catching the sun.

![The steam of the industrial zone's vents, with the labels of their effects](docs/captures/game-rendering/effects.png)

## 12. Levels side by side, and the story

**Several levels.** With a level open, choose another in Levels and **Add to the document**: it
comes at its place in the world. To work on the junction between two levels, or to build a new
level next to the game's. The inspector lists them (show, hide, frame, remove) and the levels
**around it in the game** (click one to add it). **Select** next to a level selects it whole, to
move it.

| The industrial zone, the city walls and the palace together | A level selected whole |
|---|---|
| ![several](docs/captures/levels/several-levels.png) | ![selected](docs/captures/levels/level-selected.png) |

**The story.** Some levels change during the game: Haven City's Metal Head tower is blasted, the
forest's statue falls. The bar under the view shows a level at a point of the story; **Story**
unchecked shows every state at once.

![The Metal Head tower after its destruction](docs/captures/levels/story.png)

## 13. What objects do

Select an object: the inspector's **Behavior** says what it does, and you can change it.

| Object | You choose |
|---|---|
| Doors and airlocks | when it opens: when Jak comes near, never (locked), always, or when Jak comes near **once a condition holds**; from how far |
| Jump pads, board vents | when they throw Jak up, how high |
| Elevators | when they leave, how fast |
| Crates | which crate (wood, iron, steel...) |
| Every object | whether it is there at all (always, never, once a condition holds) |

A **condition** is: after a step of the story, once a switch is pressed, or once an object is gone
(a crate broken, an enemy killed). **Parameters** lists every setting the object's code reads, with
its default.

![An airlock that opens when Jak comes within 10 m, once a crate is broken](docs/captures/behaviors/airlock.png)

Example: a door that opens only once the crate in front of it is broken, so the player has to find
a way to break it first.

**The decor** can do things too (inspector, **In the game**): collide or not, sound like stone or
metal under Jak's feet, hurt or burn him, **bounce** him up, **launch** him along an arc.
[`docs/behaviors.md`](docs/behaviors.md) explains what the game allows.

## 14. Where creatures walk: nav meshes

The game's creatures walk on **nav meshes**: flat polygons over the ground. **Level > Nav meshes**
makes them from the level's walkable surfaces, for a creature of the size you set, for the whole
level or around the selection; they are drawn over the level, one color each, and saved with the
project and the export.

![The nav meshes of the slums](docs/captures/nav-meshes/navmesh.png)

## 15. Saving

**File > Save** (Ctrl+S) saves your work as a **project**, only when you ask, in its own folder:
`library/projects/<name>/<name>.glb` (a standard 3D file Blender opens too) and its nav meshes.
**File > Open project** (Ctrl+O) and **Recent projects** open it again. Nothing is saved behind
your back.

## 16. Playing it in the game: export

**Level > Export for the game...** writes the level as a **custom level of OpenGOAL**, in
`library/export/<project>/`, laid out like jak-project, with a `README.md` that gives the exact lines
to add and how to build and go there.

![The export window and its report](docs/captures/export/export-window.png)

- Choose its **name** (3 to 10 letters) and **nickname** (3 letters), where the player starts, the
  collision (the game's own where its decor is still in place; made from the shape for what you
  placed).
- The **behaviors** you set are written as the level's own code.
- The **report** says what changed and where the collision might be too dense for the game.

**Replace a level of the game**: a level made from one of the game's (say `ctyindb`) can take its
place, so that the game loads yours wherever it loaded the original, with its continue points,
music and missions. **Level > Replace a level of the game...**: choose it, **show the original** (a
blue ghost to compare), **Place it where the original is**, check where the game starts the player
(green: ground under it), and **Export in place of ...**.

![A level made the replacement of ctyindb: the inspector's section](docs/captures/export/replace.png)

Then, in your jak-project checkout: copy the export's folders, add its lines, build (`(mi)` in the
compiler) and play. The export's `README.md` gives each step.

## 17. Building with an AI assistant

An AI assistant can build in the editor for you, through its **MCP server**
(`open-goal-level-editor --mcp`): you describe the level, it opens it, places the decor, closes the
buildings, checks the holes, looks at the result and shows it to you, in your window, every change
undoable.

- **Claude Code**: start it in this folder (it reads `.mcp.json`) and accept the server.
- Then ask, for example: *"Open the slums, build a little square with a fountain and two trees in
  front of the tower, and show me a picture at dusk"*.

**[The guide for AI agents](docs/mcp-agent-guide.md)** explains the tools and the rules an agent
follows (no holes, no crumbs in prefabs, facades closed, look before reporting), with recipes.

![What an assistant built and saw: a street of Jak II's Main Town, its buildings closed at the back](docs/captures/mcp/assistant-view.png)

## 18. Keys and mouse

| Key | Action |
|---|---|
| W, E, R | Move, rotate, scale |
| T, B | Topology tool, terrain brush (again or Esc: done) |
| Click, Shift / Ctrl + click | Select, add to (or take out of) the selection |
| Double click, left button held still | Select the whole prefab or group (else the assembly) |
| Drag from an empty spot | Box selection (Shift: add, Ctrl: take out) |
| Drag from a selected element | Move the selection (Ctrl: forward / back, Shift: finer) |
| Click again at the same place | The next element under the cursor |
| Right button held on the selection | Turn it every way |
| Ctrl+G, Ctrl+L | Select the assembly, the whole level |
| Ctrl+Shift+G | Group the selection |
| Ctrl+C, Ctrl+V, Ctrl+D, Del | Copy, paste, duplicate, delete |
| Ctrl+Z, Ctrl+Y | Undo, redo |
| F, Home | Frame the selection, the level |
| G | Game rendering on / off |
| While placing: left button held, R / Ctrl + wheel, Shift on release | Move it before it lands; turn it; keep placing copies |
| Esc | Stop placing, then clear the selection |
| Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S | New level, open, save, save as |
| F1 | The guide |

## 19. Files and folders

```text
library/                 everything made from your game (git ignores it)
  jak3/                  the extracted levels of Jak 3 (jak2/: Jak II's)
  prefabricated/         the prefabs: yours, and the game's in prefabricated/game/
  projects/<name>/       your saved projects
  export/<project>/      the levels exported for the game
iso_data/                put your disc images here if you like (git ignores them)
data/                    what the editor knows of the game: names and rules, no asset
lang/                    the interface texts (en.json, fr.json)
docs/                    this documentation's other pages and its screenshots
src/, extractor/         the editor and the extractor
tools/                   scripts that regenerate data/ and the screenshots
.claude/skills/          skills for Claude Code working on this repository
```

Only `library/prefabricated/.gitkeep` of `library/` is in git: prefabs, projects and exports hold
the game's data and stay on your computer.

## 20. More

- [Technical reference](docs/reference.md): the command line, the file formats, the game data, the
  details of the export and the known limitations.
- [What the decor can do in the game](docs/behaviors.md): a study of the game's code.
- [The guide for AI agents](docs/mcp-agent-guide.md).
- [How the screenshots are made](docs/captures/README.md).

### License

ISC (see [`LICENSE`](LICENSE)), the license of OpenGOAL. This editor is built on the work of the
[OpenGOAL](https://github.com/open-goal/jak-project) team: its extractor is made of the data readers
of OpenGOAL's decompiler, its game rendering of the game's renderer (`extractor/opengoal/`, ISC
license), its game data is generated from OpenGOAL's decompiled code. It is an independent project,
not affiliated with the OpenGOAL team nor endorsed by it. Third-party code keeps its own license
(`third-party/README.md`). Jak 3 belongs to Sony Interactive Entertainment and was developed by
Naughty Dog: no data of the game is in this repository; the screenshots show levels extracted from a
copy of the game.
