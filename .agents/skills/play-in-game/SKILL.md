---
name: play-in-game
description: Export a level made in the editor and check it in the real game (OpenGOAL's jak-project, gk and goalc). Use when the user wants to play or verify an exported level in Jak 3.
---

# From the editor to the game

**Play** does all of it when the editor has a game runtime (`get_state.runtime.available`): the
toolbar's Play, Level > Play in the game, or the call `play_level {wait?}` (then `play_state`).
It exports into `<library>/exports/<project>/` (with `jle-level.json`, the manifest), installs the
level with `jle_core::io::level_package` (marked blocks `;; jak-level-editor: begin/end <name>` in
`game.gp` and `level-info.gc`, `<file>.jle-backup`, `jle-installed.json`), builds with
`extractor <iso_data/jak3> --folder --compile --game jak3 --proj-path <data>` and starts `gk` with
its own settings (`--config-path`, `-boot -fakeiso -debug`), then goalc's REPL as the user `jle`
(`goal_src/user/jle/startup-jak3.gc`: `(lt)` then `start` at the continue point; Jak 3's kernel has
no argument for it) until the game prints `Displaying level <name>` (`crates/core/src/runtime.rs`). `runtime_remove {name}` takes it out (the files back as they were).
The standalone app's runtime is the jak-project checkout of File > Settings > Game (the editor's
branch `jle/level-builder-fixes`: `custom_assets/jak3/jle-builder.json` says `jle-1`), else the
OpenGOAL Launcher's `<install>/features/jak3/level_edited/`. Never choose the user's own checkout as
the runtime without asking: Play writes into it. Always give jak-project's programs `--proj-path`
(else they find the project by the last "jak-project" in their path).

By hand (no runtime, or to debug the build):

0. **The editor**: `target/release/jak-level-editor.exe` (or `target/debug/`, or
   `npx yarn tauri dev`; build it first: the build-and-test skill), with `--library <folder>` when
   the levels are not in `library/`; or the MCP server, which starts it.
1. **Export**: `Level > Export for the game...` or the MCP tool `export_level` (`name`, `replaces`
   for a level taking the place of one of the game's). It writes `library/exports/<project>/` laid
   out like a jak-project checkout, with a `README.md` giving the exact lines to add.
2. **Into jak-project** (the user's own checkout: ask which branch before touching it, and say how
   to go back): copy `custom_assets/` and `goal_src/` of the export into it, add the lines of the
   export's `README.md` (`game.gp`; `level-info.gc`, or the one `:nickname` to change for a
   replacement). The exported `.glb` holds the game's data: keep it out of git (`.git/info/exclude`).
3. **Build**: the compiler must come from the same branch (`cmake --build out/build/Release
   --target goalc`), then `goalc -g jak3` and `(mi)` in its REPL.
4. **Play**: start `gk` (`-boot -fakeiso -debug` for a debug game), then in goalc `(lt)` and
   `(start 'play (get-continue-by-name *game-info* "<name>-start"))`.

Checks worth doing in the game: the level loads (`(level-status? *level* '<name> #f)`), Jak stands
on its floors (the export's report lists the places denser than the game's collision cache), doors
open, and nothing shows the void (run `find_gaps` in the editor first). Close `gk` and `goalc` when
done.
