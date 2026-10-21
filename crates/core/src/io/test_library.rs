//! Test fixtures (src/app/selftest.cpp): a small library written in ogle-extract's layout, the
//! story library, and helpers to read and edit the .glb files they make.

#![allow(dead_code)] // helpers for the tests of other modules too

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::io::gltf::{ExportOptions, export_glb};
use crate::math::{Vec2, Vec3};
use crate::scene::{Material, Mesh, Node, NodeKind, Primitive, Scene};

/// A new empty folder for a test (tests run in parallel: one each)
pub fn temp_dir(name: &str) -> PathBuf {
  let dir = std::env::temp_dir()
    .join("jle-tests")
    .join(format!("{name}-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).unwrap();
  dir
}

/// The repository's library (the user's extracted game), when there is one: tests on real data
/// skip themselves without it
pub fn real_library() -> Option<PathBuf> {
  let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library");
  root.join("jak3/library.json").is_file().then_some(root)
}

/// a box of size 2, from y = 0 to 2
pub fn make_box(name: &str, material: i32) -> Arc<Mesh> {
  let mut mesh = Mesh::new();
  mesh.name = name.to_string();
  let mut p = Primitive::default();
  for i in 0..8 {
    p.pos.push(Vec3::new(
      if i & 1 != 0 { 1.0 } else { -1.0 },
      if i & 2 != 0 { 2.0 } else { 0.0 },
      if i & 4 != 0 { 1.0 } else { -1.0 },
    ));
  }
  let faces: [[u32; 4]; 6] = [
    [0, 1, 3, 2],
    [4, 6, 7, 5],
    [0, 4, 5, 1],
    [2, 3, 7, 6],
    [0, 2, 6, 4],
    [1, 5, 7, 3],
  ];
  for f in faces {
    p.idx.extend([f[0], f[1], f[2], f[0], f[2], f[3]]);
  }
  p.material = material;
  p.compute_normals();
  mesh.prims.push(p);
  Arc::new(mesh)
}

pub fn make_floor(material: i32) -> Arc<Mesh> {
  let mut mesh = Mesh::new();
  mesh.name = "floor".into();
  let mut p = Primitive {
    pos: vec![
      Vec3::new(-50.0, 0.0, -50.0),
      Vec3::new(50.0, 0.0, -50.0),
      Vec3::new(50.0, 0.0, 50.0),
      Vec3::new(-50.0, 0.0, 50.0),
    ],
    uv: vec![
      Vec2::new(0.0, 0.0),
      Vec2::new(1.0, 0.0),
      Vec2::new(1.0, 1.0),
      Vec2::new(0.0, 1.0),
    ],
    idx: vec![0, 2, 1, 0, 3, 2],
    material,
    ..Default::default()
  };
  p.compute_normals();
  mesh.prims.push(p);
  Arc::new(mesh)
}

fn json_len(bytes: &[u8]) -> Option<usize> {
  (bytes.len() >= 20).then(|| u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize)
}

/// Copies a .glb with its JSON chunk edited (the binary chunk is kept as is).
pub fn rewrite_glb_json(input: &Path, output: &Path, edit: impl FnOnce(&mut Value)) -> bool {
  let Ok(bytes) = std::fs::read(input) else {
    return false;
  };
  let Some(len) = json_len(&bytes) else {
    return false;
  };
  let Ok(mut j) = serde_json::from_slice::<Value>(&bytes[20..20 + len]) else {
    return false;
  };
  edit(&mut j);
  let mut text = serde_json::to_vec(&j).unwrap();
  while !text.len().is_multiple_of(4) {
    text.push(b' ');
  }
  let rest = &bytes[20 + len..];
  let total = 12 + 8 + text.len() + rest.len();
  let mut out = Vec::with_capacity(total);
  for v in [
    0x46546C67u32,
    2,
    total as u32,
    text.len() as u32,
    0x4E4F534A,
  ] {
    out.extend_from_slice(&v.to_le_bytes());
  }
  out.extend_from_slice(&text);
  out.extend_from_slice(rest);
  std::fs::write(output, out).is_ok()
}

pub fn write_text(path: &Path, text: &str) {
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(path, text).unwrap();
}

/// the JSON chunk of a .glb
pub fn read_glb_json(path: &Path) -> Value {
  let Ok(bytes) = std::fs::read(path) else {
    return Value::Null;
  };
  match json_len(&bytes) {
    Some(len) if 20 + len <= bytes.len() => {
      serde_json::from_slice(&bytes[20..20 + len]).unwrap_or(Value::Null)
    }
    _ => Value::Null,
  }
}

/// The bytes of an accessor of a .glb (its binary chunk)
pub fn glb_accessor_bytes(path: &Path, glb: &Value, accessor: usize) -> Vec<u8> {
  let Ok(bytes) = std::fs::read(path) else {
    return Vec::new();
  };
  let (Some(len), Some(a)) = (json_len(&bytes), glb["accessors"].get(accessor)) else {
    return Vec::new();
  };
  let bin = 20 + len + 8;
  let v = &glb["bufferViews"][a["bufferView"].as_u64().unwrap_or(0) as usize];
  let start = bin
    + v["byteOffset"].as_u64().unwrap_or(0) as usize
    + a["byteOffset"].as_u64().unwrap_or(0) as usize;
  let len = v["byteLength"].as_u64().unwrap_or(0) as usize;
  bytes
    .get(start..start + len)
    .map_or_else(Vec::new, <[u8]>::to_vec)
}

fn node(name: &str, kind: NodeKind, parent: u32, mesh: Option<Arc<Mesh>>) -> Node {
  Node {
    name: name.to_string(),
    kind,
    parent,
    mesh,
    ..Default::default()
  }
}

/// A level written like ogle-extract writes one: a background with the terrain (tfrag), a decor
/// prototype with two instances (TIE), collision and an actor; the model of its actor type; the
/// models every level shares.
pub fn write_library(root: &Path) -> bool {
  let game = root.join("jak3");
  write_text(
    &game.join("library.json"),
    r#"{"game": "jak3", "name": "Jak 3", "levels": ["LEV.DGO"]}"#,
  );
  write_text(
    &game.join("lev/level.json"),
    r#"{"level": "lev", "dgo": "LEV.DGO", "actors": 1}"#,
  );
  write_text(&game.join("_disc/DGO/stray.glb"), "not a level");

  let mut bg = Scene::new();
  let mat = bg.add_material(Material {
    name: "stone".into(),
    ..Default::default()
  });
  let tfrag_id = bg.add(node("lev-tfrag-0", NodeKind::Group, 0, None));
  bg.add(node(
    "floor-0",
    NodeKind::Render,
    tfrag_id,
    Some(make_floor(mat)),
  ));
  let tie_id = bg.add(node("lev-tie-0", NodeKind::Group, 0, None));
  let proto_id = bg.add(node("rock.mb", NodeKind::Group, tie_id, None));
  let rock = make_box("rock.mb", mat);
  for i in 0..2 {
    let mut inst = node(
      &format!("rock.mb-{i}"),
      NodeKind::Render,
      proto_id,
      Some(rock.clone()),
    );
    inst.local.t = Vec3::new(i as f32 * 10.0, 0.0, 0.0);
    bg.add(inst);
  }
  bg.add(node(
    "lev-collision",
    NodeKind::Collision,
    0,
    Some(make_floor(-1)),
  ));
  let project = root.join("bg-project.glb");
  let eo = ExportOptions::default();
  if export_glb(&bg, &project, &eo).is_err() {
    return false;
  }
  // as a rip has them: no editor extras, collision marked for the builder, an actor node
  std::fs::create_dir_all(game.join("lev/decor")).unwrap();
  let mut ok = rewrite_glb_json(&project, &game.join("lev/decor/lev-background.glb"), |j| {
    for n in j["nodes"].as_array_mut().unwrap() {
      let collision = n["extras"]["og_kind"] == "collision";
      n["extras"] = if collision {
        json!({"set_collision": 1, "set_invisible": 1, "collide_pat": 4660})
      } else {
        json!({})
      };
    }
    j["nodes"].as_array_mut().unwrap().push(json!({
      "name": "crate-9",
      "translation": [1.0, 2.0, 3.0],
      "extras": {"kind": "actor", "etype": "crate", "aid": 123, "game_task": 0,
                 "bsphere": [1.0, 2.0, 3.0, 5.0], "lump": {"name": "crate-9"}}
    }));
    let last = j["nodes"].as_array().unwrap().len() - 1;
    j["scenes"][0]["nodes"]
      .as_array_mut()
      .unwrap()
      .push(json!(last));
    j["asset"]["extras"] = json!({"opengoal": {"game": "jak3", "level": "lev"}});
  });
  // the models: one for the actor, one shared by every level
  for (folder, name) in [
    ("lev", "crate-krimson-lod0"),
    ("lev", "crate-krimson-lod1"),
    ("common", "collectables-skill-lod0"),
  ] {
    let mut m = Scene::new();
    m.add(node(name, NodeKind::Render, 0, Some(make_box(name, -1))));
    let dir = game.join(folder).join("models");
    std::fs::create_dir_all(&dir).unwrap();
    ok &= export_glb(&m, dir.join(format!("{name}.glb")), &eo).is_ok();
  }
  ok
}

/// A level as ogle-extract writes one, with the decor of `bg` and rip actors (glTF nodes with
/// their extras).
pub fn write_rip_level(
  game: &Path,
  name: &str,
  bg: &Scene,
  actors: &Value,
  collision_triangles: i32,
  node_extras: &Value,
  actor_types: &Value,
) -> bool {
  let tmp = game.join(format!("{name}-project.glb"));
  if export_glb(bg, &tmp, &ExportOptions::default()).is_err() {
    return false;
  }
  std::fs::create_dir_all(game.join(name).join("decor")).unwrap();
  let out = game
    .join(name)
    .join("decor")
    .join(format!("{name}-background.glb"));
  let ok = rewrite_glb_json(&tmp, &out, |j| {
    for n in j["nodes"].as_array_mut().unwrap() {
      let extras = node_extras.get(n["name"].as_str().unwrap_or("")).cloned();
      n["extras"] = extras.unwrap_or_else(|| json!({}));
    }
    for a in actors.as_array().into_iter().flatten() {
      j["nodes"].as_array_mut().unwrap().push(a.clone());
      let last = j["nodes"].as_array().unwrap().len() - 1;
      j["scenes"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!(last));
    }
    j["asset"]["extras"] = json!({"opengoal": {"game": "jak3", "level": name}});
  });
  let _ = std::fs::remove_file(&tmp);
  let count = actors.as_array().map_or(0, Vec::len);
  write_text(
    &game.join(name).join("level.json"),
    &json!({"level": name, "dgo": "X.DGO", "actors": count, "actor_types": actor_types,
            "collision_triangles": collision_triangles})
    .to_string(),
  );
  ok
}

pub fn rip_actor(name: &str, etype: &str, lump: Value, at: Vec3) -> Value {
  let mut l = lump;
  l["name"] = json!(name);
  json!({"name": name, "translation": [at.x, at.y, at.z],
         "extras": {"kind": "actor", "etype": etype, "aid": 1, "game_task": 0, "lump": l}})
}

/// A game with a story: the level "town" (a rock the mission breaks into rubble, a crate there
/// until the mission ends, a statue broken by it, a creature, a logic actor, a cutscene character,
/// a water pool), the hut the game shows with it, the layer of the mission's actors, and "port",
/// the level next to it (100 m away along x).
pub const STORY_GAME_DATA: &str = r#"{
 "format": 1, "game": "jak3",
 "actors": {"roles": {"grunt": "creature", "part-spawner": "logic"}, "creature_suffixes": ["-highres"],
            "effects": ["part-spawner"],
            "water_types": ["water-anim"], "water_looks": ["water-anim-test-pool"],
            "objects": {"crate": "crate", "statue-broken": "breakable", "no-model-thing": "other"}},
 "oceans": {"*ocean-map-test*": {"corner": [-96, -1.5, -96], "cell": 96, "cells": ["11", "01"], "color": [0.1, 0.2, 0.3]}},
 "levels": {
  "town": {"taskname": "town", "base_mask": 1, "ocean": "*ocean-map-test*", "layer": false, "traffic": false, "companions": ["hut"],
           "neighbors": ["hut", "port"],
           "continues": [{"name": "town-start", "position": [1, 0, 2], "shows": ["hut", "port", "town"]}]},
  "hut": {"taskname": "town", "base_mask": 1, "ocean": null, "layer": false, "traffic": false, "companions": [],
          "neighbors": ["port", "town"], "continues": []},
  "port": {"taskname": "port", "base_mask": 1, "ocean": null, "layer": false, "traffic": false, "companions": [],
           "neighbors": ["hut", "town"],
           "continues": [{"name": "port-start", "position": [100, 0, 5], "shows": ["port", "town"]}]},
  "lmission": {"taskname": "lmission", "base_mask": 0, "ocean": null, "layer": true, "traffic": false, "companions": []}
 },
 "story": {
  "reset_node": "start",
  "mask_bits": {"task0": 0, "task1": 1, "done": 8, "special": 12, "primary0": 13, "ctywide": 14, "never": 15},
  "nodes": [
   {"name": "none", "task": "", "level": ""},
   {"name": "start", "task": "intro", "level": "", "closed": true, "borrow": [["town", 0, null, null]]},
   {"name": "town-mission-introduction", "task": "town-mission", "level": "town", "parents": [1],
    "borrow": [["town", 0, "lmission", "display"]]},
   {"name": "town-mission-resolution", "task": "town-mission", "level": "town", "mask_op": "abs", "mask": 2,
    "close_task": true, "parents": [2], "borrow": [["town", 0, "lmission", "display"]]},
   {"name": "end", "task": "end", "level": "", "parents": [3]}
  ],
  "prototypes": {"town": [{"names": ["rock"], "when": ["not", ["complete", "town-mission"]]},
                          {"names": ["rubble"], "when": ["closed", "town-mission-resolution"]}]},
  "types": {"statue-broken": ["closed", "town-mission-resolution"]},
  "ocean_heights": {}
 }
}"#;

/// The story's library (STORY_GAME_DATA written as root/game-data.json)
pub fn write_story_library(root: &Path) -> bool {
  let game = root.join("jak3");
  write_text(
    &game.join("library.json"),
    r#"{"game": "jak3", "name": "Jak 3", "levels": ["X.DGO"]}"#,
  );
  let decor = |level: &str, protos: &[&str]| {
    let mut bg = Scene::new();
    let t = bg.add(node(&format!("{level}-tfrag-0"), NodeKind::Group, 0, None));
    bg.add(node(
      &format!("{level}-floor"),
      NodeKind::Render,
      t,
      Some(make_floor(-1)),
    ));
    // a piece of the shell standing apart (the extractor splits the tfrag): a small wall
    let mut wall = make_box(&format!("{level}-wall"), -1);
    for v in &mut Arc::make_mut(&mut wall).prims[0].pos {
      *v += Vec3::new(20.0, 0.0, 10.0); // in the level's space, as ripped
    }
    bg.add(node(
      &format!("{level}-wall"),
      NodeKind::Render,
      t,
      Some(wall),
    ));
    let tie_id = bg.add(node(&format!("{level}-tie-0"), NodeKind::Group, 0, None));
    for name in protos {
      let id = bg.add(node(&format!("{name}.mb"), NodeKind::Group, tie_id, None));
      bg.add(node(
        &format!("{name}.mb-0"),
        NodeKind::Render,
        id,
        Some(make_box(name, -1)),
      ));
    }
    bg
  };
  let none = Vec3::default();
  let town_actors = json!([
    rip_actor("crate-1", "crate", json!({"kill-mask": 4294905854.0}), none), // 0xffff0ffe: while the mask is task0
    rip_actor("grunt-2", "grunt", json!({}), none),
    rip_actor(
      "part-spawner-3",
      "part-spawner",
      json!({"art-name": "group-town-torch"}),
      none
    ),
    rip_actor(
      "jak-highres-4",
      "jak-highres",
      json!({"kill-mask": 32768}),
      none
    ),
    rip_actor("pool-5", "water-anim", json!({"look": 0}), none),
    rip_actor("statue-broken-6", "statue-broken", json!({}), none),
  ]);
  let terrain = json!({"town-floor": {"og_terrain": true}, "town-wall": {"og_terrain": false}});
  let town_types = json!({"crate": {"count": 1, "lump": {"name": "crate-1", "eco-info": [13, 10],
                                                          "kill-mask": 4294905854.0, "next-actor": "x"}}});
  let mut ok = write_rip_level(
    &game,
    "town",
    &decor("town", &["rock", "rubble"]),
    &town_actors,
    2,
    &terrain,
    &town_types,
  );
  ok &= write_rip_level(
    &game,
    "hut",
    &decor("hut", &[]),
    &json!([]),
    2,
    &json!({}),
    &json!({}),
  );
  // port: its shell 100 m away along x, as ripped (in the game's space)
  let mut port = decor("port", &["crane"]);
  for n in &mut port.nodes {
    if n.name != "crane.mb-0"
      && let Some(mesh) = &mut n.mesh
    {
      for v in &mut Arc::make_mut(mesh).prims[0].pos {
        *v += Vec3::new(100.0, 0.0, 0.0);
      }
    }
  }
  ok &= write_rip_level(
    &game,
    "port",
    &port,
    &json!([rip_actor(
      "barrel-1",
      "crate",
      json!({}),
      Vec3::new(100.0, 0.0, 3.0)
    )]),
    2,
    &json!({"port-floor": {"og_terrain": true}, "port-wall": {"og_terrain": false}}),
    &json!({}),
  );
  ok &= write_rip_level(
    &game,
    "lmission",
    &Scene::new(),
    &json!([rip_actor("lcrate-1", "crate", json!({}), none)]),
    0,
    &json!({}),
    &json!({}),
  );
  // the models: the pool's look, the crate
  let models = game.join("town/models");
  std::fs::create_dir_all(&models).unwrap();
  for model in ["water-anim-test-pool-lod0", "crate-test-lod0"] {
    let mut m = Scene::new();
    m.add(node(model, NodeKind::Render, 0, Some(make_box(model, -1))));
    ok &= export_glb(
      &m,
      models.join(format!("{model}.glb")),
      &ExportOptions::default(),
    )
    .is_ok();
  }
  write_text(&root.join("game-data.json"), STORY_GAME_DATA);
  ok
}
