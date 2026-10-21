//! The viewport's queries besides `pick` (app.cpp select_tool, panels.cpp ui_labels): every element
//! along a ray (a click again at the same place goes to the next one), the names drawn over the
//! view.

use serde_json::{Value, json};

use super::Editor;
use crate::math::{Ray, normalize};
use crate::scene::NodeKind;
use crate::scene::pick::{K_PICK_ALL_MAX, PickFilter, pick_all};

impl Editor {
  /// The select tool's filter, `args` may change it: `{render?, actors?, collision?, navmesh?,
  /// include_locked?, exclude?}` (the actors and nav meshes as the view shows them, the locked
  /// ground included: it hides what is behind it)
  pub(crate) fn pick_filter(&self, args: &Value) -> PickFilter {
    let flag = |k: &str, d: bool| args.get(k).and_then(Value::as_bool).unwrap_or(d);
    PickFilter {
      render: flag("render", true),
      actors: flag("actors", self.view.actors),
      collision: flag("collision", false),
      navmesh: flag("navmesh", self.view.navmesh),
      include_locked: flag("include_locked", true),
      exclude_list: args
        .get("exclude")
        .and_then(|e| serde_json::from_value(e.clone()).ok())
        .unwrap_or_default(),
    }
  }

  /// `pick_all {origin, dir, max?, ...filter}`: every element along the ray, the nearest first (the
  /// glows last), one hit each: `[{node, point, normal, distance, selectable, locked}]`, `node` the
  /// actor rather than its model, `selectable` what a click selects (0: the locked ground), `locked`
  /// the piece hit is (what is under the ground is not reached by clicking).
  pub fn pick_all_call(&self, args: &Value) -> Value {
    let v = |k: &str| serde_json::from_value(args.get(k)?.clone()).ok();
    let ray = Ray::new(
      v("origin").unwrap_or_default(),
      normalize(v("dir").unwrap_or_default()),
    );
    let max = args
      .get("max")
      .and_then(Value::as_u64)
      .map_or(K_PICK_ALL_MAX, |m| m as usize);
    let hits = pick_all(&self.scene, &ray, &self.pick_filter(args), max);
    let out: Vec<Value> = hits
      .iter()
      .filter_map(|h| {
        let n = self.scene.find(h.node)?;
        let node = match self.scene.actor_of(n) {
          0 => h.node,
          a => a,
        };
        Some(json!({
          "node": node,
          "point": h.point,
          "normal": h.normal,
          "distance": h.t,
          "selectable": self.selectable(h.node),
          "locked": n.locked,
        }))
      })
      .collect();
    json!(out)
  }

  /// `labels`: the names View > Labels draws: of the actors (with their etype) and of the selection
  /// when it is small (12 elements at most): `[{id, name, etype?}]`
  pub fn labels_call(&self) -> Value {
    let few_selected = self.selection.len() <= 12;
    let out: Vec<Value> = self
      .scene
      .nodes
      .iter()
      .filter(|n| n.kind == NodeKind::Actor || (few_selected && self.selection.contains(&n.id)))
      .map(|n| match n.kind {
        NodeKind::Actor => json!({"id": n.id, "name": n.name, "etype": n.extras.get("etype")}),
        _ => json!({"id": n.id, "name": n.name}),
      })
      .collect();
    json!(out)
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use crate::editor::Editor;
  use crate::io::test_library::make_box;
  use crate::math::Vec3;
  use crate::scene::{Node, NodeKind};

  // a click again at the same place: what stands behind, the ground last and not selectable
  #[test]
  fn pick_all_goes_through_everything_under_the_cursor() {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    let at = |z: f32, locked: bool| Node {
      kind: NodeKind::Render,
      mesh: Some(make_box("box", -1)),
      locked,
      local: crate::math::Transform {
        t: Vec3::new(0.0, 0.0, z),
        ..Default::default()
      },
      ..Default::default()
    };
    let near = ed.scene.add(at(0.0, false));
    let far = ed.scene.add(at(-5.0, false));
    let ground = ed.scene.add(at(-10.0, true));
    ed.scene.begin_frame();
    let all = ed
      .call(
        "pick_all",
        &json!({"origin": [0, 0.5, 10], "dir": [0, 0, -1]}),
      )
      .unwrap();
    let ids: Vec<u64> = all
      .as_array()
      .unwrap()
      .iter()
      .map(|h| h["node"].as_u64().unwrap())
      .collect();
    assert_eq!(ids, [near, far, ground].map(u64::from), "nearest first");
    assert!(all[2]["locked"] == true && all[2]["selectable"] == 0 && all[0]["selectable"] == near);
    let one = ed
      .call(
        "pick_all",
        &json!({"origin": [0, 0.5, 10], "dir": [0, 0, -1], "max": 1}),
      )
      .unwrap();
    assert_eq!(one.as_array().unwrap().len(), 1);
    // the names over the view: the actors, the selection when small
    let actor = ed.scene.add(Node {
      kind: NodeKind::Actor,
      name: "crate-1".into(),
      extras: json!({"etype": "crate"}),
      ..Default::default()
    });
    ed.select_only(far);
    let labels = ed.call("labels", &json!({})).unwrap();
    let ids: Vec<u64> = labels
      .as_array()
      .unwrap()
      .iter()
      .map(|l| l["id"].as_u64().unwrap())
      .collect();
    assert!(ids.contains(&(far as u64)) && ids.contains(&(actor as u64)) && ids.len() == 2);
    assert!(
      labels
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["etype"] == "crate")
    );
  }
}
