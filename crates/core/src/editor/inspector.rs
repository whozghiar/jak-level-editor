//! The inspector on the right (ui_inspector, ui_node_inspector, ui_group_section in panels.cpp and
//! groups.cpp): what it counts, the selection's group, and its edits, shown at once while a field
//! is dragged and one undo step when it is let go (behaviors.rs, surfaces.rs).

use std::sync::Arc;

use serde_json::{Value, json};

use super::Editor;
use crate::history::NodeChange;
use crate::i18n::tr;
use crate::scene::{NodeKind, Scene};

impl Editor {
  /// An edit of the inspector on `ids` (`edit` changes the scene). `live`: a field being dragged,
  /// shown now and kept; the same call without it (the drag let go, or a change made at once)
  /// makes one undo step `label` from before the drag.
  pub(crate) fn inspector_edit<R>(
    &mut self,
    ids: &[u32],
    live: bool,
    label: &str,
    edit: impl FnOnce(&mut Scene) -> R,
  ) -> R {
    let mut change = self
      .inspector_change
      .take()
      .unwrap_or_else(|| NodeChange::new(&self.scene));
    for &id in ids {
      change.save(&self.scene, id);
    }
    let r = edit(&mut self.scene);
    self.scene.begin_frame();
    if live {
      self.inspector_change = Some(change);
      self.scene_changed();
    } else {
      self
        .history
        .push(change.commit(&self.scene, tr(label), None));
    }
    r
  }

  /// `inspector {id}`: the document's counts (nothing selected), the copies of `id`'s mesh, the
  /// selection's group (all of it in one group, else whether a group can be made of it).
  pub fn inspector_info(&self, id: u32) -> Value {
    let cache = self.scene.cache();
    let mesh = self.scene.find(id).and_then(|n| n.mesh.clone());
    let (mut objects, mut actors, mut copies) = (0, 0, 0);
    for (i, n) in self.scene.nodes.iter().enumerate() {
      if n.kind == NodeKind::Render && cache.actor.get(i).is_none_or(|&a| a == 0) {
        objects += 1;
      }
      if n.kind == NodeKind::Actor {
        actors += 1;
      }
      if let (Some(m), Some(o)) = (&mesh, &n.mesh)
        && Arc::ptr_eq(m, o)
      {
        copies += 1;
      }
    }
    let roots = self.selection_roots();
    let mut gid = roots.first().map_or(0, |&r| self.group_of(r));
    if roots.iter().any(|&r| self.group_of(r) != gid) {
      gid = 0;
    }
    let group = self.group_info(gid).map(|g| {
      json!({
        "id": g.id,
        "name": g.name,
        "prefab": g.prefab,
        "roots": g.roots.len(),
        "whole": g.roots.iter().all(|r| self.selection.contains(r)),
      })
    });
    json!({
      "objects": objects,
      "actors": actors,
      "copies": copies,
      "roots": roots.len(),
      "group": group,
      "groups": self.document_groups().len(),
    })
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use serde_json::{Value, json};

  use super::Editor;
  use crate::io::game_data::GameData;
  use crate::io::test_library::make_box;
  use crate::math::Vec3;
  use crate::scene::shapes::{reshape, shape_preset};
  use crate::scene::{Material, Node, NodeKind, Texture};

  // a door, its parameters
  const GAME_DATA: &str = r#"{"format": 1, "game": "jak3",
    "actors": {"parents": {"test-door": "com-airlock"},
               "params": {"test-door": {"open-distance": ["float", 40960], "mode": ["enum:door-mode", 0],
                                        "flags": ["enum:door-flags", 0], "name-sym": ["symbol", "'none"],
                                        "path": ["path", null]}}},
    "enums": {"door-mode": {"values": [["closed", 0], ["open", 1]]},
              "door-flags": {"bitfield": true, "values": [["a", 0], ["b", 3]]}}}"#;

  struct Doc {
    ed: Editor,
    door: u32,
    walls: [u32; 2],
    shape: u32,
  }

  fn doc() -> Doc {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.game_data =
      Some(GameData::from_json(&serde_json::from_str(GAME_DATA).unwrap()).map(Arc::new));
    let s = &mut ed.scene;
    s.settings.level = "test".into();
    let door = s.add(Node {
      name: "door-1".into(),
      kind: NodeKind::Actor,
      extras: json!({"etype": "test-door", "aid": 7}),
      ..Default::default()
    });
    s.add_texture(Texture {
      name: "brick".into(),
      width: 1,
      height: 1,
      rgba: vec![255; 4],
      ..Default::default()
    });
    let m = s.add_material(Material::default());
    let mesh = make_box("wall", m);
    let walls = [0, 1].map(|i| {
      s.add(Node {
        name: format!("wall-{i}"),
        kind: NodeKind::Render,
        mesh: Some(mesh.clone()),
        ..Default::default()
      })
    });
    let shape = s.add(Node {
      name: "box".into(),
      kind: NodeKind::Render,
      ..Default::default()
    });
    reshape(s, shape, &shape_preset("box").unwrap().params);
    s.begin_frame();
    Doc {
      ed,
      door,
      walls,
      shape,
    }
  }

  fn call(ed: &mut Editor, name: &str, a: Value) -> Value {
    ed.call(name, &a).unwrap_or_else(|e| panic!("{name}: {e}"))
  }

  fn extras(ed: &Editor, id: u32) -> Value {
    ed.scene.find(id).unwrap().extras.clone()
  }

  #[test]
  fn actor_behavior_and_params() {
    let Doc { mut ed, door, .. } = doc();
    let b = call(&mut ed, "actor_behavior", json!({"id": door}));
    assert_eq!(b["kind"], "door");
    assert_eq!(b["crate_type"], Value::Null, "not a crate");
    let tags: Vec<&str> = b["params"]
      .as_array()
      .unwrap()
      .iter()
      .map(|p| p["tag"].as_str().unwrap())
      .collect();
    assert_eq!(tags, ["flags", "mode", "name-sym", "open-distance", "path"]);
    assert_eq!(b["params"][3]["meters"], true);
    assert_eq!(b["params"][0]["enum"]["bitfield"], true);
    assert_eq!(
      b["actors"],
      json!([{"id": door, "name": "door-1", "aid": 7}])
    );

    // a change made at once: one undo step
    let when = json!({"open": "when", "when": {"when": "gone", "arg": "crate-1"}});
    call(
      &mut ed,
      "set_actor_behavior",
      json!({"id": door, "behavior": when}),
    );
    assert_eq!(extras(&ed, door)["og_behavior"]["open"], "when");
    // a field dragged: shown at once, one undo step when let go
    for d in [1.0, 2.0, 3.0] {
      let a = json!({"id": door, "behavior": {"distance": d}, "live": true});
      call(&mut ed, "set_actor_behavior", a);
    }
    let steps = ed.history.labels().len();
    call(
      &mut ed,
      "set_actor_behavior",
      json!({"id": door, "behavior": {"distance": 4.0}}),
    );
    assert_eq!(
      ed.history.labels().len(),
      steps + 1,
      "a drag: one undo step"
    );
    assert_eq!(extras(&ed, door)["og_behavior"]["distance"], 4.0);
    call(&mut ed, "undo", json!({}));
    let b = extras(&ed, door)["og_behavior"].clone();
    assert!(
      b.get("distance").is_none() && b["open"] == "when",
      "undo: before the drag"
    );
    call(&mut ed, "undo", json!({}));
    assert!(
      extras(&ed, door).get("og_behavior").is_none(),
      "undo: no behavior"
    );

    // parameters: typed by their kind
    let param = |tag: &str, value: Value| json!({"id": door, "tag": tag, "value": value});
    call(
      &mut ed,
      "set_actor_param",
      param("open-distance", json!(8192.0)),
    );
    call(&mut ed, "set_actor_param", param("name-sym", json!("door")));
    call(&mut ed, "set_actor_param", param("flags", json!(9)));
    let e = extras(&ed, door);
    assert_eq!(
      e["lump"],
      json!({"open-distance": 8192.0, "name-sym": "'door", "flags": 9})
    );
    let types = json!({"open-distance": "float", "name-sym": "symbol", "flags": "uint32"});
    assert_eq!(e["lump_types"], types);
    assert!(
      ed.call("set_actor_param", &param("path", json!(1)))
        .is_err()
    );
    // a lump of the behavior section, its type given
    let mut a = param("spring-height", json!(4096.0));
    a["type"] = "float".into();
    a["label"] = "undo.behavior".into();
    call(&mut ed, "set_actor_param", a);
    let b = call(&mut ed, "actor_behavior", json!({"id": door}));
    assert_eq!(b["bouncer_height"], 1.0);
    // back to the game's value, then undo
    call(&mut ed, "set_actor_param", param("flags", Value::Null));
    assert!(extras(&ed, door)["lump"].get("flags").is_none());
    assert!(extras(&ed, door)["lump_types"].get("flags").is_none());
    for _ in 0..5 {
      call(&mut ed, "undo", json!({}));
    }
    let lump = extras(&ed, door)["lump"].clone();
    assert!(lump.as_object().is_none_or(|l| l.is_empty()), "undone");
  }

  #[test]
  fn decor_behavior_on_the_selection() {
    let Doc {
      mut ed,
      walls,
      door,
      ..
    } = doc();
    call(
      &mut ed,
      "select",
      json!({"ids": [walls[0], walls[1], door]}),
    );
    let d = call(&mut ed, "decor_behavior", json!({"id": walls[1]}));
    assert_eq!(
      d["targets"],
      json!(walls),
      "the decor selected, not the actor"
    );
    assert_eq!(d["collision"]["set"], false);
    assert_eq!(d["bounce"]["height"], 11.0, "the defaults shown");
    let none = call(&mut ed, "decor_behavior", json!({"id": door}));
    assert_eq!(none, Value::Null);

    let c = json!({"kind": "shape", "material": "ice", "event": "none", "mode": "auto"});
    let set = |bounce: Value, live: bool| json!({"ids": walls, "collision": c, "bounce": bounce, "live": live});
    call(&mut ed, "set_decor_behavior", set(Value::Null, false));
    for w in walls {
      assert_eq!(extras(&ed, w)["og_decor"]["collision"]["material"], "ice");
    }
    // a drag of the bounce's height on both: one step
    for h in [2.0, 3.0] {
      call(
        &mut ed,
        "set_decor_behavior",
        set(json!({"height": h}), true),
      );
    }
    call(
      &mut ed,
      "set_decor_behavior",
      set(json!({"height": 500.0}), false),
    );
    let bounce = extras(&ed, walls[1])["og_decor"]["bounce"]["height"].clone();
    assert_eq!(bounce, 200.0, "clamped");
    call(&mut ed, "undo", json!({}));
    assert!(extras(&ed, walls[0])["og_decor"].get("bounce").is_none());
    call(&mut ed, "undo", json!({}));
    for w in walls {
      assert!(extras(&ed, w).get("og_decor").is_none());
    }
  }

  #[test]
  fn shapes_and_materials() {
    let Doc {
      mut ed,
      walls,
      shape,
      ..
    } = doc();
    let info = call(&mut ed, "shape", json!({"id": shape}));
    assert_eq!(info["params"]["kind"], "box");
    assert_eq!(call(&mut ed, "shape", json!({"id": walls[0]})), Value::Null);
    let mesh = ed.scene.find(shape).unwrap().mesh.clone().unwrap();

    call(
      &mut ed,
      "set_shape",
      json!({"ids": [shape], "params": {"size": [3, 2, 1]}}),
    );
    let size = ed.scene.find(shape).unwrap().extras["og_shape"]["size"].clone();
    assert_eq!(size, json!([3.0, 2.0, 1.0]));
    call(&mut ed, "undo", json!({}));
    let n = ed.scene.find(shape).unwrap();
    assert!(
      Arc::ptr_eq(n.mesh.as_ref().unwrap(), &mesh),
      "undo: the mesh back"
    );
    // the scale made its size
    ed.scene.find_mut(shape).unwrap().local.s = Vec3::new(2.0, 1.0, 1.0);
    let width = |ed: &Editor| ed.scene.find(shape).unwrap().extras["og_shape"]["size"][0].as_f64();
    let before = width(&ed).unwrap();
    let a = json!({"ids": [shape], "params": {}, "apply_scale": true});
    call(&mut ed, "set_shape", a);
    assert_eq!(ed.scene.find(shape).unwrap().local.s.x, 1.0);
    assert_eq!(width(&ed), Some(before * 2.0));

    // a texture on one of the walls sharing a mesh: the other keeps its own
    call(&mut ed, "select_only", json!({"id": walls[0]}));
    let m = call(&mut ed, "materials", json!({"id": walls[0]}));
    assert_eq!(m["targets"], json!([walls[0]]));
    assert_eq!(m["slots"][0]["texture"], Value::Null);
    let a = json!({"ids": [walls[0]], "slot": 0, "texture": 0});
    assert_eq!(call(&mut ed, "set_material", a)["changed"], 1);
    let texture = |ed: &mut Editor, w: u32| {
      call(ed, "materials", json!({"ids": [w]}))["slots"][0]["texture"]["name"].clone()
    };
    assert_eq!(texture(&mut ed, walls[0]), "brick");
    assert_eq!(texture(&mut ed, walls[1]), Value::Null);
    call(&mut ed, "undo", json!({}));
    assert_eq!(texture(&mut ed, walls[0]), Value::Null);
    let a = json!({"ids": [walls[0]], "slot": 0, "texture": 9});
    assert!(ed.call("set_material", &a).is_err());
    // an image that is not one: nothing changes
    let steps = ed.history.labels().len();
    let a = json!({"ids": [walls[0]], "slot": 0, "path": "no-such.png"});
    call(&mut ed, "material_image", a);
    assert_eq!(ed.history.labels().len(), steps);
  }

  #[test]
  fn counts_and_group() {
    let Doc {
      mut ed,
      walls,
      door,
      ..
    } = doc();
    let i = call(&mut ed, "inspector", json!({"id": walls[0]}));
    let counts = [&i["objects"], &i["actors"], &i["copies"]];
    assert_eq!(counts, [3, 1, 2]);
    call(
      &mut ed,
      "select",
      json!({"ids": [walls[0], walls[1], door]}),
    );
    let i = call(&mut ed, "inspector", json!({"id": walls[0]}));
    assert_eq!((&i["roots"], &i["group"]), (&json!(3), &Value::Null));
    let g = call(&mut ed, "group_selection", json!({"name": "pair"}))["group"].clone();
    call(&mut ed, "select_only", json!({"id": walls[0]}));
    let i = call(&mut ed, "inspector", json!({"id": walls[0]}));
    assert_eq!(i["group"]["id"], g);
    assert_eq!(
      (&i["group"]["roots"], &i["group"]["whole"]),
      (&json!(3), &json!(false))
    );
  }
}
