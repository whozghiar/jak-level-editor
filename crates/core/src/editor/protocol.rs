//! The jle:// protocol's answers (docs/architecture.md §14), little-endian: the scene (in full
//! or what changed since the last answer), a mesh, a texture, the materials. The plugin serves
//! them: it copies what it needs under the editor's lock (`scene_records`, an `Arc` of a mesh, a
//! texture's bytes), then encodes without it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use super::Editor;
use crate::io::game_data::data_dir;
use crate::scene::{AlphaMode, Mesh, Scene, Texture, blend_mode_name};

/// A node as the scene's answer gives it (84 bytes)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeRecord {
  pub id: u32,
  pub parent: u32,
  pub kind: u8,
  /// 1 visible (with its parents, and the story), 2 locked, 4 reference level, 8 under an actor
  /// (its model), 16 has game lighting, 32 selected, 64 water
  pub flags: u8,
  /// an actor: the low 16 bits of the FNV-1a hash of its etype (the color of its marker)
  pub extra: u16,
  /// low 32 bits of its mesh's uid, 0 none
  pub mesh: u32,
  pub mesh_version: u32,
  pub world: [f32; 16],
}

pub const FLAG_VISIBLE: u8 = 1;
pub const FLAG_LOCKED: u8 = 2;
pub const FLAG_REFERENCE: u8 = 4;
pub const FLAG_UNDER_ACTOR: u8 = 8;
pub const FLAG_GAME_LIGHT: u8 = 16;
pub const FLAG_SELECTED: u8 = 32;
pub const FLAG_WATER: u8 = 64;

/// What the scene's answers are made from: every node's record, and the meshes by uid (the
/// document's and the ocean's) for `mesh/<uid>`.
#[derive(Default)]
pub struct SceneSnapshot {
  pub records: Vec<NodeRecord>,
  pub meshes: HashMap<u32, Arc<Mesh>>,
}

fn fnv1a32(s: &str) -> u32 {
  s.bytes()
    .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619))
}

impl Editor {
  /// The records of the scene as it is now (under the lock: copies, no encoding).
  pub fn scene_snapshot(&self) -> SceneSnapshot {
    let scene = &self.scene;
    let cache = scene.cache();
    let water: Vec<bool> = scene.materials.iter().map(|m| m.water).collect();
    let mut snap = SceneSnapshot {
      records: Vec::with_capacity(scene.nodes.len()),
      meshes: HashMap::new(),
    };
    for (i, n) in scene.nodes.iter().enumerate().take(cache.count) {
      let mut flags = 0;
      if cache.visible[i] {
        flags |= FLAG_VISIBLE;
      }
      if n.locked {
        flags |= FLAG_LOCKED;
      }
      if cache.reference[i] {
        flags |= FLAG_REFERENCE;
      }
      if cache.actor[i] != 0 {
        flags |= FLAG_UNDER_ACTOR;
      }
      if self.selection.contains(&n.id) {
        flags |= FLAG_SELECTED;
      }
      let (mut mesh, mut mesh_version) = (0, 0);
      if let Some(m) = &n.mesh {
        mesh = m.uid as u32;
        mesh_version = m.version as u32;
        if n.tod.as_ref().is_some_and(|t| t.fits(m)) {
          flags |= FLAG_GAME_LIGHT;
        }
        let is_water = |p: &crate::scene::Primitive| {
          usize::try_from(p.material).is_ok_and(|k| water.get(k).copied().unwrap_or(false))
        };
        if m.prims.iter().any(is_water) {
          flags |= FLAG_WATER;
        }
        snap.meshes.entry(mesh).or_insert_with(|| m.clone());
      }
      let extra = if n.kind == crate::scene::NodeKind::Actor {
        let etype = n.extras.get("etype").and_then(Value::as_str);
        etype.map_or(0, |e| fnv1a32(e) as u16)
      } else {
        0
      };
      snap.records.push(NodeRecord {
        id: n.id,
        parent: n.parent,
        kind: n.kind as u8,
        flags,
        extra,
        mesh,
        mesh_version,
        world: cache.worlds[i].m,
      });
    }
    if let Some(o) = self.ocean_mesh() {
      snap.meshes.insert(o.uid as u32, o.clone());
    }
    if let Some(p) = self.placement_preview_mesh() {
      snap.meshes.insert(p.uid as u32, p.clone());
    }
    if let Some(i) = self.preview_icon_mesh() {
      snap.meshes.insert(i.uid as u32, i);
    }
    snap
  }

  /// A texture's answer, encoded under the lock (a copy of its bytes anyway)
  pub fn texture_bytes(&self, index: usize) -> Option<Vec<u8>> {
    self.scene.textures.get(index).map(encode_texture)
  }

  /// A file of the library or of data/ (`asset?path=`): an absolute path in one of them, or
  /// `library/<path in the library>`, `data/<path in data/>`. Nothing outside them is served.
  pub fn asset_path(&self, path: &str) -> Option<PathBuf> {
    let path = path.replace('\\', "/");
    let path = if let Some(rest) = path.strip_prefix("library/") {
      Path::new(&self.library_root).join(rest)
    } else if let Some(rest) = path.strip_prefix("data/") {
      data_dir().join(rest)
    } else {
      PathBuf::from(&path)
    };
    let file = std::fs::canonicalize(path).ok()?;
    let roots = [PathBuf::from(&self.library_root), data_dir()];
    roots
      .iter()
      .filter(|r| !r.as_os_str().is_empty())
      .filter_map(|r| std::fs::canonicalize(r).ok())
      .any(|r| file.starts_with(&r))
      .then_some(file)
      .filter(|f| f.is_file())
  }
}

/// The server side of the scene's answers: the last records sent, to send only what changed.
#[derive(Default)]
pub struct ProtocolState {
  last: HashMap<u32, NodeRecord>,
  /// the meshes of the last snapshot, by uid
  pub meshes: HashMap<u32, Arc<Mesh>>,
}

fn put(out: &mut Vec<u8>, v: u32) {
  out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32s(out: &mut Vec<u8>, v: impl IntoIterator<Item = f32>) {
  for f in v {
    out.extend_from_slice(&f.to_le_bytes());
  }
}

impl ProtocolState {
  /// `scene` (JLES 1): `u32 "JLES", u32 1, u32 full, u32 count`, the records, `u32 removed`, the
  /// ids removed. A delta holds the nodes that changed since the last answer (their selected flag
  /// too) and the ids gone; `full` every node.
  pub fn encode_scene(&mut self, snap: SceneSnapshot, full: bool) -> Vec<u8> {
    let mut changed: Vec<&NodeRecord> = Vec::new();
    let mut now = HashMap::with_capacity(snap.records.len());
    for r in &snap.records {
      if full || self.last.get(&r.id) != Some(r) {
        changed.push(r);
      }
      now.insert(r.id, *r);
    }
    let removed: Vec<u32> = if full {
      Vec::new()
    } else {
      self
        .last
        .keys()
        .copied()
        .filter(|id| !now.contains_key(id))
        .collect()
    };
    let mut out = Vec::with_capacity(20 + changed.len() * 84 + removed.len() * 4);
    out.extend_from_slice(b"JLES");
    put(&mut out, 1);
    put(&mut out, full as u32);
    put(&mut out, changed.len() as u32);
    for r in changed {
      put(&mut out, r.id);
      put(&mut out, r.parent);
      out.extend_from_slice(&[r.kind, r.flags]);
      out.extend_from_slice(&r.extra.to_le_bytes());
      put(&mut out, r.mesh);
      put(&mut out, r.mesh_version);
      put_f32s(&mut out, r.world);
    }
    put(&mut out, removed.len() as u32);
    for id in removed {
      put(&mut out, id);
    }
    self.last = now;
    self.meshes = snap.meshes;
    out
  }
}

/// `mesh/<uid>` (JLEM 1): `u32 "JLEM", u32 1, u32 prims`, then per primitive `u32 vertices, u32
/// indices, i32 material, u32 flags (1 uv, 2 colors, 4 backdrop)` followed by its arrays: `f32
/// pos[3n], f32 nrm[3n], f32 uv[2n]` (if any), `u8 col[4n]` (if any), `u32 idx[m]`.
pub fn encode_mesh(mesh: &Mesh) -> Vec<u8> {
  let backdrop = mesh
    .extras
    .get("og_backdrop")
    .and_then(Value::as_bool)
    .unwrap_or(false);
  let size: usize = mesh
    .prims
    .iter()
    .map(|p| 16 + p.pos.len() * 36 + p.idx.len() * 4)
    .sum();
  let mut out = Vec::with_capacity(12 + size);
  out.extend_from_slice(b"JLEM");
  put(&mut out, 1);
  put(&mut out, mesh.prims.len() as u32);
  for p in &mesh.prims {
    let n = p.pos.len();
    let uv = !p.uv.is_empty() && p.uv.len() == n;
    let col = !p.col.is_empty() && p.col.len() == n;
    put(&mut out, n as u32);
    put(&mut out, p.idx.len() as u32);
    out.extend_from_slice(&p.material.to_le_bytes());
    put(
      &mut out,
      uv as u32 | (col as u32) << 1 | (backdrop as u32) << 2,
    );
    put_f32s(&mut out, p.pos.iter().flat_map(|v| [v.x, v.y, v.z]));
    if p.nrm.len() == n {
      put_f32s(&mut out, p.nrm.iter().flat_map(|v| [v.x, v.y, v.z]));
    } else {
      put_f32s(&mut out, (0..n).flat_map(|_| [0.0, 1.0, 0.0]));
    }
    if uv {
      put_f32s(&mut out, p.uv.iter().flat_map(|v| [v.x, v.y]));
    }
    if col {
      let b = |f: f32| (f * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
      for c in &p.col {
        out.extend_from_slice(&[b(c.x), b(c.y), b(c.z), b(c.w)]);
      }
    }
    for &i in &p.idx {
      put(&mut out, i);
    }
  }
  out
}

/// `texture/<index>`: `u32 width, u32 height`, RGBA bytes
pub fn encode_texture(t: &Texture) -> Vec<u8> {
  let mut out = Vec::with_capacity(8 + t.rgba.len());
  put(&mut out, t.width.max(0) as u32);
  put(&mut out, t.height.max(0) as u32);
  out.extend_from_slice(&t.rgba);
  out
}

/// `materials`: `{"materials": [...], "textures": [{"version": n}, ...]}`
pub fn materials_json(scene: &Scene) -> Value {
  let materials: Vec<Value> = scene
    .materials
    .iter()
    .map(|m| {
      json!({
        "name": m.name,
        "texture": m.texture,
        "base_color": m.base_color,
        "alpha": match m.alpha {
          AlphaMode::Opaque => "opaque",
          AlphaMode::Mask => "mask",
          AlphaMode::Blend => "blend",
        },
        "blend": blend_mode_name(m.blend),
        "cutoff": m.cutoff,
        "double_sided": m.double_sided,
        "translucent": m.translucent,
        "water": m.water,
      })
    })
    .collect();
  let textures: Vec<Value> = scene
    .textures
    .iter()
    .map(|t| json!({"version": t.version}))
    .collect();
  json!({"materials": materials, "textures": textures})
}

/// The media type of a file served by `asset?path=`
pub fn content_type(path: &Path) -> &'static str {
  match path
    .extension()
    .map(|e| e.to_string_lossy().to_lowercase())
    .as_deref()
  {
    Some("glb") => "model/gltf-binary",
    Some("gltf") | Some("json") => "application/json",
    Some("png") => "image/png",
    Some("jpg") | Some("jpeg") => "image/jpeg",
    _ => "application/octet-stream",
  }
}
