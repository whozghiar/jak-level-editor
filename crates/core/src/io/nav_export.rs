//! The nav meshes of a level for the game: entity-nav-mesh objects laid out as the game's levels
//! have them (entity, nav-mesh, route table, grid-hash of the polygons, polygons, res tags), in the
//! relocatable form OpenGOAL's level builder writes into a bsp: a DataBlob json (words, pointers,
//! types, symbols, roots "nav-mesh-<n>"), the "nav_data" of a level (goalc/build_level/common/
//! DataBlob.h). Distances in game units (4096 per meter).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use crate::i18n::tr;
use crate::log_info;
use crate::math::{AABB, Vec3, length};
use crate::navmesh::{NavMeshData, NavPoly, navmesh_problem, navmesh_route_table, world_navmeshes};
use crate::scene::Scene;

/// game units per meter
const K_UNITS: f32 = 4096.0;
/// the smallest grid cell: 4 m
const K_MIN_CELL: f32 = 16384.0;
/// the grid's cells cover its box widened by this
const K_GRID_MARGIN: f32 = 1024.0;
/// -1e9: a res tag without key frame
const K_DEFAULT_RES_TIME: u32 = 0xce6e6b28;
/// grid-hash spr-bucket-array
const K_SCRATCHPAD: u32 = 0x70000000;
/// meters: how far above or below a polygon a point may be to be on it (res nearest-y-threshold)
pub const K_NEAREST_Y_THRESHOLD: f32 = 10.0;
/// res nav-max-users
pub const K_MAX_USERS: u32 = 64;

pub struct NavExportEntry<'a> {
  pub mesh: &'a NavMeshData,
  /// the entity's id: unique in the game and not an actor's
  pub aid: u32,
  /// its res 'name
  pub name: String,
}

/// The grid-hash of a mesh's polygons (nav-mesh poly-hash), made like the game's: a box centered on
/// the mesh's origin, about one cell per polygon (cells of at least 4 m), one layer; each cell holds
/// the polygons whose box meets it, but those whose pat has bit 0. The same as the game's, byte for
/// byte, for every nav mesh of the game's levels.
#[derive(Clone, Debug, PartialEq)]
pub struct NavGrid {
  pub dims: [i32; 3],
  pub axis_scale: [f32; 3],
  pub box_min: [f32; 3],
  pub box_max: [f32; 3],
  /// bytes: one bit per polygon
  pub bucket_size: i32,
  pub bucket_count: i32,
  pub buckets: Vec<u8>,
}

impl Default for NavGrid {
  fn default() -> Self {
    Self {
      dims: [1, 1, 1],
      axis_scale: [0.0; 3],
      box_min: [0.0; 3],
      box_max: [0.0; 3],
      bucket_size: 1,
      bucket_count: 1,
      buckets: Vec::new(),
    }
  }
}

/// Words being laid out, with how each links (see the module's doc): objects 16-byte aligned,
/// basics with their type tag first and their pointer 4 bytes after it.
#[derive(Default)]
struct BlobWriter {
  words: Vec<u32>,
  /// word -> byte
  pointers: Vec<(usize, usize)>,
  types: Vec<(usize, String)>,
  symbols: Vec<(usize, String)>,
}

impl BlobWriter {
  fn size(&self) -> usize {
    self.words.len() * 4
  }
  fn align16(&mut self) {
    while !self.words.len().is_multiple_of(4) {
      self.words.push(0);
    }
  }
  /// zeroed room for `bytes`, 16-aligned; returns its first byte
  fn reserve(&mut self, bytes: usize) -> usize {
    self.align16();
    let at = self.size();
    self.words.resize(self.words.len() + bytes.div_ceil(4), 0);
    at
  }
  /// a basic of `bytes` (type tag included); returns its pointer (tag + 4)
  fn basic(&mut self, ty: &str, bytes: usize) -> usize {
    let tag = self.reserve(bytes);
    self.types.push((tag / 4, ty.to_string()));
    tag + 4
  }
  fn u32(&mut self, byte: usize, v: u32) {
    self.words[byte / 4] = v;
  }
  fn f32(&mut self, byte: usize, v: f32) {
    self.u32(byte, v.to_bits());
  }
  fn u8(&mut self, byte: usize, v: u8) {
    let w = &mut self.words[byte / 4];
    let s = 8 * (byte % 4);
    *w = (*w & !(0xff << s)) | ((v as u32) << s);
  }
  fn u16(&mut self, byte: usize, v: u16) {
    self.u8(byte, v as u8);
    self.u8(byte + 1, (v >> 8) as u8);
  }
  fn ptr(&mut self, byte: usize, to: usize) {
    self.pointers.push((byte / 4, to));
  }
  fn sym(&mut self, byte: usize, s: &str) {
    self.symbols.push((byte / 4, s.to_string()));
  }
  fn ty(&mut self, byte: usize, t: &str) {
    self.types.push((byte / 4, t.to_string()));
  }
}

/// res tags of an entity sorted as the game looks them up: by their first 8 characters read as a
/// little-endian number (goalc/build_level/common/ResLump.cpp)
fn res_key(name: &str) -> u64 {
  let mut k = [0u8; 8];
  let n = name.len().min(8);
  k[..n].copy_from_slice(&name.as_bytes()[..n]);
  u64::from_le_bytes(k)
}

fn write_mesh(w: &mut BlobWriter, e: &NavExportEntry) -> usize {
  let m = e.mesh;
  let n = m.polys.len();
  let mut vb = AABB::default();
  for &v in &m.verts {
    vb.add(v);
  }
  let radius = if vb.valid() {
    length(vb.lo).max(length(vb.hi))
  } else {
    0.0
  };

  // entity-nav-mesh (res-lump, entity, the mesh), then the mesh
  let ent = w.basic("entity-nav-mesh", 56);
  let t = ent - 4; // offsets from the type tag, as in the type's fields
  let mesh = w.basic("nav-mesh", 112);
  let mm = mesh - 4;
  w.f32(t + 44, 1.0); // trans w (trans at the origin: the mesh has its own)
  w.u32(t + 48, e.aid);
  w.ptr(t + 52, mesh);

  // the route table
  let route = navmesh_route_table(m);
  let route_at = w.reserve(route.len());
  for (i, &b) in route.iter().enumerate() {
    w.u8(route_at + i, b);
  }

  // the grid-hash of the polygons and its buckets
  let g = navmesh_grid(m);
  let grid = w.basic("grid-hash", 88);
  let gg = grid - 4;
  w.u16(gg + 14, g.bucket_size as u16);
  for k in 0..3 {
    w.f32(gg + 16 + 4 * k, g.axis_scale[k]);
    w.u8(gg + 28 + k, g.dims[k] as u8);
    w.f32(gg + 36 + 4 * k, g.box_min[k]);
    w.f32(gg + 48 + 4 * k, g.box_max[k]);
  }
  w.u8(gg + 31, 1); // vertical-cell-count
  w.u16(gg + 60, n as u16);
  w.u16(gg + 62, g.bucket_count as u16);
  w.f32(gg + 64, K_MIN_CELL);
  w.u32(gg + 68, g.buckets.len() as u32);
  w.u32(gg + 76, K_SCRATCHPAD);
  w.sym(gg + 80, "#f");
  w.sym(gg + 84, "#f");
  let buckets = w.reserve(g.buckets.len());
  for (i, &b) in g.buckets.iter().enumerate() {
    w.u8(buckets + i, b);
  }
  w.ptr(gg + 32, buckets);
  w.ptr(gg + 72, buckets);

  // the polygons: 4 corners of 16 bytes, their w words holding id, pat, count, link, the adjacent
  // polygons, min-y and max-y
  let polys = w.reserve(64 * n);
  for (i, p) in m.polys.iter().enumerate() {
    let q = polys + 64 * i;
    let (mut lo, mut hi) = (1e30f32, -1e30f32);
    for k in 0..4 {
      let v = m.verts[p.v[if k < p.count { k } else { 0 }]] * K_UNITS; // a triangle repeats its first corner
      w.f32(q + 16 * k, v.x);
      w.f32(q + 16 * k + 4, v.y);
      w.f32(q + 16 * k + 8, v.z);
      if k < p.count {
        lo = lo.min(v.y);
        hi = hi.max(v.y);
      }
    }
    w.u8(q + 12, i as u8);
    w.u8(q + 13, p.pat);
    w.u8(q + 14, p.count as u8);
    w.u8(q + 15, 255); // link: none
    for k in 0..4 {
      let a = if k < p.count && p.adj[k] >= 0 {
        p.adj[k] as u8
      } else {
        255
      };
      w.u8(q + 28 + k, a);
    }
    w.f32(q + 44, lo);
    w.f32(q + 60, hi);
  }

  // res tags (name, nearest-y-threshold, nav-max-users) and their data, then the name: name,
  // type, offset in the data, inlined
  let mut tags = [
    ("name", "string", 0u32, false),
    ("nearest-y-threshold", "float", 4, true),
    ("nav-max-users", "int32", 8, true),
  ];
  tags.sort_by_key(|tag| res_key(tag.0)); // stable
  let tag_at = w.reserve(16 * tags.len());
  for (i, &(name, ty, offset, inlined)) in tags.iter().enumerate() {
    let tg = tag_at + 16 * i;
    w.sym(tg, name);
    w.u32(tg + 4, K_DEFAULT_RES_TIME);
    w.ty(tg + 8, ty);
    w.u32(
      tg + 12,
      offset | (1 << 16) | if inlined { 0x80000000 } else { 0 },
    );
  }
  let data = w.reserve(16);
  w.f32(data + 4, K_NEAREST_Y_THRESHOLD * K_UNITS);
  w.u32(data + 8, K_MAX_USERS);
  // the string: type tag, length, characters
  let str_at = w.basic("string", 8 + e.name.len() + 1);
  w.u32(str_at, e.name.len() as u32);
  for (i, &c) in e.name.as_bytes().iter().enumerate() {
    w.u8(str_at + 4 + i, c);
  }
  w.ptr(data, str_at);
  w.align16();

  w.u32(t + 4, tags.len() as u32); // length
  w.u32(t + 8, tags.len() as u32); // allocated-length
  w.ptr(t + 12, data); // data-base
  w.ptr(t + 16, data + 16); // data-top
  w.u32(t + 20, 16); // data-size
  w.ptr(t + 28, tag_at); // tags

  // the mesh
  w.ptr(mm + 8, polys);
  w.u8(mm + 12, 0); // static-sphere-count (the game reads them from 'nav-mesh-sphere)
  w.u8(mm + 13, n as u8);
  w.ptr(mm + 16, route_at);
  w.ptr(mm + 20, grid);
  let o = m.origin * K_UNITS;
  w.f32(mm + 48, o.x);
  w.f32(mm + 52, o.y);
  w.f32(mm + 56, o.z);
  w.f32(mm + 60, radius * K_UNITS);
  ent
}

pub fn navmesh_grid(m: &NavMeshData) -> NavGrid {
  let mut g = NavGrid::default();
  let n = m.polys.len();
  let mut vb = AABB::default();
  for &v in &m.verts {
    vb.add(v * K_UNITS);
  }
  if !vb.valid() || n == 0 {
    return g;
  }
  // the box written reaches the farthest corner on each side of the origin (the corners' own box
  // when the origin is their center); the cells divide it widened by a quarter meter on each side
  let mut size = [0f32; 3];
  for k in 0..3 {
    let half = vb.lo[k].abs().max(vb.hi[k].abs());
    g.box_min[k] = -half;
    g.box_max[k] = half;
    size[k] = K_MIN_CELL.max(2.0 * half + 2.0 * K_GRID_MARGIN);
  }
  // about one cell per polygon, the cells as square as the box allows (the rule the game's own
  // nav meshes follow)
  let n = n as i32;
  g.bucket_size = (n + 7) / 8;
  let f = (n as f32 / (size[0] * size[2])).sqrt();
  let cap = 126.min(n);
  g.dims[0] = ((f * size[0]) as i32).min(cap).max(1);
  g.dims[1] = 1;
  g.dims[2] = ((f * size[2]) as i32).min(cap).max(1);
  for k in [0, 2] {
    g.dims[k] = g.dims[k].min(((size[k] / K_MIN_CELL) as i32).max(1));
  }
  for k in 0..3 {
    g.axis_scale[k] = g.dims[k] as f32 / size[k];
  }
  g.axis_scale[1] = 0.0; // one layer: every height falls in it
  g.bucket_count = g.dims[0] * g.dims[1] * g.dims[2];
  g.buckets = vec![0; (g.bucket_count * g.bucket_size) as usize];
  let cell = |p: f32, k: usize| {
    ((p - g.box_min[k]) * g.axis_scale[k])
      .min((g.dims[k] - 1) as f32)
      .max(0.0) as i32
  };
  for (i, p) in m.polys.iter().enumerate() {
    if p.pat & 1 != 0 {
      continue;
    }
    let mut b = AABB::default();
    for k in 0..p.count {
      b.add(m.verts[p.v[k]] * K_UNITS);
    }
    for y in cell(b.lo.y, 1)..=cell(b.hi.y, 1) {
      for z in cell(b.lo.z, 2)..=cell(b.hi.z, 2) {
        for x in cell(b.lo.x, 0)..=cell(b.hi.x, 0) {
          let c = (x + g.dims[0] * (z + g.dims[2] * y)) as usize;
          g.buckets[c * g.bucket_size as usize + i / 8] |= 1 << (i % 8);
        }
      }
    }
  }
  g
}

/// The nav_data blob of these meshes (nearest-y-threshold K_NEAREST_Y_THRESHOLD, nav-max-users
/// K_MAX_USERS).
pub fn navmesh_blob(meshes: &[NavExportEntry]) -> Value {
  let mut w = BlobWriter::default();
  let mut roots = BTreeMap::new();
  for (i, e) in meshes.iter().enumerate() {
    roots.insert(format!("nav-mesh-{i}"), write_mesh(&mut w, e));
  }
  json!({
    "words": w.words,
    "pointers": w.pointers,
    "types": w.types,
    "symbols": w.symbols,
    "empty_lists": [],
    "roots": roots,
  })
}

/// Writes the blob, once every mesh suits the game.
pub fn write_navmesh_blob(meshes: &[NavExportEntry], path: &Path) -> Result<(), String> {
  for e in meshes {
    if let Some(problem) = navmesh_problem(e.mesh) {
      return Err(format!("{}: {problem}", e.name));
    }
  }
  std::fs::write(path, navmesh_blob(meshes).to_string())
    .map_err(|_| format!("cannot write {}", path.display()))
}

/// The document's nav meshes for the game (App::export_navmeshes): each where it is now, the ids
/// from `first_aid` on. Returns how many were written.
pub fn export_navmeshes(scene: &Scene, path: &Path, first_aid: u32) -> Result<usize, String> {
  let (meshes, names) = world_navmeshes(scene);
  if meshes.is_empty() {
    return Err(tr("navmesh.none").to_string());
  }
  let entries: Vec<NavExportEntry> = meshes
    .iter()
    .zip(names)
    .enumerate()
    .map(|(i, (mesh, name))| NavExportEntry {
      mesh,
      aid: first_aid + i as u32,
      name,
    })
    .collect();
  write_navmesh_blob(&entries, path)?;
  log_info!(
    tr("log.navmesh_exported"),
    meshes.len(),
    path.display().to_string()
  );
  Ok(meshes.len())
}

/// A blob read back (tests): words, pointers (word -> byte), roots.
#[derive(Clone, Debug, Default)]
pub struct NavBlobView {
  pub words: Vec<u32>,
  pub pointers: Vec<(usize, usize)>,
  pub types: Vec<(usize, String)>,
  pub symbols: Vec<(usize, String)>,
  /// sorted by name
  pub roots: Vec<(String, usize)>,
}

impl NavBlobView {
  pub fn u32(&self, byte: usize) -> u32 {
    self.words[byte / 4]
  }
  pub fn u8(&self, byte: usize) -> u8 {
    (self.words[byte / 4] >> (8 * (byte % 4))) as u8
  }
  pub fn f32(&self, byte: usize) -> f32 {
    f32::from_bits(self.u32(byte))
  }
  /// None when that word is no pointer
  pub fn ptr(&self, byte: usize) -> Option<usize> {
    self
      .pointers
      .iter()
      .find(|(word, _)| *word == byte / 4)
      .map(|&(_, to)| to)
  }
}

pub fn read_navmesh_blob(j: &Value) -> Option<NavBlobView> {
  let pairs = |key: &str| -> Option<Vec<(usize, Value)>> {
    j.get(key)?
      .as_array()?
      .iter()
      .map(|p| Some((p.get(0)?.as_u64()? as usize, p.get(1)?.clone())))
      .collect()
  };
  let named = |key: &str| -> Option<Vec<(usize, String)>> {
    pairs(key)?
      .into_iter()
      .map(|(w, s)| Some((w, s.as_str()?.to_string())))
      .collect()
  };
  let mut roots: Vec<(String, usize)> = j
    .get("roots")?
    .as_object()?
    .iter()
    .map(|(name, byte)| Some((name.clone(), byte.as_u64()? as usize)))
    .collect::<Option<_>>()?;
  roots.sort(); // as a json object of the C++ editor lists them, whatever serde_json keeps
  Some(NavBlobView {
    words: j
      .get("words")?
      .as_array()?
      .iter()
      .map(|w| w.as_u64().map(|w| w as u32))
      .collect::<Option<_>>()?,
    pointers: pairs("pointers")?
      .into_iter()
      .map(|(w, b)| Some((w, b.as_u64()? as usize)))
      .collect::<Option<_>>()?,
    types: named("types")?,
    symbols: named("symbols")?,
    roots,
  })
}

/// The meshes of a blob (the game's, from decompiler_out/<game>/entities/<level>-nav.json, or ours).
pub fn navmeshes_of_blob(b: &NavBlobView) -> Option<Vec<NavMeshData>> {
  let mut out = Vec::new();
  for (name, ent) in &b.roots {
    if !name.starts_with("nav-mesh-") {
      continue;
    }
    let mesh = b.ptr(ent + 48)?;
    let polys = b.ptr(mesh + 4)?;
    let n = b.u8(mesh + 9) as usize;
    let mut m = NavMeshData {
      origin: Vec3::new(b.f32(mesh + 44), b.f32(mesh + 48), b.f32(mesh + 52)) / K_UNITS,
      ..Default::default()
    };
    // shared corners
    let mut index: BTreeMap<(u32, u32, u32), usize> = BTreeMap::new();
    for i in 0..n {
      let q = polys + 64 * i;
      let mut p = NavPoly {
        count: b.u8(q + 14) as usize,
        pat: b.u8(q + 13),
        ..Default::default()
      };
      if p.count < 3 || p.count > 4 {
        return None;
      }
      for k in 0..p.count {
        let key = (
          b.u32(q + 16 * k),
          b.u32(q + 16 * k + 4),
          b.u32(q + 16 * k + 8),
        );
        p.v[k] = *index.entry(key).or_insert_with(|| {
          m.verts.push(
            Vec3::new(
              b.f32(q + 16 * k),
              b.f32(q + 16 * k + 4),
              b.f32(q + 16 * k + 8),
            ) / K_UNITS,
          );
          m.verts.len() - 1
        });
        let a = b.u8(q + 28 + k);
        p.adj[k] = if a == 255 { -1 } else { a as i32 };
      }
      m.polys.push(p);
    }
    out.push(m);
  }
  Some(out)
}
