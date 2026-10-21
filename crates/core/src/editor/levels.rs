//! The levels of a document (src/app/actions.cpp): a level of the library opened, made ready for
//! editing; the levels added next to it (its neighbors in the game), the levels the game shows with
//! each (companions) and the layers of their missions; a project's file; the clipboard; the story
//! view of all of them.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{Value, json};

use super::groups::{group_tag, new_group_id};
use super::{Editor, tr_err};
use crate::history::NodeChange;
use crate::i18n::tr;
use crate::io::asset_library::{AssetLibrary, LevelEntry, load_model_asset, merge_mesh};
use crate::io::catalog::resolve_actor_model;
use crate::io::game_data::{GameData, actor_kept};
use crate::io::gltf::{ExportMode, ExportOptions, ImportOptions, export_glb, import_gltf};
use crate::math::{AABB, Mat4, Quat, Transform, Vec3, Vec4, dot, length, radians};
use crate::scene::assemblies::{K_ASSEMBLY_MAX_SIZE, assembly_of, find_assemblies};
use crate::scene::pick::covered_below;
use crate::scene::relight::K_LIT_HERE;
use crate::scene::story::{StoryView, apply_story, story_changes};
use crate::scene::{
  Mesh, Node, NodeKind, Scene, TodColors, focus_box, ground_piece, instance_tree, natural_ground,
  piece_floor, split_ground_piece, structure_texture,
};
use crate::{log_error, log_info, log_warn, strf};

fn etype_of(n: &Node) -> String {
  n.extras
    .get("etype")
    .and_then(Value::as_str)
    .unwrap_or_default()
    .to_string()
}

// The game draws water and dark eco as a see-through layer lit by a reflection map, which is not
// drawn here: water becomes a bluish translucent surface, dark eco (a black layer) a dark purple
// one. Lava keeps its own look.
pub(crate) fn style_water_mesh(scene: &mut Scene, mesh: &Mesh, model: &str) {
  if model.contains("lava") {
    return;
  }
  let dark_eco = model.contains("dark-eco");
  let mut done = BTreeSet::new();
  for prim in &mesh.prims {
    let Some(m) = usize::try_from(prim.material)
      .ok()
      .filter(|&i| done.insert(i))
      .and_then(|i| scene.materials.get_mut(i))
    else {
      continue;
    };
    m.alpha = crate::scene::AlphaMode::Blend;
    m.water = true;
    if dark_eco {
      m.texture = -1;
      m.base_color = Vec4::new(0.2, 0.07, 0.3, 0.88);
    } else {
      let c = m.base_color;
      m.base_color = Vec4::new(c.x * 0.55, c.y * 0.8, c.z, c.w.min(0.75));
    }
  }
}

// Draws under each actor of scene.nodes[first..] the model the library has for it (from the
// folder `group` first). Returns the number of actors with one.
fn attach_actor_models(
  scene: &mut Scene,
  library: &AssetLibrary,
  data: Option<&GameData>,
  first: usize,
  group: &str,
) -> usize {
  // model file -> mesh of the document
  let mut meshes: HashMap<String, Option<Arc<Mesh>>> = HashMap::new();
  let actors: Vec<(u32, String, Value)> = scene.nodes[first..]
    .iter()
    .filter(|n| n.kind == NodeKind::Actor)
    .map(|n| {
      let lump = n.extras.get("lump").cloned().unwrap_or(Value::Null);
      (n.id, etype_of(n), lump)
    })
    .collect();
  let mut attached = 0;
  for (id, etype, lump) in actors {
    let Some(model) = resolve_actor_model(library, data, &etype, &lump, group) else {
      continue;
    };
    let (path, name) = (model.path.clone(), model.name.clone());
    let mesh = match meshes.get(&path) {
      Some(m) => m.clone(),
      None => {
        let mesh = match load_model_asset(&path, false) {
          Ok(lm) => Some(merge_mesh(scene, &lm.mesh, &lm.textures, &lm.materials)),
          Err(e) => {
            log_warn!("%s: %s", &path, e);
            None
          }
        };
        if let Some(m) = &mesh
          && data.is_some_and(|d| d.is_water(&etype))
        {
          style_water_mesh(scene, m, &name);
        }
        meshes.insert(path, mesh.clone());
        mesh
      }
    };
    let Some(mesh) = mesh else { continue };
    scene.add(Node {
      parent: id,
      name: name.clone(),
      kind: NodeKind::Render,
      mesh: Some(mesh),
      extras: json!({ "og_actor_visual": name }),
      ..Default::default()
    });
    attached += 1;
  }
  attached
}

// A piece of a level's shell is written in the level's space: its origin goes to the bottom center
// of its geometry, so that it moves, turns and scales around itself (its own mesh only).
fn center_piece_origin(n: &mut Node) {
  if length(n.local.t) != 0.0 {
    return;
  }
  let Some(mesh) = n.mesh.as_mut().and_then(Arc::get_mut) else {
    return;
  };
  let b = mesh.bounds();
  if !b.valid() {
    return;
  }
  let origin = Vec3::new(b.center().x, b.lo.y, b.center().z);
  for prim in &mut mesh.prims {
    for p in &mut prim.pos {
      *p = *p - origin;
    }
  }
  mesh.touch();
  n.local.t = origin;
}

// Removes the nodes of `doomed` and their descendants.
fn remove_subtrees(scene: &mut Scene, mut doomed: HashSet<u32>) {
  if doomed.is_empty() {
    return;
  }
  // parents may come after their children: repeat until nothing is added
  let mut grew = true;
  while grew {
    grew = false;
    for n in &scene.nodes {
      if n.parent != 0 && !doomed.contains(&n.id) && doomed.contains(&n.parent) {
        grew |= doomed.insert(n.id);
      }
    }
  }
  scene.nodes.retain(|n| !doomed.contains(&n.id));
}

// What a level file brings, made ready for editing (scene.nodes[first..]): without its creatures
// and logic actors (without any actor when not `actors`), but with its particle effects (steam,
// sparks, lights: part of its look, exported with it), the terrain locked, a model under each
// actor.
fn prepare_level_nodes(
  scene: &mut Scene,
  library: &AssetLibrary,
  data: Option<&GameData>,
  first: usize,
  group: &str,
  actors: bool,
) {
  if data.is_some() || !actors {
    let doomed = scene.nodes[first..]
      .iter()
      .filter(|n| n.kind == NodeKind::Actor && !actor_kept(data, &etype_of(n), actors))
      .map(|n| n.id)
      .collect();
    remove_subtrees(scene, doomed);
  }
  // where each actor is in the game: the positions inside its data (paths) are the game's, and
  // follow it when it moves (the export moves them by as much)
  for i in first..scene.nodes.len() {
    let n = &scene.nodes[i];
    if n.kind != NodeKind::Actor || n.extras.get("og_origin").is_some() {
      continue;
    }
    let w = Transform::from_matrix(&scene.world(n));
    scene.nodes[i].extras["og_origin"] = json!([w.t.x, w.t.y, w.t.z, w.r.x, w.r.y, w.r.z, w.r.w]);
  }
  // the terrain is locked: clicks go to what is on it. The extractor splits the level's shell
  // (tfrag) in pieces and marks the large ones og_terrain; older extractions have one mesh per
  // texture over the whole level, all terrain.
  let index: HashMap<u32, usize> = scene.nodes[first..]
    .iter()
    .enumerate()
    .map(|(i, n)| (n.id, first + i))
    .collect();
  let mut i = first;
  while i < scene.nodes.len() {
    let n = &scene.nodes[i];
    let (kind, parent) = (n.kind, n.parent);
    i += 1;
    if kind != NodeKind::Render {
      continue;
    }
    let i = i - 1;
    let parent_name = index.get(&parent).map(|&p| scene.nodes[p].name.clone());
    // the distant, low-detail backdrop (mountains beyond the playable area): made to be seen
    // through the game's fog, which the editor has none of; hazed at render time instead
    // (og_backdrop persists through save/reopen and export, on the mesh: it round-trips there)
    if parent_name
      .as_deref()
      .is_some_and(|p| p.contains("-tfrag-lowres"))
      && let Some(mesh) = scene.nodes[i].mesh.as_mut()
      && mesh.extras.get("og_backdrop").is_none()
    {
      Arc::make_mut(mesh).extras["og_backdrop"] = json!(true);
    }
    if let Some(terrain) = scene.nodes[i]
      .extras
      .get("og_terrain")
      .and_then(Value::as_bool)
    {
      // the extractor marks every piece 60 m across; only the ground stays locked (a facade or a
      // door of the shell as large is selected like the rest), the walls the game made one with a
      // ground given a piece of their own (selected like a facade)
      if terrain && level_ground(scene, i) {
        if split_ground_piece(scene, i).is_some() {
          scene.nodes[i].extras["og_split"] = json!(true);
        }
        let g = &mut scene.nodes[i];
        g.locked = g.mesh.as_ref().is_some_and(|m| ground_piece(m));
        if !g.locked {
          center_piece_origin(g);
        }
      } else {
        let n = &mut scene.nodes[i];
        n.locked = false;
        center_piece_origin(n);
      }
      continue;
    }
    if parent_name
      .as_deref()
      .is_some_and(|p| p.contains("-tfrag-"))
    {
      scene.nodes[i].locked = true;
    }
  }
  free_raised_strips(scene, first);
  attach_actor_models(scene, library, data, first, group);
}

// Adds a level of the library under a group telling what it is (a layer, a companion, a level
// added: see scene/story), under `parent`. Returns the group, 0 when there is nothing to add.
fn add_extra_level(
  scene: &mut Scene,
  library: &AssetLibrary,
  data: Option<&GameData>,
  level: &LevelEntry,
  key: &str,
  parent: u32,
  actors: bool,
) -> u32 {
  let id = scene.add(Node {
    name: level.name.clone(),
    kind: NodeKind::Group,
    parent,
    extras: json!({ key: level.name }),
    ..Default::default()
  });
  let first = scene.nodes.len();
  let o = ImportOptions {
    parent: id,
    ..Default::default()
  };
  if let Err(e) = import_gltf(scene, &level.background, &o) {
    log_warn!("%s: %s", &level.background, e);
    scene.remove(&[id]);
    return 0;
  }
  prepare_level_nodes(scene, library, data, first, &level.name, actors);
  // a layer of creatures only brings nothing
  let content = scene.nodes[first..]
    .iter()
    .any(|n| n.kind == NodeKind::Actor || n.kind == NodeKind::Render);
  if !content {
    scene.remove(&[id]);
    return 0;
  }
  id
}

/// A level of Jak II made decor of Jak 3 (its nodes from `first`): without its actors (their code
/// is Jak II's), with its collision (the surfaces of Jak II are written as Jak 3's: the same bits,
/// the same materials and events, Jak 3 has more), its textures and materials renamed apart from
/// Jak 3's (jak2-...), its pieces marked as Jak II's, and its groups named jak2-<level>-...: nothing
/// takes it for a level of Jak 3 of the same name (Jak II has a ctyport, a stadium...).
pub fn make_jak2_level(scene: &mut Scene, first: usize, level: &str) {
  let doomed = scene.nodes[first..]
    .iter()
    .filter(|n| n.kind == NodeKind::Actor)
    .map(|n| n.id)
    .collect();
  remove_subtrees(scene, doomed);
  let prefixed = |s: &str| s.starts_with("jak2-");
  for t in &mut scene.textures {
    if !prefixed(&t.name) {
      t.name = format!("jak2-{}", t.name);
    }
  }
  for m in &mut scene.materials {
    if !prefixed(&m.name) {
      m.name = format!("jak2-{}", m.name);
    }
  }
  let group = format!("{level}-");
  for n in &mut scene.nodes[first..] {
    if n.name.starts_with(&group)
      && (instance_tree(&n.name)
        || n.name.contains("-tfrag-")
        || n.name == format!("{group}collision")
        || n.name == format!("{group}actors"))
    {
      n.name = format!("jak2-{}", n.name);
    }
    if n.kind == NodeKind::Render && n.mesh.is_some() {
      n.extras["og_import"] = json!({"game": "jak2", "level": level});
    }
  }
}

pub(crate) fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
  let mut name = base.to_string();
  let mut i = 2;
  while used.contains(&name) {
    name = format!("{base}-{i}");
    i += 1;
  }
  used.insert(name.clone());
  name
}

// ------------------------------------------------------------------------------------------------
// document operations (no editor needed)
// ------------------------------------------------------------------------------------------------

/// ... and not a long narrow piece standing more than 10 m over the level's ground (under a quarter
/// of the ground's area): a wall's top, a walkway (the farms' walls). Unlocks those of the nodes
/// from `first`.
pub fn free_raised_strips(scene: &mut Scene, first: usize) {
  // the ground's floors, the level's ground under a quarter of their area (the lowest first)
  struct G {
    i: usize,
    floor: f32,
    area: f32,
  }
  let mut grounds = Vec::new();
  for (i, n) in scene.nodes.iter().enumerate().skip(first) {
    let Some(mesh) = &n.mesh else { continue };
    if !n.locked || n.kind != NodeKind::Render || n.extras.get("og_terrain").is_none() {
      continue;
    }
    let (floor, area) = piece_floor(mesh, &scene.world(n));
    grounds.push(G { i, floor, area });
  }
  if grounds.len() < 2 {
    return;
  }
  grounds.sort_by(|a, b| a.floor.total_cmp(&b.floor));
  let total: f64 = grounds.iter().map(|g| g.area as f64).sum();
  let mut acc = 0f64;
  let mut level = grounds[0].floor;
  for g in &grounds {
    level = g.floor;
    acc += g.area as f64;
    if acc >= 0.25 * total {
      break;
    }
  }
  let strips: Vec<usize> = grounds
    .iter()
    .filter(|g| {
      let s = scene.world_bounds(&scene.nodes[g.i]).size();
      let (w, l) = (s.x.min(s.z), s.x.max(s.z));
      w < 12.0 && l >= 8.0 * w && g.floor > level + 10.0
    })
    .map(|g| g.i)
    .collect();
  for i in strips {
    scene.nodes[i].locked = false;
    center_piece_origin(&mut scene.nodes[i]);
  }
}

/// Whether a large piece of a level's shell is its ground (locked): large and flat (ground_piece),
/// not named after a structure's texture (a roof, a bridge, a pillar...), and natural ground (sand,
/// grass, rock...) or with nothing under it (not a roof, a ceiling, debris lying on the ground, a
/// water surface)
pub fn level_ground(scene: &Scene, i: usize) -> bool {
  let n = &scene.nodes[i];
  let Some(mesh) = &n.mesh else { return false };
  if !ground_piece(mesh) || structure_texture(&n.name) {
    return false;
  }
  natural_ground(&n.name) || covered_below(scene, i, 1.0) < 0.5
}

/// A project (.glb saved by the editor) into `scene`.
pub fn open_project(scene: &mut Scene, path: &str) -> Result<(), String> {
  let ext = std::path::Path::new(path)
    .extension()
    .map(|e| e.to_string_lossy().to_lowercase())
    .unwrap_or_default();
  if ext != "glb" && ext != "gltf" {
    return Err("not a .glb file".into());
  }
  let o = ImportOptions {
    read_settings: true,
    ..Default::default()
  };
  import_gltf(scene, path, &o)?;
  // projects saved before the ground alone was locked: their large walls and doors are freed; and
  // before the walls standing on a ground had a piece of their own: given one, as a level opened
  let mut i = 0;
  while i < scene.nodes.len() {
    let n = &scene.nodes[i];
    i += 1;
    let Some(mesh) = &n.mesh else { continue };
    if !n.locked || n.kind != NodeKind::Render || n.extras.get("og_terrain").is_none() {
      continue;
    }
    let i = i - 1;
    let split = n.extras.get("og_split").is_some();
    if !ground_piece(mesh) || (!split && !level_ground(scene, i)) {
      let n = &mut scene.nodes[i];
      n.locked = false;
      center_piece_origin(n);
      continue;
    }
    if split || split_ground_piece(scene, i).is_none() {
      continue;
    }
    let g = &mut scene.nodes[i];
    g.extras["og_split"] = json!(true);
    g.locked = g.mesh.as_ref().is_some_and(|m| ground_piece(m));
  }
  free_raised_strips(scene, 0);
  Ok(())
}

/// Writes the document as a project .glb.
pub fn save_project(scene: &Scene, path: &str) -> Result<(), String> {
  let o = ExportOptions {
    mode: ExportMode::Project,
    ..Default::default()
  };
  // write to a temporary file first: a crash while saving must not destroy the project
  let tmp = format!("{path}.tmp");
  export_glb(scene, &tmp, &o)?;
  if std::fs::rename(&tmp, path).is_err() {
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path).map_err(|_| format!("cannot replace {path}"))?;
  }
  Ok(())
}

/// A level of the library: its decor, its actors with the model the library has for each, and its
/// collision (kept, not shown). The terrain is locked: clicks go to the objects on it. With the
/// game's data: without the creatures and logic actors, the water actors with the model of their
/// look, and the levels the game shows with it (companions, the layers the story lends it: see
/// scene/story). Without `actors`, the level comes without any actor (its objects too): its decor
/// alone.
pub fn open_level(
  scene: &mut Scene,
  library: &AssetLibrary,
  data: Option<&GameData>,
  level: &LevelEntry,
  actors: bool,
) -> Result<(), String> {
  import_gltf(scene, &level.background, &ImportOptions::default())?;
  // the game's name of the level (a copy found in another DGO has its folder named after both)
  scene.settings.level = if level.level.is_empty() {
    level.name.clone()
  } else {
    level.level.clone()
  };
  scene.settings.source_group = level.name.clone();
  prepare_level_nodes(scene, library, data, 0, &level.name, actors);
  let Some(data) = data else { return Ok(()) };
  // the levels the game shows with it, and the layers the story lends it
  let name = scene.settings.level.clone();
  for c in data
    .level(&name)
    .map(|i| i.companions.clone())
    .unwrap_or_default()
  {
    if let Some(c) = library.level(&c) {
      add_extra_level(scene, library, Some(data), c, "og_companion", 0, actors);
    }
  }
  for l in data.layers_of(&name) {
    if let Some(l) = library.level(&l) {
      add_extra_level(scene, library, Some(data), l, "og_layer", 0, actors);
    }
  }
  Ok(())
}

/// Adds a level of the library to a document, at its place in the game's world (the levels' files
/// share the game's coordinates, so levels side by side in the game meet here too): under a group
/// (extras.og_level), with the levels the game shows with it and the layers of its missions under
/// that group. Returns the group.
pub fn add_level(
  scene: &mut Scene,
  library: &AssetLibrary,
  data: Option<&GameData>,
  level: &LevelEntry,
  actors: bool,
) -> Result<u32, String> {
  let name = if level.level.is_empty() {
    &level.name
  } else {
    &level.level
  };
  let group = add_extra_level(scene, library, data, level, "og_level", 0, actors);
  if group == 0 {
    return Err(format!("cannot read {}", level.background));
  }
  let Some(data) = data else { return Ok(group) };
  for c in data
    .level(name)
    .map(|i| i.companions.clone())
    .unwrap_or_default()
  {
    if let Some(l) = library.level(&c) {
      add_extra_level(scene, library, Some(data), l, "og_companion", group, actors);
    }
  }
  for layer in data.layers_of(name) {
    if let Some(l) = library.level(&layer) {
      add_extra_level(scene, library, Some(data), l, "og_layer", group, actors);
    }
  }
  Ok(group)
}

/// Nodes copied with Copy, pasted with Paste: the copied roots with their descendants.
#[derive(Clone, Debug, Default)]
pub struct Clipboard {
  pub nodes: Vec<Node>,
  pub roots: Vec<u32>,
  /// world matrix of each root when it was copied
  pub root_worlds: BTreeMap<u32, Mat4>,
}

impl Clipboard {
  pub fn is_empty(&self) -> bool {
    self.roots.is_empty()
  }
}

pub fn copy_nodes(scene: &Scene, roots: &[u32]) -> Clipboard {
  let mut clip = Clipboard::default();
  let below = scene.descendants_of(roots);
  for (r, &root) in roots.iter().enumerate() {
    let Some(n) = scene.find(root) else { continue };
    clip.roots.push(root);
    clip.root_worlds.insert(root, scene.world(n));
    clip.nodes.push(n.clone());
    clip
      .nodes
      .extend(below[r].iter().filter_map(|&d| scene.find(d).cloned()));
  }
  clip
}

/// Adds copies of the clipboard's nodes (under their old parent when it is still there, at the
/// same place). Returns the new roots.
pub fn paste_nodes(scene: &mut Scene, clip: &Clipboard) -> Vec<u32> {
  let mut ids = HashMap::new();
  for n in &clip.nodes {
    ids.insert(n.id, scene.next_id);
    scene.next_id += 1;
  }
  let mut names: HashSet<String> = scene.nodes.iter().map(|n| n.name.clone()).collect();
  // a copy of a group is a group of its own
  let mut groups: HashMap<u32, u32> = HashMap::new();
  let mut next_group = new_group_id(scene);
  let mut roots = Vec::new();
  for src in &clip.nodes {
    let mut n = src.clone();
    n.id = ids[&src.id];
    let g = group_tag(src);
    if g != 0 {
      let id = *groups.entry(g).or_insert_with(|| {
        next_group += 1;
        next_group - 1
      });
      n.extras["og_group"]["id"] = json!(id);
    }
    let root_world = clip.root_worlds.get(&src.id);
    if let Some(w) = root_world {
      // under its old parent when it is still in the document
      n.parent = if scene.find(src.parent).is_some() {
        src.parent
      } else {
        0
      };
      n.name = unique_name(&src.name, &mut names);
      let parent = if n.parent != 0 {
        scene.world_of(n.parent)
      } else {
        Mat4::identity()
      };
      n.local = Transform::from_matrix(&(parent.inverse() * *w));
    } else {
      n.parent = ids.get(&src.parent).copied().unwrap_or(0);
    }
    if n.kind == NodeKind::Render && n.mesh.is_some() {
      n.extras[K_LIT_HERE] = json!(true);
    }
    if n.kind == NodeKind::Actor {
      // a copied actor is a new one: the game gives it its id, and names it like the others of
      // its type (<type>-<number>)
      if let Some(o) = n.extras.as_object_mut() {
        o.remove("aid");
      }
      let etype = etype_of(&n);
      if root_world.is_some() && !etype.is_empty() {
        names.remove(&n.name);
        let mut i = 1;
        while names.contains(&format!("{etype}-{i}")) {
          i += 1;
        }
        n.name = format!("{etype}-{i}");
        names.insert(n.name.clone());
      }
      let name = n.name.clone();
      if let Some(lump) = n.extras.get_mut("lump").and_then(Value::as_object_mut) {
        lump.insert("name".into(), json!(name));
      }
    }
    let id = scene.add(n);
    if root_world.is_some() {
      roots.push(id);
    }
  }
  roots
}

// ------------------------------------------------------------------------------------------------
// several levels in a document
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LevelRole {
  #[default]
  Main,
  Added,
  Companion,
  Layer,
}

/// A level of the document: the level opened, the levels added (neighbors), the companions and
/// layers of each
#[derive(Clone, Debug, Default, Serialize)]
pub struct DocumentLevel {
  pub name: String,
  pub role: LevelRole,
  /// its group; 0 for the level opened
  pub group: u32,
  /// companions and layers: the group of the level they come with, 0 the one opened
  pub host: u32,
  /// Added: shown to compare with (the game's level a new one replaces)
  pub reference: bool,
}

/// The levels read on a worker thread, and what the document's levels were last found to be.
#[derive(Default)]
pub(crate) struct LevelJobs {
  /// the level being opened
  pub opening: Option<String>,
  /// the level being added, and whether as a reference
  pub adding: Option<(String, bool)>,
  /// why the last level could not be added, "" when it was
  pub add_error: String,
  /// counts the additions finished (the remote waits on it)
  pub levels_added: u64,
  cache: Mutex<Option<((u64, u64, usize), Arc<Vec<DocumentLevel>>)>>,
}

impl Editor {
  /// Opens a level of the library (of Jak II's: its decor made Jak 3's) on a worker thread; the
  /// document is replaced when it is read.
  pub fn load_level(&mut self, name: &str, jak2: bool) -> Result<(), String> {
    if self.levels.opening.is_some() {
      log_warn!("%s", tr("log.level_busy"));
      return Err(tr("log.level_busy").to_string());
    }
    let lib = if jak2 {
      self.library_jak2.clone()
    } else {
      self.library.clone()
    };
    let Some(entry) = lib.level(name).cloned() else {
      return Err(format!("no level '{name}' in the library"));
    };
    self.stop_placement();
    let shown = if jak2 {
      format!("jak2-{name}")
    } else {
      name.to_string()
    };
    self.levels.opening = Some(shown.clone());
    // a level of the game: seen as the game draws it
    self.view.game_view = self.view.game_view_ready;
    let data = if jak2 { None } else { self.game_data() };
    let actors = self.load_actors && !jak2;
    log_info!(tr("log.level_opening"), name);
    let label = strf!(tr("log.level_opening"), name);
    self.run_job(
      "open_level",
      label,
      move || {
        let mut s = Scene::new();
        if let Err(e) = open_level(&mut s, &lib, data.as_deref(), &entry, actors) {
          log_error!("%s: %s", &entry.background, e);
          return None;
        }
        if jak2 {
          // Jak II's: its decor, made Jak 3's, a level of its own
          make_jak2_level(&mut s, 0, &entry.name);
          s.settings.level = format!("jak2-{}", entry.name);
          s.settings.source_group = s.settings.level.clone();
        }
        Some(s)
      },
      |ed, s| ed.level_opened(s),
    );
    Ok(())
  }

  // poll_level_opening
  fn level_opened(&mut self, loaded: Option<Scene>) {
    let name = self.levels.opening.take().unwrap_or_default();
    let Some(loaded) = loaded else {
      log_error!(tr("log.level_failed"), &name);
      return;
    };
    self.new_document();
    self.scene = loaded;
    self.history.clear();
    self.saved_revision = self.history.revision();
    self.story_document_changed();
    let actors = self
      .scene
      .nodes
      .iter()
      .filter(|n| n.kind == NodeKind::Actor)
      .count();
    log_info!(tr("log.level_opened"), &name, actors);
  }

  /// The document's levels (found again when the document changes)
  pub fn document_levels(&self) -> Arc<Vec<DocumentLevel>> {
    let key = (
      self.history.revision(),
      self.scene_epoch,
      self.scene.nodes.len(),
    );
    let mut cache = self.levels.cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, levels)) = cache.as_ref()
      && *k == key
    {
      return levels.clone();
    }
    let levels = Arc::new(self.find_document_levels());
    *cache = Some((key, levels.clone()));
    levels
  }

  fn find_document_levels(&self) -> Vec<DocumentLevel> {
    let mut out = Vec::new();
    if !self.document_open() {
      return out;
    }
    out.push(DocumentLevel {
      name: self.scene.settings.level.clone(),
      ..Default::default()
    });
    let by_id: HashMap<u32, &Node> = self.scene.nodes.iter().map(|n| (n.id, n)).collect();
    let host_of = |n: &Node| -> u32 {
      let mut p = by_id.get(&n.parent);
      let mut guard = 0;
      while let Some(node) = p {
        if node.extras.get("og_level").is_some() {
          return node.id;
        }
        guard += 1;
        if guard > 1000 {
          break;
        }
        p = by_id.get(&node.parent);
      }
      0
    };
    for n in &self.scene.nodes {
      if n.kind != NodeKind::Group {
        continue;
      }
      for (key, role) in [
        ("og_level", LevelRole::Added),
        ("og_companion", LevelRole::Companion),
        ("og_layer", LevelRole::Layer),
      ] {
        let Some(name) = n.extras.get(key).and_then(Value::as_str) else {
          continue;
        };
        out.push(DocumentLevel {
          name: name.to_string(),
          role,
          group: n.id,
          host: if role == LevelRole::Added {
            0
          } else {
            host_of(n)
          },
          reference: role == LevelRole::Added
            && n
              .extras
              .get("og_reference")
              .and_then(Value::as_bool)
              .unwrap_or(false),
        });
      }
    }
    out
  }

  pub fn level_in_document(&self, name: &str) -> bool {
    self
      .document_levels()
      .iter()
      .any(|l| l.name == name && l.role != LevelRole::Layer && !l.reference)
  }

  /// A level of the library added on a worker thread, then to the document (undoable) when ready;
  /// `reference`: shown to compare with, washed in blue, not selected nor exported (the level the
  /// document replaces, as the game has it)
  pub fn add_level_to_document(
    &mut self,
    level: &str,
    reference: bool,
    jak2: bool,
  ) -> Result<(), String> {
    let fail = |key: &str| Err(tr_err(key, level));
    if !self.document_open() {
      return fail("levels.add.no_document");
    }
    if self.levels.adding.is_some() || self.levels.opening.is_some() {
      return fail("levels.add.busy");
    }
    let lib = if jak2 {
      self.library_jak2.clone()
    } else {
      self.library.clone()
    };
    let Some(entry) = lib.level(level).cloned() else {
      return Err(format!("no level '{level}' in the library"));
    };
    // a level is in the document once; the reference of the level replaced may join a document
    // made from that very level
    let name = if jak2 {
      format!("jak2-{level}")
    } else {
      level.to_string()
    };
    let taken = self.document_levels().iter().any(|l| {
      l.name == name
        && l.role != LevelRole::Layer
        && if reference { l.reference } else { !l.reference }
    });
    if taken || (!reference && self.level_in_document(&name)) {
      return fail("levels.add.already");
    }
    self.stop_placement();
    self.levels.adding = Some((name.clone(), reference));
    let data = if jak2 { None } else { self.game_data() };
    let actors = self.load_actors && !jak2;
    log_info!(tr("log.level_adding"), level);
    let label = strf!(tr("log.level_adding"), level);
    self.run_job(
      "add_level",
      label,
      move || {
        let mut s = Scene::new();
        let group = match add_level(&mut s, &lib, data.as_deref(), &entry, actors) {
          Ok(g) => g,
          Err(e) => {
            log_error!("%s", e);
            return None;
          }
        };
        if jak2 {
          make_jak2_level(&mut s, 0, &entry.name);
          let g = s.find_mut(group).unwrap();
          g.name = name.clone();
          g.extras["og_level"] = json!(name);
        }
        if reference {
          s.find_mut(group).unwrap().extras["og_reference"] = json!(true);
        }
        Some(s)
      },
      |ed, s| ed.level_added(s),
    );
    Ok(())
  }

  // poll_level_adding
  fn level_added(&mut self, loaded: Option<Scene>) {
    let name = self.levels.adding.take().map(|a| a.0).unwrap_or_default();
    self.levels.levels_added += 1;
    let Some(loaded) = loaded else {
      self.levels.add_error = strf!(tr("log.level_add_failed"), &name);
      log_error!("%s", &self.levels.add_error);
      return;
    };
    self.levels.add_error.clear();
    let elements = loaded
      .nodes
      .iter()
      .filter(|n| n.kind == NodeKind::Render || n.kind == NodeKind::Actor)
      .count();
    let change = NodeChange::new(&self.scene);
    self.scene.append(loaded);
    self
      .history
      .push(change.commit(&self.scene, strf!(tr("undo.add_level"), &name), None));
    self.story_document_changed();
    self.frame_box(self.scene.focus_bounds());
    log_info!(tr("log.level_added"), &name, elements);
  }

  pub fn remove_document_level(&mut self, group: u32) -> Result<(), String> {
    let Some(g) = self
      .scene
      .find(group)
      .filter(|g| g.extras.get("og_level").is_some())
    else {
      return Err(tr("levels.remove.not_added").to_string());
    };
    let name = g.name.clone();
    let mut change = NodeChange::new(&self.scene);
    change.save_subtree(&self.scene, group);
    let gone = self.scene.descendants(group);
    self.scene.remove(&[group]);
    self
      .history
      .push(change.commit(&self.scene, strf!(tr("undo.remove_level"), &name), None));
    for id in gone {
      self.selection.remove(&id);
    }
    self.selection.remove(&group);
    self.fix_primary();
    self.story_document_changed();
    log_info!(tr("log.level_removed"), &name);
    Ok(())
  }

  /// Where most of a level of the document is (0: the level opened, its collision left out)
  pub fn document_level_bounds(&self, group: u32) -> AABB {
    let cache = self.scene.cache();
    let nodes = &self.scene.nodes;
    let index: HashMap<u32, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    // the level each node belongs to: the first level group above it (companions and layers go
    // with their own group), else the level opened
    let level_of = |i: usize| -> u32 {
      let mut cur = i;
      for _ in 0..1000 {
        let n = &nodes[cur];
        if n.kind == NodeKind::Group
          && ["og_level", "og_companion", "og_layer"]
            .iter()
            .any(|k| n.extras.get(*k).is_some())
        {
          return n.id;
        }
        match index.get(&n.parent) {
          Some(&p) if n.parent != 0 => cur = p,
          _ => return 0,
        }
      }
      0
    };
    let boxes: Vec<AABB> = (0..nodes.len().min(cache.count))
      .filter(|&i| nodes[i].kind == NodeKind::Render && cache.bounds[i].valid())
      .filter(|&i| level_of(i) == group)
      .map(|i| cache.bounds[i])
      .collect();
    // where most of the level is: some levels hold a piece far away from the rest
    focus_box(&boxes)
  }

  /// A whole level, to move it: an added level's group, or the root elements of the level opened
  /// (with its companions and layers, without the levels added nor the nav meshes)
  pub fn document_level_roots(&self, group: u32) -> Vec<u32> {
    if group != 0 {
      return self
        .scene
        .find(group)
        .map(|_| vec![group])
        .unwrap_or_default();
    }
    self
      .scene
      .nodes
      .iter()
      .filter(|n| {
        n.parent == 0 && n.kind != NodeKind::NavMesh && n.extras.get("og_level").is_none()
      })
      .map(|n| n.id)
      .collect()
  }

  /// the root elements of the document's own level
  pub fn own_level_roots(&self) -> Vec<u32> {
    self.document_level_roots(0)
  }

  pub fn own_level_bounds(&self) -> AABB {
    self.document_level_bounds(0)
  }

  /// Replacing a level of the game (scene.settings.replaces): the document's own level moved so
  /// that its middle and its bottom are where the replaced level's are (the reference shown)
  pub fn align_to_replaced(&mut self) -> Result<(), String> {
    let level = self.scene.settings.replaces.clone();
    let fail = |key: &str| Err(tr_err(key, &level));
    if level.is_empty() {
      return fail("replace.none");
    }
    // where the level replaced is: the reference shown (the game's level as it is)
    let mut target = AABB::default();
    for l in self.document_levels().iter() {
      if l.reference && l.name == level {
        target = self.document_level_bounds(l.group);
      }
    }
    if !target.valid() {
      return fail("replace.show_first");
    }
    let own = self.own_level_bounds();
    let roots = self.own_level_roots();
    if !own.valid() || roots.is_empty() {
      return fail("replace.empty");
    }
    let offset = Vec3::new(
      target.center().x - own.center().x,
      target.lo.y - own.lo.y,
      target.center().z - own.center().z,
    );
    self.move_roots(
      &roots,
      Mat4::translate(offset),
      strf!(tr("undo.align_replaced"), &level),
    );
    log_info!(
      tr("log.aligned_replaced"),
      &level,
      offset.x,
      offset.y,
      offset.z
    );
    Ok(())
  }

  /// The document's own level turned around its middle
  pub fn turn_own_level(&mut self, degrees: f32) {
    let own = self.own_level_bounds();
    let roots = self.own_level_roots();
    if !own.valid() || roots.is_empty() {
      return;
    }
    let c = own.center();
    let turn = Mat4::translate(c)
      * Mat4::rotate(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), radians(degrees)))
      * Mat4::translate(-c);
    self.move_roots(&roots, turn, tr("undo.turn_level").to_string());
  }

  // each of `roots` moved by `m` in the world, one undo step
  fn move_roots(&mut self, roots: &[u32], m: Mat4, label: String) {
    let mut change = NodeChange::new(&self.scene);
    for &id in roots {
      change.save(&self.scene, id);
      let w = self.scene.world_of(id);
      self.scene.set_world(id, &(m * w));
    }
    self.history.push(change.commit(&self.scene, label, None));
  }

  /// the added level's group the node is in, 0 for the one opened
  pub fn level_of_node(&self, id: u32) -> u32 {
    let mut n = self.scene.find(id);
    let mut guard = 0;
    while let Some(node) = n {
      if node.kind == NodeKind::Group && node.extras.get("og_level").is_some() {
        return node.id;
      }
      guard += 1;
      if node.parent == 0 || guard > 1000 {
        break;
      }
      n = self.scene.find(node.parent);
    }
    0
  }

  /// roughly: within the camera's field of view and not past its far plane (a cone, not the exact
  /// frustum: good enough to decide whether a point needs framing to be seen)
  pub fn camera_sees(&self, p: Vec3) -> bool {
    let to = p - self.camera.pos;
    let dist = length(to);
    if dist < 1e-3 {
      return true;
    }
    if dist > super::CameraState::ZFAR {
      return false;
    }
    // a bit tighter than the actual field of view: near the edge, "visible" is not "easy to grab"
    let cos_fov = (radians(self.camera.fov) * 0.5 * 0.85).cos();
    dot(to * (1.0 / dist), self.camera.forward()) > cos_fov
  }

  /// The viewport frames a box (jle:request), keeping its orientation.
  pub(crate) fn frame_box(&mut self, b: AABB) {
    if b.valid() {
      self.request(json!({"kind": "camera", "frame": {"lo": b.lo, "hi": b.hi}}));
    }
  }

  /// A whole level selected (Ctrl+L): its group, or the roots of the level opened
  pub fn select_level(&mut self, group: u32) {
    let roots = self.document_level_roots(group);
    self.selection = roots.iter().copied().collect();
    self.primary = roots.first().copied().unwrap_or(0);
    if roots.is_empty() {
      return;
    }
    let name = match self.scene.find(group).filter(|_| group != 0) {
      Some(g) => g.name.clone(),
      None => self.scene.settings.level.clone(),
    };
    log_info!(tr("log.level_selected"), &name, roots.len());
    // its gizmo stands in the middle of the level: without this, it can land off-screen (a level
    // is large, and the camera was looking at only part of it, or away from it)
    let b = self.document_level_bounds(group);
    if b.valid() && !self.camera_sees(b.center()) {
      self.frame_box(b);
    }
  }

  /// the assembly the element is part of, all of it (scene/assemblies): a house with its facades,
  /// roof and windows
  pub fn select_assembly(&mut self, id: u32) {
    let mut members = assembly_of(&find_assemblies(&mut self.scene, K_ASSEMBLY_MAX_SIZE), id);
    if members.is_empty() {
      members = vec![id]; // the ground, a detail of it, an actor: itself
    }
    self.selection = members.iter().copied().collect();
    self.primary = id;
    log_info!(tr("log.assembly_selected"), members.len());
    let cache = self.scene.cache();
    let mut b = AABB::default();
    for &m in &members {
      if let Some(i) = self.scene.index_of(m) {
        b.add_box(&cache.bounds[i]);
      }
    }
    if b.valid() && !self.camera_sees(b.center()) {
      self.frame_box(b);
    }
  }

  /// where two levels of the document meet: the overlap of their boxes (a few meters around the
  /// seam), invalid when they do not touch
  pub fn document_junction(&self, group_a: u32, group_b: u32) -> AABB {
    let a = self.document_level_bounds(group_a).expanded(8.0);
    let b = self.document_level_bounds(group_b).expanded(8.0);
    let mut out = AABB::default();
    if !a.valid() || !b.valid() {
      return out;
    }
    let lo = crate::math::vmax(a.lo, b.lo);
    let hi = crate::math::vmin(a.hi, b.hi);
    if lo.x > hi.x || lo.y > hi.y || lo.z > hi.z {
      return out;
    }
    out.add(lo);
    out.add(hi);
    out
  }

  /// the levels the game displays with those of the document (its data), in the library and not
  /// in the document yet
  pub fn document_neighbors(&mut self) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(data) = self.document_game_data() else {
      return out;
    };
    let levels = self.document_levels();
    for l in levels.iter().filter(|l| l.role != LevelRole::Layer) {
      let Some(info) = data.level(&l.name) else {
        continue;
      };
      for n in &info.neighbors {
        if !self.library.level(n).is_some_and(|e| e.listed) || out.contains(n) {
          continue;
        }
        if !levels
          .iter()
          .any(|d| &d.name == n && d.role != LevelRole::Layer)
        {
          out.push(n.clone());
        }
      }
    }
    out
  }

  /// The game's lighting (the 8 palettes of each vertex) of the levels' decor that misses it (a
  /// document made before the extraction kept it): from the levels as extracted, piece by piece
  /// where its mesh is the same. How many pieces got it back.
  pub fn restore_level_lighting(&mut self) -> usize {
    // a piece's place in its level: the names from its draw group (<level>-tfrag-*, -tie-*,
    // -shrub-*) down to it, and that level
    fn place(s: &Scene, n: &Node) -> Option<(String, String)> {
      let mut path = n.name.clone();
      let mut p = s.find(n.parent).filter(|_| n.parent != 0);
      while let Some(node) = p {
        path = format!("{}/{path}", node.name);
        for group in ["-tfrag-", "-tie-", "-shrub-"] {
          if let Some(at) = node.name.rfind(group)
            && at > 0
            && (group == "-tfrag-" || instance_tree(&node.name))
          {
            return Some((path, node.name[..at].to_string()));
          }
        }
        p = s.find(node.parent).filter(|_| node.parent != 0);
      }
      None
    }
    // level -> the pieces without their lighting
    let mut missing: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, n) in self.scene.nodes.iter().enumerate() {
      let Some(mesh) = &n.mesh else { continue };
      if n.kind != NodeKind::Render || n.tod.as_ref().is_some_and(|t| t.fits(mesh)) {
        continue;
      }
      if let Some((_, level)) = place(&self.scene, n) {
        missing.entry(level).or_default().push(i);
      }
    }
    let mut restored = 0;
    for (level, pieces) in missing {
      let Some(entry) = self.library.level(&level) else {
        continue;
      };
      let mut original = Scene::new();
      if let Err(e) = import_gltf(&mut original, &entry.background, &ImportOptions::default()) {
        log_warn!("%s: %s", &entry.background, e);
        continue;
      }
      let mut by_place: HashMap<String, (Arc<TodColors>, usize)> = HashMap::new();
      for (i, n) in original.nodes.iter().enumerate() {
        if let Some(tod) = &n.tod
          && let Some((path, _)) = place(&original, n)
        {
          by_place.entry(path).or_insert((tod.clone(), i));
        }
      }
      let mut materials_done = BTreeSet::new();
      for i in pieces {
        let Some((path, _)) = place(&self.scene, &self.scene.nodes[i]) else {
          continue;
        };
        let Some((tod, o)) = by_place.get(&path) else {
          continue;
        };
        let n = &self.scene.nodes[i];
        let mesh = n.mesh.clone().unwrap();
        if !tod.fits(&mesh) {
          continue;
        }
        self.scene.nodes[i].tod = Some(tod.clone());
        restored += 1;
        // and how the game draws its materials, as extracted now (a wall blended with its depth
        // written, a see-through surface), each material once
        let Some(omesh) = original.nodes[*o].mesh.clone() else {
          continue;
        };
        for (p, op) in mesh.prims.iter().zip(&omesh.prims) {
          let (dm, om) = (p.material, op.material);
          if dm < 0
            || dm as usize >= self.scene.materials.len()
            || om < 0
            || om as usize >= original.materials.len()
            || !materials_done.insert(dm)
          {
            continue;
          }
          let was = &original.materials[om as usize];
          let d = &mut self.scene.materials[dm as usize];
          if d.name != was.name || d.blend != was.blend {
            continue;
          }
          d.translucent = was.translucent;
          if d.alpha == crate::scene::AlphaMode::Blend && was.alpha == crate::scene::AlphaMode::Mask
          {
            d.alpha = crate::scene::AlphaMode::Mask;
          }
        }
      }
    }
    if restored > 0 {
      self.scene_changed();
    }
    restored
  }

  /// The particle effects of the document's levels as the game has them (steam, sparks, lights)
  /// added back where the document has none (a project made before the editor kept them, effects
  /// deleted); not those whose decor was taken out of the level. Undoable. How many were added,
  /// and how many were left out.
  pub fn restore_level_effects(&mut self) -> (usize, usize) {
    let mut left_out = 0;
    let Some(data) = self.document_game_data() else {
      return (0, 0);
    };
    // the draw groups of an extracted level: <level>-tfrag-*, -tie-*, -shrub-*
    fn in_decor(s: &Scene, n: &Node) -> bool {
      let mut p = s.find(n.parent).filter(|_| n.parent != 0);
      while let Some(node) = p {
        if ["-tfrag-", "-tie-", "-shrub-"]
          .iter()
          .any(|g| node.name.contains(g))
        {
          return true;
        }
        p = s.find(node.parent).filter(|_| node.parent != 0);
      }
      false
    }
    let change = NodeChange::new(&self.scene);
    let mut added = 0;
    let levels = self.document_levels();
    for level in levels.iter() {
      if level.reference || !matches!(level.role, LevelRole::Main | LevelRole::Added) {
        continue;
      }
      let Some(entry) = self.library.level(&level.name) else {
        continue;
      };
      let mut original = Scene::new();
      if let Err(e) = import_gltf(&mut original, &entry.background, &ImportOptions::default()) {
        log_warn!("%s: %s", &entry.background, e);
        continue;
      }
      // what the document has of that level: its actors by name, its decor where it is
      let mut actors = HashSet::new();
      let mut decor: HashMap<String, AABB> = HashMap::new();
      let mut actors_group = 0;
      for n in &self.scene.nodes {
        if self.level_of_node(n.id) != level.group {
          continue;
        }
        if n.kind == NodeKind::Actor {
          actors.insert(n.name.clone());
        } else if n.kind == NodeKind::Render && n.mesh.is_some() && in_decor(&self.scene, n) {
          decor.insert(n.name.clone(), self.scene.world_bounds(n));
        } else if n.kind == NodeKind::Group && n.name == format!("{}-actors", level.name) {
          actors_group = n.id;
        }
      }
      if actors_group == 0 {
        actors_group = level.group;
      }
      // the decor of the level as the game has it, and whether the document still has each piece
      let mut pieces: Vec<(AABB, bool)> = Vec::new();
      for n in &original.nodes {
        if n.kind != NodeKind::Render || n.mesh.is_none() || !in_decor(&original, n) {
          continue;
        }
        let was = original.world_bounds(n);
        let kept = decor
          .get(&n.name)
          .is_some_and(|b| length(b.lo - was.lo) < 0.05 && length(b.hi - was.hi) < 0.05);
        pieces.push((was, kept));
      }
      let into = if actors_group != 0 {
        self.scene.world_of(actors_group).inverse()
      } else {
        Mat4::identity()
      };
      for n in &original.nodes {
        if n.kind != NodeKind::Actor || actors.contains(&n.name) || !data.is_effect(&etype_of(n)) {
          continue;
        }
        let world = original.world(n);
        let at = world.point(Vec3::default());
        // the decor it comes out of (within 1.5 m): taken out of the level, the effect goes with it
        let near: Vec<bool> = pieces
          .iter()
          .filter(|p| p.0.expanded(1.5).contains(at))
          .map(|p| p.1)
          .collect();
        if !near.is_empty() && !near.contains(&true) {
          left_out += 1;
          continue;
        }
        let mut a = n.clone();
        a.id = 0;
        a.parent = actors_group;
        a.local = Transform::from_matrix(&(into * world));
        let w = Transform::from_matrix(&world);
        a.extras["og_origin"] = json!([w.t.x, w.t.y, w.t.z, w.r.x, w.r.y, w.r.z, w.r.w]);
        self.scene.add(a);
        added += 1;
      }
    }
    if added > 0 {
      self
        .history
        .push(change.commit(&self.scene, strf!(tr("undo.restore_effects"), added), None));
      self.story_document_changed();
    }
    (added, left_out)
  }

  // ----------------------------------------------------------------------------------------------
  // the story view
  // ----------------------------------------------------------------------------------------------

  /// the states of a level: 1, then each step where it changes ("" the level opened)
  pub fn story_states(&self, level: &str) -> Vec<i32> {
    let mut out = vec![1];
    let changes = if level.is_empty() {
      Some(&self.story_steps)
    } else {
      self.level_story_steps.get(level)
    };
    if let Some(c) = changes {
      out.extend(c);
    }
    out
  }

  /// the story view: after a new document, after the view or the document's levels changed
  pub fn story_document_changed(&mut self) {
    let data = self.document_game_data();
    self.story_steps = data
      .as_ref()
      .map(|d| story_changes(&self.scene, d, ""))
      .unwrap_or_default();
    // each level added changes at its own steps
    self.level_story_steps.clear();
    if let Some(d) = &data {
      for l in self.document_levels().iter() {
        if l.role == LevelRole::Added {
          let steps = story_changes(&self.scene, d, &l.name);
          self.level_story_steps.insert(l.name.clone(), steps);
        }
      }
    }
    if !self.story_level.is_empty() && !self.level_story_steps.contains_key(&self.story_level) {
      self.story_level.clear();
    }
    if let Some(d) = data.filter(|d| d.has_story()) {
      self.story.step = self.story.step.clamp(1, d.step_count());
    }
    self.apply_story_view();
    self.update_ocean();
  }

  pub fn set_story_view(&mut self, view: StoryView) {
    self.story = view;
    if let Some(d) = self.document_game_data().filter(|d| d.has_story()) {
      self.story.step = self.story.step.clamp(1, d.step_count());
    }
    self.apply_story_view();
    self.update_ocean();
  }

  pub fn apply_story_view(&mut self) {
    let data = self.document_game_data();
    apply_story(&mut self.scene, data.as_deref(), &self.story);
    self.story_revision = self.history.revision();
    self.scene_changed();
    // what the story hides cannot stay selected
    let hidden: Vec<u32> = self
      .selection
      .iter()
      .copied()
      .filter(|&id| {
        !self
          .scene
          .find(id)
          .is_some_and(|n| self.scene.effectively_visible(n))
      })
      .collect();
    for id in hidden {
      self.selection.remove(&id);
    }
    self.fix_primary();
  }

  /// the story bar's level ("" the level opened)
  pub fn story_level(&self) -> &str {
    &self.story_level
  }

  pub fn set_story_level(&mut self, level: &str) {
    if level.is_empty() || self.level_story_steps.contains_key(level) {
      self.story_level = level.to_string();
    }
  }

  /// The story view the story bar's state `state` (1: the start) of the level it shows gives.
  pub fn story_view_at_state(&self, state: i32) -> StoryView {
    let states = self.story_states(&self.story_level);
    let to = states[(state.clamp(1, states.len() as i32) - 1) as usize];
    let mut v = self.story.clone();
    if self.story_level.is_empty() {
      v.step = to;
    } else {
      v.level_steps.insert(self.story_level.clone(), to);
    }
    v
  }

  pub fn story_view(&self) -> &StoryView {
    &self.story
  }
}

/// A StoryView from the call's arguments over `base`
pub(crate) fn story_view_from(base: &StoryView, args: &Value) -> StoryView {
  let mut v = base.clone();
  if let Some(e) = args.get("enabled").and_then(Value::as_bool) {
    v.enabled = e;
  }
  if let Some(s) = args.get("step").and_then(Value::as_i64) {
    v.step = s as i32;
  }
  if let Some(steps) = args.get("level_steps").and_then(Value::as_object) {
    for (k, s) in steps {
      match s.as_i64() {
        Some(s) => v.level_steps.insert(k.clone(), s as i32),
        None => v.level_steps.remove(k),
      };
    }
  }
  v
}
