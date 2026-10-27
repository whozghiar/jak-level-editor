//! The selection and what is done to it (src/app/actions.cpp, app.cpp): select, copy, paste,
//! delete, the moves the viewport's gizmo ends with, the inspector's edits, picking and the box
//! of the select tool.

use serde_json::{Value, json};

use super::Editor;
use super::levels::{copy_nodes, paste_nodes};
use crate::history::{HistoryEntry, NodeChange};
use crate::i18n::tr;
use crate::math::{AABB, Mat4, Quat, Ray, Transform, Vec3, Vec4, normalize};
use crate::scene::pick::pick;
use crate::scene::relight::{K_LIGHT_MAX_VERTICES, light_like_surroundings, lit_here_nodes};
use crate::scene::{K_MARKER_RADIUS, Node, NodeKind, focus_box};
use crate::{log_info, strf};

/// How a box (or a list) of the viewport changes the selection
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
  Replace,
  /// Shift
  Add,
  /// Ctrl: taken out
  Remove,
  /// Shift or Ctrl and a click: in, else out
  Toggle,
}

impl SelectMode {
  pub fn from_name(s: &str) -> SelectMode {
    match s {
      "add" => SelectMode::Add,
      "remove" => SelectMode::Remove,
      "toggle" => SelectMode::Toggle,
      _ => SelectMode::Replace,
    }
  }
}

impl Editor {
  pub fn select_only(&mut self, id: u32) {
    self.selection.clear();
    if id != 0 && self.scene.find(id).is_some() {
      self.selection.insert(id);
      self.primary = id;
    } else {
      self.primary = 0;
    }
  }

  pub fn toggle_select(&mut self, id: u32) {
    if id == 0 {
      return;
    }
    if self.selection.remove(&id) {
      self.fix_primary();
    } else {
      self.selection.insert(id);
      self.primary = id;
    }
  }

  /// Several elements at once (the box of the select tool, the Groups panel): the last one primary.
  pub fn select_ids(&mut self, ids: &[u32], mode: SelectMode) {
    match mode {
      SelectMode::Remove => {
        for id in ids {
          self.selection.remove(id);
        }
        self.fix_primary();
        return;
      }
      SelectMode::Replace => self.selection.clear(),
      SelectMode::Add => {}
      SelectMode::Toggle => {
        for &id in ids {
          self.toggle_select(id);
        }
        return;
      }
    }
    let ids: Vec<u32> = ids
      .iter()
      .copied()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    self.selection.extend(&ids);
    if let Some(&last) = ids.last() {
      self.primary = last;
    }
    self.fix_primary();
  }

  /// the primary element is selected, else the first one is
  pub(crate) fn fix_primary(&mut self) {
    if !self.selection.contains(&self.primary) {
      self.primary = self.selection.first().copied().unwrap_or(0);
    }
  }

  /// selected nodes without selected ancestors
  pub fn selection_roots(&self) -> Vec<u32> {
    // each selected node, unless one of its ancestors is selected too: a walk up from each, not a
    // test of every pair (thousands of elements are selected with a box)
    let c = self.scene.cache();
    let mut out = Vec::with_capacity(self.selection.len());
    for &id in &self.selection {
      let Some(i) = self.scene.index_of(id) else {
        continue;
      };
      let mut under_selected = false;
      if i < c.count && c.parent.len() == c.count {
        let mut p = c.parent[i];
        let mut guard = 0;
        while p >= 0 && !under_selected && guard < 1000 {
          under_selected = self.selection.contains(&self.scene.nodes[p as usize].id);
          p = c.parent[p as usize];
          guard += 1;
        }
      } else {
        let mut n = &self.scene.nodes[i];
        while n.parent != 0 && !under_selected {
          under_selected = self.selection.contains(&n.parent);
          match self.scene.find(n.parent) {
            Some(p) => n = p,
            None => break,
          }
        }
      }
      if !under_selected {
        out.push(id);
      }
    }
    out
  }

  /// the world box of a node and its descendants (from the scene's cache), how many descendants
  pub fn subtree_bounds(&self, id: u32) -> (AABB, usize) {
    // down the cache's tree (which node is under which: no walk of the whole scene per element),
    // with the matrices as they are now (a change made since counts)
    let c = self.scene.cache();
    let mut b = AABB::default();
    let Some(i) = self.scene.index_of(id).filter(|&i| i < c.count) else {
      return (b, 0);
    };
    let mut stack = vec![(i, self.scene.world(&self.scene.nodes[i]))];
    let mut n = 0;
    while let Some((index, world)) = stack.pop() {
      n += 1;
      let node = &self.scene.nodes[index];
      if let Some(mesh) = &node.mesh {
        b.add_box(&mesh.bounds().transformed(&world));
      } else if node.kind == NodeKind::Actor {
        let p = world.col3(3);
        let r = Vec3::new(K_MARKER_RADIUS, K_MARKER_RADIUS, K_MARKER_RADIUS);
        b.add(p - r);
        b.add(p + r);
      } else {
        b.add(world.col3(3));
      }
      for k in c.kid_start[index]..c.kid_start[index + 1] {
        let kid = c.kids[k as usize] as usize;
        stack.push((kid, world * self.scene.nodes[kid].local.matrix()));
      }
    }
    (b, n - 1)
  }

  /// of the selection and what it holds
  pub fn selection_bounds(&self) -> AABB {
    let mut b = AABB::default();
    for id in self.selection_roots() {
      b.add_box(&self.subtree_bounds(id).0);
    }
    b
  }

  /// the middle of most of a large group's geometry (a whole level), in the world
  // ponytail: found again each time (the C++ kept it per group until the document changed)
  fn group_center(&self, id: u32) -> Vec3 {
    let c = self.scene.cache();
    let mut boxes = Vec::new();
    if let Some(i) = self.scene.index_of(id).filter(|&i| i < c.count) {
      let mut nodes = Vec::new();
      self.scene.subtree(i, &mut nodes);
      boxes.extend(
        nodes[1..]
          .iter()
          .map(|&k| c.bounds[k as usize])
          .filter(AABB::valid),
      );
    }
    let focus = focus_box(&boxes);
    if focus.valid() {
      focus.center()
    } else {
      self.scene.world_of(id).col3(3)
    }
  }

  /// gizmo pivot and orientation: the middle of the selection, turned as its primary element
  pub fn selection_frame(&self) -> Mat4 {
    let roots = self.selection_roots();
    if roots.is_empty() {
      return Mat4::identity();
    }
    let mut p = Vec3::default();
    for &id in &roots {
      let origin = self.scene.world_of(id).col3(3);
      let (b, below) = self.subtree_bounds(id);
      if below > 64 {
        // a large group (a whole level): the middle of most of it, a stray piece far away left out
        p += self.group_center(id);
        continue;
      }
      // the node's origin, unless it is far from its geometry (meshes in world space have their
      // origin at 0,0,0): then the center of its box
      p += if b.valid() && !b.expanded(1.0).contains(origin) {
        b.center()
      } else {
        origin
      };
    }
    p = p * (1.0 / roots.len() as f32);
    let r = if self.selection.contains(&self.primary) {
      self.primary
    } else {
      roots[0]
    };
    let t = Transform::from_matrix(&self.scene.world_of(r));
    Mat4::translate(p) * Mat4::rotate(t.r)
  }

  /// what a click on `node` selects: the actor of an actor's model, nothing on the locked ground
  /// (unless select_terrain)
  pub fn selectable(&self, node: u32) -> u32 {
    let Some(n) = self.scene.find(node) else {
      return 0;
    };
    // the ground (locked) hides what is behind it but is not chosen, unless asked
    if n.locked && !self.view.select_terrain {
      return 0;
    }
    // an actor's model selects the actor
    match self.scene.actor_of(n) {
      0 => n.id,
      actor => actor,
    }
  }

  /// What a box of the view selects (app.cpp box_nodes): the elements whose middle the box holds,
  /// projected with the viewport's view-projection matrix (column-major) on a view of `size` CSS
  /// pixels; the box's corners `a`, `b` in those pixels (origin top-left).
  pub fn box_nodes(&self, view_proj: &Mat4, size: [f32; 2], a: [f32; 2], b: [f32; 2]) -> Vec<u32> {
    let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
    let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
    let cache = self.scene.cache();
    let mut out = Vec::new();
    for (i, n) in self.scene.nodes.iter().enumerate().take(cache.count) {
      let kind_ok = n.kind == NodeKind::Render
        || n.kind == NodeKind::Actor
        || (n.kind == NodeKind::NavMesh && self.view.navmesh);
      if !kind_ok
        || (n.locked && !self.view.select_terrain)
        || !cache.visible[i]
        || cache.reference[i]
      {
        continue;
      }
      if cache.actor[i] != 0 {
        continue; // an actor's model: the actor is selected instead
      }
      if n.kind == NodeKind::Actor && !self.view.actors {
        continue;
      }
      let center = if cache.bounds[i].valid() {
        cache.bounds[i].center()
      } else {
        cache.worlds[i].col3(3)
      };
      let c = *view_proj * Vec4::from_vec3(center, 1.0);
      if c.w <= 1e-5 {
        continue;
      }
      let sx = (c.x / c.w * 0.5 + 0.5) * size[0];
      let sy = (1.0 - (c.y / c.w * 0.5 + 0.5)) * size[1];
      if sx >= x0 && sx <= x1 && sy >= y0 && sy <= y1 {
        out.push(n.id);
      }
    }
    out
  }

  /// The first surface the ray meets (the select tool's filter, `args` may change it): `{node,
  /// point, normal, distance, selectable}`, the node being the actor rather than its model,
  /// `selectable` what a click selects (0: the locked ground).
  pub fn pick_ray(&self, ray: &Ray, args: &Value) -> Value {
    let f = self.pick_filter(args);
    let ray = Ray::new(ray.o, normalize(ray.d));
    let h = pick(&self.scene, &ray, &f);
    if !h.hit() {
      return Value::Null;
    }
    let node = self
      .scene
      .find(h.node)
      .map_or(h.node, |n| match self.scene.actor_of(n) {
        0 => h.node,
        a => a,
      });
    json!({
      "node": node,
      "point": h.point,
      "normal": h.normal,
      "distance": h.t,
      "selectable": self.selectable(h.node),
    })
  }

  pub fn delete_selection(&mut self) {
    let roots = self.selection_roots();
    if roots.is_empty() {
      return;
    }
    let mut c = NodeChange::new(&self.scene);
    c.save_subtrees(&self.scene, &roots);
    self.scene.remove(&roots);
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.delete"), roots.len()), None));
    self.selection.clear();
    self.primary = 0;
  }

  pub fn copy_selection(&mut self) {
    let roots = self.selection_roots();
    if roots.is_empty() {
      return;
    }
    self.clipboard = copy_nodes(&self.scene, &roots);
    log_info!(tr("log.copied"), roots.len());
  }

  pub fn paste(&mut self) {
    if self.clipboard.is_empty() {
      return;
    }
    let c = NodeChange::new(&self.scene);
    let roots = paste_nodes(&mut self.scene, &self.clipboard);
    if roots.is_empty() {
      return;
    }
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.paste"), roots.len()), None));
    self.selection = roots.iter().copied().collect();
    self.primary = roots[0];
  }

  pub fn duplicate_selection(&mut self) {
    let roots = self.selection_roots();
    if roots.is_empty() {
      return;
    }
    let c = NodeChange::new(&self.scene);
    let clip = copy_nodes(&self.scene, &roots);
    let copies = paste_nodes(&mut self.scene, &clip);
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.duplicate"), copies.len()), None));
    self.selection = copies.iter().copied().collect();
    self.primary = copies.first().copied().unwrap_or(0);
  }

  /// the elements under `roots` lit where they stood (placed, pasted), lit again where they are
  /// now, within `change` (saved before they change)
  pub fn relight_moved(&mut self, change: &mut NodeChange, roots: &[u32]) {
    let lit = lit_here_nodes(&self.scene, roots);
    if lit.is_empty() {
      return;
    }
    for &id in &lit {
      change.save(&self.scene, id);
    }
    light_like_surroundings(&mut self.scene, &lit, K_LIGHT_MAX_VERTICES);
  }

  /// The end of a move of the viewport (the gizmo, the selection dragged, the free rotation): each
  /// of `ids` at its world matrix, lit like where it goes, one undo step (`label`: undo.move,
  /// undo.rotate, undo.scale).
  pub fn transform_nodes(&mut self, ids: &[u32], worlds: &[Mat4], label: &str) {
    // a whole structure scaled: made again at its new size with more of its pieces, not stretched
    if label == "undo.scale" && self.scale_structure(ids, worlds) {
      return;
    }
    let mut change = NodeChange::new(&self.scene);
    let mut moved = Vec::new();
    for (&id, w) in ids.iter().zip(worlds) {
      if self.scene.find(id).is_none() {
        continue;
      }
      change.save(&self.scene, id);
      self.scene.set_world(id, w);
      moved.push(id);
    }
    if moved.is_empty() {
      return;
    }
    // what was placed is lit like where it goes
    self.relight_moved(&mut change, &moved);
    self.scene.begin_frame();
    self
      .history
      .push(change.commit(&self.scene, tr(label), None));
  }

  /// An undoable property edit (the inspector): the node as it was before, as it is now.
  pub fn commit_node_edit(&mut self, before: Node, label: &str) {
    let Some(after) = self.scene.find(before.id).cloned() else {
      return;
    };
    let id = before.id;
    let set = |n: Node| {
      move |s: &mut crate::scene::Scene| {
        if let Some(x) = s.find_mut(id) {
          *x = n.clone();
        }
      }
    };
    self
      .history
      .push(HistoryEntry::new(tr(label), set(before), set(after)));
    self.scene.begin_frame();
  }

  /// A field of the inspector starts changing (a drag): the node is kept as it was.
  pub fn edit_begin(&mut self, id: u32) {
    if self.edit_before.as_ref().is_none_or(|n| n.id != id) {
      self.edit_before = self.scene.find(id).cloned();
    }
  }

  /// ... and is let go: one undo step from where it began.
  pub fn edit_end(&mut self, label: &str) {
    if let Some(before) = self.edit_before.take() {
      self.commit_node_edit(before, label);
    }
  }

  /// The inspector's edit of a node: `{id, name?, position?, rotation? (euler degrees) | quat?,
  /// scale?, world? (16 floats), visible?, locked?, extras? (merged: null removes a key), label?
  /// (its undo key), live?}`. `live`: the field is being dragged, shown but not undoable yet (the
  /// next edit without it makes one undo step from before the drag).
  pub fn set_node(&mut self, args: &Value) -> Result<(), String> {
    let id = args.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    let Some(before) = self.scene.find(id).cloned() else {
      return Err(format!("no node {id}"));
    };
    let vec3 = |k: &str| -> Option<Vec3> { serde_json::from_value(args.get(k)?.clone()).ok() };
    let mut label = "undo.param";
    let mut n = before.clone();
    if let Some(name) = args.get("name").and_then(Value::as_str) {
      n.name = name.to_string();
      label = "undo.rename";
    }
    if let Some(t) = vec3("position") {
      n.local.t = t;
      label = "undo.move";
    }
    if let Some(e) = vec3("rotation") {
      n.local.r = Quat::from_euler_deg(e);
      label = "undo.rotate";
    }
    if let Some(q) = args
      .get("quat")
      .and_then(|q| serde_json::from_value::<Quat>(q.clone()).ok())
    {
      n.local.r = q.normalized();
      label = "undo.rotate";
    }
    if let Some(s) = vec3("scale") {
      n.local.s = Vec3::new(s.x.max(0.01), s.y.max(0.01), s.z.max(0.01));
      label = "undo.scale";
    }
    if let Some(v) = args.get("visible").and_then(Value::as_bool) {
      n.visible = v;
      label = "undo.visibility";
    }
    if let Some(l) = args.get("locked").and_then(Value::as_bool) {
      n.locked = l;
      label = "undo.lock";
    }
    if let Some(extras) = args.get("extras").and_then(Value::as_object) {
      if !n.extras.is_object() {
        n.extras = json!({});
      }
      let o = n.extras.as_object_mut().unwrap();
      for (k, v) in extras {
        if v.is_null() {
          o.remove(k);
        } else {
          o.insert(k.clone(), v.clone());
        }
      }
    }
    let label = args
      .get("label")
      .and_then(Value::as_str)
      .unwrap_or(label)
      .to_string();
    let live = args.get("live").and_then(Value::as_bool).unwrap_or(false);
    if live {
      self.edit_begin(id);
    }
    let world = args
      .get("world")
      .and_then(|w| serde_json::from_value::<Mat4>(w.clone()).ok());
    *self.scene.find_mut(id).unwrap() = n;
    if let Some(w) = world {
      self.scene.set_world(id, &w);
    }
    self.scene.begin_frame();
    if live {
      self.scene_changed();
      return Ok(());
    }
    let before = match self.edit_before.take() {
      Some(b) if b.id == id => b,
      _ => before,
    };
    self.commit_node_edit(before, &label);
    if args.get("visible").is_some() {
      // a level's group shown or hidden: the ocean follows
      self.update_ocean();
    }
    Ok(())
  }
}
