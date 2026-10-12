---
name: build-and-test
description: Build the OpenGOAL Level Editor and run its self-test. Use after any change to src/, lang/ or data/, before saying a change works, and when the link fails because the editor is running.
---

# Build and test the editor

## Build

From the repository root (Ninja + Clang, C++20, CMake 3.20+):

```bash
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release   # first time only
cmake --build build
```

`build/bin/` gets the editor, `ogle-extract`, and copies of `lang/`, `data/` and the game's shaders:
rebuild after editing `lang/*.json` or `data/` too, the editor reads the copies.

**The link fails with "permission denied"** when an editor is running (the user's, or one an MCP
server started). Never kill it (it may hold unsaved work): rename the running file, Windows allows
it, and build again:

```bash
mv build/bin/open-goal-level-editor.exe build/bin/open-goal-level-editor.inuse-1.exe
```

## Self-test

```bash
build/bin/open-goal-level-editor --selftest   # ends with "OK (0 failures)"
```

It needs no game data (it writes a small library of its own) and checks, among others: the scene,
glTF round trips, prefabs and groups, the export for the game, the translations (every key of
`lang/en.json` in every language, the same `%` arguments), the game view, the holes finder, the
weathers, the terrain brush.

Every new piece of logic leaves one check there (`src/app/selftest.cpp`, a `check(condition,
"what it proves")` in a block of its own, before the cleanup at the end of `run_selftest`).

## Check in the running editor

Drive a hidden editor through the remote control to see a change on real levels (needs Jak 3
extracted in `library/`): `tools/capture_docs.py` has a small `Editor` class (one JSON line per
request: `{"id", "tool", "args"}`), and the tool `ui_input` gives it mouse and key events and
pictures of the whole window. Look at the pictures before saying it works.
