use std::sync::Mutex;

use super::catalog::SHAPES;
use super::*;
use crate::editor::remote::run_tool;
use crate::io::test_library::real_library;
use crate::scene::Node;
use crate::scene::shapes::{make_shape, shape_preset};

fn editor() -> Mutex<Editor> {
  let mut ed = Editor::new();
  ed.write_prefs = false;
  Mutex::new(ed)
}

fn ok(ed: &Mutex<Editor>, tool: &str, args: Value) -> Value {
  let a = run_tool(ed, tool, &args);
  assert_eq!(a["ok"], true, "{tool}: {a}");
  a["result"].clone()
}

fn fail(ed: &Mutex<Editor>, tool: &str, args: Value) -> String {
  let a = run_tool(ed, tool, &args);
  assert_eq!(a["ok"], false, "{tool}: {a}");
  a["error"].as_str().unwrap().to_string()
}

/// a new level holding a floor shape and a box shape on it
fn test_document(ed: &Mutex<Editor>) -> (u32, u32) {
  ok(ed, "new_level", json!({}));
  let mut e = ed.lock().unwrap();
  let material = crate::scene::shapes::shape_default_material(&mut e.scene);
  let mut add = |name: &str, preset: &str, size: Vec3, at: Vec3| {
    let mut p = shape_preset(preset).unwrap().params.clone();
    p.size = size;
    let mut n = Node {
      name: name.into(),
      kind: NodeKind::Render,
      mesh: Some(make_shape(&p, material)),
      ..Default::default()
    };
    n.local.t = at;
    n.extras["og_shape"] = p.to_json();
    e.scene.add(n)
  };
  let floor = add(
    "floor-1",
    "floor",
    Vec3::new(20.0, 0.5, 20.0),
    Vec3::default(),
  );
  let crate_box = add(
    "box-1",
    "box",
    Vec3::new(2.0, 2.0, 2.0),
    Vec3::new(3.0, 0.5, 4.0),
  );
  e.scene.begin_frame();
  (floor, crate_box)
}

#[test]
fn find_transform_undo_on_a_test_document() {
  let ed = editor();
  let (floor, b) = test_document(&ed);
  let status = ok(&ed, "get_status", json!({}));
  assert_eq!(status["document"]["open"], true);
  assert_eq!(status["document"]["elements"], 2);
  assert_eq!(status["document"]["levels"][0]["role"], "opened");

  // by name, near a point
  let found = ok(&ed, "find_nodes", json!({"name": "BOX"}));
  assert_eq!(found["total"], 1);
  assert_eq!(found["nodes"][0]["id"], b);
  assert_eq!(found["nodes"][0]["kind"], "render");
  assert_eq!(found["nodes"][0]["size"], json!([2.0, 2.0, 2.0]));
  let near = ok(&ed, "find_nodes", json!({"near": [3, 1.5, 4], "radius": 1}));
  assert_eq!(near["nodes"].as_array().unwrap().len(), 1);
  let boxed = ok(
    &ed,
    "find_nodes",
    json!({"box": [[-1, -1, -1], [1, 1, 1]], "outside": true}),
  );
  assert_eq!(boxed["nodes"][0]["id"], b);

  // the floor under a point
  let hit = ok(&ed, "raycast", json!({"origin": [-5, 10, -5]}));
  assert_eq!(hit["hit"], true);
  assert_eq!(hit["node"]["id"], floor);
  assert_eq!(hit["point"][1], 0.5);

  // moved, turned, scaled: one undo step each
  let moved = ok(
    &ed,
    "transform_nodes",
    json!({"ids": [b], "position": [10, 0.5, 0], "scale_by": 2}),
  );
  assert_eq!(moved["nodes"][0]["position"], json!([10.0, 0.5, 0.0]));
  assert_eq!(moved["nodes"][0]["size"], json!([4.0, 4.0, 4.0]));
  let undone = ok(&ed, "undo", json!({}));
  assert_eq!(undone["done"], true);
  assert_eq!(undone["change"], tr("undo.scale"));
  let back = ok(&ed, "get_node", json!({"id": b}));
  assert_eq!(back["position"], json!([3.0, 0.5, 4.0]));
  assert_eq!(back["shape"]["kind"], "box");
  ok(&ed, "redo", json!({}));
  assert_eq!(
    ok(&ed, "get_node", json!({"id": b}))["position"],
    json!([10.0, 0.5, 0.0])
  );

  // a shape resized, copied, deleted, grouped
  let resized = ok(&ed, "set_shape", json!({"id": b, "size": [1, 3, 1]}));
  assert_eq!(resized["size"][1], 6.0); // scaled by 2
  let copies = ok(
    &ed,
    "duplicate_nodes",
    json!({"ids": [b], "offset": [0, 0, 5]}),
  );
  let copy = copies["nodes"][0]["id"].as_u64().unwrap() as u32;
  assert_eq!(copies["nodes"][0]["position"], json!([10.0, 0.5, 5.0]));
  let g = ok(&ed, "group", json!({"ids": [b, copy], "name": "pair"}));
  let groups = ok(&ed, "list_groups", json!({}));
  assert_eq!(groups["groups"][0]["group"], g["group"]);
  assert_eq!(groups["groups"][0]["ids"].as_array().unwrap().len(), 2);
  let sel = ok(&ed, "select", json!({"group": g["group"]}));
  assert_eq!(sel["selection"].as_array().unwrap().len(), 2);
  assert_eq!(
    ok(&ed, "delete_nodes", json!({"ids": [copy]}))["deleted"],
    1
  );
  assert!(fail(&ed, "get_node", json!({"id": copy})).contains("no element"));
  ok(&ed, "undo", json!({}));
  assert_eq!(ok(&ed, "get_node", json!({"id": copy}))["id"], copy);

  // hidden, then shown back
  assert_eq!(
    ok(
      &ed,
      "set_visibility",
      json!({"ids": [floor], "visible": false})
    )["changed"],
    1
  );
  assert_eq!(
    ok(&ed, "find_nodes", json!({"name": "floor"}))["nodes"][0]["hidden"],
    true
  );

  // the camera computed here, given to the viewport
  let cam = ok(
    &ed,
    "set_camera",
    json!({"position": [0, 10, 10], "look_at": [0, 0, 0]}),
  );
  assert_eq!(cam["position"], json!([0.0, 10.0, 10.0]));
  assert_eq!(cam["pitch_deg"], -45.0);
  assert!(fail(&ed, "set_view", json!({"weather": "snow"})).starts_with("weather: one of clear"));
  assert_eq!(ok(&ed, "set_view", json!({"hour": 30}))["hour"], 24.0);

  // the arguments checked as the C++ did
  assert_eq!(
    fail(&ed, "transform_nodes", json!({})),
    "ids: the elements to move"
  );
  assert_eq!(fail(&ed, "raycast", json!({})), "origin: [x, y, z]");
  assert_eq!(
    fail(&ed, "set_shape", json!({"id": floor + 100})),
    "id: a shape (place kind 'shape')"
  );
  assert_eq!(
    fail(&ed, "place", json!({"kind": "shape", "name": "box"})),
    "position: [x, y, z] in meters"
  );
}

// what was opened is framed at once (app.cpp frame_all): the answer gives the camera the C++
// gave, not the one before the viewport framed it
#[test]
fn a_project_opened_is_framed() {
  let ed = editor();
  test_document(&ed);
  let dir = crate::io::test_library::temp_dir("remote-framed");
  let path = dir.join("framed.glb").to_string_lossy().replace('\\', "/");
  ok(&ed, "save_project", json!({ "path": path }));
  let status = ok(&ed, "open_project", json!({ "path": path }));
  let cam = &status["camera"];
  assert_eq!(cam["yaw_deg"], json!(round3(0.6f32.to_degrees())));
  assert_eq!(cam["pitch_deg"], json!(round3((-0.45f32).to_degrees())));
  // the middle of what is open straight ahead
  let f = |v: &Value, k: usize| v[k].as_f64().unwrap();
  let b = &status["document"]["bounds"];
  let to: Vec<f64> = (0..3)
    .map(|k| (f(&b["min"], k) + f(&b["max"], k)) / 2.0 - f(&cam["position"], k))
    .collect();
  let d = to.iter().map(|x| x * x).sum::<f64>().sqrt();
  let along: f64 = (0..3).map(|k| to[k] / d * f(&cam["forward"], k)).sum();
  assert!(along > 0.999, "{along}");
  let _ = std::fs::remove_dir_all(dir);
}

// the land around the ocean still being made: the document is loading (the C++ waited on it)
#[test]
fn loading_while_the_ocean_land_is_made() {
  let ed = editor();
  test_document(&ed);
  assert_eq!(
    ok(&ed, "get_status", json!({}))["document"]["loading"],
    false
  );
  let job = Default::default();
  ed.lock().unwrap().jobs.insert("ocean_land".into(), job);
  assert_eq!(
    ok(&ed, "get_status", json!({}))["document"]["loading"],
    true
  );
}

// the export writes the models as they are: an animation preview going on is stopped first
// (App::export_for_game's stop_preview)
#[test]
fn export_stops_the_animation_preview() {
  let ed = editor();
  test_document(&ed);
  let model = {
    let mut e = ed.lock().unwrap();
    let a = e.scene.add(Node {
      name: "fac-elevator".into(),
      kind: NodeKind::Actor,
      extras: json!({"etype": "fac-elevator"}),
      ..Default::default()
    });
    let m = e.scene.add(Node {
      name: "fac-elevator-model".into(),
      parent: a,
      kind: NodeKind::Render,
      mesh: Some(crate::io::test_library::make_box("fac-elevator", -1)),
      extras: json!({"og_actor_visual": "fac-elevator"}),
      ..Default::default()
    });
    e.scene.begin_frame();
    e.call("start_preview", &json!({"id": a, "what": "motion"}))
      .unwrap();
    m
  };
  assert_eq!(ed.lock().unwrap().get_state()["preview"]["node"], model);
  let dir = crate::io::test_library::temp_dir("remote-export-preview");
  let folder = dir.to_string_lossy().replace('\\', "/");
  run_tool(
    &ed,
    "export_level",
    &json!({"name": "qatest", "folder": folder}),
  );
  assert_eq!(ed.lock().unwrap().get_state()["preview"]["node"], 0);
  let _ = std::fs::remove_dir_all(dir);
}

// the region's error names the keys the tool's description gives (the C++ text)
#[test]
fn navmesh_region_error_says_min_max() {
  let ed = editor();
  test_document(&ed);
  assert_eq!(
    fail(
      &ed,
      "generate_navmesh",
      json!({"region": {"min": [0, 0, 0]}})
    ),
    "region: \"all\", \"selection\" or {\"min\": [x, y, z], \"max\": [x, y, z]}"
  );
}

// the game's numbers read as the C++ read them (strtod, correctly rounded): an actor's bsphere of
// hiphog was one ulp off in the projects saved (serde_json's float_roundtrip)
#[test]
fn numbers_read_correctly_rounded() {
  let text = "13.193374633789063";
  let v: Value = serde_json::from_str(text).unwrap();
  assert_eq!(v.as_f64(), text.parse::<f64>().ok());
}

#[test]
fn capture_view_answers_the_viewports_picture() {
  let ed = std::sync::Arc::new(editor());
  test_document(&ed);
  assert_eq!(ok(&ed, "get_status", json!({}))["interface_ready"], false);
  // the viewport: ready a moment later (the capture waits for it), then answers the request it
  // was sent (jle:request {kind: "capture", id: 1})
  let viewport = {
    let ed = ed.clone();
    std::thread::spawn(move || {
      std::thread::sleep(Duration::from_millis(100));
      assert_eq!(ed.lock().unwrap().next_request, 1);
      ed.lock()
        .unwrap()
        .call("frontend_ready", &Value::Null)
        .unwrap();
      for _ in 0..2000 {
        {
          let mut e = ed.lock().unwrap();
          if e.next_request > 1 {
            let id = e.next_request - 1;
            let camera = json!({"pos": [1, 2, 3], "yaw": 0.0, "pitch": 0.0, "fov": 60.0});
            e.call(
              "frontend_reply",
              &json!({"id": id, "png": "iVBORw0KGgo=", "width": 320, "height": 200, "camera": camera}),
            )
            .unwrap();
            return;
          }
        }
        std::thread::sleep(Duration::from_millis(2));
      }
    })
  };
  let a = run_tool(&ed, "capture_view", &json!({"width": 320, "height": 200}));
  viewport.join().unwrap();
  assert_eq!(a["ok"], true, "{a}");
  assert_eq!(a["image"], "iVBORw0KGgo=");
  assert_eq!(a["result"]["width"], 320);
  assert_eq!(a["result"]["camera"]["position"], json!([1.0, 2.0, 3.0]));
  assert_eq!(ok(&ed, "get_status", json!({}))["interface_ready"], true);
}

#[test]
fn levels_of_the_real_library() {
  let Some(root) = real_library() else {
    return;
  };
  let ed = editor();
  ed.lock().unwrap().set_library(&root.to_string_lossy());
  let levels = ok(&ed, "list_levels", json!({"filter": "ctyinda"}));
  assert_eq!(levels["levels"][0]["name"], "ctyinda");
  assert!(fail(&ed, "open_level", json!({"name": "nowhere"})).starts_with("no level 'nowhere'"));
  let status = ok(
    &ed,
    "open_level",
    json!({"name": "ctyinda", "actors": true}),
  );
  assert_eq!(status["document"]["level"], "ctyinda");
  assert!(status["document"]["actors"].as_u64().unwrap() > 0);
  assert!(status["document"]["bounds"]["min"].is_array());
  let info = ok(&ed, "get_level_info", json!({}));
  assert_eq!(info["in_document"], "opened");
  let crates = ok(&ed, "find_nodes", json!({"kind": "actor", "limit": 5}));
  assert!(crates["total"].as_u64().unwrap() > 0);
  let id = crates["nodes"][0]["id"].clone();
  let node = ok(&ed, "get_node", json!({"id": id}));
  assert!(node["game_data"]["etype"].is_string());
  let asm = ok(&ed, "list_assemblies", json!({"limit": 3}));
  assert!(asm["count"].as_u64().unwrap() > 0);
  // an object of the catalog on the ground, a level's decor (read by the store's workers)
  // above an actor of the level: something is under it
  let at = &crates["nodes"][0]["position"];
  let c = |k: usize| at[k].as_f64().unwrap();
  let p = [c(0), c(1) + 30.0, c(2)];
  let placed = ok(
    &ed,
    "place",
    json!({"kind": "object", "name": "crate", "position": p, "on_ground": true}),
  );
  assert_eq!(placed["nodes"][0]["etype"], "crate");
  assert!(placed["nodes"][0]["position"][1].as_f64().unwrap() < p[1]);
  let decor = ok(&ed, "list_decor", json!({"level": "ctyinda", "limit": 3}));
  assert!(decor["total"].as_u64().unwrap() > 0);
  let part = decor["parts"][0]["name"].clone();
  let placed = ok(
    &ed,
    "place",
    json!({"kind": "decor", "name": part, "level": "ctyinda", "position": p}),
  );
  assert_eq!(placed["nodes"][0]["kind"], "render");
}

#[test]
fn shapes_placed_one_and_many() {
  let ed = editor();
  let (floor, _) = test_document(&ed);
  // dropped onto the floor (its top at 0.5 m)
  let one = ok(
    &ed,
    "place",
    json!({"kind": "shape", "name": "pillar", "position": [-5, 8, -5], "on_ground": true}),
  );
  let pillar = &one["nodes"][0];
  assert_eq!(pillar["position"], json!([-5.0, 0.5, -5.0]));
  assert!(pillar["name"].as_str().unwrap().starts_with("pillar"));
  assert_eq!(
    fail(
      &ed,
      "place",
      json!({"kind": "shape", "name": "blob", "position": [0, 0, 0]})
    ),
    SHAPES
  );
  assert_eq!(
    fail(
      &ed,
      "place",
      json!({"kind": "thing", "name": "x", "position": [0, 0, 0]})
    ),
    "kind: object, model, decor, prefab or shape"
  );
  // many at once, one undo step; an item refused says why
  let items = json!([
    {"kind": "shape", "name": "box", "position": [1, 0.5, 1], "size": [1, 1, 1]},
    {"kind": "shape", "name": "wall", "position": [6, 0.5, 1], "yaw_deg": 90},
    {"kind": "object", "name": "nothing-of-the-sort", "position": [0, 0.5, 0]},
  ]);
  let many = ok(&ed, "place_many", json!({ "items": items }));
  assert_eq!(many["placed"], 2);
  assert_eq!(many["nodes"][2]["index"], 2);
  assert!(
    many["nodes"][2]["error"]
      .as_str()
      .unwrap()
      .starts_with("no object")
  );
  assert_eq!(ok(&ed, "get_status", json!({}))["document"]["elements"], 5);
  let undone = ok(&ed, "undo", json!({}));
  assert_eq!(undone["change"], strf!(tr("undo.place_many"), 2));
  assert_eq!(ok(&ed, "get_status", json!({}))["document"]["elements"], 3);
  assert_eq!(ok(&ed, "get_node", json!({"id": floor}))["id"], floor);
}
