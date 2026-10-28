//! The README.md at the top of the export: how to add the level to the game.

use super::*;

/// The README.md at the top of the export: how to add the level to the game.
#[allow(clippy::too_many_arguments)]
pub(super) fn readme(
  scene: &Scene,
  o: &LevelExportOptions,
  rep: &LevelExportReport,
  replaced: Option<&LevelInfo>,
  install_dir: &str,
  code_dir: &str,
  code_stem: &str,
  generated: &str,
  game_gp: &[String],
) -> String {
  let name = &o.name;
  let nick_upper = o.nickname.to_ascii_uppercase();
  let has_code = !generated.is_empty();
  let mut md = format!(
    "# {name} for Jak 3{}\n\nExported by Jak Level Editor from {}: {} meshes ({} triangles), {} collision triangles of the game, {} actors, {} art groups",
    if replaced.is_some() {
      format!(", in place of {}", o.replaces)
    } else {
      String::new()
    },
    if scene.settings.level.is_empty() {
      "a new level"
    } else {
      &scene.settings.level
    },
    rep.meshes,
    rep.triangles,
    rep.collision_triangles,
    rep.actors,
    rep.art_groups
  );
  if rep.navmeshes > 0 {
    md += &format!(", {} nav meshes", rep.navmeshes);
  }
  if rep.behaviors > 0 {
    md += &format!(
      ", {} objects with a behavior of their own (`{code_stem}.gc`)",
      rep.behaviors
    );
  }
  md += &format!(
    ".

## What is in this folder

It is laid out like the jak-project checkout (OpenGOAL's source folder, the one with `goal_src` and
`custom_assets`) it goes into: a checkout of OpenGOAL's jak-project. Its own level builder will do:
the export and the level's code make up for what it does not write (below).

- `{install_dir}`: the level for OpenGOAL's level builder: `{name}.glb` (its decor and collision), `{name}.jsonc` (its names and actors), `{name}.gd` (its DGO, {nick_upper}.DGO), `level-info.gc` (its definition in the game's code){}.
",
    if rep.navmeshes > 0 {
      format!(", `{name}-nav.json` (its nav meshes)")
    } else {
      String::new()
    }
  );
  if has_code {
    md += &format!(
      "- `{code_dir}{code_stem}.gc`: the behaviors set in the editor (the level's code).\n"
    );
  }
  md += &format!(
    "
## Adding it to the game

1. Copy the folder{} of this folder into your jak-project checkout: {} with the folder{} of the same name there (nothing of the game is replaced: the level has a
   folder of its own).
2. In `goal_src/jak3/game.gp`, next to the lines of the test-zone:

   ```
",
    if has_code {
      "s `custom_assets` and `goal_src`"
    } else {
      " `custom_assets`"
    },
    if has_code { "they merge" } else { "it merges" },
    if has_code { "s" } else { "" }
  );
  for line in game_gp {
    md += &format!("   {line}\n");
  }
  md += "   ```\n\n";
  if let Some(r) = replaced {
    let rep_name = &o.replaces;
    let first = r
      .continues
      .first()
      .map_or_else(|| format!("{rep_name}-start"), |c| c.name.clone());
    md += &format!(
      "3. In `goal_src/jak3/engine/level/level-info.gc`, in `(define {rep_name} ...)`, change
   `:nickname '{old}` to `:nickname '{nick}` (see `{install_dir}level-info.gc`). The game loads `<nickname>.DGO` for a level: it now loads {nick_upper}.DGO, this level,
   wherever it loads {rep_name}, and keeps everything else of {rep_name} (its continue points, its music,
   its missions, the levels around it). The game's own `{dgo}` is still built: put the nickname
   back to play the game's {rep_name} again.
4. Build the game: start the compiler (`goalc`, or `task repl` in the checkout) and type `(mi)`.
5. Start the game (`gk -g jak3 -- -boot -fakeiso -debug`) and go there as in the game (from a
   level next to it), or in the REPL: `(lt)` then
   `(start 'play (get-continue-by-name *game-info* \"{first}\"))`.

",
      old = r.nickname,
      nick = o.nickname,
      dgo = r.dgo
    );
  } else {
    md += &format!(
      "3. In `goal_src/jak3/engine/level/level-info.gc`, add the definition of `{install_dir}level-info.gc`
   and give it an `:index` no other level has. The player starts at
   ({}, {}, {}) m.
4. Build the game: start the compiler (`goalc`, or `task repl` in the checkout) and type `(mi)`.
5. Start the game there: start `gk -g jak3 -- -boot -fakeiso -debug`, then in the REPL `(lt)` and
   `(start 'play (get-continue-by-name *game-info* \"{name}-start\"))`.
   (`bg` loads it next to the levels already there: from the city, no memory is left.)

",
      num(o.start.x as f64),
      num(o.start.y as f64),
      num(o.start.z as f64)
    );
  }
  if !rep.notes.is_empty() {
    md += "## What the export changed\n\n";
    for note in &rep.notes {
      md += &format!(
        "- {}\n",
        strf!(tr_en(&note.key), note.count, note.detail.as_str())
      );
    }
    md += "\n";
  }
  md += &format!(
    "## Good to know

- The collision of the levels' own decor is the game's where the decor is still in place (exact,
  and light enough for the game: Jak 3 gathers at most 461 collision triangles around Jak, more
  and he falls through the floor); where a piece was deleted, moved or hidden, it is taken out.
  The pieces placed or moved collide by their shape, made by the builder.
- The code of the game's objects that live in a level's own files is in `{name}.gd`, with
  the files before it in that level's DGO (`goal_src/jak3/dgos/`).
- The paths of the actors move with them; the other positions inside their data (targets) are
  the game's.
- The scripts of the game's objects (their `on-...` data) are left out: OpenGOAL's level
  builder cannot write them. What an object does is its behavior{}",
    if has_code {
      format!(", in `{code_stem}.gc`.\n")
    } else {
      " (here, the game's own).\n".to_string()
    }
  );
  if rep.navmeshes > 0 {
    md += &format!(
      "- The nav meshes are in the level's code, linked when the game births the level, and in
  `{name}-nav.json` (`nav_data`, for a builder that writes them in the level itself).
"
    );
  }
  md += "- The lighting of the decor is the game's: the 8 palettes of each vertex (`_SUNRISE`...), which the
  game mixes with the hour of the day and the level's lights; the pieces made in the editor are lit
  like the level's from their color.
- The surfaces seen through (stains, decals, glass, water, glows) are blended over what is behind
  them: the builder puts them in the level's .fr3, and the level's code adds the tree that draws
  them. Their PS2 blend (`extras.og_blend`: a glow adding light) is the editor's jak-project
  branch's builder's only; OpenGOAL's own blends them as plain transparency.
- What OpenGOAL's own builder lacks, the export writes itself: the collision of the decor colliding
  by its shape, from both sides (invisible meshes `-collision-shape-*`; the game's collision stays
  one-sided), its flat triangles moved by 1/64 m (the builder loses some of a flat floor's), a
  region that does nothing when the level has none (the builder dies without one), and a face
  giving the builder's grid a cell along a side of the level under 7.3 m.
- The level's code (the `-login` function of its `-obs.gc`, which its definition calls when the
  game births it) sets what the builder does not write in the level: the list of what is visible
  (everything: with the PC setting PS2 actor vis, on by default, its actors would never be born),
  the tree drawing its translucent surfaces, its nav meshes. Each only when the level lacks it.
";
  if rep.regions > 0 {
    md += &format!(
      "- The level's regions are the ones of {} ({}): where the game loads and shows the levels around it as Jak or the camera goes by.\n",
      o.replaces, rep.regions
    );
  }
  md
}
