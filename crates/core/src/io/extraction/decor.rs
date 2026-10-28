//! The decor of a level in the library, `<level>/decor/<level>-background.glb`, written from the
//! level's .fr3 as ogle-extract wrote it (the decompiler's fr3_to_gltf.cpp,
//! save_level_background_as_gltf, as the editor's extractor had changed it):
//!
//! - `<level>-tfrag-<kind>`: the level's shell split in pieces, the triangles their vertices
//!   connect (a building, a wall, a bridge); the large flat ones are the ground (`og_terrain`)
//! - `<level>-tie-<n>`, `<level>-shrub-<n>`: a group per prototype holding a node per instance with
//!   its matrix, the instances of a prototype sharing its mesh
//! - `<level>-collision`: a mesh per surface (pat), `<level>-actors`: a node per actor
//!
//! A vertex color is the brightest of the level's 8 light palettes (Jak II and Jak 3 mix them as
//! light layers); the palettes themselves are kept for the game view and the export, in the nodes'
//! extras `og_tod` (`palette`: an accessor of 8 RGBA per color, `index`: per primitive an accessor
//! of each vertex's color). Materials keep what glTF cannot say: `og_blend` (the PS2's additive and
//! subtractive blends), `og_translucent` (drawn without writing the depth: glass, water, stains).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde_json::{Map, Value, json};

use super::fr3::{
  DrawMode, Level, Matrix, ShrubTree, TFRAG_TREE_NAMES, TfragTree, TieTree, TimeOfDay,
  is_envmap_first_draw, is_envmap_second_draw,
};
use super::strips::{unstrip_plain, unstrip_tfrag_tie};
use crate::io::gltf::{ELEMENT_ARRAY_BUFFER, GlbWriter, UNSIGNED_INT, png};

/// A tfrag piece this wide or deep (meters), mostly flat, is the ground: the editor locks it.
const TERRAIN_EXTENT: f32 = 60.0;
/// The alpha test of the game's renderers
const ALPHA_CUTOFF: f32 = 0x26 as f32 / 255.0;
const LINEAR: u32 = 9729;
const NEAREST: u32 = 9728;
const REPEAT: u32 = 10497;
const CLAMP: u32 = 33071;

/// The color of a vertex for the editor: the brightest of the 8 palettes, channel by channel (the
/// lamps of each area light it: palette 0 alone is black indoors)
fn lit_color(tod: &TimeOfDay, color: u16) -> [f32; 4] {
  let mut c = [1.0; 4];
  for (ch, v) in c.iter_mut().take(3).enumerate() {
    let best = (0..8)
      .map(|k| tod.read(color as usize, k, ch))
      .max()
      .unwrap_or(0);
    *v = best as f32 / 255.0;
  }
  c
}

/// The alphaMode of a material drawn with `mode` (shiny: with an envmap), and its extras: what
/// glTF cannot say of its blend (decor and models)
pub(crate) fn material_alpha(mode: DrawMode, envmap: bool) -> (&'static str, Option<Value>) {
  // (Cs - Cs) * As + Cs blends nothing: the texture as it is, its texels under the reference cut
  let mut alpha = if mode.blend() && mode.alpha_blend() != 6 {
    "BLEND"
  } else {
    "MASK"
  };
  // glTF blends with the source alpha only: the other PS2 blends go to extras.og_blend
  let og_blend = match mode.alpha_blend() {
    _ if !mode.blend() => None,
    2 => Some("add_alpha"),
    3 | 7 => Some("add"),
    5 => Some("subtract"),
    4 => Some("half"),
    _ => None,
  };
  // drawn without its depth (or its texels failing the alpha test drawn without it): seen
  // through, as glass, water or light shafts
  let translucent =
    alpha == "BLEND" && (!mode.depth_write() || (mode.alpha_test() && mode.alpha_fail() == 1));
  let mut extras = Map::new();
  if let Some(b) = og_blend {
    extras.insert("og_blend".into(), json!(b));
  }
  if translucent {
    extras.insert("og_translucent".into(), json!(true));
  }
  // a shine over an opaque base
  let src_alpha = mode.blend() && matches!(mode.alpha_blend(), 1 | 2 | 5);
  if envmap && !src_alpha && og_blend.is_none() {
    alpha = "OPAQUE";
  }
  (alpha, (!extras.is_empty()).then_some(Value::Object(extras)))
}

/// A tree's palettes: 8 RGBA per color, color c's palette k at c * 8 + k
fn tod_bytes(tod: &TimeOfDay) -> Vec<u8> {
  let mut out = Vec::with_capacity(tod.colors() * 32);
  for c in 0..tod.colors() {
    for k in 0..8 {
      for ch in 0..4 {
        out.push(tod.read(c, k, ch));
      }
    }
  }
  out
}

/// An instance matrix of the .fr3 (columns, translation in game units) as a glTF node matrix
fn instance_matrix(m: &Matrix) -> Vec<f64> {
  let mut out = Vec::with_capacity(16);
  for (c, col) in m.iter().enumerate() {
    for &x in &col[..3] {
      out.push(if c == 3 { x as f64 / 4096.0 } else { x as f64 });
    }
    out.push(if c == 3 { 1.0 } else { 0.0 });
  }
  out
}

fn proto_name(names: &[String], idx: u16, kind: &str) -> String {
  match names.get(idx as usize) {
    Some(n) if !n.is_empty() => n.clone(),
    _ => format!("{kind}-proto-{idx}"),
  }
}

/// A prototype's instance: its matrix (-1: vertices in world space), its triangles as indices of
/// the prototype's packed vertices per material, and for each packed vertex one unpacked vertex
/// (its color)
/// Triangles per material, as indices of a prototype's packed vertices
type Geometry = BTreeMap<usize, Vec<u32>>;

#[derive(Default)]
struct ProtoInstance {
  matrix_idx: i32,
  tris: Geometry,
  unpacked_of: HashMap<u32, u32>,
}

impl ProtoInstance {
  fn add(&mut self, packed: u32, unpacked: u32) -> u32 {
    self.unpacked_of.entry(packed).or_insert(unpacked);
    packed
  }
}

/// A glTF file of a level's geometry: the decor's, a model's (models.rs)
pub(super) struct Writer<'a> {
  level: &'a Level,
  pub(super) w: GlbWriter,
  pub(super) nodes: Vec<Value>,
  pub(super) meshes: Vec<Value>,
  pub(super) materials: Vec<Value>,
  pub(super) images: Vec<Value>,
  pub(super) textures: Vec<Value>,
  pub(super) samplers: Vec<Value>,
  roots: Vec<usize>,
  /// (texture, its envmap strength) -> image
  tex_images: HashMap<(usize, bool), usize>,
  pub(super) specular: bool,
}

impl<'a> Writer<'a> {
  pub(super) fn new(level: &'a Level) -> Self {
    Writer {
      level,
      w: GlbWriter::default(),
      nodes: Vec::new(),
      meshes: Vec::new(),
      // the material of the draws without texture
      materials: vec![json!({
        "doubleSided": true,
        "pbrMetallicRoughness": {"baseColorFactor": [1.0, 0.9f32, 0.9f32, 1.0], "metallicFactor": 0.0},
      })],
      images: Vec::new(),
      textures: Vec::new(),
      samplers: Vec::new(),
      roots: Vec::new(),
      tex_images: HashMap::new(),
      specular: false,
    }
  }

  fn node(&mut self, node: Value) -> usize {
    self.nodes.push(node);
    self.nodes.len() - 1
  }
  fn root(&mut self, name: String) -> usize {
    let n = self.node(json!({ "name": name }));
    self.roots.push(n);
    n
  }
  fn child(&mut self, parent: usize, node: Value) -> usize {
    let n = self.node(node);
    let children = self.nodes[parent]
      .as_object_mut()
      .unwrap()
      .entry("children")
      .or_insert(json!([]));
    children.as_array_mut().unwrap().push(json!(n));
    n
  }
  fn set_extra(&mut self, node: usize, key: &str, value: Value) {
    let extras = self.nodes[node]
      .as_object_mut()
      .unwrap()
      .entry("extras")
      .or_insert(json!({}));
    extras[key] = value;
  }
  /// og_tod: the node's palettes and, per primitive, its vertices' colors in them
  fn set_tod(&mut self, node: usize, palette: i64, index: usize, prims: usize) {
    self.set_extra(
      node,
      "og_tod",
      json!({"palette": palette, "index": vec![index; prims]}),
    );
  }

  fn floats<const N: usize>(&mut self, data: &[[f32; N]], position: bool) -> usize {
    let ty = ["", "SCALAR", "VEC2", "VEC3", "VEC4"][N];
    self.w.add_floats(data, |v| *v, ty, position)
  }
  fn tod_palette(&mut self, rgba8: &[u8]) -> usize {
    self.w.add_u8(rgba8, rgba8.len() / 4, 0, "VEC4")
  }
  fn tod_index(&mut self, index: &[u16]) -> usize {
    self.w.add_u16(index, 0)
  }
  fn index_view(&mut self, indices: &[u32]) -> usize {
    self.w.add_u32_view(indices, ELEMENT_ARRAY_BUFFER)
  }
  fn index_range(&mut self, view: usize, start: u32, count: u32) -> usize {
    let a = self
      .w
      .accessor(view, count as usize, UNSIGNED_INT, "SCALAR");
    if start > 0 {
      self.w.accessors[a]["byteOffset"] = json!(start * 4);
    }
    a
  }
  pub(super) fn indices(&mut self, indices: &[u32]) -> usize {
    let view = self.index_view(indices);
    self.index_range(view, 0, indices.len() as u32)
  }
  fn mesh(&mut self, name: &str, prims: Vec<Value>) -> usize {
    self.meshes.push(json!({"name": name, "primitives": prims}));
    self.meshes.len() - 1
  }

  fn texture_name(&self, tex: i32) -> String {
    if tex < 0 {
      format!("anim-slot-{}", -(tex + 1))
    } else {
      self.level.textures[tex as usize].name.clone()
    }
  }

  fn image(&mut self, tex: usize, strength: bool) -> usize {
    if let Some(&i) = self.tex_images.get(&(tex, strength)) {
      return i;
    }
    let t = &self.level.textures[tex];
    let mut rgba = t.rgba.clone();
    let mut name = t.name.clone();
    if strength {
      // the envmap's strength as glTF metallic: the texture's alpha doubled
      name += "-envmap-strength";
      for p in rgba.chunks_exact_mut(4) {
        let metallic = (p[3] as u32 * 2).min(255) as u8;
        p.copy_from_slice(&[255, 0, metallic, 255]);
      }
    }
    let data = png(t.w as i32, t.h as i32, &rgba).unwrap_or_default();
    let view = self.w.add_view(0, |bin| bin.extend_from_slice(&data));
    self
      .images
      .push(json!({"name": name, "mimeType": "image/png", "bufferView": view}));
    let i = self.images.len() - 1;
    self.tex_images.insert((tex, strength), i);
    i
  }

  fn gltf_texture(&mut self, image: usize, name: &str, mode: DrawMode) -> usize {
    let filter = if mode.filter() { LINEAR } else { NEAREST };
    let wrap = |clamp: bool| if clamp { CLAMP } else { REPEAT };
    self.samplers.push(json!({
      "magFilter": filter, "minFilter": filter,
      "wrapS": wrap(mode.clamp_s()), "wrapT": wrap(mode.clamp_t()),
    }));
    self
      .textures
      .push(json!({"name": name, "sampler": self.samplers.len() - 1, "source": image}));
    self.textures.len() - 1
  }

  /// A material of a texture drawn with `mode` (0, the plain one, without texture)
  pub(super) fn material(
    &mut self,
    tex: i32,
    mode: DrawMode,
    envmap: Option<(i32, DrawMode)>,
  ) -> usize {
    if tex < 0 {
      return 0;
    }
    let t = tex as usize;
    let name = self.level.textures[t].name.clone();
    let base = self.image(t, false);
    let base = self.gltf_texture(base, &name, mode);
    let (alpha_mode, extras) = material_alpha(mode, envmap.is_some());
    let mut mat = json!({
      "name": name,
      "doubleSided": true,
      // 2: the PS2's blending, where 0.5 behaves like 1.0
      "pbrMetallicRoughness": {
        "baseColorFactor": [2.0, 2.0, 2.0, 2.0],
        "baseColorTexture": {"index": base},
        "metallicFactor": 0.0,
      },
      "alphaCutoff": ALPHA_CUTOFF,
    });
    if let Some(e) = extras {
      mat["extras"] = e;
    }
    if let Some((env_tex, env_mode)) = envmap {
      mat["name"] = json!(format!("{name}-envmap"));
      let strength = self.image(t, true);
      let strength = self.gltf_texture(strength, &format!("{name}-envmap-strength"), mode);
      // metallic and rough: glTF's defaults (1)
      let pbr = &mut mat["pbrMetallicRoughness"];
      pbr.as_object_mut().unwrap().remove("metallicFactor");
      pbr["metallicRoughnessTexture"] = json!({ "index": strength });
      let env_name = self.level.textures[env_tex as usize].name.clone();
      let env = self.image(env_tex as usize, false);
      let env = self.gltf_texture(env, &env_name, env_mode);
      mat["extensions"] =
        json!({"KHR_materials_specular": {"specularColorTexture": {"index": env}}});
      self.specular = true;
    }
    if alpha_mode != "OPAQUE" {
      mat["alphaMode"] = json!(alpha_mode);
    }
    self.materials.push(mat);
    self.materials.len() - 1
  }

  /// A tfrag tree under "<level>-tfrag-<kind>", in pieces: the triangles their vertices connect
  /// (vertices at the same place are one). A piece has a primitive per draw (texture),
  /// `og_terrain` (the pieces TERRAIN_EXTENT across or more whose surface mostly faces up or down)
  /// and `og_measure` (its surface and the length of its edges, the same however it is turned: the
  /// editor's catalog shows the copies of a piece once).
  fn tfrag(&mut self, tree: &TfragTree) {
    let u = tree.unpack();
    let root = self.root(format!(
      "{}-tfrag-{}",
      self.level.name,
      TFRAG_TREE_NAMES[(tree.kind as usize).min(7)]
    ));
    let positions: Vec<[f32; 3]> = u.vertices.iter().map(|v| v.pos).collect();
    let pos: Vec<[f32; 3]> = positions.iter().map(|p| p.map(|x| x / 4096.0)).collect();
    let pos_acc = self.floats(&pos, true);
    let st: Vec<[f32; 2]> = u.vertices.iter().map(|v| v.st).collect();
    let st_acc = self.floats(&st, false);
    let colors: Vec<[f32; 4]> = u
      .vertices
      .iter()
      .map(|v| lit_color(&tree.colors, v.color_index))
      .collect();
    let col_acc = self.floats(&colors, false);
    let palette = self.tod_palette(&tod_bytes(&tree.colors));
    let tod: Vec<u16> = u.vertices.iter().map(|v| v.color_index).collect();
    let tod_index = self.tod_index(&tod);

    let (unstripped, old_to_new) = unstrip_tfrag_tie(&u.indices, &positions);

    // the pieces: union-find over the vertices, the ones at the same place (to 4 mm) first
    let n = positions.len();
    let mut up: Vec<usize> = (0..n).collect();
    fn root_of(up: &mut [usize], mut x: usize) -> usize {
      while up[x] != x {
        up[x] = up[up[x]];
        x = up[x];
      }
      x
    }
    let unite = |up: &mut Vec<usize>, a: usize, b: usize| {
      let (a, b) = (root_of(up, a), root_of(up, b));
      if a != b {
        up[a.max(b)] = a.min(b);
      }
    };
    let q = |f: f32| ((f / 16.0).round() as i32 as u32 as u64) & 0x1f_ffff;
    let mut first_at: HashMap<u64, usize> = HashMap::with_capacity(n);
    for (i, p) in positions.iter().enumerate() {
      let key = q(p[0]) | (q(p[1]) << 21) | (q(p[2]) << 42);
      let first = *first_at.entry(key).or_insert(i);
      if first != i {
        unite(&mut up, i, first);
      }
    }
    let ranges: Vec<(usize, usize)> = tree
      .draws
      .iter()
      .enumerate()
      .map(|(d, draw)| {
        if draw.num_triangles == 0 {
          (0, 0)
        } else {
          (
            old_to_new[u.draw_first[d] as usize] as usize,
            draw.num_triangles as usize * 3,
          )
        }
      })
      .collect();
    for &(start, count) in &ranges {
      for t in unstripped[start..start + count].chunks_exact(3) {
        unite(&mut up, t[0] as usize, t[1] as usize);
        unite(&mut up, t[0] as usize, t[2] as usize);
      }
    }
    // piece (its root vertex) -> draw -> indices
    let mut pieces: BTreeMap<usize, BTreeMap<usize, Vec<u32>>> = BTreeMap::new();
    for (d, &(start, count)) in ranges.iter().enumerate() {
      for t in unstripped[start..start + count].chunks_exact(3) {
        let r = root_of(&mut up, t[0] as usize);
        pieces
          .entry(r)
          .or_default()
          .entry(d)
          .or_default()
          .extend_from_slice(t);
      }
    }
    // one index buffer, the pieces one after the other
    let mut grouped = Vec::with_capacity(unstripped.len());
    let mut piece_ranges = HashMap::new();
    for (&piece, by_draw) in &pieces {
      for (&d, indices) in by_draw {
        piece_ranges.insert((piece, d), (grouped.len() as u32, indices.len() as u32));
        grouped.extend_from_slice(indices);
      }
    }
    let view = self.index_view(&grouped);

    let mut draw_material: Vec<Option<usize>> = vec![None; tree.draws.len()];
    for (number, (&piece, by_draw)) in pieces.iter().enumerate() {
      let (mut lo, mut hi) = ([1e30f32; 3], [-1e30f32; 3]);
      let (mut flat_area, mut area) = (0.0f64, 0.0f64);
      let (mut main_draw, mut main_count) = (*by_draw.keys().next().unwrap(), 0);
      let (mut surface, mut edges) = (0.0f64, 0.0f64);
      for (&d, indices) in by_draw {
        for &v in indices {
          for k in 0..3 {
            lo[k] = lo[k].min(positions[v as usize][k] / 4096.0);
            hi[k] = hi[k].max(positions[v as usize][k] / 4096.0);
          }
        }
        for t in indices.chunks_exact(3) {
          let [a, b, c] = [0, 1, 2].map(|k| positions[t[k] as usize]);
          let e1 = [0, 1, 2].map(|k| (b[k] - a[k]) as f64);
          let e2 = [0, 1, 2].map(|k| (c[k] - a[k]) as f64);
          let (nx, ny, nz) = (
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
          );
          let len = (nx * nx + ny * ny + nz * nz).sqrt();
          area += len;
          if ny.abs() > 0.7 * len {
            flat_area += len;
          }
          // the same in meters
          let qm = [a, b, c].map(|p| p.map(|x| x / 4096.0));
          let length = |u: [f32; 3], v: [f32; 3]| {
            (0..3)
              .map(|k| {
                let d = (u[k] - v[k]) as f64;
                d * d
              })
              .sum::<f64>()
              .sqrt()
          };
          let e1 = [0, 1, 2].map(|k| (qm[1][k] - qm[0][k]) as f64);
          let e2 = [0, 1, 2].map(|k| (qm[2][k] - qm[0][k]) as f64);
          let (cx, cy, cz) = (
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
          );
          surface += 0.5 * (cx * cx + cy * cy + cz * cz).sqrt();
          edges += length(qm[0], qm[1]) + length(qm[1], qm[2]) + length(qm[2], qm[0]);
        }
        if indices.len() > main_count {
          main_count = indices.len();
          main_draw = d;
        }
      }
      // the ground: large, and mostly flat (a facade, a wall or a door as large stays selectable)
      let terrain = (hi[0] - lo[0] >= TERRAIN_EXTENT || hi[2] - lo[2] >= TERRAIN_EXTENT)
        && area > 0.0
        && flat_area >= 0.5 * area;
      let name = format!("{}-{number}", self.texture_name(tree.draws[main_draw].tex));
      let mut prims = Vec::new();
      for &d in by_draw.keys() {
        let draw = &tree.draws[d];
        let mat = match draw_material[d] {
          Some(m) => m,
          None => {
            let m = self.material(draw.tex, draw.mode, None);
            draw_material[d] = Some(m);
            m
          }
        };
        let (start, count) = piece_ranges[&(piece, d)];
        let idx = self.index_range(view, start, count);
        prims.push(json!({
          "attributes": {"POSITION": pos_acc, "TEXCOORD_0": st_acc, "COLOR_0": col_acc},
          "indices": idx, "material": mat, "mode": 4,
        }));
      }
      let nprims = prims.len();
      let mesh = self.mesh(&name, prims);
      let round2 = |x: f64| (x * 100.0).round() / 100.0;
      let node = self.child(
        root,
        json!({"name": name, "mesh": mesh, "extras": {
          "og_terrain": terrain, "og_measure": [round2(surface), round2(edges)],
        }}),
      );
      self.set_tod(node, palette as i64, tod_index, nprims);
    }
  }

  /// A prototype: a group node named after it under `parent`, a mesh per distinct geometry (in
  /// the prototype's space), a node per instance with its matrix and its own lighting
  #[allow(clippy::too_many_arguments)]
  fn emit_proto(
    &mut self,
    parent: usize,
    name: &str,
    instances: Vec<ProtoInstance>,
    matrices: &[Matrix],
    packed_vertex: impl Fn(u32) -> ([f32; 3], [f32; 2]),
    unpacked_color: impl Fn(u32) -> [f32; 4],
    palette: i64,
    mut unpacked_tod: impl FnMut(u32) -> u16,
  ) -> Vec<usize> {
    let group = self.child(parent, json!({ "name": name }));
    // geometry (triangles per material) -> its mesh and the packed vertices of its arrays
    let mut mesh_of: HashMap<&Geometry, (usize, Vec<u32>)> = HashMap::new();
    let mut nodes = Vec::new();
    for (count, inst) in instances.iter().enumerate() {
      if !mesh_of.contains_key(&inst.tris) {
        // the vertices this geometry uses, renumbered in their order
        let mut local: BTreeMap<u32, u32> = BTreeMap::new();
        for tris in inst.tris.values() {
          for &v in tris {
            local.insert(v, 0);
          }
        }
        for (i, idx) in local.values_mut().enumerate() {
          *idx = i as u32;
        }
        let (mut pos, mut st, mut colors) = (Vec::new(), Vec::new(), Vec::new());
        for &v in local.keys() {
          let (p, t) = packed_vertex(v);
          pos.push(p);
          st.push(t);
          colors.push(
            inst
              .unpacked_of
              .get(&v)
              .map_or([1.0; 4], |&u| unpacked_color(u)),
          );
        }
        let pos_acc = self.floats(&pos, true);
        let st_acc = self.floats(&st, false);
        let col_acc = self.floats(&colors, false);
        let mut prims = Vec::new();
        for (&mat, tris) in &inst.tris {
          let indices: Vec<u32> = tris.iter().map(|v| local[v]).collect();
          let idx = self.indices(&indices);
          prims.push(json!({
            "attributes": {"POSITION": pos_acc, "TEXCOORD_0": st_acc, "COLOR_0": col_acc},
            "indices": idx, "material": mat, "mode": 4,
          }));
        }
        let mesh = self.mesh(name, prims);
        mesh_of.insert(&inst.tris, (mesh, local.into_keys().collect()));
      }
      let (mesh, order) = &mesh_of[&inst.tris];
      // the instance's own lighting: the instances share their prototype's mesh, not their colors
      let tod: Vec<u16> = order
        .iter()
        .map(|v| inst.unpacked_of.get(v).map_or(0, |&u| unpacked_tod(u)))
        .collect();
      let tod_index = self.tod_index(&tod);
      let mut node = json!({"name": format!("{name}-{count}"), "mesh": mesh});
      if inst.matrix_idx >= 0 && (inst.matrix_idx as usize) < matrices.len() {
        node["matrix"] = json!(instance_matrix(&matrices[inst.matrix_idx as usize]));
      }
      let node = self.child(group, node);
      self.set_tod(node, palette, tod_index, inst.tris.len());
      nodes.push(node);
    }
    nodes
  }

  /// A TIE tree with a node per instance: under "<level>-tie-<n>", a group per prototype holding a
  /// node per instance, and the swaying instances (wind) under theirs.
  fn tie(&mut self, tree: &TieTree, tree_idx: usize) {
    let u = tree.unpack();
    let root = self.root(format!("{}-tie-{tree_idx}", self.level.name));
    let palette = self.tod_palette(&tod_bytes(&tree.colors)) as i64;

    // unpacked vertex -> the matrix group it was unpacked by
    let groups = &tree.matrix_groups;
    let mut group_of = vec![0u32; u.vertices.len()];
    let mut group_first = vec![0u32; groups.len()];
    let mut i = 0usize;
    for (g, grp) in groups.iter().enumerate() {
      group_first[g] = i as u32;
      for _ in grp.start..grp.end {
        if i < group_of.len() {
          group_of[i] = g as u32;
          i += 1;
        }
      }
    }

    // the materials of the static draws (an envmapped draw's shine is its second draw)
    let mut envmap_at: HashMap<u32, (i32, DrawMode)> = HashMap::new();
    for cat in (0..9).filter(|&c| is_envmap_second_draw(c)) {
      let (a, b) = (
        tree.category_draw_indices[cat],
        tree.category_draw_indices[cat + 1],
      );
      for draw in &tree.static_draws[a as usize..b as usize] {
        for &(v0, _) in &draw.runs {
          envmap_at.insert(v0, (draw.tex, draw.mode));
        }
      }
    }
    let mut draws = Vec::new();
    let mut draw_material = HashMap::new();
    for cat in (0..9).filter(|&c| !is_envmap_second_draw(c)) {
      let (a, b) = (
        tree.category_draw_indices[cat],
        tree.category_draw_indices[cat + 1],
      );
      for d in a as usize..b as usize {
        let draw = &tree.static_draws[d];
        let envmap = draw
          .runs
          .first()
          .filter(|_| is_envmap_first_draw(cat))
          .and_then(|&(v0, len)| envmap_at.get(&(v0 + len as u32)))
          .filter(|(t, _)| *t >= 0 && (*t as usize) < self.level.textures.len())
          .copied();
        draws.push(d);
        let m = self.material(draw.tex, draw.mode, envmap);
        draw_material.insert(d, m);
      }
    }

    // triangles, per prototype and instance (the instance is the matrix of its vertices)
    let positions: Vec<[f32; 3]> = u.vertices.iter().map(|v| v.pos).collect();
    let (unstripped, old_to_new) = unstrip_tfrag_tie(&u.indices, &positions);
    let mut protos: BTreeMap<u16, BTreeMap<i64, ProtoInstance>> = BTreeMap::new();
    let packed_of = |unpacked: u32| {
      let g = group_of[unpacked as usize] as usize;
      groups[g].start + (unpacked - group_first[g])
    };
    for &d in &draws {
      let draw = &tree.static_draws[d];
      let mat = draw_material[&d];
      let mut old_start = u.draw_first[d] as usize;
      for grp in &draw.vis_groups {
        if grp.num_tris > 0 {
          let first = old_to_new[old_start] as usize;
          for t in 0..grp.num_tris as usize {
            let tri = &unstripped[first + t * 3..first + t * 3 + 3];
            let g = group_of[tri[0] as usize] as usize;
            let m = groups[g].matrix_idx;
            // world space groups are one instance each, the others one per matrix
            let key = if m >= 0 { m as i64 } else { -1 - g as i64 };
            let inst = protos
              .entry(grp.proto_idx)
              .or_default()
              .entry(key)
              .or_default();
            inst.matrix_idx = m;
            for &v in tri {
              let packed = inst.add(packed_of(v), v);
              inst.tris.entry(mat).or_default().push(packed);
            }
          }
        }
        old_start += grp.num_inds as usize;
      }
    }
    for (proto, by_instance) in protos {
      self.emit_proto(
        root,
        &proto_name(&tree.proto_names, proto, "tie"),
        by_instance.into_values().collect(),
        &tree.matrices,
        |p| {
          let v = &tree.vertices[p as usize];
          (v.pos.map(|x| x / 4096.0), v.st)
        },
        |up| lit_color(&tree.colors, u.vertices[up as usize].color_index),
        palette,
        |up| u.vertices[up as usize].color_index,
      );
    }

    // swaying instances: a node each, its primitives on the tree's vertex arrays, under its
    // prototype's node as the other instances are
    if tree.wind_draws.is_empty() {
      return;
    }
    let wind_protos = wind_protos(tree, &u, &packed_of);
    let pos: Vec<[f32; 3]> = positions.iter().map(|p| p.map(|x| x / 4096.0)).collect();
    let pos_acc = self.floats(&pos, true);
    let st: Vec<[f32; 2]> = u.vertices.iter().map(|v| v.st).collect();
    let st_acc = self.floats(&st, false);
    let colors: Vec<[f32; 4]> = u
      .vertices
      .iter()
      .map(|v| lit_color(&tree.colors, v.color_index))
      .collect();
    let col_acc = self.floats(&colors, false);
    let mut wind_indices = Vec::new();
    let mut ranges = Vec::new();
    for draw in &tree.wind_draws {
      let mut offset = 0usize;
      let mut r = Vec::new();
      for &(num, _) in &draw.instance_groups {
        let start = wind_indices.len();
        unstrip_plain(&draw.index_stream, offset, num as usize, &mut wind_indices);
        r.push((start as u32, (wind_indices.len() - start) as u32));
        offset += num as usize;
      }
      ranges.push(r);
    }
    let view = self.index_view(&wind_indices);
    let mut mesh_of_instance: BTreeMap<u32, usize> = BTreeMap::new();
    let mut proto_node: HashMap<u16, usize> = HashMap::new();
    let mut wind_nodes = Vec::new();
    for (d, draw) in tree.wind_draws.iter().enumerate() {
      let mat = self.material(draw.tex, draw.mode, None);
      for (g, &(_, inst)) in draw.instance_groups.iter().enumerate() {
        let mesh = match mesh_of_instance.get(&inst) {
          Some(&m) => m,
          None => {
            let m = self.mesh(&format!("wind-{inst}"), vec![]);
            mesh_of_instance.insert(inst, m);
            let proto = wind_protos.get(inst as usize).copied().unwrap_or(0);
            let name = proto_name(&tree.proto_names, proto, "tie");
            let parent = match proto_node.get(&proto) {
              Some(&p) => p,
              None => {
                let p = self.child(root, json!({ "name": name }));
                proto_node.insert(proto, p);
                p
              }
            };
            let mut node = json!({"name": format!("{name}-{inst}"), "mesh": m});
            if let Some(w) = tree.wind_instances.get(inst as usize) {
              node["matrix"] = json!(instance_matrix(&w.matrix));
            }
            let n = self.child(parent, node);
            wind_nodes.push(n);
            m
          }
        };
        let (start, count) = ranges[d][g];
        let idx = self.index_range(view, start, count);
        self.meshes[mesh]["primitives"]
          .as_array_mut()
          .unwrap()
          .push(json!({
            "attributes": {"POSITION": pos_acc, "TEXCOORD_0": st_acc, "COLOR_0": col_acc},
            "indices": idx, "material": mat, "mode": 4,
          }));
      }
    }
    let tod: Vec<u16> = u.vertices.iter().map(|v| v.color_index).collect();
    let tod_index = self.tod_index(&tod);
    for n in wind_nodes {
      let mesh = self.nodes[n]["mesh"].as_u64().unwrap() as usize;
      let prims = self.meshes[mesh]["primitives"].as_array().unwrap().len();
      self.set_tod(n, palette, tod_index, prims);
    }
  }

  /// A shrub tree with a node per instance: under "<level>-shrub-<n>", a group per prototype
  /// holding a node per instance. Their lighting as tfrag's (the builder makes them tfrag): the
  /// shrub renderer multiplies a color of the tree's palettes by the vertex's own, by 4 where tfrag
  /// doubles it; a palette of the pairs met.
  fn shrub(&mut self, tree: &ShrubTree, tree_idx: usize) {
    let u = tree.unpack();
    let root = self.root(format!("{}-shrub-{tree_idx}", self.level.name));
    let groups = &tree.groups;
    let mut group_of = vec![0u32; u.len()];
    let mut group_first = vec![0u32; groups.len()];
    let mut i = 0usize;
    for (g, grp) in groups.iter().enumerate() {
      group_first[g] = i as u32;
      for _ in grp.start..grp.end {
        if i < group_of.len() {
          group_of[i] = g as u32;
          i += 1;
        }
      }
    }
    let mut protos: BTreeMap<u16, BTreeMap<i64, ProtoInstance>> = BTreeMap::new();
    for draw in &tree.draws {
      let mat = self.material(draw.tex as i32, draw.mode, None);
      let mut tris = Vec::new();
      unstrip_plain(
        &tree.indices,
        draw.first_index as usize,
        draw.num_indices as usize,
        &mut tris,
      );
      for t in tris.chunks_exact(3) {
        let g = group_of[t[0] as usize] as usize;
        let inst = protos
          .entry(draw.proto_idx)
          .or_default()
          .entry(groups[g].matrix_idx as i64)
          .or_default();
        inst.matrix_idx = groups[g].matrix_idx;
        for &v in t {
          let gk = group_of[v as usize] as usize;
          let packed = inst.add(groups[gk].start + (v - group_first[gk]), v);
          inst.tris.entry(mat).or_default().push(packed);
        }
      }
    }
    let mut pairs: HashMap<u64, u16> = HashMap::new();
    let mut tod: Vec<u8> = Vec::new();
    let mut users = Vec::new();
    for (proto, by_instance) in protos {
      let nodes = self.emit_proto(
        root,
        &proto_name(&tree.proto_names, proto, "shrub"),
        by_instance.into_values().collect(),
        &tree.matrices,
        |p| {
          let v = &tree.vertices[p as usize];
          (v.pos.map(|x| x / 4096.0), v.st.map(|x| x / 4096.0))
        },
        |up| lit_color(&tree.colors, u[up as usize].color_index),
        -1, // set once every color is met
        |up| {
          let v = &u[up as usize];
          let key = v.color_index as u64
            | (v.rgba[0] as u64) << 16
            | (v.rgba[1] as u64) << 24
            | (v.rgba[2] as u64) << 32;
          let next = pairs.len() as u16;
          *pairs.entry(key).or_insert_with(|| {
            for k in 0..8 {
              for ch in 0..4 {
                let c = tree.colors.read(v.color_index as usize, k, ch) as u32;
                tod.push(if ch == 3 {
                  c as u8
                } else {
                  (c * v.rgba[ch] as u32 * 2 / 255).min(255) as u8
                });
              }
            }
            next
          })
        },
      );
      users.extend(nodes);
    }
    let palette = self.tod_palette(&tod);
    for n in users {
      self.nodes[n]["extras"]["og_tod"]["palette"] = json!(palette);
    }
  }

  /// The collision: a node per surface value (pat), with the level builder's collision extras
  fn collision(&mut self, game: &str) {
    let verts = &self.level.collision;
    if verts.len() < 3 {
      return;
    }
    let mut by_pat: BTreeMap<u32, Vec<[f32; 6]>> = BTreeMap::new();
    for t in verts.chunks_exact(3) {
      let list = by_pat.entry(t[0].pat).or_default();
      for v in t {
        list.push([
          v.pos[0],
          v.pos[1],
          v.pos[2],
          v.normal[0] as f32,
          v.normal[1] as f32,
          v.normal[2] as f32,
        ]);
      }
    }
    let root = self.root(format!("{}-collision", self.level.name));
    for (pat, tri_verts) in by_pat {
      let name = format!("{}-collision-{pat:08x}", self.level.name);
      let pos: Vec<[f32; 3]> = tri_verts
        .iter()
        .map(|v| [v[0] / 4096.0, v[1] / 4096.0, v[2] / 4096.0])
        .collect();
      let pos_acc = self.floats(&pos, true);
      let normals: Vec<[f32; 3]> = tri_verts
        .iter()
        .map(|v| [v[3] / 32767.0, v[4] / 32767.0, v[5] / 32767.0])
        .collect();
      let n_acc = self.floats(&normals, false);
      let seq: Vec<u32> = (0..tri_verts.len() as u32).collect();
      let idx = self.indices(&seq);
      let mesh = self.mesh(
        &name,
        vec![
          json!({"attributes": {"POSITION": pos_acc, "NORMAL": n_acc}, "indices": idx, "mode": 4}),
        ],
      );
      self.child(
        root,
        json!({"name": name, "mesh": mesh, "extras": {
          "set_collision": 1, "set_invisible": 1, "collide_pat": pat as i32, "collide_game": game,
        }}),
      );
    }
  }

  /// The actors: an empty node each, at its place and rotation, with "kind": "actor" and the rest
  /// of its data (bsp.rs) in the extras
  fn actors(&mut self, actors: &[Value]) {
    if actors.is_empty() {
      return;
    }
    let root = self.root(format!("{}-actors", self.level.name));
    for (i, actor) in actors.iter().enumerate() {
      let mut extras = actor.as_object().cloned().unwrap_or_default();
      let trans = extras.remove("trans");
      let quat = extras.remove("quat");
      extras.insert("kind".into(), json!("actor"));
      let name = match actor["lump"]["name"].as_str() {
        Some(n) => n.to_string(),
        None => format!("{}-{i}", actor["etype"].as_str().unwrap_or("actor")),
      };
      // an actor named "": a node without name
      let mut node = if name.is_empty() {
        json!({})
      } else {
        json!({ "name": name })
      };
      if let Some(t) = trans
        .as_ref()
        .and_then(Value::as_array)
        .filter(|t| t.len() >= 3)
      {
        node["translation"] = Value::Array(t[..3].to_vec());
      }
      if let Some(q) = quat
        .as_ref()
        .and_then(Value::as_array)
        .filter(|q| q.len() == 4)
      {
        node["rotation"] = json!(q);
      }
      node["extras"] = Value::Object(extras);
      self.child(root, node);
    }
  }
}

/// The prototype of each swaying instance. The .fr3 does not say it: a prototype's packed vertices
/// follow the previous prototype's, and all its instances sway or none; an instance's vertices
/// are its prototype's (`packed_of` an unpacked vertex), the static instances' draws name theirs.
fn wind_protos(
  tree: &TieTree,
  u: &super::fr3::UnpackedTree,
  packed_of: &dyn Fn(u32) -> u32,
) -> Vec<u16> {
  // the packed vertices used (a group of them can run over the next prototype's: the extractor
  // merges the groups that follow each other)
  let range_of = |unpacked: u32, r: &mut (u32, u32)| {
    let p = packed_of(unpacked);
    r.0 = r.0.min(p);
    r.1 = r.1.max(p + 1);
  };
  // the packed vertices of the prototypes with static instances
  let mut known: BTreeMap<u16, (u32, u32)> = BTreeMap::new();
  for (d, draw) in tree.static_draws.iter().enumerate() {
    let mut at = u.draw_first[d] as usize;
    for grp in &draw.vis_groups {
      let r = known.entry(grp.proto_idx).or_insert((u32::MAX, 0));
      for &i in &u.indices[at..at + grp.num_inds as usize] {
        if i != u32::MAX {
          range_of(i, r);
        }
      }
      at += grp.num_inds as usize;
    }
  }
  // each swaying instance's
  let mut wind = vec![(u32::MAX, 0u32); tree.wind_instances.len()];
  for draw in &tree.wind_draws {
    let mut at = 0usize;
    for &(num, inst) in &draw.instance_groups {
      if let Some(r) = wind.get_mut(inst as usize) {
        for &i in &draw.index_stream[at..at + num as usize] {
          if i != u32::MAX {
            range_of(i, r);
          }
        }
      }
      at += num as usize;
    }
  }
  assign_wind_protos(
    &known,
    &wind,
    tree.proto_names.len(),
    tree.vertices.len() as u32,
  )
}

/// The prototypes of the swaying instances from their packed vertices (`wind`), between the
/// prototypes whose static instances name them (`known`): the prototypes between two known ones
/// own the vertices between theirs, in order. A swaying prototype following a known one (or
/// preceding it) without a gap is exact; in a gap holding prototypes without any instance (their
/// vertices are never used: unknown), the swaying ones take the first places.
fn assign_wind_protos(
  known: &BTreeMap<u16, (u32, u32)>,
  wind: &[(u32, u32)],
  protos: usize,
  vertices: u32,
) -> Vec<u16> {
  let mut ranges: Vec<(u32, u32)> = wind.iter().copied().filter(|r| r.0 <= r.1).collect();
  ranges.sort();
  ranges.dedup();
  // (index, start, end) of the known prototypes, with the ends of the list
  let mut anchors = vec![(-1i64, 0u32, 0u32)];
  anchors.extend(
    known
      .iter()
      .filter(|(_, r)| r.0 <= r.1)
      .map(|(&p, &(s, e))| (p as i64, s, e)),
  );
  anchors.push((protos.max(1) as i64, vertices, vertices));
  let mut proto_of: HashMap<(u32, u32), u16> = HashMap::new();
  for pair in anchors.windows(2) {
    let ((a, _, a_end), (b, b_start, _)) = (pair[0], pair[1]);
    let gap: Vec<(u32, u32)> = ranges
      .iter()
      .copied()
      .filter(|r| r.0 >= a_end && r.1 <= b_start)
      .collect();
    // from the known one before, then from the one after, while they follow without a gap
    let (mut i, mut lo, mut at) = (0, a + 1, a_end);
    while i < gap.len() && gap[i].0 == at {
      proto_of.insert(gap[i], lo as u16);
      (lo, at, i) = (lo + 1, gap[i].1, i + 1);
    }
    let (mut j, mut hi, mut at) = (gap.len(), b - 1, b_start);
    while j > i && gap[j - 1].1 == at {
      proto_of.insert(gap[j - 1], hi as u16);
      (hi, at, j) = (hi - 1, gap[j - 1].0, j - 1);
    }
    for (k, r) in gap[i..j].iter().enumerate() {
      proto_of.insert(*r, (lo + k as i64) as u16);
    }
  }
  wind
    .iter()
    .map(|r| proto_of.get(r).copied().unwrap_or(0))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn swaying_instances_given_their_prototypes() {
    // prototypes 0 (static, vertices 0..10), 1 (no instance, 10..14), 2 and 3 (swaying, 14..20
    // and 20..26), 4 (static, 26..30): 3 is found after 4's start, 2 after 3
    let known = BTreeMap::from([(0, (0, 10)), (4, (26, 30))]);
    let wind = [(14, 20), (20, 26), (20, 26)];
    assert_eq!(assign_wind_protos(&known, &wind, 5, 30), [2, 3, 3]);
    // following a known one: exact
    let known = BTreeMap::from([(0, (0, 10))]);
    assert_eq!(assign_wind_protos(&known, &[(10, 12)], 3, 20), [1]);
  }
}

/// Writes the decor of `level` (its .fr3) with its `actors` (bsp.rs) to `path`
pub fn write_decor(level: &Level, game: &str, actors: &[Value], path: &Path) -> Result<(), String> {
  let mut w = Writer::new(level);
  for tree in &level.tfrag_trees[0] {
    w.tfrag(tree);
  }
  for (i, tree) in level.tie_trees[0].iter().enumerate() {
    w.tie(tree, i);
  }
  for (i, tree) in level.shrub_trees.iter().enumerate() {
    w.shrub(tree, i);
  }
  w.collision(game);
  w.actors(actors);
  let mut doc = json!({
    "asset": {"version": "2.0", "generator": "opengoal",
              "extras": {"opengoal": {"game": game, "level": level.name}}},
    "scene": 0,
    "scenes": [{"nodes": w.roots}],
    "nodes": w.nodes,
    "meshes": w.meshes,
    "materials": w.materials,
    "images": w.images,
    "textures": w.textures,
    "samplers": w.samplers,
  });
  if w.specular {
    doc["extensionsUsed"] = json!(["KHR_materials_specular"]);
  }
  if let Some(dir) = path.parent() {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  }
  w.w.write(doc, path)
}
