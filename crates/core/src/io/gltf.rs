//! .glb / .gltf import and .glb export (own reader and writer, on serde_json).
//!
//! Export modes:
//!   Project : the editor's document. Keeps the hierarchy, every node kind and the editor's extras
//!             (og_kind, the surface of each collision triangle, actor data).
//!   Prefab  : some nodes and their descendants, placed around a pivot (a prefab file).
//!   Builder : for OpenGOAL's level builder (io/game_export): some nodes alone, their vertices in
//!             world space (no matrix), with the builder's extras and none of the editor's, and
//!             more meshes.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::log_warn;
use crate::math::{Mat4, Quat, Transform, Vec2, Vec3, Vec4};
use crate::scene::LevelSettings;
use crate::scene::skeleton::{Animation, Channel, Skeleton};
use crate::scene::{
  AlphaMode, BlendMode, Material, Mesh, Node, NodeKind, Primitive, Scene, Texture, TodColors,
  TodPalette, blend_mode_from_name, blend_mode_name, kind_from_name, kind_name, tod_from_color,
  tod_ratios, translucent_in_game,
};

#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
  /// attach the imported roots under this node
  pub parent: u32,
  /// take LevelSettings from the file's asset extras (projects)
  pub read_settings: bool,
  /// keep each vertex's joints and weights (JOINTS_0, WEIGHTS_0 in extra_attrs)
  pub skin: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ImportResult {
  pub roots: Vec<u32>,
  pub generator: String,
  /// the file's asset.extras (a rip's "opengoal": {"game", "level"})
  pub asset_extras: Value,
  pub nodes: usize,
  pub triangles: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportMode {
  #[default]
  Project,
  Prefab,
  Builder,
}

#[derive(Clone, Debug, Default)]
pub struct ExportOptions {
  pub mode: ExportMode,
  /// Prefab mode: these nodes and their descendants; Builder: these alone
  pub only: Vec<u32>,
  /// Prefab mode: the roots are written relative to this frame
  pub pivot: Mat4,
  /// Builder mode: the extras of each node (none when missing), and meshes in world space written
  /// as more nodes, with their extras
  pub node_extras: HashMap<u32, Value>,
  pub extra_meshes: Vec<(Arc<Mesh>, Value)>,
}

// ------------------------------------------------------------------------------------------------
// the file: its JSON (what the editor reads of it) and its buffers
// ------------------------------------------------------------------------------------------------

const BYTE: u32 = 5120;
const UNSIGNED_BYTE: u32 = 5121;
const SHORT: u32 = 5122;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const FLOAT: u32 = 5126;

const MODE_TRIANGLES: i64 = 4;
const MODE_TRIANGLE_STRIP: i64 = 5;
const MODE_TRIANGLE_FAN: i64 = 6;

fn none() -> i64 {
  -1
}
fn half() -> f64 {
  0.5
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Doc {
  asset: Asset,
  #[serde(default = "none")]
  scene: i64,
  scenes: Vec<GScene>,
  nodes: Vec<GNode>,
  meshes: Vec<GMesh>,
  materials: Vec<GMaterial>,
  textures: Vec<GTexture>,
  images: Vec<GImage>,
  accessors: Vec<GAccessor>,
  buffer_views: Vec<GBufferView>,
  buffers: Vec<GBuffer>,
  skins: Vec<GSkin>,
  animations: Vec<GAnimation>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Asset {
  generator: String,
  extras: Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GScene {
  nodes: Vec<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GNode {
  name: String,
  children: Vec<i64>,
  #[serde(default = "none")]
  mesh: i64,
  matrix: Vec<f64>,
  translation: Vec<f64>,
  rotation: Vec<f64>,
  scale: Vec<f64>,
  extras: Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GMesh {
  name: String,
  primitives: Vec<GPrimitive>,
  extras: Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GPrimitive {
  attributes: BTreeMap<String, i64>,
  #[serde(default = "none")]
  indices: i64,
  #[serde(default = "none")]
  material: i64,
  #[serde(default = "none")]
  mode: i64,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GMaterial {
  name: String,
  pbr_metallic_roughness: GPbr,
  alpha_mode: String,
  #[serde(default = "half")]
  alpha_cutoff: f64,
  double_sided: bool,
  extras: Value,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GPbr {
  base_color_factor: Vec<f64>,
  base_color_texture: Option<GTextureRef>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GTextureRef {
  #[serde(default = "none")]
  index: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GTexture {
  #[serde(default = "none")]
  source: i64,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GImage {
  name: String,
  uri: String,
  #[serde(default = "none")]
  buffer_view: i64,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GAccessor {
  #[serde(default = "none")]
  buffer_view: i64,
  byte_offset: usize,
  component_type: u32,
  count: usize,
  #[serde(rename = "type")]
  ty: String,
  normalized: bool,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GBufferView {
  #[serde(default = "none")]
  buffer: i64,
  byte_offset: usize,
  byte_length: usize,
  byte_stride: usize,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GBuffer {
  uri: String,
  byte_length: usize,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GSkin {
  joints: Vec<i64>,
  #[serde(default = "none")]
  inverse_bind_matrices: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GAnimation {
  name: String,
  channels: Vec<GChannel>,
  samplers: Vec<GSampler>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GChannel {
  #[serde(default = "none")]
  sampler: i64,
  target: GTarget,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GTarget {
  #[serde(default = "none")]
  node: i64,
  path: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GSampler {
  #[serde(default = "none")]
  input: i64,
  #[serde(default = "none")]
  output: i64,
  interpolation: String,
}

fn get<T>(v: &[T], i: i64) -> Option<&T> {
  usize::try_from(i).ok().and_then(|i| v.get(i))
}

/// A read file: its JSON, the bytes of the file (a .glb's BIN chunk is a range of them) and its
/// folder (external buffers and images)
struct GltfFile {
  doc: Doc,
  bytes: Vec<u8>,
  bin: Option<(usize, usize)>,
  dir: PathBuf,
}

fn load_file(path: &Path, binary: bool) -> Result<GltfFile, String> {
  let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
  let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
  let u32_at = |b: &[u8], at: usize| {
    b.get(at..at + 4)
      .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
  };
  let (json, bin) = if binary {
    if u32_at(&bytes, 0) != Some(0x46546C67) {
      return Err("unreadable glTF file: not a .glb".into());
    }
    let len = u32_at(&bytes, 12).unwrap_or(0) as usize;
    if u32_at(&bytes, 16) != Some(0x4E4F534A) || 20 + len > bytes.len() {
      return Err("unreadable glTF file: no JSON chunk".into());
    }
    let at = 20 + len;
    let bin = match (u32_at(&bytes, at), u32_at(&bytes, at + 4)) {
      (Some(n), Some(0x004E4942)) => {
        Some((at + 8, (n as usize).min(bytes.len().saturating_sub(at + 8))))
      }
      _ => None,
    };
    (20..at, bin)
  } else {
    (0..bytes.len(), None)
  };
  let text = &bytes[json];
  let end = text
    .iter()
    .rposition(|&c| c != 0 && c != b' ')
    .map_or(0, |i| i + 1);
  let doc: Doc =
    serde_json::from_slice(&text[..end]).map_err(|e| format!("unreadable glTF file: {e}"))?;
  Ok(GltfFile {
    doc,
    bytes,
    bin,
    dir,
  })
}

impl GltfFile {
  /// The bytes of a URI: data (base64) or a file next to the glTF
  fn uri_bytes(&self, uri: &str) -> Option<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
      let at = rest.find(";base64,")?;
      return base64_decode(&rest[at + 8..]);
    }
    std::fs::read(self.dir.join(percent_decode(uri))).ok()
  }

  fn buffers(&self) -> Result<Vec<Cow<'_, [u8]>>, String> {
    let mut out = Vec::with_capacity(self.doc.buffers.len());
    for (i, b) in self.doc.buffers.iter().enumerate() {
      if b.uri.is_empty() {
        // a .glb's own buffer: its BIN chunk
        let Some((at, len)) = self.bin else {
          return Err(format!("unreadable glTF file: buffer {i} has no data"));
        };
        out.push(Cow::Borrowed(&self.bytes[at..at + len.min(b.byte_length)]));
      } else {
        let data = self
          .uri_bytes(&b.uri)
          .ok_or_else(|| format!("unreadable glTF file: cannot read buffer {}", b.uri))?;
        out.push(Cow::Owned(data));
      }
    }
    Ok(out)
  }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
  let mut out = Vec::with_capacity(s.len() / 4 * 3);
  let (mut acc, mut bits) = (0u32, 0);
  for c in s.bytes() {
    let v = match c {
      b'A'..=b'Z' => c - b'A',
      b'a'..=b'z' => c - b'a' + 26,
      b'0'..=b'9' => c - b'0' + 52,
      b'+' | b'-' => 62,
      b'/' | b'_' => 63,
      b'=' => break,
      b' ' | b'\n' | b'\r' | b'\t' => continue,
      _ => return None,
    };
    acc = (acc << 6) | v as u32;
    bits += 6;
    if bits >= 8 {
      bits -= 8;
      out.push((acc >> bits) as u8);
    }
  }
  Some(out)
}

fn percent_decode(s: &str) -> String {
  let b = s.as_bytes();
  let mut out = Vec::with_capacity(b.len());
  let mut i = 0;
  while i < b.len() {
    let hex = (b[i] == b'%')
      .then(|| s.get(i + 1..i + 3))
      .flatten()
      .and_then(|h| u8::from_str_radix(h, 16).ok());
    match hex {
      Some(v) => {
        out.push(v);
        i += 3;
      }
      None => {
        out.push(b[i]);
        i += 1;
      }
    }
  }
  String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------------------------------------------
// accessors
// ------------------------------------------------------------------------------------------------

struct AccessorView<'a> {
  data: &'a [u8],
  count: usize,
  comps: usize,
  ctype: u32,
  csize: usize,
  stride: usize,
  normalized: bool,
}

fn num_components(ty: &str) -> usize {
  match ty {
    "SCALAR" => 1,
    "VEC2" => 2,
    "VEC3" => 3,
    "VEC4" | "MAT2" => 4,
    "MAT3" => 9,
    "MAT4" => 16,
    _ => 0,
  }
}

fn component_size(ctype: u32) -> usize {
  match ctype {
    BYTE | UNSIGNED_BYTE => 1,
    SHORT | UNSIGNED_SHORT => 2,
    5124 | UNSIGNED_INT | FLOAT => 4,
    5130 => 8,
    _ => 0,
  }
}

fn view_accessor<'a>(doc: &Doc, bufs: &'a [Cow<'_, [u8]>], idx: i64) -> Option<AccessorView<'a>> {
  let a = get(&doc.accessors, idx)?;
  let bv = get(&doc.buffer_views, a.buffer_view)?;
  let buf: &[u8] = get(bufs, bv.buffer)?;
  let comps = num_components(&a.ty);
  let csize = component_size(a.component_type);
  if comps == 0 || csize == 0 {
    return None;
  }
  let stride = if bv.byte_stride != 0 {
    bv.byte_stride
  } else {
    comps * csize
  };
  let start = bv.byte_offset.checked_add(a.byte_offset)?;
  if a.count > 0 {
    let end = stride
      .checked_mul(a.count - 1)?
      .checked_add(start)?
      .checked_add(comps * csize)?;
    if end > buf.len() {
      return None;
    }
  }
  Some(AccessorView {
    data: if a.count > 0 { &buf[start..] } else { &[] },
    count: a.count,
    comps,
    ctype: a.component_type,
    csize,
    stride,
    normalized: a.normalized,
  })
}

fn read_component(p: &[u8], ctype: u32, normalized: bool) -> f32 {
  match ctype {
    FLOAT => f32::from_le_bytes([p[0], p[1], p[2], p[3]]),
    UNSIGNED_BYTE => {
      if normalized {
        p[0] as f32 / 255.0
      } else {
        p[0] as f32
      }
    }
    BYTE => {
      let v = p[0] as i8;
      if normalized {
        (v as f32 / 127.0).max(-1.0)
      } else {
        v as f32
      }
    }
    UNSIGNED_SHORT => {
      let v = u16::from_le_bytes([p[0], p[1]]);
      if normalized {
        v as f32 / 65535.0
      } else {
        v as f32
      }
    }
    SHORT => {
      let v = i16::from_le_bytes([p[0], p[1]]);
      if normalized {
        (v as f32 / 32767.0).max(-1.0)
      } else {
        v as f32
      }
    }
    UNSIGNED_INT => u32::from_le_bytes([p[0], p[1], p[2], p[3]]) as f32,
    _ => 0.0,
  }
}

impl AccessorView<'_> {
  /// Element `i` as a vec4; missing components get 0 and `fill_w` (w defaults to 1 for colors).
  /// `force_normalized`: integers read as 0..1 (texture coordinates, colors)
  fn vec4(&self, i: usize, force_normalized: bool, fill_w: f32) -> Vec4 {
    let p = &self.data[i * self.stride..];
    let mut r = Vec4::new(0.0, 0.0, 0.0, fill_w);
    let n = self.comps.min(4);
    if self.ctype == FLOAT {
      for c in 0..n {
        r[c] = f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap());
      }
    } else {
      let norm = self.normalized || force_normalized;
      for c in 0..n {
        r[c] = read_component(&p[c * self.csize..], self.ctype, norm);
      }
    }
    r
  }

  fn all_vec4(&self, force_normalized: bool, fill_w: f32) -> Vec<Vec4> {
    (0..self.count)
      .map(|i| self.vec4(i, force_normalized, fill_w))
      .collect()
  }

  fn index(&self, i: usize) -> u32 {
    let p = &self.data[i * self.stride..];
    match self.ctype {
      UNSIGNED_BYTE => p[0] as u32,
      UNSIGNED_SHORT => u16::from_le_bytes([p[0], p[1]]) as u32,
      _ => u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
    }
  }
}

fn read_vec4(
  doc: &Doc,
  bufs: &[Cow<'_, [u8]>],
  idx: i64,
  force_normalized: bool,
  fill_w: f32,
) -> Vec<Vec4> {
  view_accessor(doc, bufs, idx).map_or_else(Vec::new, |v| v.all_vec4(force_normalized, fill_w))
}

fn read_indices(doc: &Doc, bufs: &[Cow<'_, [u8]>], idx: i64) -> Vec<u32> {
  view_accessor(doc, bufs, idx)
    .map_or_else(Vec::new, |v| (0..v.count).map(|i| v.index(i)).collect())
}

fn read_u32_view(doc: &Doc, bufs: &[Cow<'_, [u8]>], view: i64) -> Vec<u32> {
  let Some(bv) = get(&doc.buffer_views, view) else {
    return Vec::new();
  };
  let Some(buf) = get(bufs, bv.buffer) else {
    return Vec::new();
  };
  let Some(bytes) = buf.get(bv.byte_offset..bv.byte_offset + bv.byte_length) else {
    return Vec::new();
  };
  bytes
    .as_chunks::<4>()
    .0
    .iter()
    .map(|c| u32::from_le_bytes(*c))
    .collect()
}

// Keys of the OpenGOAL level builder on collision nodes: a rip's collision is marked with them,
// and collide_pat is the exact surface of the node (ogle-extract, the decompiler's rips).
const COLLISION_KEYS: [&str; 13] = [
  "set_collision",
  "set_invisible",
  "ignore",
  "collide_material",
  "collide_event",
  "collide_mode",
  "nolineofsight",
  "noedge",
  "nocamera",
  "noentity",
  "collide_pat",
  "collide_game",
  "kind",
];

fn json_int(e: &Value, key: &str, dflt: i32) -> i32 {
  match e.get(key) {
    Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) as i32,
    Some(Value::Bool(b)) => *b as i32,
    _ => dflt,
  }
}

fn json_bool(e: &Value, key: &str) -> bool {
  e.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn json_str<'a>(e: &'a Value, key: &str) -> &'a str {
  e.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The pixels of an image of the file, RGBA; None when they cannot be read
fn decode_image(
  file: &GltfFile,
  bufs: &[Cow<'_, [u8]>],
  img: &GImage,
) -> Option<(u32, u32, Vec<u8>)> {
  let owned;
  let bytes: &[u8] = if let Some(bv) = get(&file.doc.buffer_views, img.buffer_view) {
    get(bufs, bv.buffer)?.get(bv.byte_offset..bv.byte_offset + bv.byte_length)?
  } else {
    owned = file.uri_bytes(&img.uri)?;
    &owned
  };
  let rgba = image::load_from_memory(bytes).ok()?.into_rgba8();
  Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// `f` over `items` on every core: a level's images are decoded in parallel
fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(usize, &T) -> R + Sync) -> Vec<R> {
  let threads = std::thread::available_parallelism()
    .map_or(1, |n| n.get())
    .min(items.len().max(1));
  let chunk = items.len().div_ceil(threads).max(1);
  std::thread::scope(|s| {
    let handles: Vec<_> = items
      .chunks(chunk)
      .enumerate()
      .map(|(c, part)| {
        let f = &f;
        s.spawn(move || {
          part
            .iter()
            .enumerate()
            .map(|(i, t)| f(c * chunk + i, t))
            .collect::<Vec<R>>()
        })
      })
      .collect();
    handles
      .into_iter()
      .flat_map(|h| h.join().unwrap())
      .collect()
  })
}

// ------------------------------------------------------------------------------------------------
// import
// ------------------------------------------------------------------------------------------------

/// per primitive kept: the glTF primitive it is, and the vertices of that one it kept (the nodes'
/// lighting, og_tod, is given per vertex of the glTF primitives)
struct PrimSource {
  prim: usize,
  used: Vec<u32>,
}

struct Importer<'a> {
  file: &'a GltfFile,
  bufs: &'a [Cow<'a, [u8]>],
  skin: bool,
  material_map: Vec<i32>,
  /// meshes (shared between nodes: instancing is kept), made when a node first uses them
  meshes: Vec<Option<(Mesh, Vec<PrimSource>)>>,
  /// the nodes' lighting: a palette (8 RGBA bytes per color) shared by the nodes of a tree
  palettes: HashMap<i64, Option<Arc<TodPalette>>>,
  /// a vertex array's index -> the primitive's (u32::MAX: not used yet), reset after each primitive
  remap: Vec<u32>,
  triangles: usize,
}

impl Importer<'_> {
  fn view(&self, idx: i64) -> Option<AccessorView<'_>> {
    view_accessor(&self.file.doc, self.bufs, idx)
  }

  fn build_mesh(&mut self, mi: usize) {
    let (file, bufs) = (self.file, self.bufs);
    let doc = &file.doc;
    let gmesh = &doc.meshes[mi];
    let mut mesh = Mesh::new();
    mesh.name = if gmesh.name.is_empty() {
      format!("mesh-{mi}")
    } else {
      gmesh.name.clone()
    };
    let mut mesh_extras = gmesh.extras.clone();
    let mut pat_views: Vec<i64> = Vec::new();
    if let Some(v) = mesh_extras
      .as_object_mut()
      .and_then(|o| o.remove("og_tri_pat_views"))
    {
      pat_views = v
        .as_array()
        .map(|a| a.iter().map(|x| x.as_i64().unwrap_or(-1)).collect())
        .unwrap_or_default();
    }
    if mesh_extras.is_object() {
      mesh.extras = mesh_extras;
    }
    let mut sources = Vec::new();
    for (pi, gp) in gmesh.primitives.iter().enumerate() {
      let mode = if gp.mode < 0 { MODE_TRIANGLES } else { gp.mode };
      if mode != MODE_TRIANGLES && mode != MODE_TRIANGLE_STRIP && mode != MODE_TRIANGLE_FAN {
        continue;
      }
      let Some(&pos_acc) = gp.attributes.get("POSITION") else {
        continue;
      };
      let Some(pos) = view_accessor(doc, bufs, pos_acc) else {
        continue;
      };
      let nv = pos.count;
      if nv == 0 {
        continue;
      }
      // indices first: only the vertices this primitive uses are kept
      let mut raw = if gp.indices >= 0 {
        read_indices(doc, bufs, gp.indices)
      } else {
        (0..nv as u32).collect()
      };
      if raw.iter().any(|&i| i as usize >= nv) {
        log_warn!(
          "mesh %s: indices out of range, primitive skipped",
          &mesh.name
        );
        continue;
      }
      // (a small part of a large vertex array: the pieces of a level's shell share theirs)
      if self.remap.len() < nv {
        self.remap.resize(nv, u32::MAX);
      }
      let mut used = Vec::new();
      for i in raw.iter_mut() {
        let r = &mut self.remap[*i as usize];
        if *r == u32::MAX {
          *r = used.len() as u32;
          used.push(*i);
        }
        *i = *r;
      }
      for &u in &used {
        self.remap[u as usize] = u32::MAX;
      }
      // an attribute's values at the vertices kept, when it has one per vertex
      let gather = |acc: i64, norm: bool, fill_w: f32| -> Option<Vec<Vec4>> {
        let v = view_accessor(doc, bufs, acc).filter(|v| v.count == nv)?;
        Some(
          used
            .iter()
            .map(|&u| v.vec4(u as usize, norm, fill_w))
            .collect(),
        )
      };
      let mut p = Primitive {
        pos: used
          .iter()
          .map(|&u| pos.vec4(u as usize, false, 1.0).xyz())
          .collect(),
        ..Default::default()
      };
      for (name, &acc) in &gp.attributes {
        match name.as_str() {
          "POSITION" => {}
          "NORMAL" => {
            if let Some(v) = gather(acc, false, 0.0) {
              p.nrm = v.iter().map(Vec4::xyz).collect();
            }
          }
          "TEXCOORD_0" => {
            if let Some(v) = gather(acc, true, 0.0) {
              p.uv = v.iter().map(|v| Vec2::new(v.x, v.y)).collect();
            }
          }
          "COLOR_0" => {
            if let Some(v) = gather(acc, true, 1.0) {
              p.col = v;
            }
          }
          n if n.starts_with('_') => {
            if let Some(v) = gather(acc, true, 1.0) {
              p.extra_attrs.insert(name.clone(), v);
            }
          }
          "JOINTS_0" | "WEIGHTS_0" if self.skin => {
            if let Some(v) = gather(acc, name == "WEIGHTS_0", 0.0) {
              p.extra_attrs.insert(name.clone(), v);
            }
          }
          _ => {}
        }
      }
      if mode == MODE_TRIANGLES {
        raw.truncate(raw.len() / 3 * 3);
        p.idx = raw;
      } else if mode == MODE_TRIANGLE_STRIP {
        p.idx.reserve(raw.len().saturating_sub(2) * 3);
        for i in 2..raw.len() {
          if i % 2 == 0 {
            p.idx.extend([raw[i - 2], raw[i - 1], raw[i]]);
          } else {
            p.idx.extend([raw[i - 1], raw[i - 2], raw[i]]);
          }
        }
      } else {
        p.idx.reserve(raw.len().saturating_sub(2) * 3);
        for i in 2..raw.len() {
          p.idx.extend([raw[0], raw[i - 1], raw[i]]);
        }
      }
      if p.nrm.is_empty() {
        p.compute_normals();
      }
      if let Some(&m) = get(&self.material_map, gp.material) {
        p.material = m;
      }
      if let Some(&view) = pat_views.get(pi)
        && view >= 0
      {
        p.tri_pat = read_u32_view(doc, bufs, view);
        if p.tri_pat.len() != p.tri_count() {
          p.tri_pat.clear();
        }
      }
      self.triangles += p.tri_count();
      mesh.prims.push(p);
      sources.push(PrimSource { prim: pi, used });
    }
    self.meshes[mi] = Some((mesh, sources));
  }

  fn read_tod(&mut self, j: &Value, mi: usize) -> Option<TodColors> {
    let index = j.get("index")?.as_array()?;
    j.get("palette")?;
    let pa = json_int(j, "palette", -1) as i64;
    if !self.palettes.contains_key(&pa) {
      let pal = self
        .view(pa)
        .filter(|v| v.ctype == UNSIGNED_BYTE && v.comps == 4)
        .map(|v| {
          let n = v.count / 8 * 8;
          let mut rgba = Vec::with_capacity(n * 4);
          for i in 0..n {
            rgba.extend_from_slice(&v.data[i * v.stride..i * v.stride + 4]);
          }
          Arc::new(TodPalette {
            rgba,
            local: json_bool(j, "local"),
          })
        });
      self.palettes.insert(pa, pal);
    }
    let palette = self.palettes[&pa].clone()?;
    let size = palette.size();
    let (_, sources) = self.meshes[mi].as_ref()?;
    let mut tod = TodColors {
      palette: Some(palette.clone()),
      index: Vec::with_capacity(sources.len()),
    };
    for src in sources {
      let acc = index
        .get(src.prim)
        .filter(|v| v.is_i64() || v.is_u64())?
        .as_i64()?;
      let v = view_accessor(&self.file.doc, self.bufs, acc)
        .filter(|v| v.comps == 1 && v.ctype == UNSIGNED_SHORT)?;
      let mut out = Vec::with_capacity(src.used.len());
      for &u in &src.used {
        if u as usize >= v.count {
          return None;
        }
        let at = u as usize * v.stride;
        let c = u16::from_le_bytes([v.data[at], v.data[at + 1]]);
        if c as usize >= size {
          return None;
        }
        out.push(c);
      }
      tod.index.push(out);
    }
    Some(tod)
  }
}

fn import_file(
  scene: &mut Scene,
  path: &Path,
  opts: &ImportOptions,
) -> Result<ImportResult, String> {
  let binary = !path
    .extension()
    .is_some_and(|e| e.eq_ignore_ascii_case("gltf"));
  let file = load_file(path, binary)?;
  let bufs = file.buffers()?;
  let doc = &file.doc;
  let mut res = ImportResult {
    generator: doc.asset.generator.clone(),
    asset_extras: doc.asset.extras.clone(),
    ..Default::default()
  };
  if opts.read_settings
    && let Some(settings) = res.asset_extras.get("ogle").and_then(|o| o.get("settings"))
  {
    scene.settings = LevelSettings::from_json(settings);
  }

  // images -> textures
  let decoded = parallel_map(&doc.images, |_, img| decode_image(&file, &bufs, img));
  let mut image_to_texture = Vec::with_capacity(doc.images.len());
  for (i, (img, pixels)) in doc.images.iter().zip(decoded).enumerate() {
    let mut t = Texture {
      name: if img.name.is_empty() {
        format!("image-{i}")
      } else {
        img.name.clone()
      },
      ..Default::default()
    };
    match pixels {
      Some((w, h, rgba)) if w > 0 && h > 0 => {
        t.width = w as i32;
        t.height = h as i32;
        t.rgba = rgba;
      }
      _ => {
        // unsupported: 2x2 magenta checker, so the problem is visible
        t.width = 2;
        t.height = 2;
        t.rgba = vec![
          255, 0, 255, 255, 40, 0, 40, 255, 40, 0, 40, 255, 255, 0, 255, 255,
        ];
        log_warn!(
          "image %s: unsupported format, replaced by a checker",
          &t.name
        );
      }
    }
    image_to_texture.push(scene.add_texture(t));
  }

  // materials
  let mut material_map = Vec::with_capacity(doc.materials.len());
  for (i, gm) in doc.materials.iter().enumerate() {
    let mut m = Material {
      name: if gm.name.is_empty() {
        format!("material-{i}")
      } else {
        gm.name.clone()
      },
      ..Default::default()
    };
    let pbr = &gm.pbr_metallic_roughness;
    if let [r, g, b, a] = pbr.base_color_factor[..] {
      m.base_color = Vec4::new(r as f32, g as f32, b as f32, a as f32);
    }
    if let Some(t) = &pbr.base_color_texture
      && let Some(tex) = get(&doc.textures, t.index)
      && let Some(&ti) = get(&image_to_texture, tex.source)
    {
      m.texture = ti;
    }
    m.alpha = match gm.alpha_mode.as_str() {
      "MASK" => AlphaMode::Mask,
      "BLEND" => AlphaMode::Blend,
      _ => AlphaMode::Opaque,
    };
    m.cutoff = gm.alpha_cutoff as f32;
    m.double_sided = gm.double_sided;
    if let Some(blend) = gm.extras.get("og_blend").and_then(Value::as_str) {
      m.blend = blend_mode_from_name(blend);
      if m.blend != BlendMode::Alpha {
        m.alpha = AlphaMode::Blend;
      }
    }
    m.translucent = json_bool(&gm.extras, "og_translucent");
    material_map.push(scene.add_material(m));
  }

  let mut im = Importer {
    file: &file,
    bufs: &bufs,
    skin: opts.skin,
    material_map,
    meshes: (0..doc.meshes.len()).map(|_| None).collect(),
    palettes: HashMap::new(),
    remap: Vec::new(),
    triangles: 0,
  };

  // nodes, depth first as the file has them; their meshes are given once all are read (the
  // collision's surfaces are set on its mesh as its nodes come)
  let roots: Vec<i64> = if !doc.scenes.is_empty() {
    let si = if doc.scene >= 0 { doc.scene } else { 0 };
    get(&doc.scenes, si).unwrap_or(&doc.scenes[0]).nodes.clone()
  } else {
    let mut children = vec![false; doc.nodes.len()];
    for n in &doc.nodes {
      for &c in &n.children {
        if let Some(c) = usize::try_from(c).ok().and_then(|c| children.get_mut(c)) {
          *c = true;
        }
      }
    }
    (0..doc.nodes.len() as i64)
      .filter(|&i| !children[i as usize])
      .collect()
  };
  let mut with_mesh: Vec<(usize, usize)> = Vec::new(); // scene index, glTF mesh
  let mut visited = vec![false; doc.nodes.len()];
  let mut stack: Vec<(usize, u32)> = roots
    .iter()
    .rev()
    .filter_map(|&r| usize::try_from(r).ok().filter(|&r| r < doc.nodes.len()))
    .map(|r| (r, opts.parent))
    .collect();
  while let Some((gi, parent)) = stack.pop() {
    if std::mem::replace(&mut visited[gi], true) {
      continue; // a node twice in the tree (not a tree): once
    }
    let gn = &doc.nodes[gi];
    let mut n = Node {
      parent,
      name: if gn.name.is_empty() {
        format!("node-{gi}")
      } else {
        gn.name.clone()
      },
      ..Default::default()
    };
    if gn.matrix.len() == 16 {
      let mut m = Mat4::identity();
      for (k, v) in gn.matrix.iter().enumerate() {
        m.m[k] = *v as f32;
      }
      n.local = Transform::from_matrix(&m);
    } else {
      if let [x, y, z] = gn.translation[..] {
        n.local.t = Vec3::new(x as f32, y as f32, z as f32);
      }
      if let [x, y, z, w] = gn.rotation[..] {
        n.local.r = Quat::new(x as f32, y as f32, z as f32, w as f32).normalized();
      }
      if let [x, y, z] = gn.scale[..] {
        n.local.s = Vec3::new(x as f32, y as f32, z as f32);
      }
    }
    let mut extras = if gn.extras.is_object() {
      gn.extras.clone()
    } else {
      json!({})
    };
    let mesh = usize::try_from(gn.mesh)
      .ok()
      .filter(|&m| m < doc.meshes.len());
    if let Some(mi) = mesh
      && im.meshes[mi].is_none()
    {
      im.build_mesh(mi);
    }
    let obj = extras.as_object_mut().unwrap();
    if let Some(mi) = mesh
      && let Some(j) = obj.get("og_tod")
    {
      n.tod = im
        .read_tod(j, mi)
        .filter(|t| t.fits(&im.meshes[mi].as_ref().unwrap().0))
        .map(Arc::new);
    }
    obj.remove("og_tod");

    let mut kind = if mesh.is_some() {
      NodeKind::Render
    } else {
      NodeKind::Group
    };
    if let Some(k) = extras.get("og_kind") {
      kind = kind_from_name(k.as_str().unwrap_or(""), kind);
    } else if json_int(&extras, "set_collision", 0) != 0
      && json_int(&extras, "set_invisible", 0) != 0
      && json_int(&extras, "ignore", 0) == 0
      && mesh.is_some()
    {
      kind = NodeKind::Collision;
    }
    let obj = extras.as_object_mut().unwrap();
    if json_str(&gn.extras, "kind") == "actor" && obj.contains_key("etype") && mesh.is_none() {
      // an actor of a rip: etype, aid, game_task, bsphere and lump, as the game has them
      kind = NodeKind::Actor;
      obj.remove("kind");
    }
    n.kind = kind;
    if kind == NodeKind::Collision
      && let Some(mi) = mesh
    {
      // one surface per triangle: from the project's own data, or the node's collide_pat
      let m = &mut im.meshes[mi].as_mut().unwrap().0;
      if !m.prims.iter().all(|p| p.tri_pat.len() == p.tri_count()) {
        let pat = obj
          .get("collide_pat")
          .filter(|v| v.is_i64() || v.is_u64())
          .and_then(Value::as_i64)
          .unwrap_or(0) as u32;
        for p in &mut m.prims {
          p.tri_pat = vec![pat; p.tri_count()];
        }
      }
      for k in COLLISION_KEYS {
        obj.remove(k);
      }
    }
    n.visible = !json_bool(&extras, "og_hidden");
    n.locked = json_bool(&extras, "og_locked");
    let obj = extras.as_object_mut().unwrap();
    for k in ["og_kind", "og_id", "og_hidden", "og_locked"] {
      obj.remove(k);
    }
    n.extras = extras;
    let id = scene.add(n);
    if let Some(mi) = mesh {
      with_mesh.push((scene.nodes.len() - 1, mi));
    }
    res.nodes += 1;
    if parent == opts.parent {
      res.roots.push(id);
    }
    for &c in gn.children.iter().rev() {
      if let Ok(c) = usize::try_from(c)
        && c < doc.nodes.len()
      {
        stack.push((c, id));
      }
    }
  }
  let meshes: Vec<Option<Arc<Mesh>>> = std::mem::take(&mut im.meshes)
    .into_iter()
    .map(|m| m.map(|(m, _)| Arc::new(m)))
    .collect();
  for (i, mi) in with_mesh {
    scene.nodes[i].mesh = meshes[mi].clone();
  }
  res.triangles = im.triangles;
  Ok(res)
}

/// Reads a .glb or .gltf into the scene: its images, materials, meshes (shared between the nodes
/// that use them) and nodes (under `opts.parent`).
pub fn import_gltf(
  scene: &mut Scene,
  path: impl AsRef<Path>,
  opts: &ImportOptions,
) -> Result<ImportResult, String> {
  import_file(scene, path.as_ref(), opts)
}

/// The skeleton of a model file and its animations (scene/skeleton); an error when it has none
pub fn load_skeleton(path: impl AsRef<Path>) -> Result<Skeleton, String> {
  // its images are not needed
  let file = load_file(path.as_ref(), true)?;
  let bufs = file.buffers()?;
  let doc = &file.doc;
  let Some(skin) = doc.skins.first() else {
    return Err("no skeleton".into());
  };
  let n = doc.nodes.len();
  let mut s = Skeleton {
    parent: vec![-1; n],
    rest: vec![Mat4::identity(); n],
    ..Default::default()
  };
  for (i, node) in doc.nodes.iter().enumerate() {
    for &c in &node.children {
      if c >= 0 && (c as usize) < n {
        s.parent[c as usize] = i as i32;
      }
    }
    if node.matrix.len() == 16 {
      for (k, v) in node.matrix.iter().enumerate() {
        s.rest[i].m[k] = *v as f32;
      }
    } else {
      let (mut t, mut sc, mut r) = (Vec3::default(), Vec3::new(1.0, 1.0, 1.0), Quat::default());
      if let [x, y, z] = node.translation[..] {
        t = Vec3::new(x as f32, y as f32, z as f32);
      }
      if let [x, y, z] = node.scale[..] {
        sc = Vec3::new(x as f32, y as f32, z as f32);
      }
      if let [x, y, z, w] = node.rotation[..] {
        r = Quat::new(x as f32, y as f32, z as f32, w as f32);
      }
      s.rest[i] = Mat4::translate(t) * Mat4::rotate(r) * Mat4::scale(sc);
    }
  }
  s.joints = skin.joints.iter().map(|&j| j as i32).collect();
  if let Some(ibm) = view_accessor(doc, &bufs, skin.inverse_bind_matrices)
    && ibm.comps == 16
    && ibm.ctype == FLOAT
  {
    for j in 0..ibm.count {
      let mut m = Mat4::identity();
      for k in 0..16 {
        let at = j * ibm.stride + k * 4;
        m.m[k] = f32::from_le_bytes(ibm.data[at..at + 4].try_into().unwrap());
      }
      s.inverse_bind.push(m);
    }
  }
  for a in &doc.animations {
    let mut anim = Animation {
      name: a.name.clone(),
      ..Default::default()
    };
    for ch in &a.channels {
      let Some(sm) = get(&a.samplers, ch.sampler) else {
        continue;
      };
      let path = match ch.target.path.as_str() {
        "translation" => 0,
        "rotation" => 1,
        "scale" => 2,
        _ => continue,
      };
      let c = Channel {
        node: ch.target.node as i32,
        path,
        step: sm.interpolation == "STEP",
        times: read_vec4(doc, &bufs, sm.input, false, 0.0)
          .iter()
          .map(|v| v.x)
          .collect(),
        values: read_vec4(doc, &bufs, sm.output, false, 0.0),
      };
      if let Some(&last) = c.times.last() {
        anim.duration = anim.duration.max(last);
      }
      anim.channels.push(c);
    }
    s.animations.push(anim);
  }
  Ok(s)
}

// ------------------------------------------------------------------------------------------------
// export
// ------------------------------------------------------------------------------------------------

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

#[derive(Default)]
struct GlbWriter {
  views: Vec<Value>,
  accessors: Vec<Value>,
  bin: Vec<u8>,
}

impl GlbWriter {
  /// A buffer view of what `fill` appends to the binary chunk (4 bytes aligned)
  fn add_view(&mut self, target: u32, fill: impl FnOnce(&mut Vec<u8>)) -> usize {
    while !self.bin.len().is_multiple_of(4) {
      self.bin.push(0);
    }
    let offset = self.bin.len();
    fill(&mut self.bin);
    let mut bv = json!({"buffer": 0, "byteOffset": offset, "byteLength": self.bin.len() - offset});
    if target != 0 {
      bv["target"] = json!(target);
    }
    self.views.push(bv);
    self.views.len() - 1
  }

  fn accessor(&mut self, view: usize, count: usize, ctype: u32, ty: &str) -> usize {
    self
      .accessors
      .push(json!({"bufferView": view, "componentType": ctype, "count": count, "type": ty}));
    self.accessors.len() - 1
  }

  /// floats, N per element (`f` gives them), with their bounds when `minmax`
  fn add_floats<T, const N: usize>(
    &mut self,
    data: &[T],
    f: impl Fn(&T) -> [f32; N],
    ty: &str,
    minmax: bool,
  ) -> usize {
    let (mut lo, mut hi) = ([1e30f32; N], [-1e30f32; N]);
    let view = self.add_view(ARRAY_BUFFER, |bin| {
      bin.reserve(data.len() * N * 4);
      for v in data {
        let a = f(v);
        for c in 0..N {
          bin.extend_from_slice(&a[c].to_le_bytes());
          lo[c] = lo[c].min(a[c]);
          hi[c] = hi[c].max(a[c]);
        }
      }
    });
    let acc = self.accessor(view, data.len(), FLOAT, ty);
    if minmax && !data.is_empty() {
      self.accessors[acc]["min"] = json!(lo.to_vec());
      self.accessors[acc]["max"] = json!(hi.to_vec());
    }
    acc
  }

  fn add_u8(&mut self, data: &[u8], count: usize, target: u32, ty: &str) -> usize {
    let view = self.add_view(target, |bin| bin.extend_from_slice(data));
    self.accessor(view, count, UNSIGNED_BYTE, ty)
  }

  fn add_u16(&mut self, data: &[u16], target: u32) -> usize {
    let view = self.add_view(target, |bin| {
      bin.reserve(data.len() * 2);
      for v in data {
        bin.extend_from_slice(&v.to_le_bytes());
      }
    });
    self.accessor(view, data.len(), UNSIGNED_SHORT, "SCALAR")
  }

  fn add_u32_view(&mut self, data: &[u32], target: u32) -> usize {
    self.add_view(target, |bin| {
      bin.reserve(data.len() * 4);
      for v in data {
        bin.extend_from_slice(&v.to_le_bytes());
      }
    })
  }

  fn write(mut self, mut doc: Value, path: &Path) -> Result<(), String> {
    doc["bufferViews"] = Value::Array(std::mem::take(&mut self.views));
    doc["accessors"] = Value::Array(std::mem::take(&mut self.accessors));
    // no geometry (an empty document): no buffer, no binary chunk (a buffer is never empty)
    if !self.bin.is_empty() {
      doc["buffers"] = json!([{"byteLength": self.bin.len()}]);
    }
    let mut text = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    while !text.len().is_multiple_of(4) {
      text.push(b' ');
    }
    while !self.bin.len().is_multiple_of(4) {
      self.bin.push(0);
    }
    let total = 12
      + 8
      + text.len()
      + if self.bin.is_empty() {
        0
      } else {
        8 + self.bin.len()
      };
    let cannot = |_| format!("cannot write {}", path.display());
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(cannot)?);
    let mut head = Vec::with_capacity(20);
    for v in [
      0x46546C67u32,
      2,
      total as u32,
      text.len() as u32,
      0x4E4F534A,
    ] {
      head.extend_from_slice(&v.to_le_bytes()); // "glTF", version, length; the JSON chunk
    }
    f.write_all(&head).map_err(cannot)?;
    f.write_all(&text).map_err(cannot)?;
    if !self.bin.is_empty() {
      f.write_all(&(self.bin.len() as u32).to_le_bytes())
        .map_err(cannot)?;
      f.write_all(&0x004E4942u32.to_le_bytes()).map_err(cannot)?; // BIN
      f.write_all(&self.bin).map_err(cannot)?;
    }
    f.flush().map_err(cannot)
  }
}

fn png(width: i32, height: i32, rgba: &[u8]) -> Option<Vec<u8>> {
  use image::ImageEncoder;
  let mut out = Vec::new();
  image::codecs::png::PngEncoder::new(&mut out)
    .write_image(
      rgba,
      width as u32,
      height as u32,
      image::ExtendedColorType::Rgba8,
    )
    .ok()?;
  Some(out)
}

struct ExportContext<'a> {
  scene: &'a Scene,
  w: GlbWriter,
  nodes: Vec<Value>,
  meshes: Vec<Value>,
  materials: Vec<Value>,
  images: Vec<Value>,
  textures: Vec<Value>,
  /// scene material -> gltf material
  material_map: HashMap<i32, i32>,
  /// scene texture, alpha doubled -> gltf texture
  texture_map: HashMap<(i32, bool), i32>,
  /// mesh uid -> gltf mesh
  mesh_map: HashMap<u64, usize>,
  /// the nodes' lighting -> accessor
  palette_map: HashMap<usize, usize>,
  /// builder: the geometry of each primitive (mesh uid, primitive), written once for the nodes
  /// sharing its mesh, each with its own lighting
  prim_map: HashMap<(u64, usize), Value>,
  /// builder: the lighting of the geometry without its own
  tod_ratios: [f32; 8],
  builder: bool,
}

impl<'a> ExportContext<'a> {
  fn new(scene: &'a Scene) -> Self {
    Self {
      scene,
      w: GlbWriter::default(),
      nodes: Vec::new(),
      meshes: Vec::new(),
      materials: Vec::new(),
      images: Vec::new(),
      textures: Vec::new(),
      material_map: HashMap::new(),
      texture_map: HashMap::new(),
      mesh_map: HashMap::new(),
      palette_map: HashMap::new(),
      prim_map: HashMap::new(),
      tod_ratios: [0.0; 8],
      builder: false,
    }
  }

  /// `double_alpha`: a texture of the game, its alpha the PS2's (128 is opaque), for the builder,
  /// which halves the alpha of the files it reads
  fn texture(&mut self, ti: i32, double_alpha: bool) -> i32 {
    if let Some(&gi) = self.texture_map.get(&(ti, double_alpha)) {
      return gi;
    }
    let Some(t) = usize::try_from(ti)
      .ok()
      .and_then(|i| self.scene.textures.get(i))
    else {
      return -1;
    };
    if t.width <= 0 || t.height <= 0 || t.rgba.len() != t.width as usize * t.height as usize * 4 {
      return -1;
    }
    let encoded = if double_alpha {
      let mut rgba = t.rgba.clone();
      for a in rgba.iter_mut().skip(3).step_by(4) {
        *a = (*a as u32 * 2).min(255) as u8;
      }
      png(t.width, t.height, &rgba)
    } else {
      png(t.width, t.height, &t.rgba)
    };
    let Some(encoded) = encoded else { return -1 };
    let view = self.w.add_view(0, |bin| bin.extend_from_slice(&encoded));
    self
      .images
      .push(json!({"name": t.name, "bufferView": view, "mimeType": "image/png"}));
    self
      .textures
      .push(json!({"source": self.images.len() - 1, "sampler": 0}));
    let gi = self.textures.len() as i32 - 1;
    self.texture_map.insert((ti, double_alpha), gi);
    gi
  }

  fn material(&mut self, mi: i32) -> i32 {
    if let Some(&gi) = self.material_map.get(&mi) {
      return gi;
    }
    let Some(m) = usize::try_from(mi)
      .ok()
      .and_then(|i| self.scene.materials.get(i))
    else {
      return -1;
    };
    let c = m.base_color;
    let mut pbr = json!({"baseColorFactor": [c.x, c.y, c.z, c.w], "metallicFactor": 0.0, "roughnessFactor": 1.0});
    // a material of the game (its colors doubled, as the PS2 blends them: its texture's alpha is
    // the PS2's): for the builder, the alpha of its texture and its reference as a file has them
    let game_alpha = self.builder && m.base_color.w > 1.5;
    let cutoff = if game_alpha {
      (m.cutoff * 2.0).min(1.0)
    } else {
      m.cutoff
    };
    let ti = self.texture(m.texture, game_alpha);
    if ti >= 0 {
      pbr["baseColorTexture"] = json!({"index": ti});
    }
    let mut jm =
      json!({"name": m.name, "pbrMetallicRoughness": pbr, "doubleSided": m.double_sided});
    if m.alpha == AlphaMode::Mask {
      jm["alphaMode"] = json!("MASK");
      jm["alphaCutoff"] = json!(cutoff);
    } else if m.alpha == AlphaMode::Blend && self.builder && translucent_in_game(m) {
      // see through: the level builder's translucent tree, with the PS2 blend and the texels under
      // the reference dropped (og_blend: jak-project's builder of the editor's branch; the game's
      // own builder blends them over what is behind)
      jm["alphaMode"] = json!("BLEND");
      jm["alphaCutoff"] = json!(cutoff);
      let blend = if m.blend == BlendMode::Alpha {
        "alpha"
      } else {
        blend_mode_name(m.blend)
      };
      jm["extras"] = json!({"og_blend": blend});
    } else if m.alpha == AlphaMode::Blend && self.builder {
      // a wall the game blends and writes the depth of: in the builder's tree of solid surfaces,
      // its texels under the reference cut out (the soft edges of its texture are lost)
      jm["alphaMode"] = json!("MASK");
      jm["alphaCutoff"] = json!(cutoff);
    } else if m.alpha == AlphaMode::Blend {
      // the PS2 also drops the texels of blended surfaces under a reference: kept like the
      // extractor writes it (without it, water and glass would come back with glTF's 0.5)
      jm["alphaMode"] = json!("BLEND");
      jm["alphaCutoff"] = json!(m.cutoff);
    }
    if m.blend != BlendMode::Alpha && jm.get("extras").is_none() {
      jm["extras"] = json!({"og_blend": blend_mode_name(m.blend)});
    }
    if m.translucent && !self.builder {
      jm["extras"]["og_translucent"] = json!(true);
    }
    self.materials.push(jm);
    let gi = self.materials.len() as i32 - 1;
    self.material_map.insert(mi, gi);
    gi
  }

  fn primitive(&mut self, p: &Primitive, with_material: bool) -> Value {
    let mut attrs = serde_json::Map::new();
    attrs.insert(
      "POSITION".into(),
      self
        .w
        .add_floats(&p.pos, |v| (*v).into(), "VEC3", true)
        .into(),
    );
    let computed;
    let nrm = if p.nrm.len() == p.pos.len() {
      &p.nrm
    } else {
      let mut tmp = Primitive {
        pos: p.pos.clone(),
        idx: p.idx.clone(),
        ..Default::default()
      };
      tmp.compute_normals();
      computed = tmp.nrm;
      &computed
    };
    attrs.insert(
      "NORMAL".into(),
      self
        .w
        .add_floats(nrm, |v| (*v).into(), "VEC3", false)
        .into(),
    );
    if p.uv.len() == p.pos.len() {
      attrs.insert(
        "TEXCOORD_0".into(),
        self
          .w
          .add_floats(&p.uv, |v| (*v).into(), "VEC2", false)
          .into(),
      );
    }
    if p.col.len() == p.pos.len() {
      attrs.insert(
        "COLOR_0".into(),
        self
          .w
          .add_floats(&p.col, |v| (*v).into(), "VEC4", false)
          .into(),
      );
    }
    // (for the builder, the nodes' lighting replaces the palettes a file brought)
    for (name, data) in &p.extra_attrs {
      if data.len() == p.pos.len() && !self.builder {
        attrs.insert(
          name.clone(),
          self
            .w
            .add_floats(data, |v| (*v).into(), "VEC4", false)
            .into(),
        );
      }
    }
    let view = self.w.add_u32_view(&p.idx, ELEMENT_ARRAY_BUFFER);
    let indices = self.w.accessor(view, p.idx.len(), UNSIGNED_INT, "SCALAR");
    let mut jp = json!({"mode": 4, "indices": indices});
    jp["attributes"] = Value::Object(attrs);
    if with_material && p.material >= 0 {
      let mi = self.material(p.material);
      if mi >= 0 {
        jp["material"] = json!(mi);
      }
    }
    jp
  }

  /// the whole mesh, shared between nodes (project and object exports)
  fn mesh(&mut self, m: &Mesh, collision: bool) -> usize {
    if let Some(&gi) = self.mesh_map.get(&m.uid) {
      return gi;
    }
    let mut prims = Vec::new();
    let mut pat_views: Vec<i64> = Vec::new();
    for p in &m.prims {
      if p.idx.is_empty() {
        continue;
      }
      prims.push(self.primitive(p, !collision));
      if collision {
        pat_views.push(if p.tri_pat.len() == p.tri_count() {
          self.w.add_u32_view(&p.tri_pat, 0) as i64
        } else {
          -1
        });
      }
    }
    let mut jm = json!({"name": m.name});
    jm["primitives"] = Value::Array(prims);
    let mut extras = if m.extras.is_object() {
      m.extras.clone()
    } else {
      json!({})
    };
    if collision {
      extras["og_tri_pat_views"] = json!(pat_views);
    }
    if !extras.as_object().unwrap().is_empty() {
      jm["extras"] = extras;
    }
    self.meshes.push(jm);
    let gi = self.meshes.len() - 1;
    self.mesh_map.insert(m.uid, gi);
    gi
  }

  /// a node's lighting (og_tod): its palette, once, and an index per primitive written
  fn tod(&mut self, mesh: &Mesh, tod: &TodColors) -> Value {
    let pal = tod.palette.as_ref().unwrap();
    let key = Arc::as_ptr(pal) as usize;
    let palette = match self.palette_map.get(&key) {
      Some(&a) => a,
      None => {
        let a = self.w.add_u8(&pal.rgba, pal.rgba.len() / 4, 0, "VEC4");
        self.palette_map.insert(key, a);
        a
      }
    };
    let mut index = Vec::new();
    for (p, v) in mesh.prims.iter().zip(&tod.index) {
      if !p.idx.is_empty() {
        index.push(self.w.add_u16(v, 0));
      }
    }
    let mut out = json!({"palette": palette, "index": index});
    if pal.local {
      out["local"] = json!(true);
    }
    out
  }

  /// builder: the node's own mesh, its lighting as the 8 palettes per vertex the builder reads
  /// (_SUNRISE...: the names of Jak 1's times of the day), on the geometry of its mesh
  fn builder_mesh(&mut self, n: &Node, m: &Mesh) -> usize {
    const PALETTES: [&str; 8] = [
      "_SUNRISE",
      "_MORNING",
      "_NOON",
      "_AFTERNOON",
      "_SUNSET",
      "_TWILIGHT",
      "_EVENING",
      "_GREENSUN",
    ];
    let lit = n.tod.as_ref().filter(|t| t.fits(m));
    let mut prims = Vec::new();
    for (pi, p) in m.prims.iter().enumerate() {
      if p.idx.is_empty() {
        continue;
      }
      let mut jp = match self.prim_map.get(&(m.uid, pi)) {
        Some(jp) => jp.clone(),
        None => {
          let jp = self.primitive(p, true);
          self.prim_map.insert((m.uid, pi), jp.clone());
          jp
        }
      };
      // in the game's space: the builder's own product of the node's matrix by each vertex
      // rounds them its own way (in float), and a coordinate far from the origin (beyond 4.4 km)
      // can fail the check of its packing; each one written where it packs (packable)
      let w = self.scene.world(n);
      let pos: Vec<Vec3> = p
        .pos
        .iter()
        .map(|&v| w.point(v))
        .map(|v| Vec3::new(packable(v.x), packable(v.y), packable(v.z)))
        .collect();
      let nrm = if p.nrm.len() == p.pos.len() {
        let nm = w.inverse().transposed();
        p.nrm
          .iter()
          .map(|&d| crate::math::normalize(nm.dir(d)))
          .collect()
      } else {
        let mut tmp = Primitive {
          pos: pos.clone(),
          idx: p.idx.clone(),
          ..Default::default()
        };
        tmp.compute_normals();
        tmp.nrm
      };
      jp["attributes"]["POSITION"] = json!(self.w.add_floats(&pos, |v| (*v).into(), "VEC3", true));
      jp["attributes"]["NORMAL"] = json!(self.w.add_floats(&nrm, |v| (*v).into(), "VEC3", false));
      let nv = p.pos.len();
      let mut pal: [Vec<u8>; 8] = std::array::from_fn(|_| vec![0u8; nv * 4]);
      let mat = usize::try_from(p.material)
        .ok()
        .and_then(|i| self.scene.materials.get(i));
      for v in 0..nv {
        let c: [u8; 32] = match lit {
          Some(tod) => {
            let at = tod.index[pi][v] as usize * 32;
            tod
              .palette
              .as_ref()
              .unwrap()
              .rgba
              .get(at..at + 32)
              .map_or([0; 32], |s| s.try_into().unwrap())
          }
          None => {
            // new geometry: lit like the level's, from the color shown
            let mut shown = p
              .col
              .get(v)
              .copied()
              .unwrap_or(Vec4::new(1.0, 1.0, 1.0, 1.0));
            if let Some(mat) = mat {
              let b = mat.base_color;
              shown = Vec4::new(shown.x * b.x, shown.y * b.y, shown.z * b.z, shown.w);
            }
            tod_from_color(shown, &self.tod_ratios)
          }
        };
        // the alpha the PS2's (128 is 1.0): the builder halves the alpha it reads, as a texture's
        for k in 0..8 {
          pal[k][v * 4..v * 4 + 4].copy_from_slice(&c[k * 4..k * 4 + 4]);
          pal[k][v * 4 + 3] = (c[k * 4 + 3] as u32 * 2).min(255) as u8;
        }
      }
      for k in 0..8 {
        jp["attributes"][PALETTES[k]] = json!(self.w.add_u8(&pal[k], nv, ARRAY_BUFFER, "VEC4"));
      }
      prims.push(jp);
    }
    let mut jm = json!({"name": m.name});
    jm["primitives"] = Value::Array(prims);
    self.meshes.push(jm);
    self.meshes.len() - 1
  }

  fn add_node(&mut self, jn: Value) -> usize {
    self.nodes.push(jn);
    self.nodes.len() - 1
  }

  fn write(
    self,
    scene_roots: Vec<usize>,
    asset_extras: Option<Value>,
    path: &Path,
  ) -> Result<(), String> {
    let mut doc = json!({
      "asset": {"version": "2.0", "generator": "jak-level-editor"},
      "scenes": [{"nodes": scene_roots}],
      "scene": 0,
    });
    doc["nodes"] = Value::Array(self.nodes);
    if let Some(e) = asset_extras {
      doc["asset"]["extras"] = e;
    }
    if !self.meshes.is_empty() {
      doc["meshes"] = Value::Array(self.meshes);
    }
    if !self.materials.is_empty() {
      doc["materials"] = Value::Array(self.materials);
    }
    if !self.images.is_empty() {
      doc["images"] = Value::Array(self.images);
      doc["textures"] = Value::Array(self.textures);
      // linear, repeat. The builder asserts that every texture has a sampler.
      doc["samplers"] =
        json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]);
    }
    self.w.write(doc, path)
  }
}

/// Whether OpenGOAL's level builder packs a coordinate of the decor (meters): its
/// position_to_cluster_and_offset (common/custom_data/pack_helpers.cpp), in float as it computes
/// it, gets it back within the 7 units (4096 a meter) it checks, else stops the build
pub(crate) fn packs(x: f32) -> bool {
  const CLUSTER: f32 = 4096.0 * 40.0;
  const MASTER: f32 = 12000.0 * 4096.0;
  let i = x * 4096.0 + MASTER;
  let cell = (i / CLUSTER) as i32;
  let left = i - cell as f32 * CLUSTER;
  let offset = ((left / CLUSTER) * 65535.0) as u16;
  let back = (cell as f32 + offset as f32 / 65535.0) * CLUSTER;
  (back - i).abs() < 7.0
}

/// `x` moved by the fewest units (4096 a meter: 2 mm at most within 25 km of the origin) that let
/// the level builder pack it (packs; within 4.4 km of the origin every coordinate does)
pub(crate) fn packable(x: f32) -> f32 {
  (0..=32)
    .flat_map(|k| [k, -k])
    .map(|k| x + k as f32 / 4096.0)
    .find(|&y| packs(y))
    .unwrap_or(x)
}

fn trs_json(jn: &mut Value, t: &Transform) {
  jn["translation"] = json!([t.t.x, t.t.y, t.t.z]);
  jn["rotation"] = json!([t.r.x, t.r.y, t.r.z, t.r.w]);
  jn["scale"] = json!([t.s.x, t.s.y, t.s.z]);
}

/// nlohmann's json::empty(): null, or an empty object or array
fn json_empty(v: &Value) -> bool {
  match v {
    Value::Null => true,
    Value::Object(o) => o.is_empty(),
    Value::Array(a) => a.is_empty(),
    _ => false,
  }
}

/// Writes the scene (or a part of it, see ExportMode) as a .glb.
pub fn export_glb(
  scene: &Scene,
  path: impl AsRef<Path>,
  opts: &ExportOptions,
) -> Result<(), String> {
  let path = path.as_ref();
  let mut ctx = ExportContext::new(scene);
  let mut scene_roots = Vec::new();

  if opts.mode == ExportMode::Builder {
    // flat: every node at its world matrix, the builder's extras only
    ctx.builder = true;
    ctx.tod_ratios = tod_ratios(scene, &opts.only);
    for &id in &opts.only {
      let Some(n) = scene.find(id) else { continue };
      let mut jn = json!({"name": n.name});
      match &n.mesh {
        // its vertices in the game's space, the node without a matrix (builder_mesh)
        Some(mesh) => jn["mesh"] = json!(ctx.builder_mesh(n, mesh)),
        None => trs_json(&mut jn, &Transform::from_matrix(&scene.world(n))),
      }
      if let Some(e) = opts.node_extras.get(&id)
        && !json_empty(e)
      {
        jn["extras"] = e.clone();
      }
      scene_roots.push(ctx.add_node(jn));
    }
    for (mesh, extras) in &opts.extra_meshes {
      let mut jn = json!({"name": mesh.name, "mesh": ctx.mesh(mesh, false)});
      if !json_empty(extras) {
        jn["extras"] = extras.clone();
      }
      scene_roots.push(ctx.add_node(jn));
    }
    return ctx.write(scene_roots, None, path);
  }

  // hierarchy with local transforms and the editor's extras
  let pivot_inv = opts.pivot.inverse();
  let included: HashSet<u32> = if opts.mode == ExportMode::Prefab {
    let mut set: HashSet<u32> = opts.only.iter().copied().collect();
    for d in scene.descendants_of(&opts.only) {
      set.extend(d);
    }
    set
  } else {
    scene.nodes.iter().map(|n| n.id).collect()
  };
  let mut gltf_index: HashMap<u32, usize> = HashMap::with_capacity(included.len());
  // first pass: create the nodes
  for n in &scene.nodes {
    if !included.contains(&n.id) {
      continue;
    }
    let mut jn = json!({"name": n.name});
    let mut t = n.local;
    if opts.mode == ExportMode::Prefab && !included.contains(&n.parent) {
      // its parents are not in the file: its world matrix, relative to the pivot
      t = Transform::from_matrix(&(pivot_inv * scene.world(n)));
    }
    trs_json(&mut jn, &t);
    if let Some(mesh) = &n.mesh {
      jn["mesh"] = json!(ctx.mesh(mesh, n.kind == NodeKind::Collision));
    }
    let mut extras = if n.extras.is_object() {
      n.extras.clone()
    } else {
      json!({})
    };
    extras["og_kind"] = json!(kind_name(n.kind));
    if let (Some(mesh), Some(tod)) = (&n.mesh, &n.tod)
      && tod.fits(mesh)
    {
      extras["og_tod"] = ctx.tod(mesh, tod);
    }
    if opts.mode == ExportMode::Project {
      extras["og_id"] = json!(n.id);
    }
    if !n.visible {
      extras["og_hidden"] = json!(true);
    }
    if n.locked {
      extras["og_locked"] = json!(true);
    }
    jn["extras"] = extras;
    gltf_index.insert(n.id, ctx.add_node(jn));
  }
  // second pass: children and roots
  for n in &scene.nodes {
    let Some(&gi) = gltf_index.get(&n.id) else {
      continue;
    };
    match gltf_index.get(&n.parent) {
      Some(&parent) => {
        let p = &mut ctx.nodes[parent];
        if p.get("children").is_none() {
          p["children"] = json!([]);
        }
        p["children"].as_array_mut().unwrap().push(json!(gi));
      }
      None => scene_roots.push(gi),
    }
  }
  let asset_extras = (opts.mode == ExportMode::Project)
    .then(|| json!({"ogle": {"settings": scene.settings.to_json()}}));
  ctx.write(scene_roots, asset_extras, path)
}
