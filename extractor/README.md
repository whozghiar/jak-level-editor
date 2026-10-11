# ogle-extract

`ogle-extract` reads **your own copy** of Jak 3 (an `.iso` image or the folder of a disc already
extracted) and writes the `.glb` files the editor loads. The editor runs it itself (**File >
Extract game assets...**), but it also works alone:

```text
ogle-extract <game.iso | disc folder> <output folder> [--levels CIA,CIB] [--textures]
ogle-extract --detect <game.iso | folder>   # game, version and serial (json)
ogle-extract --list <game.iso | folder>     # the levels of the disc (json)
```

| Option | Effect |
|---|---|
| `--levels CIA,CIB` | extracts these DGO files only (without it: every level of the game) |
| `--textures` | also writes the textures of each level as `.png` files |
| `--data <folder>` | the game's tables (by default `ogle-extract-data/` next to the executable) |
| `--work <folder>` | where the files read from an `.iso` are copied (by default `<output>/_disc/`, deleted at the end) |

An `.iso` is read in place: `--detect` and `--list` read its file table, and an extraction copies
only the files it reads (the `DGO` and `CGO` folders, about 400 MB of a 4 GB disc).

The extractor reads nothing but the disc and its tables, and writes nothing outside the output
folder: the decompiler code it is built from looks for its files below a "project" folder, which
is always the output folder (the folder of the executable for `--detect` and `--list`), never a
jak-project checkout.

## What is written

```text
<output>/jak3/library.json                              the game, its version, the levels extracted
<output>/jak3/common/models/*.glb                       models every level uses (collectables, Jak...)
<output>/jak3/<level>/decor/<level>-background.glb      decor (one node per instance), collision, actors
<output>/jak3/<level>/models/<model>-lodN.glb           the models of the level (crates, doors, enemies...)
<output>/jak3/<level>/textures/*.png                    the textures of the level (--textures)
<output>/jak3/<level>/level.json                        what the folder holds (DGO, actors, models,
                                                       regions)
```

In the decor, the level's shell (tfrag: ground, walls, floors, bridges) is split in pieces: the
triangles their vertices connect, vertices at the same place counting as one, so that a building
or a bridge is a node of its own. The pieces 60 m across or more whose surface mostly faces up or
down (at least half of it within about 45 degrees of horizontal) are the ground
(`extras.og_terrain: true`): the editor locks them. A tall facade, a long wall or a door 60 m
wide is not ground, and stays selectable. The vertex colors are, for each vertex, the
brightest of the level's 8 palettes: the game mixes them each frame, and they are layers of
light (palette 0 alone leaves the insides of buildings black). The palettes themselves are kept
for the game view and the export, in the node extras `og_tod`: `palette`, an accessor of the
tree's colors (8 RGBA bytes each, as the game has them), and `index`, per primitive an accessor
of the color of each vertex (per instance for TIE and shrub: the instances of a prototype share
its mesh, not their lighting; a shrub's colors times its vertex colors, as tfrag's). A material
the game draws without writing its depth (the translucent and water trees: stains, decals, glass)
has `extras.og_translucent`; one that blends nothing (`(Cs - Cs) * As + Cs`) is `MASK`.

`level.json` also lists the actors by type (`actor_types`: how many, the data of the first one
and the art-names of all, `art_names`), the templates of the editor's catalog, and the level's
regions (`region_trees`: per tree, `camera`, `target`..., its regions as the level builder reads
them: their shape, sphere, face or volume in meters, and their scripts, `on-enter`, `on-inside`,
`on-exit`). A region of several faces has one entry per face, with the same id.

A level goes to the folder of its name. When a second DGO holds the same level (a copy of it), it
goes to `<level>-<dgo>`.

A disc of **Jak II** is extracted too, to `<output>/jak2/`, in the same layout and with the same
writer: the editor offers its structures (buildings, the palace, pillars, cables) to Jak 3 levels
and converts them when they are placed (see the editor's README, *Structures of Jak II*). Its tables
are in `data/jak2/`. A disc of another game is recognized (`--detect` says which) but not extracted
(`not_jak3`). Nothing of the game is in this repository: everything written comes from the user's
disc.

## Messages for the editor

The lines starting with `OGLE_` are read by the editor, which translates their codes:

| Line | Meaning |
|---|---|
| `OGLE_GAME {json}` | the game of the disc: `game`, `version`, `name`, `serial`, `iso` |
| `OGLE_LEVELS [json]` | the answer to `--list`: `[{"dgo": "CIA.DGO", "names": ["ctyinda"]}, ...]` |
| `OGLE_STEP <code> [numbers]` | the current step: `iso <done> <total>`, `read`, `textures`, `levels <count>` |
| `OGLE_PROGRESS <done> <total> <name>` | a level (or `common`) is written |
| `OGLE_DONE <folder>` | the end: the folder of the game |
| `OGLE_ERROR <code> [detail]` | the failure: `not_found`, `unreadable`, `not_iso`, `not_jak`, `no_executable`, `unknown_disc`, `not_jak3`, `tables_missing`, `tables_unreadable`, `no_levels`, `cannot_create`, `iso_read`, `extraction` |

## Where the code comes from

`opengoal/` holds the data readers of the decompiler of [OpenGOAL](https://github.com/open-goal/jak-project)
(ISC license, see `opengoal/LICENSE`), copied with their paths from commit `a42c07199`. Only the
files `CMakeLists.txt` compiles and the headers they include are kept: no code analysis, no GOAL
compiler, no game runtime, no REPL.

Changes from jak-project:

- `decompiler/level_extractor/fr3_to_gltf.*`: the decor can hold the collision (the exact
  surfaces) and the actors, and each decor instance (TIE, shrub) is a glTF node sharing the mesh of
  its prototype. Each model keeps only its own vertices: the original code wrote the whole vertex
  buffer of the level in every model file (once more per face blend shape), nearly four times the
  size. The PS2's additive and subtractive materials (glows, searchlight beams),
  which glTF cannot describe, keep their blend in `extras.og_blend` (`add`, `add_alpha`,
  `subtract`, `half`) instead of turning opaque.
  The level's shell is split in pieces and its vertex colors are the brightest palette (see
  above); the original code wrote one mesh per texture over the whole level and the first palette.
  The 8 palettes of every vertex are kept (`og_tod`), and the translucent materials marked.
- `decompiler/level_extractor/BspHeader.*`: the region trees of a Jak 3 level, in `level.json`.
- `decompiler/ObjectFile/LinkedObjectFile.cpp`: the scripts of a list print their integers
  (fixnums, which came out times 8) and their floats (`bfloat`, which came out as a label).
- `decompiler/level_extractor/extract_level.*`: the layout above, the common models, progress,
  levels extracted in parallel (as many as the processor has cores), the actors by type in
  `level.json`; no `.fr3` files for the game.
- `decompiler/config.h`: the options `rip_level_entities`, `rip_output_root`, `rip_textures_png`,
  `on_level_done`, `rip_level_folder`.
- `decompiler/ObjectFile/ObjectFileDB.*`: without the file patches (xdelta3) and the extractors of
  the game's texts, counts and subtitles.
- `common/goos/Reader.*`: without the REPL.
- `decompiler/level_extractor/extract_anim.*`: without the dependency on `goalc`.
- `common/util/FileUtil.cpp`: the project folder is always given; no search for a jak-project
  checkout from the executable's path (the original code took a sibling `jak-project` folder for
  it when the path contained that name).
- `stubs.cpp` (outside `opengoal/`): the only symbol of the code analysis the link needs.

`opengoal/game/graphics/` is not part of the extractor: it is the game's background renderer
(`TFragment`, `background_common`, `Shader`, `BucketRenderer` and the shaders `tfrag3.*`), copied from
jak-project's `game/graphics` and built into the editor for its game view (`src/game_view`), with
the editor's stand-ins for what the game's renderer takes from the running game (`Loader.h`,
`Profiler.h`, `TexturePool.h`, `pipelines/opengl.h`: marked `ogle:`) and its shaders compiled when
first used (`Shader.*`).

`data/jak3/` (and `data/jak2/` for Jak II) holds the tables of jak-project the reading needs: type
definitions (`all-types.gc`), the files of the disc and its levels (`inputs.json`), the textures some
levels miss (`missing_textures.json`), and the level names of each DGO (`levels.json`, from
`level-info.gc`).
