//! Putting the selection in order and repeating it (Edit > Arrange, Edit > Repeat the
//! selection...), the undo history listed (Edit > History...), and what makes a level playable
//! placed in the view (the Build panel's Gameplay: where Jak starts, the checkpoints he continues
//! from, the zones he falls to his death in; the export writes them: io/game_export).

use std::f32::consts::PI;
use std::sync::Arc;

use serde_json::{Value, json};

use super::Editor;
use super::levels::{copy_nodes, paste_nodes};
use super::placement::{Placement, PlacementKind};
use crate::history::NodeChange;
use crate::i18n::tr;
use crate::math::{AABB, Mat4, Quat, Ray, Transform, Vec3, Vec4};
use crate::scene::pick::{PickFilter, pick};
use crate::scene::shapes::{ShapeParams, make_shape};
use crate::scene::{Mesh, NodeKind, Scene};
use crate::strf;

/// the actor types of the gameplay markers (not the game's: the export makes them what they do)
pub const K_CHECKPOINT: &str = "checkpoint";
pub const K_START: &str = "jak-start";
/// the Place tool adding points to the route of an actor (its id in the placement's path)
pub const K_ROUTE_POINT: &str = "route-point";
/// a creature's spawn point: the Place tool's marker (the creature's type in the placement's path);
/// its actor is the creature's type, with og_spawn: the editor shows its flag, never the creature
pub const K_CREATURE: &str = "creature-spawn";
pub const K_SPAWN: &str = "og_spawn";
/// the creatures a spawn point offers: enemies born in the game as entities of a level of their own
/// (checked in the game; a spydroid is never born so, only by the city's squads, and a marauder
/// born so runs out of stack: the game dies)
pub const K_CREATURES: [&str; 4] = ["grunt", "kg-grunt", "flitter", "roboguard"];

/// How the selection is repeated (Edit > Repeat the selection)
#[derive(Clone, Debug, PartialEq)]
pub struct RepeatOptions {
  /// "row", "circle" or "scatter"
  pub mode: String,
  /// copies (not counting the selection)
  pub count: i32,
  /// row: "x" or "z", the way it goes
  pub axis: String,
  /// row: meters from one to the next (0: as wide as the selection, touching)
  pub spacing: f32,
  /// circle: meters from its middle; scatter: how far around
  pub radius: f32,
  /// scatter: how much their size varies (0 to 0.5) and other draws
  pub vary: f32,
  pub seed: u32,
}

impl RepeatOptions {
  pub fn from_json(j: &Value) -> Self {
    let num = |k: &str, d: f32| j.get(k).and_then(Value::as_f64).map_or(d, |v| v as f32);
    let s = |k: &str, d: &str| j.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
    Self {
      mode: s("mode", "row"),
      count: (num("count", 4.0) as i32).clamp(1, 200),
      axis: s("axis", "x"),
      spacing: num("spacing", 0.0).clamp(0.0, 1000.0),
      radius: num("radius", 10.0).clamp(0.5, 1000.0),
      vary: num("vary", 0.2).clamp(0.0, 0.5),
      seed: j.get("seed").and_then(Value::as_u64).unwrap_or(1) as u32,
    }
  }
}

/// A repeat being shown: what is repeated, the document before it, its copies so far
pub(crate) struct Repeating {
  source: Vec<u32>,
  change: NodeChange,
  copies: Vec<u32>,
}

/// game units per meter
const K_UNITS: f32 = 4096.0;

/// the same numbers for a seed: 0..1
fn draw(seed: u32, k: u32) -> f32 {
  let mut x = seed
    .wrapping_mul(0x9E37_79B9)
    .wrapping_add(k.wrapping_mul(0x85EB_CA6B));
  x ^= x >> 15;
  x = x.wrapping_mul(0x2C1B_3C6D);
  x ^= x >> 12;
  (x >> 8) as f32 / (1u32 << 24) as f32
}

/// the box of these roots and all under them, in the world
fn roots_box(scene: &Scene, roots: &[u32]) -> AABB {
  let cache = scene.cache();
  let mut b = AABB::default();
  let below = scene.descendants_of(roots);
  for (r, &root) in roots.iter().enumerate() {
    for id in std::iter::once(root).chain(below[r].iter().copied()) {
      if let Some(i) = scene.index_of(id)
        && i < cache.bounds.len()
        && cache.bounds[i].valid()
      {
        b.add_box(&cache.bounds[i]);
      } else if let Some(i) = scene.index_of(id) {
        b.add(cache.worlds[i].col3(3));
      }
    }
  }
  b
}

/// The ground under a point (the first surface below it, but `skip` and what is under them)
fn ground_below(scene: &Scene, p: Vec3, skip: &[u32]) -> Option<f32> {
  let f = PickFilter {
    actors: false,
    include_locked: true,
    exclude_list: skip.to_vec(),
    ..Default::default()
  };
  let hit = pick(scene, &Ray::new(p, Vec3::new(0.0, -1.0, 0.0)), &f);
  hit.hit().then_some(hit.point.y)
}

/// A flag on a pole, the color of what it marks: in the editor only (the export never draws it)
pub fn marker_mesh(kind: &str) -> Arc<Mesh> {
  let color = if kind == K_START {
    Vec4::new(0.15, 0.45, 1.0, 1.0)
  } else if kind == K_ROUTE_POINT {
    Vec4::new(1.0, 0.85, 0.15, 1.0)
  } else if kind == K_CREATURE {
    Vec4::new(1.0, 0.35, 0.1, 1.0)
  } else {
    Vec4::new(0.2, 0.9, 0.3, 1.0)
  };
  let mut mesh = Mesh::new();
  mesh.name = format!("marker-{kind}");
  let mut add = |params: ShapeParams, at: Vec3, color: Vec4| {
    let shape = make_shape(&params, -1);
    let mut p = shape.prims[0].clone();
    for v in &mut p.pos {
      *v += at;
    }
    p.col = vec![color; p.pos.len()];
    mesh.prims.push(p);
  };
  let grey = Vec4::new(0.35, 0.35, 0.35, 1.0);
  add(
    ShapeParams {
      kind: "cylinder".into(),
      size: Vec3::new(0.12, 2.4, 0.12),
      sides: 8,
      ..Default::default()
    },
    Vec3::default(),
    grey,
  );
  add(
    ShapeParams {
      kind: "box".into(),
      size: Vec3::new(1.0, 0.6, 0.05),
      ..Default::default()
    },
    Vec3::new(0.56, 1.75, 0.0),
    color,
  );
  // its base: a disc on the ground
  add(
    ShapeParams {
      kind: "cylinder".into(),
      size: Vec3::new(1.6, 0.05, 1.6),
      sides: 16,
      ..Default::default()
    },
    Vec3::default(),
    Vec4::new(color.x * 0.6, color.y * 0.6, color.z * 0.6, 1.0),
  );
  Arc::new(mesh)
}

impl Editor {
  // ----------------------------------------------------------------------------------------------
  // arrange
  // ----------------------------------------------------------------------------------------------

  /// The selection put in order, one undo step: "ground" each dropped onto what is under it,
  /// "height" all as high as the first selected, "line" in a straight line from the first to the
  /// farthest, evenly spaced, "face" all turned the way the first is. How many moved.
  pub fn arrange_selection(&mut self, how: &str) -> usize {
    let mut roots = self.selection_roots();
    if roots.is_empty() {
      return 0;
    }
    // the first selected first
    if let Some(i) = roots.iter().position(|&r| r == self.primary) {
      roots.swap(0, i);
    }
    self.scene.begin_frame();
    let worlds: Vec<Mat4> = roots.iter().map(|&r| self.scene.world_of(r)).collect();
    let boxes: Vec<AABB> = roots
      .iter()
      .map(|&r| roots_box(&self.scene, &[r]))
      .collect();
    let mut out = worlds.clone();
    match how {
      "ground" => {
        for (k, &r) in roots.iter().enumerate() {
          let b = boxes[k];
          if !b.valid() {
            continue;
          }
          let c = b.center();
          let top = Vec3::new(c.x, b.lo.y + 0.05, c.z);
          if let Some(y) = ground_below(&self.scene, top, &[r]) {
            out[k] = Mat4::translate(Vec3::new(0.0, y - b.lo.y, 0.0)) * worlds[k];
          }
        }
      }
      "height" => {
        let y0 = boxes[0].lo.y;
        for k in 1..roots.len() {
          if boxes[k].valid() {
            out[k] = Mat4::translate(Vec3::new(0.0, y0 - boxes[k].lo.y, 0.0)) * worlds[k];
          }
        }
      }
      "line" => {
        if roots.len() < 3 {
          return 0;
        }
        // from the first to the one farthest from it, the others in their order along the way
        let at = |k: usize| worlds[k].col3(3);
        let a = at(0);
        let far = (1..roots.len())
          .max_by(|&i, &j| {
            crate::math::length2(at(i) - a).total_cmp(&crate::math::length2(at(j) - a))
          })
          .unwrap_or(1);
        let d = at(far) - a;
        let mut order: Vec<usize> = (1..roots.len()).collect();
        order.sort_by(|&i, &j| {
          crate::math::dot(at(i) - a, d).total_cmp(&crate::math::dot(at(j) - a, d))
        });
        let n = order.len() as f32;
        for (step, &k) in order.iter().enumerate() {
          let want = a + d * ((step + 1) as f32 / n);
          out[k] = Mat4::translate(want - at(k)) * worlds[k];
        }
      }
      "face" => {
        let r0 = crate::math::Transform::from_matrix(&worlds[0]).r;
        for k in 1..roots.len() {
          let t = crate::math::Transform::from_matrix(&worlds[k]);
          out[k] = Mat4::translate(t.t) * Mat4::rotate(r0) * Mat4::scale(t.s);
        }
      }
      _ => return 0,
    }
    let moved: Vec<(u32, Mat4)> = roots
      .iter()
      .zip(&out)
      .zip(&worlds)
      .filter(|((_, a), b)| *a != *b)
      .map(|((&r, a), _)| (r, *a))
      .collect();
    if moved.is_empty() {
      return 0;
    }
    let (ids, ws): (Vec<u32>, Vec<Mat4>) = moved.into_iter().unzip();
    self.transform_nodes(&ids, &ws, "undo.arrange");
    ids.len()
  }

  // ----------------------------------------------------------------------------------------------
  // repeat: the copies shown as they change, one undo step when kept
  // ----------------------------------------------------------------------------------------------

  /// The copies of the selection made again for `o` (the ones shown before taken away): how many
  pub fn repeat_preview(&mut self, o: &RepeatOptions) -> usize {
    if self.repeating.is_none() {
      let source = self.selection_roots();
      if source.is_empty() {
        return 0;
      }
      self.repeating = Some(Repeating {
        source,
        change: NodeChange::new(&self.scene),
        copies: Vec::new(),
      });
    }
    let r = self.repeating.as_mut().unwrap();
    let old = std::mem::take(&mut r.copies);
    let source: Vec<u32> = r
      .source
      .iter()
      .copied()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    self.scene.remove(&old);
    if source.is_empty() {
      self.repeating = None;
      self.scene_changed();
      return 0;
    }
    self.scene.begin_frame();
    let b = roots_box(&self.scene, &source);
    if !b.valid() {
      return 0;
    }
    let c = b.center();
    let size = b.hi - b.lo;
    let clip = copy_nodes(&self.scene, &source);
    let mut copies = Vec::new();
    for k in 1..=o.count {
      let kf = k as f32;
      // the copy's move in the world, around the selection's middle
      let m = match o.mode.as_str() {
        "circle" => {
          // the selection on the circle, its middle `radius` meters towards -x
          let mid = c - Vec3::new(o.radius, 0.0, 0.0);
          let a = 2.0 * PI * kf / (o.count + 1) as f32;
          Mat4::translate(mid)
            * Mat4::rotate(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), a))
            * Mat4::translate(-mid)
        }
        "scatter" => {
          let a = 2.0 * PI * draw(o.seed, k as u32 * 4);
          let d = o.radius * draw(o.seed, k as u32 * 4 + 1).sqrt().max(0.25);
          let turn = 2.0 * PI * draw(o.seed, k as u32 * 4 + 2);
          let s = 1.0 + o.vary * (2.0 * draw(o.seed, k as u32 * 4 + 3) - 1.0);
          let at = c + Vec3::new(a.cos() * d, 0.0, a.sin() * d);
          // on the ground there (the selection itself left out)
          let ground = ground_below(
            &self.scene,
            at + Vec3::new(0.0, size.y + 20.0, 0.0),
            &source,
          );
          let dy = ground.map_or(0.0, |y| y - b.lo.y);
          let base = Vec3::new(c.x, b.lo.y, c.z);
          Mat4::translate(at - c + Vec3::new(0.0, dy, 0.0))
            * Mat4::translate(base)
            * Mat4::rotate(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), turn))
            * Mat4::scale(Vec3::new(s, s, s))
            * Mat4::translate(-base)
        }
        _ => {
          let x = o.axis != "z";
          let step = if o.spacing > 0.0 {
            o.spacing
          } else if x {
            size.x.max(0.1)
          } else {
            size.z.max(0.1)
          };
          Mat4::translate(if x {
            Vec3::new(step * kf, 0.0, 0.0)
          } else {
            Vec3::new(0.0, 0.0, step * kf)
          })
        }
      };
      let new = paste_nodes(&mut self.scene, &clip);
      for &id in &new {
        let w = self.scene.world_of(id);
        self.scene.set_world(id, &(m * w));
      }
      copies.extend(new);
    }
    self.scene.begin_frame();
    // not a step of the history yet: the view told of it
    self.scene_changed();
    let n = copies.len();
    self.repeating.as_mut().unwrap().copies = copies;
    n
  }

  /// The repeat ended: its copies kept (lit where they are, one undo step, selected with the
  /// selection) or taken away
  pub fn repeat_finish(&mut self, keep: bool) {
    let Some(r) = self.repeating.take() else {
      return;
    };
    if !keep || r.copies.is_empty() {
      self.scene.remove(&r.copies);
      self.scene.begin_frame();
      self.scene_changed();
      return;
    }
    let mut change = r.change;
    self.relight_moved(&mut change, &r.copies);
    self.scene.begin_frame();
    self
      .history
      .push(change.commit(&self.scene, strf!(tr("undo.repeat"), r.copies.len()), None));
    self.selection.extend(r.copies.iter().copied());
  }

  // ----------------------------------------------------------------------------------------------
  // the undo history
  // ----------------------------------------------------------------------------------------------

  /// {steps: [labels, the oldest first], at: how many are done}
  fn history_list(&self) -> Value {
    json!({"steps": self.history.labels(), "at": self.history.cursor()})
  }

  /// Undoes or redoes until `at` steps are done
  pub fn history_goto(&mut self, at: usize) {
    let at = at.min(self.history.labels().len());
    while self.history.cursor() > at && self.history.can_undo() {
      self.undo_redo(true);
    }
    while self.history.cursor() < at && self.history.can_redo() {
      self.undo_redo(false);
    }
  }

  // ----------------------------------------------------------------------------------------------
  // gameplay: where Jak starts, the checkpoints, the death zones
  // ----------------------------------------------------------------------------------------------

  /// The Place tool with a gameplay marker: "start" (where Jak starts), "checkpoint" (where he
  /// continues from once he passed it), "death_zone" (a fall to his death: an invisible box)
  pub fn start_gameplay_placement(&mut self, what: &str, once: bool) -> bool {
    let mut pl = Placement {
      once,
      ..Default::default()
    };
    match what {
      "start" | "checkpoint" => {
        pl.kind = PlacementKind::Marker;
        pl.name = if what == "start" {
          K_START
        } else {
          K_CHECKPOINT
        }
        .into();
      }
      "creature" => {
        // which creature: one of those the game lets a level place, which it has
        let creature = self.placing.creature.clone();
        if !K_CREATURES.contains(&creature.as_str())
          || !self.game_data().is_some_and(|d| d.has_type(&creature))
        {
          return false;
        }
        pl.kind = PlacementKind::Marker;
        pl.name = K_CREATURE.into();
        pl.path = creature;
      }
      "death_zone" => {
        pl.kind = PlacementKind::Shape;
        pl.name = "death-zone".into();
        pl.shape = ShapeParams {
          kind: "box".into(),
          size: Vec3::new(16.0, 2.0, 16.0),
          color: Vec4::new(1.0, 0.15, 0.1, 1.0),
          ..Default::default()
        };
        pl.extras =
          json!({"og_decor": {"collision": {"kind": "invisible", "event": "endlessfall"}}});
      }
      _ => return false,
    }
    self.begin_placement(pl);
    true
  }

  // ----------------------------------------------------------------------------------------------
  // routes: where a moving object goes (a platform, an elevator: its "path", points in the world)
  // ----------------------------------------------------------------------------------------------

  /// The parameter of an actor's type that is its route (a path), if it has one
  fn route_tag(&mut self, id: u32) -> Option<String> {
    let etype = self
      .scene
      .find(id)
      .filter(|n| n.kind == NodeKind::Actor)?
      .extras["etype"]
      .as_str()?
      .to_string();
    let data = self.game_data()?;
    data
      .params(&etype)?
      .iter()
      .find(|p| p.kind == "path")
      .map(|p| p.tag.clone())
  }

  /// How far the actor moved since its route was drawn (its og_origin, as the export moves it)
  fn route_moved(&self, id: u32) -> Mat4 {
    let Some(n) = self.scene.find(id) else {
      return Mat4::identity();
    };
    let Some(og) = n
      .extras
      .get("og_origin")
      .and_then(Value::as_array)
      .filter(|a| a.len() == 7)
    else {
      return Mat4::identity();
    };
    let f = |k: usize| og[k].as_f64().unwrap_or(0.0) as f32;
    let t0 = Transform {
      t: Vec3::new(f(0), f(1), f(2)),
      r: Quat::new(f(3), f(4), f(5), f(6)),
      ..Default::default()
    };
    let mut t1 = Transform::from_matrix(&self.scene.world_of(id));
    t1.s = Vec3::new(1.0, 1.0, 1.0);
    t1.matrix() * t0.matrix().inverse()
  }

  /// `actor_route {id}`: {tag, points: [[x, y, z]] in meters, in the world}, null when its type
  /// has no route
  pub fn actor_route(&mut self, id: u32) -> Value {
    let Some(tag) = self.route_tag(id) else {
      return Value::Null;
    };
    let moved = self.route_moved(id);
    let n = self.scene.find(id).unwrap();
    let points: Vec<Value> = n.extras["lump"][&tag]
      .as_array()
      .into_iter()
      .flatten()
      .filter_map(|p| {
        let a = p.as_array()?;
        let c = |k: usize| a.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let w = moved.point(Vec3::new(c(0), c(1), c(2)) / K_UNITS);
        Some(json!([w.x, w.y, w.z]))
      })
      .collect();
    json!({"tag": tag, "points": points})
  }

  /// The route of actor `id` made `points` (in the world, meters; none: the game's own back),
  /// one undo step
  pub fn set_route(&mut self, id: u32, points: &[Vec3], label: &str) -> Result<(), String> {
    let Some(tag) = self.route_tag(id) else {
      return Err(format!("{id}: no route (its type has no path)"));
    };
    let Some(before) = self.scene.find(id).cloned() else {
      return Err(format!("no actor {id}"));
    };
    // where it is now is where its route is drawn from: it follows it when it moves (og_origin)
    if before.extras.get("og_origin").is_none() {
      let w = Transform::from_matrix(&self.scene.world_of(id));
      let n = self.scene.find_mut(id).unwrap();
      n.extras["og_origin"] = json!([w.t.x, w.t.y, w.t.z, w.r.x, w.r.y, w.r.z, w.r.w]);
    }
    let back = self.route_moved(id).inverse();
    let n = self.scene.find_mut(id).unwrap();
    for key in ["lump", "lump_types"] {
      if !n.extras[key].is_object() {
        n.extras[key] = json!({});
      }
    }
    if points.is_empty() {
      n.extras["lump"].as_object_mut().unwrap().remove(&tag);
      n.extras["lump_types"].as_object_mut().unwrap().remove(&tag);
    } else {
      let stored: Vec<Value> = points
        .iter()
        .map(|&p| {
          let q = back.point(p) * K_UNITS;
          json!([q.x, q.y, q.z, 1.0])
        })
        .collect();
      n.extras["lump"][&tag] = Value::Array(stored);
      n.extras["lump_types"][&tag] = "vector".into();
    }
    self.commit_node_edit(before, label);
    Ok(())
  }

  /// A point at the end of actor `id`'s route (a route starts where the actor is)
  pub fn route_add_point(&mut self, id: u32, p: Vec3) -> Result<usize, String> {
    let mut points: Vec<Vec3> = self.actor_route(id)["points"]
      .as_array()
      .into_iter()
      .flatten()
      .filter_map(|v| serde_json::from_value(v.clone()).ok())
      .collect();
    if points.is_empty() {
      points.push(self.scene.world_of(id).col3(3));
    }
    points.push(p);
    self.set_route(id, &points, "undo.route")?;
    Ok(points.len())
  }

  /// What makes the level playable, to find it again (the Build panel's list): where Jak starts,
  /// the checkpoints, the creatures' spawn points, the death zones, the moving objects with a route;
  /// [{id, kind, name, etype?, position, points?}]
  pub fn gameplay_list(&mut self) -> Value {
    // a creature with no walkable ground under it: the export leaves it out
    let meshes = crate::navmesh::world_navmeshes(&self.scene).0;
    let cache = self.scene.cache();
    let mut out = Vec::new();
    let mut routed = Vec::new();
    for (i, n) in self.scene.nodes.iter().enumerate() {
      let etype = n.extras["etype"].as_str().unwrap_or_default();
      let kind = if n.kind == NodeKind::Actor && etype == K_START {
        "start"
      } else if n.kind == NodeKind::Actor && etype == K_CHECKPOINT {
        "checkpoint"
      } else if n.kind == NodeKind::Actor && n.extras.get(K_SPAWN).is_some() {
        "creature"
      } else if n.kind == NodeKind::Render
        && n.extras["og_decor"]["collision"]["event"] == "endlessfall"
      {
        "death_zone"
      } else {
        if n.kind == NodeKind::Actor && n.extras["lump"].get("path").is_some() {
          routed.push(n.id);
        }
        continue;
      };
      let p = cache.worlds[i].col3(3);
      let stranded = kind == "creature" && crate::navmesh::navmesh_under(&meshes, p).is_none();
      out.push(
        json!({"id": n.id, "kind": kind, "name": n.name, "etype": etype,
        "position": [p.x, p.y, p.z], "stranded": stranded}),
      );
    }
    for id in routed {
      if let Some(n) = self.scene.find(id) {
        let (name, etype) = (n.name.clone(), n.extras["etype"].clone());
        let route = self.actor_route(id);
        if route.is_null() {
          continue;
        }
        let p = self.scene.world_of(id).col3(3);
        out.push(
          json!({"id": id, "kind": "route", "name": name, "etype": etype,
          "position": [p.x, p.y, p.z], "points": route["points"].as_array().map_or(0, Vec::len)}),
        );
      }
    }
    Value::Array(out)
  }

  /// The Place tool adding points to the route of the actor selected (each click one, Esc stops)
  pub fn start_route_placement(&mut self) -> Result<(), String> {
    let id = self.primary;
    if self.route_tag(id).is_none() {
      return Err(tr("route.none").to_string());
    }
    self.begin_placement(Placement {
      kind: PlacementKind::Marker,
      name: K_ROUTE_POINT.into(),
      path: id.to_string(),
      ..Default::default()
    });
    Ok(())
  }

  /// The calls of the arranging, the repeat, the history and the gameplay markers (api.rs)
  pub(crate) fn arrange_call(&mut self, name: &str, a: &Value) -> Option<Result<Value, String>> {
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or_default();
    Some(match name {
      "arrange_selection" => Ok(json!({"moved": self.arrange_selection(s("how"))})),
      "repeat_preview" => Ok(json!({"copies": self.repeat_preview(&RepeatOptions::from_json(a))})),
      "repeat_finish" => {
        self.repeat_finish(a.get("keep").and_then(Value::as_bool).unwrap_or(false));
        Ok(Value::Null)
      }
      "history_list" => Ok(self.history_list()),
      "history_goto" => {
        let at = a.get("at").and_then(Value::as_u64).unwrap_or(0) as usize;
        self.history_goto(at);
        Ok(Value::Null)
      }
      "actor_route" => {
        let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
        Ok(self.actor_route(id))
      }
      "start_route_placement" => self.start_route_placement().map(|_| {
        self.placement_mesh();
        Value::Null
      }),
      "set_route" => {
        let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
        let points: Vec<Vec3> = a
          .get("points")
          .and_then(|p| serde_json::from_value(p.clone()).ok())
          .unwrap_or_default();
        self
          .set_route(id, &points, "undo.route")
          .map(|_| Value::Null)
      }
      "gameplay_list" => Ok(self.gameplay_list()),
      "start_gameplay_placement" => {
        let once = a.get("once").and_then(Value::as_bool).unwrap_or(false);
        if let Some(c) = a.get("creature").and_then(Value::as_str) {
          self.placing.creature = c.into();
        }
        if self.start_gameplay_placement(s("what"), once) {
          self.placement_mesh();
          Ok(Value::Null)
        } else {
          Err(format!("no gameplay element '{}'", s("what")))
        }
      }
      _ => return None,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::editor::Editor;
  use crate::scene::{Node, NodeKind};

  fn editor_with_boxes(n: usize) -> (Editor, Vec<u32>) {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.new_document();
    // a ground, then boxes above it
    let ground = make_shape(
      &ShapeParams {
        kind: "box".into(),
        size: Vec3::new(200.0, 1.0, 200.0),
        ..Default::default()
      },
      -1,
    );
    ed.scene.add(Node {
      name: "ground".into(),
      kind: NodeKind::Render,
      mesh: Some(ground),
      ..Default::default()
    });
    let mut ids = Vec::new();
    for i in 0..n {
      let mesh = make_shape(&ShapeParams::default(), -1);
      let mut node = Node {
        name: format!("box{i}"),
        kind: NodeKind::Render,
        mesh: Some(mesh),
        ..Default::default()
      };
      node.local.t = Vec3::new(i as f32 * 7.0, 3.0 + i as f32, (i * i) as f32);
      ids.push(ed.scene.add(node));
    }
    ed.scene.begin_frame();
    ed.selection = ids.iter().copied().collect();
    ed.primary = ids.first().copied().unwrap_or(0);
    (ed, ids)
  }

  #[test]
  fn the_selection_arranged() {
    let (mut ed, ids) = editor_with_boxes(4);
    let at = |ed: &Editor, id: u32| ed.scene.world_of(id).col3(3);
    // dropped onto the ground (its top at y 1)
    assert_eq!(ed.arrange_selection("ground"), 4);
    assert!(ids.iter().all(|&id| (at(&ed, id).y - 1.0).abs() < 0.01));
    ed.undo_redo(true);
    // all as high as the first; then in a line, evenly spaced
    ed.arrange_selection("height");
    assert!(ids.iter().all(|&id| (at(&ed, id).y - 3.0).abs() < 0.01));
    ed.arrange_selection("line");
    let (a, b, c) = (at(&ed, ids[0]), at(&ed, ids[1]), at(&ed, ids[2]));
    let d = at(&ed, ids[3]);
    assert!(
      crate::math::length((b - a) - (c - b)) < 0.01
        && crate::math::length((c - b) - (d - c)) < 0.01
    );
    // undone step by step through the history list
    let steps = ed.history.labels().len();
    ed.history_goto(0);
    assert!(ed.history.cursor() == 0 && (at(&ed, ids[1]).y - 4.0).abs() < 0.01);
    ed.history_goto(steps);
    assert_eq!(ed.history.cursor(), steps);
  }

  #[test]
  fn the_selection_repeated() {
    let (mut ed, ids) = editor_with_boxes(1);
    let count = |ed: &Editor| ed.scene.nodes.len();
    let before = count(&ed);
    let steps = ed.history.labels().len();
    // a row of 3 along x, then 5: the copies made again, not added
    let mut o = RepeatOptions::from_json(&json!({"mode": "row", "count": 3}));
    assert_eq!(ed.repeat_preview(&o), 3);
    o.count = 5;
    assert_eq!(ed.repeat_preview(&o), 5);
    assert_eq!(count(&ed), before + 5);
    // cancelled: gone, nothing to undo
    ed.repeat_finish(false);
    assert!(count(&ed) == before && ed.history.labels().len() == steps);
    // in a circle, kept: one undo step takes them all away
    ed.selection = [ids[0]].into_iter().collect();
    let o = RepeatOptions::from_json(&json!({"mode": "circle", "count": 7, "radius": 10}));
    ed.repeat_preview(&o);
    ed.repeat_finish(true);
    assert!(count(&ed) == before + 7 && ed.history.labels().len() == steps + 1);
    ed.undo_redo(true);
    assert_eq!(count(&ed), before);
    // scattered: on the ground around it
    let o = RepeatOptions::from_json(&json!({"mode": "scatter", "count": 6, "radius": 20}));
    ed.repeat_preview(&o);
    ed.repeat_finish(true);
    let r = &ed.scene.nodes[ed.scene.nodes.len() - 1];
    let w = ed.scene.world(r);
    assert!((w.col3(3).y - 1.0).abs() < 0.6, "{:?}", w.col3(3));
  }

  #[test]
  fn gameplay_markers_placed() {
    let (mut ed, _) = editor_with_boxes(0);
    assert!(ed.start_gameplay_placement("checkpoint", true));
    let roots = ed.place_at(&Mat4::translate(Vec3::new(5.0, 1.0, 5.0)));
    let n = ed.scene.find(roots[0]).unwrap();
    assert!(n.kind == NodeKind::Actor && n.extras["etype"] == K_CHECKPOINT);
    assert!(ed.scene.children(roots[0]).len() == 1);
    assert!(ed.start_gameplay_placement("death_zone", true));
    let zone = ed.place_at(&Mat4::translate(Vec3::new(0.0, -5.0, 0.0)));
    let z = ed.scene.find(zone[0]).unwrap();
    assert_eq!(z.extras["og_decor"]["collision"]["event"], "endlessfall");
    assert!(!ed.start_gameplay_placement("dragon", true));
  }

  #[test]
  fn a_creature_spawn_point() {
    let (mut ed, _) = editor_with_boxes(0);
    if ed.game_data().is_none() {
      eprintln!("skipped: no game data");
      return;
    }
    // a type the game lacks, or one that kills the game placed by a level: no placing
    for bad in ["dragon", "marauder", "spydroid"] {
      ed.placing.creature = bad.into();
      assert!(!ed.start_gameplay_placement("creature", true), "{bad}");
    }
    // a guard: an actor of its type, shown by its flag, named after it
    ed.placing.creature = "kg-grunt".into();
    assert!(ed.start_gameplay_placement("creature", true));
    let roots = ed.place_at(&Mat4::translate(Vec3::new(3.0, 0.0, 2.0)));
    let n = ed.scene.find(roots[0]).unwrap();
    assert!(n.kind == NodeKind::Actor && n.extras["etype"] == "kg-grunt" && n.name == "kg-grunt-1");
    assert!(n.extras.get(K_SPAWN).is_some() && ed.scene.children(roots[0]).len() == 1);
    // nothing to preview on its flag
    assert_eq!(ed.preview_options_json(roots[0])["section"], false);
    // listed with the level's playable things, stranded while no ground it can walk is under it
    let listed = |ed: &mut Editor| {
      ed.gameplay_list()
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "creature" && e["id"] == roots[0])
        .cloned()
        .unwrap()
    };
    assert_eq!(listed(&mut ed)["stranded"], true);
    let ground = crate::navmesh::NavMeshData {
      origin: Vec3::new(3.0, 0.0, 2.0),
      verts: vec![
        Vec3::new(-5.0, 0.0, -5.0),
        Vec3::new(5.0, 0.0, -5.0),
        Vec3::new(5.0, 0.0, 5.0),
        Vec3::new(-5.0, 0.0, 5.0),
      ],
      polys: vec![crate::navmesh::NavPoly {
        count: 4,
        v: [0, 3, 2, 1],
        ..Default::default()
      }],
    };
    let region = AABB {
      lo: Vec3::new(-2.0, -1.0, -3.0),
      hi: Vec3::new(8.0, 1.0, 7.0),
    };
    crate::navmesh::replace_navmeshes(&mut ed.scene, &region, &[ground]);
    assert_eq!(listed(&mut ed)["stranded"], false);
  }
}

#[cfg(test)]
mod route_tests {
  use super::*;
  use crate::editor::Editor;
  use crate::scene::Node;

  #[test]
  fn a_platform_given_a_route() {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.new_document();
    if ed.game_data().is_none() {
      eprintln!("skipped: no game data");
      return;
    }
    let mut plat = Node {
      name: "plat-1".into(),
      kind: NodeKind::Actor,
      extras: json!({"etype": "plat"}),
      ..Default::default()
    };
    plat.local.t = Vec3::new(10.0, 2.0, 0.0);
    let id = ed.scene.add(plat);
    ed.scene.begin_frame();
    // two clicks: the route goes from where it is to them
    ed.route_add_point(id, Vec3::new(20.0, 2.0, 0.0)).unwrap();
    assert_eq!(
      ed.route_add_point(id, Vec3::new(20.0, 8.0, 5.0)).unwrap(),
      3
    );
    let route = ed.actor_route(id);
    assert_eq!(route["points"][0], json!([10.0, 2.0, 0.0]));
    assert_eq!(route["points"][2], json!([20.0, 8.0, 5.0]));
    // stored in game units, as the game's paths
    let n = ed.scene.find(id).unwrap();
    assert_eq!(
      n.extras["lump"]["path"][1],
      json!([81920.0, 8192.0, 0.0, 1.0])
    );
    assert_eq!(n.extras["lump_types"]["path"], "vector");
    // the platform moved 5 m along z: its route with it
    let w = Mat4::translate(Vec3::new(10.0, 2.0, 5.0));
    ed.transform_nodes(&[id], &[w], "undo.move");
    assert_eq!(ed.actor_route(id)["points"][1], json!([20.0, 2.0, 5.0]));
    // an object without a route has none
    let crate_id = ed.scene.add(Node {
      kind: NodeKind::Actor,
      extras: json!({"etype": "crate"}),
      ..Default::default()
    });
    assert!(ed.actor_route(crate_id).is_null());
    assert!(ed.route_add_point(crate_id, Vec3::default()).is_err());
  }
}
