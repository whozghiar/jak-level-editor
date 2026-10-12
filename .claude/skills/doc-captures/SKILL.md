---
name: doc-captures
description: Make the screenshots of the documentation again (README.md, docs/mcp-agent-guide.md) in docs/captures/<feature>/. Use after a change to the interface or a feature shown in the docs, or when adding a feature to the README.
---

# Documentation captures

The editor takes its own screenshots, in hidden windows (1600 x 900, English interface). They go
in `docs/captures/<feature>/<name>.png`, one folder per feature of the README (getting-started,
levels, selecting, groups, catalog, prefabs, building, holes, game-rendering,
behaviors, nav-meshes, export, mcp).

```bash
python tools/capture_docs.py                       # all of them (needs Jak 3 extracted)
python tools/capture_docs.py --only groups,holes   # some features
python tools/capture_docs.py --only selecting --quick   # only those made through the remote control
python tools/capture_docs.py --disc "<your Jak 3 .iso or folder>"   # with the extraction window
```

Two ways, both in the script:

- **Setups** of the editor: `open-goal-level-editor --screenshot <level|project|disc> <out.png>
  <setup> --lang en [--camera x,y,z,yaw,pitch] [--game <hour>] [--select ...] [--add ...]
  [--story ...]` (setups: levels, select, scale, prefab, catalog, models, decor, decor-all,
  materials, effects, actors, navmesh, export, extract, guide, tools, help, build, project).
- **The remote control** of a hidden editor (`Editor` in the script): the MCP tools, plus
  `ui_input` with mouse and key events (`move`, `move_to_node`, `down`, `up`, `click`, `wheel`,
  `key` with `mods`, `text`, `wait` frames, `shot`) and `capture_view` for the 3D view alone.

Tips:

- Tools that wait a moment (the hover label: 0.35 s, the held button: 0.6 s) need real time, not
  frames: `time.sleep` between two `ui` calls.
- `move_to_node` puts the cursor over a visible point of an element; the answer gives the cursor
  (`mouse`), for drags from there.
- Close the windows a section opened before the next one (they stay open in the same editor).
- Look at every picture (a contact sheet of several helps) before using it in the docs.
- The pictures show the game's levels: never put the extracted files themselves in git, only the
  pictures.
