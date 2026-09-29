# A guided tour of the OpenGOAL Level Editor

This tour shows, picture by picture, how the editor works and why it works that way: from your
game disc to a level you built yourself, by hand or with an AI assistant, and exported for the
game. Every picture was made by the editor itself (its `--screenshot` mode, see
[the last part](#making-these-screenshots-again)), with Jak II (PAL) and the interface in English,
and two with Jak and Daxter and Jak 3 (NTSC).

1. [The approach](#the-approach)
2. [Extract your game](#1-extract-your-game)
3. [Open a level](#2-open-a-level)
4. [See it along the story](#3-see-it-along-the-story)
5. [Select and edit](#4-select-and-edit)
6. [Stretch, grow, shrink](#5-stretch-grow-shrink)
7. [The catalog](#6-the-catalog)
8. [Prefabs](#7-prefabs)
9. [Build a new level](#8-build-a-new-level)
10. [Let an assistant build it](#9-let-an-assistant-build-it)
11. [A whole district, by an assistant](#10-a-whole-district-by-an-assistant)
12. [Nav meshes: where the creatures walk](#11-nav-meshes-where-the-creatures-walk)
13. [Several levels, side by side](#12-several-levels-side-by-side)
14. [Export it for the game](#13-export-it-for-the-game)
15. [Save, and find help](#14-save-and-find-help)
16. [Making these screenshots again](#making-these-screenshots-again)

## The approach

The editor holds nothing of the games: it reads them from **your own copy**, turns them into
standard 3D files once, and edits those.

```mermaid
flowchart LR
    disc["Your disc<br/>(.iso or folder)"] --> extract["ogle-extract<br/>(OpenGOAL's readers)"]
    extract --> library["library/<br/>levels, models, textures<br/>as glTF files"]
    code["OpenGOAL's decompiled<br/>game code"] --> gen["tools/gen_game_data.py"]
    gen --> data["data/game-data.json<br/>creatures, story, water,<br/>oceans, models"]
    library --> editor["The editor"]
    data --> editor
    editor --> project["Your level<br/>(project .glb, prefabs)"]
    editor --> export["A custom level of OpenGOAL<br/>for Jak 1, Jak II or Jak 3"]
    assistant["AI assistant<br/>(Claude Code...)"] <-->|"MCP tools"| editor
```

- **Extraction.** `ogle-extract` is made of the data readers of
  [OpenGOAL](https://github.com/open-goal/jak-project)'s decompiler. It reads the levels of your
  disc and writes each one as a [glTF](https://www.khronos.org/gltf/) file (`.glb`), the standard
  that Blender and most 3D tools open: its shell cut in pieces, its decor (one mesh per prototype,
  shared by all its copies), its collision and its actors (objects, with the data the game gives
  them). The models of the actors and the textures go next to it. This happens once.
- **Game data.** Some knowledge is not in the level files: which actors are creatures, which parts
  of a level appear or vanish as the story goes, which model an object draws, where the ocean is.
  `tools/gen_game_data.py` reads it from OpenGOAL's decompiled game code and writes
  `data/<game>/game-data.json`, which comes with the editor (it holds names and rules, no data of
  the game).
- **Editing.** A level opens as a scene of elements you can select, move, rotate, scale, copy and
  delete; every change can be undone. What you build is saved as a project: a `.glb` file too.
- **The catalog** gathers what can be put in a level: every object of the game, every other model
  and the decor of every level, found from the extracted files and the game data.
- **The assistant** drives the same editor through the Model Context Protocol (MCP): its tools are
  the editor's own actions (open, place, move, look, save), so everything it does can be seen and
  undone in the window.

## 1. Extract your game

![The extraction window after the extraction of Jak II](02-extraction.png)

**File > Extract game assets** opens this window. Choose the game (1), point to your disc (2), an
`.iso` file or the folder of a disc already copied: the editor recognizes the game, its version and
its serial number (here Jak II, PAL, SCES-51608). Choose where the levels go (3), `library/` in the
editor's folder by default, and click **Extract all the levels**. For Jak II, the 147 levels of the
disc took 54 seconds.

The disc can be anywhere: nothing has to be copied to a particular folder, but the dialog opens on
`iso_data/` in the editor's own folder by default (`iso_data/jak1`, `iso_data/jak2`,
`iso_data/jak3`, or however you like: it is ignored by git), ready if that is where you put yours.
Each game goes to its own folder of the library, and the Levels panel switches between them.

![Jak and Daxter: the village, its sea and the far sea to the horizon](19-jak1.png)

![Jak 3: Spargus](20-jak3.png)

## 2. Open a level

![A level opened: the Levels panel, the 3D view and the inspector](01-overview.png)

The window has three parts:

- on the left, the **Levels** panel: the levels of the game you extracted (the real levels only: the
  pieces the game loads for a mission or a cutscene are left out), a picture of the one chosen and
  **Load the level**;
- in the middle, the **3D view**: fly with the right mouse button and WASD (ZQSD on AZERTY), frame
  the level with Home;
- on the right, the **inspector**: what is loaded, or what is selected.

Here is Jak II's atoll: 13,278 elements and 81 actors, drawn with the colors of its lights, its
water and its ocean, and the far ocean to the horizon as the game draws it. Levels open **without
their creatures** (enemies, characters, the Titan suit): the editor is for places and objects; with
**With its actors** unchecked, a level opens with its decor alone. The level the game shows with
this one (`atollext`) is shown too, and can be hidden in the inspector.

## 3. See it along the story

![The palace square after Mar's tomb opened: the broken statue and its rubble](05-story.png)

A level changes as the story goes: in Jak II, the statue of the palace square falls when Mar's tomb
opens. The **story bar**, at the bottom of the view, shows the level at a point of the game. Its
slider goes through the **states of the level**, not through the 324 steps of the story: the start
of the game, then each step that changes this level. The palace square has three: here the third,
just after `canyon-insert-items-resolution`, with the broken statue and its rubble that the start
of the game does not show. With several levels, a list next to **Story** chooses the level: each
one is at its own point of the story. Uncheck **Story** to see every state of the level at once.

The editor knows this from the game data: the missions in the order of the game, and what each one
shows or hides.

## 4. Select and edit

![A flight of steps of the palace selected, with the gizmo and the inspector](03-selection.png)

**Click** an element to select it: here a flight of steps of the palace (outlined in orange). Each
element is its own: these steps are one of 14 copies of the same mesh (the inspector says so), and
the click selected this one only. **Shift or Ctrl + click** adds to the selection, a **drag** in an
empty spot selects everything in a rectangle.

The **gizmo** moves the selection (**W**: drag an arrow or a square) or rotates it (**E**: drag a
ring); the inspector gives the exact position, rotation and scale. **Copy** (Ctrl+C), **Paste**
(Ctrl+V), **Duplicate** (Ctrl+D), **Delete** (Del), **Undo** (Ctrl+Z) and **Redo** (Ctrl+Y) do
what they say. The large pieces of ground are locked, so that clicks go to what stands on them.

## 5. Stretch, grow, shrink

![A canal bridge stretched two and a half times along its length, with the scale gizmo](07-scale.png)

**Scale (R)** turns the gizmo into squares: drag the square at the end of an axis to stretch the
selection along it, or the square in the middle to make it bigger or smaller. The axes are the
element's own. Here a canal bridge of the city, 16 m long, stretched 2.5 times to span 40 m over a
pool; the inspector shows its scale (2.50, 1.00, 1.00). With **Keep the proportions** checked,
changing one value changes the three.

The game keeps the scale of decor (pieces, prototypes, models) but gives most of its objects their
own size, whatever the level says: the inspector warns when an object is scaled.

## 6. The catalog

![The objects of the catalog, by family](06-catalog.png)

The **Catalog** panel lists what can be put in a level, placed like a prefab: click an element,
then click in the view (R turns it, Esc stops), or drag it into the view. It has three tabs.

**Objects** (1,171 for Jak II), by family and with a filter:

- the objects of the game: crates, collectables, platforms and elevators, doors, buttons, jump
  pads, hazards, lights, turrets, vehicles, the water, dark eco and lava of each level...;
- the particle effects of the levels (lights, neon signs, steam), and the logic that makes sense in
  another level.

A new object takes the data of an object of its type found in the game's levels (the pickup of a
crate...), without what ties that one to its level.

![The models of the catalog](09-models.png)

**Models** (182): every other model of the game but the characters, placed as decor: parts of
objects, debris, props.

![The decor of every level searched for "catwalk"](08-decor-search.png)

**Decor**: the decor of the levels, to reuse anywhere: their prototypes (a pillar, an arch, a lamp
post) and the pieces of their shell (a wall, a bridge). **All levels** searches the 31,552 parts of
Jak II's levels by name: `catwalk` finds the 23 catwalks of the industrial zone, the slums, the
drill platform and `caspad`. Choose a level instead to browse its whole decor.

The first time a library is shown, the editor reads what the catalog needs of every level in the
background (a few seconds) and keeps it: the next times, the catalog is complete at once.

## 7. Prefabs

![A prefab made of barrels and a crate, and a copy of it placed in front](04-prefabs.png)

A prefab is a group of elements saved together, to place again anywhere. Select the elements (here
three barrels and a market crate), click **Create a prefab** and give it a name: it appears in the
**Prefabs** panel, with a picture. Click it, then click in the view to place copies: here a copy was
placed in front of the original group, and is selected. A prefab sits on the surface it is placed
on, and its objects get names of their own (`market-crate-2`), as the game needs.

## 8. Build a new level

![A new level built from the catalog: two decks, a bridge over a pool, lamps, crates and orbs](10-new-level.png)

**File > New level** (Ctrl+N) starts an empty level of the game shown in the Levels panel. This one
was built only from the catalog: two decks made of eight catwalk modules of the industrial zone
(Decor), the palace's pool between them (Objects, Water), the canal bridge of the previous picture
stretched across it, lamps, orbs along the bridge, crates and barrels, the palace's fountain bowl
and a fruit stand of the market. There is no ground at first: an element placed in the void goes to
height 0, and later ones can be dropped onto the surfaces below.

## 9. Let an assistant build it

![What the assistant sees of its level: the picture capture_view gives it](11-assistant-view.png)

The level of the previous picture was not built by hand: a script built it through the editor's
remote control, with the very tools the MCP server gives an assistant such as Claude Code. That is
what an assistant does when you ask it for *"a small crossing: two decks of the industrial zone and
a bridge over the palace's pool"*: it looks for parts, places them, and checks its work with a
picture of the view (above: what `capture_view` returns). The calls look like this:

```json
{"tool": "new_level",       "args": {}}
{"tool": "list_decor",      "args": {"filter": "catwalk"}}
{"tool": "place",           "args": {"kind": "decor", "name": "city-ind-catwalk-main-01", "position": [-44, 0, -8]}}
{"tool": "place",           "args": {"kind": "object", "name": "water-anim-ctypal-lrgsqr-pool", "position": [0, -1.5, 0]}}
{"tool": "place",           "args": {"kind": "decor", "name": "city-canal-bridge-span", "position": [0, 0.83, 0]}}
{"tool": "transform_nodes", "args": {"ids": [11], "scale_by": [2.5, 1, 1]}}
{"tool": "place",           "args": {"kind": "object", "name": "ctyn-lamp", "position": [-22, 10, 5], "on_ground": true}}
{"tool": "capture_view",    "args": {"width": 1280, "height": 720}}
{"tool": "save_project",    "args": {"path": "build/demo/canal-crossing.glb"}}
```

With an assistant, every call happens in the editor window you see, and can be undone. The whole
sequence is in [`tools/build_demo_level.py`](../../tools/build_demo_level.py), which builds this
level again (`python tools/build_demo_level.py`); how to connect Claude Code is in
[the README](../../README.md#5-building-levels-with-an-assistant-mcp).

## 10. A whole district, by an assistant

![Ironworks from the air: blocks of the industrial zone's facades, streets, catwalks and a yard](13-district.png)

The demo level is small. **Ironworks** is a district of Haven City in the style of the industrial
zone, and as large: 448 by 448 meters, 15,465 elements, built in about 100 seconds through the
editor's MCP server by [`tools/build_industrial_district.py`](../../tools/build_industrial_district.py),
a small MCP client that calls the tools Claude Code gets (`place_many` places hundreds of elements
in one call). The pictures are what `capture_view` returned to it.

It builds the way the game's designers built the zone: from its modules, placed as the game places
them. Each block is a ring of four *facade columns*, a ground facade, the upper facade standing on
it and everything the game hangs on them (pipes, lamps, windows, stains, red lights and their
glow), read from the zone's own facades of the same kind; three kinds of blocks, grey, orange with
towers and with mansard roofs, each side from a different facade. The blocks get flat roofs and,
on the orange ones, the game's corner joints; the streets end against wall buildings at the edge of
the district. Nothing is left open, even where the game's camera never goes: the roofs, the tops of
the street ends, the corners, under the ground.

![The yard: floor vents with their red glow, steam stacks, the catwalk's ramp, a pipe bridge and barrels](14-district-yard.png)

In the middle, a yard: floor vents with their red glow, steam stacks, warning lights, the zone's
catwalk ramp coming down from the catwalks of two streets, a pipe bridge on metal pillars, barrel
stacks. The neon signs are black in the editor: the game draws them with particle effects, which
the editor shows as markers.

## 11. Nav meshes: where the creatures walk

![The nav meshes of the district: the streets, the yard and the catwalks, one color per mesh, the Nav meshes window and the inspector](15-navmesh.png)

The enemies and citizens of Jak II and Jak 3 walk on nav meshes: flat polygons over the ground,
on which the game finds their way. **Tools > Nav meshes** generates them from the walkable surfaces
of the level, for a creature of the size you give (radius, height, step, slope): Recast (from
recastnavigation) builds the polygons, and the editor fits them to what the game reads, convex
polygons of 3 or 4 corners, at most 255 per mesh, each mesh in one piece, with the route table and
the polygon grid the game uses. Here, the district's: the streets, the yard, the ramp and the
catwalks, one color per mesh; the inspector says the selected one suits the game.

**Only the ground under the selection** keeps the pieces of ground the selected elements stand on:
creatures do not walk on roofs, ledges or in closed rooms. The script above keeps the ground
connected to a few street points (the `from` of `generate_navmesh`): 8 nav meshes, 1,323 polygons,
96,800 m2 in 0.6 seconds, and 151 pieces of ground away from the streets left out, the insides of
the blocks among them. **Export the nav meshes...** writes them as the game's levels hold them, for
a custom level builder.

## 12. Several levels, side by side

![ctyinda with ctyindb and ctywide added: the industrial zone, the city walls and the palace; the inspector lists the levels added and the levels around them](16-levels.png)

In the game, levels follow one another: Haven City alone is some twenty of them, loaded two or
three at a time as the player goes. The extracted levels keep the game's coordinates, so levels
that are side by side in the game meet in the editor too. With `ctyinda` open, **Add to the
document** (Levels panel) brings `ctyindb` at its place, and the streets of the industrial zone run
from one level into the other; `ctywide` brings the city walls, the palace and its towers around
them. Here the document holds 27,160 elements of three levels.

The inspector lists the **levels**: show or hide each one added, frame it, frame the **Junction**
where it meets the level opened, or remove it. **Around it in the game** gives the levels the game
displays with those of the document, read from its continue points: a click adds one. That is what
working on a transition between two levels needs, and it works from a new level too: **File > New
level**, add the levels of the zone to join, and build next to them at the right place in the
game's world. An assistant does the same with `add_level`, `get_level_info` (neighbors, continue
points, bounds) and `set_camera` with `{"frame": {"junction": ["ctyinda", "ctyindb"]}}`.

![ctyindb selected whole next to ctyinda: the gizmo in its middle](17-level-selected.png)

**Select**, next to a level in the inspector (or **Edit > Select its level**, Ctrl+L, from any of
its elements), selects the whole level: the gizmo stands in its middle, and moving or turning it
moves the level with its decor, collision and actors, to shift a level or make room between two.

## 13. Export it for the game

![The export window: the slums of Jak II exported for Jak 3, and the report of what changed](18-export.png)

**File > Export for the game...** writes what the view shows as a **custom level of OpenGOAL**: the
`.glb`, `.jsonc` and `.gd` files its level builder takes, the level's definition for the game's
code and an `INSTALL.md` with the lines to add. Choose the destination: Jak and Daxter, Jak II or
Jak 3. For another game than the level's own, the level is adapted: here the slums of Jak II for
Jak 3. The collision surfaces are written as the builders take them (the materials Jak 3's builder
cannot give take the nearest), the types Jak 3 does not have are left out (the market's fruit
stands are only placeholders in Jak 3), the hideout's door becomes Jak 3's `cty-door`, the red pass
a security wall asks for is renumbered (29 in Jak II, 39 in Jak 3), and the data tied to Jak II's
story is left out. The report says each change and how many elements it concerns.

The level keeps its place in the game's world: exported with the levels added to it, a transition
between two levels or a new level next to the game's comes out where it was built.

## 14. Save, and find help

![The Getting started window](12-help.png)

**File > Save** (Ctrl+S) writes your level as a project, a `.glb` file (Blender opens it too), with
backups next to it; **File > Open project** (Ctrl+O) opens it again. **Help > Getting started**
(F1) sums up this tour: the steps, the camera, the story and the editing shortcuts.

## Making these screenshots again

The editor makes these pictures itself, in a hidden window. From the repository root, after
building the editor and extracting Jak II (add `--library <folder>` when your levels are not in
`library/`):

```bash
E=build/bin/open-goal-level-editor
$E --screenshot "<your Jak II disc: .iso or folder>" docs/captures/02-extraction.png extract --lang en
$E --screenshot atoll docs/captures/01-overview.png levels --lang en
$E --screenshot ctypal docs/captures/05-story.png levels --lang en \
   --camera 192,45,845,180,-14 --story canyon-insert-items-resolution
$E --screenshot ctypal docs/captures/03-selection.png select --lang en
$E --screenshot ctypal docs/captures/06-catalog.png catalog --lang en
$E --screenshot ctypal docs/captures/09-models.png models --lang en
$E --screenshot ctyindb docs/captures/08-decor-search.png decor-all --lang en
$E --screenshot atoll docs/captures/12-help.png help --lang en

# the demo level, built through the remote control, and the assistant's view of it
python tools/build_demo_level.py --view docs/captures/11-assistant-view.png
D=build/demo/canal-crossing.glb
$E --screenshot $D docs/captures/07-scale.png scale --lang en
$E --screenshot $D docs/captures/04-prefabs.png prefab --lang en \
   --select cty-tanker-barrel,cty-tanker-barrel-2,cty-tanker-barrel-3,market-crate-1
$E --screenshot $D docs/captures/10-new-level.png project --lang en --camera -18,52,84,-12.1,-31.2

# the district, built through the MCP server, its pictures from capture_view, and its nav meshes
python tools/build_industrial_district.py --views build/demo
cp build/demo/district-aerial.png docs/captures/13-district.png
cp build/demo/district-yard.png docs/captures/14-district-yard.png
$E --screenshot build/demo/ironworks.glb docs/captures/15-navmesh.png navmesh --lang en \
   --camera 120,110,90,37.57,-31.88

# several levels side by side, one selected whole
$E --screenshot ctyinda docs/captures/16-levels.png levels --lang en --add ctyindb,ctywide \
   --camera 1330,330,330,140.77,-26.11
$E --screenshot ctyinda docs/captures/17-level-selected.png select --lang en --add ctyindb --select ctyindb

# the export for the game, and a level of Jak and Daxter and of Jak 3
$E --screenshot ctysluma docs/captures/18-export.png export --lang en
$E --screenshot village1 docs/captures/19-jak1.png levels --lang en --game jak1
$E --screenshot wascitya docs/captures/20-jak3.png levels --lang en --game jak3
```

The setups (the word after the file name):

| Setup | What the picture shows |
|---|---|
| `levels` | the level given loaded, the Levels panel |
| `select` | the level given, one of its decor elements selected (near the middle of the level), the gizmo on it; a level added named by `--select`: selected whole |
| `scale` | the element stretched, the scale gizmo: a catwalk of the level given, or the first stretched element of a project |
| `prefab` | a prefab made of a few elements and a copy placed next to them, the Prefabs panel |
| `catalog`, `models` | the level given loaded, the objects or the models of the Catalog panel |
| `decor`, `decor-all` | the decor of the level given in the Catalog panel, or the decor of every level searched for `catwalk` |
| `project` | a project (`.glb`) opened, the objects of the Catalog panel |
| `navmesh` | the level or project given, its nav meshes drawn (made for the whole level when it has none), the largest selected, the Nav meshes window |
| `export` | the level given exported for another game (Jak 3 for Jak II's levels, else Jak II) to a temporary folder: the export window and its report |
| `extract` | the extraction window, extracting the disc given to a temporary folder deleted afterwards |
| `help` | the Getting started window |

The options: `--size WxH` (1600x900 by default), `--game jak1|jak2|jak3` (the game whose levels are
used), `--camera x,y,z,yaw,pitch` (position in meters, angles in degrees), `--story off|<step>|<node>` (the point of the story shown),
`--select a,b,...` (the elements `select`, `scale` and `prefab` use, by name) and `--add a,b,...`
(levels added to the one loaded). A capture never
changes the editor's preferences, and the prefab of `prefab` is written to a temporary folder.

The screenshots show levels of Jak II, Jak and Daxter and Jak 3 extracted from copies of the games.
The games belong to Sony Interactive Entertainment and were developed by Naughty Dog; the editor is built on the work of the
OpenGOAL team, with which it is not affiliated (see [`LICENSE`](../../LICENSE)).
