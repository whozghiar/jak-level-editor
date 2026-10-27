//! The Place tool, prefabs, the catalog (selftest.cpp's prefab blocks, through the editor's calls);
//! on the user's extracted game when there is one (library/jak3/library.json).

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};

use super::super::selection::SelectMode;
use super::super::{Editor, ToolMode};
use crate::i18n::tr;
use crate::io::catalog::CatalogObject;
use crate::io::test_library::{make_box, real_library, temp_dir, write_library};
use crate::math::{Mat4, Ray, Transform, Vec3};
use crate::scene::pick::PickHit;
use crate::scene::{Node, NodeKind};
use crate::strf;

/// an editor on its own (its jobs run in the call), the preferences left as they are
fn editor(root: &std::path::Path) -> Editor {
  let mut ed = Editor::new();
  ed.write_prefs = false;
  ed.set_library(root.to_str().unwrap());
  ed
}

/// the plugin's timer, until `done` (the store's threads, the index...)
fn poll_until(ed: &mut Editor, mut done: impl FnMut(&mut Editor) -> bool) {
  for _ in 0..6000 {
    ed.poll_library_work();
    if done(ed) {
      return;
    }
    std::thread::sleep(std::time::Duration::from_millis(10));
  }
  panic!("still waiting");
}

fn close(a: Vec3, b: Vec3) -> bool {
  let d = a - b;
  d.x.abs() < 1e-3 && d.y.abs() < 1e-3 && d.z.abs() < 1e-3
}

#[test]
fn placing_and_prefabs() {
  let tmp = temp_dir("editor-placement");
  let root = tmp.join("library");
  assert!(write_library(&root));
  let mut ed = editor(&root);
  ed.placing.prefab_dir_override = tmp.join("prefabs").to_string_lossy().into();
  ed.load_level("lev", false).unwrap();

  // where it goes: the surface, else the ground plane, else 15 m along the ray; snapped, turned
  ed.call("start_shape_placement", &json!({"preset": "box"}))
    .unwrap();
  assert_eq!(ed.tool, ToolMode::Place);
  let down = Ray::new(Vec3::new(1000.3, 10.0, 0.2), Vec3::new(0.0, -1.0, 0.0));
  let miss = PickHit::default();
  assert!(close(
    ed.placement_matrix(&miss, &down).col3(3),
    Vec3::new(1000.3, 0.0, 0.2)
  ));
  let up = Ray::new(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
  assert!(close(
    ed.placement_matrix(&miss, &up).col3(3),
    Vec3::new(0.0, 16.0, 0.0)
  ));
  ed.gizmo.snap.enabled = true;
  assert!(close(
    ed.placement_matrix(&miss, &down).col3(3),
    Vec3::new(1000.5, 0.0, 0.0)
  ));
  ed.gizmo.snap.enabled = false;
  ed.call("turn_placement", &json!({"degrees": 90})).unwrap();
  let m = ed.placement_matrix(&miss, &down);
  assert!(close(
    m.dir(Vec3::new(1.0, 0.0, 0.0)),
    Vec3::new(0.0, 0.0, -1.0)
  ));
  // on the floor of the level, from the engine's pick
  let p = ed
    .call(
      "placement_preview",
      &json!({"origin": [3.0, 10.0, 3.0], "dir": [0.0, -1.0, 0.0]}),
    )
    .unwrap();
  assert_eq!(p["hit"], true, "{p}");
  assert_eq!(ed.get_state()["placement"]["kind"], "shape");

  // a shape placed: its own mesh, its parameters; selected, the tool goes on with Shift (keep)
  let preview = ed.placement_preview_mesh().cloned().unwrap();
  let r = ed
    .call(
      "place_at",
      &json!({"matrix": Mat4::translate(Vec3::new(5.0, 0.0, 5.0)), "keep": true}),
    )
    .unwrap();
  let shape = r["roots"][0].as_u64().unwrap() as u32;
  let n = ed.scene.find(shape).unwrap();
  assert!(
    n.extras["og_shape"]["kind"] == "box"
      && !Arc::ptr_eq(n.mesh.as_ref().unwrap(), &preview)
      && ed.tool == ToolMode::Place
      && ed.selection.contains(&shape)
      && ed.history.undo_label() == strf!(tr("undo.place"), "box"),
    "{}",
    ed.history.undo_label()
  );
  ed.call(
    "place_at",
    &json!({"matrix": Mat4::translate(Vec3::new(9.0, 0.0, 5.0))}),
  )
  .unwrap();
  assert_eq!(ed.tool, ToolMode::Select, "placed without Shift: done");

  // a prefab of the selection, without its small pieces
  let tiny = ed.scene.add(Node {
    name: "bolt".into(),
    kind: NodeKind::Render,
    mesh: Some(make_box("bolt", -1)),
    local: Transform {
      s: Vec3::new(0.1, 0.1, 0.1),
      ..Default::default()
    },
    ..Default::default()
  });
  ed.scene.begin_frame();
  let rocks: Vec<u32> = ed
    .scene
    .nodes
    .iter()
    .filter(|n| n.name.starts_with("rock.mb-"))
    .map(|n| n.id)
    .collect();
  ed.select_ids(&[rocks[0], rocks[1], tiny], SelectMode::Replace);
  let d = ed.call("prefab_dialog", &json!({"min_size": 0.5})).unwrap();
  assert_eq!(
    (d["roots"].as_u64(), d["kept"].as_u64()),
    (Some(3), Some(2)),
    "{d}"
  );
  let r = ed
    .call(
      "create_prefab",
      &json!({"name": "Rocks & crate", "min_size": 0.5}),
    )
    .unwrap();
  let path = r["path"].as_str().unwrap().to_string();
  assert!(
    path.ends_with("Rocks_crate.glb") && r["left_out"] == json!(["bolt"]),
    "{r}"
  );
  let listed = ed.call("prefabs", &Value::Null).unwrap();
  assert_eq!(listed["mine"][0]["name"], "Rocks_crate");

  // placed twice: its own group each time, named after it, the meshes shared
  ed.call(
    "start_prefab_placement",
    &json!({"path": path, "name": "Rocks_crate"}),
  )
  .unwrap();
  let a = ed.place_at(&Mat4::translate(Vec3::new(100.0, 5.0, 0.0)));
  let b = ed.place_at(&Mat4::translate(Vec3::new(200.0, 5.0, 0.0)));
  assert_eq!((a.len(), b.len()), (2, 2));
  let (ga, gb) = (ed.group_of(a[0]), ed.group_of(b[0]));
  let info = ed.group_info(gb).unwrap();
  assert!(ga != 0 && gb != 0 && ga != gb && info.prefab && info.name == "Rocks_crate");
  let mesh = |ed: &Editor, id: u32| ed.scene.find(id).unwrap().mesh.clone().unwrap();
  assert!(Arc::ptr_eq(&mesh(&ed, a[0]), &mesh(&ed, b[0])));
  assert_eq!(
    ed.selection,
    b.iter().copied().collect::<BTreeSet<_>>(),
    "the last one placed is selected"
  );
  let bounds = ed.subtree_bounds(b[0]).0;
  assert!(
    (bounds.lo.y - 5.0).abs() < 1e-3,
    "it sits on the point given"
  );
  ed.call("undo", &Value::Null).unwrap();
  assert!(ed.scene.find(b[0]).is_none() && ed.scene.find(a[0]).is_some());

  // dragged from a panel (once): placed where it is let go, then back to selecting
  ed.call(
    "start_prefab_placement",
    &json!({"path": path, "once": true}),
  )
  .unwrap();
  let r = ed
    .call(
      "place_here",
      &json!({"origin": [3.0, 10.0, 3.0], "dir": [0.0, -1.0, 0.0]}),
    )
    .unwrap();
  assert!(r["roots"].as_array().unwrap().len() == 2 && ed.tool == ToolMode::Select);

  // Escape stops placing first
  ed.call("start_shape_placement", &json!({"preset": "wall"}))
    .unwrap();
  ed.select_only(shape);
  ed.call("escape", &Value::Null).unwrap();
  assert!(ed.tool == ToolMode::Select && ed.selection.contains(&shape));

  // an object of the catalog: an actor named like the game's, its model under it
  let model = root
    .join("jak3/lev/models/crate-krimson-lod0.glb")
    .to_string_lossy()
    .replace('\\', "/");
  let crate_object = CatalogObject {
    etype: "crate".into(),
    name: "crate".into(),
    family: "crate".into(),
    interactive: true,
    model: "crate-krimson".into(),
    model_path: model,
    lump: json!({"name": "crate-1"}),
    ..Default::default()
  };
  ed.start_object_placement(&crate_object, false);
  poll_until(&mut ed, |ed| ed.placement_preview_mesh().is_some());
  let r = ed.place_at(&Mat4::translate(Vec3::new(2.0, 0.0, 2.0)));
  let actor = ed.scene.find(r[0]).unwrap();
  let kids = ed.scene.children(r[0]);
  assert!(
    actor.kind == NodeKind::Actor
      && actor.name.starts_with("crate-")
      && kids.len() == 1
      && ed.scene.find(kids[0]).unwrap().extras["og_actor_visual"] == "crate-krimson",
    "{}",
    actor.name
  );

  // the prefab deleted: gone from the panel
  ed.call("delete_prefab", &json!({"path": path})).unwrap();
  assert!(ed.call("prefabs", &Value::Null).unwrap()["mine"] == json!([]));
  let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn delete_prefab_only_in_its_folder() {
  let tmp = temp_dir("editor-delete-prefab");
  let folder = tmp.join("prefabs");
  std::fs::create_dir_all(&folder).unwrap();
  let mine = folder.join("mine.glb");
  let outside = tmp.join("other.glb");
  for f in [&mine, &outside] {
    std::fs::write(f, b"glTF").unwrap();
  }
  let mut ed = Editor::new();
  ed.write_prefs = false;
  ed.placing.prefab_dir_override = folder.to_string_lossy().into();
  let delete = |ed: &mut Editor, p: &std::path::Path| {
    ed.call("delete_prefab", &json!({"path": p.to_string_lossy()}))
  };
  // outside the folder, also through "..": refused, the file kept
  assert!(delete(&mut ed, &outside).is_err() && outside.exists());
  assert!(delete(&mut ed, &folder.join("..").join("other.glb")).is_err() && outside.exists());
  assert!(delete(&mut ed, &folder).is_err() && folder.exists());
  assert!(delete(&mut ed, &folder.join("missing.glb")).is_err());
  delete(&mut ed, &mine).unwrap();
  assert!(!mine.exists());
  let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn catalog_of_the_real_library() {
  let Some(root) = real_library() else {
    eprintln!("skipped: no library/jak3/library.json");
    return;
  };
  let mut ed = editor(&root);
  ed.wait_library_index();
  let list = |ed: &mut Editor, a: Value| ed.call("catalog", &a).unwrap();
  // the counts of the C++ editor's catalog (Jak 3: 747 objects, 197 models, 6261 decor parts)
  let objects: u64 = ["objects", "interactive", "effects"]
    .iter()
    .map(|t| list(&mut ed, json!({"tab": t}))["total"].as_u64().unwrap())
    .sum();
  let models = list(&mut ed, json!({"tab": "models"}));
  let decor = list(&mut ed, json!({"tab": "decor", "limit": 10}));
  assert_eq!(
    (
      objects,
      models["total"].as_u64(),
      decor["total"].as_u64(),
      decor["items"].as_array().map(Vec::len)
    ),
    (747, Some(197), Some(6261), Some(10)),
  );
  let effects = list(&mut ed, json!({"tab": "effects", "family": "effect.smoke"}));
  assert!(
    effects["count"].as_u64() > Some(0)
      && effects["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|o| o["category"] == "effect.smoke")
  );
  let catwalks = list(&mut ed, json!({"tab": "decor", "filter": "catwalk"}));
  assert!(catwalks["count"].as_u64() > Some(0));
  // a level's decor: read in the background, then listed
  poll_until(&mut ed, |ed| {
    list(ed, json!({"tab": "decor", "level": "ctyinda"}))["loading"] != true
  });
  let one = list(&mut ed, json!({"tab": "decor", "level": "ctyinda"}));
  assert!(one["total"].as_u64() > Some(0), "{}", one["failed"]);
  // the textures of every level, of a level
  let all = ed.call("texture_list", &json!({})).unwrap();
  let wall = ed
    .call(
      "texture_list",
      &json!({"level": "ctyinda", "filter": "wall"}),
    )
    .unwrap();
  assert!(all["total"].as_u64() > Some(500) && wall["count"].as_u64() > Some(0));
  let ti = ed.call("palette_texture", &wall["items"][0]).unwrap()["texture"]
    .as_i64()
    .unwrap();
  assert!(ti >= 0 && ed.scene.textures[ti as usize].width > 0);
  // Jak II's models
  if !ed.library_jak2.is_empty() {
    let models = list(&mut ed, json!({"tab": "models", "jak2": true}));
    assert_eq!(models["total"].as_u64(), Some(535));
  }
}

#[test]
fn game_prefabs_of_the_real_library() {
  let Some(root) = real_library() else {
    eprintln!("skipped: no library/jak3/library.json");
    return;
  };
  let tmp = temp_dir("editor-game-prefabs");
  let mut ed = editor(&root);
  ed.placing.prefab_dir_override = tmp.to_string_lossy().into();
  ed.refresh_prefabs();
  let listed = ed.call("prefabs", &Value::Null).unwrap();
  let game = listed["game"].as_array().unwrap().clone();
  assert!(game.len() >= 143 && game.iter().all(|p| p["made"] == false));
  // clicked before it is made: made with its level's (each level read once), then placed
  let p = game.iter().find(|p| p["level"] == "onintent").unwrap();
  let r = ed
    .call(
      "start_prefab_placement",
      &json!({"path": p["path"], "name": p["name"]}),
    )
    .unwrap();
  assert_eq!(r["waiting"], true);
  poll_until(&mut ed, |ed| ed.catalog.game_prefab_runs > 0);
  let after = ed.call("prefabs", &Value::Null).unwrap();
  let made: Vec<&Value> = after["game"]
    .as_array()
    .unwrap()
    .iter()
    .filter(|p| p["made"] == true)
    .collect();
  assert!(
    !made.is_empty()
      && made.iter().all(|p| p["level"] == "onintent")
      && ed.tool == ToolMode::Place
      && ed.placement().path == p["path"].as_str().unwrap()
  );
  let _ = std::fs::remove_dir_all(&tmp);
}
