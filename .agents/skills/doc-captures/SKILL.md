---
name: doc-captures
description: Make the screenshots of the documentation again (docs/user-guide.md, docs/mcp.md, README.md) in docs/captures/<feature>/. Use after a change to the interface or a feature shown in the docs, or when adding a feature to the user guide.
---

# Documentation captures

The pictures go in `docs/captures/<feature>/<name>.png`, one folder per feature of the user guide
(getting-started, levels, selecting, groups, catalog, prefabs, building, holes, game-rendering,
behaviors, nav-meshes, export, mcp), 1600 x 900, the interface in English.

```bash
python tools/capture_docs.py                            # all of them (needs Jak 3 extracted, the editor built)
python tools/capture_docs.py --only groups,holes        # some features
python tools/capture_docs.py --only levels --out <dir>  # elsewhere, to compare before replacing
```

The script starts the editor (`target/release`, else `target/debug` `jak-level-editor`;
`--editor <program>` for a copy) and drives it two ways:

- **The remote control** (`Editor.call`: the MCP server's tools, one JSON line per request on
  `--remote-port`) sets up the scene: `open_level`, `add_level`, `place`, `select`, `set_camera`,
  `set_view`, `set_story`, `find_gaps`... `capture_view` gives a picture of the 3D view alone; it
  answers once the interface is up (the script waits for it).
- **The web view's DevTools** (`Editor.web`): the editor started with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` and a
  `WEBVIEW2_USER_DATA_FOLDER` of its own (one shared with a running editor keeps the window from
  appearing). `shot(path)` is `Page.captureScreenshot` of the whole window (the page set to
  1600 x 900 with `Emulation.setDeviceMetricsOverride`), `click("Levels")` the first shown button
  or entry with that text or aria-label, `key("F1")` a key for the interface's shortcuts,
  `engine(name, args)` any call of the engine as the interface makes it
  (`window.__TAURI_INTERNALS__.invoke("plugin:jak-level-editor|call", {name, args})`).

Not done by the script (it prints the list): the pictures of the mouse over the 3D view (the
cursor's label, a box being drawn, the button held, a drag, the terrain brush and the topology
tool), the windows naming a prefab or a group, and the extraction window. Take them by hand, or
with `Input.dispatchMouseEvent` / `Input.dispatchKeyEvent` over the same DevTools connection (the
viewport's coordinates are the page's).

Tips:

- Wait for the view to draw before a picture (`shot(path, settle=seconds)`): meshes, textures and
  the game rendering's chunks come after the call that made them.
- Close the windows a section opened before the next one (`close_windows()`: Escape closes only
  the modal ones, and the first "Close" of the page is the editor's own).
- Look at every picture (a contact sheet of several helps) before using it in the docs.
- The pictures show the game's levels: never put the extracted files themselves in git, only the
  pictures.
