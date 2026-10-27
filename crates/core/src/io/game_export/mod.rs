//! A document exported as a custom level of OpenGOAL for Jak 3: the files its level builder
//! (goalc/build_level) takes, laid out in <folder> (the project's export: <library>/exports/<project>/)
//! as in the jak-project checkout they are copied into:
//!
//! ```text
//!   README.md                                   how to bring it into the game (the folders to copy,
//!                                               the lines to add to game.gp and level-info.gc, how to
//!                                               build it and go there) and what the export changed
//!   custom_assets/jak3/levels/<name>/           the level:
//!   goal_src/jak3/levels/<name>/<name>-obs.gc   the behaviors set in the editor (when there are some)
//! ```
//!
//! The level's files:
//!
//! ```text
//!   <name>.glb     what the view shows of the decor, in the game's space; the collision of the
//!                  game where the levels have it (exact: one node per surface), the rest made by
//!                  the builder from the decor (automatic walls). Where a piece of a level's decor
//!                  was deleted, moved or hidden, the game's collision there is left out (the level
//!                  as the game has it, `originals`, tells which pieces are still in place)
//!   <name>.jsonc   the level: its names, the actors (their data as the builder writes res lumps),
//!                  the art groups of their models
//!   <name>.gd      the DGO: the art groups and the level
//!   <name>-nav.json  the nav meshes of the document (nav_data)
//!   level-info.gc  its definition in the game's code (or the line to change, for a level replacing
//!                  one of the game's)
//! ```
//!
//! The collision surfaces are written as the builder takes them (Jak 1's fields, which it converts:
//! the materials and events it lacks become the nearest ones); the kill masks are dropped (the new
//! level has its own story). What the builder cannot write is left out, and said.
//!
//! Its parts: builder.rs (the builder's formats), collision.rs, write.rs (the export),
//! readme.rs.

mod builder;
mod collision;
mod readme;
#[cfg(test)]
mod tests;
mod write;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::i18n::{tr, tr_en};
use crate::io::behaviors::{
  Condition, ConditionKind, behavior_code, behavior_type_name, condition_of,
};
use crate::io::decor_behaviors::{bounce_code, collision_extras, decor_behavior_of, launch_code};
use crate::io::game_data::{GameData, LevelInfo};
use crate::io::gltf::{ExportMode, ExportOptions, ImportOptions, export_glb, import_gltf};
use crate::io::nav_export::{NavExportEntry, write_navmesh_blob};
use crate::math::{
  AABB, Mat4, Quat, Ray, Transform, Vec3, cross, dot, length, length2, normalize, radians,
  ray_aabb, transform_ray,
};
use crate::navmesh::{NavMeshData, navmesh_ignores};
use crate::prefs::Prefs;
use crate::scene::pick::{PickFilter, pick};
use crate::scene::{Mesh, Node, NodeKind, Primitive, Scene};
use crate::strf;

use builder::*;
pub use builder::{builder_lump_value, builder_surface};
use collision::*;
pub(crate) use collision::{TriGrid, collision_group_level, piece_level};
pub use collision::{collision_levels, default_decor_collision, own_level_node};
use readme::*;
pub use write::export_level;

/// the actors' ids by default: above the ids of the game's own actors (72,851 at most)
pub const K_DEFAULT_BASE_ID: u32 = 90000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelExportOptions {
  /// long_name: a symbol of the game ("my-level")
  pub name: String,
  /// 3 letters: the DGO's name
  pub nickname: String,
  /// at most 8 letters and digits, upper case
  pub iso_name: String,
  /// the actors' ids from it (0: K_DEFAULT_BASE_ID)
  pub base_id: u32,
  /// the export's folder: README.md, custom_assets/..., goal_src/...
  pub folder: String,
  pub actors: bool,
  /// the levels' own collision where they have it (else from the decor)
  pub game_collision: bool,
  /// where the player starts (the continue point), meters
  pub start: Vec3,
  /// a level of the game the level replaces ("ctyindb"), "" for a level of its own. The game then
  /// loads it instead of that level (its definition's nickname names this DGO): with that level's
  /// continue points, music and missions; only the document's own level is written (not the
  /// levels added next to it), and its DGO has that level's code
  pub replaces: String,
  /// the levels of the game as extracted, by name: the pieces of their decor that the document
  /// deleted, moved or hid lose the game's collision there (game_collision); a level without one
  /// keeps all of its collision. `collision_levels` names them, `read_original` reads one.
  #[serde(skip)]
  pub originals: BTreeMap<String, Arc<Scene>>,
  /// the regions of the level replaced (its level.json, region_trees: LevelEntry::region_trees):
  /// the game loads and shows the levels around it by them
  #[serde(skip)]
  pub region_trees: Value,
  /// the document's nav meshes, in the game's space (origin: the center of their corners):
  /// navmesh::world_navmeshes
  #[serde(skip)]
  pub navmeshes: Vec<NavMeshData>,
  #[serde(skip)]
  pub navmesh_names: Vec<String>,
}

impl Default for LevelExportOptions {
  fn default() -> Self {
    Self {
      name: String::new(),
      nickname: String::new(),
      iso_name: String::new(),
      base_id: 0,
      folder: String::new(),
      actors: true,
      game_collision: true,
      start: Vec3::new(0.0, 0.0, 0.0),
      replaces: String::new(),
      originals: BTreeMap::new(),
      region_trees: Value::Null,
      navmeshes: Vec::new(),
      navmesh_names: Vec::new(),
    }
  }
}

/// What the export changed or left out: a text key (lang/*.json, "export.note.*") with a count
/// and details (names of types, lumps...).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ExportNote {
  pub key: String,
  pub count: usize,
  pub detail: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LevelExportReport {
  /// the folder written (the export's: README.md at its top)
  pub dir: String,
  /// the level's files: <dir>/custom_assets/jak3/levels/<name>
  pub level_dir: String,
  pub files: Vec<String>,
  pub meshes: usize,
  pub triangles: usize,
  /// the game's, exact or approximated
  pub collision_triangles: usize,
  /// the meshes whose collision the builder makes
  pub collision_from_decor: usize,
  pub actors: usize,
  pub art_groups: usize,
  pub navmeshes: usize,
  /// actors whose behavior is the level's code
  pub behaviors: usize,
  /// the places of its floors where Jak would meet more collision than the game gathers at once
  /// (461 triangles): he can fall through there
  pub dense_places: usize,
  /// the 100 densest of them: triangles, where
  pub densest: Vec<(usize, Vec3)>,
  /// of the level replaced (region_trees)
  pub regions: usize,
  /// that code, <name>-obs.gc ("" when none)
  pub code_file: String,
  pub notes: Vec<ExportNote>,
}

/// Where the player would start in a level, for each continue point of the game's level `level`:
/// the height of the ground under it in the document (NaN when there is none within a few meters).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContinueCheck {
  pub name: String,
  pub position: Vec3,
  /// meters, NaN: nothing under it
  pub ground: f32,
}

impl ContinueCheck {
  pub fn ok(&self) -> bool {
    !self.ground.is_nan()
  }
}

/// The collision an element of the decor has in the game when the editor sets none (the export's
/// rule): the game's own (a piece of a level whose collision the document has, where it is still in
/// place), none (a surface seen through: water, glass; a piece under 4 m: a pipe, a lamp, a decal)
/// or one made from its shape by the level builder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultCollision {
  Game,
  SeeThrough,
  Small,
  Shape,
}

/// A collision surface (the pattern of a triangle, in the game's layout) as the level builder takes
/// it: Jak 1's fields (the builder converts them). Ordered by its fields, in this order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct BuilderSurface {
  pub material: i32,
  pub event: i32,
  pub mode: i32,
  pub noentity: bool,
  pub nocamera: bool,
  pub noedge: bool,
  pub nolineofsight: bool,
}

fn from_path(p: &Path) -> String {
  p.to_string_lossy().replace('\\', "/")
}

fn write_text(p: &Path, text: &str) -> Result<(), String> {
  std::fs::write(p, text).map_err(|_| format!("cannot write {}", from_path(p)))
}

/// numbers as the game's files have them: 4 decimals at most, no exponent
fn num(v: f64) -> String {
  let v = if v.abs() < 5e-5 { 0.0 } else { v };
  let mut s = format!("{v:.4}");
  while s.len() > 1 && s.ends_with('0') {
    s.pop();
  }
  if s.ends_with('.') {
    s.push('0');
  }
  s
}

/// what the export changed, by note then detail
#[derive(Default)]
struct Notes {
  by_key: BTreeMap<String, BTreeMap<String, usize>>,
  counts: BTreeMap<String, usize>,
}

impl Notes {
  fn add(&mut self, key: &str, detail: &str, n: usize) {
    *self.counts.entry(key.to_string()).or_default() += n;
    if !detail.is_empty() {
      *self
        .by_key
        .entry(key.to_string())
        .or_default()
        .entry(detail.to_string())
        .or_default() += n;
    }
  }

  fn list(&self) -> Vec<ExportNote> {
    let mut out = Vec::new();
    for (key, &count) in &self.counts {
      let mut note = ExportNote {
        key: key.clone(),
        count,
        detail: String::new(),
      };
      if let Some(details) = self.by_key.get(key) {
        let mut items: Vec<(&String, usize)> = details.iter().map(|(d, &n)| (d, n)).collect();
        items.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        for (i, (detail, n)) in items.iter().take(12).enumerate() {
          if i > 0 {
            note.detail += ", ";
          }
          note.detail += detail;
          if *n > 1 {
            note.detail += &format!(" ({n})");
          }
        }
        if items.len() > 12 {
          note.detail += ", ...";
        }
      }
      out.push(note);
    }
    out
  }
}

/// A nickname (3 letters: "m" and the name's first letters after "my-") made from a level name.
pub fn default_nickname(name: &str) -> String {
  // "m" (mine: apart from the game's levels) and the first letters of the name, after "my-"
  let base = name.strip_prefix("my-").unwrap_or(name);
  let mut out = String::from("m");
  for c in base.bytes() {
    if c.is_ascii_alphanumeric() && out.len() < 3 {
      out.push(c.to_ascii_lowercase() as char);
    }
  }
  while out.len() < 3 {
    out.push('x');
  }
  out
}

/// An ISO name (8 letters and digits, upper case) made from a level name.
pub fn default_iso_name(name: &str) -> String {
  let out: String = name
    .bytes()
    .filter(u8::is_ascii_alphanumeric)
    .take(8)
    .map(|c| c.to_ascii_uppercase() as char)
    .collect();
  if out.is_empty() { "CUSTOM".into() } else { out }
}

/// None when the names and the folder suit the builder and the game, else the text key of what is
/// wrong ("export.bad.name"...). With the game's data: not the name or the nickname of one of its
/// levels (its DGO would be written over the game's), and a level to replace that the game has.
pub fn check_export_names(o: &LevelExportOptions, data: Option<&GameData>) -> Option<&'static str> {
  let symbol_ok = |s: &str| {
    s.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
      && s
        .bytes()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
      && !s.ends_with('-')
  };
  if !symbol_ok(&o.name) || o.name.len() < 3 || o.name.len() > 10 {
    return Some("export.bad.name");
  }
  if o.nickname.len() != 3 || !symbol_ok(&o.nickname) {
    return Some("export.bad.nickname");
  }
  if o.iso_name.is_empty()
    || o.iso_name.len() > 8
    || !o
      .iso_name
      .bytes()
      .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
  {
    return Some("export.bad.iso_name");
  }
  if o.folder.is_empty() {
    return Some("export.bad.folder");
  }
  if let Some(data) = data {
    // the game's own levels (the ones whose DGO the game builds): their files would be written over
    for (name, info) in data.levels() {
      if info.dgo.is_empty() {
        continue;
      }
      if *name == o.name {
        return Some("export.bad.name_taken");
      }
      if info.nickname == o.nickname {
        return Some("export.bad.nickname_taken");
      }
    }
    if !o.replaces.is_empty()
      && data
        .level(&o.replaces)
        .is_none_or(|info| info.dgo.is_empty() || info.layer)
    {
      return Some("export.bad.replaces");
    }
  }
  None
}

/// Where the player would start, for each continue point of the game's level `level`: the ground
/// under it in the document's own level (not the levels added next to it nor a reference).
pub fn check_continues(scene: &Scene, data: Option<&GameData>, level: &str) -> Vec<ContinueCheck> {
  let Some(info) = data.and_then(|d| d.level(level)) else {
    return Vec::new();
  };
  let cache = scene.cache();
  let count = scene.nodes.len().min(cache.count);
  // the document's own decor shown
  let ground: Vec<usize> = (0..count)
    .filter(|&i| {
      let n = &scene.nodes[i];
      n.kind == NodeKind::Render && n.mesh.is_some() && cache.visible[i] && own_level_node(scene, i)
    })
    .collect();
  let mut out = Vec::new();
  for cp in &info.continues {
    // from a little above the point, down to a few meters below it
    let ray = Ray {
      o: cp.position + Vec3::new(0.0, 2.0, 0.0),
      d: Vec3::new(0.0, -1.0, 0.0),
    };
    let mut best = 8.0f32;
    for &i in &ground {
      if ray_aabb(&ray, &cache.bounds[i].expanded(0.01), best).is_none() {
        continue;
      }
      let mesh = scene.nodes[i].mesh.as_ref().unwrap();
      let local = transform_ray(&cache.worlds[i].inverse(), &ray);
      for (p, prim) in mesh.prims.iter().enumerate() {
        if let Some((t, _)) = mesh.bvh(p).raycast(&local, &prim.pos, &prim.idx, best) {
          best = t;
        }
      }
    }
    out.push(ContinueCheck {
      name: cp.name.clone(),
      position: cp.position,
      ground: if best < 8.0 { ray.at(best).y } else { f32::NAN },
    });
  }
  out
}

/// Where the player starts by default (App::export_start_point): the first continue point of the
/// level opened (where its shell is now) inside the document, else the ground at its middle. Makes
/// the scene's cache again (the world matrices after the last changes).
pub fn export_start_point(scene: &mut Scene, data: Option<&GameData>) -> Vec3 {
  // the level opened where it is now: the group of its shell (moved with it)
  let prefix = format!("{}-", scene.settings.level);
  let placed = scene
    .nodes
    .iter()
    .find(|n| n.parent == 0 && n.kind == NodeKind::Group && n.name.starts_with(&prefix))
    .map_or(Mat4::identity(), |n| scene.world(n));
  scene.begin_frame();
  let b = scene.focus_bounds();
  if let Some(info) = data.and_then(|d| d.level(&scene.settings.level)) {
    for c in &info.continues {
      let p = placed.point(c.position);
      if !b.valid() || b.expanded(50.0).contains(p) {
        return p;
      }
    }
  }
  if !b.valid() {
    return Vec3::new(0.0, 2.0, 0.0);
  }
  // else the ground at the middle of the document
  let top = Vec3::new(b.center().x, b.hi.y + 10.0, b.center().z);
  let f = PickFilter {
    actors: false,
    include_locked: true,
    ..Default::default()
  };
  let hit = pick(
    scene,
    &Ray {
      o: top,
      d: Vec3::new(0.0, -1.0, 0.0),
    },
    &f,
  );
  if hit.hit() {
    hit.point + Vec3::new(0.0, 1.0, 0.0)
  } else {
    Vec3::new(top.x, b.lo.y + 2.0, top.z)
  }
}

/// `nickname`, or one of "m??" when a level of the game has it (a DGO named like one of the game's
/// would be built over the game's own).
pub fn untaken_nickname(nickname: &str, data: Option<&GameData>) -> String {
  let taken = |nick: &str| {
    data.is_some_and(|d| {
      d.levels()
        .values()
        .any(|info| !info.dgo.is_empty() && info.nickname == nick)
    })
  };
  let digits = b"abcdefghijklmnopqrstuvwxyz0123456789";
  let mut out = nickname.to_string();
  let mut k = 0;
  while taken(&out) && k < 36 * 36 {
    out = format!(
      "m{}{}",
      digits[(k / 36) % 36] as char,
      digits[k % 36] as char
    );
    k += 1;
  }
  out
}

/// The options of a first export of the document (App::default_export_options): a name the game
/// does not have ("my-" and the level's), the folder <library_root>/exports/<project_folder>
/// (`library_root` "": the editor's preferences folder; `project_folder` "": the name), the start
/// point. `project_folder`: the project's name as a folder can have it (App::project_folder_name of
/// App::project_name).
pub fn default_export_options(
  scene: &mut Scene,
  data: Option<&GameData>,
  library_root: &str,
  project_folder: &str,
) -> LevelExportOptions {
  let mut o = LevelExportOptions {
    replaces: scene.settings.replaces.clone(),
    ..Default::default()
  };
  let mut base = if o.replaces.is_empty() {
    scene.settings.level.as_str()
  } else {
    o.replaces.as_str()
  };
  if base.is_empty() || base == "custom-level" {
    base = "level";
  }
  // a name the game does not have: "my-" and the level's, 10 characters at most
  let mut name = String::from("my-");
  for c in base.bytes() {
    if c.is_ascii_alphanumeric() || c == b'-' {
      name.push(c.to_ascii_lowercase() as char);
    }
  }
  name.truncate(10);
  while name.ends_with('-') {
    name.pop();
  }
  o.nickname = untaken_nickname(&default_nickname(&name), data);
  o.iso_name = default_iso_name(&name);
  o.base_id = K_DEFAULT_BASE_ID;
  // the project's export, next to the extracted game (not in the editor's sources: the level holds
  // its textures): <library>/exports/<project>/
  let root = if library_root.is_empty() {
    Prefs::dir()
  } else {
    library_root.to_string()
  };
  let project = if project_folder.is_empty() {
    name.as_str()
  } else {
    project_folder
  };
  o.folder = from_path(&Path::new(&root).join("exports").join(project));
  o.name = name;
  o.start = export_start_point(scene, data);
  o
}

/// A level as extracted (its LevelEntry::background), for LevelExportOptions::originals. A level
/// of Jak II is then made decor of Jak 3 as the document has it (make_jak2_level) by the caller.
pub fn read_original(background: &Path) -> Result<Scene, String> {
  let mut scene = Scene::new();
  import_gltf(&mut scene, background, &ImportOptions::default())?;
  scene.begin_frame();
  Ok(scene)
}
