//! The editor's document: a level loaded from the library (and the levels added to it, each under
//! its group: see scene/story), made of nodes.
//!
//!   Render    : visible geometry (terrain, decor instances, actor models)
//!   Collision : the level's collision surfaces, kept with the level but not shown
//!   Actor     : an entity of the level (etype, res-lump), with its model as a Render child
//!   Group     : an empty node that organizes the others (a decor prototype and its instances)
//!
//! A project is saved as a .glb: the same data, with the editor's node extras (og_kind...).
//!
//! Sharing: a node's mesh is an `Arc<Mesh>` (the instances of a prototype share it; change one
//! with `Arc::make_mut`, which copies it when it is shared: the copy gets a new uid). Meshes'
//! derived data (bounds, BVH) is made lazily and is thread safe, and a Scene is Send + Sync: an
//! `Arc<Scene>` (a clone of the document) can go to a worker thread.

pub mod bvh;
mod ground;
pub mod pick;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use bvh::{Bvh, BvhNode};
pub use ground::{
  ground_piece, natural_ground, piece_floor, split_ground_piece, structure_texture,
};

use crate::math::{AABB, Mat4, Transform, Vec2, Vec3, Vec4, cross, fmax, length2, normalize};

// ------------------------------------------------------------------------------------------------
// kinds
// ------------------------------------------------------------------------------------------------

/// NavMesh: where creatures walk (tools/navmesh), drawn over the level, not part of its decor.
/// Serialized as its `kind_name`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum NodeKind {
  #[default]
  Group = 0,
  Render,
  Collision,
  Actor,
  NavMesh,
}

impl NodeKind {
  pub const ALL: [NodeKind; 5] = [
    NodeKind::Group,
    NodeKind::Render,
    NodeKind::Collision,
    NodeKind::Actor,
    NodeKind::NavMesh,
  ];
}

/// "group", "render", "collision", "actor", "navmesh"
pub fn kind_name(k: NodeKind) -> &'static str {
  match k {
    NodeKind::Group => "group",
    NodeKind::Render => "render",
    NodeKind::Collision => "collision",
    NodeKind::Actor => "actor",
    NodeKind::NavMesh => "navmesh",
  }
}

pub fn kind_from_name(s: &str, fallback: NodeKind) -> NodeKind {
  NodeKind::ALL
    .into_iter()
    .find(|&k| kind_name(k) == s)
    .unwrap_or(fallback)
}

// ------------------------------------------------------------------------------------------------
// textures and materials
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Texture {
  pub name: String,
  pub width: i32,
  pub height: i32,
  /// width * height * 4
  pub rgba: Vec<u8>,
  /// bumped on change, the renderer re-uploads
  pub version: u64,
}

impl Default for Texture {
  fn default() -> Self {
    Self {
      name: String::new(),
      width: 0,
      height: 0,
      rgba: Vec::new(),
      version: 1,
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlphaMode {
  #[default]
  Opaque = 0,
  Mask,
  Blend,
}

/// How a Blend material mixes with what is behind it. glTF only has Alpha; the others are the PS2's
/// additive and subtractive blends (glows, light beams), kept in the material's extras.og_blend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
  #[default]
  Alpha = 0,
  Add,
  AddAlpha,
  Subtract,
  Half,
}

/// "" for Alpha, else "add", "add_alpha"...
pub fn blend_mode_name(b: BlendMode) -> &'static str {
  match b {
    BlendMode::Add => "add",
    BlendMode::AddAlpha => "add_alpha",
    BlendMode::Subtract => "subtract",
    BlendMode::Half => "half",
    BlendMode::Alpha => "",
  }
}

pub fn blend_mode_from_name(name: &str) -> BlendMode {
  [
    BlendMode::Add,
    BlendMode::AddAlpha,
    BlendMode::Subtract,
    BlendMode::Half,
  ]
  .into_iter()
  .find(|&b| blend_mode_name(b) == name)
  .unwrap_or(BlendMode::Alpha)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
  pub name: String,
  /// index into Scene::textures, -1 none
  pub texture: i32,
  pub base_color: Vec4,
  pub alpha: AlphaMode,
  pub blend: BlendMode,
  pub cutoff: f32,
  pub double_sided: bool,
  /// drawn by the game without writing its depth (extras.og_translucent: the extractor's): see
  /// through, like glass and water. The game blends most of a level's other surfaces too, but
  /// writes their depth: they are walls (the alpha test makes their holes).
  pub translucent: bool,
  /// a water surface of the game (a water actor's model): its texture flows, as the game animates
  /// it (set when its model is drawn, not saved)
  pub water: bool,
}

impl Default for Material {
  fn default() -> Self {
    Self {
      name: String::new(),
      texture: -1,
      base_color: Vec4::new(1.0, 1.0, 1.0, 1.0),
      alpha: AlphaMode::Opaque,
      blend: BlendMode::Alpha,
      cutoff: 0.5,
      double_sided: true,
      translucent: false,
      water: false,
    }
  }
}

/// Whether the game draws a material in its translucent pass, behind which the rest shows (glass,
/// water, glows, light shafts): additive and subtractive blends, the levels' surfaces drawn so
/// (Material::translucent), and the editor's own blended materials. What the export gives the level
/// builder's translucent tree, and the game view draws last.
pub fn translucent_in_game(m: &Material) -> bool {
  // a level's material is drawn twice as bright (base_color 2: the PS2's colors), the editor's not
  m.alpha == AlphaMode::Blend
    && (m.blend != BlendMode::Alpha || m.translucent || m.base_color.w <= 1.5)
}

// ------------------------------------------------------------------------------------------------
// meshes
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Primitive {
  pub pos: Vec<Vec3>,
  /// same size as pos
  pub nrm: Vec<Vec3>,
  /// empty or same size as pos
  pub uv: Vec<Vec2>,
  /// empty or same size as pos (vertex colors, 0..1)
  pub col: Vec<Vec4>,
  /// triangle list
  pub idx: Vec<u32>,
  /// index into Scene::materials, -1 none
  pub material: i32,
  /// collision only: the surface of each triangle
  pub tri_pat: Vec<u32>,
  /// extra vertex attributes kept for round trips, e.g. the 8 time-of-day palettes _SUNRISE...
  pub extra_attrs: BTreeMap<String, Vec<Vec4>>,
}

impl Default for Primitive {
  /// empty, without material (-1)
  fn default() -> Self {
    Self {
      pos: Vec::new(),
      nrm: Vec::new(),
      uv: Vec::new(),
      col: Vec::new(),
      idx: Vec::new(),
      material: -1,
      tri_pat: Vec::new(),
      extra_attrs: BTreeMap::new(),
    }
  }
}

impl Primitive {
  pub fn tri_count(&self) -> usize {
    self.idx.len() / 3
  }

  /// smooth normals from faces
  pub fn compute_normals(&mut self) {
    self.nrm = vec![Vec3::default(); self.pos.len()];
    let mut t = 0;
    while t + 2 < self.idx.len() {
      let (ia, ib, ic) = (
        self.idx[t] as usize,
        self.idx[t + 1] as usize,
        self.idx[t + 2] as usize,
      );
      let (a, b, c) = (self.pos[ia], self.pos[ib], self.pos[ic]);
      let n = cross(b - a, c - a); // area weighted
      self.nrm[ia] += n;
      self.nrm[ib] += n;
      self.nrm[ic] += n;
      t += 3;
    }
    for n in &mut self.nrm {
      *n = normalize(*n);
      if length2(*n) == 0.0 {
        *n = Vec3::new(0.0, 1.0, 0.0);
      }
    }
  }
}

static NEXT_MESH_UID: AtomicU64 = AtomicU64::new(1);

/// A mesh: its primitives and its derived data (bounds, a BVH per primitive), made lazily and
/// thread safe, reset by `touch()`. Clones get a new uid (C++ copy constructor).
#[derive(Debug)]
pub struct Mesh {
  pub name: String,
  pub prims: Vec<Primitive>,
  pub extras: Value,
  /// unique for the life of the program (render cache key)
  pub uid: u64,
  /// bumped by touch()
  pub version: u64,
  bounds: OnceLock<AABB>,
  bvh: OnceLock<Vec<OnceLock<Arc<Bvh>>>>,
}

impl Mesh {
  pub fn new() -> Self {
    Self {
      name: String::new(),
      prims: Vec::new(),
      extras: json!({}),
      uid: NEXT_MESH_UID.fetch_add(1, Ordering::Relaxed),
      version: 1,
      bounds: OnceLock::new(),
      bvh: OnceLock::new(),
    }
  }

  /// derived data, rebuilt lazily after touch()
  pub fn bounds(&self) -> AABB {
    *self.bounds.get_or_init(|| {
      let mut b = AABB::default();
      for p in &self.prims {
        for v in &p.pos {
          b.add(*v);
        }
      }
      b
    })
  }

  /// The BVH of a primitive (in the mesh's space).
  pub fn bvh(&self, prim: usize) -> Arc<Bvh> {
    let build = || Arc::new(Bvh::build(&self.prims[prim].pos, &self.prims[prim].idx));
    let slots = self
      .bvh
      .get_or_init(|| (0..self.prims.len()).map(|_| OnceLock::new()).collect());
    if slots.len() != self.prims.len() {
      return build(); // primitives added without touch(): not kept
    }
    slots[prim].get_or_init(build).clone()
  }

  /// After a change of the geometry: a new version, the derived data made again.
  pub fn touch(&mut self) {
    self.version += 1;
    self.bounds = OnceLock::new();
    self.bvh = OnceLock::new();
  }

  pub fn tri_count(&self) -> usize {
    self.prims.iter().map(Primitive::tri_count).sum()
  }
}

impl Default for Mesh {
  fn default() -> Self {
    Self::new()
  }
}

impl Clone for Mesh {
  /// copies get a new uid
  fn clone(&self) -> Self {
    Self {
      name: self.name.clone(),
      prims: self.prims.clone(),
      extras: self.extras.clone(),
      ..Self::new()
    }
  }
}

/// The identity of a node's mesh (its address), 0 for none: what the cache groups nodes by.
pub fn mesh_key(mesh: &Option<Arc<Mesh>>) -> usize {
  mesh.as_ref().map_or(0, |m| Arc::as_ptr(m) as usize)
}

// ------------------------------------------------------------------------------------------------
// the game's lighting
// ------------------------------------------------------------------------------------------------

/// The game's lighting of a level's geometry (Jak 3): the game mixes 8 palettes per color (the
/// ambient light, 4 directions of the sun, the lamps of each area...) with weights it sets every
/// frame from the time of the day and the level's mood. A table of colors, 8 RGBA bytes each...
#[derive(Clone, Debug, Default)]
pub struct TodPalette {
  /// color c's palette k at c * 32 + k * 4
  pub rgba: Vec<u8>,
  /// made for one element, lit like the level around it (scene/relight), not a tree of the level's
  pub local: bool,
}

impl TodPalette {
  pub fn size(&self) -> usize {
    self.rgba.len() / 32
  }
}

/// ...and the color of each vertex of a node's mesh in it. The instances of a decor prototype share
/// its mesh, not their lighting: it is the node's.
#[derive(Clone, Debug, Default)]
pub struct TodColors {
  pub palette: Option<Arc<TodPalette>>,
  /// per primitive of the mesh, per vertex (< palette's size)
  pub index: Vec<Vec<u16>>,
}

impl TodColors {
  /// whether it still matches the mesh (its vertices edited, the node is lit like new geometry)
  pub fn fits(&self, mesh: &Mesh) -> bool {
    self.palette.is_some()
      && self.index.len() == mesh.prims.len()
      && self
        .index
        .iter()
        .zip(&mesh.prims)
        .all(|(i, p)| i.len() == p.pos.len())
  }
}

/// How much of a lit color each palette gives on average (the colors of the extracted levels are
/// the brightest of their 8): new geometry, which has one color, gets its palettes this way. From
/// the nodes of the scene that have their lighting, else a district of the city's (ctyindb).
pub fn tod_ratios(scene: &Scene, nodes: &[u32]) -> [f32; 8] {
  // a district of the city (ctyindb): the ambient light gives almost all of the brightest color
  let mut ratios = [0.986, 0.117, 0.17, 0.164, 0.135, 0.007, 0.043, 0.037];
  let mut sum = [0f64; 8];
  let mut brightest = 0f64;
  for &id in nodes {
    let Some(n) = scene.find(id) else { continue };
    let (Some(mesh), Some(tod)) = (&n.mesh, &n.tod) else {
      continue;
    };
    if !tod.fits(mesh) {
      continue;
    }
    let rgba = &tod.palette.as_ref().unwrap().rgba;
    for prim in &tod.index {
      for &color in prim.iter().step_by(7) {
        let Some(c) = rgba.get(color as usize * 32..color as usize * 32 + 32) else {
          continue;
        };
        let mut best = 0f32;
        for k in 0..8 {
          let lum = (c[k * 4] as f32 + c[k * 4 + 1] as f32 + c[k * 4 + 2] as f32) / 3.0;
          sum[k] += lum as f64;
          best = fmax(best, lum);
        }
        brightest += best as f64;
      }
    }
  }
  if brightest > 0.0 {
    for k in 0..8 {
      ratios[k] = (sum[k] / brightest) as f32;
    }
  }
  ratios
}

/// a color as the editor shows it (the vertex's times its material's: 1 is the texture as it is)
/// as 8 palettes
pub fn tod_from_color(shown: Vec4, ratios: &[f32; 8]) -> [u8; 32] {
  let mut out = [0u8; 32];
  // the game doubles the colors it mixes (128 is 1.0)
  for k in 0..8 {
    let c = [shown.x, shown.y, shown.z];
    for ch in 0..3 {
      out[k * 4 + ch] = (c[ch] * ratios[k] * 127.5 + 0.5).clamp(0.0, 255.0) as u8;
    }
    out[k * 4 + 3] = 128; // the game's (the alpha it mixes is capped there)
  }
  out
}

// ------------------------------------------------------------------------------------------------
// nodes, settings
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Node {
  pub id: u32,
  /// 0 = root
  pub parent: u32,
  pub name: String,
  pub kind: NodeKind,
  pub local: Transform,
  /// Render, Collision
  pub mesh: Option<Arc<Mesh>>,
  /// actor data (etype, aid, lump...), glTF extras: an object
  pub extras: Value,
  /// its lighting in the game, when known (Render)
  pub tod: Option<Arc<TodColors>>,
  pub visible: bool,
  /// not selectable (the terrain)
  pub locked: bool,
  /// not shown at the point of the story viewed (not saved)
  pub story_hidden: bool,
}

impl Default for Node {
  fn default() -> Self {
    Self {
      id: 0,
      parent: 0,
      name: String::new(),
      kind: NodeKind::Group,
      local: Transform::default(),
      mesh: None,
      extras: json!({}),
      tod: None,
      visible: true,
      locked: false,
      story_hidden: false,
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelSettings {
  /// the library level the document comes from ("desert")
  pub level: String,
  /// its folder in the library: its models are drawn for its actors
  pub source_group: String,
  /// a level of the game this one is made to replace in the game ("ctyindb"), "" for a level of
  /// its own: the game then loads it at that level's place, with that level's continue points
  pub replaces: String,
}

impl LevelSettings {
  pub fn to_json(&self) -> Value {
    let mut j = json!({"level": self.level, "source_group": self.source_group});
    if !self.replaces.is_empty() {
      j["replaces"] = json!(self.replaces);
    }
    j
  }

  pub fn from_json(j: &Value) -> LevelSettings {
    let s = |k: &str| j.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    LevelSettings {
      level: s("level"),
      source_group: s("source_group"),
      replaces: s("replaces"),
    }
  }
}

// ------------------------------------------------------------------------------------------------
// the cache
// ------------------------------------------------------------------------------------------------

/// World matrices, effective visibility and owning actor of every node, aligned with Scene::nodes.
/// Computed once per Scene::begin_frame() instead of walking the parents of each node again for
/// every query (a whole city is 150 000 nodes).
#[derive(Clone, Debug)]
pub struct SceneCache {
  pub worlds: Vec<Mat4>,
  /// the node and all its parents are visible (and not hidden by the story)
  pub visible: Vec<bool>,
  /// Render nodes under an actor (its model): the actor, else 0
  pub actor: Vec<u32>,
  /// under a group marked og_reference: a level shown to compare with, not part of the document's
  /// level (not selected, not exported)
  pub reference: Vec<bool>,
  /// world box of the node's mesh (Render, Collision), else empty
  pub bounds: Vec<AABB>,
  /// render nodes grouped by mesh (the instances of a prototype): the mesh (`mesh_key`), node
  /// indices; the mesh itself is `nodes[indices[0]].mesh`
  pub mesh_groups: Vec<(usize, Vec<u32>)>,
  /// each node's mesh (`mesh_key`) when they were grouped
  pub meshes: Vec<usize>,
  /// the tree by index: each node's parent (-1 for a root), and its children (kids[kid_start[i] ..
  /// kid_start[i + 1]]), so that walking a subtree does not scan the whole scene
  pub parent: Vec<i32>,
  pub kid_start: Vec<u32>,
  pub kids: Vec<u32>,
  pub parents_hash: u64,
  /// counts the times the cache was made: what was computed from it is still good while it holds
  pub version: u64,
  pub frame: u64,
  pub count: usize,
  pub first_id: u32,
  pub last_id: u32,
}

impl Default for SceneCache {
  fn default() -> Self {
    Self {
      worlds: Vec::new(),
      visible: Vec::new(),
      actor: Vec::new(),
      reference: Vec::new(),
      bounds: Vec::new(),
      mesh_groups: Vec::new(),
      meshes: Vec::new(),
      parent: Vec::new(),
      kid_start: Vec::new(),
      kids: Vec::new(),
      parents_hash: 0,
      version: 0,
      frame: u64::MAX,
      count: 0,
      first_id: 0,
      last_id: 0,
    }
  }
}

/// id -> index of the nodes, checked against the vector (Scene::index_of)
struct IndexMap {
  index: HashMap<u32, usize>,
  /// the node ids the map was made from (a sum and the count)
  ids: u64,
}

impl Default for IndexMap {
  fn default() -> Self {
    Self {
      index: HashMap::new(),
      ids: u64::MAX,
    }
  }
}

const K_FNV_PRIME: u64 = 1099511628211;

// ------------------------------------------------------------------------------------------------
// the scene
// ------------------------------------------------------------------------------------------------

pub struct Scene {
  /// parents before children is not required
  pub nodes: Vec<Node>,
  pub textures: Vec<Texture>,
  pub materials: Vec<Material>,
  pub settings: LevelSettings,
  pub next_id: u32,
  frame: u64,
  cache: Mutex<Option<Arc<SceneCache>>>,
  index: RwLock<IndexMap>,
}

impl Default for Scene {
  fn default() -> Self {
    Self {
      nodes: Vec::new(),
      textures: Vec::new(),
      materials: Vec::new(),
      settings: LevelSettings::default(),
      next_id: 1,
      frame: 0,
      cache: Mutex::new(None),
      index: RwLock::new(IndexMap::default()),
    }
  }
}

impl Clone for Scene {
  /// the document's data; the copy makes its caches again
  fn clone(&self) -> Self {
    Self {
      nodes: self.nodes.clone(),
      textures: self.textures.clone(),
      materials: self.materials.clone(),
      settings: self.settings.clone(),
      next_id: self.next_id,
      frame: self.frame,
      ..Default::default()
    }
  }
}

impl Scene {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn find(&self, id: u32) -> Option<&Node> {
    self.index_of(id).map(|i| &self.nodes[i])
  }

  pub fn find_mut(&mut self, id: u32) -> Option<&mut Node> {
    self.index_of(id).map(|i| &mut self.nodes[i])
  }

  pub fn index_of(&self, id: u32) -> Option<usize> {
    if id == 0 {
      return None;
    }
    // fast path: a map keyed by id, checked against the vector
    let found = self
      .index
      .read()
      .unwrap_or_else(|e| e.into_inner())
      .index
      .get(&id)
      .copied();
    if let Some(i) = found
      && i < self.nodes.len()
      && self.nodes[i].id == id
    {
      return Some(i);
    }
    // an id that is not there (a node deleted): the map is made again only when the nodes changed,
    // not for every question about a missing node (that rebuild walks a whole city each time)
    let mut ids = self.nodes.len() as u64;
    for n in &self.nodes {
      ids = ids.wrapping_mul(K_FNV_PRIME).wrapping_add(n.id as u64);
    }
    let mut map = self.index.write().unwrap_or_else(|e| e.into_inner());
    if found.is_none() && ids == map.ids {
      return None;
    }
    map.index.clear();
    map.index.reserve(self.nodes.len());
    for (i, n) in self.nodes.iter().enumerate() {
      map.index.insert(n.id, i);
    }
    map.ids = ids;
    map.index.get(&id).copied()
  }

  /// assigns an id when node.id == 0
  pub fn add(&mut self, mut node: Node) -> u32 {
    if node.id == 0 {
      node.id = self.next_id;
      self.next_id += 1;
    }
    self.next_id = self.next_id.max(node.id + 1);
    let id = node.id;
    self.nodes.push(node);
    id
  }

  /// removes the nodes and all their descendants
  pub fn remove(&mut self, ids: &[u32]) {
    let mut doomed: HashSet<u32> = ids.iter().copied().collect();
    for d in self.descendants_of(ids) {
      doomed.extend(d);
    }
    self.nodes.retain(|n| !doomed.contains(&n.id));
  }

  pub fn children(&self, id: u32) -> Vec<u32> {
    self
      .nodes
      .iter()
      .filter(|n| n.parent == id)
      .map(|n| n.id)
      .collect()
  }

  pub fn descendants(&self, id: u32) -> Vec<u32> {
    self.descendants_of(&[id]).swap_remove(0)
  }

  /// The descendants of each of `roots` (not the roots themselves), in one pass over the nodes
  /// however many roots: a node under two of them goes to the nearest.
  pub fn descendants_of(&self, roots: &[u32]) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); roots.len()];
    if roots.is_empty() || self.nodes.is_empty() {
      return out;
    }
    let n = self.nodes.len();
    // id -> index (ids are small and dense)
    let (max_id, index) = self.id_index();
    let parent_index = |i: usize| -> i32 {
      let p = self.nodes[i].parent;
      if p != 0 && p <= max_id {
        index[p as usize]
      } else {
        -1
      }
    };
    // which root is each node a root of; the nearest root strictly above each node (-1: none, -2:
    // not known yet), found walking up once per node: what is above a node is known when it is
    // reached
    let mut root_of = vec![-1i32; n];
    let mut above = vec![-2i32; n];
    for (r, &root) in roots.iter().enumerate() {
      if root <= max_id && index[root as usize] >= 0 {
        root_of[index[root as usize] as usize] = r as i32;
      }
    }
    let mut chain: Vec<i32> = Vec::new();
    for i in 0..n {
      if above[i] != -2 {
        continue;
      }
      chain.clear();
      let mut cur = i as i32;
      while cur >= 0 && above[cur as usize] == -2 && chain.len() < 1000 {
        chain.push(cur);
        cur = parent_index(cur as usize);
      }
      for &c in chain.iter().rev() {
        let p = parent_index(c as usize);
        above[c as usize] = if p < 0 {
          -1
        } else if root_of[p as usize] >= 0 {
          root_of[p as usize]
        } else {
          above[p as usize].max(-1)
        };
      }
    }
    for i in 0..n {
      if above[i] >= 0 {
        out[above[i] as usize].push(self.nodes[i].id);
      }
    }
    out
  }

  /// the largest id and id -> index (-1 none)
  fn id_index(&self) -> (u32, Vec<i32>) {
    let max_id = self.nodes.iter().map(|n| n.id).max().unwrap_or(0);
    let mut index = vec![-1i32; max_id as usize + 1];
    for (i, n) in self.nodes.iter().enumerate() {
      index[n.id as usize] = i as i32;
    }
    (max_id, index)
  }

  /// The indices (in `nodes`) of the subtree of the node at `index`, itself first, from the cache's
  /// tree: as the nodes were when the cache was made.
  pub fn subtree(&self, index: usize, out: &mut Vec<u32>) {
    let c = self.cache();
    if index >= c.count {
      return;
    }
    let first = out.len();
    out.push(index as u32);
    let mut k = first;
    while k < out.len() {
      let i = out[k] as usize;
      for j in c.kid_start[i]..c.kid_start[i + 1] {
        out.push(c.kids[j as usize]);
      }
      k += 1;
    }
  }

  pub fn is_ancestor(&self, ancestor: u32, id: u32) -> bool {
    let mut n = self.find(id);
    let mut guard = 0;
    while let Some(node) = n {
      if node.parent == 0 || guard >= 1000 {
        break;
      }
      guard += 1;
      if node.parent == ancestor {
        return true;
      }
      n = self.find(node.parent);
    }
    false
  }

  pub fn effectively_visible(&self, n: &Node) -> bool {
    let mut cur = Some(n);
    let mut guard = 0;
    while let Some(node) = cur {
      if guard >= 1000 {
        break;
      }
      guard += 1;
      if !node.visible || node.story_hidden {
        return false;
      }
      if node.parent == 0 {
        return true;
      }
      cur = self.find(node.parent);
    }
    true
  }

  /// the actor a render node belongs to (its model drawn under it), or 0
  pub fn actor_of(&self, n: &Node) -> u32 {
    if n.kind != NodeKind::Render {
      return 0;
    }
    let mut p = if n.parent != 0 {
      self.find(n.parent)
    } else {
      None
    };
    let mut guard = 0;
    while let Some(node) = p {
      if guard >= 64 {
        break;
      }
      guard += 1;
      if node.kind == NodeKind::Actor {
        return node.id;
      }
      p = if node.parent != 0 {
        self.find(node.parent)
      } else {
        None
      };
    }
    0
  }

  pub fn world(&self, n: &Node) -> Mat4 {
    let mut m = n.local.matrix();
    let mut p = if n.parent != 0 {
      self.find(n.parent)
    } else {
      None
    };
    let mut guard = 0;
    while let Some(node) = p {
      if guard >= 1000 {
        break;
      }
      guard += 1;
      m = node.local.matrix() * m;
      p = if node.parent != 0 {
        self.find(node.parent)
      } else {
        None
      };
    }
    m
  }

  /// the world matrix of node `id`, the identity when there is none
  pub fn world_of(&self, id: u32) -> Mat4 {
    self.find(id).map_or(Mat4::identity(), |n| self.world(n))
  }

  /// A new frame: the cache is made again at its next use.
  pub fn begin_frame(&mut self) {
    self.frame += 1;
  }

  /// the cache of the current frame (recomputed after begin_frame() or when nodes were added or
  /// removed); queries made after a change in the same frame see the old matrices
  pub fn cache(&self) -> Arc<SceneCache> {
    let mut slot = self.cache.lock().unwrap_or_else(|e| e.into_inner());
    let nodes = &self.nodes;
    let n = nodes.len();
    // the node set itself (as opposed to only their transforms, which a drag changes every frame):
    // count and the id at each end are enough to tell, as cheaply as the frame counter itself
    let same = |c: &SceneCache| {
      c.count == n && (n == 0 || (c.first_id == nodes[0].id && c.last_id == nodes[n - 1].id))
    };
    if let Some(c) = slot.as_ref()
      && c.frame == self.frame
      && same(c)
    {
      return c.clone();
    }
    // the previous one's buffers are reused (copied when someone still holds it)
    let mut c = slot.take().map(Arc::unwrap_or_clone).unwrap_or_default();
    let same_nodes = same(&c);
    c.frame = self.frame;
    c.version += 1;
    c.count = n;
    c.first_id = if n > 0 { nodes[0].id } else { 0 };
    c.last_id = if n > 0 { nodes[n - 1].id } else { 0 };
    c.worlds.resize(n, Mat4::identity());
    c.visible.clear();
    c.visible.resize(n, false);
    c.actor.clear();
    c.actor.resize(n, 0);
    c.reference.clear();
    c.reference.resize(n, false);
    c.bounds.clear();
    c.bounds.resize(n, AABB::default());
    c.meshes.resize(n, 0);
    let mut meshes_changed = false; // a node given another mesh (a new texture, a shape resized, undo)
    // id -> index (ids are small and dense)
    let (max_id, index) = self.id_index();
    let parent_of = |node: &Node| -> i32 {
      if node.parent != 0 && node.parent <= max_id {
        index[node.parent as usize]
      } else {
        -1
      }
    };
    let mut done = vec![false; n];
    let mut chain: Vec<usize> = Vec::new();
    for i in 0..n {
      if done[i] {
        continue;
      }
      // the unresolved ancestors, then down from the first resolved one
      chain.clear();
      let mut cur = i as i32;
      while cur >= 0 && !done[cur as usize] && chain.len() < 1000 {
        chain.push(cur as usize);
        cur = parent_of(&nodes[cur as usize]);
      }
      for &j in chain.iter().rev() {
        let node = &nodes[j];
        let p = parent_of(node);
        let local = node.local.matrix();
        let reference = node.kind == NodeKind::Group && node.extras.get("og_reference").is_some();
        if p >= 0 && done[p as usize] {
          let p = p as usize;
          c.worlds[j] = c.worlds[p] * local;
          c.visible[j] = c.visible[p] && node.visible && !node.story_hidden;
          c.reference[j] = c.reference[p] || reference;
          if node.kind == NodeKind::Render {
            c.actor[j] = if nodes[p].kind == NodeKind::Actor {
              nodes[p].id
            } else {
              c.actor[p]
            };
          } else if node.kind != NodeKind::Actor {
            c.actor[j] = c.actor[p];
          }
        } else {
          c.worlds[j] = local;
          c.visible[j] = node.visible && !node.story_hidden;
          c.reference[j] = reference;
        }
        if let Some(mesh) = &node.mesh {
          c.bounds[j] = mesh.bounds().transformed(&c.worlds[j]);
        }
        let key = mesh_key(&node.mesh);
        if c.meshes[j] != key {
          c.meshes[j] = key;
          meshes_changed = true;
        }
        done[j] = true;
      }
    }
    // the tree by index, made again when a node came, went or changed parent
    let mut parents_hash = n as u64;
    for node in nodes {
      parents_hash = parents_hash
        .wrapping_mul(K_FNV_PRIME)
        .wrapping_add(node.parent as u64);
    }
    if !same_nodes || parents_hash != c.parents_hash || c.kid_start.len() != n + 1 {
      c.parents_hash = parents_hash;
      c.parent = nodes.iter().map(parent_of).collect();
      c.kid_start.clear();
      c.kid_start.resize(n + 1, 0);
      for i in 0..n {
        if c.parent[i] >= 0 {
          c.kid_start[c.parent[i] as usize + 1] += 1;
        }
      }
      for i in 0..n {
        c.kid_start[i + 1] += c.kid_start[i];
      }
      c.kids.clear();
      c.kids.resize(c.kid_start[n] as usize, 0);
      let mut fill = c.kid_start[..n].to_vec();
      for i in 0..n {
        if c.parent[i] >= 0 {
          let p = c.parent[i] as usize;
          c.kids[fill[p] as usize] = i as u32;
          fill[p] += 1;
        }
      }
    }
    // instances of a prototype (a decor piece, an actor's model) keep the same index in `nodes`
    // while only transforms change (a drag): regrouping by mesh, with its many small allocations,
    // is only needed when the node set itself changed, or a node's mesh
    if !same_nodes || meshes_changed {
      c.mesh_groups.clear();
      let mut group_of: HashMap<usize, usize> = HashMap::with_capacity(n / 4 + 1);
      for (i, node) in nodes.iter().enumerate() {
        if node.kind != NodeKind::Render || node.mesh.is_none() {
          continue;
        }
        let key = mesh_key(&node.mesh);
        let g = *group_of.entry(key).or_insert_with(|| {
          c.mesh_groups.push((key, Vec::new()));
          c.mesh_groups.len() - 1
        });
        c.mesh_groups[g].1.push(i as u32);
      }
    }
    let c = Arc::new(c);
    *slot = Some(c.clone());
    c
  }

  /// sets the local transform of node `id` so that its world matrix becomes `w`
  pub fn set_world(&mut self, id: u32, w: &Mat4) {
    let Some(i) = self.index_of(id) else { return };
    let parent = self.nodes[i].parent;
    let parent = if parent != 0 {
      self.world_of(parent)
    } else {
      Mat4::identity()
    };
    self.nodes[i].local = Transform::from_matrix(&(parent.inverse() * *w));
  }

  /// mesh bounds, or the marker of an actor
  pub fn world_bounds(&self, n: &Node) -> AABB {
    let w = self.world(n);
    if let Some(mesh) = &n.mesh {
      return mesh.bounds().transformed(&w);
    }
    let mut b = AABB::default();
    let c = w.col3(3);
    if n.kind == NodeKind::Actor {
      let r = Vec3::new(K_MARKER_RADIUS, K_MARKER_RADIUS, K_MARKER_RADIUS);
      b.add(c - r);
      b.add(c + r);
    } else {
      b.add(c);
    }
    b
  }

  /// the world box of each node but the groups and the collision, from the cache (a city is
  /// 150 000 nodes: their world matrices are not made again for this)
  fn drawn_boxes(&self) -> Vec<AABB> {
    let c = self.cache();
    let mut boxes = Vec::with_capacity(c.count);
    for (i, n) in self.nodes.iter().enumerate().take(c.count) {
      if n.kind == NodeKind::Group || n.kind == NodeKind::Collision {
        continue;
      }
      if c.bounds[i].valid() {
        boxes.push(c.bounds[i]);
      } else {
        let p = c.worlds[i].col3(3);
        let r = if n.kind == NodeKind::Actor {
          K_MARKER_RADIUS
        } else {
          0.0
        };
        let mut b = AABB::default();
        b.add(p - Vec3::new(r, r, r));
        b.add(p + Vec3::new(r, r, r));
        boxes.push(b);
      }
    }
    boxes
  }

  /// whole scene (collision left out)
  pub fn bounds(&self) -> AABB {
    let mut b = AABB::default();
    for bx in self.drawn_boxes() {
      b.add_box(&bx);
    }
    b
  }

  /// where most of the scene is: a stray piece far away does not shrink the view (framing)
  pub fn focus_bounds(&self) -> AABB {
    let boxes = self.drawn_boxes();
    let b = focus_box(&boxes);
    if b.valid() {
      return b;
    }
    let mut all = AABB::default();
    for bx in &boxes {
      all.add_box(bx);
    }
    all
  }

  pub fn add_material(&mut self, m: Material) -> i32 {
    self.materials.push(m);
    self.materials.len() as i32 - 1
  }

  pub fn add_texture(&mut self, t: Texture) -> i32 {
    self.textures.push(t);
    self.textures.len() as i32 - 1
  }

  pub fn clear(&mut self) {
    self.nodes.clear();
    self.textures.clear();
    self.materials.clear();
    self.next_id = 1;
  }

  /// Moves the nodes, materials and textures of `src` into this scene (a level loaded apart): the
  /// nodes get new ids, the textures and materials this scene already has are shared. Returns the
  /// new ids of src's roots.
  pub fn append(&mut self, mut src: Scene) -> Vec<u32> {
    // textures: the same image (name, size, pixels) is shared
    let mut by_hash: HashMap<u64, Vec<i32>> = HashMap::new();
    for (i, t) in self.textures.iter().enumerate() {
      by_hash.entry(fnv1a(&t.rgba)).or_default().push(i as i32);
    }
    let mut tex_map = vec![-1i32; src.textures.len()];
    for (i, t) in std::mem::take(&mut src.textures).into_iter().enumerate() {
      let h = fnv1a(&t.rgba);
      let list = by_hash.entry(h).or_default();
      for &j in list.iter() {
        let o = &self.textures[j as usize];
        if o.name == t.name && o.width == t.width && o.height == t.height && o.rgba == t.rgba {
          tex_map[i] = j;
        }
      }
      if tex_map[i] < 0 {
        tex_map[i] = self.add_texture(t);
        list.push(tex_map[i]);
      }
    }
    let mut mat_map = vec![-1i32; src.materials.len()];
    for (i, mut m) in std::mem::take(&mut src.materials).into_iter().enumerate() {
      if m.texture >= 0 && (m.texture as usize) < tex_map.len() {
        m.texture = tex_map[m.texture as usize];
      }
      if let Some(j) = self.materials.iter().position(|o| same_material(o, &m)) {
        mat_map[i] = j as i32;
      } else {
        mat_map[i] = self.add_material(m);
      }
    }
    // the meshes (shared by the copies of a prototype) take the new material numbers once: taken
    // from their nodes so that the copy on write does not copy them, given back after
    let mut meshes: HashMap<usize, Arc<Mesh>> = HashMap::new();
    let keys: Vec<usize> = src
      .nodes
      .iter_mut()
      .map(|n| match n.mesh.take() {
        Some(m) => {
          let key = Arc::as_ptr(&m) as usize;
          meshes.entry(key).or_insert(m);
          key
        }
        None => 0,
      })
      .collect();
    let renumbered = |m: i32| {
      if m >= 0 && (m as usize) < mat_map.len() {
        mat_map[m as usize]
      } else {
        m
      }
    };
    for mesh in meshes.values_mut() {
      if mesh
        .prims
        .iter()
        .any(|p| renumbered(p.material) != p.material)
      {
        for p in &mut Arc::make_mut(mesh).prims {
          p.material = renumbered(p.material);
        }
      }
    }
    for (n, key) in src.nodes.iter_mut().zip(keys) {
      if key != 0 {
        n.mesh = Some(meshes[&key].clone());
      }
    }
    let mut ids: HashMap<u32, u32> = HashMap::new();
    for n in &src.nodes {
      ids.insert(n.id, self.next_id);
      self.next_id += 1;
    }
    let mut roots = Vec::new();
    self.nodes.reserve(src.nodes.len());
    for mut n in std::mem::take(&mut src.nodes) {
      n.id = ids[&n.id];
      n.parent = if n.parent != 0 {
        ids.get(&n.parent).copied().unwrap_or(0)
      } else {
        0
      };
      if n.parent == 0 {
        roots.push(n.id);
      }
      self.nodes.push(n);
    }
    roots
  }
}

fn fnv1a(data: &[u8]) -> u64 {
  let mut h: u64 = 1469598103934665603;
  for &b in data {
    h = (h ^ b as u64).wrapping_mul(K_FNV_PRIME);
  }
  h
}

fn same_material(a: &Material, b: &Material) -> bool {
  a.name == b.name
    && a.texture == b.texture
    && a.alpha == b.alpha
    && a.blend == b.blend
    && a.translucent == b.translucent
    && a.cutoff == b.cutoff
    && a.double_sided == b.double_sided
    && a.base_color == b.base_color
}

/// Where most of these boxes are: the 2nd to 98th percentile on each axis, so that a stray piece
/// far away does not count; their union when they are few.
pub fn focus_box(boxes: &[AABB]) -> AABB {
  let mut all = AABB::default();
  let mut lo: [Vec<f32>; 3] = Default::default();
  let mut hi: [Vec<f32>; 3] = Default::default();
  for b in boxes {
    if !b.valid() {
      continue;
    }
    all.add_box(b);
    for a in 0..3 {
      lo[a].push(b.lo[a]);
      hi[a].push(b.hi[a]);
    }
  }
  if lo[0].len() < 20 {
    return all;
  }
  let mut out = AABB::default();
  let (mut l, mut h) = (Vec3::default(), Vec3::default());
  for a in 0..3 {
    // the two percentiles alone, not the whole order
    let (l_at, h_at) = (lo[a].len() * 2 / 100, hi[a].len() * 98 / 100);
    l[a] = *lo[a].select_nth_unstable_by(l_at, f32::total_cmp).1;
    h[a] = *hi[a].select_nth_unstable_by(h_at, f32::total_cmp).1;
  }
  out.add(l);
  out.add(h);
  if out.valid() { out } else { all }
}

/// Whether a node of an extracted level is one of its trees of instanced decor, <level>-tie-<n> or
/// <level>-shrub-<n>: its children are the prototypes of its decor, their children the instances.
/// A prototype's own name may hold "-tie-" or "-shrub-" too (des-stone-shrub-01.mb).
pub fn instance_tree(name: &str) -> bool {
  for kind in ["-tie-", "-shrub-"] {
    let Some(at) = name.rfind(kind) else { continue };
    if at == 0 {
      continue;
    }
    let digits = &name[at + kind.len()..];
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
      return true;
    }
  }
  false
}

/// Marker size of actors without a model (meters)
pub const K_MARKER_RADIUS: f32 = 0.6;
