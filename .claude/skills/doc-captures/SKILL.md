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
python tools/capture_docs.py --only groups,holes        # some features (first-start: the first-run assistant, the extraction)
python tools/capture_docs.py --only levels --out <dir>  # elsewhere, to compare before replacing
```

The script starts the editor (`target/release`, else `target/debug` `jak-level-editor`;
`--editor <program>` for a copy; `--port` and `--devtools` when another editor uses the defaults)
with preferences of its own in `--work` (the library `--library` only read, English, the guide
seen; never the user's preferences) and drives it two ways:

- **The remote control** (`Editor.call`: the MCP server's tools, one JSON line per request on
  `--remote-port`) sets up the scene: `open_level`, `add_level`, `place`, `terrain`, `select`,
  `set_camera`, `set_view`, `set_story`, `find_gaps`... `capture_view` gives a picture of the 3D view
  alone; it answers once the interface is up (the script waits for it).
- **The web view's DevTools** (`Editor.web`): the editor started with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` and a
  `WEBVIEW2_USER_DATA_FOLDER` of its own (one shared with a running editor keeps the window from
  appearing). `shot(path)` is `Page.captureScreenshot` of the whole window (the page set to
  1600 x 900 with `Emulation.setDeviceMetricsOverride`), the paths of the computer replaced first
  (`anonymize`: the library shown as `C:/Users/you/Documents/Jak Level Editor`); `click("Export")`
  the first shown button or entry with that text or aria-label (waited for), `tab("Build")` a panel
  of the bar of icons (the menu bar has a Build too), `menu("Level", "Export for the game...")`,
  `type_into(selector, text)`, `key("F1")`, `engine(name, args)` any call of the engine as the
  interface makes it (`window.__TAURI_INTERNALS__.invoke("plugin:jak-level-editor|call", {name, args})`).
- **The mouse over the 3D view**: `center(dx, dy)` a point from the view's middle (where
  `set_camera`'s `look_at` is), `mouse("moved" | "pressed" | "released", at, button, buttons)`,
  `drag(a, b, button, release=False)` to take the picture while the button is held (the box, the
  ring of the button held, a drag, the terrain brush's stroke), then Escape and release.
  `shot(..., mouse=True)` keeps the mouse where it is; otherwise it is moved out of the view first
  (its label would show).

Tips:

- Wait for the view to draw before a picture (`shot(path, settle=seconds)`): meshes, textures and
  the game rendering's chunks come after the call that made them.
- Several mouse events in one DevTools connection: a new connection sets the page's size again,
  and the view forgets where the mouse is.
- The terrain brush's window covers the right of the view: aim the mouse at the left.
- Close the windows a section opened before the next one (`close_windows()`: Escape closes only
  the modal ones, and the first "Close" of the page is the editor's own).
- Never let the script write into the library: exports and extractions go to `--work`. The
  remote control's requests carry the token of `<work>/prefs/remote.json` (`Editor.token`).
- Look at every picture (a contact sheet of several helps) before using it in the docs; optimise
  the PNG files.
- The pictures show the game's levels: never put the extracted files themselves in git, only the
  pictures.
