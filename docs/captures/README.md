# Screenshots of the documentation

The pictures of the [README](../../README.md) and of the [guide for AI agents](../mcp-agent-guide.md),
one folder per feature:

| Folder | What it shows |
|---|---|
| `getting-started/` | the editor's window, the guide (F1), the extraction of a disc |
| `levels/` | the Levels panel, the story, several levels side by side, a level selected whole, the ocean |
| `selecting/` | a selection and its gizmo, what the cursor tells, a box being drawn, the button held on a prefab, a prefab dragged, the scale tool |
| `groups/` | the Groups panel, a group being named |
| `catalog/` | the objects, the interactive objects, the visual effects and their kinds, the models, the decor searched, the materials |
| `prefabs/` | the game's prefabs, searched, a tower selected by its assembly, a prefab being made (the small pieces left out) |
| `building/` | the shapes, a level's style, the terrain brush, the ground it raises, the magnet brush, the topology tool, the preview of an object, a level of Jak II |
| `holes/` | the holes of a yard found, in the editor and as an assistant sees them |
| `game-rendering/` | the Game rendering window: noon, dusk, storm, night, haze; the effects |
| `behaviors/` | an airlock that opens once a crate is broken |
| `nav-meshes/` | the nav meshes of a level |
| `export/` | the export window, a level made the replacement of one of the game's |
| `mcp/` | what an assistant built and saw; a facade from the street and from behind |

The pictures were taken with the editor's previous interface (C++ and Dear ImGui): they are to be
taken again with the present one.

## Making them again

`tools/capture_docs.py` starts the editor (built before: `target/release`, else `target/debug`),
the interface in English, and drives it two ways: its **remote control** (the MCP server's tools)
sets up each scene and gives the pictures of the 3D view alone (`capture_view`: the holes and the
assistant's pictures), and the **DevTools protocol of its web view** (WebView2, so Windows) gives
the pictures of the whole window at 1600 x 900, clicks a tab or a menu by its English label and
presses keys. From the repository root, after building the editor and extracting Jak 3 (and Jak II
for its pictures):

```bash
python tools/capture_docs.py                                 # all of them
python tools/capture_docs.py --only groups,holes             # some folders
python tools/capture_docs.py --out <folder>                  # somewhere else, to compare first
```

Some pictures use prefabs of your own (`main-town-building-a` to `-d`, `small-fountain`,
`haven-tree`, `ind-facade-60` in `library/prefabricated/`): use yours instead.

The pictures of a tool used with the mouse over the 3D view (the cursor's label, a box being drawn,
the button held, a drag, the terrain brush, the topology tool, a prefab or a group being named) and
the extraction window are not taken by the script (it prints their list): take them by hand, or
drive the mouse through the same DevTools connection (`Input.dispatchMouseEvent`).

The screenshots show levels of Jak 3 (and Jak II) extracted from a copy of the game. Jak 3 belongs to
Sony Interactive Entertainment and was developed by Naughty Dog; the editor is built on the work of
the OpenGOAL team, with which it is not affiliated (see [`LICENSE`](../../LICENSE)).
