//! The topology tool: the points or the faces of one element's mesh, picked with the mouse (a click,
//! Shift to add, a box), moved, turned or scaled with the gizmo, deleted, pulled out (extruded),
//! cut in four, turned over (scene/topology.rs). Every change is undone with Ctrl+Z
//! (src/app/topology_tool.cpp).
//!
//! The viewport (ui/render/topology_tool.ts) draws the points and the faces picked
//! (`topology_view`), sends its clicks and boxes with its camera's view-projection (as box_nodes)
//! and the gizmo's drag on what is picked (`topology_drag` while it goes on, `topology_drag_end`:
//! one undo step).

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};

use super::{Editor, ToolMode};
use crate::math::{Mat4, Ray, Vec3, Vec4, dot, length, normalize, ray_triangle};
use crate::scene::topology::{
  MeshPoints, MeshRef, delete_faces, extrude_faces, face_vertices, faces_of_vertices, flip_faces,
  mesh_points, mesh_ref, ref_index, ref_prim, subdivide_faces,
};
use crate::scene::{Mesh, Node, NodeKind};

/// The points drawn over the view up to this many (a mesh of more: picked by a box)
const K_MANY_POINTS: usize = 40000;

/// One element's points or faces picked, a gizmo drag on them (App's m_topo_*)
pub struct TopologyState {
  pub node: u32,
  /// picks faces, else points
  pub faces: bool,
  /// into `cache`
  points: BTreeSet<u32>,
  faces_sel: BTreeSet<MeshRef>,
  cache: MeshPoints,
  /// the mesh (uid, version) the cache is of
  cache_key: (u64, u64),
  /// a gizmo drag going on: the node before it
  before: Option<Node>,
  /// its vertices where they were (world)
  start: Vec<(MeshRef, Vec3)>,
  pivot: Vec3,
  axes: Mat4,
  revision: u64,
  /// the inspector's distance of an extrusion (meters)
  pub extrude: f32,
}

impl Default for TopologyState {
  fn default() -> Self {
    Self {
      node: 0,
      faces: false,
      points: BTreeSet::new(),
      faces_sel: BTreeSet::new(),
      cache: MeshPoints::default(),
      cache_key: (0, 0),
      before: None,
      start: Vec::new(),
      pivot: Vec3::default(),
      axes: Mat4::identity(),
      revision: 0,
      extrude: 1.0,
    }
  }
}

/// world -> view pixels (origin top-left) with the viewport's view-projection, None behind it
fn project(vp: &Mat4, size: [f32; 2], w: Vec3) -> Option<[f32; 2]> {
  let c = *vp * Vec4::from_vec3(w, 1.0);
  (c.w > 1e-5).then(|| {
    [
      (c.x / c.w * 0.5 + 0.5) * size[0],
      (1.0 - (c.y / c.w * 0.5 + 0.5)) * size[1],
    ]
  })
}

fn get<T: serde::de::DeserializeOwned>(a: &Value, k: &str) -> Option<T> {
  serde_json::from_value(a.get(k)?.clone()).ok()
}

impl Editor {
  /// Whether the topology tool can edit the element's mesh (a mesh of its own, not an actor's)
  pub(crate) fn topology_editable(&self, id: u32) -> bool {
    self.scene.find(id).is_some_and(|n| {
      n.kind == NodeKind::Render
        && n.mesh.as_ref().is_some_and(|m| !m.prims.is_empty())
        && self.scene.actor_of(n) == 0
    })
  }

  pub fn start_topology(&mut self) -> bool {
    if !self.topology_editable(self.primary) {
      return false;
    }
    self.stop_placement();
    self.stop_preview();
    if self.tool == ToolMode::Terrain {
      self.stop_terrain_brush();
    }
    let t = &mut self.topology;
    t.node = self.primary;
    t.points.clear();
    t.faces_sel.clear();
    t.before = None;
    t.revision = self.history.revision();
    self.tool = ToolMode::Topology;
    true
  }

  pub(crate) fn stop_topology(&mut self) {
    if self.tool == ToolMode::Topology {
      self.tool = ToolMode::Select;
    }
    let t = &mut self.topology;
    t.node = 0;
    t.points.clear();
    t.faces_sel.clear();
    t.before = None;
  }

  /// the element gone: the tool stops; undone or redone: what was picked may be gone (false: no
  /// element)
  fn topology_sync(&mut self) -> bool {
    if self.tool != ToolMode::Topology
      || !self
        .scene
        .find(self.topology.node)
        .is_some_and(|n| n.mesh.is_some())
    {
      if self.tool == ToolMode::Topology {
        self.stop_topology();
      }
      return false;
    }
    let t = &mut self.topology;
    if self.history.revision() != t.revision && t.before.is_none() {
      t.revision = self.history.revision();
      t.points.clear();
      t.faces_sel.clear();
    }
    self.topology_cache();
    true
  }

  fn topology_mesh(&self) -> Option<(Arc<Mesh>, Mat4)> {
    let n = self.scene.find(self.topology.node)?;
    Some((n.mesh.clone()?, self.scene.world(n)))
  }

  /// the points of the element's mesh (made again when it changed)
  fn topology_cache(&mut self) {
    let Some((mesh, _)) = self.topology_mesh() else {
      return;
    };
    let t = &mut self.topology;
    if t.cache_key != (mesh.uid, mesh.version) {
      t.cache = mesh_points(&mesh);
      t.cache_key = (mesh.uid, mesh.version);
    }
  }

  /// the vertices of what is picked
  fn topology_vertices(&self) -> BTreeSet<MeshRef> {
    let Some((mesh, _)) = self.topology_mesh() else {
      return BTreeSet::new();
    };
    let t = &self.topology;
    if t.faces {
      return face_vertices(&mesh, &t.faces_sel);
    }
    let mut out = BTreeSet::new();
    for &p in &t.points {
      if let Some(v) = t.cache.verts.get(p as usize) {
        out.extend(v.iter().copied());
      }
    }
    out
  }

  /// the faces an edit works on: those picked, or around the points picked
  fn topology_faces(&self) -> BTreeSet<MeshRef> {
    if self.topology.faces {
      return self.topology.faces_sel.clone();
    }
    match self.topology_mesh() {
      Some((mesh, _)) => faces_of_vertices(&mesh, &self.topology_vertices()),
      None => BTreeSet::new(),
    }
  }

  /// An edit of the mesh, one undo step (its shape no longer made from its size)
  fn topology_edit<R>(&mut self, undo_key: &str, edit: impl FnOnce(&mut Mesh) -> R) -> Option<R> {
    let n = self.scene.find_mut(self.topology.node)?;
    let before = n.clone();
    // its own, the one before kept for undo
    let mesh = Arc::make_mut(n.mesh.as_mut()?);
    let r = edit(mesh);
    mesh.touch();
    if let Some(o) = n.extras.as_object_mut() {
      o.remove("og_shape");
    }
    self.commit_node_edit(before, undo_key);
    self.topology.revision = self.history.revision();
    self.topology_cache();
    Some(r)
  }

  /// the middle of what is picked (world), where the gizmo stands
  fn topology_center(&self) -> Option<Vec3> {
    let (mesh, world) = self.topology_mesh()?;
    let verts = self.topology_vertices();
    if verts.is_empty() {
      return None;
    }
    let mut c = Vec3::default();
    for &r in &verts {
      c += world.point(mesh.prims[ref_prim(r)].pos[ref_index(r) as usize]);
    }
    Some(c * (1.0 / verts.len() as f32))
  }

  /// A click (topology_tool): the face under the cursor, or the point nearest it on the screen
  /// (within 12 pixels, the nearest the camera among equals); `add` (Shift, Ctrl) adds it or takes
  /// it out
  fn topology_pick(&mut self, ray: &Ray, vp: &Mat4, size: [f32; 2], mouse: [f32; 2], add: bool) {
    let Some((mesh, world)) = self.topology_mesh() else {
      return;
    };
    let t = &mut self.topology;
    if t.faces {
      // the face under the cursor
      let mut best = 1e30f32;
      let mut hit = None;
      for (pi, p) in mesh.prims.iter().enumerate() {
        for (k, tri) in p.idx.chunks_exact(3).enumerate() {
          let [a, b, c] = [0, 1, 2].map(|j| world.point(p.pos[tri[j] as usize]));
          let d = ray_triangle(ray, a, b, c);
          if d > 0.0 && d < best {
            best = d;
            hit = Some(mesh_ref(pi, k));
          }
        }
      }
      if !add {
        t.faces_sel.clear();
      }
      if let Some(h) = hit
        && !(add && t.faces_sel.remove(&h))
      {
        t.faces_sel.insert(h);
      }
      return;
    }
    let mut best: Option<u32> = None;
    let (mut best_d, mut best_depth) = (12f32, 1e30f32);
    for (i, &p) in t.cache.pos.iter().enumerate() {
      let w = world.point(p);
      let Some(s) = project(vp, size, w) else {
        continue;
      };
      let d = (s[0] - mouse[0]).hypot(s[1] - mouse[1]);
      let depth = length(w - ray.o);
      if d < best_d - 0.5 || (d <= best_d + 0.5 && depth < best_depth) {
        best = Some(i as u32);
        best_d = best_d.min(d);
        best_depth = depth;
      }
    }
    if !add {
      t.points.clear();
    }
    if let Some(b) = best
      && !(add && t.points.remove(&b))
    {
      t.points.insert(b);
    }
  }

  /// A box drawn over the view: the points (or the faces' middles) inside it
  fn topology_box(&mut self, vp: &Mat4, size: [f32; 2], a: [f32; 2], b: [f32; 2], add: bool) {
    let Some((mesh, world)) = self.topology_mesh() else {
      return;
    };
    let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
    let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
    let inside = |local: Vec3| {
      project(vp, size, world.point(local))
        .is_some_and(|s| s[0] >= x0 && s[0] <= x1 && s[1] >= y0 && s[1] <= y1)
    };
    let t = &mut self.topology;
    if !add {
      t.points.clear();
      t.faces_sel.clear();
    }
    if t.faces {
      for (pi, p) in mesh.prims.iter().enumerate() {
        for (k, tri) in p.idx.chunks_exact(3).enumerate() {
          let c = (p.pos[tri[0] as usize] + p.pos[tri[1] as usize] + p.pos[tri[2] as usize])
            * (1.0 / 3.0);
          if inside(c) {
            t.faces_sel.insert(mesh_ref(pi, k));
          }
        }
      }
    } else {
      for (i, &p) in t.cache.pos.iter().enumerate() {
        if inside(p) {
          t.points.insert(i as u32);
        }
      }
    }
  }

  /// Points or faces: what is picked kept (the faces picked give their points, the points their
  /// faces)
  fn topology_set_faces(&mut self, faces: bool) {
    let Some((mesh, _)) = self.topology_mesh() else {
      return;
    };
    if faces == self.topology.faces {
      return;
    }
    if faces {
      self.topology.faces_sel = faces_of_vertices(&mesh, &self.topology_vertices());
    } else {
      let v = face_vertices(&mesh, &self.topology.faces_sel);
      let t = &mut self.topology;
      t.points = (0..t.cache.verts.len() as u32)
        .filter(|&i| t.cache.verts[i as usize].iter().any(|r| v.contains(r)))
        .collect();
    }
    self.topology.faces = faces;
  }

  fn topology_select_all(&mut self, all: bool) {
    let Some((mesh, _)) = self.topology_mesh() else {
      return;
    };
    let t = &mut self.topology;
    t.points.clear();
    t.faces_sel.clear();
    if !all {
      return;
    }
    if t.faces {
      for (pi, p) in mesh.prims.iter().enumerate() {
        t.faces_sel
          .extend((0..p.idx.len() / 3).map(|k| mesh_ref(pi, k)));
      }
    } else {
      t.points.extend(0..t.cache.pos.len() as u32);
    }
  }

  /// The gizmo dragged on what is picked: `frame` the gizmo's (its axes scale along), `delta` its
  /// move or turn (new = delta * start, world), else `scale` its factors along the frame's axes.
  /// The mesh changes as it goes, outside the history (topology_drag_end makes it one step).
  fn topology_drag(&mut self, frame: Mat4, delta: Option<Mat4>, scale: Option<Vec3>) {
    if self.topology.before.is_none() {
      let verts = self.topology_vertices();
      let Some(center) = self.topology_center() else {
        return;
      };
      let Some((mesh, world)) = self.topology_mesh() else {
        return;
      };
      let t = &mut self.topology;
      t.before = self.scene.find(t.node).cloned();
      t.start = verts
        .iter()
        .map(|&r| {
          let p = mesh.prims[ref_prim(r)].pos[ref_index(r) as usize];
          (r, world.point(p))
        })
        .collect();
      t.pivot = center;
      t.axes = frame;
    }
    let inv = self.scene.world_of(self.topology.node).inverse();
    let t = &self.topology;
    let Some(mesh) = self.scene.find_mut(t.node).and_then(|n| n.mesh.as_mut()) else {
      return;
    };
    // its own, the one before kept for undo
    let m = Arc::make_mut(mesh);
    for &(r, start) in &t.start {
      let w = match (scale, delta) {
        (Some(s), _) => {
          // along the gizmo's axes, around the middle of what is picked
          let d = start - t.pivot;
          let mut out = t.pivot;
          for (k, f) in [s.x, s.y, s.z].into_iter().enumerate() {
            let axis = normalize(t.axes.col3(k));
            out += axis * (dot(d, axis) * f);
          }
          out
        }
        (None, Some(delta)) => delta.point(start),
        (None, None) => start,
      };
      if let Some(p) = m.prims.get_mut(ref_prim(r))
        && let Some(v) = p.pos.get_mut(ref_index(r) as usize)
      {
        *v = inv.point(w);
      }
    }
    m.touch();
    self.scene_changed();
  }

  /// The drag let go: one undo step (`op`: translate, rotate, scale: its label)
  fn topology_drag_end(&mut self, op: &str) {
    let Some(before) = self.topology.before.take() else {
      return;
    };
    let prims: BTreeSet<usize> = self
      .topology
      .start
      .iter()
      .map(|&(r, _)| ref_prim(r))
      .collect();
    if let Some(n) = self.scene.find_mut(self.topology.node)
      && let Some(mesh) = n.mesh.as_mut()
    {
      let m = Arc::make_mut(mesh);
      for p in prims {
        if let Some(p) = m.prims.get_mut(p) {
          p.compute_normals();
        }
      }
      m.touch();
      if let Some(o) = n.extras.as_object_mut() {
        o.remove("og_shape");
      }
    }
    let label = match op {
      "rotate" => "undo.topology.rotate",
      "scale" => "undo.topology.scale",
      _ => "undo.topology.move",
    };
    self.commit_node_edit(before, label);
    self.topology.revision = self.history.revision();
    self.topology.start.clear();
  }

  pub(crate) fn topology_delete(&mut self) {
    if !self.topology_sync() {
      return;
    }
    let faces = self.topology_faces();
    if faces.is_empty() {
      return;
    }
    self.topology_edit("undo.topology.delete", |m| delete_faces(m, &faces));
    self.topology.points.clear();
    self.topology.faces_sel.clear();
  }

  /// What the viewport draws (draw_topology_overlay, the points of topology_tool): the element,
  /// what is picked (points: indices into `points`; faces: primitive, triangle pairs), the middle
  /// of what is picked (the gizmo's), and with `points` the points themselves (in the element's
  /// space; none above 40000)
  fn topology_view(&mut self, with_points: bool) -> Value {
    if !self.topology_sync() {
      return Value::Null;
    }
    let Some((mesh, _)) = self.topology_mesh() else {
      return Value::Null;
    };
    let t = &self.topology;
    let points = (with_points && t.cache.pos.len() <= K_MANY_POINTS).then(|| {
      t.cache
        .pos
        .iter()
        .flat_map(|p| [p.x, p.y, p.z])
        .collect::<Vec<f32>>()
    });
    let faces: Vec<u64> = t
      .faces_sel
      .iter()
      .flat_map(|&f| [ref_prim(f) as u64, ref_index(f) as u64])
      .collect();
    json!({
      "node": t.node,
      "mesh": mesh.uid as u32,
      "version": mesh.version as u32,
      "faces": t.faces,
      "selected_points": t.points,
      "selected_faces": faces,
      "center": self.topology_center(),
      "count": t.cache.pos.len(),
      "points": points,
    })
  }

  /// The inspector's section (ui_topology_inspector): the element, its counts, what is picked
  pub(crate) fn topology_state(&mut self) -> Value {
    if self.tool != ToolMode::Topology || !self.topology_sync() {
      return Value::Null;
    }
    let Some(n) = self.scene.find(self.topology.node) else {
      return Value::Null;
    };
    let t = &self.topology;
    let tris = n.mesh.as_ref().map_or(0, |m| m.tri_count());
    let faces = if t.faces {
      t.faces_sel.len()
    } else {
      self.topology_faces().len()
    };
    json!({
      "node": t.node,
      "name": n.name,
      "faces": t.faces,
      "points": t.cache.pos.len(),
      "triangles": tris,
      "picked": if t.faces { t.faces_sel.len() } else { t.points.len() },
      "editable_faces": faces,
      "extrude": t.extrude,
    })
  }

  /// what of the tool get_state's changes follow (cheap: every call)
  pub(crate) fn topology_key(&self) -> Value {
    let t = &self.topology;
    let mesh = self
      .scene
      .find(t.node)
      .and_then(|n| n.mesh.as_ref())
      .map(|m| (m.uid, m.version));
    json!([
      t.node,
      t.faces,
      t.points.len(),
      t.faces_sel.len(),
      t.extrude,
      mesh
    ])
  }

  /// The tool's calls (api.rs)
  pub(crate) fn topology_call(&mut self, name: &str, a: &Value) -> Result<Value, String> {
    let add = a.get("add").and_then(Value::as_bool).unwrap_or(false);
    let vp: Option<Mat4> = get(a, "view_proj");
    let size = [
      get::<f32>(a, "width").unwrap_or(1.0),
      get::<f32>(a, "height").unwrap_or(1.0),
    ];
    match name {
      "start_topology" => return Ok(json!(self.start_topology())),
      "stop_topology" => {
        self.stop_topology();
        return Ok(Value::Null);
      }
      "topology_delete" => self.topology_delete(),
      "topology_view" => {}
      // {origin, dir, view_proj, width, height, mouse: [x, y], add}
      "topology_pick" => {
        if self.topology_sync() {
          let ray = Ray::new(
            get(a, "origin").unwrap_or_default(),
            normalize(get(a, "dir").unwrap_or_default()),
          );
          let vp = vp.ok_or("view_proj: the view's 16 floats")?;
          let mouse = get(a, "mouse").unwrap_or_default();
          self.topology_pick(&ray, &vp, size, mouse, add);
        }
      }
      // {view_proj, width, height, a: [x, y], b: [x, y], add}
      "topology_box" => {
        if self.topology_sync() {
          let vp = vp.ok_or("view_proj: the view's 16 floats")?;
          let (ca, cb) = (
            get(a, "a").unwrap_or_default(),
            get(a, "b").unwrap_or_default(),
          );
          self.topology_box(&vp, size, ca, cb, add);
        }
      }
      // {faces?: bool, all?: bool (false: none), extrude?: meters}
      "topology_select" => {
        if self.topology_sync() {
          if let Some(f) = a.get("faces").and_then(Value::as_bool) {
            self.topology_set_faces(f);
          }
          if let Some(all) = a.get("all").and_then(Value::as_bool) {
            self.topology_select_all(all);
          }
        }
        if let Some(d) = get::<f32>(a, "extrude") {
          self.topology.extrude = d.clamp(-100.0, 100.0);
        }
      }
      // {frame: [16], delta?: [16] | scale?: [x, y, z]}
      "topology_drag" => {
        if self.topology_sync() {
          let frame = get(a, "frame").unwrap_or_else(Mat4::identity);
          self.topology_drag(frame, get(a, "delta"), get(a, "scale"));
        }
      }
      // {op: translate | rotate | scale}
      "topology_drag_end" => {
        let op = a.get("op").and_then(Value::as_str).unwrap_or("translate");
        self.topology_drag_end(op);
      }
      // {distance?}: pulled out along their normal; the new faces picked
      "topology_extrude" => {
        if let Some(d) = get::<f32>(a, "distance") {
          self.topology.extrude = d.clamp(-100.0, 100.0);
        }
        if self.topology_sync() {
          let (faces, d) = (self.topology_faces(), self.topology.extrude);
          if !faces.is_empty()
            && let Some(made) =
              self.topology_edit("undo.topology.extrude", |m| extrude_faces(m, &faces, d))
          {
            self.topology.faces = true;
            self.topology.faces_sel = made;
          }
        }
      }
      "topology_subdivide" => {
        if self.topology_sync() {
          let faces = self.topology_faces();
          if !faces.is_empty()
            && let Some(made) =
              self.topology_edit("undo.topology.subdivide", |m| subdivide_faces(m, &faces))
          {
            self.topology.faces = true;
            self.topology.faces_sel = made;
          }
        }
      }
      "topology_flip" => {
        if self.topology_sync() {
          let faces = self.topology_faces();
          if !faces.is_empty() {
            self.topology_edit("undo.topology.flip", |m| flip_faces(m, &faces));
          }
        }
      }
      _ => return Err(format!("unknown call: {name}")),
    }
    let with_points = a.get("points").and_then(Value::as_bool).unwrap_or(false);
    Ok(self.topology_view(with_points))
  }
}

#[cfg(test)]
mod tests {
  use serde_json::{Value, json};

  use super::super::{Editor, ToolMode};
  use crate::io::test_library::make_box;
  use crate::math::{Mat4, Vec3};
  use crate::scene::{Node, NodeKind};

  /// a document with a box (2 m, from y 0 to 2) selected
  fn editor_with_box() -> (Editor, u32) {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.new_level();
    let id = ed.scene.add(Node {
      name: "box".into(),
      kind: NodeKind::Render,
      mesh: Some(make_box("box", -1)),
      ..Default::default()
    });
    ed.scene.begin_frame();
    ed.select_only(id);
    (ed, id)
  }

  fn call(ed: &mut Editor, name: &str, a: Value) -> Value {
    ed.call(name, &a).unwrap()
  }

  fn bounds(ed: &Editor, id: u32) -> crate::math::AABB {
    ed.scene.find(id).unwrap().mesh.as_ref().unwrap().bounds()
  }

  fn tris(ed: &Editor, id: u32) -> usize {
    ed.scene
      .find(id)
      .unwrap()
      .mesh
      .as_ref()
      .unwrap()
      .tri_count()
  }

  #[test]
  fn points_moved_faces_extruded_undone_redone() {
    let (mut ed, id) = editor_with_box();
    assert!(
      ed.get_state()["selection"]["topology_editable"]
        .as_bool()
        .unwrap()
    );
    assert_eq!(call(&mut ed, "start_topology", json!({})), json!(true));
    assert_eq!(ed.tool, ToolMode::Topology);
    let v = call(&mut ed, "topology_view", json!({"points": true}));
    assert_eq!(
      v["count"], 8,
      "the box's corners: the vertices at the same place, one point"
    );
    assert_eq!(v["points"].as_array().unwrap().len(), 24);
    // a box over the bottom points (identity view: x, y in -1..1 on a 2 x 2 view)
    let v = call(
      &mut ed,
      "topology_box",
      json!({"view_proj": Mat4::identity(), "width": 2.0, "height": 2.0,
             "a": [0.0, 0.5], "b": [2.0, 1.5]}),
    );
    assert_eq!(v["selected_points"].as_array().unwrap().len(), 4);
    assert!(v["center"][1].as_f64().unwrap().abs() < 1e-5);
    // the gizmo dragged up 1 m: shown as it goes, one undo step when let go
    for dy in [0.5, 1.0] {
      call(
        &mut ed,
        "topology_drag",
        json!({"frame": Mat4::identity(), "delta": Mat4::translate(Vec3::new(0.0, dy, 0.0))}),
      );
    }
    assert!(!ed.history.can_undo(), "not in the history while dragged");
    assert!(
      (bounds(&ed, id).lo.y - 1.0).abs() < 1e-5,
      "shown as it goes"
    );
    call(&mut ed, "topology_drag_end", json!({"op": "translate"}));
    let b = bounds(&ed, id);
    assert!(
      (b.lo.y - 1.0).abs() < 1e-5 && (b.hi.y - 2.0).abs() < 1e-5,
      "{b:?}"
    );
    assert_eq!(ed.history.labels().len(), 1);
    assert_eq!(
      ed.history.undo_label(),
      crate::i18n::tr("undo.topology.move")
    );
    call(&mut ed, "undo", json!({}));
    assert!(bounds(&ed, id).lo.y.abs() < 1e-5);
    // undone: what was picked forgotten
    let v = call(&mut ed, "topology_view", json!({}));
    assert!(v["selected_points"].as_array().unwrap().is_empty());
    call(&mut ed, "redo", json!({}));
    assert!((bounds(&ed, id).lo.y - 1.0).abs() < 1e-5);
    // scaled along the gizmo's axes around the middle of what is picked
    call(&mut ed, "topology_select", json!({"all": true}));
    call(
      &mut ed,
      "topology_drag",
      json!({"frame": Mat4::identity(), "scale": [2.0, 1.0, 1.0]}),
    );
    call(&mut ed, "topology_drag_end", json!({"op": "scale"}));
    let b = bounds(&ed, id);
    assert!(
      (b.hi.x - 2.0).abs() < 1e-5 && (b.lo.x + 2.0).abs() < 1e-5,
      "{b:?}"
    );
    assert_eq!(
      ed.history.undo_label(),
      crate::i18n::tr("undo.topology.scale")
    );
    // faces: the top's pulled out (walls join it to the rest; the test box's faces look inward:
    // along their normal is down), then subdivided, flipped,
    // deleted; each undone
    let before = tris(&ed, id);
    call(
      &mut ed,
      "topology_select",
      json!({"faces": true, "all": false}),
    );
    let v = call(
      &mut ed,
      "topology_box",
      json!({"view_proj": Mat4::identity(), "width": 2.0, "height": 2.0,
             "a": [0.0, -1.2], "b": [2.0, -0.8]}),
    );
    assert_eq!(
      v["selected_faces"].as_array().unwrap().len(),
      4,
      "the top's two triangles"
    );
    let v = call(&mut ed, "topology_extrude", json!({"distance": -0.5}));
    assert!(tris(&ed, id) == before + 8 && v["faces"].as_bool().unwrap());
    assert!((bounds(&ed, id).hi.y - 2.5).abs() < 1e-4);
    call(&mut ed, "topology_subdivide", json!({}));
    call(&mut ed, "topology_flip", json!({}));
    call(&mut ed, "topology_delete", json!({}));
    assert!(tris(&ed, id) < before + 8);
    assert_eq!(ed.history.labels().len(), 6);
    for _ in 0..4 {
      call(&mut ed, "undo", json!({}));
    }
    assert_eq!(tris(&ed, id), before);
    // the element gone: the tool ends
    ed.scene.remove(&[id]);
    assert!(call(&mut ed, "topology_view", json!({})).is_null());
    assert_eq!(ed.tool, ToolMode::Select);
  }

  #[test]
  fn a_click_picks_a_point_or_a_face() {
    let (mut ed, _) = editor_with_box();
    call(&mut ed, "start_topology", json!({}));
    // the point projected nearest the cursor (identity view: (1, y, z) at x = 2 of the view)
    let pick = |ed: &mut Editor, add: bool, faces: bool| {
      ed.call("topology_select", &json!({"faces": faces}))
        .unwrap();
      ed.call(
        "topology_pick",
        &json!({"origin": [0.5, 10.0, 0.5], "dir": [0.0, -1.0, 0.0], "view_proj": Mat4::identity(),
                "width": 2.0, "height": 2.0, "mouse": [2.0, 1.0], "add": add}),
      )
      .unwrap()
    };
    let v = pick(&mut ed, false, false);
    assert_eq!(v["selected_points"].as_array().unwrap().len(), 1);
    // Shift on it again: taken out
    let v = pick(&mut ed, true, false);
    assert!(v["selected_points"].as_array().unwrap().is_empty());
    // faces: the top's triangle under the ray
    let v = pick(&mut ed, false, true);
    assert_eq!(v["selected_faces"].as_array().unwrap().len(), 2);
    assert_eq!(ed.get_state()["topology"]["picked"], 1);
  }
}
