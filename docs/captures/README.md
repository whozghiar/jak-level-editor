# Screenshots of the documentation

The pictures of the [README](../../README.md) and of the [guide for AI agents](../mcp-agent-guide.md),
one folder per feature:

| Folder | What it shows |
|---|---|
| `getting-started/` | the editor's window, the guide (F1), the first-run assistant, the settings, the extraction of a disc |
| `levels/` | the Levels panel, the story, several levels side by side, a level selected whole, the ocean |
| `selecting/` | a selection and its gizmo, what the cursor tells, a box being drawn, the button held on a prefab, a prefab dragged, the scale tool |
| `groups/` | the Groups panel, a group being named |
| `catalog/` | the objects, the interactive objects, the visual effects and their kinds, the models, the decor searched, the materials |
| `prefabs/` | the game's prefabs, searched, a tower selected by its assembly, a prefab being made (the small pieces left out) |
| `building/` | the shapes, a level's style, the terrain brush, the ground it raises, the magnet brush, the topology tool, the preview of an object, a level of Jak II |
| `holes/` | the holes of a yard found, in the editor and as an assistant sees them |
| `game-rendering/` | the Sky, hour and weather window: noon, dusk, storm, night, haze; the effects |
| `behaviors/` | an airlock that opens once a crate is broken |
| `nav-meshes/` | the nav meshes of a level |
| `export/` | the export window and its report, a level made the replacement of one of the game's, the Play window the first time, the OpenGOAL settings |
| `mcp/` | what an assistant built and saw; a facade from the street and from behind |

## Making them again

`tools/capture_docs.py` starts the editor (built before: `target/release`, else `target/debug`, or
`--editor <program>`), the interface in English, with preferences of its own (never yours: the
library given, the guide already seen) in a work folder (`--work`, by default `jle-captures` in the
temporary folder), and drives it two ways:

- its **remote control** (the MCP server's tools) sets up each scene: open a level, place prefabs
  and shapes, draw a terrain, set the camera, the hour and the weather; `capture_view` gives the
  pictures of the 3D view alone (the holes, the assistant's pictures);
- the **DevTools protocol of its web view** (WebView2, so Windows) gives the pictures of the whole
  window at 1600 x 900, clicks tabs, menus and buttons by their English labels, types in fields,
  presses keys, and moves the mouse over the 3D view (`Input.dispatchMouseEvent`): the cursor's
  label, a box being drawn, the button held, a prefab dragged, the terrain brush and its right
  button, the magnet brush, the topology tool's box.

From the repository root, after building the editor and extracting Jak 3 (and Jak II for its
pictures):

```bash
python tools/capture_docs.py                                 # all of them
python tools/capture_docs.py --only groups,holes             # some folders (and first-start)
python tools/capture_docs.py --out <folder>                  # somewhere else, to compare first
```

- `first-start` (in `getting-started/`) starts a second editor with no preferences at all: the
  first-run assistant, then the extraction of a disc (`--disc`, by default the one the OpenGOAL
  Launcher extracted) into the work folder; its preferences and the levels it wrote are deleted
  afterwards.
- The library (`--library`, by default `library/` of the repository) is only read. What the
  script writes (an export, an extraction) goes to the work folder.
- The paths of your computer the windows show are replaced in the pictures: the library is shown
  as `C:/Users/you/Documents/Jak Level Editor`, the folder the editor proposes (the first start
  creates it when it is not there: the script deletes it again when it stays empty), the disc as
  `iso_data/jak3` of the editor's folder.
- The standalone app has no Play: `export/by-hand.png` is the export's report with the guide's
  tutorial on adding the level to the game by hand, opened from it.
- Some pictures use prefabs of your own (`main-town-building-a` to `-d`, `small-fountain`,
  `haven-tree`, `ind-facade-60` in `library/prefabricated/`): use yours instead.

Look at every picture (a contact sheet of several helps) before using it in the docs, and keep them
small (`--out` a folder, then optimise the PNG files: `oxipng`, `optipng` or Pillow's `optimize`).

The screenshots show levels of Jak 3 (and Jak II) extracted from a copy of the game. Jak 3 belongs to
Sony Interactive Entertainment and was developed by Naughty Dog; the editor is built on the work of
the OpenGOAL team, with which it is not affiliated (see [`LICENSE`](../../LICENSE)).
