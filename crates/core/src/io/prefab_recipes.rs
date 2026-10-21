//! The game's prefabs: ready-made assemblies of Jak 3's decor (a slum building with its fans and
//! pipes, a bridge of the port, a floor vent of the industrial zone and its grate, a generator...),
//! described by recipes (data/jak3/prefabs.json: no data of the game, only names) and made from the
//! user's own extracted levels the first time they are needed, into <prefabs>/game/. Jak II's
//! (data/jak2/prefabs.json: Haven's buildings, the palace, the stadium...) are made from its levels
//! extracted next to Jak 3's, into <prefabs>/game/jak2/, their textures named "jak2-<name>" as the
//! structures brought in one by one (io/jak2_import).
//!
//! A recipe names a level, one decor element of it (the anchor: a prototype, the index-th of its
//! instances in the level, or a piece of its shell) and how far around it to take what the game
//! put there (`margin` in meters, `grow`: more below and above, [lo, hi]): every element of the
//! level's decor whose middle is in that box and which is not much larger than it, but the ground
//! and the names `exclude` lists. Or an assembly (scene/assemblies): the structure an element of
//! the level is part of, all of it and only it (a house of the slums, a building of Haven, the
//! palace), as a click and "Select its assembly" select it in the editor; its box when an older
//! extraction has no element of that name.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use serde_json::{Value, json};

use crate::i18n::tr;
use crate::io::asset_library::file_mtime;
use crate::io::game_data::{GameData, game_data_path};
use crate::io::prefabs::{PrefabInfo, save_prefab};
use crate::math::{AABB, Vec3};
use crate::scene::assemblies::{assembly_of, find_assemblies};
use crate::scene::story::{StoryView, apply_story};
use crate::scene::{NodeKind, Scene};
use crate::{log_info, log_warn};

#[derive(Clone, Debug)]
pub struct PrefabRecipe {
  /// shown ("Slum house with fans")
  pub name: String,
  /// "" Jak 3's, "jak2" Jak II's
  pub game: String,
  /// building, house, bridge, platform, vent, generator, industrial...
  pub category: String,
  /// what it is, where it comes from
  pub description: String,
  /// the level it is taken from
  pub level: String,
  /// a prototype ("x.mb") or a piece of the shell
  pub anchor: String,
  /// which instance of the prototype, in the level's order
  pub index: i32,
  /// meters around the anchor
  pub margin: f32,
  /// more of the box on each side (meters)
  pub grow_lo: Vec3,
  pub grow_hi: Vec3,
  /// or a box of the level's space instead of an anchor (a whole block of buildings: its facades,
  /// roofs and what hangs on them)
  pub r#box: AABB,
  /// parts of names left out
  pub exclude: Vec<String>,
  /// or the assembly of this element of the level (cut at max_size meters, as find_assemblies)
  pub assembly: String,
  pub max_size: f32,
  /// the prefab's file in <prefabs>/game/ ("<slug>.glb", Jak II's "jak2/<slug>.glb")
  pub file: String,
}

impl Default for PrefabRecipe {
  fn default() -> Self {
    Self {
      name: String::new(),
      game: String::new(),
      category: String::new(),
      description: String::new(),
      level: String::new(),
      anchor: String::new(),
      index: 0,
      margin: 1.0,
      grow_lo: Vec3::default(),
      grow_hi: Vec3::default(),
      r#box: AABB::default(),
      exclude: Vec::new(),
      assembly: String::new(),
      max_size: 60.0,
      file: String::new(),
    }
  }
}

/// The assemblies of a level found so far, by the size they were cut at: the recipes of a level
/// share them.
pub type LevelAssemblies = Vec<(f32, Vec<Vec<u32>>)>;

fn vec3_of(j: &Value, fallback: Vec3) -> Vec3 {
  match j.as_array() {
    Some(a) if a.len() == 3 => {
      let f = |i: usize| a[i].as_f64().unwrap_or(0.0) as f32;
      Vec3::new(f(0), f(1), f(2))
    }
    _ => fallback,
  }
}

/// a file name made of the recipe's name: "Slum house with fans" -> "slum-house-with-fans"
fn slug(name: &str) -> String {
  let mut out = String::new();
  for c in name.bytes() {
    let l = c.to_ascii_lowercase();
    if l.is_ascii_alphanumeric() {
      out.push(l as char);
    } else if !out.is_empty() && !out.ends_with('-') {
      out.push('-');
    }
  }
  let out = out.trim_end_matches('-');
  if out.is_empty() { "prefab" } else { out }.to_string()
}

/// data/<game>/prefabs.json (next to the game's data); `game`: "" Jak 3, "jak2".
pub fn prefab_recipes_path(game: &str) -> PathBuf {
  let jak3 = game_data_path()
    .parent()
    .map(Path::to_path_buf)
    .unwrap_or_default();
  let dir = if game.is_empty() {
    jak3
  } else {
    jak3.parent().unwrap_or(Path::new("")).join(game)
  };
  dir.join("prefabs.json")
}

/// The recipes of data/<game>/prefabs.json; `game`: "" Jak 3, "jak2".
pub fn load_prefab_recipes(path: &Path, game: &str) -> Result<Vec<PrefabRecipe>, String> {
  let j: Value = std::fs::read(path)
    .ok()
    .and_then(|b| serde_json::from_slice(&b).ok())
    .unwrap_or(Value::Null);
  let Some(list) = j.get("prefabs").and_then(Value::as_array) else {
    return Err(format!("cannot read {}", path.display()));
  };
  let mut out = Vec::new();
  for p in list.iter().filter(|p| p.is_object()) {
    let s = |k: &str, d: &str| p.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
    let f = |k: &str, d: f32| p.get(k).and_then(Value::as_f64).map_or(d, |v| v as f32);
    let mut r = PrefabRecipe {
      game: game.to_string(),
      name: s("name", ""),
      category: s("category", "other"),
      description: s("description", ""),
      level: s("level", ""),
      anchor: s("anchor", ""),
      index: p
        .get("index")
        .and_then(Value::as_f64)
        .map_or(0, |v| v as i32),
      margin: f("margin", 1.0),
      assembly: s("assembly", ""),
      max_size: f("max_size", 60.0),
      ..Default::default()
    };
    if let Some(g) = p
      .get("grow")
      .and_then(Value::as_array)
      .filter(|g| g.len() == 2)
    {
      r.grow_lo = vec3_of(&g[0], r.grow_lo);
      r.grow_hi = vec3_of(&g[1], r.grow_hi);
    }
    for e in p
      .get("exclude")
      .and_then(Value::as_array)
      .into_iter()
      .flatten()
    {
      if let Some(e) = e.as_str() {
        r.exclude.push(e.to_string());
      }
    }
    if let Some(b) = p
      .get("box")
      .and_then(Value::as_array)
      .filter(|b| b.len() == 2)
    {
      r.r#box.add(vec3_of(&b[0], Vec3::default()));
      r.r#box.add(vec3_of(&b[1], Vec3::default()));
    }
    if r.name.is_empty()
      || r.level.is_empty()
      || (r.anchor.is_empty() && r.assembly.is_empty() && !r.r#box.valid())
    {
      continue;
    }
    let dir = if game.is_empty() {
      String::new()
    } else {
      format!("{game}/")
    };
    r.file = format!("{dir}{}.glb", slug(&r.name));
    out.push(r);
  }
  Ok(out)
}

/// The elements of `level` (a document of it: opened without actors) a recipe takes: the anchor
/// and what is around it, or its assembly.
pub fn recipe_members(
  level: &mut Scene,
  r: &PrefabRecipe,
  assemblies: Option<&mut LevelAssemblies>,
) -> Result<Vec<u32>, String> {
  level.begin_frame();
  let c = level.cache();
  if !r.assembly.is_empty() {
    let element = level.nodes.iter().enumerate().find(|(i, n)| {
      n.name == r.assembly && n.kind == NodeKind::Render && n.mesh.is_some() && c.visible[*i]
    });
    if let Some((_, n)) = element {
      let id = n.id;
      let mut own = LevelAssemblies::new();
      let found = assemblies.unwrap_or(&mut own);
      let at = match found.iter().position(|(size, _)| *size == r.max_size) {
        Some(at) => at,
        None => {
          found.push((r.max_size, find_assemblies(level, r.max_size)));
          found.len() - 1
        }
      };
      let out = assembly_of(&found[at].1, id);
      if !out.is_empty() {
        return Ok(out);
      }
    }
    if !r.r#box.valid() {
      return Err(format!("no {} in {}", r.assembly, r.level));
    }
  }
  // the anchor: the index-th instance of the prototype (or the node of that name: a piece)
  let mut anchor = None;
  let mut seen = 0;
  if !r.anchor.is_empty() {
    for (i, n) in level.nodes.iter().enumerate() {
      if n.kind != NodeKind::Render || n.mesh.is_none() {
        continue;
      }
      let parent = usize::try_from(c.parent[i]).ok();
      let is_it = n.name == r.anchor || parent.is_some_and(|p| level.nodes[p].name == r.anchor);
      if is_it {
        if seen == r.index {
          anchor = Some(i);
          break;
        }
        seen += 1;
      }
    }
  }
  if anchor.is_none() && !r.r#box.valid() {
    return Err(format!("no {} ({}) in {}", r.anchor, r.index, r.level));
  }
  let mut bx = anchor.map_or(r.r#box, |a| c.bounds[a].expanded(r.margin));
  bx.lo -= r.grow_lo;
  bx.hi += r.grow_hi;
  let room = bx.size() * 1.25;
  let mut out = Vec::new();
  for (i, n) in level.nodes.iter().enumerate() {
    if n.kind != NodeKind::Render
      || n.mesh.is_none()
      || n.locked
      || !c.visible[i]
      || c.actor[i] != 0
    {
      continue;
    }
    let b = &c.bounds[i];
    if !b.valid() || !bx.contains(b.center()) {
      continue;
    }
    let s = b.size();
    if Some(i) != anchor && (s.x > room.x || s.y > room.y || s.z > room.z) {
      continue; // a wall going on far away
    }
    // the far scenery
    let mut left_out = false;
    let mut p = c.parent[i];
    while let Ok(at) = usize::try_from(p)
      && !left_out
    {
      left_out = level.nodes[at].name.contains("-lowres");
      p = c.parent[at];
    }
    left_out |= r.exclude.iter().any(|e| n.name.contains(e.as_str()));
    if !left_out || Some(i) == anchor {
      out.push(n.id);
    }
  }
  Ok(out)
}

/// Makes a recipe's prefab at `path` from its level (opened without actors): how many elements it
/// holds. The error is the level's name when the recipe takes nothing there.
pub fn save_recipe_prefab(
  level: &mut Scene,
  r: &PrefabRecipe,
  path: &str,
  assemblies: Option<&mut LevelAssemblies>,
) -> Result<usize, String> {
  let members = recipe_members(level, r, assemblies)?;
  if members.is_empty() {
    return Err(r.level.clone());
  }
  save_prefab(level, &members, path)?;
  Ok(members.len())
}

/// The game's prefabs of `recipes` as the panel lists them: made when their file is in
/// <folder>/game/.
pub fn game_prefab_infos(recipes: &[PrefabRecipe], folder: &str) -> Vec<PrefabInfo> {
  recipes
    .iter()
    .map(|r| {
      let path = format!("{folder}/game/{}", r.file);
      let made = Path::new(&path).is_file();
      PrefabInfo {
        name: r.name.clone(),
        mtime: if made { file_mtime(&path) } else { 0 },
        path,
        made,
        category: r.category.clone(),
        description: r.description.clone(),
        game: r.game.clone(),
        level: r.level.clone(),
      }
    })
    .collect()
}

/// Makes the prefabs of `recipes` not made yet in <folder>/game/ (all of them with `rebuild`; only
/// those of `levels`, (game, level), when it is not empty) on a worker thread: each level opened
/// once for all its recipes, by `open_level` ((game "" or "jak2", level folder) -> the level
/// without its actors: the editor's open_level). `done` counts the recipes done; the thread logs
/// and gives how many prefabs were made. None when there is nothing to make; else the thread and
/// how many recipes it makes.
pub fn start_game_prefabs(
  recipes: &[PrefabRecipe],
  folder: &str,
  rebuild: bool,
  levels: &BTreeSet<(String, String)>,
  data: Option<Arc<GameData>>,
  done: Arc<AtomicUsize>,
  open_level: impl Fn(&str, &str) -> Result<Scene, String> + Send + 'static,
) -> Option<(JoinHandle<usize>, usize)> {
  let todo: Vec<PrefabRecipe> = recipes
    .iter()
    .filter(|r| {
      (levels.is_empty() || levels.contains(&(r.game.clone(), r.level.clone())))
        && (rebuild || !Path::new(&format!("{folder}/game/{}", r.file)).is_file())
    })
    .cloned()
    .collect();
  if todo.is_empty() {
    return None;
  }
  let total = todo.len();
  done.store(0, Ordering::Relaxed);
  log_info!(tr("log.game_prefabs_started"), total);
  let folder = folder.to_string();
  let work = std::thread::spawn(move || {
    // each level opened once (without its actors) for all its recipes
    let mut by_level: BTreeMap<(&str, &str), Vec<&PrefabRecipe>> = BTreeMap::new();
    for r in &todo {
      by_level
        .entry((r.game.as_str(), r.level.as_str()))
        .or_default()
        .push(r);
    }
    let mut built = 0;
    for ((game, name), recipes) in by_level {
      let jak2 = game == "jak2";
      let mut opened = open_level(game, name);
      if let Ok(level) = &mut opened {
        if jak2 {
          // Jak II's textures and materials named apart from Jak 3's, where its pieces come from
          // kept
          for t in &mut level.textures {
            if !t.name.starts_with("jak2-") {
              t.name = format!("jak2-{}", t.name);
            }
          }
          for m in &mut level.materials {
            if !m.name.starts_with("jak2-") {
              m.name = format!("jak2-{}", m.name);
            }
          }
          for n in &mut level.nodes {
            if n.kind == NodeKind::Render && n.mesh.is_some() {
              n.extras["og_import"] =
                json!({"game": "jak2", "level": name, "part": n.name, "checks": []});
            }
          }
        } else {
          // as the editor shows it at the start of the story (one state of what changes: a
          // facade whole or blasted, not both)
          apply_story(level, data.as_deref(), &StoryView::default());
        }
      }
      // found once for all the recipes of the level
      let mut assemblies = LevelAssemblies::new();
      for r in recipes {
        let path = format!("{folder}/game/{}", r.file);
        let made = match &mut opened {
          Ok(level) => save_recipe_prefab(level, r, &path, Some(&mut assemblies)),
          Err(e) => Err(e.clone()),
        };
        match made {
          Ok(_) => built += 1,
          Err(e) => log_warn!(
            "%s: %s",
            &r.name,
            if e.is_empty() { name } else { e.as_str() }
          ),
        }
        done.fetch_add(1, Ordering::Relaxed);
      }
    }
    log_info!(tr("log.game_prefabs_done"), built, total);
    built
  });
  Some((work, total))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::test_library::{make_box, temp_dir};
  use crate::scene::Node;

  /// a box of `size` (make_box's, 2 m) at `at`
  fn put(s: &mut Scene, name: &str, parent: u32, at: Vec3, size: Vec3) -> u32 {
    let mut n = Node {
      name: name.into(),
      kind: NodeKind::Render,
      parent,
      mesh: Some(make_box(name, -1)),
      ..Default::default()
    };
    n.local.t = at;
    n.local.s = size;
    s.add(n)
  }

  fn fan_level() -> (Scene, u32, u32) {
    let mut lvl = Scene::new();
    let proto = lvl.add(Node {
      name: "fan.mb".into(),
      ..Default::default()
    });
    let one = Vec3::new(1.0, 1.0, 1.0);
    put(&mut lvl, "fan.mb-0", proto, Vec3::new(50.0, 0.0, 0.0), one); // the first fan elsewhere
    let anchor = put(&mut lvl, "fan.mb-1", proto, Vec3::default(), one);
    let bolt = put(&mut lvl, "bolt", 0, Vec3::new(1.5, 0.5, 0.0), one * 0.2);
    put(&mut lvl, "far-crate", 0, Vec3::new(10.0, 0.0, 0.0), one);
    put(
      &mut lvl,
      "street-dirt",
      0,
      Vec3::new(0.5, 0.0, 0.5),
      one * 0.2,
    );
    put(
      &mut lvl,
      "huge-wall",
      0,
      Vec3::new(0.0, 0.0, 0.5),
      one * 40.0,
    );
    (lvl, anchor, bolt)
  }

  fn fan_recipe() -> PrefabRecipe {
    PrefabRecipe {
      name: "Fan".into(),
      level: "test".into(),
      anchor: "fan.mb".into(),
      index: 1,
      margin: 1.0,
      exclude: vec!["dirt".into()],
      file: "fan.glb".into(),
      ..Default::default()
    }
  }

  // the game's prefabs: their recipes shipped with the editor, what one takes around its anchor
  #[test]
  fn recipes_shipped_and_members() {
    let recipes = load_prefab_recipes(&prefab_recipes_path(""), "").unwrap();
    let categories: BTreeSet<&str> = recipes.iter().map(|r| r.category.as_str()).collect();
    let assemblies = recipes.iter().filter(|r| !r.assembly.is_empty()).count();
    assert!(recipes.len() >= 120 && assemblies >= recipes.len() * 3 / 4);
    for c in ["building", "house", "bridge", "tower", "industrial"] {
      assert!(categories.contains(c), "category {c}");
    }
    // Jak II's: their own files, apart from Jak 3's
    let jak2 = load_prefab_recipes(&prefab_recipes_path("jak2"), "jak2").unwrap();
    assert!(
      !jak2.is_empty()
        && jak2
          .iter()
          .all(|j| j.game == "jak2" && j.file.starts_with("jak2/"))
    );
    assert!(load_prefab_recipes(Path::new("no-such-file.json"), "").is_err());
    assert_eq!(slug("Slum house (with fans)!"), "slum-house-with-fans");
    assert_eq!(slug("!!"), "prefab");
    // a recipe takes its anchor and what is on it, not what is far, too large or left out by name
    let (mut lvl, anchor, bolt) = fan_level();
    assert_eq!(
      recipe_members(&mut lvl, &fan_recipe(), None).unwrap(),
      vec![anchor, bolt]
    );
    let missing = PrefabRecipe {
      index: 5,
      ..fan_recipe()
    };
    assert_eq!(
      recipe_members(&mut lvl, &missing, None).unwrap_err(),
      "no fan.mb (5) in test"
    );
  }

  // a recipe of the game's prefabs finds an assembly by the name of one of its elements
  #[test]
  fn recipe_of_an_assembly() {
    let mut s = Scene::new();
    let mut house = Vec::new();
    for (name, at, size) in [
      ("A-front", (0.0, 0.0, 0.0), (10.0, 6.0, 1.0)),
      ("A-back", (0.0, 0.0, 8.0), (10.0, 6.0, 1.0)),
      ("A-left", (-5.0, 0.0, 4.0), (1.0, 6.0, 9.0)),
      ("A-right", (5.0, 0.0, 4.0), (1.0, 6.0, 9.0)),
      ("A-roof", (0.0, 6.0, 4.0), (11.0, 0.5, 10.0)),
      ("A-lamp-post", (2.0, 0.0, 4.0), (0.3, 4.0, 0.3)),
    ] {
      let (at, size) = (
        Vec3::new(at.0, at.1, at.2),
        Vec3::new(size.0, size.1, size.2),
      );
      house.push(put(&mut s, name, 0, at, size * 0.5));
    }
    put(
      &mut s,
      "sidewalk",
      0,
      Vec3::new(15.0, -0.3, -1.0),
      Vec3::new(40.0, 0.3, 10.0) * 0.5,
    );
    let r = PrefabRecipe {
      level: "test".into(),
      assembly: "A-roof".into(),
      ..Default::default()
    };
    let mut found = LevelAssemblies::new();
    let members = recipe_members(&mut s, &r, Some(&mut found)).unwrap();
    house.sort();
    assert_eq!(members, house);
    assert_eq!(
      found.len(),
      1,
      "the level's assemblies kept for its other recipes"
    );
    let none = PrefabRecipe {
      assembly: "B-roof".into(),
      ..r
    };
    assert_eq!(
      recipe_members(&mut s, &none, None).unwrap_err(),
      "no B-roof in test"
    );
  }

  // the worker: each level opened once, the prefabs made in <folder>/game/, the recipes counted
  #[test]
  fn game_prefabs_made_on_a_worker() {
    let folder = temp_dir("game-prefabs");
    let folder = folder.to_str().unwrap();
    let recipes = vec![
      fan_recipe(),
      PrefabRecipe {
        name: "Nothing".into(),
        index: 9,
        file: "nothing.glb".into(),
        ..fan_recipe()
      },
      PrefabRecipe {
        name: "Elsewhere".into(),
        level: "other".into(),
        file: "elsewhere.glb".into(),
        ..fan_recipe()
      },
    ];
    let opened = Arc::new(AtomicUsize::new(0));
    let open = {
      let opened = opened.clone();
      move |game: &str, level: &str| {
        assert_eq!(game, "");
        opened.fetch_add(1, Ordering::Relaxed);
        if level == "test" {
          Ok(fan_level().0)
        } else {
          Err(format!("cannot read {level}"))
        }
      }
    };
    let done = Arc::new(AtomicUsize::new(0));
    let (work, total) = start_game_prefabs(
      &recipes,
      folder,
      false,
      &BTreeSet::new(),
      None,
      done.clone(),
      open.clone(),
    )
    .unwrap();
    assert_eq!(total, 3);
    assert_eq!(work.join().unwrap(), 1);
    assert_eq!(done.load(Ordering::Relaxed), 3);
    assert_eq!(
      opened.load(Ordering::Relaxed),
      2,
      "a level opened once for its recipes"
    );
    let infos = game_prefab_infos(&recipes, folder);
    assert!(infos[0].made && infos[0].mtime > 0 && !infos[1].made && !infos[2].made);
    // made already: only what is missing, and only the levels asked for
    let levels = BTreeSet::from([(String::new(), "test".to_string())]);
    let (work, total) = start_game_prefabs(
      &recipes,
      folder,
      false,
      &levels,
      None,
      done.clone(),
      open.clone(),
    )
    .unwrap();
    assert_eq!((work.join().unwrap(), total), (0, 1));
    let only_made = vec![fan_recipe()];
    assert!(start_game_prefabs(&only_made, folder, false, &levels, None, done, open).is_none());
  }
}
