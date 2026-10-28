//! A level as OpenGOAL's extractor writes it for its renderer: `out/<game>/fr3/<level>.fr3`, zstd
//! compressed (jak-project's common/custom_data/Tfrag3Data.{h,cpp}, format `VERSION`). Read for
//! the decor of the library (decor.rs): the level's shell (tfrag), its prototypes and their
//! instances (TIE, shrub), the 8 light palettes of their colors, its collision with each
//! triangle's surface (pat), its textures; and for its models (models.rs) their vertices, draws
//! and blend shapes (merc).
//!
//! The file is the serialization of `tfrag3::Level`: plain values little-endian as the C++ structs
//! lay them out, a `size_t` (u64) before each vector and string. Only what the library uses is
//! kept; the rest is read past.

use std::path::Path;

/// The format read. A tooling writing another (its `TFRAG3_VERSION` changed) is not read: the
/// reader follows it when the format changes.
pub const VERSION: u16 = 43;

/// The OpenGL settings of a draw (common/dma/gs.h, DrawMode)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DrawMode(pub u32);

impl DrawMode {
  pub fn depth_write(self) -> bool {
    self.0 & 1 != 0
  }
  pub fn clamp_s(self) -> bool {
    self.0 & (1 << 5) != 0
  }
  pub fn filter(self) -> bool {
    self.0 & (1 << 6) != 0
  }
  pub fn alpha_test(self) -> bool {
    self.0 & (1 << 18) != 0
  }
  pub fn blend(self) -> bool {
    self.0 & (1 << 20) != 0
  }
  /// what a texel failing the alpha test writes: 1 FB_ONLY (its color, not its depth)
  pub fn alpha_fail(self) -> u32 {
    (self.0 >> 21) & 3
  }
  pub fn clamp_t(self) -> bool {
    self.0 & (1 << 23) != 0
  }
  /// AlphaBlend: 0 disabled, 1 (Cs - Cd) * As + Cd, 2 (Cs - 0) * As + Cd, 3 (Cs - 0) * F + Cd,
  /// 4 (Cs - Cd) * F + Cd, 5 (0 - Cs) * As + Cd, 6 (Cs - Cs) * As + Cs, 7 (Cs - 0) * Ad + Cd
  pub fn alpha_blend(self) -> u32 {
    (self.0 >> 24) & 7
  }
}

pub type Matrix = [[f32; 4]; 4];

/// A texture, RGBA8 (Texture)
#[derive(Clone, Debug, Default)]
pub struct Texture {
  pub w: u16,
  pub h: u16,
  /// the tpage's id << 16 | the texture's index in it
  pub combo_id: u32,
  pub rgba: Vec<u8>,
  pub name: String,
  pub tpage: String,
}

/// A group of triangle strips drawn with one texture and mode (StripDraw)
#[derive(Clone, Debug, Default)]
pub struct StripDraw {
  pub mode: DrawMode,
  /// a texture of the level; negative: an animated texture's slot
  pub tex: i32,
  /// (first vertex, count): the strips of consecutive vertices
  pub runs: Vec<(u32, u16)>,
  pub plain_indices: Vec<u32>,
  pub vis_groups: Vec<VisGroup>,
  pub num_triangles: u32,
}

/// Indices of a StripDraw, grouped by what is seen together (an instance for TIE)
#[derive(Clone, Copy, Debug, Default)]
pub struct VisGroup {
  pub num_inds: u32,
  pub num_tris: u32,
  pub vis_idx: u16,
  pub proto_idx: u16,
}

/// The colors of a tree's vertices: 8 palettes of each, by quads (PackedTimeOfDay)
#[derive(Clone, Debug, Default)]
pub struct TimeOfDay {
  pub data: Vec<u8>,
}

impl TimeOfDay {
  pub fn read(&self, color: usize, palette: usize, channel: usize) -> u8 {
    self.data[(color / 4) * 128 + palette * 16 + (color % 4) * 4 + channel]
  }
  pub fn colors(&self) -> usize {
    self.data.len() / 32
  }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TfragVertex {
  pub off: [u16; 3],
  pub cluster: u16,
  pub st: [i16; 2],
  pub color_index: u16,
}

/// The level's shell (TfragTree)
#[derive(Clone, Debug, Default)]
pub struct TfragTree {
  /// normal, trans, dirt, ice, lowres, lowres-trans, water (tfrag_tree_names)
  pub kind: u32,
  pub draws: Vec<StripDraw>,
  pub vertices: Vec<TfragVertex>,
  pub cluster_origins: Vec<[u16; 3]>,
  pub colors: TimeOfDay,
  pub use_strips: bool,
}

pub const TFRAG_TREE_NAMES: [&str; 8] = [
  "normal",
  "trans",
  "dirt",
  "ice",
  "lowres",
  "lowres-trans",
  "water",
  "invalid",
];

/// A vertex of a prototype, in its space (PackedTieVertices::Vertex)
#[derive(Clone, Copy, Debug, Default)]
pub struct TieVertex {
  pub pos: [f32; 3],
  pub st: [f32; 2],
}

/// Vertices of the prototypes drawn by one matrix: an instance, or -1 (world space)
#[derive(Clone, Copy, Debug, Default)]
pub struct MatrixGroup {
  pub matrix_idx: i32,
  pub start: u32,
  pub end: u32,
}

/// A draw of the swaying instances (InstancedStripDraw)
#[derive(Clone, Debug, Default)]
pub struct WindDraw {
  pub mode: DrawMode,
  pub tex: i32,
  pub index_stream: Vec<u32>,
  /// (indices, instance) in the stream's order
  pub instance_groups: Vec<(u32, u32)>,
}

/// A swaying instance (TieWindInstance)
#[derive(Clone, Copy, Debug, Default)]
pub struct WindInstance {
  pub matrix: Matrix,
  pub stiffness: f32,
}

/// TIE: the prototypes of the decor and their instances (TieTree)
#[derive(Clone, Debug, Default)]
pub struct TieTree {
  pub static_draws: Vec<StripDraw>,
  /// category n's draws: static_draws[cdi[n]..cdi[n + 1]] (TieCategory)
  pub category_draw_indices: [u32; 10],
  pub wind_draws: Vec<WindDraw>,
  pub wind_instances: Vec<WindInstance>,
  /// per unpacked vertex
  pub color_indices: Vec<u16>,
  pub matrices: Vec<Matrix>,
  pub matrix_groups: Vec<MatrixGroup>,
  pub vertices: Vec<TieVertex>,
  pub colors: TimeOfDay,
  pub use_strips: bool,
  pub proto_names: Vec<String>,
}

/// TieCategory: the draws of an envmapped prototype come twice, its base then its shine
pub fn is_envmap_first_draw(category: usize) -> bool {
  (3..6).contains(&category)
}
pub fn is_envmap_second_draw(category: usize) -> bool {
  (6..9).contains(&category)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ShrubVertex {
  pub pos: [f32; 3],
  pub st: [f32; 2],
  pub rgba: [u8; 3],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ShrubGroup {
  pub matrix_idx: i32,
  pub start: u32,
  pub end: u32,
  pub color_index: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ShrubDraw {
  pub mode: DrawMode,
  pub tex: u32,
  pub first_index: u32,
  pub num_indices: u32,
  pub proto_idx: u16,
}

/// Shrubs: small prototypes (grass, debris) and their instances (ShrubTree)
#[derive(Clone, Debug, Default)]
pub struct ShrubTree {
  pub colors: TimeOfDay,
  pub indices: Vec<u32>,
  pub matrices: Vec<Matrix>,
  pub groups: Vec<ShrubGroup>,
  pub vertices: Vec<ShrubVertex>,
  pub total_vertices: u32,
  pub draws: Vec<ShrubDraw>,
  pub proto_names: Vec<String>,
}

/// A vertex of the collision: 3 per triangle (CollisionMesh::Vertex)
#[derive(Clone, Copy, Debug, Default)]
pub struct CollisionVertex {
  pub pos: [f32; 3],
  pub normal: [i16; 3],
  pub pat: u32,
}

/// A draw of a model (MercDraw): strips of `Level::merc_indices`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MercDraw {
  pub mode: DrawMode,
  /// negative: the animated texture slot -(tex + 1)
  pub tex: i32,
  pub eye_id: u8,
  pub first_index: u32,
  pub index_count: u32,
  pub num_triangles: u32,
}

/// A vertex of the models (MercVertex): position in game units, its 3 bones (+ 1, 0: none)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MercVertex {
  pub pos: [f32; 3],
  pub normal: [f32; 3],
  pub weights: [f32; 3],
  pub st: [f32; 2],
  pub rgba: [u8; 4],
  pub mats: [u8; 3],
}

#[derive(Clone, Debug, Default)]
pub struct MercEffect {
  /// the primitives of its glTF mesh
  pub draws: Vec<MercDraw>,
  /// the draws the game changes (blend shapes): their strips index `mod_vertices`, copies of the
  /// level's vertices numbered in the order the draws first use them
  pub mod_draws: Vec<MercDraw>,
  pub mod_vertices: Vec<MercVertex>,
  /// the blend shapes (Blerc): per vertex its targets' deltas (x y z _ nx ny nz _) after its base
  /// entry; per vertex its targets, u32::MAX, its mod vertex
  pub blerc_floats: Vec<[f32; 8]>,
  pub blerc_ints: Vec<u32>,
  pub envmap: Option<(DrawMode, u32)>,
}

/// A model of the level's art groups (MercModel)
#[derive(Clone, Debug, Default)]
pub struct MercModel {
  pub name: String,
  pub effects: Vec<MercEffect>,
}

#[derive(Clone, Debug, Default)]
pub struct Level {
  pub name: String,
  pub textures: Vec<Texture>,
  /// hi-res, then the lower levels of detail (TFRAG_GEOS)
  pub tfrag_trees: [Vec<TfragTree>; 3],
  pub tie_trees: [Vec<TieTree>; 4],
  pub shrub_trees: Vec<ShrubTree>,
  pub collision: Vec<CollisionVertex>,
  pub models: Vec<MercModel>,
  /// the models' strips (u32::MAX restarts them) and vertices
  pub merc_indices: Vec<u32>,
  pub merc_vertices: Vec<MercVertex>,
}

// ------------------------------------------------------------------------------------------------
// reading
// ------------------------------------------------------------------------------------------------

/// A .fr3 file: its size (u64), then a zstd frame of the serialized level
pub fn read_file(path: &Path) -> Result<Level, String> {
  let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
  read(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read(bytes: &[u8]) -> Result<Level, String> {
  let data = decompress(bytes)?;
  parse(&data)
}

fn decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
  use std::io::Read;
  let size = bytes
    .get(..8)
    .map(|b| u64::from_le_bytes(b.try_into().unwrap()) as usize)
    .ok_or("too short")?;
  let mut out = Vec::with_capacity(size);
  ruzstd::decoding::StreamingDecoder::new(&bytes[8..])
    .map_err(|e| e.to_string())?
    .read_to_end(&mut out)
    .map_err(|e| e.to_string())?;
  if out.len() != size {
    return Err(format!("{} bytes, {size} expected", out.len()));
  }
  Ok(out)
}

struct Reader<'a> {
  b: &'a [u8],
  at: usize,
}

type R<T> = Result<T, String>;

fn f32_at(b: &[u8], o: usize) -> f32 {
  f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
  u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u16_at(b: &[u8], o: usize) -> u16 {
  u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn matrix_at(b: &[u8], o: usize) -> Matrix {
  std::array::from_fn(|c| std::array::from_fn(|r| f32_at(b, o + c * 16 + r * 4)))
}

impl<'a> Reader<'a> {
  fn take(&mut self, n: usize) -> R<&'a [u8]> {
    let end = self.at.checked_add(n).filter(|&e| e <= self.b.len());
    let end = end.ok_or_else(|| format!("truncated at {}", self.at))?;
    let s = &self.b[self.at..end];
    self.at = end;
    Ok(s)
  }
  fn u8(&mut self) -> R<u8> {
    Ok(self.take(1)?[0])
  }
  fn bool(&mut self) -> R<bool> {
    Ok(self.u8()? != 0)
  }
  fn u16(&mut self) -> R<u16> {
    Ok(u16_at(self.take(2)?, 0))
  }
  fn u32(&mut self) -> R<u32> {
    Ok(u32_at(self.take(4)?, 0))
  }
  fn i32(&mut self) -> R<i32> {
    Ok(self.u32()? as i32)
  }
  fn f32(&mut self) -> R<f32> {
    Ok(f32_at(self.take(4)?, 0))
  }
  fn mode(&mut self) -> R<DrawMode> {
    Ok(DrawMode(self.u32()?))
  }
  /// a size_t: a count of elements of `size` bytes, not more than the file holds
  fn count(&mut self, size: usize) -> R<usize> {
    let n = u64::from_le_bytes(self.take(8)?.try_into().unwrap());
    if n.saturating_mul(size.max(1) as u64) > (self.b.len() - self.at) as u64 {
      return Err(format!("bad count {n} at {}", self.at));
    }
    Ok(n as usize)
  }
  fn string(&mut self) -> R<String> {
    let n = self.count(1)?;
    Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
  }
  fn strings(&mut self) -> R<Vec<String>> {
    let n = self.count(8)?;
    (0..n).map(|_| self.string()).collect()
  }
  /// a vector of plain structs of `size` bytes, each read by `f`
  fn pods<T>(&mut self, size: usize, f: impl Fn(&[u8]) -> T) -> R<Vec<T>> {
    let n = self.count(size)?;
    let data = self.take(n * size)?;
    Ok(data.chunks_exact(size).map(f).collect())
  }
  fn skip_pods(&mut self, size: usize) -> R<()> {
    let n = self.count(size)?;
    self.take(n * size).map(|_| ())
  }
  fn list<T>(&mut self, f: impl Fn(&mut Self) -> R<T>) -> R<Vec<T>> {
    let n = self.count(1)?;
    (0..n).map(|_| f(self)).collect()
  }
}

fn time_of_day(r: &mut Reader) -> R<TimeOfDay> {
  let data = r.pods(1, |b| b[0])?;
  r.u32()?; // color_count
  Ok(TimeOfDay { data })
}

fn bvh(r: &mut Reader) -> R<()> {
  r.take(9)?; // first_leaf_node, last_leaf_node, first_root, num_roots, only_children
  r.skip_pods(24) // VisNode
}

fn strip_draw(r: &mut Reader) -> R<StripDraw> {
  Ok(StripDraw {
    mode: r.mode()?,
    tex: r.i32()?,
    runs: r.pods(8, |b| (u32_at(b, 0), u16_at(b, 4)))?,
    plain_indices: r.pods(4, |b| u32_at(b, 0))?,
    vis_groups: r.pods(12, |b| VisGroup {
      num_inds: u32_at(b, 0),
      num_tris: u32_at(b, 4),
      vis_idx: u16_at(b, 8),
      proto_idx: u16_at(b, 10),
    })?,
    num_triangles: r.u32()?,
  })
}

fn tfrag_tree(r: &mut Reader) -> R<TfragTree> {
  Ok(TfragTree {
    kind: r.u32()?,
    draws: r.list(strip_draw)?,
    vertices: r.pods(14, |b| TfragVertex {
      off: [u16_at(b, 0), u16_at(b, 2), u16_at(b, 4)],
      cluster: u16_at(b, 6),
      st: [u16_at(b, 8) as i16, u16_at(b, 10) as i16],
      color_index: u16_at(b, 12),
    })?,
    cluster_origins: r.pods(6, |b| [u16_at(b, 0), u16_at(b, 2), u16_at(b, 4)])?,
    colors: time_of_day(r)?,
    use_strips: {
      bvh(r)?;
      r.bool()?
    },
  })
}

fn tie_tree(r: &mut Reader) -> R<TieTree> {
  let static_draws = r.list(strip_draw)?;
  let cdi = r.take(40)?;
  let category_draw_indices = std::array::from_fn(|i| u32_at(cdi, i * 4));
  let wind_draws = r.list(|r| {
    let mode = r.mode()?;
    let tex = r.i32()?;
    let index_stream = r.pods(4, |b| u32_at(b, 0))?;
    let instance_groups = r.pods(12, |b| (u32_at(b, 0), u32_at(b, 4)))?;
    r.u32()?; // num_triangles
    Ok(WindDraw {
      mode,
      tex,
      index_stream,
      instance_groups,
    })
  })?;
  let wind_instances = r.list(|r| {
    let matrix = matrix_at(r.take(64)?, 0);
    r.u16()?; // wind_idx
    Ok(WindInstance {
      matrix,
      stiffness: r.f32()?,
    })
  })?;
  let color_indices = r.pods(2, |b| u16_at(b, 0))?;
  let matrices = r.pods(64, |b| matrix_at(b, 0))?;
  let matrix_groups = r.pods(16, |b| MatrixGroup {
    matrix_idx: u32_at(b, 0) as i32,
    start: u32_at(b, 4),
    end: u32_at(b, 8),
  })?;
  let vertices = r.pods(28, |b| TieVertex {
    pos: [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8)],
    st: [f32_at(b, 12), f32_at(b, 16)],
  })?;
  let colors = time_of_day(r)?;
  bvh(r)?;
  let use_strips = r.bool()?;
  r.bool()?; // has_per_proto_visibility_toggle
  Ok(TieTree {
    static_draws,
    category_draw_indices,
    wind_draws,
    wind_instances,
    color_indices,
    matrices,
    matrix_groups,
    vertices,
    colors,
    use_strips,
    proto_names: r.strings()?,
  })
}

fn shrub_tree(r: &mut Reader) -> R<ShrubTree> {
  let colors = time_of_day(r)?;
  let indices = r.pods(4, |b| u32_at(b, 0))?;
  let matrices = r.pods(64, |b| matrix_at(b, 0))?;
  let groups = r.pods(16, |b| ShrubGroup {
    matrix_idx: u32_at(b, 0) as i32,
    start: u32_at(b, 4),
    end: u32_at(b, 8),
    color_index: u16_at(b, 12),
  })?;
  let vertices = r.pods(24, |b| ShrubVertex {
    pos: [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8)],
    st: [f32_at(b, 12), f32_at(b, 16)],
    rgba: [b[20], b[21], b[22]],
  })?;
  let total_vertices = r.u32()?;
  let draws = r.list(|r| {
    let mode = r.mode()?;
    let tex = r.u32()?;
    r.u32()?; // num_triangles
    Ok(ShrubDraw {
      mode,
      tex,
      first_index: r.u32()?,
      num_indices: r.u32()?,
      proto_idx: r.u16()?,
    })
  })?;
  r.bool()?; // has_per_proto_visibility_toggle
  Ok(ShrubTree {
    colors,
    indices,
    matrices,
    groups,
    vertices,
    total_vertices,
    draws,
    proto_names: r.strings()?,
  })
}

fn merc_draw(r: &mut Reader) -> R<MercDraw> {
  let d = MercDraw {
    mode: r.mode()?,
    tex: r.i32()?,
    eye_id: r.u8()?,
    first_index: r.u32()?,
    index_count: r.u32()?,
    num_triangles: r.u32()?,
  };
  r.bool()?; // no_strip
  Ok(d)
}

fn merc_vertex(b: &[u8]) -> MercVertex {
  let f = |o: usize| f32_at(b, o);
  MercVertex {
    pos: [f(0), f(4), f(8)],
    normal: [f(16), f(20), f(24)],
    weights: [f(32), f(36), f(40)],
    st: [f(48), f(52)],
    rgba: [b[56], b[57], b[58], b[59]],
    mats: [b[60], b[61], b[62]],
  }
}

fn merc_model(r: &mut Reader) -> R<MercModel> {
  let name = r.string()?;
  let effects = r.list(|r| {
    let draws = r.list(merc_draw)?;
    // the modifiable draw group: mod_draw, fix_draw, vertices, vertex_lump4_addr, fragment_mask,
    // expect_vidx_end, blerc (float_data, int_data)
    let mod_draws = r.list(merc_draw)?;
    r.list(merc_draw)?;
    let mod_vertices = r.pods(64, merc_vertex)?;
    r.skip_pods(2)?;
    r.skip_pods(1)?;
    r.u32()?;
    let blerc_floats = r.pods(32, |b| std::array::from_fn(|i| f32_at(b, i * 4)))?;
    let blerc_ints = r.pods(4, |b| u32_at(b, 0))?;
    let mode = r.mode()?;
    let texture = r.u32()?;
    let has_envmap = r.bool()?;
    r.bool()?; // has_mod_draw
    Ok(MercEffect {
      draws,
      mod_draws,
      mod_vertices,
      blerc_floats,
      blerc_ints,
      envmap: has_envmap.then_some((mode, texture)),
    })
  })?;
  r.take(20)?; // max_draws, max_bones, st_vif_add, xyz_scale, st_magic
  Ok(MercModel { name, effects })
}

fn parse(data: &[u8]) -> R<Level> {
  let r = &mut Reader { b: data, at: 0 };
  let version = r.u16()?;
  if version != VERSION {
    return Err(format!("format {version}, {VERSION} read"));
  }
  let name = r.string()?;
  let textures = r.list(|r| {
    let w = r.u16()?;
    let h = r.u16()?;
    let combo_id = r.u32()?;
    let n = r.count(4)?;
    let rgba = r.take(n * 4)?.to_vec();
    let name = r.string()?;
    let tpage = r.string()?;
    r.bool()?; // load_to_pool
    Ok(Texture {
      w,
      h,
      combo_id,
      rgba,
      name,
      tpage,
    })
  })?;
  // index textures: w, h, combo_id, index_data, color_table, name, tpage_name, level_names
  r.list(|r| {
    r.take(8)?;
    r.skip_pods(1)?;
    r.take(1024)?;
    r.string()?;
    r.string()?;
    r.strings().map(|_| ())
  })?;
  let mut level = Level {
    name,
    textures,
    ..Default::default()
  };
  for geom in 0..3 {
    level.tfrag_trees[geom] = r.list(tfrag_tree)?;
  }
  for geom in 0..4 {
    level.tie_trees[geom] = r.list(tie_tree)?;
  }
  level.shrub_trees = r.list(shrub_tree)?;
  // hfrag: vertices, indices, corners, buckets, colors, wang_tree_tex_id, draw_mode, occlusion
  r.skip_pods(16)?;
  r.skip_pods(4)?;
  r.skip_pods(32)?;
  r.list(|r| {
    r.skip_pods(4)?;
    r.take(32).map(|_| ())
  })?;
  time_of_day(r)?;
  r.take(24)?;
  level.collision = r.pods(32, |b| CollisionVertex {
    pos: [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8)],
    normal: [
      u16_at(b, 16) as i16,
      u16_at(b, 18) as i16,
      u16_at(b, 20) as i16,
    ],
    pat: u32_at(b, 24),
  })?;
  level.models = r.list(merc_model)?;
  level.merc_indices = r.pods(4, |b| u32_at(b, 0))?;
  level.merc_vertices = r.pods(64, merc_vertex)?;
  if r.u16()? != VERSION || r.at != data.len() {
    return Err(format!("bad end at {}", r.at));
  }
  Ok(level)
}

// ------------------------------------------------------------------------------------------------
// unpacking: the vertices and indices the game's renderer draws (Tfrag3Data.cpp, unpack())
// ------------------------------------------------------------------------------------------------

/// An unpacked vertex of tfrag or TIE: position in game units (4096 a meter)
#[derive(Clone, Copy, Debug, Default)]
pub struct Unpacked {
  pub pos: [f32; 3],
  pub st: [f32; 2],
  pub color_index: u16,
}

/// The unpacked vertices of a tree, its strip indices (u32::MAX restarts a strip) and where each
/// draw's start in them
pub struct UnpackedTree {
  pub vertices: Vec<Unpacked>,
  pub indices: Vec<u32>,
  pub draw_first: Vec<u32>,
}

fn strip_indices(draws: &[StripDraw], use_strips: bool) -> (Vec<u32>, Vec<u32>) {
  let mut indices = Vec::new();
  let mut first = Vec::with_capacity(draws.len());
  for d in draws {
    first.push(indices.len() as u32);
    for &(v0, len) in &d.runs {
      indices.extend(v0..v0 + len as u32);
      if use_strips {
        indices.push(u32::MAX);
      }
    }
    indices.extend_from_slice(&d.plain_indices);
  }
  (indices, first)
}

impl TfragTree {
  pub fn unpack(&self) -> UnpackedTree {
    const CLUSTER: f32 = 4096.0 * 40.0;
    const MASTER: f32 = 12000.0 * 4096.0;
    let rescale = CLUSTER / u16::MAX as f32;
    let vertices = self
      .vertices
      .iter()
      .map(|v| {
        let c = self.cluster_origins[v.cluster as usize];
        Unpacked {
          pos: std::array::from_fn(|k| {
            (-MASTER + CLUSTER * c[k] as f32) + v.off[k] as f32 * rescale
          }),
          st: [v.st[0] as f32 / 1024.0, v.st[1] as f32 / 1024.0],
          color_index: v.color_index,
        }
      })
      .collect();
    let (indices, draw_first) = strip_indices(&self.draws, self.use_strips);
    UnpackedTree {
      vertices,
      indices,
      draw_first,
    }
  }
}

impl TieTree {
  /// Its vertices as drawn (each group by its matrix, in order), and the static draws' strips (a
  /// TIE strip always ends with a restart)
  pub fn unpack(&self) -> UnpackedTree {
    let mut vertices = Vec::with_capacity(self.color_indices.len());
    for g in &self.matrix_groups {
      let m = (g.matrix_idx >= 0).then(|| &self.matrices[g.matrix_idx as usize]);
      for src in g.start..g.end {
        let v = &self.vertices[src as usize];
        let pos = match m {
          // mat3 + x * mat0 + y * mat1 + z * mat2, in this order as the game's SSE
          Some(m) => std::array::from_fn(|k| {
            ((m[3][k] + v.pos[0] * m[0][k]) + v.pos[1] * m[1][k]) + v.pos[2] * m[2][k]
          }),
          None => v.pos,
        };
        vertices.push(Unpacked {
          pos,
          st: v.st,
          color_index: self.color_indices.get(vertices.len()).copied().unwrap_or(0),
        });
      }
    }
    let (indices, draw_first) = strip_indices(&self.static_draws, true);
    UnpackedTree {
      vertices,
      indices,
      draw_first,
    }
  }
}

/// An unpacked shrub vertex (ShrubGpuVertex)
#[derive(Clone, Copy, Debug, Default)]
pub struct ShrubUnpacked {
  pub pos: [f32; 3],
  pub color_index: u16,
  pub rgba: [u8; 3],
}

impl ShrubTree {
  pub fn unpack(&self) -> Vec<ShrubUnpacked> {
    let mut out = Vec::with_capacity(self.total_vertices as usize);
    for g in &self.groups {
      let m = &self.matrices[g.matrix_idx as usize];
      for src in g.start..g.end {
        let v = &self.vertices[src as usize];
        out.push(ShrubUnpacked {
          // mat0 * x + mat1 * y + mat2 * z + mat3, in this order as the C++ vector sums
          pos: std::array::from_fn(|k| {
            ((m[0][k] * v.pos[0] + m[1][k] * v.pos[1]) + m[2][k] * v.pos[2]) + m[3][k]
          }),
          color_index: g.color_index,
          rgba: v.rgba,
        });
      }
    }
    out
  }
}
