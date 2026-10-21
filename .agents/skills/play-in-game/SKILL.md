---
name: play-in-game
description: Export a level made in the editor and check it in the real game (OpenGOAL's jak-project, gk and goalc). Use when the user wants to play or verify an exported level in Jak 3.
---

# From the editor to the game

0. **The editor**: `target/release/jak-level-editor.exe` (or `target/debug/`, or
   `npx yarn tauri dev`; build it first: the build-and-test skill), with `--library <folder>` when
   the levels are not in `library/`; or the MCP server, which starts it.
1. **Export**: `Level > Export for the game...` or the MCP tool `export_level` (`name`, `replaces`
   for a level taking the place of one of the game's). It writes `library/export/<project>/` laid
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
