//! The level as the game draws it (Jak 3), for the viewport's game view (src/game_view/game_view.cpp).
//! The document's decor is made into what OpenGOAL's level builder makes of a custom level (tfrag
//! trees: the triangles of every piece in the game's units, a draw per material, the 8 palettes of
//! each vertex: the export writes the same), which the interface draws with the game's own
//! TFragment shader (ui/render/game_view.ts), the game's camera, its time of the day (game/mood.rs)
//! and its fog.
//!
//! The level is made in chunks: the pieces sharing a palette (a tree of an extracted level, or the
//! new geometry) in a cell of the map. An edit makes the chunks it touched again, alone.
//! Actors' models, the levels shown for reference and the editor's overlays are the editor's to
//! draw, with the game's camera and depth.
//!
//! The interface reads them on the jle:// protocol (docs/rust-architecture.md §4, "Game view"):
//! `gameview` (update, then encode_chunk_list), `gameview/chunk/<id>` (encode_chunk),
//! `gameview/palette/<key>` (encode_palette).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use serde_json::Value;

use crate::math::{AABB, Vec3, Vec4};
use crate::scene::{
  AlphaMode, BlendMode, Material, NodeKind, Scene, SceneCache, TodPalette, tod_from_color,
  tod_ratios, translucent_in_game,
};

const K_UNITS: f32 = 4096.0; // game units per meter
const K_CELL: f32 = 128.0; // meters: the side of a chunk's cell
const K_MAX_COLORS: usize = 8192; // per tree (TFragment's TIME_OF_DAY_COLOR_COUNT)

const FNV_OFFSET: u64 = 1469598103934665603;

fn fnv(mut h: u64, data: &[u8]) -> u64 {
  for &b in data {
    h = (h ^ b as u64).wrapping_mul(1099511628211);
  }
  h
}

// chunk versions and palette keys: unique for the life of the program, so that what the interface
// keeps by them is never taken for something else
static NEXT_SERIAL: AtomicU32 = AtomicU32::new(1);

fn serial() -> u32 {
  NEXT_SERIAL.fetch_add(1, Ordering::Relaxed)
}

/// How a blended surface mixes with what is behind it: the PS2's blends the level builder gives a
/// material (DrawMode::AlphaBlend), drawn as background_common.cpp setup_opengl_from_draw_mode does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum AlphaBlend {
  #[default]
  Disabled = 0,
  /// (src - dst) * src alpha + dst: alpha blending
  SrcDstSrcDst,
  /// src * src alpha + dst
  Src0SrcDst,
  /// src + dst
  Src0FixDst,
  /// dst - src * src alpha
  ZeroSrcSrcDst,
  /// (src - dst) * 0.5 + dst
  SrcDstFixDst,
}

/// What the game's renderer does with a draw (the depth test is always GEQUAL, the game's).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawMode {
  pub depth_write: bool,
  /// the alpha test (GEQUAL, fail KEEP): what is under aref / 127 is not drawn
  pub alpha_test: bool,
  pub aref: u8,
  pub blend: AlphaBlend,
}

/// A material as the level builder draws it (gltf_util.cpp: make_default_draw_mode,
/// setup_alpha_from_material, setup_draw_mode_from_sampler) from what the export writes of it
/// (gltf_io.cpp: a game material's alpha reference doubled; blended surfaces in the translucent
/// tree, with their PS2 blend, og_blend). Also: whether it goes in the translucent tree.
pub fn draw_mode(m: Option<&Material>) -> (DrawMode, bool) {
  let mut mode = DrawMode {
    depth_write: true,
    ..Default::default()
  };
  let Some(m) = m else {
    return (mode, false);
  };
  let cutoff = if m.base_color.w > 1.5 {
    (m.cutoff * 2.0).min(1.0)
  } else {
    m.cutoff
  };
  let mut alpha_test = || {
    mode.alpha_test = true;
    mode.aref = (cutoff * 127.0).clamp(0.0, 255.0) as u8;
  };
  match m.alpha {
    AlphaMode::Opaque => (mode, false),
    AlphaMode::Mask => {
      alpha_test();
      (mode, false)
    }
    // a wall the game blends and writes the depth of: the export's (masked)
    AlphaMode::Blend if !translucent_in_game(m) => {
      alpha_test();
      (mode, false)
    }
    AlphaMode::Blend => {
      alpha_test();
      mode.depth_write = false;
      mode.blend = match m.blend {
        BlendMode::Add => AlphaBlend::Src0FixDst,
        BlendMode::AddAlpha => AlphaBlend::Src0SrcDst,
        BlendMode::Subtract => AlphaBlend::ZeroSrcSrcDst,
        BlendMode::Half => AlphaBlend::SrcDstFixDst,
        BlendMode::Alpha => AlphaBlend::SrcDstSrcDst,
      };
      (mode, true)
    }
  }
}

/// A vertex as the game's renderer takes it (tfrag3::PreloadedVertex): in the game's units, its
/// uv, its color in the chunk's palette.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
  pub pos: [f32; 3],
  pub uv: [f32; 2],
  pub color: u16,
}

/// The triangles of a material in a tree of the chunk.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
  /// into the chunk's indices
  pub first: u32,
  pub count: u32,
  /// the document's texture, -1 a white one (alpha 128: the PS2's opaque)
  pub texture: i32,
  /// in the translucent tree: drawn after the editor's opaque models, the farthest chunk first
  pub translucent: bool,
  /// a texture of the editor's own, drawn the game's way: its alpha halved (the game's textures
  /// have the PS2's, 128 is opaque)
  pub own_texture: bool,
  pub mode: DrawMode,
}

/// A chunk: its trees as one vertex and index buffer, the normal tree's draws then the
/// translucent tree's.
#[derive(Clone, Debug)]
pub struct Chunk {
  pub id: u32,
  /// a new one each time it is made (unique for the life of the program)
  pub version: u32,
  /// its palette: encode_palette(palette)
  pub palette: u32,
  /// in meters
  pub bounds: AABB,
  pub triangles: usize,
  pub vertices: Vec<Vertex>,
  pub indices: Vec<u32>,
  pub draws: Vec<Draw>,
  signature: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
  pub chunks: usize,
  pub triangles: usize,
  pub rebuilt: usize,
  /// the last chunks made
  pub ms_build: f64,
}

// the palette (its address, 0 for the new geometry's), the cell (x, z)
type Key = (usize, i32, i32);

/// The game view's chunks of a document, made again where it changed (`update`).
#[derive(Default)]
pub struct GameViewChunks {
  chunks: BTreeMap<Key, Chunk>,
  // by key: the levels' palettes the chunks share, the chunks' own
  palettes: HashMap<u32, Arc<TodPalette>>,
  level_palettes: HashMap<usize, u32>,
  next_chunk: u32,
  // what the chunks were last made from: the scene's cache, its nodes, the materials
  seen: Option<(u64, usize, u32, u32, u64)>,
  pub stats: Stats,
}

// an invisible wall (its og_decor collision "invisible"): the game does not draw it (the editor's
// own view does, to edit it)
fn invisible(extras: &Value) -> bool {
  extras
    .pointer("/og_decor/collision/kind")
    .and_then(Value::as_str)
    == Some("invisible")
}

fn material(scene: &Scene, i: i32) -> Option<&Material> {
  usize::try_from(i).ok().and_then(|i| scene.materials.get(i))
}

struct Built {
  vertices: Vec<Vertex>,
  indices: Vec<u32>,
  draws: Vec<Draw>,
  bounds: AABB,
  triangles: usize,
  // the palette of the new geometry (None: the level's)
  synth: Option<Vec<u8>>,
}

// the chunk's trees, as the level builder makes them (Tfrag.cpp, gltf_mesh_extract.cpp): its nodes'
// triangles in the game's space, one draw per material, a palette of 8 colors per vertex color; the
// opaque and masked surfaces in a normal tree, the blended ones in a translucent one
fn build(
  nodes: &[u32],
  palette: Option<&Arc<TodPalette>>,
  scene: &Scene,
  cache: &SceneCache,
  ratios: &[f32; 8],
  own_textures: &[bool],
) -> Built {
  // a level's palette larger than a tree of the renderer takes (the decor instances of a whole
  // desert): the colors the chunk uses, as the new geometry's
  let compact = palette.is_some_and(|p| p.size() > K_MAX_COLORS);
  let shared = palette.filter(|_| !compact);
  let make = |shift: u32| {
    let mut verts: Vec<Vertex> = Vec::new();
    let mut by_material: [BTreeMap<i32, Vec<u32>>; 2] = Default::default(); // [translucent]
    let mut synth: Vec<u8> = Vec::new(); // the palette of the new geometry
    let mut synth_index: HashMap<[u8; 32], u16> = HashMap::new();
    let mut bounds = AABB::default();
    for &i in nodes {
      let n = &scene.nodes[i as usize];
      let Some(mesh) = &n.mesh else { continue };
      let w = &cache.worlds[i as usize];
      let tod = n.tod.as_deref().filter(|t| t.fits(mesh));
      let lit =
        shared.is_some_and(|s| tod.is_some_and(|t| Arc::ptr_eq(t.palette.as_ref().unwrap(), s)));
      let own = !lit && tod.is_some_and(|t| t.palette.as_ref().unwrap().local || compact);
      for (pi, p) in mesh.prims.iter().enumerate() {
        if p.idx.is_empty() {
          continue;
        }
        let mat = material(scene, p.material);
        let (_, trans) = draw_mode(mat);
        let base = verts.len() as u32;
        for v in 0..p.pos.len() {
          let q = w.point(p.pos[v]);
          bounds.add(q);
          let color = if lit {
            tod.unwrap().index[pi][v]
          } else {
            // new geometry: its own palettes (lit like the level around it), else lit like the
            // level's from the color shown (game_export's)
            let own_col = tod.filter(|_| own).and_then(|t| {
              let at = t.index[pi][v] as usize * 32;
              t.palette
                .as_ref()
                .unwrap()
                .rgba
                .get(at..at + 32)?
                .try_into()
                .ok()
            });
            let mut col: [u8; 32] = own_col.unwrap_or_else(|| {
              let mut shown = p
                .col
                .get(v)
                .copied()
                .unwrap_or(Vec4::new(1.0, 1.0, 1.0, 1.0));
              if let Some(m) = mat {
                shown = Vec4::new(
                  shown.x * m.base_color.x,
                  shown.y * m.base_color.y,
                  shown.z * m.base_color.z,
                  shown.w,
                );
              }
              tod_from_color(shown, ratios)
            });
            if shift > 0 {
              for b in &mut col {
                *b = (*b >> shift) << shift;
              }
            }
            let next = synth_index.len() as u16;
            *synth_index.entry(col).or_insert_with(|| {
              synth.extend_from_slice(&col);
              next
            })
          };
          let uv = p.uv.get(v).map_or([0.0, 0.0], |t| [t.x, t.y]);
          verts.push(Vertex {
            pos: [q.x * K_UNITS, q.y * K_UNITS, q.z * K_UNITS],
            uv,
            color,
          });
        }
        by_material[trans as usize]
          .entry(p.material)
          .or_default()
          .extend(p.idx.iter().map(|ix| base + ix));
      }
    }
    (verts, by_material, synth, synth_index.len(), bounds)
  };
  let mut shift = 0;
  let (vertices, by_material, synth, _, bounds) = loop {
    let made = make(shift);
    // the bits of the colors dropped when they are too many for a palette
    if shared.is_some() || made.3 <= K_MAX_COLORS || shift >= 6 {
      break made;
    }
    shift += 1;
  };

  let mut indices = Vec::new();
  let mut draws = Vec::new();
  let mut triangles = 0;
  for (t, tree) in by_material.iter().enumerate() {
    for (&mi, idx) in tree {
      let mat = material(scene, mi);
      let (mode, _) = draw_mode(mat);
      let texture = mat
        .map(|m| m.texture)
        .filter(|&t| t >= 0 && (t as usize) < scene.textures.len())
        .unwrap_or(-1);
      draws.push(Draw {
        first: indices.len() as u32,
        count: idx.len() as u32,
        texture,
        translucent: t == 1,
        own_texture: texture >= 0 && own_textures[texture as usize],
        mode,
      });
      triangles += idx.len() / 3;
      indices.extend_from_slice(idx);
    }
  }
  Built {
    vertices,
    indices,
    draws,
    bounds,
    triangles,
    synth: shared.is_none().then_some(synth),
  }
}

impl GameViewChunks {
  /// A new document: its chunks are made again.
  pub fn reset(&mut self) {
    self.chunks.clear();
    self.palettes.clear();
    self.level_palettes.clear();
    self.seen = None;
  }

  /// The chunks the scene's decor falls in, made again where it changed: the ids of the chunks
  /// made. The others keep their version; those whose decor is gone are no more.
  pub fn update(&mut self, scene: &Scene) -> Vec<u32> {
    let cache = scene.cache();
    let mut mh = FNV_OFFSET;
    for mat in &scene.materials {
      mh = fnv(mh, &mat.texture.to_le_bytes());
      for c in 0..4 {
        mh = fnv(mh, &mat.base_color[c].to_le_bytes());
      }
      mh = fnv(
        mh,
        &[mat.alpha as u8, mat.blend as u8, mat.translucent as u8],
      );
      mh = fnv(mh, &mat.cutoff.to_le_bytes());
    }
    mh = fnv(mh, &(scene.textures.len() as u64).to_le_bytes());
    let seen = (
      cache.version,
      cache.count,
      cache.first_id,
      cache.last_id,
      mh,
    );
    if self.seen == Some(seen) {
      return Vec::new();
    }
    self.seen = Some(seen);
    let t0 = Instant::now();
    // which textures are the editor's own (their alpha drawn halved): those of its materials (a
    // game material is drawn twice as bright: base_color 2, as the extractor writes them)
    let mut own_textures = vec![false; scene.textures.len()];
    for mat in &scene.materials {
      if mat.base_color.w <= 1.5
        && let Some(o) = usize::try_from(mat.texture)
          .ok()
          .and_then(|t| own_textures.get_mut(t))
      {
        *o = true;
      }
    }

    // ponytail: every node is hashed again after any change (a few ms for a city); a revision
    // per node in the scene cache would hash only the changed ones
    let mut members: BTreeMap<Key, Vec<u32>> = BTreeMap::new();
    let count = scene.nodes.len().min(cache.visible.len());
    for i in 0..count {
      let n = &scene.nodes[i];
      let Some(mesh) = &n.mesh else { continue };
      if n.kind != NodeKind::Render || !cache.visible[i] || cache.actor[i] != 0 {
        continue;
      }
      if cache.reference.get(i) == Some(&true) || invisible(&n.extras) {
        continue;
      }
      let pal = n
        .tod
        .as_ref()
        .filter(|t| t.fits(mesh))
        .and_then(|t| t.palette.as_ref())
        .filter(|p| !p.local)
        .map_or(0, |p| Arc::as_ptr(p) as usize);
      let at = match cache.bounds.get(i) {
        Some(b) if b.valid() => b.center(),
        _ => cache.worlds[i].point(Vec3::new(0.0, 0.0, 0.0)),
      };
      let cell = |v: f32| (v / K_CELL).floor() as i32;
      members
        .entry((pal, cell(at.x), cell(at.z)))
        .or_default()
        .push(i as u32);
    }
    self.chunks.retain(|k, _| members.contains_key(k));

    let mut ratios: Option<[f32; 8]> = None;
    let mut rebuilt = Vec::new();
    for (key, nodes) in &members {
      let mut sig = mh;
      for &i in nodes {
        let n = &scene.nodes[i as usize];
        let mesh = n.mesh.as_ref().unwrap();
        sig = fnv(sig, &n.id.to_le_bytes());
        sig = fnv(sig, &mesh.uid.to_le_bytes());
        sig = fnv(sig, &mesh.version.to_le_bytes());
        for v in cache.worlds[i as usize].m {
          sig = fnv(sig, &v.to_le_bytes());
        }
        let tod = n.tod.as_ref().map_or(0, |t| Arc::as_ptr(t) as usize);
        sig = fnv(sig, &tod.to_le_bytes());
      }
      if self.chunks.get(key).is_some_and(|c| c.signature == sig) {
        continue;
      }
      let palette = (key.0 != 0).then(|| {
        let tod = scene.nodes[nodes[0] as usize].tod.as_ref().unwrap();
        tod.palette.clone().unwrap()
      });
      let ratios = if key.0 == 0 {
        *ratios.get_or_insert_with(|| {
          let ids: Vec<u32> = scene.nodes[..count].iter().map(|n| n.id).collect();
          tod_ratios(scene, &ids)
        })
      } else {
        [0.0; 8]
      };
      let b = build(
        nodes,
        palette.as_ref(),
        scene,
        &cache,
        &ratios,
        &own_textures,
      );
      let palette_key = match (b.synth, palette) {
        (None, Some(p)) => {
          let k = *self.level_palettes.entry(key.0).or_insert_with(serial);
          self.palettes.insert(k, p);
          k
        }
        (synth, _) => {
          let k = serial();
          let rgba = synth.unwrap_or_default();
          self
            .palettes
            .insert(k, Arc::new(TodPalette { rgba, local: true }));
          k
        }
      };
      let id = self.chunks.get(key).map(|c| c.id).unwrap_or_else(|| {
        self.next_chunk += 1;
        self.next_chunk
      });
      self.chunks.insert(
        *key,
        Chunk {
          id,
          version: serial(),
          palette: palette_key,
          bounds: b.bounds,
          triangles: b.triangles,
          vertices: b.vertices,
          indices: b.indices,
          draws: b.draws,
          signature: sig,
        },
      );
      rebuilt.push(id);
    }
    // the palettes no chunk uses any more
    let used: HashSet<u32> = self.chunks.values().map(|c| c.palette).collect();
    self.palettes.retain(|k, _| used.contains(k));
    self.level_palettes.retain(|_, k| used.contains(k));

    self.stats.chunks = self.chunks.len();
    self.stats.triangles = self.chunks.values().map(|c| c.triangles).sum();
    self.stats.rebuilt = rebuilt.len();
    if !rebuilt.is_empty() {
      self.stats.ms_build = t0.elapsed().as_secs_f64() * 1000.0;
    }
    rebuilt
  }

  pub fn chunks(&self) -> impl Iterator<Item = &Chunk> {
    self.chunks.values()
  }

  pub fn chunk(&self, id: u32) -> Option<&Chunk> {
    self.chunks.values().find(|c| c.id == id)
  }

  /// `gameview` (format JLGL 1): `u32 magic "JLGL"`, `u32 version`, `u32 count`, then per chunk
  /// `u32 id, u32 version, u32 palette, u32 triangles, f32 lo[3], f32 hi[3]` (its box in meters).
  pub fn encode_chunk_list(&self) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + self.chunks.len() * 40);
    out.extend_from_slice(b"JLGL");
    put_u32(&mut out, 1);
    put_u32(&mut out, self.chunks.len() as u32);
    for c in self.chunks.values() {
      for v in [c.id, c.version, c.palette, c.triangles as u32] {
        put_u32(&mut out, v);
      }
      put_box(&mut out, &c.bounds);
    }
    out
  }

  /// `gameview/chunk/<id>` (format JLGC 1): `u32 magic "JLGC"`, `u32 version (1)`, `u32 id`,
  /// `u32 chunk version`, `u32 palette`, `u32 vertices, u32 indices, u32 draws`, `f32 lo[3],
  /// hi[3]`, then the draws (16 bytes each: `u32 first, u32 count, i32 texture (-1 white), u8
  /// flags (1 translucent, 2 depth write, 4 alpha test, 8 own texture), u8 aref, u8 blend
  /// (AlphaBlend), u8 0`), the vertices (24 bytes each: `f32 x, y, z` in game units, `f32 s, t`,
  /// `u16 color, u16 0`), the indices (`u32`).
  pub fn encode_chunk(&self, id: u32) -> Option<Vec<u8>> {
    let c = self.chunk(id)?;
    let mut out =
      Vec::with_capacity(56 + c.draws.len() * 16 + c.vertices.len() * 24 + c.indices.len() * 4);
    out.extend_from_slice(b"JLGC");
    for v in [
      1,
      c.id,
      c.version,
      c.palette,
      c.vertices.len() as u32,
      c.indices.len() as u32,
      c.draws.len() as u32,
    ] {
      put_u32(&mut out, v);
    }
    put_box(&mut out, &c.bounds);
    for d in &c.draws {
      put_u32(&mut out, d.first);
      put_u32(&mut out, d.count);
      out.extend_from_slice(&d.texture.to_le_bytes());
      let flags = d.translucent as u8
        | (d.mode.depth_write as u8) << 1
        | (d.mode.alpha_test as u8) << 2
        | (d.own_texture as u8) << 3;
      out.extend_from_slice(&[flags, d.mode.aref, d.mode.blend as u8, 0]);
    }
    for v in &c.vertices {
      for f in v.pos.iter().chain(&v.uv) {
        out.extend_from_slice(&f.to_le_bytes());
      }
      out.extend_from_slice(&v.color.to_le_bytes());
      out.extend_from_slice(&[0, 0]);
    }
    for &i in &c.indices {
      put_u32(&mut out, i);
    }
    Some(out)
  }

  /// `gameview/palette/<key>` (format JLGP 1): `u32 magic "JLGP"`, `u32 version (1)`, `u32
  /// colors`, then 32 bytes per color: its 8 palettes' RGBA (128 is 1.0), palette k at k * 4.
  pub fn encode_palette(&self, key: u32) -> Option<Vec<u8>> {
    let p = self.palettes.get(&key)?;
    let mut out = Vec::with_capacity(12 + p.size() * 32);
    out.extend_from_slice(b"JLGP");
    put_u32(&mut out, 1);
    put_u32(&mut out, p.size() as u32);
    out.extend_from_slice(&p.rgba[..p.size() * 32]);
    Some(out)
  }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
  out.extend_from_slice(&v.to_le_bytes());
}

fn put_box(out: &mut Vec<u8>, b: &AABB) {
  for v in [b.lo.x, b.lo.y, b.lo.z, b.hi.x, b.hi.y, b.hi.z] {
    out.extend_from_slice(&v.to_le_bytes());
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::math::Vec2;
  use crate::scene::{Mesh, Node, Primitive, Texture, TodColors};

  fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
  }
  fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
  }

  fn quad(size: f32, material: i32) -> Arc<Mesh> {
    let mut mesh = Mesh::new();
    let mut p = Primitive::default();
    let s = size / 2.0;
    p.pos = vec![
      Vec3::new(-s, 0.0, -s),
      Vec3::new(s, 0.0, -s),
      Vec3::new(s, 0.0, s),
      Vec3::new(-s, 0.0, s),
    ];
    p.uv = vec![
      Vec2::new(0.0, 0.0),
      Vec2::new(1.0, 0.0),
      Vec2::new(1.0, 1.0),
      Vec2::new(0.0, 1.0),
    ];
    p.idx = vec![0, 1, 2, 0, 2, 3];
    p.material = material;
    p.compute_normals();
    mesh.prims.push(p);
    Arc::new(mesh)
  }

  // selftest.cpp's game view: a ground lit by a palette of a level (the ambient light alone), a
  // box of new geometry lit from its color, a glass pane of the editor's
  fn document() -> (Scene, u32, u32) {
    let mut doc = Scene::new();
    let white = doc.add_texture(Texture {
      name: "white".into(),
      width: 2,
      height: 2,
      rgba: vec![255; 16],
      ..Default::default()
    });
    let ground_mat = doc.add_material(Material {
      texture: white,
      base_color: Vec4::new(2.0, 2.0, 2.0, 2.0), // a game material
      ..Default::default()
    });
    let pale = doc.add_texture(Texture {
      name: "pale".into(),
      width: 1,
      height: 1,
      rgba: vec![200; 4],
      ..Default::default()
    });
    let glass = doc.add_material(Material {
      texture: pale,
      base_color: Vec4::new(0.6, 0.8, 1.0, 0.4),
      alpha: AlphaMode::Blend,
      ..Default::default()
    });
    let mut palette = TodPalette {
      rgba: vec![0; 64],
      local: false,
    };
    palette.rgba[..4].copy_from_slice(&[128; 4]); // color 0: the ambient light alone, 1.0
    palette.rgba[32 + 20..32 + 24].copy_from_slice(&[255, 128, 0, 128]); // color 1: a lamp
    let ground = doc.add(Node {
      kind: NodeKind::Render,
      name: "ground".into(),
      mesh: Some(quad(40.0, ground_mat)),
      tod: Some(Arc::new(TodColors {
        palette: Some(Arc::new(palette)),
        index: vec![vec![0, 0, 1, 0]],
      })),
      ..Default::default()
    });
    let mut box_node = Node {
      kind: NodeKind::Render,
      name: "box".into(),
      mesh: Some(quad(2.0, -1)),
      ..Default::default()
    };
    box_node.local.t = Vec3::new(0.0, 1.0, 0.0);
    let box_id = doc.add(box_node);
    let mut pane = Node {
      kind: NodeKind::Render,
      name: "pane".into(),
      mesh: Some(quad(1.0, glass)),
      ..Default::default()
    };
    pane.local.t = Vec3::new(3.0, 1.0, 0.0);
    doc.add(pane);
    let mut wall = Node {
      kind: NodeKind::Render,
      name: "invisible wall".into(),
      mesh: Some(quad(1.0, -1)),
      ..Default::default()
    };
    wall.extras = serde_json::json!({"og_decor": {"collision": {"kind": "invisible"}}});
    doc.add(wall);
    doc.begin_frame();
    (doc, ground, box_id)
  }

  #[test]
  fn chunks_by_palette_and_cell() {
    let (mut doc, ground, box_id) = document();
    let mut gv = GameViewChunks::default();
    let made = gv.update(&doc);
    // the level's palette, the new geometry's (the box and the pane: the wall is not drawn)
    assert_eq!(made.len(), 2);
    assert_eq!(gv.stats.chunks, 2);
    assert_eq!(gv.stats.triangles, 6);
    let level = gv.chunks().find(|c| c.vertices.len() == 4).unwrap().clone();
    let new = gv.chunks().find(|c| c.vertices.len() == 8).unwrap().clone();
    assert_eq!(level.vertices[2].color, 1);
    assert_eq!(
      level.vertices[0].pos,
      [-20.0 * K_UNITS, 0.0, -20.0 * K_UNITS]
    );
    assert_eq!(gv.palettes[&level.palette].size(), 2);
    // the new geometry: one color for the white box, one for the pane
    assert_eq!(gv.palettes[&new.palette].size(), 2);
    assert_eq!(new.draws.len(), 2);
    let pane = new.draws.iter().find(|d| d.translucent).unwrap();
    assert!(pane.own_texture && !pane.mode.depth_write);
    assert_eq!(pane.mode.blend, AlphaBlend::SrcDstSrcDst);
    assert_eq!(
      new.draws.iter().find(|d| !d.translucent).unwrap().texture,
      -1
    );
    assert!(!level.draws[0].own_texture && level.draws[0].mode.depth_write);

    // nothing changed: nothing made
    assert!(gv.update(&doc).is_empty());
    doc.begin_frame();
    assert!(gv.update(&doc).is_empty());

    // a piece moved makes its chunk again, alone
    doc.find_mut(box_id).unwrap().local.t.x += 1.0;
    doc.begin_frame();
    assert_eq!(gv.update(&doc), vec![new.id]);
    assert_eq!(gv.stats.rebuilt, 1);
    let again = gv.chunk(new.id).unwrap();
    assert_ne!(again.version, new.version);
    assert_ne!(again.palette, new.palette);
    assert!(
      !gv.palettes.contains_key(&new.palette),
      "the old palette is dropped"
    );
    assert_eq!(gv.chunk(level.id).unwrap().version, level.version);

    // far away: another cell, another chunk
    doc.find_mut(box_id).unwrap().local.t.x = 300.0;
    doc.begin_frame();
    assert_eq!(gv.update(&doc).len(), 2);
    assert_eq!(gv.stats.chunks, 3);

    // the level's ground gone: its chunk and its palette
    doc.find_mut(ground).unwrap().visible = false;
    doc.begin_frame();
    gv.update(&doc);
    assert_eq!(gv.stats.chunks, 2);
    assert!(gv.encode_palette(level.palette).is_none());
    assert!(gv.encode_chunk(level.id).is_none());
  }

  #[test]
  fn encoders() {
    let (doc, _, _) = document();
    let mut gv = GameViewChunks::default();
    gv.update(&doc);
    let list = gv.encode_chunk_list();
    assert_eq!(&list[..4], b"JLGL");
    assert_eq!((u32_at(&list, 4), u32_at(&list, 8)), (1, 2));
    assert_eq!(list.len(), 12 + 2 * 40);
    for (k, c) in gv.chunks().enumerate() {
      let o = 12 + k * 40;
      assert_eq!(
        [
          u32_at(&list, o),
          u32_at(&list, o + 4),
          u32_at(&list, o + 8),
          u32_at(&list, o + 12)
        ],
        [c.id, c.version, c.palette, c.triangles as u32]
      );
      assert_eq!(f32_at(&list, o + 16), c.bounds.lo.x);
      assert_eq!(f32_at(&list, o + 36), c.bounds.hi.z);

      let b = gv.encode_chunk(c.id).unwrap();
      assert_eq!(&b[..4], b"JLGC");
      let (nv, ni, nd) = (
        u32_at(&b, 20) as usize,
        u32_at(&b, 24) as usize,
        u32_at(&b, 28) as usize,
      );
      assert_eq!(
        [u32_at(&b, 4), u32_at(&b, 8), u32_at(&b, 12), u32_at(&b, 16)],
        [1, c.id, c.version, c.palette]
      );
      assert_eq!(
        (nv, ni, nd),
        (c.vertices.len(), c.indices.len(), c.draws.len())
      );
      assert_eq!(b.len(), 56 + nd * 16 + nv * 24 + ni * 4);
      let d = 56 + (nd - 1) * 16;
      let last = c.draws[nd - 1];
      assert_eq!((u32_at(&b, d), u32_at(&b, d + 4)), (last.first, last.count));
      assert_eq!(b[d + 12] & 1 != 0, last.translucent);
      assert_eq!(b[d + 14], last.mode.blend as u8);
      let v = 56 + nd * 16 + (nv - 1) * 24;
      let lv = c.vertices[nv - 1];
      assert_eq!(
        [f32_at(&b, v), f32_at(&b, v + 8), f32_at(&b, v + 16)],
        [lv.pos[0], lv.pos[2], lv.uv[1]]
      );
      assert_eq!(u16::from_le_bytes([b[v + 20], b[v + 21]]), lv.color);
      assert_eq!(u32_at(&b, b.len() - 4), *c.indices.last().unwrap());

      let p = gv.encode_palette(c.palette).unwrap();
      assert_eq!(&p[..4], b"JLGP");
      assert_eq!(u32_at(&p, 8) as usize, gv.palettes[&c.palette].size());
      assert_eq!(&p[12..], &gv.palettes[&c.palette].rgba[..]);
    }
    assert!(gv.encode_chunk(9999).is_none());
  }

  #[test]
  fn draw_modes() {
    // a game material's alpha reference doubled; a blended wall of a level masked, written in depth
    let game = Material {
      alpha: AlphaMode::Mask,
      cutoff: 0.25,
      base_color: Vec4::new(2.0, 2.0, 2.0, 2.0),
      ..Default::default()
    };
    let (m, t) = draw_mode(Some(&game));
    assert!(m.alpha_test && m.aref == 63 && m.depth_write && !t);
    let wall = Material {
      alpha: AlphaMode::Blend,
      ..game.clone()
    };
    let (m, t) = draw_mode(Some(&wall));
    assert!(m.alpha_test && m.depth_write && m.blend == AlphaBlend::Disabled && !t);
    let glow = Material {
      blend: BlendMode::Add,
      ..wall
    };
    let (m, t) = draw_mode(Some(&glow));
    assert!(!m.depth_write && m.blend == AlphaBlend::Src0FixDst && t);
    assert_eq!(
      draw_mode(None),
      (
        DrawMode {
          depth_write: true,
          ..Default::default()
        },
        false
      )
    );
  }
}
