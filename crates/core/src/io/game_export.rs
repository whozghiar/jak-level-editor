//! A document exported as a custom level of OpenGOAL for Jak 3: the files its level builder
//! (goalc/build_level) takes, laid out in <folder> (the project's export: <library>/export/<project>/)
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

// ------------------------------------------------------------------------------------------------

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

// --- collision surfaces --------------------------------------------------------------------------

const K_MATERIALS: [&str; 34] = [
  "none",
  "ice",
  "quicksand",
  "waterbottom",
  "tar",
  "sand",
  "wood",
  "grass",
  "pcmetal",
  "snow",
  "deepsnow",
  "hotcoals",
  "lava",
  "crwood",
  "gravel",
  "dirt",
  "metal",
  "straw",
  "tube",
  "swamp",
  "stopproj",
  "rotate",
  "neutral",
  "stone",
  "crmetal",
  "carpet",
  "grmetal",
  "shmetal",
  "hdwood",
  "squish",
  "mhshroom",
  "forest",
  "mhswamp",
  "dmaker",
];
const K_EVENTS: [&str; 20] = [
  "none",
  "deadly",
  "endlessfall",
  "burn",
  "deadlyup",
  "burnup",
  "melt",
  "slide",
  "lip",
  "lipramp",
  "shock",
  "shockup",
  "hide",
  "rail",
  "slippery",
  "drag",
  "waterfloor",
  "hang",
  "fry",
  "slime",
];

fn material_name(m: usize) -> String {
  K_MATERIALS
    .get(m)
    .map_or_else(|| format!("material {m}"), |s| s.to_string())
}

fn event_name(e: usize) -> String {
  K_EVENTS
    .get(e)
    .map_or_else(|| format!("event {e}"), |s| s.to_string())
}

// --- actors --------------------------------------------------------------------------------------

fn strip_quote(s: &str) -> &str {
  s.strip_prefix('\'').unwrap_or(s)
}

fn is_integer(v: &Value) -> bool {
  v.is_i64() || v.is_u64()
}

/// the numbers of a lump value, flattened
fn numbers<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
  match v {
    Value::Number(_) => out.push(v),
    Value::Array(a) => a.iter().for_each(|x| numbers(x, out)),
    _ => {}
  }
}

/// a guess of the type of an untyped lump value (projects of older extractions)
fn guess_type(v: &Value) -> &'static str {
  match v {
    Value::String(s) => {
      if strip_quote(s) != s {
        "symbol"
      } else {
        "string"
      }
    }
    Value::Number(_) if is_integer(v) => "int32",
    Value::Number(_) => "float",
    Value::Array(a) if !a.is_empty() => {
      if a[0].is_string() {
        guess_type(&a[0])
      } else if a[0].is_array() {
        "vector"
      } else if a.iter().all(is_integer) {
        "int32"
      } else {
        "float"
      }
    }
    _ => "",
  }
}

// --- regions -------------------------------------------------------------------------------------

/// A script of a region as the builder writes it: its integers as floats (the builder would make
/// them a basic, where the game has a fixnum; the commands read a float as well)
fn builder_script(script: &str) -> String {
  let s = script.as_bytes();
  let mut out = Vec::with_capacity(s.len() + 16);
  let mut in_string = false;
  let mut i = 0;
  while i < s.len() {
    let c = s[i];
    if c == b'"' && (i == 0 || s[i - 1] != b'\\') {
      in_string = !in_string;
    }
    let start = !in_string
      && (c.is_ascii_digit() || (c == b'-' && i + 1 < s.len() && s[i + 1].is_ascii_digit()))
      && (i == 0 || s[i - 1] == b' ' || s[i - 1] == b'(');
    if !start {
      out.push(c);
      i += 1;
      continue;
    }
    let mut end = i + 1;
    while end < s.len() && s[end] != b' ' && s[end] != b')' {
      end += 1;
    }
    let token = &s[i..end];
    out.extend_from_slice(token);
    if token.iter().all(|b| b"-0123456789".contains(b)) {
      out.extend_from_slice(b".0");
    }
    i = end;
  }
  String::from_utf8_lossy(&out).into_owned()
}

fn region_id(r: &Value) -> u32 {
  r.get("id").and_then(Value::as_u64).unwrap_or(0) as u32
}

/// The regions for the builder: one prim per id (a region of the game with several faces gets a new
/// id for each other face, from `next_id`), the scripts as it writes them
fn builder_regions(trees: &Value, mut next_id: u32, count: &mut usize) -> Value {
  let mut out = Map::new();
  let Some(trees) = trees.as_object() else {
    return Value::Object(out);
  };
  let regions_of = |tree: &Value| -> Vec<Value> {
    tree
      .get("regions")
      .and_then(Value::as_array)
      .cloned()
      .unwrap_or_default()
  };
  let mut taken = BTreeSet::new();
  for tree in trees.values() {
    taken.extend(regions_of(tree).iter().map(region_id));
  }
  let mut seen = BTreeSet::new();
  for (name, tree) in trees {
    if !tree.is_object() || tree.get("bsphere").is_none() || tree.get("regions").is_none() {
      continue;
    }
    let mut regions = Vec::new();
    for mut r in regions_of(tree) {
      let mut id = region_id(&r);
      if !seen.insert(id) {
        while taken.contains(&next_id) {
          next_id += 1;
        }
        id = next_id;
        next_id += 1;
        taken.insert(id);
        seen.insert(id);
      }
      if let Some(o) = r.as_object_mut() {
        o.insert("id".into(), id.into());
        for k in ["on-enter", "on-inside", "on-exit"] {
          if let Some(Value::String(s)) = o.get(k) {
            let script = builder_script(s);
            o.insert(k.into(), script.into());
          }
        }
      }
      regions.push(r);
    }
    *count += regions.len();
    out.insert(
      name.clone(),
      json!({"bsphere": tree["bsphere"], "regions": regions}),
    );
  }
  Value::Object(out)
}

// --- the level's definition in the game's code ---------------------------------------------------

fn meters(v: f32) -> String {
  format!("(meters {})", num(v as f64))
}

fn vectorm(v: Vec3) -> String {
  format!(
    "{} {} {}",
    num(v.x as f64),
    num(v.y as f64),
    num(v.z as f64)
  )
}

fn level_info(o: &LevelExportOptions, bottom: f32) -> String {
  let (s, c) = (o.start, o.start + Vec3::new(0.0, 3.0, -6.0));
  let (name, nick) = (&o.name, &o.nickname);
  format!(
    "(define {name}
  (new 'static 'level-load-info
    :name '{name}
    :visname '{name}-vis
    :nickname '{nick}
    :dbname '{name}
    :taskname 'default
    :index #x128 ;; the test-zone's: give it a number no other level has
    :master-level #f
    :packages '()
    :run-packages '(\"common\")
    :memory-mode (level-memory-mode large)
    :music-bank #f
    :extra-sound-bank #f
    :mood-func 'update-mood-default
    :special-mood #f
    :ocean #f
    :ocean-alpha 1.0
    :priority 100
    :draw-priority 10.0
    :base-task-mask (task-mask task0)
    :bigmap-id (bigmap-id none)
    :continues '((new 'static 'continue-point
        :name \"{name}-start\"
        :level '{name}
        :trans (static-vectorm {})
        :camera-trans (static-vectorm {})
        :quat (new 'static 'vector4h :data (new 'static 'array int16 4 0 0 0 #x7fff))
        :camera-rot (new 'static 'array int16 9 #x7fff 0 0 0 #x7fff 0 0 0 #x7fff)
        :on-goto #f
        :vis-nick '{nick}
        :vehicle-type #x1b
        :want-count 1
        :want (new 'static 'inline-array level-buffer-state-small 1
          (new 'static 'level-buffer-state-small :name '{name} :display? 'display)
          )
        :want-sound (new 'static 'array symbol 3 #f #f #f)
        )
      )
    :callback-list '()
    :borrow #f
    :bottom-height {}
    :fog-height (meters 80)
    :max-rain 1.0
    :fog-mult 1.0
    :mood-range (new 'static 'mood-range :max-cloud 1.0 :min-fog 0.5 :max-fog 1.0)
    )
  )

(#when PC_PORT
  (cons! *level-load-list* '{name})
  )
",
    vectorm(s),
    vectorm(c),
    meters(bottom)
  )
}

/// a surface nothing stands on: glows and light shafts, water and glass drawn translucent (the
/// levels' own materials are blended with their texture's alpha almost everywhere: that alone does
/// not make them see-through), the same rule as the nav meshes'
fn see_through(scene: &Scene, mesh: &Mesh) -> bool {
  let mut any = false;
  for p in &mesh.prims {
    if p.idx.is_empty() {
      continue;
    }
    match usize::try_from(p.material)
      .ok()
      .and_then(|m| scene.materials.get(m))
    {
      Some(m) if navmesh_ignores(m) => any = true,
      _ => return false,
    }
  }
  any
}

/// the triangles of a primitive (those whose corners it has)
fn triangles(p: &Primitive) -> impl Iterator<Item = [Vec3; 3]> + '_ {
  p.idx.as_chunks::<3>().0.iter().filter_map(|t| {
    Some([
      *p.pos.get(t[0] as usize)?,
      *p.pos.get(t[1] as usize)?,
      *p.pos.get(t[2] as usize)?,
    ])
  })
}

/// The point of triangle abc nearest to p (Ericson, Real-Time Collision Detection, 5.1.5)
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
  let (ab, ac, ap) = (b - a, c - a, p - a);
  let (d1, d2) = (dot(ab, ap), dot(ac, ap));
  if d1 <= 0.0 && d2 <= 0.0 {
    return a;
  }
  let bp = p - b;
  let (d3, d4) = (dot(ab, bp), dot(ac, bp));
  if d3 >= 0.0 && d4 <= d3 {
    return b;
  }
  let vc = d1 * d4 - d3 * d2;
  if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
    return a + ab * (d1 / (d1 - d3));
  }
  let cp = p - c;
  let (d5, d6) = (dot(ab, cp), dot(ac, cp));
  if d6 >= 0.0 && d5 <= d6 {
    return c;
  }
  let vb = d5 * d2 - d1 * d6;
  if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
    return a + ac * (d2 / (d2 - d6));
  }
  let va = d3 * d6 - d5 * d4;
  if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
    return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
  }
  let k = 1.0 / (va + vb + vc);
  a + ab * (vb * k) + ac * (vc * k)
}

/// Triangles of the game's space hashed on a grid of `cell` meters: the distance from a point to the
/// nearest, how many meet a box.
struct TriGrid {
  cell: f32,
  tris: Vec<[Vec3; 3]>,
  boxes: Vec<AABB>,
  cells: HashMap<u64, Vec<u32>>,
  /// the last query that met each triangle
  seen: RefCell<Vec<u32>>,
  query: Cell<u32>,
}

impl TriGrid {
  fn new(cell: f32) -> Self {
    Self {
      cell,
      tris: Vec::new(),
      boxes: Vec::new(),
      cells: HashMap::new(),
      seen: RefCell::new(Vec::new()),
      query: Cell::new(0),
    }
  }

  fn key(x: i32, y: i32, z: i32) -> u64 {
    ((x.wrapping_add(1 << 20) as u32 as u64) << 42)
      | ((y.wrapping_add(1 << 20) as u32 as u64) << 21)
      | (z.wrapping_add(1 << 20) as u32 as u64)
  }

  fn at(&self, v: f32) -> i32 {
    (v / self.cell).floor() as i32
  }

  fn is_empty(&self) -> bool {
    self.tris.is_empty()
  }

  /// the cells a box covers
  fn cells_of(&self, b: &AABB) -> impl Iterator<Item = u64> + '_ {
    let (lo, hi) = (b.lo, b.hi);
    (self.at(lo.x)..=self.at(hi.x)).flat_map(move |x| {
      (self.at(lo.y)..=self.at(hi.y))
        .flat_map(move |y| (self.at(lo.z)..=self.at(hi.z)).map(move |z| Self::key(x, y, z)))
    })
  }

  fn add(&mut self, a: Vec3, b: Vec3, c: Vec3) {
    let mut bx = AABB::default();
    bx.add(a);
    bx.add(b);
    bx.add(c);
    let t = self.tris.len() as u32;
    self.tris.push([a, b, c]);
    self.boxes.push(bx);
    let side = |lo: f32, hi: f32| self.at(hi) as i64 - self.at(lo) as i64 + 1;
    let span = side(bx.lo.x, bx.hi.x) * side(bx.lo.y, bx.hi.y) * side(bx.lo.z, bx.hi.z);
    if span <= 64 {
      let keys: Vec<u64> = self.cells_of(&bx).collect();
      for k in keys {
        self.cells.entry(k).or_default().push(t);
      }
      return;
    }
    // a large one (a level's ground, a far mountain): the cells its surface crosses, not all the
    // cells of its box (a slanted triangle of a kilometer would fill millions)
    let longest = length(b - a).max(length(c - b)).max(length(a - c));
    let n = ((longest / (self.cell * 0.5)).ceil() as i32).clamp(1, 2048);
    let mut crossed = HashSet::new();
    for i in 0..=n {
      for j in 0..=(n - i) {
        let p = a + (b - a) * (i as f32 / n as f32) + (c - a) * (j as f32 / n as f32);
        crossed.insert(Self::key(self.at(p.x), self.at(p.y), self.at(p.z)));
      }
    }
    for k in crossed {
      self.cells.entry(k).or_default().push(t);
    }
  }

  /// f(t) once for each triangle in the cells the box covers
  fn each_near(&self, bx: &AABB, mut f: impl FnMut(u32)) {
    let mut seen = self.seen.borrow_mut();
    if seen.len() < self.tris.len() {
      seen.resize(self.tris.len(), 0);
    }
    let query = self.query.get() + 1;
    self.query.set(query);
    for k in self.cells_of(bx) {
      let Some(ts) = self.cells.get(&k) else {
        continue;
      };
      for &t in ts {
        if seen[t as usize] != query {
          seen[t as usize] = query;
          f(t);
        }
      }
    }
  }

  /// the distance to the nearest triangle, `limit` when none is nearer
  fn distance(&self, p: Vec3, limit: f32) -> f32 {
    let mut best = limit;
    let mut bx = AABB::default();
    bx.add(p - Vec3::new(limit, limit, limit));
    bx.add(p + Vec3::new(limit, limit, limit));
    self.each_near(&bx, |t| {
      let [a, b, c] = self.tris[t as usize];
      best = best.min(length(closest_on_triangle(p, a, b, c) - p));
    });
    best
  }

  fn count(&self, bx: &AABB) -> usize {
    let mut n = 0;
    self.each_near(bx, |t| n += self.boxes[t as usize].overlaps(bx) as usize);
    n
  }

  /// at least count(box): the triangles of the cells it covers, a triangle over several cells
  /// counted in each (cheap)
  fn bound(&self, bx: &AABB) -> usize {
    self
      .cells_of(bx)
      .filter_map(|k| self.cells.get(&k))
      .map(Vec::len)
      .sum()
  }
}

/// The level whose decor a node is part of: <level> of its outermost ancestor <level>-tfrag-*,
/// -tie-* or -shrub-* (the draw groups of an extracted level), and that group's parent (which copy
/// of the level, in a document); "" for none
fn piece_level(scene: &Scene, n: &Node) -> (u32, String) {
  let mut found = (0, String::new());
  let mut guard = 0;
  let mut p = if n.parent != 0 {
    scene.find(n.parent)
  } else {
    None
  };
  while let Some(g) = p {
    if guard >= 1000 {
      break;
    }
    guard += 1;
    for kind in ["-tfrag-", "-tie-", "-shrub-"] {
      if let Some(at) = g.name.find(kind)
        && at > 0
      {
        found = (g.parent, g.name[..at].to_string());
      }
    }
    p = if g.parent != 0 {
      scene.find(g.parent)
    } else {
      None
    };
  }
  found
}

/// "<level>" of a group "<level>-collision" (the game's collision of a level)
fn collision_group_level(name: &str) -> Option<&str> {
  name.strip_suffix("-collision").filter(|l| !l.is_empty())
}

/// The parts of triangle abc to keep, `gone(point)` saying where the collision has no piece left:
/// all of it, none, or (when its corners and its middle disagree) its quarters, each decided again,
/// down to 1 m
fn keep_parts(
  a: Vec3,
  b: Vec3,
  c: Vec3,
  depth: i32,
  gone: &impl Fn(Vec3) -> bool,
  out: &mut impl FnMut(Vec3, Vec3, Vec3),
) {
  let middle = gone((a + b + c) * (1.0 / 3.0));
  let longest = length2(b - a).max(length2(c - b)).max(length2(a - c));
  if depth < 6 && longest > 1.0 && (gone(a) != middle || gone(b) != middle || gone(c) != middle) {
    let (ab, bc, ca) = ((a + b) * 0.5, (b + c) * 0.5, (c + a) * 0.5);
    keep_parts(a, ab, ca, depth + 1, gone, out);
    keep_parts(ab, b, bc, depth + 1, gone, out);
    keep_parts(ca, bc, c, depth + 1, gone, out);
    keep_parts(ab, bc, ca, depth + 1, gone, out);
    return;
  }
  if !middle {
    out(a, b, c);
  }
}

/// Jak 3 gathers the collision around Jak in a cache of 461 triangles (collide-cache-h.gc); beyond
/// it, the triangles of the place are dropped and Jak can fall through the floor. The box it fills,
/// around his feet: his root sphere (2.2 m, 1.2 m up) and 1 m more (fill-cache-for-shape).
const K_COLLIDE_CACHE_TRIS: usize = 460;
/// the pieces of decor colliding by their shape are 4 m across at least (walls, floors, catwalks,
/// stairs); the smaller ones (unless given a collision in the editor, or shapes of the editor) none
const K_SMALL_PIECE: f32 = 4.0;
const K_CACHE_BOX_LO: Vec3 = Vec3::new(-3.2, -2.0, -3.2);
const K_CACHE_BOX_HI: Vec3 = Vec3::new(3.2, 4.4, 3.2);

/// The places of the floors (the faces up, a point every 3 m) where the cache would get more
/// triangles than it holds (`sides`: 2 when the builder doubles every face): how many, and where,
/// the densest first
fn collision_hot_spots(all: &TriGrid, sides: usize) -> Vec<(usize, Vec3)> {
  let mut floors: HashMap<u64, Vec3> = HashMap::new();
  let mut sample = |p: Vec3| {
    floors
      .entry(TriGrid::key(
        (p.x / 3.0).floor() as i32,
        p.y.floor() as i32,
        (p.z / 3.0).floor() as i32,
      ))
      .or_insert(p);
  };
  for &[a, b, c] in &all.tris {
    let n = cross(b - a, c - a);
    let len = length(n);
    if len <= 0.0 || n.y < 0.7 * len {
      continue; // facing up
    }
    let longest = length2(b - a)
      .max(length2(c - b))
      .max(length2(a - c))
      .sqrt();
    let steps = 64.min((longest / 3.0).ceil() as i32);
    if steps <= 1 {
      sample((a + b + c) * (1.0 / 3.0));
      continue;
    }
    for i in 0..=steps {
      for j in 0..=(steps - i) {
        sample(a + (b - a) * (i as f32 / steps as f32) + (c - a) * (j as f32 / steps as f32));
      }
    }
  }
  let mut out = Vec::new();
  for &at in floors.values() {
    let mut bx = AABB::default();
    bx.add(at + K_CACHE_BOX_LO);
    bx.add(at + K_CACHE_BOX_HI);
    if all.bound(&bx) * sides <= K_COLLIDE_CACHE_TRIS {
      continue;
    }
    let n = all.count(&bx) * sides;
    if n > K_COLLIDE_CACHE_TRIS {
      out.push((n, at));
    }
  }
  // the densest first; the same count by place, the order of a hash map being no order
  out.sort_by(|a, b| {
    b.0
      .cmp(&a.0)
      .then_with(|| a.1.x.total_cmp(&b.1.x))
      .then_with(|| a.1.y.total_cmp(&b.1.y))
      .then_with(|| a.1.z.total_cmp(&b.1.z))
  });
  out
}

// ------------------------------------------------------------------------------------------------

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

/// The collision the element at `index` gets in the game when the editor sets none.
pub fn default_decor_collision(scene: &Scene, index: usize) -> DefaultCollision {
  let cache = scene.cache();
  let Some(mesh) = scene.nodes.get(index).and_then(|n| n.mesh.as_ref()) else {
    return DefaultCollision::Shape;
  };
  if index >= cache.bounds.len() {
    return DefaultCollision::Shape;
  }
  let n = &scene.nodes[index];
  // a piece of a level whose collision is in the document: its "<level>-collision" group, next to it
  let level = piece_level(scene, n);
  if !level.1.is_empty() {
    let group = format!("{}-collision", level.1);
    if scene
      .nodes
      .iter()
      .any(|g| g.kind == NodeKind::Group && g.parent == level.0 && g.name == group)
    {
      return DefaultCollision::Game;
    }
  }
  if see_through(scene, mesh) {
    return DefaultCollision::SeeThrough;
  }
  let b = &cache.bounds[index];
  if n.extras.get("og_shape").is_none() && b.valid() && max3(b.size()) < K_SMALL_PIECE {
    return DefaultCollision::Small;
  }
  DefaultCollision::Shape
}

fn max3(v: Vec3) -> f32 {
  v.x.max(v.y).max(v.z)
}

/// Whether the node at index `i` is part of the document's own level: not under a level added to it,
/// a companion, a mission layer, nor a reference.
pub fn own_level_node(scene: &Scene, i: usize) -> bool {
  let c = scene.cache();
  if i >= scene.nodes.len() {
    return false;
  }
  let marked = |n: &Node| {
    n.kind == NodeKind::Group
      && n.extras.as_object().is_some_and(|e| {
        ["og_level", "og_companion", "og_layer", "og_reference"]
          .iter()
          .any(|k| e.contains_key(*k))
      })
  };
  if c.parent.len() == c.count && i < c.count {
    let mut k = i as i32;
    let mut guard = 0;
    while k >= 0 && guard < 1000 {
      if marked(&scene.nodes[k as usize]) {
        return false;
      }
      k = c.parent[k as usize];
      guard += 1;
    }
    return true;
  }
  let mut n = Some(&scene.nodes[i]);
  let mut guard = 0;
  while let Some(node) = n {
    if guard >= 1000 {
      break;
    }
    if marked(node) {
      return false;
    }
    n = if node.parent != 0 {
      scene.find(node.parent)
    } else {
      None
    };
    guard += 1;
  }
  true
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

/// A collision surface as the level builder takes it, and what it does not keep of it ("hdwood",
/// "rail", "halfpipe", "nopilot"...: "" when nothing).
pub fn builder_surface(pat: u32) -> (BuilderSurface, String) {
  let mut lost = String::new();
  let mut s = BuilderSurface {
    noentity: pat & 1 != 0,
    nocamera: (pat >> 1) & 1 != 0,
    noedge: (pat >> 2) & 1 != 0,
    ..Default::default()
  };
  // goal_src/jak3/engine/collide/pat-h.gc: the builder takes Jak 1's fields and converts them,
  // Jak 1's stone (0) becoming Jak 3's stone (23); what Jak 1 does not have becomes the nearest
  let (mode, material, event) = (
    ((pat >> 7) & 7) as i32,
    ((pat >> 10) & 63) as usize,
    ((pat >> 18) & 63) as usize,
  );
  s.nolineofsight = (pat >> 16) & 1 != 0;
  s.mode = mode;
  if mode > 2 {
    s.mode = 0; // the halfpipes of the boards: ground
    lost = "halfpipe".into();
  }
  const K_NEAREST: [i32; 34] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
    0,  // stone
    16, // crmetal: metal
    17, // carpet: straw
    16, // grmetal: metal
    16, // shmetal: metal
    6,  // hdwood: wood
    19, // squish: swamp
    7,  // mhshroom: grass
    7,  // forest: grass
    19, // mhswamp: swamp
    16, // dmaker: metal
  ];
  s.material = K_NEAREST.get(material).copied().unwrap_or(0);
  // the level builder's collision fields are written in Jak 1's own layout (the builder converts
  // them: goalc/build_level/collide/common); a surface without a material (none) becomes stone
  let kept = material == 23 || (1..=22).contains(&material);
  if !kept {
    lost = material_name(material);
  }
  s.event = if event <= 6 { event as i32 } else { 0 };
  if event > 6 {
    lost = event_name(event);
  }
  // the flags Jak 1's fields do not have (boards, vehicles): the builder sets none of them.
  // nogrind is on nearly every surface (the board grinds rails only), and off on every surface
  // of the levels it builds: not said
  if lost.is_empty() {
    const K_FLAGS: [(u32, &str); 8] = [
      (4, "nojak"),
      (5, "noboard"),
      (6, "nopilot"),
      (24, "probe"),
      (25, "nomech"),
      (26, "noproj"),
      (27, "noendlessfall"),
      (28, "noprobe"),
    ];
    for (bit, name) in K_FLAGS {
      if (pat >> bit) & 1 != 0 {
        if !lost.is_empty() {
          lost.push('+');
        }
        lost += name;
      }
    }
  }
  (s, lost)
}

/// A res lump value of an extracted actor as the builder reads it: typed arrays (["float", ...]),
/// strings and 'symbols. `ty`: the extractor's type of the tag (lump_types), guessed when "".
/// Null for data the builder cannot write.
pub fn builder_lump_value(value: &Value, ty: &str) -> Value {
  let ty = if ty.is_empty() { guess_type(value) } else { ty };
  let strings = |tag: &str, strip: bool| -> Value {
    let one = |s: &str| Value::from(if strip { strip_quote(s) } else { s });
    let mut out = vec![Value::from(tag)];
    match value {
      Value::String(s) => out.push(one(s)),
      Value::Array(a) => out.extend(a.iter().filter_map(Value::as_str).map(one)),
      _ => {}
    }
    if out.len() > 1 {
      Value::Array(out)
    } else {
      Value::Null
    }
  };
  match ty {
    "string" => {
      // a plain string is one; one beginning with a quote would be read as a symbol
      if value.as_str().is_some_and(|s| !s.starts_with('\'')) {
        return value.clone();
      }
      strings("string", false)
    }
    "symbol" => match value.as_str() {
      Some(s) => format!("'{}", strip_quote(s)).into(),
      None => strings("symbol", true),
    },
    "type" => strings("type", true),
    "pair" => {
      if value.is_string() {
        json!(["pair", value])
      } else {
        Value::Null
      }
    }
    "float" | "int32" | "int16" | "int8" | "uint32" | "uint16" | "uint8" => {
      let mut ns = Vec::new();
      numbers(value, &mut ns);
      if ns.is_empty() {
        return Value::Null;
      }
      let (is_float, is_signed) = (ty == "float", ty.starts_with('i'));
      let mut out = vec![Value::from(if is_float {
        "float"
      } else if is_signed {
        "int32"
      } else {
        "uint32"
      })];
      for n in ns {
        let d = n.as_f64().unwrap_or(0.0);
        out.push(if is_float {
          d.into()
        } else if is_signed {
          (d.round() as i64 as i32).into()
        } else {
          (d.round() as i64 as u32).into()
        });
      }
      Value::Array(out)
    }
    "vector" => {
      let Some(a) = value.as_array().filter(|a| !a.is_empty()) else {
        return Value::Null;
      };
      let vec4 = |v: &[Value]| -> Value {
        (0..4)
          .map(|i| match v.get(i).and_then(Value::as_f64) {
            Some(x) => x,
            None if i == 3 => 1.0,
            None => 0.0,
          })
          .collect()
      };
      let mut out = vec![Value::from("vector")];
      if a[0].is_array() {
        out.extend(a.iter().filter_map(Value::as_array).map(|v| vec4(v)));
      } else {
        out.push(vec4(a));
      }
      Value::Array(out)
    }
    _ => Value::Null,
  }
}

// --- what the editor gives the export ------------------------------------------------------------

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
/// does not have ("my-" and the level's), the folder <library_root>/export/<project_folder>
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
  // its textures): <library>/export/<project>/
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
  o.folder = from_path(&Path::new(&root).join("export").join(project));
  o.name = name;
  o.start = export_start_point(scene, data);
  o
}

/// The levels whose collision the document has (its "<level>-collision" groups: "jak2-<level>" for
/// a level of Jak II): the export compares their decor with the level as extracted
/// (LevelExportOptions::originals) when it keeps the game's collision. Each once, in order.
pub fn collision_levels(scene: &Scene) -> Vec<String> {
  let mut out: Vec<String> = Vec::new();
  for n in &scene.nodes {
    if n.kind == NodeKind::Group
      && let Some(level) = collision_group_level(&n.name)
      && !out.iter().any(|l| l == level)
    {
      out.push(level.to_string());
    }
  }
  out
}

/// A level as extracted (its LevelEntry::background), for LevelExportOptions::originals. A level
/// of Jak II is then made decor of Jak 3 as the document has it (make_jak2_level) by the caller.
pub fn read_original(background: &Path) -> Result<Scene, String> {
  let mut scene = Scene::new();
  import_gltf(&mut scene, background, &ImportOptions::default())?;
  scene.begin_frame();
  Ok(scene)
}

// --- the export ----------------------------------------------------------------------------------

/// The pieces of a level's decor still where the game has them: its collision holds for them.
struct Pieces {
  /// compared with the level as extracted
  known: bool,
  kept: HashSet<String>,
  /// 4 m cells: searched 8 m around
  kept_tris: TriGrid,
  removed_tris: TriGrid,
  removed: usize,
}

struct Exported {
  index: usize,
  ty: String,
  name: String,
}

/// Writes the level (see the module's doc). `data`: the game's data (None: the actor types are not
/// checked). The scene's cache must be the current one (Scene::begin_frame after the last changes).
/// The error is a text to show (a translation, or "cannot write <file>").
pub fn export_level(
  scene: &Scene,
  data: Option<&GameData>,
  o: &LevelExportOptions,
) -> Result<LevelExportReport, String> {
  let mut rep = LevelExportReport::default();
  if let Some(bad) = check_export_names(o, data) {
    return Err(tr(bad).to_string());
  }
  // a level replacing one of the game's: the document's own level alone (the levels added next to
  // it are the game's, loaded by the game as they are); never a level shown for reference
  let replacing = !o.replaces.is_empty();
  let replaced = if replacing {
    data.and_then(|d| d.level(&o.replaces))
  } else {
    None
  };
  // the export's folder, laid out like the jak-project checkout it is copied into
  let install_dir = format!("custom_assets/jak3/levels/{}/", o.name);
  let code_dir = format!("goal_src/jak3/levels/{}/", o.name);
  let top = PathBuf::from(&o.folder);
  let dir = top
    .join("custom_assets")
    .join("jak3")
    .join("levels")
    .join(&o.name);
  let _ = std::fs::create_dir_all(&dir);
  if !dir.is_dir() {
    return Err(format!("cannot create {}", from_path(&dir)));
  }
  rep.dir = from_path(&top);
  rep.level_dir = from_path(&dir);
  let base_id = if o.base_id != 0 {
    o.base_id
  } else {
    K_DEFAULT_BASE_ID
  };
  let mut notes = Notes::default();

  let cache = scene.cache();
  let count = scene.nodes.len().min(cache.visible.len());
  let exported_part = |i: usize| {
    if replacing {
      own_level_node(scene, i)
    } else {
      !cache.reference.get(i).copied().unwrap_or(false)
    }
  };
  // the elements of the levels next to it (replacing)
  let left_out_one = |i: usize| (replacing && cache.visible[i] && !cache.reference[i]) as usize;
  let mut left_out = 0;
  let index: HashMap<u32, usize> = (0..count).map(|i| (scene.nodes[i].id, i)).collect();
  let parent_index = |n: &Node| index.get(&n.parent).copied();

  // the levels with the game's collision: the parent of their "<level>-collision" group, and <level>
  // (a level is told from another copy of it, a reference, by that parent)
  let mut with_collision: BTreeSet<(u32, String)> = BTreeSet::new();
  for i in 0..count {
    let n = &scene.nodes[i];
    if n.kind != NodeKind::Collision || n.mesh.is_none() || !exported_part(i) {
      continue;
    }
    let Some(p) = parent_index(n) else { continue };
    let g = &scene.nodes[p];
    if let Some(level) = collision_group_level(&g.name) {
      with_collision.insert((g.parent, level.to_string()));
    }
  }
  // The pieces of a level's decor still where the game has them: its collision holds for them. The
  // game's collision of the others (deleted, moved, hidden, or given a collision of their own in
  // the editor) is taken out where they were: the level as extracted (o.originals) tells them apart.
  let mut pieces: BTreeMap<(u32, String), Pieces> = BTreeMap::new();
  if o.game_collision {
    for level in &with_collision {
      let Some(orig) = o.originals.get(&level.1) else {
        continue;
      };
      // the document's pieces of that level, shown, where they are
      let mut here: HashMap<&str, AABB> = HashMap::new();
      for i in 0..count {
        let n = &scene.nodes[i];
        if n.kind != NodeKind::Render
          || n.mesh.is_none()
          || cache.actor[i] != 0
          || !cache.visible[i]
        {
          continue;
        }
        if decor_behavior_of(&n.extras).collision.set {
          continue;
        }
        if piece_level(scene, n) == *level {
          here.insert(&n.name, cache.bounds[i]);
        }
      }
      let ps = pieces.entry(level.clone()).or_insert_with(|| Pieces {
        known: false,
        kept: HashSet::new(),
        kept_tris: TriGrid::new(4.0),
        removed_tris: TriGrid::new(4.0),
        removed: 0,
      });
      ps.known = true;
      let (mut kept, mut removed) = (Vec::new(), Vec::new());
      for n in &orig.nodes {
        if n.kind != NodeKind::Render || n.mesh.is_none() || piece_level(orig, n).1 != level.1 {
          continue;
        }
        let was = orig.world_bounds(n);
        let same = here
          .get(n.name.as_str())
          .is_some_and(|h| length(h.lo - was.lo) < 0.05 && length(h.hi - was.hi) < 0.05);
        if same {
          ps.kept.insert(n.name.clone());
          kept.push(n);
        } else {
          removed.push(n);
        }
      }
      ps.removed = removed.len();
      if removed.is_empty() {
        continue; // nothing to take out
      }
      let add = |grid: &mut TriGrid, n: &Node| {
        let w = orig.world(n);
        for prim in &n.mesh.as_ref().unwrap().prims {
          for [a, b, c] in triangles(prim) {
            grid.add(w.point(a), w.point(b), w.point(c));
          }
        }
      };
      for n in removed {
        add(&mut ps.removed_tris, n);
      }
      for n in kept {
        add(&mut ps.kept_tris, n);
      }
    }
  }
  // the game's collision holds for a piece of a level that has it, still in place
  let covered_by_game = |i: usize| {
    let n = &scene.nodes[i];
    let level = piece_level(scene, n);
    if level.1.is_empty() || !with_collision.contains(&level) {
      return false;
    }
    pieces
      .get(&level)
      .is_none_or(|p| !p.known || p.kept.contains(&n.name))
  };

  // --- the decor -------------------------------------------------------------------------------
  let mut eo = ExportOptions {
    mode: ExportMode::Builder,
    ..Default::default()
  };
  let mut hidden = 0;
  // the elements that bounce or launch Jak: actors of the level's code
  let mut decor_actions = Vec::new();
  // the elements whose collision the builder makes from their shape
  let mut by_shape = Vec::new();
  let mut bounds = AABB::default();
  for i in 0..count {
    let n = &scene.nodes[i];
    let Some(mesh) = &n.mesh else { continue };
    if n.kind != NodeKind::Render || cache.actor[i] != 0 {
      continue;
    }
    if !exported_part(i) {
      left_out += left_out_one(i);
      continue;
    }
    if !cache.visible[i] {
      hidden += 1;
      continue;
    }
    let covered = covered_by_game(i);
    eo.only.push(n.id);
    rep.meshes += 1;
    rep.triangles += mesh.tri_count();
    let b = cache.bounds.get(i).copied().unwrap_or_default();
    if b.valid() {
      bounds.add(b.lo);
      bounds.add(b.hi);
    }
    // the game's collision there, or none for see-through surfaces (water, glass): the builder
    // makes the collision of the others from their shape; a collision set in the editor (its
    // surface, an invisible wall, none) where the game's does not already hold it
    let decor = decor_behavior_of(&n.extras);
    if decor.collision.set && !(o.game_collision && covered) {
      let mut e = collision_extras(&decor.collision);
      if decor.collision.kind != "none" {
        rep.collision_from_decor += 1;
        by_shape.push(i);
        e["double_sided"] = 1.into();
      }
      eo.node_extras.insert(n.id, e);
    } else if (o.game_collision && covered) || see_through(scene, mesh) {
      eo.node_extras
        .insert(n.id, json!({"set_collision": 1, "ignore": 1}));
      if decor.collision.set {
        notes.add("export.note.decor_collision_game", &n.name, 1);
      }
    } else if n.extras.get("og_shape").is_none() && b.valid() && max3(b.size()) < K_SMALL_PIECE {
      // a small piece (a pipe, a lamp, a window, a decal): the game gives most of its details no
      // collision, and theirs would fill the cache around Jak (he falls through the floor there)
      eo.node_extras
        .insert(n.id, json!({"set_collision": 1, "ignore": 1}));
      notes.add("export.note.collision_small", &n.name, 1);
    } else {
      rep.collision_from_decor += 1;
      by_shape.push(i);
      // the decor faces either way (the game draws both sides, its ground often faces down): a
      // collision made from it holds from both sides (the game's own collision stays one-sided)
      eo.node_extras.insert(n.id, json!({"double_sided": 1}));
    }
    if decor.bounce.set || decor.launch.set {
      decor_actions.push(i);
    }
  }
  if rep.collision_from_decor > 0 {
    notes.add(
      "export.note.collision_from_decor",
      "",
      rep.collision_from_decor,
    );
  }

  // --- the game's collision: one mesh per surface, in the game's space ---------------------------
  let mut game_collision: Vec<Arc<Mesh>> = Vec::new();
  if o.game_collision {
    let mut surfaces: BTreeMap<BuilderSurface, Mesh> = BTreeMap::new();
    let mut taken_out = 0;
    for i in 0..count {
      let n = &scene.nodes[i];
      let Some(mesh) = &n.mesh else { continue };
      if n.kind != NodeKind::Collision || !exported_part(i) {
        continue;
      }
      let p = parent_index(n);
      if p.is_some_and(|p| !cache.visible[p]) {
        continue; // its level is hidden
      }
      // the pieces of its level that were taken out: their collision with them
      let level_pieces = p
        .and_then(|p| {
          let g = &scene.nodes[p];
          collision_group_level(&g.name).and_then(|l| pieces.get(&(g.parent, l.to_string())))
        })
        .filter(|ps| !ps.removed_tris.is_empty());
      // nearer to a piece taken out than to the pieces in place, by 5 cm at least, and within 8 m of
      // it: the game's collision of a piece in place lies on it, so it stays; around rubble it is
      // coarser than its pieces (the fallen palace's in the port: 1 to 5 m off them, which a closer
      // limit left standing as invisible walls across the street)
      const K_NEAR: f32 = 8.0;
      let w = &cache.worlds[i];
      for prim in &mesh.prims {
        let normals = prim.nrm.len() == prim.pos.len();
        for (t, tri) in prim.idx.as_chunks::<3>().0.iter().enumerate() {
          if tri.iter().any(|&v| v as usize >= prim.pos.len()) {
            continue;
          }
          let pat = prim.tri_pat.get(t).copied().unwrap_or(0);
          let (s, loss) = builder_surface(pat);
          let out = &mut surfaces
            .entry(s)
            .or_insert_with(|| {
              let mut m = Mesh::new();
              m.name = format!(
                "{}-collision-{:02}-{}-{}",
                o.name, s.material, s.event, s.mode
              );
              m.prims.push(Primitive::default());
              m
            })
            .prims[0];
          let v = tri.map(|k| w.point(prim.pos[k as usize]));
          let mut nrm = Vec3::new(0.0, 0.0, 0.0);
          if normals {
            // the game's normals: the builder turns the triangles that face the other way
            for &k in tri {
              nrm += w.dir(prim.nrm[k as usize]);
            }
          }
          nrm = if normals && length2(nrm) > 0.0 {
            normalize(nrm)
          } else {
            Vec3::new(0.0, 1.0, 0.0)
          };
          let before = out.idx.len();
          let mut write = |a: Vec3, b: Vec3, c: Vec3| {
            for q in [a, b, c] {
              out.idx.push(out.pos.len() as u32);
              out.pos.push(q);
              out.nrm.push(nrm);
            }
          };
          if let Some(lp) = level_pieces {
            let gone = |at: Vec3| {
              let to_removed = lp.removed_tris.distance(at, K_NEAR);
              to_removed < K_NEAR && to_removed + 0.05 < lp.kept_tris.distance(at, K_NEAR)
            };
            keep_parts(v[0], v[1], v[2], 0, &gone, &mut write);
          } else {
            write(v[0], v[1], v[2]);
          }
          let written = (out.idx.len() - before) / 3;
          if written == 0 {
            taken_out += 1;
            continue;
          }
          rep.collision_triangles += written;
          if !loss.is_empty() {
            notes.add("export.note.surfaces", &loss, 1);
          }
        }
      }
    }
    for (s, mut mesh) in surfaces {
      let p = &mut mesh.prims[0];
      p.idx.truncate(p.idx.len() / 3 * 3);
      if p.idx.is_empty() {
        continue;
      }
      if p.nrm.iter().all(|n| n.x == 0.0 && n.y == 1.0 && n.z == 0.0) {
        p.compute_normals();
      }
      let mesh = Arc::new(mesh);
      game_collision.push(mesh.clone());
      eo.extra_meshes.push((
        mesh,
        json!({
          "set_collision": 1,
          "set_invisible": 1,
          "collide_material": s.material,
          "collide_event": s.event,
          "collide_mode": s.mode,
          "noentity": s.noentity as i32,
          "nocamera": s.nocamera as i32,
          "noedge": s.noedge as i32,
          "nolineofsight": s.nolineofsight as i32,
        }),
      ));
    }
    let pieces_out: usize = pieces.values().map(|ps| ps.removed).sum();
    if taken_out > 0 {
      notes.add(
        "export.note.collision_taken_out",
        &pieces_out.to_string(),
        1,
      );
      if taken_out > 1 {
        notes.add("export.note.collision_taken_out", "", taken_out - 1);
      }
    }
  }

  // --- where Jak would meet more collision than the game gathers at once -----------------------
  {
    let mut all = TriGrid::new(2.0);
    for mesh in &game_collision {
      for prim in &mesh.prims {
        for [a, b, c] in triangles(prim) {
          all.add(a, b, c);
        }
      }
    }
    // the faces made from the decor's shape twice: the builder doubles them
    for &i in &by_shape {
      let w = &cache.worlds[i];
      for prim in &scene.nodes[i].mesh.as_ref().unwrap().prims {
        for [a, b, c] in triangles(prim) {
          for _side in 0..2 {
            all.add(w.point(a), w.point(b), w.point(c));
          }
        }
      }
    }
    let dense = collision_hot_spots(&all, 1);
    for (n, at) in dense.iter().take(3) {
      notes.add(
        "export.note.collision_dense",
        &format!("({:.0}, {:.0}, {:.0}): {n}", at.x, at.y, at.z),
        1,
      );
    }
    if dense.len() > 3 {
      notes.add("export.note.collision_dense", "", dense.len() - 3);
    }
    rep.dense_places = dense.len();
    rep.densest = dense.into_iter().take(100).collect();
  }

  // --- the actors ------------------------------------------------------------------------------
  // first the actors exported and their names (no other actor of the level has it), by their id
  // in the game: the links between actors by id become links by name (the builder numbers the
  // actors again)
  let mut exported: Vec<Exported> = Vec::new();
  let mut name_of_aid: HashMap<u32, String> = HashMap::new();
  // the name in the editor -> in the level
  let mut name_of_node: HashMap<String, String> = HashMap::new();
  let mut absent = 0;
  {
    let mut names = HashSet::new();
    for i in 0..count {
      let n = &scene.nodes[i];
      if n.kind != NodeKind::Actor {
        continue;
      }
      let etype = n
        .extras
        .get("etype")
        .and_then(Value::as_str)
        .unwrap_or_default();
      // without actors: decor alone, but the platforms and bridges needed to get around stay
      // (the same rule the document itself loads a level by)
      if !o.actors && !data.is_some_and(|d| d.object_family(etype) == "platform") {
        continue;
      }
      if !exported_part(i) {
        left_out += left_out_one(i);
        continue;
      }
      if !cache.visible[i] {
        hidden += 1;
        continue;
      }
      if etype.is_empty() {
        continue;
      }
      if data.is_some_and(|d| !d.has_type(etype)) {
        notes.add("export.note.types_missing", etype, 1);
        continue;
      }
      // never there (its behavior): not in the level
      let present = n
        .extras
        .get("og_behavior")
        .and_then(|b| b.get("present"))
        .unwrap_or(&Value::Null);
      if condition_of(present, Condition::default()).kind == ConditionKind::Never {
        absent += 1;
        continue;
      }
      let base = if n.name.is_empty() { etype } else { &n.name };
      let mut name = base.to_string();
      let mut k = 2;
      while names.contains(&name) {
        name = format!("{base}-{k}");
        k += 1;
      }
      names.insert(name.clone());
      if !n.name.is_empty() {
        name_of_node
          .entry(n.name.clone())
          .or_insert_with(|| name.clone());
      }
      if let Some(aid) = n.extras.get("aid").filter(|a| is_integer(a)) {
        let aid = aid
          .as_i64()
          .map_or_else(|| aid.as_u64().unwrap_or(0) as u32, |a| a as u32);
        name_of_aid.insert(aid, name.clone());
      }
      exported.push(Exported {
        index: i,
        ty: etype.to_string(),
        name,
      });
    }
  }
  let mut actor_lines: Vec<String> = Vec::new();
  let mut art_groups: BTreeSet<String> = BTreeSet::new();
  let mut kill_masks = 0;
  // the code of the level (<name>-obs.gc)
  let mut generated = String::new();
  // the code of the game the level needs, in order
  let mut code_files: Vec<String> = Vec::new();
  // The code the game already has around the level replaced: the levels it shows with it (its
  // neighbors, the levels of its continue points; an alias of a level, as ctywide-kg, is that
  // level, ctywide). The DGO does not bring it again (the effects of the city draw particle
  // groups of ctywide-part, which needs the code of the whole city).
  let mut loaded_around: HashSet<String> = HashSet::new();
  if let (Some(replaced), Some(data)) = (replaced, data) {
    let mut around: BTreeSet<String> = replaced.neighbors.iter().cloned().collect();
    for c in &replaced.continues {
      around.extend(c.shows.iter().cloned());
    }
    around.remove(&o.replaces);
    for mut name in around {
      let mut l = data.level(&name);
      while l.is_none_or(|l| l.code.is_empty())
        && let Some(dash) = name.rfind('-')
      {
        name.truncate(dash);
        l = data.level(&name);
      }
      if let Some(l) = l
        && name != o.replaces
      {
        loaded_around.extend(l.code.iter().cloned());
      }
    }
  }
  let mut need = |files: &[String]| {
    for f in files {
      if !loaded_around.contains(f) && !code_files.contains(f) {
        code_files.push(f.clone());
      }
    }
  };
  // the level replaced: its own code (its particles, its objects, what its missions spawn there)
  if let Some(r) = replaced {
    need(&r.code);
  }
  for e in &exported {
    let n = &scene.nodes[e.index];
    // the code of its type, and of the particle group it draws (a part-spawner's art-name)
    if let Some(data) = data {
      need(&data.code_files(&e.ty));
      if let Some(art) = n
        .extras
        .get("lump")
        .and_then(|l| l.get("art-name"))
        .and_then(Value::as_str)
      {
        need(&data.effect_code_files(strip_quote(art)));
      }
    }
    // its behavior, as a type of the level deriving its own, the actors it names by their names in
    // the level
    let mut etype = e.ty.clone();
    if let Value::Object(mut behavior) = n
      .extras
      .get("og_behavior")
      .cloned()
      .unwrap_or_else(|| json!({}))
    {
      for c in behavior.values_mut() {
        let renamed = c
          .get("arg")
          .and_then(Value::as_str)
          .and_then(|a| name_of_node.get(a))
          .cloned();
        if let Some(name) = renamed {
          c["arg"] = name.into();
        }
      }
      let own = behavior_type_name(&o.name, &e.name);
      let code = behavior_code(
        data,
        &e.ty,
        &Value::Object(behavior),
        &own,
        &format!("{} ({})", e.name, e.ty),
      );
      if !code.is_empty() {
        generated += &code;
        generated.push('\n');
        etype = own;
        rep.behaviors += 1;
      }
    }
    // where it was in the game: the positions inside its data move with it
    let mut moved = Mat4::identity();
    let mut has_origin = false;
    if let Some(og) = n
      .extras
      .get("og_origin")
      .and_then(Value::as_array)
      .filter(|a| a.len() == 7)
    {
      let f = |k: usize| og[k].as_f64().unwrap_or(0.0) as f32;
      let t0 = Transform {
        t: Vec3::new(f(0), f(1), f(2)),
        r: Quat::new(f(3), f(4), f(5), f(6)),
        ..Default::default()
      };
      let mut t1 = Transform::from_matrix(&cache.worlds[e.index]);
      t1.s = Vec3::new(1.0, 1.0, 1.0);
      moved = t1.matrix() * t0.matrix().inverse();
      has_origin = true;
    }
    let path_tags: HashSet<&str> = data
      .and_then(|d| d.params(&e.ty))
      .map(|ps| {
        ps.iter()
          .filter(|p| p.kind == "path")
          .map(|p| p.tag.as_str())
          .collect()
      })
      .unwrap_or_default();

    let w = &cache.worlds[e.index];
    let pos = w.point(Vec3::new(0.0, 0.0, 0.0));
    let q = Transform::from_matrix(w).r.normalized();
    let mut radius = n
      .extras
      .get("bsphere")
      .and_then(Value::as_array)
      .filter(|b| b.len() == 4)
      .and_then(|b| b[3].as_f64())
      .unwrap_or(0.0) as f32;
    if radius <= 0.0 {
      // a placed one: its model's
      for c in scene.children(n.id) {
        if let Some(mesh) = scene.find(c).and_then(|m| m.mesh.as_ref()) {
          radius = radius.max(mesh.bounds().radius());
        }
      }
      if radius <= 0.0 {
        radius = 5.0;
      }
    }

    // its data (res lumps)
    let mut lump = Map::new();
    lump.insert("name".into(), e.name.clone().into());
    let types = n.extras.get("lump_types").filter(|t| t.is_object());
    if let Some(Value::Object(lj)) = n.extras.get("lump") {
      for (tag, v) in lj {
        if tag == "name" || tag == "visvol" {
          continue; // its name is set, its visibility box is the game's
        }
        let ltype = types
          .and_then(|t| t.get(tag))
          .and_then(Value::as_str)
          .unwrap_or_default();
        if tag == "kill-mask" {
          // when the actor is there along the story of its level: the new level has its own
          if v.as_f64() != Some(0.0) {
            kill_masks += 1;
          }
          continue;
        }
        if ltype == "actor-name" {
          // a link set in the editor: another actor by its name in the editor, then in the level
          match v.as_str().and_then(|s| name_of_node.get(s)) {
            Some(name) => {
              lump.insert(tag.clone(), json!(["string", name]));
            }
            None => notes.add("export.note.links_dropped", tag, 1),
          }
          continue;
        }
        if (ltype == "uint32" || ltype == "int32" || ltype.is_empty())
          && tag.len() > 6
          && tag.ends_with("-actor")
          && tag != "task-actor"
        {
          // other actors by their ids: by their names, when they are in the level
          let mut ids = Vec::new();
          numbers(v, &mut ids);
          let mut linked = vec![Value::from("string")];
          for id in &ids {
            let aid = id.as_f64().unwrap_or(0.0) as i64 as u32;
            let Some(name) = name_of_aid.get(&aid) else {
              break;
            };
            linked.push(name.clone().into());
          }
          if ids.is_empty() || linked.len() != ids.len() + 1 {
            notes.add("export.note.links_dropped", tag, 1);
            continue;
          }
          lump.insert(tag.clone(), Value::Array(linked));
          continue;
        }
        if ltype == "pair" {
          // a script of the game: the level builder cannot write them (the behavior set in the
          // editor is the level's code instead)
          notes.add("export.note.scripts_dropped", tag, 1);
          continue;
        }
        if matches!(ltype, "int8" | "int16" | "uint8" | "uint16") {
          notes.add("export.note.lumps_widened", tag, 1);
        }
        let mut bv = builder_lump_value(v, ltype);
        if has_origin
          && path_tags.contains(tag.as_str())
          && let Some(points) = bv.as_array_mut()
          && points.len() > 1
          && points[0] == "vector"
        {
          // a path, in game units: moved as the actor was
          for pt in &mut points[1..] {
            let c = |k: usize| pt[k].as_f64().unwrap_or(0.0) as f32;
            let p = moved.point(Vec3::new(c(0), c(1), c(2)) / 4096.0) * 4096.0;
            pt[0] = p.x.into();
            pt[1] = p.y.into();
            pt[2] = p.z.into();
          }
        }
        if bv.is_null() {
          let what = if ltype.is_empty() {
            tag.clone()
          } else {
            format!("{tag} ({ltype})")
          };
          notes.add("export.note.lumps_dropped", &what, 1);
          continue;
        }
        lump.insert(tag.clone(), bv);
      }
    }
    let game_task = n
      .extras
      .get("game_task")
      .and_then(Value::as_i64)
      .unwrap_or(0) as i32;
    actor_lines.push(format!(
      "{{\"trans\": [{}, {}, {}], \"etype\": \"{etype}\", \"game_task\": {game_task}, \"quat\": [{}, {}, {}, {}], \"bsphere\": [{}, {}, {}, {}], \"lump\": {}}}",
      num(pos.x as f64),
      num(pos.y as f64),
      num(pos.z as f64),
      num(q.x as f64),
      num(q.y as f64),
      num(q.z as f64),
      num(q.w as f64),
      num(pos.x as f64),
      num(pos.y as f64),
      num(pos.z as f64),
      num(radius as f64),
      Value::Object(lump)
    ));
    rep.actors += 1;
    // the models its type draws: their art groups, from the game's DGOs
    if let Some(data) = data
      && let Some(models) = data.type_models(&e.ty)
    {
      for m in models {
        let ag = data.level_art_group(m);
        if !ag.is_empty() {
          art_groups.insert(format!("{ag}-ag"));
        }
      }
    }
  }
  // the decor that bounces or launches Jak: an actor of the level's code on its top
  {
    let mut taken: HashSet<String> = exported.iter().map(|e| e.name.clone()).collect();
    for &i in &decor_actions {
      let n = &scene.nodes[i];
      let decor = decor_behavior_of(&n.extras);
      let b = cache.bounds[i];
      if !b.valid() {
        continue;
      }
      let top = Vec3::new(b.center().x, b.hi.y, b.center().z);
      let radius = (b.size().x.max(b.size().z) * 0.5).clamp(1.0, 20.0);
      let mut add_actor = |what: &str, code_of_type: String, mut lump: Value, ty: &str| {
        let base = format!("{}-{what}", n.name);
        let mut name = base.clone();
        let mut k = 2;
        while taken.contains(&name) {
          name = format!("{base}-{k}");
          k += 1;
        }
        taken.insert(name.clone());
        generated += &code_of_type;
        generated.push('\n');
        rep.behaviors += 1;
        lump["name"] = name.into();
        actor_lines.push(format!(
          "{{\"trans\": [{}, {}, {}], \"etype\": \"{ty}\", \"game_task\": 0, \"quat\": [0.0, 0.0, 0.0, 1.0], \"bsphere\": [{}, {}, {}, {}], \"lump\": {lump}}}",
          num(top.x as f64),
          num(top.y as f64),
          num(top.z as f64),
          num(top.x as f64),
          num(top.y as f64),
          num(top.z as f64),
          num(radius as f64)
        ));
        rep.actors += 1;
      };
      if decor.bounce.set {
        let ty = behavior_type_name(&o.name, &format!("{}-bounce", n.name));
        let comment = format!(
          "{}: throws Jak up {} m",
          n.name,
          num(decor.bounce.height as f64)
        );
        add_actor(
          "bounce",
          bounce_code(&ty, radius, &comment),
          json!({"spring-height": ["float", decor.bounce.height * 4096.0]}),
          &ty,
        );
      }
      if decor.launch.set {
        // where Jak lands: `distance` along the element's facing turned by `yaw`, `height` above
        let t = Transform::from_matrix(&cache.worlds[i]);
        let mut facing = t.r.rotate(Vec3::new(0.0, 0.0, 1.0));
        facing.y = 0.0;
        facing = if length2(facing) > 1e-6 {
          normalize(facing)
        } else {
          Vec3::new(0.0, 0.0, 1.0)
        };
        let dir =
          Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), radians(decor.launch.yaw)).rotate(facing);
        let to = top + dir * decor.launch.distance + Vec3::new(0.0, decor.launch.height, 0.0);
        let to_units = [to.x * 4096.0, to.y * 4096.0, to.z * 4096.0];
        let ty = behavior_type_name(&o.name, &format!("{}-launch", n.name));
        let comment = format!(
          "{}: throws Jak {} m away, {} m up",
          n.name,
          num(decor.launch.distance as f64),
          num(decor.launch.height as f64)
        );
        add_actor(
          "launch",
          launch_code(&ty, radius, to_units, decor.launch.time, &comment),
          json!({}),
          &ty,
        );
      }
    }
  }
  if kill_masks > 0 {
    notes.add("export.note.kill_masks", "", kill_masks);
  }
  if left_out > 0 {
    notes.add("export.note.replace_left_out", "", left_out);
  }
  // where the game starts the player in the level replaced: on the ground of the new one?
  if replaced.is_some() {
    for c in check_continues(scene, data, &o.replaces) {
      if !c.ok() {
        notes.add("export.note.continue_no_ground", &c.name, 1);
      }
    }
  }
  if absent > 0 {
    notes.add("export.note.absent", "", absent);
  }
  if hidden > 0 {
    notes.add("export.note.hidden", "", hidden);
  }
  rep.art_groups = art_groups.len();

  // --- the files -------------------------------------------------------------------------------
  let glb = dir.join(format!("{}.glb", o.name));
  export_glb(scene, &glb, &eo)?;
  rep.files.push(from_path(&glb));

  let mut nav_line = String::new();
  if !o.navmeshes.is_empty() {
    // their ids after the actors', far from them
    let first = base_id + rep.actors as u32 + 1000;
    let entries: Vec<NavExportEntry> = o
      .navmeshes
      .iter()
      .enumerate()
      .map(|(i, mesh)| NavExportEntry {
        mesh,
        aid: first + i as u32,
        name: o
          .navmesh_names
          .get(i)
          .cloned()
          .unwrap_or_else(|| format!("nav-mesh-{i}")),
      })
      .collect();
    let nav = dir.join(format!("{}-nav.json", o.name));
    write_navmesh_blob(&entries, &nav)?;
    rep.files.push(from_path(&nav));
    rep.navmeshes = entries.len();
    nav_line = format!("  \"nav_data\": \"{install_dir}{}-nav.json\",\n", o.name);
  }

  // the regions of the level replaced: the levels the game loads and shows around it
  let mut regions_line = String::new();
  if replacing && o.region_trees.as_object().is_some_and(|t| !t.is_empty()) {
    let regions = builder_regions(&o.region_trees, base_id + 2000, &mut rep.regions);
    regions_line = format!(
      "  // where the game loads and shows the levels around this one (the level replaced's)\n  \"region_trees\": {regions},\n"
    );
  } else if replacing && !o.region_trees.is_object() {
    // unknown (extracted before the regions were): {} is a level that has none (ctywide)
    notes.add("export.note.regions_missing", &o.replaces, 1);
  }

  // the level for the builder
  let from = if scene.settings.level.is_empty() {
    "a new level"
  } else {
    &scene.settings.level
  };
  let auto_walls = rep.collision_from_decor > 0 || !o.game_collision;
  let art_list: Vec<String> = art_groups.iter().map(|ag| format!("\"{ag}\"")).collect();
  let mut j = format!(
    "// {name}: exported by Jak Level Editor for Jak 3 from {from}.
// To add it to the game: README.md, at the top of the export's folder.
{{
  \"long_name\": \"{name}\",
  \"iso_name\": \"{iso}\",
  \"nickname\": \"{nick}\",
  \"gltf_file\": \"{install_dir}{name}.glb\",
  // the game's collision is in the file; the builder makes the rest from the decor's shape
  \"automatic_wall_detection\": {auto_walls},
  \"automatic_wall_angle\": 45.0,
  \"double_sided_collide\": {double},
  \"base_id\": {base_id},
  \"base_region_id\": {base_id},
  \"cameras\": [],
{regions_line}{nav_line}  \"custom_models\": [],
  \"textures\": [],
  \"art_groups\": [{arts}],
  \"actors\": [
",
    name = o.name,
    iso = o.iso_name,
    nick = o.nickname,
    // the faces made from the decor are doubled each (extras double_sided, the builder of the
    // editor's fork); all of them when the level has no collision of the game's
    double = rep.collision_from_decor > 0 && !o.game_collision,
    arts = art_list.join(", "),
  );
  for (i, line) in actor_lines.iter().enumerate() {
    j += &format!(
      "    {line}{}",
      if i + 1 < actor_lines.len() {
        ",\n"
      } else {
        "\n"
      }
    );
  }
  j += "  ]\n}\n";
  let jsonc = dir.join(format!("{}.jsonc", o.name));
  write_text(&jsonc, &j)?;
  rep.files.push(from_path(&jsonc));

  // the level's code: the behaviors set in the editor
  let code_stem = format!("{}-obs", o.name);
  if !generated.is_empty() {
    let gc = format!(
      ";;-*-Lisp-*-
(in-package goal)

;; {}: the behaviors of its objects, set in Jak Level Editor. Each object
;; whose behavior differs from the game's has a type of its own deriving the game's.

{generated}",
      o.name
    );
    let gc_dir = top
      .join("goal_src")
      .join("jak3")
      .join("levels")
      .join(&o.name);
    let _ = std::fs::create_dir_all(&gc_dir);
    let gc_path = gc_dir.join(format!("{code_stem}.gc"));
    write_text(&gc_path, &gc)?;
    rep.files.push(from_path(&gc_path));
    rep.code_file = from_path(&gc_path);
  }

  // the DGO
  let nick_upper = o.nickname.to_ascii_uppercase();
  let mut gd = format!(
    ";; DGO definition file for {}, exported by Jak Level Editor for Jak 3.\n(\"{nick_upper}.DGO\"\n (\n",
    o.name
  );
  // the code of the game its objects need beyond the common code, then its own
  for f in &code_files {
    gd += &format!("  \"{f}.o\"\n");
  }
  if !generated.is_empty() {
    gd += &format!("  \"{code_stem}.o\"\n");
  }
  for ag in &art_groups {
    gd += &format!("  \"{ag}.go\"\n");
  }
  gd += &format!("  \"{}.go\"\n  ))\n", o.name);
  let gd_path = dir.join(format!("{}.gd", o.name));
  write_text(&gd_path, &gd)?;
  rep.files.push(from_path(&gd_path));

  // the level's definition in the game's code
  let bottom = if bounds.valid() {
    bounds.lo.y
  } else {
    o.start.y
  } - 20.0;
  let info_path = dir.join("level-info.gc");
  let info_text = if let Some(r) = replaced {
    // the game's own definition stays (continue points, music, missions...): its nickname names the
    // DGO the game loads for it
    format!(
      ";; {name} replaces {rep_name} in the game. In goal_src/jak3/engine/level/level-info.gc,
;; in (define {rep_name} ...), the game loads <nickname>.DGO for the level: change its
;; nickname so that it loads {nick_upper}.DGO, this level (put '{old} back to play the game's own {rep_name}):
;;
;;   :nickname '{old}
;; becomes
    :nickname '{nick}
",
      name = o.name,
      rep_name = o.replaces,
      old = r.nickname,
      nick = o.nickname
    )
  } else {
    format!(
      ";; add this to goal_src/jak3/engine/level/level-info.gc\n\n{}",
      level_info(o, bottom)
    )
  };
  write_text(&info_path, &info_text)?;
  rep.files.push(from_path(&info_path));

  rep.notes = notes.list();

  // how to add it to the game
  let readme = readme(
    scene,
    o,
    &rep,
    replaced,
    &install_dir,
    &code_dir,
    &code_stem,
    &generated,
    &code_files,
  );
  let md_path = top.join("README.md");
  write_text(&md_path, &readme)?;
  rep.files.push(from_path(&md_path));
  Ok(rep)
}

/// The README.md at the top of the export: how to add the level to the game.
#[allow(clippy::too_many_arguments)]
fn readme(
  scene: &Scene,
  o: &LevelExportOptions,
  rep: &LevelExportReport,
  replaced: Option<&LevelInfo>,
  install_dir: &str,
  code_dir: &str,
  code_stem: &str,
  generated: &str,
  code_files: &[String],
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
`custom_assets`) it goes into:

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
   (build-custom-level \"{name}\")
",
    if has_code {
      "s `custom_assets` and `goal_src`"
    } else {
      " `custom_assets`"
    },
    if has_code { "they merge" } else { "it merges" },
    if has_code { "s" } else { "" }
  );
  if has_code {
    // after the common code and the code of the game it derives from
    md += &format!("   (goal-src \"levels/{name}/{code_stem}.gc\" \"entity-debug\"");
    for f in code_files {
      md += &format!(" \"{f}\"");
    }
    md += ")\n";
  }
  md += &format!("   (custom-level-cgo \"{nick_upper}.DGO\" \"{name}/{name}.gd\")\n   ```\n\n");
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
4. Build the game: start the compiler (`goalc`, or `task repl` in the checkout), type `(mi)`, then
   start the game (`gk`). Go there as in the game (from a level next to it), or in the REPL:
   `(lt)` then `(start 'play (get-continue-by-name *game-info* \"{first}\"))`.

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
4. Build the game: start the compiler (`goalc`, or `task repl` in the checkout), type `(mi)`, then
   start the game (`gk`) and, in the REPL, `(lt)` then go there:
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
      "- The nav meshes are in `{name}-nav.json` (`nav_data` of the level): OpenGOAL's level builder
  for Jak 3 writes them in the level only when it reads `nav_data` (see the editor's README).
"
    );
  }
  md += "- The lighting of the decor is the game's: the 8 palettes of each vertex (`_SUNRISE`...), which the
  game mixes with the hour of the day and the level's lights; the pieces made in the editor are lit
  like the level's from their color.
- The surfaces seen through (stains, decals, glass, water, glows) are blended over what is behind
  them by the level builder of the editor's jak-project branch (the translucent tree, and the PS2
  blend of each material: `extras.og_blend`); OpenGOAL's own builder does not draw them.
- That builder also: makes the collision of the decor colliding by its shape hold from both sides
  (`extras.double_sided`, the game's collision staying one-sided), lists everything visible in the
  level (no visibility data: with the PC settings' PS2 actor visibility, the actors of a level are
  born only where it says they are seen) and keeps the grid of a collision spread over a whole
  city to 2 million cells (the level's memory).
";
  if rep.regions > 0 {
    md += &format!(
      "- The level's regions are the ones of {} ({}): where the game loads and shows the levels around it as Jak or the camera goes by.\n",
      o.replaces, rep.regions
    );
  }
  md
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::asset_library::AssetLibrary;
  use crate::io::game_data::tests::real_data;
  use crate::io::test_library::{
    glb_accessor_bytes, make_box, make_floor, read_glb_json, real_library, temp_dir,
  };
  use crate::scene::story::{StoryView, apply_story};
  use crate::scene::{AlphaMode, Material, TodColors, TodPalette};

  fn read_text(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
  }

  /// a .jsonc of the builder, its comment lines left out
  fn read_jsonc(p: &Path) -> Value {
    let text = read_text(p);
    let lines: Vec<&str> = text
      .lines()
      .filter(|l| !l.trim_start().starts_with("//"))
      .collect();
    serde_json::from_str(&lines.join("\n")).unwrap_or(Value::Null)
  }

  /// the nodes of a .glb by name: their extras
  fn glb_extras(glb: &Value) -> HashMap<String, Value> {
    glb["nodes"]
      .as_array()
      .into_iter()
      .flatten()
      .map(|n| {
        (
          n["name"].as_str().unwrap_or_default().to_string(),
          n.get("extras").cloned().unwrap_or_else(|| json!({})),
        )
      })
      .collect()
  }

  fn render(name: &str, mesh: Arc<Mesh>, parent: u32, at: Vec3) -> Node {
    let mut n = Node {
      name: name.into(),
      kind: NodeKind::Render,
      parent,
      mesh: Some(mesh),
      ..Default::default()
    };
    n.local.t = at;
    n
  }

  fn group(name: &str) -> Node {
    Node {
      name: name.into(),
      kind: NodeKind::Group,
      ..Default::default()
    }
  }

  #[test]
  fn surfaces_as_the_builder_takes_them() {
    let (wall, loss) = builder_surface(0x5cc8); // stone wall, nopilot
    assert!(wall.material == 0 && wall.mode == 1 && wall.event == 0 && loss == "nopilot");
    assert_eq!(builder_surface((28 << 10) | (13 << 18)).1, "rail"); // hdwood, rail
    let (hdwood, loss) = builder_surface(28 << 10);
    assert!(hdwood.material == 6 && loss == "hdwood");
    let (wood, loss) = builder_surface((6 << 10) | (1 << 16) | 4); // wood, nolos, noedge
    assert!(
      wood.material == 6 && wood.nolineofsight && wood.noedge && loss.is_empty(),
      "export: collision surfaces as the builder takes them (Jak 1's fields), what is lost said"
    );
  }

  #[test]
  fn lumps_as_the_builder_reads_them() {
    let v = builder_lump_value;
    assert_eq!(v(&json!("abc"), "string"), json!("abc"));
    assert_eq!(v(&json!("'steel"), "symbol"), json!("'steel"));
    assert_eq!(
      v(&json!(["'a", "'b"]), "symbol"),
      json!(["symbol", "a", "b"])
    );
    assert_eq!(v(&json!(1.5), "float"), json!(["float", 1.5]));
    assert_eq!(v(&json!([3, 4]), "uint8"), json!(["uint32", 3, 4]));
    assert_eq!(
      v(&json!([1, 2, 3, 4]), "vector"),
      json!(["vector", [1.0, 2.0, 3.0, 4.0]])
    );
    assert_eq!(
      v(&json!("'citb-bunny"), "type"),
      json!(["type", "citb-bunny"])
    );
    assert_eq!(
      v(&json!("(want-load 'a)"), "pair"),
      json!(["pair", "(want-load 'a)"])
    );
    assert_eq!(v(&json!(3), ""), json!(["int32", 3]));
    assert!(v(&json!([1]), "actor-group").is_null());
    // the regions' scripts: their integers as floats, not inside a string
    assert_eq!(
      builder_script("(want-vis 'a 2 -3 \"x 4\") (b 1.5)"),
      "(want-vis 'a 2.0 -3.0 \"x 4\") (b 1.5)"
    );
    assert!(
      default_nickname("my-test") == "mte"
        && default_nickname("a") == "max"
        && default_iso_name("my-test") == "MYTEST"
        && default_iso_name("--") == "CUSTOM"
        && num(-0.00001) == "0.0"
        && num(1115.82081) == "1115.8208"
        && num(2.0) == "2.0"
    );
  }

  /// a small level: its shell with the game's collision, a box placed, actors; then the same in
  /// place of ctyindb (src/app/selftest.cpp)
  #[test]
  fn a_small_level() {
    let data = real_data();
    let tmp = temp_dir("game-export");
    let mut s = Scene::new();
    s.settings.level = "testlvl".into();
    // blended with its texture's alpha, as the levels' materials nearly all are: still a wall
    let m = Material {
      name: "stone".into(),
      alpha: AlphaMode::Blend,
      base_color: crate::math::Vec4::new(2.0, 2.0, 2.0, 2.0), // a level's (the PS2's colors)
      ..Default::default()
    };
    let mat = s.add_material(m.clone());
    let glass = s.add_material(Material {
      name: "glass".into(),
      translucent: true, // drawn by the game without writing its depth
      ..m
    });
    let tfrag_id = s.add(group("testlvl-tfrag-normal"));
    let mut floor = render(
      "testlvl-floor",
      make_floor(mat),
      tfrag_id,
      Vec3::new(0.0, 0.0, 0.0),
    );
    {
      // its lighting in the game: one color, each of its 8 palettes recognizable
      let mut pal = TodPalette::default();
      for k in 0..8u8 {
        pal.rgba.extend([10 + k, 20 + k, 30 + k, 128]);
      }
      floor.tod = Some(Arc::new(TodColors {
        palette: Some(Arc::new(pal)),
        index: vec![vec![0; floor.mesh.as_ref().unwrap().prims[0].pos.len()]],
      }));
    }
    s.add(floor);
    let col_group = s.add(group("testlvl-collision"));
    let mut col_mesh = make_floor(-1);
    Arc::make_mut(&mut col_mesh).prims[0].tri_pat = vec![0x5cc8, 28 << 10];
    s.add(Node {
      name: "testlvl-collision-0".into(),
      kind: NodeKind::Collision,
      parent: col_group,
      mesh: Some(col_mesh),
      ..Default::default()
    });
    let mut bx = render("box-1", make_box("box", mat), 0, Vec3::new(10.0, 0.0, 0.0));
    bx.local.s = Vec3::new(3.0, 3.0, 3.0); // 6 m: a piece that collides by its shape
    s.add(bx);
    // see through: the builder's translucent tree
    s.add(render(
      "glass-1",
      make_box("glass", glass),
      0,
      Vec3::new(30.0, 0.0, 0.0),
    ));
    // 2 m: a detail, without collision
    s.add(render(
      "lamp-1",
      make_box("lamp", mat),
      0,
      Vec3::new(20.0, 0.0, 0.0),
    ));
    let mut actor =
      |name: &str, etype: &str, aid: u32, mut lump: Value, types: Value, visible: bool| {
        lump["name"] = name.into();
        let mut a = Node {
          name: name.into(),
          kind: NodeKind::Actor,
          visible,
          extras: json!({"etype": etype, "aid": aid, "game_task": 7, "bsphere": [5, 1, 5, 2],
          "lump": lump, "lump_types": types}),
          ..Default::default()
        };
        a.local.t = Vec3::new(5.0, 1.0, 5.0);
        s.add(a);
      };
    actor(
      "crate-1",
      "crate",
      101,
      json!({"eco-info": [9, 5], "kill-mask": 16384, "task-actor": 3, "next-actor": 102,
        "visvol": [[0, 0, 0, 1], [1, 1, 1, 1]]}),
      json!({"eco-info": "int32", "kill-mask": "uint32", "task-actor": "uint8",
        "next-actor": "uint32", "visvol": "vector", "name": "string"}),
      true,
    );
    actor(
      "cty-door-1",
      "cty-door",
      102,
      json!({"on-activate": "(want-load 'x)"}),
      json!({"on-activate": "pair"}),
      true,
    );
    actor(
      "cty-fruit-stand-1",
      "cty-fruit-stand",
      103,
      json!({}),
      json!({}),
      true,
    );
    actor("crate-2", "crate", 104, json!({}), json!({}), false);
    actor(
      "switch-a",
      "sew-floor-switch",
      105,
      json!({}),
      json!({}),
      true,
    );
    actor(
      "vent-1",
      "industrial-board-vent",
      106,
      json!({"spring-height": 40960.0}),
      json!({"spring-height": "float"}),
      true,
    );
    actor("ghost-1", "crate", 107, json!({}), json!({}), true);
    actor(
      "plat-1",
      "plat",
      108,
      json!({"path": [[0, 0, 0, 1], [40960, 0, 0, 1]]}),
      json!({"path": "vector"}),
      true,
    );
    for a in &mut s.nodes {
      match a.name.as_str() {
        "cty-door-1" => {
          a.extras["og_behavior"] =
            json!({"open": "when", "when": {"when": "pressed", "arg": "switch-a"}, "distance": 12})
        }
        "vent-1" => {
          a.extras["og_behavior"] =
            json!({"active": {"when": "task", "arg": "arena-fight-1-resolution"}})
        }
        "ghost-1" => a.extras["og_behavior"] = json!({"present": {"when": "never"}}),
        "crate-1" => {
          a.extras["lump"]["alt-actor"] = "switch-a".into();
          a.extras["lump_types"]["alt-actor"] = "actor-name".into();
        }
        "plat-1" => {
          // found at (5, 1, 5) in the game, moved 10 m along x since
          a.extras["og_origin"] = json!([5, 1, 5, 0, 0, 0, 1]);
          a.local.t = Vec3::new(15.0, 1.0, 5.0);
        }
        _ => {}
      }
    }
    s.begin_frame();

    let out = tmp.join("export");
    let o = LevelExportOptions {
      name: "my-test".into(),
      nickname: default_nickname("my-test"),
      iso_name: default_iso_name("my-test"),
      folder: from_path(&out),
      start: Vec3::new(1.0, 2.0, 3.0),
      ..Default::default()
    };
    assert_eq!(check_export_names(&o, None), None);
    let r = export_level(&s, Some(&data), &o).expect("exported");
    // laid out like the jak-project checkout: the level, its code, the README at the top
    let dir = out.join("custom_assets/jak3/levels/my-test");
    let code = out.join("goal_src/jak3/levels/my-test/my-test-obs.gc");
    assert!(
      r.files.len() == 6
        && r.behaviors == 2
        && out.join("README.md").is_file()
        && code.is_file()
        && Path::new(&r.level_dir) == dir
        && r.code_file == from_path(&code),
      "export: the files ({:?})",
      r.files
    );
    for f in [
      "my-test.glb",
      "my-test.jsonc",
      "my-test.gd",
      "level-info.gc",
    ] {
      assert!(dir.join(f).is_file(), "{f}");
    }
    let level = read_jsonc(&dir.join("my-test.jsonc"));
    let by_name: HashMap<String, &Value> = level["actors"]
      .as_array()
      .expect("actors")
      .iter()
      .map(|a| {
        (
          a["lump"]["name"].as_str().unwrap_or_default().to_string(),
          a,
        )
      })
      .collect();
    let (crate_j, door_j, vent_j, plat_j) = (
      by_name["crate-1"],
      by_name["cty-door-1"],
      by_name["vent-1"],
      by_name["plat-1"],
    );
    assert_eq!(level["actors"].as_array().unwrap().len(), 5);
    assert!(!by_name.contains_key("ghost-1"));
    assert_eq!(crate_j["etype"], "crate");
    assert_eq!(crate_j["lump"]["eco-info"], json!(["int32", 9, 5]));
    assert_eq!(
      crate_j["lump"]["next-actor"],
      json!(["string", "cty-door-1"])
    );
    assert_eq!(crate_j["lump"]["alt-actor"], json!(["string", "switch-a"]));
    assert_eq!(crate_j["lump"]["task-actor"], json!(["uint32", 3]));
    assert_eq!(crate_j["game_task"], 7);
    assert!(crate_j["lump"].get("kill-mask").is_none() && crate_j["lump"].get("visvol").is_none());
    assert_eq!(door_j["etype"], "my-test-cty-door-1");
    assert!(door_j["lump"].get("on-activate").is_none());
    assert_eq!(vent_j["etype"], "my-test-vent-1");
    assert_eq!(vent_j["lump"]["spring-height"], json!(["float", 40960.0]));
    assert_eq!(plat_j["etype"], "plat");
    assert_eq!(
      plat_j["lump"]["path"],
      json!(["vector", [40960.0, 0.0, 0.0, 1.0], [81920.0, 0.0, 0.0, 1.0]]),
      "export: a path moved with its actor"
    );
    assert!(
      level["automatic_wall_detection"] == true
        && level["nickname"] == "mte"
        && level["base_id"] == K_DEFAULT_BASE_ID
    );
    // the level's code and the code of the game it needs, in its DGO
    let (gc, gd, install) = (
      read_text(&code),
      read_text(&dir.join("my-test.gd")),
      read_text(&out.join("README.md")),
    );
    let at = |s: &str| gd.find(s).unwrap_or(usize::MAX);
    let (at_part, at_obs, at_own, at_level) = (
      at("\"ctyinda-part.o\""),
      at("\"ctyinda-obs.o\""),
      at("\"my-test-obs.o\""),
      at("\"my-test.go\""),
    );
    assert!(
      gc.contains("(in-package goal)")
        && gc.contains("(deftype my-test-cty-door-1 (cty-door) ())")
        && gc.contains("(entity-status? \"switch-a\" subtask-complete)")
        && gc.contains("(deftype my-test-vent-1 (industrial-board-vent) ())")
        && gd.contains("\"sewer-obs.o\"")
        && at_part < at_obs
        && at_obs < at_own
        && at_own < at_level
        && at_level != usize::MAX
        && install.contains("(goal-src \"levels/my-test/my-test-obs.gc\" \"entity-debug\"")
        && install.contains("(get-continue-by-name *game-info* \"my-test-start\")"),
      "export: the behaviors as the level's code, its DGO with the game's code its objects need"
    );
    let glb = read_glb_json(&dir.join("my-test.glb"));
    let extras = glb_extras(&glb);
    let surfaces = extras.values().filter(|e| e["set_invisible"] == 1).count();
    let keys: BTreeSet<&str> = r.notes.iter().map(|n| n.key.as_str()).collect();
    for k in [
      "export.note.types_missing",
      "export.note.surfaces",
      "export.note.kill_masks",
      "export.note.hidden",
      "export.note.scripts_dropped",
      "export.note.lumps_widened",
      "export.note.absent",
      "export.note.collision_small",
    ] {
      assert!(keys.contains(k), "{k}");
    }
    assert!(
      surfaces == 2
        && extras["testlvl-floor"]["ignore"] == 1
        && extras["box-1"].get("set_collision").is_none()
        && extras["lamp-1"]["ignore"] == 1
        && r.collision_triangles == 2
        && r.collision_from_decor == 1
        && r.dense_places == 0,
      "export: the game's collision one node per surface, the shell not colliding twice, a 6 m box \
       by its shape, a 2 m lamp without collision"
    );
    // a level's blended walls (their depth written) cut out in the builder's solid tree; the decor
    // faces either way, its collision holds from both sides (the game's own stays one-sided)
    let materials = glb["materials"].as_array().expect("materials");
    let blend = |m: &Value| m["alphaMode"] == "BLEND";
    let glass_blended = materials
      .iter()
      .any(|m| m["name"] == "glass" && blend(m) && m["extras"]["og_blend"] == "alpha");
    let walls_cut = materials
      .iter()
      .filter(|m| m["name"] != "glass")
      .all(|m| !blend(m));
    assert!(
      walls_cut
        && glass_blended
        && extras["box-1"]["double_sided"] == 1
        && level["double_sided_collide"] == false,
      "export: a level's blended walls cut out, its glass in the builder's translucent tree \
       (og_blend), the decor's collision from both sides"
    );
    // the game's lighting: every mesh with the 8 palettes the builder reads, a node's own as it has
    // them (the floor's), the new geometry's made from its color
    let mut all_lit = glb.get("meshes").is_some();
    let mut floor_exact = false;
    for n in glb["nodes"].as_array().unwrap() {
      // (the game's collision is not drawn)
      let Some(mesh) = n["mesh"].as_u64() else {
        continue;
      };
      if n["extras"]["set_invisible"] == 1 {
        continue;
      }
      for p in glb["meshes"][mesh as usize]["primitives"]
        .as_array()
        .unwrap()
      {
        for k in [
          "_SUNRISE",
          "_MORNING",
          "_NOON",
          "_AFTERNOON",
          "_SUNSET",
          "_TWILIGHT",
          "_EVENING",
          "_GREENSUN",
        ] {
          all_lit &= p["attributes"].get(k).is_some();
        }
      }
      if n["name"] == "testlvl-floor" {
        let p = &glb["meshes"][mesh as usize]["primitives"][0];
        let noon = glb_accessor_bytes(
          &dir.join("my-test.glb"),
          &glb,
          p["attributes"]["_NOON"].as_u64().unwrap() as usize,
        );
        floor_exact = noon.len() >= 4 && noon[..4] == [12, 22, 32, 255];
      }
    }
    assert!(
      all_lit && floor_exact,
      "export: the 8 palettes of the game's lighting on every mesh, a piece's own exactly"
    );

    // the same level in place of ctyindb: a level of the game next to it and the original shown for
    // reference are left out; ground under ctyindb-start only
    let mut bad = o.clone();
    bad.name = "ctyindb".into();
    assert_eq!(
      check_export_names(&bad, Some(&data)),
      Some("export.bad.name_taken")
    );
    let mut bad = o.clone();
    bad.nickname = "cib".into();
    assert_eq!(
      check_export_names(&bad, Some(&data)),
      Some("export.bad.nickname_taken")
    );
    let mut bad = o.clone();
    bad.replaces = "no-such-level".into();
    assert_eq!(
      check_export_names(&bad, Some(&data)),
      Some("export.bad.replaces")
    );
    for (name, key) in [("ctyinda", None), ("ctyindb", Some("og_reference"))] {
      let mut g = group(name);
      g.extras = json!({ "og_level": name });
      if let Some(key) = key {
        g.extras[key] = true.into();
      }
      let gid = s.add(g);
      s.add(render(
        &format!("{name}-box"),
        make_box("box", mat),
        gid,
        Vec3::new(0.0, 0.0, 0.0),
      ));
    }
    s.add(render(
      "start-ground",
      make_floor(mat),
      0,
      Vec3::new(971.42, 7.0, 546.11),
    ));
    s.begin_frame();
    let checks = check_continues(&s, Some(&data), "ctyindb");
    let start_ok = checks
      .iter()
      .any(|c| c.name == "ctyindb-start" && c.ok() && (c.ground - 7.0).abs() < 1e-3);
    let falling = checks
      .iter()
      .filter(|c| c.name != "ctyindb-start" && !c.ok())
      .count();
    let ro = LevelExportOptions {
      name: "my-ctyb".into(),
      nickname: "mcy".into(),
      replaces: "ctyindb".into(),
      folder: from_path(&tmp.join("export-replace")),
      ..o.clone()
    };
    assert_eq!(check_export_names(&ro, Some(&data)), None);
    let rr = export_level(&s, Some(&data), &ro).expect("exported in place of ctyindb");
    let rdir = tmp.join("export-replace/custom_assets/jak3/levels/my-ctyb");
    let (info, rgd, rinstall) = (
      read_text(&rdir.join("level-info.gc")),
      read_text(&rdir.join("my-ctyb.gd")),
      read_text(&tmp.join("export-replace/README.md")),
    );
    let rextras = glb_extras(&read_glb_json(&rdir.join("my-ctyb.glb")));
    let counts: HashMap<&str, usize> = rr.notes.iter().map(|n| (n.key.as_str(), n.count)).collect();
    let (at_part, at_obs) = (
      rgd.find("\"ctyindb-part.o\"").unwrap_or(usize::MAX),
      rgd.find("\"ctyindb-obs.o\"").unwrap_or(usize::MAX),
    );
    // the floor of 100 m under ctyindb-start also holds ctyindb-hijack-vehicle; the three others fall
    assert!(start_ok && checks.len() == 5 && falling == 3, "{checks:?}");
    assert!(
      !rextras.contains_key("ctyinda-box")
        && !rextras.contains_key("ctyindb-box")
        && rextras.contains_key("start-ground")
        && !info.contains("level-load-info")
        && info.contains(":nickname 'cib")
        && info.contains(":nickname 'mcy")
        && at_part < at_obs
        && at_obs != usize::MAX
        && rinstall.contains("(get-continue-by-name *game-info* \"ctyindb-start\")")
        && counts.get("export.note.replace_left_out") == Some(&1)
        && counts.get("export.note.continue_no_ground") == Some(&falling)
        && counts.contains_key("export.note.regions_missing"),
      "export in place of a level of the game: its own level only, the replaced level's code in \
       its DGO, the nickname to change, the continue points without ground said ({:?})",
      rr.notes
    );
  }

  /// the decor's behaviors: a slab of deadly ice, a cable Jak goes through, a pad that bounces, a
  /// ramp that launches
  #[test]
  fn the_decors_behaviors() {
    let data = real_data();
    let tmp = temp_dir("game-export-decor");
    let mut d = Scene::new();
    for (name, x, decor) in [
      (
        "ice-slab",
        0.0,
        json!({"collision": {"material": "ice", "event": "deadly", "noedge": true}}),
      ),
      ("cable", 5.0, json!({"collision": {"kind": "none"}})),
      ("pad", 10.0, json!({"bounce": {"height": 15}})),
      (
        "ramp",
        15.0,
        json!({"launch": {"distance": 30, "height": 5, "yaw": 90, "time": 1.5}}),
      ),
    ] {
      let mut n = render(name, make_box(name, -1), 0, Vec3::new(x, 0.0, 0.0));
      n.extras = json!({ "og_decor": decor });
      d.add(n);
    }
    d.begin_frame();
    let o = LevelExportOptions {
      name: "my-decor".into(),
      nickname: "mdc".into(),
      iso_name: "MYDECOR".into(),
      folder: from_path(&tmp.join("export-decor")),
      ..Default::default()
    };
    let dr = export_level(&d, Some(&data), &o).expect("exported");
    let dd = tmp.join("export-decor/custom_assets/jak3/levels/my-decor");
    let extras = glb_extras(&read_glb_json(&dd.join("my-decor.glb")));
    let dlevel = read_jsonc(&dd.join("my-decor.jsonc"));
    let actors: HashMap<&str, &Value> = dlevel["actors"]
      .as_array()
      .expect("actors")
      .iter()
      .map(|a| (a["lump"]["name"].as_str().unwrap_or_default(), a))
      .collect();
    let dgc = read_text(&tmp.join("export-decor/goal_src/jak3/levels/my-decor/my-decor-obs.gc"));
    let ice = &extras["ice-slab"];
    assert!(ice["collide_material"] == 1 && ice["collide_event"] == 1 && ice["noedge"] == 1);
    assert_eq!(extras["cable"]["ignore"], 1);
    let pad = actors["pad-bounce"];
    assert!(
      pad["etype"] == "my-decor-pad-bounce"
        && pad["lump"]["spring-height"] == json!(["float", 15.0 * 4096.0])
        && (pad["trans"][1].as_f64().unwrap() - 2.0).abs() < 1e-3
    );
    assert_eq!(actors["ramp-launch"]["etype"], "my-decor-ramp-launch");
    // the ramp faces +z: turned 90 degrees, Jak lands toward +x, 30 m away, 5 m above its top
    assert!(dgc.contains("(set-vector! (-> this landing) 184320.0 28672.0 0.0 1.0)"));
    assert!(
      dgc.contains("(deftype my-decor-pad-bounce (bouncer) ())")
        && dgc.contains("(deftype my-decor-ramp-launch (process-drawable)")
        && dgc.contains("'launch-dir")
        && dgc.contains("450")
        && dr.behaviors == 2,
      "export: the decor's collision (deadly ice, none), a bounce and a launch as actors of the \
       level's code"
    );
  }

  /// the game's collision follows its decor: a level as extracted (a floor, a wall, a block, their
  /// collision), and the document made from it, the wall deleted and the block moved 30 m
  #[test]
  fn the_games_collision_follows_its_decor() {
    let data = real_data();
    let tmp = temp_dir("game-export-follow");
    let level_scene = |edited: bool| {
      let mut sc = Scene::new();
      let tfrag = sc.add(group("lvx-tfrag-normal"));
      let mut piece = |name: &str, mesh: Arc<Mesh>, at: Vec3, scale: Vec3| {
        let mut n = render(name, mesh, tfrag, at);
        n.local.s = scale;
        sc.add(n);
      };
      let one = Vec3::new(1.0, 1.0, 1.0);
      piece("lvx-floor", make_floor(-1), Vec3::new(0.0, 0.0, 0.0), one);
      if !edited {
        let (at, size) = (Vec3::new(-20.0, 0.0, 0.0), Vec3::new(5.0, 5.0, 0.5));
        piece("lvx-wall", make_box("wall", -1), at, size);
        // rubble, deleted: its collision is coarser than it (a box 1.5 to 3 m off its faces)
        let (at, size) = (Vec3::new(0.0, 10.0, 20.0), Vec3::new(2.0, 2.0, 2.0));
        piece("lvx-rubble", make_box("rubble", -1), at, size);
      }
      let block_at = Vec3::new(20.0, 0.0, if edited { 30.0 } else { 0.0 });
      piece(
        "lvx-block",
        make_box("block", -1),
        block_at,
        Vec3::new(3.0, 3.0, 3.0),
      );
      let colg = sc.add(group("lvx-collision"));
      // the game's collision: the floor, and the wall and the block as they were
      let mut cmesh = make_floor(-1);
      let cp = &mut Arc::make_mut(&mut cmesh).prims[0];
      cp.uv.clear();
      let b = make_box("c", -1).prims[0].clone();
      for (at, scale) in [
        (Vec3::new(-20.0, 0.0, 0.0), Vec3::new(5.0, 5.0, 0.5)),
        (Vec3::new(20.0, 0.0, 0.0), Vec3::new(3.0, 3.0, 3.0)),
        (Vec3::new(0.0, 10.0, 20.0), Vec3::new(5.0, 5.0, 5.0)),
      ] {
        let base = cp.pos.len() as u32;
        for (p, nrm) in b.pos.iter().zip(&b.nrm) {
          cp.pos
            .push(Vec3::new(p.x * scale.x, p.y * scale.y, p.z * scale.z) + at);
          cp.nrm.push(*nrm);
        }
        cp.idx.extend(b.idx.iter().map(|v| base + v));
      }
      cp.tri_pat = vec![0; cp.idx.len() / 3];
      sc.add(Node {
        name: "lvx-collision-0".into(),
        kind: NodeKind::Collision,
        parent: colg,
        mesh: Some(cmesh),
        ..Default::default()
      });
      sc.begin_frame();
      sc
    };
    let original = Arc::new(level_scene(false));
    let mut doc = level_scene(true);
    assert_eq!(collision_levels(&doc), ["lvx"]);
    let mut fo = LevelExportOptions {
      name: "my-follow".into(),
      nickname: "mfl".into(),
      iso_name: "MYFOLLOW".into(),
      folder: from_path(&tmp.join("export-follow")),
      ..Default::default()
    };
    fo.originals.insert("lvx".into(), original);
    let fr = export_level(&doc, Some(&data), &fo).expect("exported");
    let taken_out = fr
      .notes
      .iter()
      .find(|n| n.key == "export.note.collision_taken_out")
      .map_or(0, |n| n.count);
    let extras = glb_extras(&read_glb_json(
      &tmp.join("export-follow/custom_assets/jak3/levels/my-follow/my-follow.glb"),
    ));
    // the floor's collision kept (and the bottoms of the wall and the block, on it), the rest of
    // theirs taken out, and all of the rubble's though it lies off it; the floor still the game's,
    // the block colliding by its shape where it is
    assert!(
      (2..=8).contains(&fr.collision_triangles)
        && taken_out >= 16
        && extras["lvx-floor"]["ignore"] == 1
        && extras["lvx-block"].get("set_collision").is_none(),
      "export: the game's collision taken out with the pieces deleted or moved (rubble's coarser \
       one too), a moved piece colliding by its shape: collision {}, taken out {taken_out}, block {}",
      fr.collision_triangles,
      extras["lvx-block"]
    );
    // and a dense piece (5 m, 3,200 triangles) on the floor: said as a place where Jak would fall
    let mut dp = Primitive::default();
    for z in 0..=40 {
      for x in 0..=40 {
        dp.pos.push(Vec3::new(
          x as f32 * 0.125 - 2.5,
          0.3 + 0.01 * ((x + z) % 2) as f32,
          z as f32 * 0.125 - 2.5,
        ));
      }
    }
    for z in 0..40u32 {
      for x in 0..40u32 {
        let a = z * 41 + x;
        dp.idx.extend([a, a + 41, a + 1, a + 1, a + 41, a + 42]);
      }
    }
    dp.compute_normals();
    let mut dense = Mesh::new();
    dense.prims.push(dp);
    doc.add(render(
      "grating",
      Arc::new(dense),
      0,
      Vec3::new(0.0, 0.0, -30.0),
    ));
    doc.begin_frame();
    fo.name = "my-dense".into();
    fo.nickname = "mdn".into();
    let fr2 = export_level(&doc, Some(&data), &fo).expect("exported");
    assert!(
      fr2.dense_places >= 1
        && fr2
          .notes
          .iter()
          .any(|n| n.key == "export.note.collision_dense"),
      "export: a place denser than Jak 3 gathers (461 triangles) said"
    );
  }

  // --- the real game's data ----------------------------------------------------------------------

  /// The options the C++ editor exported with, read back from its files: names, ids, start.
  fn options_of_export(export: &Path, name: &str) -> LevelExportOptions {
    let dir = export.join("custom_assets/jak3/levels").join(name);
    let level = read_jsonc(&dir.join(format!("{name}.jsonc")));
    let readme = read_text(&export.join("README.md"));
    let replaces = readme
      .lines()
      .next()
      .and_then(|l| l.split(", in place of ").nth(1))
      .unwrap_or_default()
      .to_string();
    let info = read_text(&dir.join("level-info.gc"));
    let start = info
      .split(":trans (static-vectorm ")
      .nth(1)
      .and_then(|s| s.split(')').next())
      .map(|s| {
        let v: Vec<f32> = s.split(' ').filter_map(|x| x.parse().ok()).collect();
        Vec3::new(v[0], v[1], v[2])
      })
      .unwrap_or(Vec3::new(0.0, 0.0, 0.0));
    LevelExportOptions {
      name: name.into(),
      nickname: level["nickname"].as_str().unwrap_or_default().into(),
      iso_name: level["iso_name"].as_str().unwrap_or_default().into(),
      base_id: level["base_id"].as_u64().unwrap_or(0) as u32,
      start,
      replaces,
      ..Default::default()
    }
  }

  /// The text files of an export (README, .jsonc, .gd, level-info.gc, the level's code): path in
  /// the export -> text
  fn export_texts(export: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, top: &Path, out: &mut BTreeMap<String, String>) {
      for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
          walk(&p, top, out);
        } else if p.extension().is_some_and(|x| x != "glb") {
          out.insert(from_path(p.strip_prefix(top).unwrap()), read_text(&p));
        }
      }
    }
    let mut out = BTreeMap::new();
    walk(export, export, &mut out);
    out
  }

  /// The first line two texts differ at, for a message
  fn first_difference(a: &str, b: &str) -> Option<String> {
    if a == b {
      return None;
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    // a line of JSON (the regions of the .jsonc): the same values, though nlohmann's numbers are
    // not always the shortest (-150.27688598632813 for -150.27688598632812, 4.6e-06 for 4.6e-6)
    fn value_of(line: &str) -> Option<Value> {
      let v = line.trim().trim_end_matches(',');
      let v = v.split_once("\": ").map_or(v, |(_, v)| v);
      serde_json::from_str(v).ok()
    }
    fn close(a: &Value, b: &Value) -> bool {
      match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
          let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
          (x - y).abs() <= 1e-12 * x.abs().max(1.0)
        }
        (Value::Array(x), Value::Array(y)) => {
          x.len() == y.len() && x.iter().zip(y).all(|(x, y)| close(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
          x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w)))
        }
        _ => a == b,
      }
    }
    let same = |x: &&str, y: &&str| {
      x == y || matches!((value_of(x), value_of(y)), (Some(v), Some(w)) if close(&v, &w))
    };
    if a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| same(x, y)) {
      return None;
    }
    let at = a
      .iter()
      .zip(&b)
      .position(|(x, y)| !same(x, y))
      .unwrap_or(a.len().min(b.len()));
    Some(format!(
      "line {}\n  C++:  {}\n  Rust: {}",
      at + 1,
      a.get(at).unwrap_or(&"<end>"),
      b.get(at).unwrap_or(&"<end>")
    ))
  }

  /// The user's projects exported by the C++ editor (library/export/<project>/, made from the
  /// project as saved in library/projects/<project>/<project>.glb): exported again, the same text
  /// files and the same nodes in the .glb.
  #[test]
  fn the_projects_exported_like_the_cpp_editor() {
    let Some(root) = real_library() else {
      return;
    };
    crate::i18n::i18n_init(&crate::i18n::lang_dir(), "en");
    let data = real_data();
    let mut library = AssetLibrary::default();
    library
      .scan(&from_path(&root.join("jak3")))
      .expect("library");
    let tmp = temp_dir("game-export-projects");
    let mut compared = 0;
    for project in ["demo-oasis", "wascitybis"] {
      let (export, file) = (
        root.join("export").join(project),
        root
          .join("projects")
          .join(project)
          .join(format!("{project}.glb")),
      );
      let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
      // the export made from the project as saved
      if !file.is_file() || mtime(&export.join("README.md")) < mtime(&file) {
        continue;
      }
      let mut scene = Scene::new();
      let read = ImportOptions {
        read_settings: true,
        ..Default::default()
      };
      import_gltf(&mut scene, &file, &read).expect("project");
      apply_story(&mut scene, Some(&data), &StoryView::default());
      let mut o = options_of_export(&export, project);
      o.folder = from_path(&tmp.join(project));
      // what App::export_for_game adds: the nav meshes, the levels as extracted, the regions
      (o.navmeshes, o.navmesh_names) = crate::navmesh::world_navmeshes(&scene);
      for level in collision_levels(&scene) {
        if let Some(entry) = library.level(&level) {
          let original = read_original(Path::new(&entry.background)).expect("level");
          o.originals.insert(level, Arc::new(original));
        }
      }
      if let Some(entry) = library.level(&o.replaces) {
        o.region_trees = entry.region_trees.clone();
      }
      scene.begin_frame();
      export_level(&scene, Some(&data), &o).expect("exported");
      let (theirs, ours) = (export_texts(&export), export_texts(&tmp.join(project)));
      assert_eq!(
        theirs.keys().collect::<Vec<_>>(),
        ours.keys().collect::<Vec<_>>(),
        "{project}: the files"
      );
      for (path, text) in &theirs {
        // the editor's name in the comments and the README: renamed since
        let text = text.replace("the OpenGOAL Level Editor", "Jak Level Editor");
        if let Some(d) = first_difference(&text, &ours[path]) {
          panic!("{project}/{path}: {d}");
        }
      }
      // the .glb: the same nodes with the same extras
      let glb = |dir: &Path| {
        glb_extras(&read_glb_json(
          &dir.join(format!("custom_assets/jak3/levels/{project}/{project}.glb")),
        ))
      };
      let (theirs, ours) = (glb(&export), glb(&tmp.join(project)));
      assert_eq!(theirs.len(), ours.len(), "{project}: the .glb's nodes");
      for (name, e) in &theirs {
        assert_eq!(Some(e), ours.get(name), "{project}: the extras of {name}");
      }
      compared += 1;
    }
    eprintln!("projects compared with the C++ editor's exports: {compared}");
  }
}
