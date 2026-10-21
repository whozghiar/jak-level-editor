//! The inspector's sections of an actor (behavior_panel.cpp): its behavior, set in simple terms
//! for the objects whose behavior is the game's code (doors, bouncers, elevators, whether an object
//! is there at all: io/behaviors.rs), and its parameters, every res lump the code of its type reads
//! (game data: params), typed; and what the game does with an element of the decor (its collision,
//! bounce, launch: io/decor_behaviors.rs).

use serde_json::{Map, Value, json};

use super::Editor;
use crate::io::behaviors::behavior_kind;
use crate::io::decor_behaviors::{DecorBehavior, decor_behavior_json, decor_behavior_of};
use crate::io::game_export::{DefaultCollision, default_decor_collision};
use crate::scene::{Node, NodeKind, Scene};

/// game units per meter
const K_UNITS: f64 = 4096.0;

/// the parameters given in meters: distances, heights, sizes
pub fn in_meters(tag: &str) -> bool {
  [
    "distance",
    "height",
    "radius",
    "dist",
    "width",
    "range",
    "threshold",
    "depth",
    "offset",
  ]
  .iter()
  .any(|w| tag.contains(w))
}

/// a lump value as numbers (a number, or an array of them)
fn numbers_of(v: &Value) -> Vec<f64> {
  match v {
    Value::Array(a) => a.iter().filter_map(Value::as_f64).collect(),
    v => v.as_f64().into_iter().collect(),
  }
}

fn etype_of(n: &Node) -> &str {
  n.extras
    .get("etype")
    .and_then(Value::as_str)
    .unwrap_or_default()
}

/// og_behavior as the inspector reads it: a value of another kind than the inspector writes (a
/// project edited by hand) reads as unset
fn behavior_of(n: &Node) -> Map<String, Value> {
  let mut b = n
    .extras
    .get("og_behavior")
    .and_then(Value::as_object)
    .cloned()
    .unwrap_or_default();
  for k in ["open", "loads"] {
    if b.get(k).is_some_and(|v| !v.is_string()) {
      b.remove(k);
    }
  }
  if b.get("distance").is_some_and(|v| !v.is_number()) {
    b.remove("distance");
  }
  b
}

/// the lump type the export writes for a parameter of that kind (None: not set here: paths,
/// scripts, groups of actors)
fn lump_type(kind: &str) -> Option<&'static str> {
  Some(match kind {
    "float" | "floats" => "float",
    "int" | "ints" | "eco-info" => "int32",
    "vector" => "vector",
    "symbol" => "symbol",
    "string" => "string",
    "actor" => "actor-name",
    k if k.starts_with("enum:") => "uint32",
    _ => return None,
  })
}

/// a piece of decor: a mesh of the document that is not an actor's model
fn decor_node(scene: &Scene, id: u32) -> bool {
  scene
    .find(id)
    .is_some_and(|n| n.kind == NodeKind::Render && n.mesh.is_some() && scene.actor_of(n) == 0)
}

/// every field of a decor behavior, the parts not set with their defaults (the inspector shows them)
fn decor_fields(b: &DecorBehavior) -> Value {
  let (c, l) = (&b.collision, &b.launch);
  json!({
    "collision": {
      "set": c.set, "kind": c.kind, "material": c.material, "event": c.event, "mode": c.mode,
      "noedge": c.noedge, "nocamera": c.nocamera, "noentity": c.noentity,
      "nolineofsight": c.nolineofsight,
    },
    "bounce": {"set": b.bounce.set, "height": b.bounce.height},
    "launch": {"set": l.set, "distance": l.distance, "height": l.height, "yaw": l.yaw, "time": l.time},
  })
}

impl Editor {
  /// `actor_behavior {id}`: what the behavior and parameters sections show of an actor (null: not
  /// an actor, no game data): {etype, kind: door|bouncer|elevator|other, behavior (og_behavior),
  /// bouncer_height, elevator_speed (meters, m/s), crate_type (null: not a crate), params: [{tag,
  /// kind, value (its lump, else the game's), set, meters, enum: {bitfield, values: [[name,
  /// value]]} | null}], tasks (the story's task nodes), actors: [{id, name, aid}]}
  pub fn actor_behavior(&mut self, id: u32) -> Option<Value> {
    let data = self.game_data()?;
    let n = self.scene.find(id).filter(|n| n.kind == NodeKind::Actor)?;
    let etype = etype_of(n);
    if etype.is_empty() {
      return None;
    }
    let params = data.params(etype);
    let lump = n.extras.get("lump").filter(|l| l.is_object());
    let lump_float = |tag: &str, fallback: f64| {
      lump
        .and_then(|l| l.get(tag))
        .and_then(|v| numbers_of(v).first().copied())
        .or_else(|| {
          params?
            .iter()
            .find(|p| p.tag == tag)
            .and_then(|p| p.def.as_f64())
        })
        .unwrap_or(fallback)
    };
    // what kind of crate, what it holds
    let crate_type = data.derives_from(etype, "crate").then(|| {
      let mut look = "wood".to_string();
      if let Some(v) = lump.and_then(|l| l.get("crate-type")) {
        if let Some(s) = v.as_str() {
          look = s.to_string();
        }
        if let Some(s) = look.strip_prefix('\'') {
          look = s.to_string();
        }
      }
      look
    });
    let params: Vec<Value> = params
      .into_iter()
      .flatten()
      .map(|p| {
        let set = lump.is_some_and(|l| l.get(&p.tag).is_some());
        let value = lump.and_then(|l| l.get(&p.tag)).unwrap_or(&p.def).clone();
        let en = match p.kind.strip_prefix("enum:") {
          Some(e) if !e.is_empty() => data.enumeration(e),
          _ if p.kind == "eco-info" => data.enumeration("pickup-type"),
          _ => None,
        };
        json!({
          "tag": p.tag,
          "kind": p.kind,
          "value": value,
          "set": set,
          "meters": in_meters(&p.tag),
          "enum": en.map(|e| json!({"bitfield": e.bitfield, "values": e.values})),
        })
      })
      .collect();
    let tasks: Vec<&str> = (1..data.step_count() as usize)
      .map(|i| data.node(i).name.as_str())
      .collect();
    let actors: Vec<Value> = self
      .scene
      .nodes
      .iter()
      .filter(|o| o.kind == NodeKind::Actor && !o.name.is_empty())
      .map(|o| json!({"id": o.id, "name": o.name, "aid": o.extras.get("aid")}))
      .collect();
    Some(json!({
      "etype": etype,
      "kind": format!("{:?}", behavior_kind(Some(&data), etype)).to_lowercase(),
      "behavior": behavior_of(n),
      "bouncer_height": lump_float("spring-height", 45056.0) / K_UNITS,
      "elevator_speed": lump_float("elevator-move-rate", 25600.0) / K_UNITS,
      "crate_type": crate_type,
      "params": params,
      "tasks": tasks,
      "actors": actors,
    }))
  }

  /// `set_actor_behavior {id, behavior: {key: value | null (removed)}, live?}`: merged into the
  /// actor's og_behavior, undoable (undo.behavior)
  pub fn set_actor_behavior(&mut self, a: &Value) -> Result<(), String> {
    let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    if !self
      .scene
      .find(id)
      .is_some_and(|n| n.kind == NodeKind::Actor)
    {
      return Err(format!("no actor {id}"));
    }
    let changes = a
      .get("behavior")
      .and_then(Value::as_object)
      .cloned()
      .unwrap_or_default();
    let live = a.get("live").and_then(Value::as_bool).unwrap_or(false);
    self.inspector_edit(&[id], live, "undo.behavior", |scene| {
      let n = scene.find_mut(id).unwrap();
      let mut b = behavior_of(n);
      for (k, v) in changes {
        if v.is_null() {
          b.remove(&k);
        } else {
          b.insert(k, v);
        }
      }
      if let Some(o) = n.extras.as_object_mut() {
        if b.is_empty() {
          o.remove("og_behavior");
        } else {
          o.insert("og_behavior".into(), Value::Object(b));
        }
      }
    });
    Ok(())
  }

  /// `set_actor_param {id, tag, value (null: back to the game's), type?, label?, live?}`: a res lump
  /// of the actor, typed by its parameter's kind (or `type`: a lump the behavior section sets),
  /// undoable (`label`, else undo.param)
  pub fn set_actor_param(&mut self, a: &Value) -> Result<(), String> {
    let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    let tag = a
      .get("tag")
      .and_then(Value::as_str)
      .unwrap_or_default()
      .to_string();
    let etype = match self.scene.find(id) {
      Some(n) if n.kind == NodeKind::Actor => etype_of(n).to_string(),
      _ => return Err(format!("no actor {id}")),
    };
    let ty = match a.get("type").and_then(Value::as_str) {
      Some(t) => t.to_string(),
      None => {
        let data = self.game_data();
        let p = data
          .as_ref()
          .and_then(|d| d.params(&etype)?.iter().find(|p| p.tag == tag).cloned())
          .ok_or_else(|| format!("{tag}: not a parameter of {etype}"))?;
        lump_type(&p.kind)
          .ok_or_else(|| format!("{tag} ({}): not settable", p.kind))?
          .to_string()
      }
    };
    let mut value = a.get("value").cloned().unwrap_or(Value::Null);
    if ty == "symbol"
      && let Some(s) = value.as_str().filter(|s| !s.starts_with('\''))
    {
      value = format!("'{s}").into();
    }
    let label = a
      .get("label")
      .and_then(Value::as_str)
      .unwrap_or("undo.param");
    let live = a.get("live").and_then(Value::as_bool).unwrap_or(false);
    let reset = value.is_null();
    self.inspector_edit(&[id], live, label, |scene| {
      let n = scene.find_mut(id).unwrap();
      for (key, v) in [("lump", value), ("lump_types", Value::String(ty))] {
        if !n.extras[key].is_object() {
          n.extras[key] = json!({});
        }
        let o = n.extras[key].as_object_mut().unwrap();
        if reset {
          o.remove(&tag);
        } else {
          o.insert(tag.clone(), v);
        }
      }
    });
    Ok(())
  }

  /// `decor_behavior {id}`: what the game does with a piece of decor (null: not decor): {targets
  /// (the decor selected, which a change gives the same), collision, bounce, launch (every field,
  /// `set` when the node has it), default: game|see_through|small|shape (its collision when not set:
  /// what the export gives it)}
  pub fn decor_behavior(&self, id: u32) -> Option<Value> {
    if !decor_node(&self.scene, id) {
      return None;
    }
    // every selected piece of decor gets what is set here
    let mut targets: Vec<u32> = self
      .selection
      .iter()
      .copied()
      .filter(|&s| decor_node(&self.scene, s))
      .collect();
    if !targets.contains(&id) {
      targets.push(id);
    }
    let n = self.scene.find(id)?;
    let by_default = self
      .scene
      .index_of(id)
      .map_or(DefaultCollision::Shape, |i| {
        default_decor_collision(&self.scene, i)
      });
    let mut j = decor_fields(&decor_behavior_of(&n.extras));
    j["targets"] = json!(targets);
    j["default"] = json!(match by_default {
      DefaultCollision::Game => "game",
      DefaultCollision::SeeThrough => "see_through",
      DefaultCollision::Small => "small",
      DefaultCollision::Shape => "shape",
    });
    Some(j)
  }

  /// `set_decor_behavior {ids, collision?, bounce?, launch? (an object: set, else not), live?}`: the
  /// same behavior on every piece of decor of `ids` (names checked, values clamped), undoable
  /// (undo.decor)
  pub fn set_decor_behavior(&mut self, a: &Value) -> Result<(), String> {
    let ids: Vec<u32> = a
      .get("ids")
      .and_then(|v| serde_json::from_value::<Vec<u32>>(v.clone()).ok())
      .unwrap_or_default()
      .into_iter()
      .filter(|&id| decor_node(&self.scene, id))
      .collect();
    if ids.is_empty() {
      return Err("ids: elements of the decor (not actors)".into());
    }
    let mut d = json!({});
    for part in ["collision", "bounce", "launch"] {
      if let Some(p) = a.get(part).filter(|p| p.is_object()) {
        d[part] = p.clone();
      }
    }
    let value = decor_behavior_json(&decor_behavior_of(&json!({ "og_decor": d })));
    let live = a.get("live").and_then(Value::as_bool).unwrap_or(false);
    self.inspector_edit(&ids, live, "undo.decor", |scene| {
      for &id in &ids {
        let Some(o) = scene.find_mut(id).and_then(|n| n.extras.as_object_mut()) else {
          continue;
        };
        if value.is_null() {
          o.remove("og_decor");
        } else {
          o.insert("og_decor".into(), value.clone());
        }
      }
    });
    Ok(())
  }
}
