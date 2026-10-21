//! What the editor knows of Jak 3 beyond its extracted files: data/jak3/game-data.json, made by
//! tools/gen_game_data.py from the decompiled game code.
//!
//!   actor roles  the actor types that are creatures (enemies, characters, the Titan suit) or logic
//!                (particle spawners, cameras, triggers): levels open without them
//!   water        the model of each "look" of the water actors (water, dark eco and lava pools)
//!   oceans       where the ocean of a level is, and its height
//!   levels       per level: its task level and mask, its ocean, whether it is a layer (the actors
//!                a mission adds to another level), its companions (levels the game shows with it),
//!                its neighbors (the levels the game displays together with it somewhere) and its
//!                continue points (where the game starts the player, and the levels shown there)
//!   story        the task nodes in the order of the game, and what they change in the levels
//!   params       per object type, the res lumps its code reads (what configures an actor of it)
//!   parents      the parent of each such type, up to process (what kind of object it is)
//!   code         the code file of the types outside the common code, and what each file needs
//!   enums        the enums of those lumps (option bits, pickups)
//!
//! The story: each level has a task mask, changed by the task nodes of its
//! task level as they close; an actor exists while its kill mask and that mask share no bit.
//! Some decor prototypes and actor types are shown or hidden by conditions on the nodes, and open
//! nodes lend layers to levels.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::math::Vec3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActorRole {
  #[default]
  Object,
  Creature,
  Logic,
}

/// A condition on the story, evaluated at a point of the game. Closed, Open: a node; Complete: a
/// task.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum StoryCond {
  False,
  #[default]
  True,
  Closed(usize),
  Open(usize),
  Complete(usize),
  Not(Box<StoryCond>),
  And(Vec<StoryCond>),
  Or(Vec<StoryCond>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoryBorrow {
  /// the level borrowing
  pub host: String,
  pub slot: i32,
  /// the layer it shows, "" to empty the slot
  pub level: String,
  /// only the layer's actors meant for it are shown ('special)
  pub special: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskOp {
  #[default]
  NoMask,
  Abs,
  Set,
  Clear,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoryNode {
  pub name: String,
  /// the task level whose mask it changes when it closes
  pub level: String,
  /// its task (StoryCond::Complete)
  pub task: Option<usize>,
  pub mask_op: MaskOp,
  pub mask: u32,
  /// closing it completes its task
  pub close_task: bool,
  /// closed from the start of the game
  pub closed: bool,
  pub parents: Vec<usize>,
  pub borrow: Vec<StoryBorrow>,
}

/// A place where the game starts the player (a continue point), in the editor's space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContinuePoint {
  pub name: String,
  /// meters
  pub position: Vec3,
  /// the levels the game displays there
  pub shows: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelInfo {
  /// the task level of its task mask
  pub taskname: String,
  /// its DGO: the game loads <nickname>.DGO for it ("" for a custom level: the game's code does
  /// not build its DGO); the code in it beyond the common code (.o stems), which a level replacing
  /// it brings in its own DGO
  pub nickname: String,
  pub dgo: String,
  pub code: Vec<String>,
  pub base_mask: u32,
  /// its ocean map, "" without one
  pub ocean: String,
  /// set by the level (Jak 3), else the map's
  pub ocean_height: Option<f32>,
  /// the far ocean is drawn while it is shown (the sea to the horizon)
  pub ocean_far: bool,
  /// borrowed by the story: its actors appear in other levels
  pub layer: bool,
  /// part of the city: the actors of the city traffic are there
  pub traffic: bool,
  pub companions: Vec<String>,
  /// what its slots show before the story changes them
  pub borrow: Vec<StoryBorrow>,
  /// the levels the game displays together with it at a continue point (layers left out): the
  /// levels around it in the game's world
  pub neighbors: Vec<String>,
  pub continues: Vec<ContinuePoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OceanMap {
  /// meters; y: its height
  pub corner: Vec3,
  pub cell: f32,
  pub size: usize,
  /// size x size, rows along z: 1 where the ocean is (the mid ocean's)
  pub cells: Vec<u8>,
  /// the near ocean's water by the shores (the game draws it near the camera, the mid ocean's
  /// cells masked there): per cell (row * size + column), 32 rows of 32 squares of 3 m (bit x of
  /// row z: 1 water); an empty row list: water everywhere in the cell
  pub near: BTreeMap<usize, Vec<u32>>,
  pub color: Vec3,
  /// the far ocean's
  pub far_color: Vec3,
}

impl Default for OceanMap {
  fn default() -> Self {
    Self {
      corner: Vec3::default(),
      cell: 96.0,
      size: 48,
      cells: Vec::new(),
      near: BTreeMap::new(),
      color: Vec3::new(0.1, 0.3, 0.35),
      far_color: Vec3::new(0.02, 0.24, 0.29),
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrototypeRule {
  /// decor prototypes, without ".mb"
  pub names: Vec<String>,
  /// shown when true
  pub when: StoryCond,
}

/// A point of the story: the nodes closed and open, the tasks complete.
#[derive(Clone, Debug, PartialEq)]
pub struct StoryMoment {
  pub step: i32,
  /// per node
  pub closed: Vec<bool>,
  pub open: Vec<bool>,
  /// per task
  pub complete: Vec<bool>,
}

/// A res lump the code of a type reads: tag -> kind ("float", "floats", "int", "enum:<name>",
/// "symbol", "string", "vector", "path", "actor", "actor-groups", "eco-info", "script", "data")
/// and default (null when none)
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
  pub tag: String,
  pub kind: String,
  pub def: Value,
}

/// An enum of those lumps: its names and values (bit indices for a bitfield)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Enum {
  pub bitfield: bool,
  pub values: Vec<(String, i64)>,
}

#[derive(Clone, Debug)]
struct HeightRule {
  when: StoryCond,
  height: f32,
}

#[derive(Clone, Debug)]
struct MaskBits {
  special: u32,
  primary: u32,
  ctywide: u32,
  never: u32,
}

impl Default for MaskBits {
  fn default() -> Self {
    Self {
      special: 1 << 12,
      primary: 1 << 13,
      ctywide: 1 << 14,
      never: 1 << 15,
    }
  }
}

/// The game's data, read from game-data.json (an empty one knows nothing: no story).
#[derive(Clone, Debug, Default)]
pub struct GameData {
  roles: HashMap<String, ActorRole>,
  creature_suffixes: Vec<String>,
  water_types: HashSet<String>,
  effects: BTreeSet<String>,
  unlisted: BTreeSet<String>,
  placeholders: BTreeSet<String>,
  water_looks: Vec<String>,
  objects: HashMap<String, String>,
  catalog: HashMap<String, String>,
  type_models: HashMap<String, Vec<String>>,
  skeletons: HashMap<String, String>,
  art_groups: HashMap<String, String>,
  common_art_groups: BTreeSet<String>,
  parents: HashMap<String, String>,
  code: HashMap<String, String>,
  effect_code: HashMap<String, String>,
  code_needs: HashMap<String, Vec<String>>,
  params: HashMap<String, Vec<Param>>,
  enums: HashMap<String, Enum>,
  levels: BTreeMap<String, LevelInfo>,
  oceans: BTreeMap<String, OceanMap>,
  nodes: Vec<StoryNode>,
  node_index: HashMap<String, usize>,
  tasks: Vec<String>,
  /// per task: the nodes completing it
  task_closers: Vec<Vec<usize>>,
  /// its borrows empty the slots first
  reset_node: Option<usize>,
  prototypes: BTreeMap<String, Vec<PrototypeRule>>,
  types: HashMap<String, StoryCond>,
  heights: BTreeMap<String, Vec<HeightRule>>,
  bits: MaskBits,
}

// ------------------------------------------------------------------------------------------------
// reading
// ------------------------------------------------------------------------------------------------

static NULL: Value = Value::Null;

// j[key] without a copy, null when missing (iterating it gives nothing)
fn at<'a>(j: &'a Value, key: &str) -> &'a Value {
  j.get(key).unwrap_or(&NULL)
}

fn str_of(j: &Value) -> String {
  j.as_str().unwrap_or_default().to_string()
}

fn list(j: &Value) -> impl Iterator<Item = &Value> {
  j.as_array().into_iter().flatten()
}

fn items(j: &Value) -> impl Iterator<Item = (&String, &Value)> {
  j.as_object().into_iter().flatten()
}

fn strings(j: &Value) -> Vec<String> {
  list(j).map(str_of).collect()
}

fn bool_at(j: &Value, key: &str) -> bool {
  j.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn u32_at(j: &Value, key: &str) -> u32 {
  j.get(key)
    .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
    .map_or(0, |v| v as u32)
}

fn f32_of(j: &Value) -> f32 {
  j.as_f64().unwrap_or(0.0) as f32
}

fn vec3_of(j: &Value) -> Option<Vec3> {
  let a = j.as_array().filter(|a| a.len() == 3)?;
  Some(Vec3::new(f32_of(&a[0]), f32_of(&a[1]), f32_of(&a[2])))
}

fn read_borrow(b: &[Value], host: &str, slot: i32) -> StoryBorrow {
  StoryBorrow {
    host: host.to_string(),
    slot,
    level: str_of(&b[0]),
    special: b[1].as_str() == Some("special"),
  }
}

// conditions: names -> indices
fn read_cond(
  e: &Value,
  nodes: &HashMap<String, usize>,
  tasks: &HashMap<String, usize>,
) -> StoryCond {
  if let Some(b) = e.as_bool() {
    return if b { StoryCond::True } else { StoryCond::False };
  }
  let Some(a) = e.as_array() else {
    return StoryCond::True;
  };
  let Some(op) = a.first().and_then(Value::as_str) else {
    return StoryCond::True;
  };
  match (op, a.len()) {
    ("closed" | "open", 2) => match nodes.get(&str_of(&a[1])) {
      Some(&i) if op == "closed" => StoryCond::Closed(i),
      Some(&i) => StoryCond::Open(i),
      None => StoryCond::True,
    },
    ("complete", 2) => tasks
      .get(&str_of(&a[1]))
      .map_or(StoryCond::True, |&t| StoryCond::Complete(t)),
    ("not", 2) => StoryCond::Not(Box::new(read_cond(&a[1], nodes, tasks))),
    ("and" | "or", n) if n > 1 => {
      let args = a[1..].iter().map(|x| read_cond(x, nodes, tasks)).collect();
      if op == "and" {
        StoryCond::And(args)
      } else {
        StoryCond::Or(args)
      }
    }
    _ => StoryCond::True,
  }
}

static DATA_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The folder of the game's tables the application ships (its resources): `data_dir()` then
/// returns it.
pub fn set_data_dir(dir: impl Into<PathBuf>) {
  *DATA_DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.into());
}

/// The folder of the game's tables (jak3/game-data.json, jak3/prefabs.json...): the one the
/// application set, else data/ next to the executable, else the repository's (a build in
/// development).
pub fn data_dir() -> PathBuf {
  if let Some(dir) = DATA_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
    return dir;
  }
  if let Some(dir) = std::env::current_exe()
    .ok()
    .and_then(|e| e.parent().map(|p| p.join("data")))
    && dir.is_dir()
  {
    return dir;
  }
  Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// data/jak3/game-data.json (data_dir()).
pub fn game_data_path() -> PathBuf {
  data_dir().join("jak3").join("game-data.json")
}

impl GameData {
  pub fn load(path: &Path) -> Result<GameData, String> {
    let bytes = std::fs::read(path).map_err(|_| format!("cannot read {}", path.display()))?;
    let j: Value = serde_json::from_slice(&bytes).unwrap_or_default();
    Self::from_json(&j).ok_or_else(|| format!("not a game data file: {}", path.display()))
  }

  /// The game data of a game-data.json's content; None when it is not one.
  pub fn from_json(j: &Value) -> Option<GameData> {
    if !j.is_object() || j.get("format").and_then(Value::as_f64) != Some(1.0) {
      return None;
    }
    let mut d = GameData::default();

    // actors
    let actors = at(j, "actors");
    for (t, role) in items(at(actors, "roles")) {
      let role = if role.as_str() == Some("creature") {
        ActorRole::Creature
      } else {
        ActorRole::Logic
      };
      d.roles.insert(t.clone(), role);
    }
    d.creature_suffixes = strings(at(actors, "creature_suffixes"));
    d.water_types = strings(at(actors, "water_types")).into_iter().collect();
    d.effects = strings(at(actors, "effects")).into_iter().collect();
    d.unlisted = strings(at(actors, "unlisted")).into_iter().collect();
    d.placeholders = strings(at(actors, "placeholders")).into_iter().collect();
    d.water_looks = strings(at(actors, "water_looks"));
    let string_map = |j: &Value| -> HashMap<String, String> {
      items(j).map(|(k, v)| (k.clone(), str_of(v))).collect()
    };
    d.objects = string_map(at(actors, "objects"));
    d.catalog = string_map(at(actors, "catalog"));
    d.type_models = items(at(actors, "models"))
      .map(|(t, models)| (t.clone(), strings(models)))
      .collect();
    d.skeletons = string_map(at(actors, "skeletons"));
    d.art_groups = string_map(at(actors, "art_groups"));
    d.common_art_groups = strings(at(actors, "common_art_groups"))
      .into_iter()
      .collect();
    d.parents = string_map(at(actors, "parents"));
    d.code = string_map(at(actors, "code"));
    d.effect_code = string_map(at(actors, "effect_code"));
    d.code_needs = items(at(j, "code_needs"))
      .map(|(stem, files)| (stem.clone(), strings(files)))
      .collect();
    for (t, ps) in items(at(actors, "params")) {
      let mut list: Vec<Param> = items(ps)
        .filter_map(|(tag, p)| {
          let p = p.as_array().filter(|p| p.len() == 2)?;
          Some(Param {
            tag: tag.clone(),
            kind: str_of(&p[0]),
            def: p[1].clone(),
          })
        })
        .collect();
      // by tag, as the C++ editor's json kept them, whatever the order of the file
      list.sort_by(|a, b| a.tag.cmp(&b.tag));
      d.params.insert(t.clone(), list);
    }
    for (name, e) in items(at(j, "enums")) {
      let values = list(at(e, "values"))
        .filter_map(|v| {
          let v = v.as_array().filter(|v| v.len() == 2)?;
          let n = v[1].as_i64().or_else(|| v[1].as_f64().map(|f| f as i64))?;
          Some((str_of(&v[0]), n))
        })
        .collect();
      d.enums.insert(
        name.clone(),
        Enum {
          bitfield: bool_at(e, "bitfield"),
          values,
        },
      );
    }

    // oceans
    for (name, o) in items(at(j, "oceans")) {
      let mut map = OceanMap::default();
      if let Some(c) = vec3_of(at(o, "corner")) {
        map.corner = c;
      }
      map.cell = o
        .get("cell")
        .and_then(Value::as_f64)
        .map_or(96.0, |c| c as f32);
      let rows = at(o, "cells").as_array().map_or(&[][..], |r| r.as_slice());
      map.size = rows.len();
      map.cells = vec![0; map.size * map.size];
      for (r, row) in rows.iter().enumerate() {
        let row = row.as_str().unwrap_or_default().as_bytes();
        for (col, &ch) in row.iter().take(map.size).enumerate() {
          map.cells[r * map.size + col] = (ch == b'1') as u8;
        }
      }
      for (key, v) in items(at(o, "near")) {
        let Some((r, col)) = key.split_once(',') else {
          continue;
        };
        let (Ok(r), Ok(col), Some(hex)) = (
          r.trim().parse::<usize>(),
          col.trim().parse::<usize>(),
          v.as_str(),
        ) else {
          continue;
        };
        let mut bits = Vec::new();
        if hex != "full" {
          let mut i = 0;
          while i + 8 <= hex.len() && bits.len() < 32 {
            bits.push(u32::from_str_radix(hex.get(i..i + 8).unwrap_or(""), 16).unwrap_or(0));
            i += 8;
          }
        }
        if hex == "full" || bits.len() == 32 {
          map.near.insert(r * map.size + col, bits);
        }
      }
      if let Some(c) = vec3_of(at(o, "color")) {
        map.color = c;
      }
      map.far_color = vec3_of(at(o, "far_color")).unwrap_or(map.color);
      d.oceans.insert(name.clone(), map);
    }

    // levels
    for (name, l) in items(at(j, "levels")) {
      let text = |key: &str| str_of(at(l, key));
      let info = LevelInfo {
        taskname: l
          .get("taskname")
          .and_then(Value::as_str)
          .unwrap_or(name)
          .to_string(),
        base_mask: u32_at(l, "base_mask"),
        ocean: text("ocean"),
        ocean_height: l
          .get("ocean_height")
          .and_then(Value::as_f64)
          .map(|h| h as f32),
        layer: bool_at(l, "layer"),
        ocean_far: bool_at(l, "ocean_far"),
        traffic: bool_at(l, "traffic"),
        nickname: text("nickname"),
        dgo: text("dgo"),
        code: strings(at(l, "code")),
        companions: strings(at(l, "companions")),
        neighbors: strings(at(l, "neighbors")),
        continues: list(at(l, "continues"))
          .map(|c| ContinuePoint {
            name: str_of(at(c, "name")),
            position: vec3_of(at(c, "position")).unwrap_or_default(),
            shows: strings(at(c, "shows")),
          })
          .collect(),
        borrow: list(at(l, "borrow"))
          .enumerate()
          .filter_map(|(i, b)| {
            let b = b.as_array().filter(|b| b.len() == 2)?;
            Some(read_borrow(b, name, i as i32))
          })
          .collect(),
      };
      d.levels.insert(name.clone(), info);
    }

    // story
    let story = at(j, "story");
    if story.is_object() {
      let bits = at(story, "mask_bits");
      let bit = |name: &str, fallback: u32| {
        bits
          .get(name)
          .and_then(Value::as_i64)
          .map_or(fallback, |b| 1u32.checked_shl(b as u32).unwrap_or(0))
      };
      d.bits = MaskBits {
        special: bit("special", d.bits.special),
        primary: bit("primary0", d.bits.primary),
        ctywide: bit("ctywide", d.bits.ctywide),
        never: bit("never", d.bits.never),
      };
      let nodes = at(story, "nodes")
        .as_array()
        .map_or(&[][..], |n| n.as_slice());
      let mut task_index: HashMap<String, usize> = HashMap::new();
      for (i, n) in nodes.iter().enumerate() {
        d.node_index.insert(str_of(at(n, "name")), i);
      }
      for (i, n) in nodes.iter().enumerate() {
        let task = str_of(at(n, "task"));
        let task = (!task.is_empty()).then(|| {
          *task_index.entry(task.clone()).or_insert_with(|| {
            d.tasks.push(task.clone());
            d.task_closers.push(Vec::new());
            d.tasks.len() - 1
          })
        });
        let node = StoryNode {
          name: str_of(at(n, "name")),
          level: str_of(at(n, "level")),
          task,
          mask_op: match at(n, "mask_op").as_str() {
            Some("abs") => MaskOp::Abs,
            Some("set") => MaskOp::Set,
            Some("clear") => MaskOp::Clear,
            _ => MaskOp::NoMask,
          },
          mask: u32_at(n, "mask"),
          close_task: bool_at(n, "close_task"),
          closed: bool_at(n, "closed") || i == 0,
          parents: list(at(n, "parents"))
            .filter_map(|p| p.as_i64())
            .filter(|&p| p >= 0 && (p as usize) < nodes.len())
            .map(|p| p as usize)
            .collect(),
          borrow: list(at(n, "borrow"))
            .filter_map(|b| {
              let b = b.as_array().filter(|b| b.len() == 4)?;
              Some(StoryBorrow {
                host: str_of(&b[0]),
                slot: b[1].as_i64().unwrap_or(0) as i32,
                level: str_of(&b[2]),
                special: b[3].as_str() == Some("special"),
              })
            })
            .collect(),
        };
        if let (true, Some(t)) = (node.close_task, node.task) {
          d.task_closers[t].push(i);
        }
        d.nodes.push(node);
      }
      d.reset_node = d.node_index.get(&str_of(at(story, "reset_node"))).copied();

      let cond = |e: &Value| read_cond(e, &d.node_index, &task_index);
      let prototypes = items(at(story, "prototypes"))
        .map(|(level, rules)| {
          let rules = list(rules)
            .map(|r| PrototypeRule {
              names: strings(at(r, "names")),
              when: cond(at(r, "when")),
            })
            .collect();
          (level.clone(), rules)
        })
        .collect();
      let types = items(at(story, "types"))
        .map(|(t, when)| (t.clone(), cond(when)))
        .collect();
      let heights = items(at(story, "ocean_heights"))
        .map(|(level, rules)| {
          let rules = list(rules)
            .map(|r| HeightRule {
              when: cond(at(r, "when")),
              height: f32_of(at(r, "height")),
            })
            .collect();
          (level.clone(), rules)
        })
        .collect();
      d.prototypes = prototypes;
      d.types = types;
      d.heights = heights;
    }
    Some(d)
  }

  // ----------------------------------------------------------------------------------------------
  // actors, levels
  // ----------------------------------------------------------------------------------------------

  pub fn role(&self, etype: &str) -> ActorRole {
    if let Some(&r) = self.roles.get(etype) {
      return r;
    }
    if self
      .creature_suffixes
      .iter()
      .any(|s| !s.is_empty() && etype.ends_with(s.as_str()))
    {
      return ActorRole::Creature;
    }
    ActorRole::Object
  }

  /// creatures, logic
  pub fn roles(&self) -> &HashMap<String, ActorRole> {
    &self.roles
  }

  pub fn is_water(&self, etype: &str) -> bool {
    self.water_types.contains(etype)
  }

  /// a particle effect (part-spawner: its art-name names the particle group)
  pub fn is_effect(&self, etype: &str) -> bool {
    self.effects.contains(etype)
  }

  /// logic the catalog does not offer: it does nothing, brings creatures, runs a cutscene
  pub fn is_unlisted(&self, etype: &str) -> bool {
    self.unlisted.contains(etype)
  }

  /// the water types (water-anim and its subtypes), sorted
  pub fn water_types(&self) -> Vec<String> {
    let mut out: Vec<String> = self.water_types.iter().cloned().collect();
    out.sort();
    out
  }

  /// "" when unknown
  pub fn water_model(&self, look: i32) -> &str {
    usize::try_from(look)
      .ok()
      .and_then(|l| self.water_looks.get(l))
      .map_or("", String::as_str)
  }

  pub fn water_look_count(&self) -> usize {
    self.water_looks.len()
  }

  /// the object types (drawn, neither creatures nor logic) and their family: crate, platform...
  pub fn objects(&self) -> &HashMap<String, String> {
    &self.objects
  }

  pub fn object_family(&self, etype: &str) -> String {
    match self.objects.get(etype) {
      Some(f) if !f.is_empty() => f.clone(),
      _ => "other".into(),
    }
  }

  /// what the catalog makes of an object type (tools/object_categories.json): "static" (a prop
  /// that only stands there), "door", "interactive", "collectible", "technical" (shots, vehicles,
  /// controllers...), "" when unknown
  pub fn catalog_category(&self, etype: &str) -> String {
    self.catalog.get(etype).cloned().unwrap_or_default()
  }

  /// the models a type loads (its code), the one it draws first; None when the game data has none
  pub fn type_models(&self, etype: &str) -> Option<&Vec<String>> {
    self.type_models.get(etype)
  }

  pub fn all_type_models(&self) -> &HashMap<String, Vec<String>> {
    &self.type_models
  }

  /// the model of a skeleton group named `name` (without "skel-"), "" when it has the same name:
  /// the actors that choose their model by name (art-name) give the group's
  pub fn skeleton_model(&self, name: &str) -> String {
    self.skeletons.get(name).cloned().unwrap_or_default()
  }

  /// the reverse, "" when none
  pub fn skeleton_of_model(&self, model: &str) -> String {
    self
      .skeletons
      .iter()
      .filter(|(_, m)| m.as_str() == model)
      .map(|(name, _)| name)
      .min()
      .cloned()
      .unwrap_or_default()
  }

  /// the art group a level's DGO needs for a model ("x" for x-ag.go), "" when the common code
  /// (GAME.CGO) has it: the skill's model is in "collectables", always loaded
  pub fn level_art_group(&self, model: &str) -> String {
    let art = self.art_groups.get(model).map_or(model, String::as_str);
    if self.common_art_groups.contains(art) {
      String::new()
    } else {
      art.to_string()
    }
  }

  /// whether the game has an actor type of that name (known by its role, family, models...), not
  /// a placeholder its type table keeps for an older game's entities
  pub fn has_type(&self, etype: &str) -> bool {
    if self.placeholders.contains(etype) {
      return false;
    }
    self.roles.contains_key(etype)
      || self.objects.contains_key(etype)
      || self.type_models.contains_key(etype)
      || self.effects.contains(etype)
      || self.unlisted.contains(etype)
      || self.water_types.contains(etype)
  }

  /// whether `etype` is `base` or derives from it
  pub fn derives_from(&self, etype: &str, base: &str) -> bool {
    let mut t = etype;
    for _ in 0..64 {
      if t.is_empty() {
        break;
      }
      if t == base {
        return true;
      }
      match self.parents.get(t) {
        Some(up) => t = up,
        None => return false,
      }
    }
    false
  }

  /// the res lumps the code of `etype` reads, by tag
  pub fn params(&self, etype: &str) -> Option<&Vec<Param>> {
    self.params.get(etype)
  }

  pub fn enumeration(&self, name: &str) -> Option<&Enum> {
    self.enums.get(name)
  }

  fn needs_then(&self, stem: Option<&String>) -> Vec<String> {
    let Some(stem) = stem else {
      return Vec::new();
    };
    let mut out = self.code_needs.get(stem).cloned().unwrap_or_default();
    out.push(stem.clone());
    out
  }

  /// the code files (.o stems, in their order) a type needs beyond the common code: the file it
  /// is defined in, after the files before it in its level's DGO; empty for the common code
  pub fn code_files(&self, etype: &str) -> Vec<String> {
    self.needs_then(self.code.get(etype))
  }

  /// the same for a particle group a part-spawner draws (its art-name, "group-...")
  pub fn effect_code_files(&self, group: &str) -> Vec<String> {
    self.needs_then(self.effect_code.get(group))
  }

  pub fn level(&self, name: &str) -> Option<&LevelInfo> {
    self.levels.get(name)
  }

  pub fn levels(&self) -> &BTreeMap<String, LevelInfo> {
    &self.levels
  }

  pub fn ocean(&self, name: &str) -> Option<&OceanMap> {
    self.oceans.get(name)
  }

  // ----------------------------------------------------------------------------------------------
  // story
  // ----------------------------------------------------------------------------------------------

  // The story, in steps: at step s the nodes before s are closed (1: the start of the game,
  // step_count(): the end).
  pub fn has_story(&self) -> bool {
    !self.nodes.is_empty()
  }

  pub fn step_count(&self) -> i32 {
    self.nodes.len() as i32
  }

  pub fn node(&self, i: usize) -> &StoryNode {
    &self.nodes[i]
  }

  pub fn moment(&self, step: i32) -> StoryMoment {
    let n = self.nodes.len();
    let step = step.clamp(1, (n as i32).max(1));
    let closed: Vec<bool> = (0..n)
      .map(|i| self.nodes[i].closed || (i as i32) < step)
      .collect();
    let open = (0..n)
      .map(|i| !closed[i] && self.nodes[i].parents.iter().all(|&p| closed[p]))
      .collect();
    let complete = self
      .task_closers
      .iter()
      .map(|closers| closers.iter().any(|&i| closed[i]))
      .collect();
    StoryMoment {
      step,
      closed,
      open,
      complete,
    }
  }

  pub fn eval(&self, c: &StoryCond, m: &StoryMoment) -> bool {
    let get = |v: &Vec<bool>, i: usize| v.get(i).copied().unwrap_or(false);
    match c {
      StoryCond::False => false,
      StoryCond::True => true,
      StoryCond::Closed(i) => get(&m.closed, *i),
      StoryCond::Open(i) => get(&m.open, *i),
      StoryCond::Complete(t) => get(&m.complete, *t),
      StoryCond::Not(a) => !self.eval(a, m),
      StoryCond::And(args) => args.iter().all(|a| self.eval(a, m)),
      StoryCond::Or(args) => args.iter().any(|a| self.eval(a, m)),
    }
  }

  /// The task mask of the actors of `level` at that moment, in `host` (the level opened: the city
  /// traffic and the primary levels change bits).
  pub fn task_mask(&self, level: &str, host: &str, m: &StoryMoment) -> u32 {
    let info = self.level(level);
    let host_info = self.level(host);
    // the story bits: task0..task7, done (level-method-22 / set-proto-vis!)
    let mut mask = info.map_or(0, |i| i.base_mask & 0x1ff);
    if let Some(info) = info {
      for (i, n) in self.nodes.iter().enumerate() {
        if !m.closed.get(i).copied().unwrap_or(false)
          || n.mask_op == MaskOp::NoMask
          || n.level != info.taskname
        {
          continue;
        }
        match n.mask_op {
          MaskOp::Abs => mask = n.mask,
          MaskOp::Set => mask |= n.mask,
          _ => mask &= !n.mask,
        }
      }
    }
    // the settings' bits: "never" always, "ctywide" out of the city (its traffic clears it),
    // "primary0" when no level shown is a primary one
    mask |= self.bits.never;
    if !host_info.is_some_and(|h| h.traffic) {
      mask |= self.bits.ctywide;
    }
    if !host_info.is_some_and(|h| h.base_mask & self.bits.primary != 0) {
      mask |= self.bits.primary;
    }
    mask
  }

  /// Whether an actor with that kill mask exists under that task mask, in a level shown normally
  /// or 'special (only the actors meant for it).
  pub fn actor_alive(&self, kill_mask: u32, task_mask: u32, special: bool) -> bool {
    // the movie bits (16 and up) only matter during cutscenes
    let killed = (kill_mask & 0xffff & task_mask) != 0;
    if special {
      return (kill_mask & self.bits.special) != 0 && !killed;
    }
    !killed
  }

  /// The layers `host` shows at that moment: layer -> shown 'special.
  pub fn borrowed(&self, host: &str, m: &StoryMoment) -> BTreeMap<String, bool> {
    // the level's own, then the reset node's (it empties the slots), then the open nodes'
    let lists = self
      .level(host)
      .map(|i| &i.borrow)
      .into_iter()
      .chain(self.reset_node.map(|r| &self.nodes[r].borrow))
      .chain(
        self
          .nodes
          .iter()
          .enumerate()
          .filter(|(i, _)| m.open.get(*i).copied().unwrap_or(false))
          .map(|(_, n)| &n.borrow),
      );
    let mut slots: BTreeMap<i32, &StoryBorrow> = BTreeMap::new();
    for b in lists.flatten().filter(|b| b.host == host) {
      slots.insert(b.slot, b);
    }
    slots
      .into_values()
      .filter(|b| !b.level.is_empty())
      .map(|b| (b.level.clone(), b.special))
      .collect()
  }

  /// Every layer `host` shows at some point of the story.
  pub fn layers_of(&self, host: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    if let Some(info) = self.level(host) {
      out.extend(
        info
          .borrow
          .iter()
          .filter(|b| !b.level.is_empty())
          .map(|b| b.level.clone()),
      );
    }
    for n in &self.nodes {
      out.extend(
        n.borrow
          .iter()
          .filter(|b| b.host == host && !b.level.is_empty())
          .map(|b| b.level.clone()),
      );
    }
    out.into_iter().collect()
  }

  pub fn prototype_rules(&self, level: &str) -> Option<&Vec<PrototypeRule>> {
    self.prototypes.get(level)
  }

  pub fn type_rule(&self, etype: &str) -> Option<&StoryCond> {
    self.types.get(etype)
  }

  /// The height of the ocean of `level` at that step (meters).
  pub fn ocean_height(&self, level: &str, step: i32) -> f32 {
    let mut height = 0.0;
    if let Some(info) = self.level(level) {
      if let Some(h) = info.ocean_height {
        height = h;
      } else if let Some(map) = self.ocean(&info.ocean) {
        height = map.corner.y;
      }
    }
    // the story sets it (the sewer drained): the rule that held last, at that step or before
    if let Some(rules) = self.heights.get(level)
      && self.has_story()
    {
      for s in (1..=step.clamp(1, self.step_count())).rev() {
        let m = self.moment(s);
        if let Some(r) = rules.iter().find(|r| self.eval(&r.when, &m)) {
          return r.height;
        }
      }
    }
    height
  }
}

// ------------------------------------------------------------------------------------------------
// the rules of the levels opened (src/app/actions.cpp) that only need the game data
// ------------------------------------------------------------------------------------------------

/// Whether the list of levels offers a level of the library (level_listed): not a copy found in
/// another DGO (`level`, the game's name, set and not `name`, its folder's), not a level without
/// collision, not the layer of a mission.
pub fn level_listed(
  name: &str,
  level: &str,
  collision_triangles: usize,
  data: Option<&GameData>,
) -> bool {
  if !level.is_empty() && level != name {
    return false;
  }
  if collision_triangles == 0 {
    return false;
  }
  !data.and_then(|d| d.level(name)).is_some_and(|i| i.layer)
}

/// Whether a level opened keeps an actor of that type (prepare_level_nodes): without its
/// creatures and logic actors, but with its particle effects (steam, sparks, lights: part of its
/// look, exported with it); with `actors` false (the decor alone), only the platforms and bridges
/// that are part of getting around the level (a rope bridge to a hut is part of its shape, not a
/// gameplay item).
pub fn actor_kept(data: Option<&GameData>, etype: &str, actors: bool) -> bool {
  if !actors {
    return data.is_some_and(|d| d.object_family(etype) == "platform");
  }
  data.is_none_or(|d| d.role(etype) == ActorRole::Object || d.is_effect(etype))
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;

  // A game with a story (src/app/selftest.cpp kStoryGameData): the level "town" (a rock the
  // mission breaks into rubble, a crate there until the mission ends, a statue broken by it, a
  // creature, a logic actor, a cutscene character, a water pool), the hut the game shows with it,
  // the layer of the mission's actors, and "port", the level next to it (100 m away along x).
  pub(crate) const K_STORY_GAME_DATA: &str = r#"{
 "format": 1, "game": "jak3",
 "actors": {"roles": {"grunt": "creature", "part-spawner": "logic"}, "creature_suffixes": ["-highres"],
            "effects": ["part-spawner"],
            "water_types": ["water-anim"], "water_looks": ["water-anim-test-pool"],
            "objects": {"crate": "crate", "statue-broken": "breakable", "no-model-thing": "other"}},
 "oceans": {"*ocean-map-test*": {"corner": [-96, -1.5, -96], "cell": 96, "cells": ["11", "01"], "color": [0.1, 0.2, 0.3]}},
 "levels": {
  "town": {"taskname": "town", "base_mask": 1, "ocean": "*ocean-map-test*", "layer": false, "traffic": false, "companions": ["hut"],
           "neighbors": ["hut", "port"],
           "continues": [{"name": "town-start", "position": [1, 0, 2], "shows": ["hut", "port", "town"]}]},
  "hut": {"taskname": "town", "base_mask": 1, "ocean": null, "layer": false, "traffic": false, "companions": [],
          "neighbors": ["port", "town"], "continues": []},
  "port": {"taskname": "port", "base_mask": 1, "ocean": null, "layer": false, "traffic": false, "companions": [],
           "neighbors": ["hut", "town"],
           "continues": [{"name": "port-start", "position": [100, 0, 5], "shows": ["port", "town"]}]},
  "lmission": {"taskname": "lmission", "base_mask": 0, "ocean": null, "layer": true, "traffic": false, "companions": []}
 },
 "story": {
  "reset_node": "start",
  "mask_bits": {"task0": 0, "task1": 1, "done": 8, "special": 12, "primary0": 13, "ctywide": 14, "never": 15},
  "nodes": [
   {"name": "none", "task": "", "level": ""},
   {"name": "start", "task": "intro", "level": "", "closed": true, "borrow": [["town", 0, null, null]]},
   {"name": "town-mission-introduction", "task": "town-mission", "level": "town", "parents": [1],
    "borrow": [["town", 0, "lmission", "display"]]},
   {"name": "town-mission-resolution", "task": "town-mission", "level": "town", "mask_op": "abs", "mask": 2,
    "close_task": true, "parents": [2], "borrow": [["town", 0, "lmission", "display"]]},
   {"name": "end", "task": "end", "level": "", "parents": [3]}
  ],
  "prototypes": {"town": [{"names": ["rock"], "when": ["not", ["complete", "town-mission"]]},
                          {"names": ["rubble"], "when": ["closed", "town-mission-resolution"]}]},
  "types": {"statue-broken": ["closed", "town-mission-resolution"]},
  "ocean_heights": {}
 }
}"#;

  pub(crate) fn story_data() -> GameData {
    GameData::from_json(&serde_json::from_str(K_STORY_GAME_DATA).unwrap()).expect("game data")
  }

  pub(crate) fn real_data() -> GameData {
    GameData::load(&game_data_path()).expect("data/jak3/game-data.json")
  }

  #[test]
  fn a_game_with_a_story() {
    let data = story_data();
    assert!(
      data.role("grunt") == ActorRole::Creature
        && data.role("part-spawner") == ActorRole::Logic
        && data.role("jak-highres") == ActorRole::Creature
        && data.role("crate") == ActorRole::Object
        && data.water_model(0) == "water-anim-test-pool"
        && data.water_model(1).is_empty()
        && data.step_count() == 5,
      "game data: roles, water looks, story nodes"
    );
    // the levels listed: the layer of a mission is not (nor a copy, nor a level without collision)
    let listed: Vec<&str> = ["hut", "lmission", "port", "town"]
      .into_iter()
      .filter(|l| level_listed(l, "", 2, Some(&data)))
      .collect();
    assert_eq!(
      listed,
      ["hut", "port", "town"],
      "levels listed: the layer of a mission is not"
    );
    assert!(!level_listed("town-x", "town", 2, Some(&data)) && !level_listed("town", "", 0, None));
    // a level opened: creatures and cutscene characters left out, its particle effects kept; the
    // decor alone keeps only what one walks on
    assert!(
      !actor_kept(Some(&data), "grunt", true)
        && !actor_kept(Some(&data), "jak-highres", true)
        && actor_kept(Some(&data), "part-spawner", true)
        && actor_kept(Some(&data), "crate", true)
        && actor_kept(Some(&data), "statue-broken", true)
        && actor_kept(None, "grunt", true)
        && !actor_kept(Some(&data), "crate", false),
      "level with game data: creatures and cutscene characters left out, its particle effects kept"
    );
    let (town, port) = (data.level("town").unwrap(), data.level("port").unwrap());
    assert!(
      town.neighbors.iter().any(|n| n == "port")
        && town.companions == ["hut"]
        && port.continues.len() == 1
        && port.continues[0].name == "port-start"
        && crate::math::length(port.continues[0].position - Vec3::new(100.0, 0.0, 5.0)) < 1e-4
        && port.continues[0].shows == ["port", "town"],
      "game data: a level's neighbors and continue points"
    );
    assert_eq!(data.layers_of("town"), ["lmission"]);
    assert!(
      data
        .ocean("*ocean-map-test*")
        .is_some_and(|o| o.cells == [1, 1, 0, 1] && o.size == 2)
        && (data.ocean_height("town", 1) + 1.5).abs() < 1e-5,
      "game data: the ocean of a level, its cells and height"
    );
    // the story: the mission's layer borrowed while it is under way, the crate gone after it
    let (start, end) = (data.moment(1), data.moment(4));
    assert_eq!(
      data.borrowed("town", &start),
      BTreeMap::from([("lmission".to_string(), false)])
    );
    assert!(data.borrowed("town", &end).is_empty());
    let crate_mask = 4294905854.0f64 as i64 as u32; // 0xffff0ffe: while the mask is task0
    assert!(data.actor_alive(crate_mask, data.task_mask("town", "town", &start), false));
    assert!(!data.actor_alive(crate_mask, data.task_mask("town", "town", &end), false));
    let rubble = &data.prototype_rules("town").unwrap()[1].when;
    assert!(!data.eval(rubble, &start) && data.eval(rubble, &end));
    assert!(GameData::from_json(&serde_json::json!({"format": 2})).is_none());
  }

  #[test]
  fn the_game_data_shipped() {
    let data = real_data();
    // the mine's bridge broken by the story
    let bridge = data
      .type_rule("cav-break-bridge")
      .expect("cav-break-bridge");
    let closed_at = (0..data.step_count() as usize)
      .rfind(|&i| data.node(i).name == "mine-boss-introduction")
      .map_or(0, |i| i as i32 + 1);
    let bridge_gone = closed_at > 1
      && data.eval(bridge, &data.moment(closed_at - 1))
      && !data.eval(bridge, &data.moment(closed_at));
    let desert = data.level("desert").expect("desert");
    // the city's ocean: the port's basins (masked for the mid ocean) are the near ocean's water,
    // and by the shores its squares of 3 m
    let city = data.ocean("*ocean-map-city*").expect("the city's ocean");
    let port = 33 * city.size + 10;
    let shore = city.near.values().any(|rows| rows.len() == 32);
    let port_water =
      city.cells[port] == 0 && city.near.get(&port).is_some_and(|r| r.is_empty()) && shore;
    assert!(
      data.role("mech") == ActorRole::Creature
        && data.role("marauder") == ActorRole::Creature
        && data.role("part-spawner") == ActorRole::Logic
        && data.role("crate") == ActorRole::Object
        && data.is_water("water-anim")
        && bridge_gone
        && !desert.ocean.is_empty()
        && desert.companions.iter().filter(|c| *c == "deserta").count() == 1
        && port_water,
      "game data: roles, water, the mine's bridge broken by the story, the desert's ocean and \
       companions, the near ocean of the city's port"
    );
    // the catalog: props and doors, the interactive objects apart, no collectible; the living ones
    // the ancestry misses are creatures (left out of the levels too)
    assert!(
      data.catalog_category("wascity-windmill") == "static"
        && data.catalog_category("vin-door-ctyinda") == "door"
        && data.catalog_category("crate") == "interactive"
        && data.catalog_category("skill") == "collectible"
        && data.role("tizard") == ActorRole::Creature
        && data.role("pecker-ingame") == ActorRole::Creature
        && data.role("citizen-norm-rider") == ActorRole::Creature,
      "game data: the catalog's categories (props, doors, interactive, collectibles), the animals \
       and riders creatures"
    );
    // a level's DGO takes the art group holding a model, none from the common code
    assert!(
      data.level_art_group("ctycrate-blue") == "ctycrate"
        && data.level_art_group("collectables-skill").is_empty()
        && data.level_art_group("vin-door-ctyinda") == "vin-door-ctyinda",
      "game data: the art groups a level's DGO needs (the skill's is common)"
    );
    assert!(
      data.has_type("crate") && data.has_type("cty-door") && !data.has_type("cty-fruit-stand"),
      "export: the game's actor types (a placeholder is not one)"
    );
    // what configures the objects: the lumps their code reads, what they are, where their code is
    let vent = data
      .params("industrial-board-vent")
      .expect("the vent's params");
    let height = vent
      .iter()
      .any(|p| p.tag == "spring-height" && p.kind == "float" && p.def.as_f64() == Some(45056.0));
    let door = data.params("cty-door").expect("the door's params");
    let notice = door
      .iter()
      .any(|p| p.tag == "on-notice" && p.kind == "script");
    let options = door
      .iter()
      .any(|p| p.tag == "options" && p.kind == "enum:airlock-options");
    assert!(
      height
        && notice
        && options
        && data
          .enumeration("airlock-options")
          .is_some_and(|e| e.bitfield)
        && data.derives_from("industrial-board-vent", "bouncer")
        && data.derives_from("cty-door", "com-airlock")
        && !data.derives_from("crate", "com-airlock")
        && data.code_files("industrial-board-vent") == ["elec-gate", "ctyinda-part", "ctyinda-obs"]
        && data.code_files("cty-door").is_empty()
        && data
          .effect_code_files("group-ctyindb-red-floor-vent")
          .last()
          .is_some_and(|f| f == "ctyindb-part"),
      "game data: the parameters of a type (lumps its code reads), what it derives from, the code \
       it needs"
    );
  }
}
