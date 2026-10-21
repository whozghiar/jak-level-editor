---
name: build-and-test
description: Build Jak Level Editor (Rust + Tauri + Svelte) and run its tests. Use after any change to crates/, ui/, src/, src-tauri/, lang/ or data/, before saying a change works, and when the link fails because the editor is running.
---

# Build and test the editor

## Build

From the repository root (Rust stable, Node 22; `npx yarn install` once, and again only when
`package.json` changes):

```bash
npx vite build                       # the interface, into dist/: the program embeds it
cargo build -p jak-level-editor      # the program: target/debug/jak-level-editor(.exe)
cargo build -p jak-level-editor-mcp  # the MCP server, next to it
npx yarn tauri dev                   # or: build it and start it, the interface reloading as it changes
npx yarn tauri build                 # the release program and its installers (target/release/bundle/)
```

- A build copies `lang/` and `data/` next to the program (`target/<profile>/`), which reads the
  copies, and the interface embeds the texts of `lang/`: build both again after editing them.
- Several agents at once: each its own target dir (`CARGO_TARGET_DIR=target/<name> cargo ...`), and
  each running editor its own WebView2 data folder (`WEBVIEW2_USER_DATA_FOLDER=<folder>`), else a
  second instance shows no window.
- ogle-extract (C++, only when `extractor/` changed): `cmake -S . -B build -G Ninja
  -DCMAKE_BUILD_TYPE=Release` once, then `cmake --build build` (`build/bin/ogle-extract` and
  `build/bin/ogle-extract-data/`, where the editor finds it).

**The link fails with "Access is denied" / "failed to remove file"** when an editor is running
(the user's, or one an MCP server started). Never kill it (it may hold unsaved work): rename the
running file, Windows allows it, and build again:

```bash
mv target/debug/jak-level-editor.exe target/debug/jak-level-editor.inuse-1.exe
```

The same for `jak-level-editor-mcp.exe` (Claude Code keeps it running) and for
`build/bin/ogle-extract.exe` during an extraction.

## Tests

```bash
cargo test -p jak-level-editor-core           # the engine
cargo test -p jak-level-editor-mcp            # the MCP server's protocol
npx svelte-check --tsconfig ./tsconfig.json   # the interface's types
npx vite build                                # the interface builds
```

They need no game data: the tests that read the extracted game skip themselves when
`library/jak3/library.json` is missing. They check, among others: the scene, glTF round trips,
prefabs and groups, the export for the game, the translations (every key of `lang/en.json` in every
language, with the same `%` arguments), the game view, the holes finder, the weathers, the terrain
brush, the nav meshes.

Every new piece of logic leaves one check: a `#[test]` in the `#[cfg(test)] mod tests` of its file
(or `crates/core/tests/*.rs` for the whole engine).

## Check in the running editor

Run the program on real levels (Jak 3 extracted in `library/`):
`target/debug/jak-level-editor.exe --library library --open-level ctyinda`. Drive it through its
remote control (`tools/capture_docs.py`'s `Editor` class: one JSON line per request, `{"id", "tool",
"args"}`; `capture_view` gives a picture of the 3D view) and through its web view's DevTools
(`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` set before starting it:
pictures of the whole window, mouse and keys; see the doc-captures skill). Look at the pictures
before saying it works.
