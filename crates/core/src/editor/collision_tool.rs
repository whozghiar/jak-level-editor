//! The collision tool (C), as OpenRAC's collision dock for Ratchet & Clank: the collision of the
//! document drawn over the view, a color per surface (its material, its effect on Jak or its kind:
//! ground, wall, obstacle), and its faces picked (a click, Shift to add or take out, Alt the faces
//! of the same surface touching it, a box, a surface of the legend, the faces under the pieces
//! selected) and changed: given another surface, deleted, moved, or more added (a floor or a box
//! where the last click landed, the shape of the pieces selected). Every change is one undo step.
//!
//! The collision is the document's Collision nodes (a level's, as extracted, one per surface; those
//! the tool adds, marked `og_collision: "made"`): a surface per triangle (Primitive::tri_pat, the
//! game's pat-surface, engine/collide/pat-h.gc). The export writes them by surface
//! (game_export/write.rs), the levels' only with the game's collision, the tool's always.
//!
//! The viewport (ui/render/collision_tool.svelte.ts) asks `collision_overlay` for the triangles to
//! draw (x, y, z and a color each, in the world) when its key changes, and sends its clicks and
//! boxes with its camera's view-projection.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use serde_json::{Value, json};

use super::{Editor, ToolMode, fnv};
use crate::i18n::tr;
use crate::io::decor_behaviors::{
  COLLISION_EVENTS, COLLISION_MATERIALS, builder_event, builder_material, builder_mode,
  decor_behavior_json, decor_behavior_of,
};
use crate::io::game_export::{TriGrid, builder_surface, collision_hot_spots};
use crate::math::{Mat4, Ray, Transform, Vec3, Vec4, cross, dot, length, normalize, ray_plane};
use crate::scene::pick::{PickFilter, pick};
use crate::scene::{Mesh, Node, NodeKind, Primitive};

/// A face: its node, primitive and triangle
type Face = (u32, u32, u32);

/// The tool's faces picked, where the last click landed, how the collision is colored
#[derive(Default)]
pub struct CollisionToolState {
  /// material, effect, kind
  pub color_by: String,
  picked: BTreeSet<Face>,
  /// where a floor or a box goes
  last_point: Option<Vec3>,
  /// the places where Jak may fall through (collision_check), for the collision as it was then
  hot: Vec<(usize, Vec3)>,
  hot_key: u64,
}

const K_KINDS: [&str; 3] = ["ground", "wall", "obstacle"];

/// The name of a surface in the legend: its material, its effect on Jak or its kind, as the level
/// builder writes it (builder_surface: Jak 1's fields)
fn surface_key(pat: u32, by: &str) -> &'static str {
  let (s, _) = builder_surface(pat);
  match by {
    "effect" => COLLISION_EVENTS
      .get(s.event as usize)
      .copied()
      .unwrap_or("none"),
    "kind" => K_KINDS.get(s.mode as usize).copied().unwrap_or("ground"),
    _ => COLLISION_MATERIALS
      .get(s.material as usize)
      .copied()
      .unwrap_or("stone"),
  }
}

/// Its color in the view (OpenRAC's: one hue per surface)
fn key_color(key: &str) -> [f32; 3] {
  match key {
    "stone" | "none" => [0.62, 0.62, 0.64],
    "ice" => [0.55, 0.9, 1.0],
    "quicksand" => [0.85, 0.68, 0.35],
    "waterbottom" => [0.2, 0.45, 0.95],
    "tar" => [0.22, 0.2, 0.25],
    "sand" => [0.95, 0.85, 0.5],
    "wood" => [0.65, 0.42, 0.2],
    "grass" => [0.3, 0.78, 0.25],
    "pcmetal" => [0.35, 0.85, 0.75],
    "snow" => [0.97, 0.97, 1.0],
    "deepsnow" => [0.78, 0.84, 0.97],
    "hotcoals" => [0.9, 0.35, 0.1],
    "lava" => [1.0, 0.22, 0.0],
    "crwood" => [0.8, 0.58, 0.3],
    "gravel" => [0.55, 0.5, 0.42],
    "dirt" => [0.5, 0.34, 0.2],
    "metal" => [0.45, 0.56, 0.8],
    "straw" => [0.92, 0.82, 0.3],
    "tube" => [0.7, 0.3, 0.85],
    "swamp" => [0.35, 0.47, 0.18],
    "stopproj" => [0.92, 0.2, 0.6],
    "rotate" => [0.3, 0.3, 0.95],
    "neutral" => [0.85, 0.85, 0.8],
    "deadly" => [0.95, 0.1, 0.1],
    "endlessfall" => [0.5, 0.1, 0.85],
    "burn" => [1.0, 0.5, 0.0],
    "deadlyup" => [0.7, 0.0, 0.25],
    "burnup" => [1.0, 0.72, 0.1],
    "melt" => [1.0, 0.95, 0.2],
    "ground" => [0.3, 0.8, 0.35],
    "wall" => [0.25, 0.5, 0.95],
    "obstacle" => [0.95, 0.75, 0.2],
    _ => [0.8, 0.8, 0.8],
  }
}

/// A surface given to faces: what the inspector's In the game offers (Jak 1's names, the level
/// builder's), each field kept when not given
#[derive(Clone, Debug, Default)]
pub struct SurfaceChange {
  pub material: Option<String>,
  pub event: Option<String>,
  pub mode: Option<String>,
  pub flags: Vec<(u32, bool)>,
}

impl SurfaceChange {
  fn from_json(a: &Value) -> Self {
    let s = |k: &str| a.get(k).and_then(Value::as_str).map(str::to_string);
    let mut c = Self {
      material: s("material").filter(|m| builder_material(m) >= 0),
      event: s("event").filter(|e| builder_event(e) >= 0),
      mode: s("mode").filter(|m| builder_mode(m) >= 0),
      flags: Vec::new(),
    };
    // pat-h.gc's bits
    for (key, bit) in [
      ("noentity", 0),
      ("nocamera", 1),
      ("noedge", 2),
      ("nolineofsight", 16),
    ] {
      if let Some(on) = a.get(key).and_then(Value::as_bool) {
        c.flags.push((bit, on));
      }
    }
    c
  }

  /// `pat` with these fields: Jak 1's stone (0) is Jak 3's 23, the other materials and the
  /// effects the same numbers (builder_surface reads them back)
  pub fn apply(&self, mut pat: u32) -> u32 {
    if let Some(m) = &self.material {
      let m = match builder_material(m) {
        0 => 23,
        k => k as u32,
      };
      pat = (pat & !(63 << 10)) | (m << 10);
    }
    if let Some(e) = &self.event {
      pat = (pat & !(63 << 18)) | ((builder_event(e) as u32) << 18);
    }
    if let Some(m) = &self.mode {
      pat = (pat & !(7 << 7)) | ((builder_mode(m) as u32) << 7);
    }
    for &(bit, on) in &self.flags {
      pat = if on {
        pat | (1 << bit)
      } else {
        pat & !(1 << bit)
      };
    }
    pat
  }
}

/// A floor (`size` x and z) or a box around the origin's ground, facing out
fn floor_or_box(boxed: bool, size: Vec3) -> Primitive {
  let (hx, hz, h) = (size.x * 0.5, size.z * 0.5, size.y);
  let mut p = Primitive::default();
  if boxed {
    for i in 0..8 {
      p.pos.push(Vec3::new(
        if i & 1 != 0 { hx } else { -hx },
        if i & 2 != 0 { h } else { 0.0 },
        if i & 4 != 0 { hz } else { -hz },
      ));
    }
    // counter-clockwise from outside: bottom, top, -z, +z, -x, +x
    p.idx = vec![
      0, 1, 5, 0, 5, 4, 2, 6, 7, 2, 7, 3, 0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 4, 6, 0, 6, 2, 1,
      3, 7, 1, 7, 5,
    ];
  } else {
    p.pos = vec![
      Vec3::new(-hx, 0.0, -hz),
      Vec3::new(hx, 0.0, -hz),
      Vec3::new(hx, 0.0, hz),
      Vec3::new(-hx, 0.0, hz),
    ];
    p.idx = vec![0, 2, 1, 0, 3, 2];
  }
  p.compute_normals();
  p
}

impl Editor {
  pub fn start_collision_tool(&mut self) -> bool {
    if !self.document_open() {
      return false;
    }
    self.stop_placement();
    self.stop_preview();
    match self.tool {
      ToolMode::Terrain => self.stop_terrain_brush(),
      ToolMode::Topology => self.stop_topology(),
      _ => {}
    }
    let c = &mut self.collision;
    c.picked.clear();
    c.hot.clear();
    if c.color_by.is_empty() {
      c.color_by = "material".into();
    }
    self.tool = ToolMode::Collision;
    true
  }

  pub(crate) fn stop_collision_tool(&mut self) {
    if self.tool == ToolMode::Collision {
      self.tool = ToolMode::Select;
    }
    self.collision.picked.clear();
    self.collision.hot.clear();
  }

  /// The document's collision nodes shown (not a level shown to compare with)
  fn collision_nodes(&self) -> Vec<usize> {
    let cache = self.scene.cache();
    (0..cache.count)
      .filter(|&i| {
        let n = &self.scene.nodes[i];
        n.kind == NodeKind::Collision && n.mesh.is_some() && cache.visible[i] && !cache.reference[i]
      })
      .collect()
  }

  /// Every face of the collision shown: its corners in the world, its surface
  fn collision_faces(&self) -> Vec<(Face, [Vec3; 3], u32)> {
    let cache = self.scene.cache();
    let mut out = Vec::new();
    for i in self.collision_nodes() {
      let n = &self.scene.nodes[i];
      let w = &cache.worlds[i];
      for (pi, p) in n.mesh.as_ref().unwrap().prims.iter().enumerate() {
        for (t, tri) in p.idx.as_chunks::<3>().0.iter().enumerate() {
          if tri.iter().any(|&v| v as usize >= p.pos.len()) {
            continue;
          }
          let v = [0, 1, 2].map(|k| w.point(p.pos[tri[k] as usize]));
          let pat = p.tri_pat.get(t).copied().unwrap_or(0);
          out.push(((n.id, pi as u32, t as u32), v, pat));
        }
      }
    }
    out
  }

  /// What the overlay's drawing depends on: the collision nodes, where they are, the colors
  fn collision_key(&self) -> u64 {
    let cache = self.scene.cache();
    let mut h = fnv(1469598103934665603, self.collision.color_by.len() as u64);
    for b in self.collision.color_by.bytes() {
      h = fnv(h, b as u64);
    }
    for i in self.collision_nodes() {
      let m = self.scene.nodes[i].mesh.as_ref().unwrap();
      h = fnv(h, self.scene.nodes[i].id as u64);
      h = fnv(h, m.uid ^ (m.version << 32));
      for v in cache.worlds[i].m {
        h = fnv(h, v.to_bits() as u64);
      }
    }
    h
  }

  /// The faces picked that are still there (an undo may have taken them away)
  fn collision_picked(&self) -> BTreeSet<Face> {
    self
      .collision
      .picked
      .iter()
      .copied()
      .filter(|&(id, pi, t)| {
        self
          .scene
          .find(id)
          .and_then(|n| n.mesh.as_ref())
          .and_then(|m| m.prims.get(pi as usize))
          .is_some_and(|p| (t as usize) * 3 + 2 < p.idx.len())
      })
      .collect()
  }

  /// The triangles to draw (x, y, z, rgba each, in the world), shaded by the way they face, and the
  /// legend: each surface, its color and its faces
  fn collision_overlay_bytes(&self, picked_only: bool) -> (Vec<u8>, Vec<(String, usize)>) {
    let by = self.collision.color_by.as_str();
    let picked = self.collision_picked();
    let mut bytes = Vec::new();
    let mut legend: BTreeMap<&'static str, usize> = BTreeMap::new();
    let light = normalize(Vec3::new(0.35, 1.0, 0.25));
    for (face, v, pat) in self.collision_faces() {
      let key = surface_key(pat, by);
      *legend.entry(key).or_default() += 1;
      if picked_only != picked.contains(&face) {
        continue;
      }
      let n = cross(v[1] - v[0], v[2] - v[0]);
      let lit = if length(n) > 0.0 {
        0.55 + 0.45 * dot(normalize(n), light).abs()
      } else {
        1.0
      };
      let rgba = if picked_only {
        [255, 150, 30, 200]
      } else {
        let c = key_color(key);
        [
          (c[0] * lit * 255.0) as u8,
          (c[1] * lit * 255.0) as u8,
          (c[2] * lit * 255.0) as u8,
          140,
        ]
      };
      for p in v {
        for f in [p.x, p.y, p.z] {
          bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes.extend_from_slice(&rgba);
      }
    }
    (
      bytes,
      legend
        .into_iter()
        .map(|(k, n)| (k.to_string(), n))
        .collect(),
    )
  }

  /// The faces of the same surface touching `seed`, through corners within 1 cm
  fn collision_connected(&self, seed: Face) -> BTreeSet<Face> {
    let faces = self.collision_faces();
    let Some(&(_, _, pat)) = faces.iter().find(|f| f.0 == seed) else {
      return BTreeSet::new();
    };
    let q = |p: Vec3| {
      (
        (p.x * 100.0).round() as i64,
        (p.y * 100.0).round() as i64,
        (p.z * 100.0).round() as i64,
      )
    };
    let same: Vec<&(Face, [Vec3; 3], u32)> = faces.iter().filter(|f| f.2 == pat).collect();
    let mut at: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (k, f) in same.iter().enumerate() {
      for p in f.1 {
        at.entry(q(p)).or_default().push(k);
      }
    }
    let start = same.iter().position(|f| f.0 == seed).unwrap();
    let mut seen = vec![false; same.len()];
    seen[start] = true;
    let mut todo = VecDeque::from([start]);
    while let Some(k) = todo.pop_front() {
      for p in same[k].1 {
        for &j in &at[&q(p)] {
          if !seen[j] {
            seen[j] = true;
            todo.push_back(j);
          }
        }
      }
    }
    (0..same.len())
      .filter(|&k| seen[k])
      .map(|k| same[k].0)
      .collect()
  }

  /// `faces` picked: in place of those picked, or (`add`) added, or taken out when all of them
  /// were picked already
  fn collision_take(&mut self, faces: BTreeSet<Face>, add: bool) {
    let p = &mut self.collision.picked;
    if !add {
      *p = faces;
    } else if !faces.is_empty() && faces.iter().all(|f| p.contains(f)) {
      p.retain(|f| !faces.contains(f));
    } else {
      p.extend(faces);
    }
  }

  /// A click: the face under the ray (`connected`: with the faces of its surface touching it); off
  /// the collision, where it lands on the decor or the ground is kept for a floor or a box
  fn collision_pick(&mut self, ray: &Ray, add: bool, connected: bool) {
    let f = PickFilter {
      render: false,
      actors: false,
      collision: true,
      include_locked: true,
      ..Default::default()
    };
    let hit = pick(&self.scene, ray, &f);
    if hit.hit() && hit.prim >= 0 {
      self.collision.last_point = Some(hit.point);
      let face = (hit.node, hit.prim as u32, hit.tri);
      let faces = if connected {
        self.collision_connected(face)
      } else {
        BTreeSet::from([face])
      };
      self.collision_take(faces, add);
      return;
    }
    let decor = pick(
      &self.scene,
      ray,
      &PickFilter {
        actors: false,
        include_locked: true,
        ..Default::default()
      },
    );
    self.collision.last_point = if decor.hit() {
      Some(decor.point)
    } else {
      ray_plane(ray, Vec3::default(), Vec3::new(0.0, 1.0, 0.0))
        .filter(|&t| t > 0.0 && t < 2000.0)
        .map(|t| ray.at(t))
    };
    if !add {
      self.collision.picked.clear();
    }
  }

  /// A box drawn over the view: the faces whose middle is in it, seen or not
  fn collision_box(&mut self, vp: &Mat4, size: [f32; 2], a: [f32; 2], b: [f32; 2], add: bool) {
    let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
    let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
    let faces = self
      .collision_faces()
      .into_iter()
      .filter(|(_, v, _)| {
        let c = *vp * Vec4::from_vec3((v[0] + v[1] + v[2]) * (1.0 / 3.0), 1.0);
        if c.w <= 1e-5 {
          return false;
        }
        let x = (c.x / c.w * 0.5 + 0.5) * size[0];
        let y = (1.0 - (c.y / c.w * 0.5 + 0.5)) * size[1];
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
      })
      .map(|f| f.0)
      .collect();
    self.collision_take(faces, add);
  }

  /// The faces of a surface of the legend, or those under the pieces selected (within their box)
  fn collision_select(&mut self, surface: Option<&str>, pieces: bool, add: bool) {
    let by = self.collision.color_by.clone();
    let boxes: Vec<_> = if pieces {
      let cache = self.scene.cache();
      self
        .selection
        .iter()
        .filter_map(|&id| self.scene.index_of(id))
        .filter(|&i| self.scene.nodes[i].kind == NodeKind::Render)
        .map(|i| cache.bounds[i].expanded(0.05))
        .collect()
    } else {
      Vec::new()
    };
    let faces = self
      .collision_faces()
      .into_iter()
      .filter(|(_, v, pat)| {
        if let Some(s) = surface {
          return surface_key(*pat, &by) == s;
        }
        let c = (v[0] + v[1] + v[2]) * (1.0 / 3.0);
        boxes.iter().any(|b| b.contains(c))
      })
      .map(|f| f.0)
      .collect();
    self.collision_take(faces, add);
  }

  /// One undo step over the collision nodes of the faces picked: `edit` gets each node's mesh and
  /// its faces picked (primitive, triangle)
  fn collision_edit(
    &mut self,
    label: &str,
    mut edit: impl FnMut(&mut Mesh, &Mat4, &BTreeSet<(u32, u32)>),
  ) {
    let picked = self.collision_picked();
    let mut by_node: BTreeMap<u32, BTreeSet<(u32, u32)>> = BTreeMap::new();
    for (id, pi, t) in picked {
      by_node.entry(id).or_default().insert((pi, t));
    }
    if by_node.is_empty() {
      return;
    }
    let ids: Vec<u32> = by_node.keys().copied().collect();
    let worlds: Vec<Mat4> = ids.iter().map(|&id| self.scene.world_of(id)).collect();
    self.inspector_edit(&ids, false, label, |scene| {
      for (k, (id, faces)) in by_node.iter().enumerate() {
        let Some(mesh) = scene.find_mut(*id).and_then(|n| n.mesh.as_mut()) else {
          continue;
        };
        // its own, the one before kept for undo
        let m = Arc::make_mut(mesh);
        edit(m, &worlds[k], faces);
        m.touch();
      }
    });
  }

  fn collision_paint(&mut self, change: &SurfaceChange) {
    self.collision_edit("undo.collision.paint", |m, _, faces| {
      for &(pi, t) in faces {
        if let Some(p) = m.prims.get_mut(pi as usize) {
          let n = p.tri_count();
          p.tri_pat.resize(n, 0);
          if let Some(pat) = p.tri_pat.get_mut(t as usize) {
            *pat = change.apply(*pat);
          }
        }
      }
    });
  }

  fn collision_delete(&mut self) {
    self.collision_edit("undo.collision.delete", |m, _, faces| {
      for (pi, p) in m.prims.iter_mut().enumerate() {
        let n = p.tri_count();
        p.tri_pat.resize(n, 0);
        let keep: Vec<usize> = (0..n)
          .filter(|&t| !faces.contains(&(pi as u32, t as u32)))
          .collect();
        if keep.len() == n {
          continue;
        }
        p.idx = keep
          .iter()
          .flat_map(|&t| p.idx[t * 3..t * 3 + 3].to_vec())
          .collect();
        p.tri_pat = keep.iter().map(|&t| p.tri_pat[t]).collect();
      }
    });
    self.collision.picked.clear();
  }

  /// The faces picked moved by `delta` (world, meters): their corners their own, so that the faces
  /// around stay where they are
  fn collision_move(&mut self, delta: Vec3) {
    self.collision_edit("undo.collision.move", |m, world, faces| {
      let inv = world.inverse();
      for &(pi, t) in faces {
        let Some(p) = m.prims.get_mut(pi as usize) else {
          continue;
        };
        for k in 0..3 {
          let at = t as usize * 3 + k;
          let Some(&v) = p.idx.get(at) else { continue };
          let moved = inv.point(world.point(p.pos[v as usize]) + delta);
          p.idx[at] = p.pos.len() as u32;
          p.pos.push(moved);
          if p.nrm.len() + 1 == p.pos.len() {
            p.nrm.push(Vec3::new(0.0, 1.0, 0.0));
          }
        }
        p.nrm.resize(p.pos.len(), Vec3::new(0.0, 1.0, 0.0));
      }
      for p in &mut m.prims {
        p.compute_normals();
      }
    });
  }

  /// A new collision node: `prim` (its surface `pat`) placed by `local`, kept by the export
  /// whatever its choice of collision
  fn collision_add_node(
    &mut self,
    name: &str,
    mut prim: Primitive,
    pat: u32,
    local: Transform,
  ) -> u32 {
    prim.tri_pat = vec![pat; prim.tri_count()];
    let mut mesh = Mesh::new();
    mesh.name = name.into();
    mesh.prims.push(prim);
    let mut names: std::collections::HashSet<String> =
      self.scene.nodes.iter().map(|n| n.name.clone()).collect();
    self.scene.add(Node {
      name: super::levels::unique_name(name, &mut names),
      kind: NodeKind::Collision,
      mesh: Some(Arc::new(mesh)),
      extras: json!({"og_collision": "made"}),
      local,
      ..Default::default()
    })
  }

  /// A floor or a box (`size`: width, height, depth) where the last click landed, of that surface
  fn collision_add(
    &mut self,
    boxed: bool,
    size: Vec3,
    change: &SurfaceChange,
  ) -> Result<(), String> {
    let at = self
      .collision
      .last_point
      .ok_or_else(|| tr("collision.no_point").to_string())?;
    let size = Vec3::new(
      size.x.clamp(0.1, 500.0),
      size.y.clamp(0.1, 500.0),
      size.z.clamp(0.1, 500.0),
    );
    // the default surface: stone ground
    let pat = change.apply(
      SurfaceChange {
        material: Some("stone".into()),
        ..Default::default()
      }
      .apply(0),
    );
    let label = if boxed {
      "undo.collision.box"
    } else {
      "undo.collision.floor"
    };
    let name = if boxed {
      "collision-box"
    } else {
      "collision-floor"
    };
    // the node is new: the change knows it (no save before)
    let change_nodes = crate::history::NodeChange::new(&self.scene);
    let id = self.collision_add_node(
      name,
      floor_or_box(boxed, size),
      pat,
      Transform {
        t: at,
        ..Default::default()
      },
    );
    self.scene.begin_frame();
    self
      .history
      .push(change_nodes.commit(&self.scene, tr(label), None));
    self.collision.picked = (0..if boxed { 12 } else { 2 })
      .map(|t| (id, 0, t))
      .collect();
    Ok(())
  }

  /// The pieces selected collide by faces of their shape, in the tool (their own collision none):
  /// faces to change one by one
  fn collision_from_pieces(&mut self, change: &SurfaceChange) -> usize {
    let cache = self.scene.cache();
    let pieces: Vec<(u32, String, Mat4, Arc<Mesh>)> = self
      .selection
      .iter()
      .filter_map(|&id| self.scene.index_of(id))
      .filter(|&i| {
        let n = &self.scene.nodes[i];
        n.kind == NodeKind::Render && n.mesh.is_some() && cache.actor[i] == 0
      })
      .map(|i| {
        let n = &self.scene.nodes[i];
        (
          n.id,
          n.name.clone(),
          cache.worlds[i],
          n.mesh.clone().unwrap(),
        )
      })
      .collect();
    if pieces.is_empty() {
      return 0;
    }
    let pat = change.apply(
      SurfaceChange {
        material: Some("stone".into()),
        ..Default::default()
      }
      .apply(0),
    );
    let ids: Vec<u32> = pieces.iter().map(|p| p.0).collect();
    let mut made = Vec::new();
    self.inspector_edit(&ids, false, "undo.collision.from_pieces", |scene| {
      for (id, _, _, _) in &pieces {
        // the builder makes it no collision of its own: the tool's faces are its collision
        if let Some(n) = scene.find_mut(*id) {
          let mut b = decor_behavior_of(&n.extras);
          b.collision.set = true;
          b.collision.kind = "none".into();
          n.extras["og_decor"] = decor_behavior_json(&b);
        }
      }
    });
    // ponytail: a second undo step for the faces (the pieces' collision first); one step if users
    // find two confusing
    let change_nodes = crate::history::NodeChange::new(&self.scene);
    for (_, name, world, mesh) in &pieces {
      let mut prim = Primitive::default();
      for p in &mesh.prims {
        let base = prim.pos.len() as u32;
        prim.pos.extend(p.pos.iter().map(|&v| world.point(v)));
        prim.idx.extend(p.idx.iter().map(|&i| base + i));
      }
      prim.compute_normals();
      made.push(self.collision_add_node(
        &format!("{name}-collision"),
        prim,
        pat,
        Transform::default(),
      ));
    }
    self.scene.begin_frame();
    self
      .history
      .push(change_nodes.commit(&self.scene, tr("undo.collision.from_pieces"), None));
    made.len()
  }

  /// The places of the floors where Jak meets more collision than the game gathers at once (as the
  /// export's report counts them, the collision of the tool alone)
  fn collision_check(&mut self) -> usize {
    let mut all = TriGrid::new(2.0);
    for (_, v, _) in self.collision_faces() {
      all.add(v[0], v[1], v[2]);
    }
    self.collision.hot = collision_hot_spots(&all, 1);
    self.collision.hot_key = self.collision_key();
    self.collision.hot.len()
  }

  /// What the interface shows (get_state's "collision")
  pub(crate) fn collision_state(&self) -> Value {
    if self.tool != ToolMode::Collision && !self.view.collision {
      return Value::Null;
    }
    let picked = self.collision_picked();
    let by = self.collision.color_by.as_str();
    let mut surfaces: BTreeMap<&str, usize> = BTreeMap::new();
    if !picked.is_empty() {
      for (face, _, pat) in self.collision_faces() {
        if picked.contains(&face) {
          *surfaces.entry(surface_key(pat, by)).or_default() += 1;
        }
      }
    }
    let key = self.collision_key();
    let pieces = self
      .selection
      .iter()
      .filter(|&&id| {
        self
          .scene
          .find(id)
          .is_some_and(|n| n.kind == NodeKind::Render)
      })
      .count();
    let hot: Vec<Value> = if self.collision.hot_key == key {
      self
        .collision
        .hot
        .iter()
        .map(|(n, at)| json!({"triangles": n, "at": at}))
        .collect()
    } else {
      Vec::new()
    };
    json!({
      "color_by": by,
      "key": key,
      "picked": picked.len(),
      "picked_key": picked.iter().fold(key, |h, f| fnv(fnv(fnv(h, f.0 as u64), f.1 as u64), f.2 as u64)),
      "surfaces": surfaces,
      "last_point": self.collision.last_point,
      "pieces": pieces,
      "hot": hot,
      "checked": self.collision.hot_key == key,
    })
  }

  /// The tool's calls (api.rs)
  pub(crate) fn collision_call(&mut self, name: &str, a: &Value) -> Result<Value, String> {
    let get = |k: &str| a.get(k).cloned().unwrap_or(Value::Null);
    let add = a.get("add").and_then(Value::as_bool).unwrap_or(false);
    let vec3 = |k: &str| serde_json::from_value::<Vec3>(get(k)).unwrap_or_default();
    match name {
      "start_collision_tool" => return Ok(json!(self.start_collision_tool())),
      "stop_collision_tool" => self.stop_collision_tool(),
      // {by: material | effect | kind}
      "collision_color_by" => {
        let by = a.get("by").and_then(Value::as_str).unwrap_or("material");
        self.collision.color_by = match by {
          "effect" | "kind" => by.into(),
          _ => "material".into(),
        };
      }
      // {key?: the one the view has}: the triangles to draw when they changed, and those picked
      "collision_overlay" => {
        let key = self.collision_key();
        let mut out = json!({"key": key});
        if a.get("key").and_then(Value::as_u64) != Some(key) {
          let (bytes, legend) = self.collision_overlay_bytes(false);
          out["triangles"] = json!(super::preview::base64(&bytes));
          out["legend"] = legend
            .iter()
            .map(|(k, n)| json!({"surface": k, "faces": n, "color": key_color(k)}))
            .collect();
        }
        out["picked"] = json!(super::preview::base64(
          &self.collision_overlay_bytes(true).0
        ));
        return Ok(out);
      }
      // {origin, dir, add, connected}
      "collision_pick" => {
        let ray = Ray::new(vec3("origin"), normalize(vec3("dir")));
        let connected = a.get("connected").and_then(Value::as_bool).unwrap_or(false);
        self.collision_pick(&ray, add, connected);
      }
      // {view_proj, width, height, a: [x, y], b: [x, y], add}
      "collision_box" => {
        let vp: Mat4 =
          serde_json::from_value(get("view_proj")).map_err(|_| "view_proj: 16 floats")?;
        let size: [f32; 2] = [
          a.get("width").and_then(Value::as_f64).unwrap_or(1.0) as f32,
          a.get("height").and_then(Value::as_f64).unwrap_or(1.0) as f32,
        ];
        let pa: [f32; 2] = serde_json::from_value(get("a")).unwrap_or_default();
        let pb: [f32; 2] = serde_json::from_value(get("b")).unwrap_or_default();
        self.collision_box(&vp, size, pa, pb, add);
      }
      // {surface?: a key of the legend, pieces?: true, none?: true, add}
      "collision_select" => {
        if a.get("none").and_then(Value::as_bool).unwrap_or(false) {
          self.collision.picked.clear();
        } else {
          let surface = a.get("surface").and_then(Value::as_str).map(str::to_string);
          let pieces = a.get("pieces").and_then(Value::as_bool).unwrap_or(false);
          self.collision_select(surface.as_deref(), pieces, add);
        }
      }
      // {material?, event?, mode?, noentity?, nocamera?, noedge?, nolineofsight?}
      "collision_paint" => self.collision_paint(&SurfaceChange::from_json(a)),
      "collision_delete" => self.collision_delete(),
      // {delta: [x, y, z]}
      "collision_move" => self.collision_move(vec3("delta")),
      // {shape: floor | box, size: [w, h, d], the surface's fields}
      "collision_add" => {
        let boxed = a.get("shape").and_then(Value::as_str) == Some("box");
        self.collision_add(boxed, vec3("size"), &SurfaceChange::from_json(a))?;
      }
      // the surface's fields
      "collision_from_pieces" => {
        return Ok(json!(
          self.collision_from_pieces(&SurfaceChange::from_json(a))
        ));
      }
      "collision_check" => return Ok(json!(self.collision_check())),
      _ => return Err(format!("unknown call {name}")),
    }
    self.scene_changed();
    Ok(self.collision_state())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::test_library::make_floor;

  fn editor() -> Editor {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.new_level();
    ed
  }

  /// a level's collision: a floor of 2 triangles, stone ground
  fn floor(ed: &mut Editor) -> u32 {
    let mut m = (*make_floor(-1)).clone();
    for p in &mut m.prims {
      p.tri_pat = vec![23 << 10; p.tri_count()];
    }
    let id = ed.scene.add(Node {
      name: "lvx-collision-0".into(),
      kind: NodeKind::Collision,
      mesh: Some(Arc::new(m)),
      ..Default::default()
    });
    ed.scene.begin_frame();
    id
  }

  #[test]
  fn kept_by_the_project() {
    // a face painted ice and a box added: saved with the project, read back the same
    let mut ed = editor();
    floor(&mut ed);
    ed.start_collision_tool();
    let down = Ray::new(Vec3::new(0.5, 10.0, 0.5), Vec3::new(0.0, -1.0, 0.0));
    ed.collision_pick(&down, false, false);
    ed.call("collision_paint", &json!({"material": "ice"}))
      .unwrap();
    ed.call("collision_add", &json!({"shape": "box", "size": [1, 1, 1]}))
      .unwrap();
    let path = std::env::temp_dir().join(format!("jle-collision-{}.glb", std::process::id()));
    crate::io::gltf::export_glb(&ed.scene, &path, &Default::default()).unwrap();
    let mut back = crate::scene::Scene::new();
    crate::io::gltf::import_gltf(&mut back, &path, &Default::default()).unwrap();
    let _ = std::fs::remove_file(&path);
    let pats: Vec<(String, Vec<u32>)> = back
      .nodes
      .iter()
      .filter(|n| n.kind == NodeKind::Collision)
      .map(|n| {
        let m = n.mesh.as_ref().unwrap();
        (
          n.name.clone(),
          m.prims.iter().flat_map(|p| p.tri_pat.clone()).collect(),
        )
      })
      .collect();
    let floor = &pats.iter().find(|p| p.0 == "lvx-collision-0").unwrap().1;
    assert_eq!(
      floor
        .iter()
        .filter(|&&p| builder_surface(p).0.material == 1)
        .count(),
      1
    );
    let made = back
      .nodes
      .iter()
      .find(|n| n.extras["og_collision"] == "made")
      .unwrap();
    assert_eq!(made.mesh.as_ref().unwrap().tri_count(), 12);
  }

  #[test]
  fn surfaces_written_as_the_builder_reads_them() {
    let ice_deadly = SurfaceChange {
      material: Some("ice".into()),
      event: Some("deadly".into()),
      mode: Some("wall".into()),
      flags: vec![(2, true)],
    };
    let (s, lost) = builder_surface(ice_deadly.apply(23 << 10));
    assert!(lost.is_empty() && s.material == 1 && s.event == 1 && s.mode == 1 && s.noedge);
    let stone = SurfaceChange {
      material: Some("stone".into()),
      ..Default::default()
    };
    assert_eq!(builder_surface(stone.apply(1 << 10)).0.material, 0);
    assert_eq!(surface_key(stone.apply(0), "material"), "stone");
  }

  #[test]
  fn faces_picked_painted_moved_deleted_and_added() {
    let mut ed = editor();
    let id = floor(&mut ed);
    assert!(ed.start_collision_tool());
    // a click on the floor from above: one face; Alt: both (same surface, touching)
    let down = Ray::new(Vec3::new(0.5, 10.0, 0.5), Vec3::new(0.0, -1.0, 0.0));
    ed.collision_pick(&down, false, false);
    assert_eq!(ed.collision_picked().len(), 1);
    ed.collision_pick(&down, false, true);
    assert_eq!(ed.collision_picked().len(), 2);
    // painted ice: one undo step
    ed.call("collision_paint", &json!({"material": "ice"}))
      .unwrap();
    let pats = |ed: &Editor| {
      ed.scene.find(id).unwrap().mesh.as_ref().unwrap().prims[0]
        .tri_pat
        .clone()
    };
    assert!(
      pats(&ed)
        .iter()
        .all(|&p| builder_surface(p).0.material == 1)
    );
    ed.call("undo", &Value::Null).unwrap();
    assert!(
      pats(&ed)
        .iter()
        .all(|&p| builder_surface(p).0.material == 0)
    );
    ed.call("redo", &Value::Null).unwrap();
    // the legend: 2 faces of ice
    let o = ed.call("collision_overlay", &json!({})).unwrap();
    assert_eq!(o["legend"][0]["surface"], "ice");
    assert_eq!(o["legend"][0]["faces"], 2);
    // moved 1 m up: its corners its own
    ed.collision_pick(&down, false, false);
    ed.call("collision_move", &json!({"delta": [0, 1, 0]}))
      .unwrap();
    let ys: Vec<f32> = {
      let m = ed.scene.find(id).unwrap().mesh.clone().unwrap();
      let p = &m.prims[0];
      p.idx.iter().map(|&v| p.pos[v as usize].y).collect()
    };
    assert_eq!(ys.iter().filter(|&&y| (y - 1.0).abs() < 1e-4).count(), 3);
    // deleted
    ed.call("collision_delete", &json!({})).unwrap();
    assert_eq!(
      ed.scene
        .find(id)
        .unwrap()
        .mesh
        .as_ref()
        .unwrap()
        .tri_count(),
      1
    );
    // a box where the last click landed, metal: a node the export keeps
    ed.call(
      "collision_add",
      &json!({"shape": "box", "size": [2, 1, 2], "material": "metal"}),
    )
    .unwrap();
    let made = ed
      .scene
      .nodes
      .iter()
      .find(|n| n.extras["og_collision"] == "made")
      .unwrap();
    let m = made.mesh.as_ref().unwrap();
    assert_eq!(m.tri_count(), 12);
    assert!(
      m.prims[0]
        .tri_pat
        .iter()
        .all(|&p| builder_surface(p).0.material == 16)
    );
    assert_eq!(ed.collision_picked().len(), 12);
    // a box over the view: the faces whose middle is in it (a camera above, looking down)
    ed.collision_select(None, false, false);
    let vp = Mat4::identity();
    ed.collision_box(&vp, [2.0, 2.0], [-100.0, -100.0], [100.0, 100.0], false);
    assert!(!ed.collision_picked().is_empty());
  }
}
