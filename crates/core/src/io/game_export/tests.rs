use super::*;
use crate::io::asset_library::AssetLibrary;
use crate::io::game_data::tests::real_data;
use crate::io::test_library::{
  glb_accessor_bytes, make_box, make_floor, read_glb_json, real_library, temp_dir,
};
use crate::scene::story::{StoryView, apply_story};
use crate::scene::{AlphaMode, Material, TodColors, TodPalette};

fn read_text(p: &Path) -> String {
  std::fs::read_to_string(p).unwrap_or_default()
}

/// a .jsonc of the builder, its comment lines left out
fn read_jsonc(p: &Path) -> Value {
  let text = read_text(p);
  let lines: Vec<&str> = text
    .lines()
    .filter(|l| !l.trim_start().starts_with("//"))
    .collect();
  serde_json::from_str(&lines.join("\n")).unwrap_or(Value::Null)
}

/// the nodes of a .glb by name: their extras
fn glb_extras(glb: &Value) -> HashMap<String, Value> {
  glb["nodes"]
    .as_array()
    .into_iter()
    .flatten()
    .map(|n| {
      (
        n["name"].as_str().unwrap_or_default().to_string(),
        n.get("extras").cloned().unwrap_or_else(|| json!({})),
      )
    })
    .collect()
}

fn render(name: &str, mesh: Arc<Mesh>, parent: u32, at: Vec3) -> Node {
  let mut n = Node {
    name: name.into(),
    kind: NodeKind::Render,
    parent,
    mesh: Some(mesh),
    ..Default::default()
  };
  n.local.t = at;
  n
}

fn group(name: &str) -> Node {
  Node {
    name: name.into(),
    kind: NodeKind::Group,
    ..Default::default()
  }
}

#[test]
fn surfaces_as_the_builder_takes_them() {
  let (wall, loss) = builder_surface(0x5cc8); // stone wall, nopilot
  assert!(wall.material == 0 && wall.mode == 1 && wall.event == 0 && loss == "nopilot");
  assert_eq!(builder_surface((28 << 10) | (13 << 18)).1, "rail"); // hdwood, rail
  let (hdwood, loss) = builder_surface(28 << 10);
  assert!(hdwood.material == 6 && loss == "hdwood");
  let (wood, loss) = builder_surface((6 << 10) | (1 << 16) | 4); // wood, nolos, noedge
  assert!(
    wood.material == 6 && wood.nolineofsight && wood.noedge && loss.is_empty(),
    "export: collision surfaces as the builder takes them (Jak 1's fields), what is lost said"
  );
}

#[test]
fn lumps_as_the_builder_reads_them() {
  let v = builder_lump_value;
  assert_eq!(v(&json!("abc"), "string"), json!("abc"));
  assert_eq!(v(&json!("'steel"), "symbol"), json!("'steel"));
  assert_eq!(
    v(&json!(["'a", "'b"]), "symbol"),
    json!(["symbol", "a", "b"])
  );
  assert_eq!(v(&json!(1.5), "float"), json!(["float", 1.5]));
  assert_eq!(v(&json!([3, 4]), "uint8"), json!(["uint32", 3, 4]));
  assert_eq!(
    v(&json!([1, 2, 3, 4]), "vector"),
    json!(["vector", [1.0, 2.0, 3.0, 4.0]])
  );
  assert_eq!(
    v(&json!("'citb-bunny"), "type"),
    json!(["type", "citb-bunny"])
  );
  assert_eq!(
    v(&json!("(want-load 'a)"), "pair"),
    json!(["pair", "(want-load 'a)"])
  );
  assert_eq!(v(&json!(3), ""), json!(["int32", 3]));
  assert!(v(&json!([1]), "actor-group").is_null());
  // the regions' scripts: their integers as floats, not inside a string
  assert_eq!(
    builder_script("(want-vis 'a 2 -3 \"x 4\") (b 1.5)"),
    "(want-vis 'a 2.0 -3.0 \"x 4\") (b 1.5)"
  );
  assert!(
    default_nickname("my-test") == "mte"
      && default_nickname("a") == "max"
      && default_iso_name("my-test") == "MYTEST"
      && default_iso_name("--") == "CUSTOM"
      && num(-0.00001) == "0.0"
      && num(1115.82081) == "1115.8208"
      && num(2.0) == "2.0"
  );
}

/// a small level: its shell with the game's collision, a box placed, actors; then the same in
/// place of ctyindb (src/app/selftest.cpp)
#[test]
fn a_small_level() {
  let data = real_data();
  let tmp = temp_dir("game-export");
  let mut s = Scene::new();
  s.settings.level = "testlvl".into();
  // blended with its texture's alpha, as the levels' materials nearly all are: still a wall
  let m = Material {
    name: "stone".into(),
    alpha: AlphaMode::Blend,
    base_color: crate::math::Vec4::new(2.0, 2.0, 2.0, 2.0), // a level's (the PS2's colors)
    ..Default::default()
  };
  let mat = s.add_material(m.clone());
  let glass = s.add_material(Material {
    name: "glass".into(),
    translucent: true, // drawn by the game without writing its depth
    ..m
  });
  let tfrag_id = s.add(group("testlvl-tfrag-normal"));
  let mut floor = render(
    "testlvl-floor",
    make_floor(mat),
    tfrag_id,
    Vec3::new(0.0, 0.0, 0.0),
  );
  {
    // its lighting in the game: one color, each of its 8 palettes recognizable
    let mut pal = TodPalette::default();
    for k in 0..8u8 {
      pal.rgba.extend([10 + k, 20 + k, 30 + k, 128]);
    }
    floor.tod = Some(Arc::new(TodColors {
      palette: Some(Arc::new(pal)),
      index: vec![vec![0; floor.mesh.as_ref().unwrap().prims[0].pos.len()]],
    }));
  }
  s.add(floor);
  let col_group = s.add(group("testlvl-collision"));
  let mut col_mesh = make_floor(-1);
  Arc::make_mut(&mut col_mesh).prims[0].tri_pat = vec![0x5cc8, 28 << 10];
  s.add(Node {
    name: "testlvl-collision-0".into(),
    kind: NodeKind::Collision,
    parent: col_group,
    mesh: Some(col_mesh),
    ..Default::default()
  });
  let mut bx = render("box-1", make_box("box", mat), 0, Vec3::new(10.0, 0.0, 0.0));
  bx.local.s = Vec3::new(3.0, 3.0, 3.0); // 6 m: a piece that collides by its shape
  s.add(bx);
  // see through: the builder's translucent tree
  s.add(render(
    "glass-1",
    make_box("glass", glass),
    0,
    Vec3::new(30.0, 0.0, 0.0),
  ));
  // 2 m: a detail, without collision
  s.add(render(
    "lamp-1",
    make_box("lamp", mat),
    0,
    Vec3::new(20.0, 0.0, 0.0),
  ));
  let mut actor =
    |name: &str, etype: &str, aid: u32, mut lump: Value, types: Value, visible: bool| {
      lump["name"] = name.into();
      let mut a = Node {
        name: name.into(),
        kind: NodeKind::Actor,
        visible,
        extras: json!({"etype": etype, "aid": aid, "game_task": 7, "bsphere": [5, 1, 5, 2],
        "lump": lump, "lump_types": types}),
        ..Default::default()
      };
      a.local.t = Vec3::new(5.0, 1.0, 5.0);
      s.add(a);
    };
  actor(
    "crate-1",
    "crate",
    101,
    json!({"eco-info": [9, 5], "kill-mask": 16384, "task-actor": 3, "next-actor": 102,
      "visvol": [[0, 0, 0, 1], [1, 1, 1, 1]]}),
    json!({"eco-info": "int32", "kill-mask": "uint32", "task-actor": "uint8",
      "next-actor": "uint32", "visvol": "vector", "name": "string"}),
    true,
  );
  actor(
    "cty-door-1",
    "cty-door",
    102,
    json!({"on-activate": "(want-load 'x)"}),
    json!({"on-activate": "pair"}),
    true,
  );
  actor(
    "cty-fruit-stand-1",
    "cty-fruit-stand",
    103,
    json!({}),
    json!({}),
    true,
  );
  actor("crate-2", "crate", 104, json!({}), json!({}), false);
  actor(
    "switch-a",
    "sew-floor-switch",
    105,
    json!({}),
    json!({}),
    true,
  );
  actor(
    "vent-1",
    "industrial-board-vent",
    106,
    json!({"spring-height": 40960.0}),
    json!({"spring-height": "float"}),
    true,
  );
  actor("ghost-1", "crate", 107, json!({}), json!({}), true);
  actor(
    "plat-1",
    "plat",
    108,
    json!({"path": [[0, 0, 0, 1], [40960, 0, 0, 1]]}),
    json!({"path": "vector"}),
    true,
  );
  for a in &mut s.nodes {
    match a.name.as_str() {
      "cty-door-1" => {
        a.extras["og_behavior"] =
          json!({"open": "when", "when": {"when": "pressed", "arg": "switch-a"}, "distance": 12})
      }
      "vent-1" => {
        a.extras["og_behavior"] =
          json!({"active": {"when": "task", "arg": "arena-fight-1-resolution"}})
      }
      "ghost-1" => a.extras["og_behavior"] = json!({"present": {"when": "never"}}),
      "crate-1" => {
        a.extras["lump"]["alt-actor"] = "switch-a".into();
        a.extras["lump_types"]["alt-actor"] = "actor-name".into();
      }
      "plat-1" => {
        // found at (5, 1, 5) in the game, moved 10 m along x since
        a.extras["og_origin"] = json!([5, 1, 5, 0, 0, 0, 1]);
        a.local.t = Vec3::new(15.0, 1.0, 5.0);
      }
      _ => {}
    }
  }
  s.begin_frame();

  let out = tmp.join("export");
  let o = LevelExportOptions {
    name: "my-test".into(),
    nickname: default_nickname("my-test"),
    iso_name: default_iso_name("my-test"),
    folder: from_path(&out),
    start: Vec3::new(1.0, 2.0, 3.0),
    ..Default::default()
  };
  assert_eq!(check_export_names(&o, None), None);
  let r = export_level(&s, Some(&data), &o).expect("exported");
  // laid out like the jak-project checkout: the level, its code, the README at the top
  let dir = out.join("custom_assets/jak3/levels/my-test");
  let code = out.join("goal_src/jak3/levels/my-test/my-test-obs.gc");
  assert!(
    r.files.len() == 6
      && r.behaviors == 2
      && out.join("README.md").is_file()
      && code.is_file()
      && Path::new(&r.level_dir) == dir
      && r.code_file == from_path(&code),
    "export: the files ({:?})",
    r.files
  );
  for f in [
    "my-test.glb",
    "my-test.jsonc",
    "my-test.gd",
    "level-info.gc",
  ] {
    assert!(dir.join(f).is_file(), "{f}");
  }
  let level = read_jsonc(&dir.join("my-test.jsonc"));
  let by_name: HashMap<String, &Value> = level["actors"]
    .as_array()
    .expect("actors")
    .iter()
    .map(|a| {
      (
        a["lump"]["name"].as_str().unwrap_or_default().to_string(),
        a,
      )
    })
    .collect();
  let (crate_j, door_j, vent_j, plat_j) = (
    by_name["crate-1"],
    by_name["cty-door-1"],
    by_name["vent-1"],
    by_name["plat-1"],
  );
  assert_eq!(level["actors"].as_array().unwrap().len(), 5);
  assert!(!by_name.contains_key("ghost-1"));
  assert_eq!(crate_j["etype"], "crate");
  assert_eq!(crate_j["lump"]["eco-info"], json!(["int32", 9, 5]));
  assert_eq!(
    crate_j["lump"]["next-actor"],
    json!(["string", "cty-door-1"])
  );
  assert_eq!(crate_j["lump"]["alt-actor"], json!(["string", "switch-a"]));
  assert_eq!(crate_j["lump"]["task-actor"], json!(["uint32", 3]));
  assert_eq!(crate_j["game_task"], 7);
  assert!(crate_j["lump"].get("kill-mask").is_none() && crate_j["lump"].get("visvol").is_none());
  assert_eq!(door_j["etype"], "my-test-cty-door-1");
  assert!(door_j["lump"].get("on-activate").is_none());
  assert_eq!(vent_j["etype"], "my-test-vent-1");
  assert_eq!(vent_j["lump"]["spring-height"], json!(["float", 40960.0]));
  assert_eq!(plat_j["etype"], "plat");
  assert_eq!(
    plat_j["lump"]["path"],
    json!(["vector", [40960.0, 0.0, 0.0, 1.0], [81920.0, 0.0, 0.0, 1.0]]),
    "export: a path moved with its actor"
  );
  assert!(
    level["automatic_wall_detection"] == true
      && level["nickname"] == "mte"
      && level["base_id"] == K_DEFAULT_BASE_ID
  );
  // the level's code and the code of the game it needs, in its DGO
  let (gc, gd, install) = (
    read_text(&code),
    read_text(&dir.join("my-test.gd")),
    read_text(&out.join("README.md")),
  );
  let at = |s: &str| gd.find(s).unwrap_or(usize::MAX);
  let (at_part, at_obs, at_own, at_level) = (
    at("\"ctyinda-part.o\""),
    at("\"ctyinda-obs.o\""),
    at("\"my-test-obs.o\""),
    at("\"my-test.go\""),
  );
  assert!(
    gc.contains("(in-package goal)")
      && gc.contains("(deftype my-test-cty-door-1 (cty-door) ())")
      && gc.contains("(entity-status? \"switch-a\" subtask-complete)")
      && gc.contains("(deftype my-test-vent-1 (industrial-board-vent) ())")
      && gd.contains("\"sewer-obs.o\"")
      && at_part < at_obs
      && at_obs < at_own
      && at_own < at_level
      && at_level != usize::MAX
      && install.contains("(goal-src \"levels/my-test/my-test-obs.gc\" \"entity-debug\"")
      && install.contains("(get-continue-by-name *game-info* \"my-test-start\")"),
    "export: the behaviors as the level's code, its DGO with the game's code its objects need"
  );
  // without the sky flag the game draws no sky around a new level: a black void; with the default
  // mood its lights (palettes 5 to 7, lit in the editor) stay off: the pieces of indoor levels black
  let info = read_text(&dir.join("level-info.gc"));
  assert!(
    info.contains(":level-flags (level-flags sky)")
      && info.contains(":mood-func 'update-mood-ctyindb")
  );
  let glb = read_glb_json(&dir.join("my-test.glb"));
  let extras = glb_extras(&glb);
  let surfaces = extras.values().filter(|e| e["set_invisible"] == 1).count();
  let keys: BTreeSet<&str> = r.notes.iter().map(|n| n.key.as_str()).collect();
  for k in [
    "export.note.types_missing",
    "export.note.surfaces",
    "export.note.kill_masks",
    "export.note.hidden",
    "export.note.scripts_dropped",
    "export.note.lumps_widened",
    "export.note.absent",
    "export.note.collision_small",
  ] {
    assert!(keys.contains(k), "{k}");
  }
  // the box's collision written apart, invisible, from both sides; a face giving the builder's grid
  // a cell along the level's height (under 7.3 m)
  let shape = &extras["my-test-collision-shape-0"];
  assert!(
    surfaces == 4
      && extras["my-test-collision-span"]["set_collision"] == 1
      && shape["set_collision"] == 1
      && extras["testlvl-floor"]["ignore"] == 1
      && extras["box-1"]["ignore"] == 1
      && extras["lamp-1"]["ignore"] == 1
      && r.collision_triangles == 2
      && r.collision_from_decor == 1
      && r.dense_places == 0,
    "export: the game's collision one node per surface, the shell not colliding twice, a 6 m box \
     by its shape (written apart), a 2 m lamp without collision"
  );
  // a level's blended walls (their depth written) cut out in the builder's solid tree; the decor
  // faces either way, its collision holds from both sides (the game's own stays one-sided)
  let materials = glb["materials"].as_array().expect("materials");
  let blend = |m: &Value| m["alphaMode"] == "BLEND";
  let glass_blended = materials
    .iter()
    .any(|m| m["name"] == "glass" && blend(m) && m["extras"]["og_blend"] == "alpha");
  let walls_cut = materials
    .iter()
    .filter(|m| m["name"] != "glass")
    .all(|m| !blend(m));
  assert!(
    walls_cut && glass_blended && level["double_sided_collide"] == false,
    "export: a level's blended walls cut out, its glass in the builder's translucent tree \
     (og_blend), the decor's collision from both sides"
  );
  // the game's lighting: every mesh with the 8 palettes the builder reads, a node's own as it has
  // them (the floor's), the new geometry's made from its color
  let mut all_lit = glb.get("meshes").is_some();
  let mut floor_exact = false;
  for n in glb["nodes"].as_array().unwrap() {
    // (the game's collision is not drawn)
    let Some(mesh) = n["mesh"].as_u64() else {
      continue;
    };
    if n["extras"]["set_invisible"] == 1 {
      continue;
    }
    for p in glb["meshes"][mesh as usize]["primitives"]
      .as_array()
      .unwrap()
    {
      for k in [
        "_SUNRISE",
        "_MORNING",
        "_NOON",
        "_AFTERNOON",
        "_SUNSET",
        "_TWILIGHT",
        "_EVENING",
        "_GREENSUN",
      ] {
        all_lit &= p["attributes"].get(k).is_some();
      }
    }
    if n["name"] == "testlvl-floor" {
      let p = &glb["meshes"][mesh as usize]["primitives"][0];
      let noon = glb_accessor_bytes(
        &dir.join("my-test.glb"),
        &glb,
        p["attributes"]["_NOON"].as_u64().unwrap() as usize,
      );
      floor_exact = noon.len() >= 4 && noon[..4] == [12, 22, 32, 255];
    }
  }
  assert!(
    all_lit && floor_exact,
    "export: the 8 palettes of the game's lighting on every mesh, a piece's own exactly"
  );

  // the same level in place of ctyindb: a level of the game next to it and the original shown for
  // reference are left out; ground under ctyindb-start only
  let mut bad = o.clone();
  bad.name = "ctyindb".into();
  assert_eq!(
    check_export_names(&bad, Some(&data)),
    Some("export.bad.name_taken")
  );
  let mut bad = o.clone();
  bad.nickname = "cib".into();
  assert_eq!(
    check_export_names(&bad, Some(&data)),
    Some("export.bad.nickname_taken")
  );
  let mut bad = o.clone();
  bad.replaces = "no-such-level".into();
  assert_eq!(
    check_export_names(&bad, Some(&data)),
    Some("export.bad.replaces")
  );
  for (name, key) in [("ctyinda", None), ("ctyindb", Some("og_reference"))] {
    let mut g = group(name);
    g.extras = json!({ "og_level": name });
    if let Some(key) = key {
      g.extras[key] = true.into();
    }
    let gid = s.add(g);
    s.add(render(
      &format!("{name}-box"),
      make_box("box", mat),
      gid,
      Vec3::new(0.0, 0.0, 0.0),
    ));
  }
  s.add(render(
    "start-ground",
    make_floor(mat),
    0,
    Vec3::new(971.42, 7.0, 546.11),
  ));
  s.begin_frame();
  let checks = check_continues(&s, Some(&data), "ctyindb");
  let start_ok = checks
    .iter()
    .any(|c| c.name == "ctyindb-start" && c.ok() && (c.ground - 7.0).abs() < 1e-3);
  let falling = checks
    .iter()
    .filter(|c| c.name != "ctyindb-start" && !c.ok())
    .count();
  let ro = LevelExportOptions {
    name: "my-ctyb".into(),
    nickname: "mcy".into(),
    replaces: "ctyindb".into(),
    folder: from_path(&tmp.join("export-replace")),
    ..o.clone()
  };
  assert_eq!(check_export_names(&ro, Some(&data)), None);
  let rr = export_level(&s, Some(&data), &ro).expect("exported in place of ctyindb");
  let rdir = tmp.join("export-replace/custom_assets/jak3/levels/my-ctyb");
  let (info, rgd, rinstall) = (
    read_text(&rdir.join("level-info.gc")),
    read_text(&rdir.join("my-ctyb.gd")),
    read_text(&tmp.join("export-replace/README.md")),
  );
  let rextras = glb_extras(&read_glb_json(&rdir.join("my-ctyb.glb")));
  let counts: HashMap<&str, usize> = rr.notes.iter().map(|n| (n.key.as_str(), n.count)).collect();
  let (at_part, at_obs) = (
    rgd.find("\"ctyindb-part.o\"").unwrap_or(usize::MAX),
    rgd.find("\"ctyindb-obs.o\"").unwrap_or(usize::MAX),
  );
  // the floor of 100 m under ctyindb-start also holds ctyindb-hijack-vehicle; the three others fall
  assert!(start_ok && checks.len() == 5 && falling == 3, "{checks:?}");
  assert!(
    !rextras.contains_key("ctyinda-box")
      && !rextras.contains_key("ctyindb-box")
      && rextras.contains_key("start-ground")
      && !info.contains("level-load-info")
      && info.contains(":nickname 'cib")
      && info.contains(":nickname 'mcy")
      && at_part < at_obs
      && at_obs != usize::MAX
      && rinstall.contains("(get-continue-by-name *game-info* \"ctyindb-start\")")
      && counts.get("export.note.replace_left_out") == Some(&1)
      && counts.get("export.note.continue_no_ground") == Some(&falling)
      && counts.contains_key("export.note.regions_missing"),
    "export in place of a level of the game: its own level only, the replaced level's code in \
     its DGO, the nickname to change, the continue points without ground said ({:?})",
    rr.notes
  );
}

/// the decor's behaviors: a slab of deadly ice, a cable Jak goes through, a pad that bounces, a
/// ramp that launches
#[test]
fn the_decors_behaviors() {
  let data = real_data();
  let tmp = temp_dir("game-export-decor");
  let mut d = Scene::new();
  for (name, x, decor) in [
    (
      "ice-slab",
      0.0,
      json!({"collision": {"material": "ice", "event": "deadly", "noedge": true}}),
    ),
    ("cable", 5.0, json!({"collision": {"kind": "none"}})),
    (
      "statue",
      20.0,
      json!({"collision": {"kind": "box", "material": "metal"}}),
    ),
    ("pad", 10.0, json!({"bounce": {"height": 15}})),
    (
      "ramp",
      15.0,
      json!({"launch": {"distance": 30, "height": 5, "yaw": 90, "time": 1.5}}),
    ),
  ] {
    let mut n = render(name, make_box(name, -1), 0, Vec3::new(x, 0.0, 0.0));
    n.extras = json!({ "og_decor": decor });
    d.add(n);
  }
  d.begin_frame();
  let o = LevelExportOptions {
    name: "my-decor".into(),
    nickname: "mdc".into(),
    iso_name: "MYDECOR".into(),
    folder: from_path(&tmp.join("export-decor")),
    ..Default::default()
  };
  let dr = export_level(&d, Some(&data), &o).expect("exported");
  let dd = tmp.join("export-decor/custom_assets/jak3/levels/my-decor");
  let extras = glb_extras(&read_glb_json(&dd.join("my-decor.glb")));
  let dlevel = read_jsonc(&dd.join("my-decor.jsonc"));
  let actors: HashMap<&str, &Value> = dlevel["actors"]
    .as_array()
    .expect("actors")
    .iter()
    .map(|a| (a["lump"]["name"].as_str().unwrap_or_default(), a))
    .collect();
  let dgc = read_text(&tmp.join("export-decor/goal_src/jak3/levels/my-decor/my-decor-obs.gc"));
  // the deadly ice drawn, its collision written apart with its surface
  assert_eq!(extras["ice-slab"]["ignore"], 1);
  assert!(extras.values().any(|e| e["set_invisible"] == 1
    && e["collide_material"] == 1
    && e["collide_event"] == 1
    && e["noedge"] == 1));
  assert_eq!(extras["cable"]["ignore"], 1);
  // collision by the box around it: none of its own, an invisible box of metal in its place
  assert_eq!(extras["statue"]["ignore"], 1);
  let b = &extras["statue-collision-box"];
  assert!(b["set_invisible"] == 1 && b["collide_material"] == 16);
  let pad = actors["pad-bounce"];
  assert!(
    pad["etype"] == "my-decor-pad-bounce"
      && pad["lump"]["spring-height"] == json!(["float", 15.0 * 4096.0])
      && (pad["trans"][1].as_f64().unwrap() - 2.0).abs() < 1e-3
  );
  assert_eq!(actors["ramp-launch"]["etype"], "my-decor-ramp-launch");
  // the ramp faces +z: turned 90 degrees, Jak lands toward +x, 30 m away, 5 m above its top
  assert!(dgc.contains("(set-vector! (-> this landing) 184320.0 28672.0 0.0 1.0)"));
  assert!(
    dgc.contains("(deftype my-decor-pad-bounce (bouncer) ())")
      && dgc.contains("(deftype my-decor-ramp-launch (process-drawable)")
      && dgc.contains("'launch-dir")
      && dgc.contains("450")
      && dr.behaviors == 2,
    "export: the decor's collision (deadly ice, none), a bounce and a launch as actors of the \
     level's code"
  );
}

/// the collision made with the collision tool is written whatever the export's choice of collision
/// (here the decor's), by its surface
#[test]
fn the_collision_made_in_the_tool() {
  let data = real_data();
  let tmp = temp_dir("game-export-made-collision");
  let mut d = Scene::new();
  d.add(render(
    "slab",
    make_box("slab", -1),
    0,
    Vec3::new(0.0, 0.0, 0.0),
  ));
  let mut floor = (*make_floor(-1)).clone();
  for p in &mut floor.prims {
    p.tri_pat = vec![16 << 10; p.tri_count()]; // metal
  }
  d.add(Node {
    name: "collision-floor".into(),
    kind: NodeKind::Collision,
    mesh: Some(Arc::new(floor)),
    extras: json!({"og_collision": "made"}),
    ..Default::default()
  });
  d.begin_frame();
  let o = LevelExportOptions {
    name: "my-made".into(),
    nickname: "mmd".into(),
    iso_name: "MYMADE".into(),
    folder: from_path(&tmp.join("export-made")),
    game_collision: false,
    ..Default::default()
  };
  let r = export_level(&d, Some(&data), &o).expect("exported");
  let dd = tmp.join("export-made/custom_assets/jak3/levels/my-made");
  let extras = glb_extras(&read_glb_json(&dd.join("my-made.glb")));
  let c = &extras["my-made-collision-16-0-0"];
  assert!(
    c["collide_material"] == 16 && c["set_invisible"] == 1,
    "{extras:?}"
  );
  assert_eq!(r.collision_triangles, 2);
}

/// the game's collision follows its decor: a level as extracted (a floor, a wall, a block, their
/// collision), and the document made from it, the wall deleted and the block moved 30 m
#[test]
fn the_games_collision_follows_its_decor() {
  let data = real_data();
  let tmp = temp_dir("game-export-follow");
  let level_scene = |edited: bool| {
    let mut sc = Scene::new();
    let tfrag = sc.add(group("lvx-tfrag-normal"));
    let mut piece = |name: &str, mesh: Arc<Mesh>, at: Vec3, scale: Vec3| {
      let mut n = render(name, mesh, tfrag, at);
      n.local.s = scale;
      sc.add(n);
    };
    let one = Vec3::new(1.0, 1.0, 1.0);
    piece("lvx-floor", make_floor(-1), Vec3::new(0.0, 0.0, 0.0), one);
    if !edited {
      let (at, size) = (Vec3::new(-20.0, 0.0, 0.0), Vec3::new(5.0, 5.0, 0.5));
      piece("lvx-wall", make_box("wall", -1), at, size);
      // rubble, deleted: its collision is coarser than it (a box 1.5 to 3 m off its faces)
      let (at, size) = (Vec3::new(0.0, 10.0, 20.0), Vec3::new(2.0, 2.0, 2.0));
      piece("lvx-rubble", make_box("rubble", -1), at, size);
    }
    let block_at = Vec3::new(20.0, 0.0, if edited { 30.0 } else { 0.0 });
    piece(
      "lvx-block",
      make_box("block", -1),
      block_at,
      Vec3::new(3.0, 3.0, 3.0),
    );
    let colg = sc.add(group("lvx-collision"));
    // the game's collision: the floor, and the wall and the block as they were
    let mut cmesh = make_floor(-1);
    let cp = &mut Arc::make_mut(&mut cmesh).prims[0];
    cp.uv.clear();
    let b = make_box("c", -1).prims[0].clone();
    for (at, scale) in [
      (Vec3::new(-20.0, 0.0, 0.0), Vec3::new(5.0, 5.0, 0.5)),
      (Vec3::new(20.0, 0.0, 0.0), Vec3::new(3.0, 3.0, 3.0)),
      (Vec3::new(0.0, 10.0, 20.0), Vec3::new(5.0, 5.0, 5.0)),
    ] {
      let base = cp.pos.len() as u32;
      for (p, nrm) in b.pos.iter().zip(&b.nrm) {
        cp.pos
          .push(Vec3::new(p.x * scale.x, p.y * scale.y, p.z * scale.z) + at);
        cp.nrm.push(*nrm);
      }
      cp.idx.extend(b.idx.iter().map(|v| base + v));
    }
    cp.tri_pat = vec![0; cp.idx.len() / 3];
    sc.add(Node {
      name: "lvx-collision-0".into(),
      kind: NodeKind::Collision,
      parent: colg,
      mesh: Some(cmesh),
      ..Default::default()
    });
    sc.begin_frame();
    sc
  };
  let original = Arc::new(level_scene(false));
  let mut doc = level_scene(true);
  assert_eq!(collision_levels(&doc), ["lvx"]);
  let mut fo = LevelExportOptions {
    name: "my-follow".into(),
    nickname: "mfl".into(),
    iso_name: "MYFOLLOW".into(),
    folder: from_path(&tmp.join("export-follow")),
    ..Default::default()
  };
  fo.originals.insert("lvx".into(), original);
  let fr = export_level(&doc, Some(&data), &fo).expect("exported");
  let taken_out = fr
    .notes
    .iter()
    .find(|n| n.key == "export.note.collision_taken_out")
    .map_or(0, |n| n.count);
  let extras = glb_extras(&read_glb_json(
    &tmp.join("export-follow/custom_assets/jak3/levels/my-follow/my-follow.glb"),
  ));
  // the floor's collision kept (and the bottoms of the wall and the block, on it), the rest of
  // theirs taken out, and all of the rubble's though it lies off it; the floor still the game's,
  // the block colliding by its shape where it is
  assert!(
    (2..=8).contains(&fr.collision_triangles)
      && taken_out >= 16
      && extras["lvx-floor"]["ignore"] == 1
      && extras["lvx-block"]["ignore"] == 1
      && extras["my-follow-collision-shape-0"]["set_invisible"] == 1,
    "export: the game's collision taken out with the pieces deleted or moved (rubble's coarser \
     one too), a moved piece colliding by its shape: collision {}, taken out {taken_out}, block {}",
    fr.collision_triangles,
    extras["lvx-block"]
  );
  // and a dense piece (5 m, 3,200 triangles) on the floor: said as a place where Jak would fall
  let mut dp = Primitive::default();
  for z in 0..=40 {
    for x in 0..=40 {
      dp.pos.push(Vec3::new(
        x as f32 * 0.125 - 2.5,
        0.3 + 0.01 * ((x + z) % 2) as f32,
        z as f32 * 0.125 - 2.5,
      ));
    }
  }
  for z in 0..40u32 {
    for x in 0..40u32 {
      let a = z * 41 + x;
      dp.idx.extend([a, a + 41, a + 1, a + 1, a + 41, a + 42]);
    }
  }
  dp.compute_normals();
  let mut dense = Mesh::new();
  dense.prims.push(dp);
  doc.add(render(
    "grating",
    Arc::new(dense),
    0,
    Vec3::new(0.0, 0.0, -30.0),
  ));
  doc.begin_frame();
  fo.name = "my-dense".into();
  fo.nickname = "mdn".into();
  let fr2 = export_level(&doc, Some(&data), &fo).expect("exported");
  assert!(
    fr2.dense_places >= 1
      && fr2
        .notes
        .iter()
        .any(|n| n.key == "export.note.collision_dense"),
    "export: a place denser than Jak 3 gathers (461 triangles) said"
  );
}

// --- the real game's data ----------------------------------------------------------------------

/// The options the C++ editor exported with, read back from its files: names, ids, start.
fn options_of_export(export: &Path, name: &str) -> LevelExportOptions {
  let dir = export.join("custom_assets/jak3/levels").join(name);
  let level = read_jsonc(&dir.join(format!("{name}.jsonc")));
  let readme = read_text(&export.join("README.md"));
  let replaces = readme
    .lines()
    .next()
    .and_then(|l| l.split(", in place of ").nth(1))
    .unwrap_or_default()
    .to_string();
  let info = read_text(&dir.join("level-info.gc"));
  let start = info
    .split(":trans (static-vectorm ")
    .nth(1)
    .and_then(|s| s.split(')').next())
    .map(|s| {
      let v: Vec<f32> = s.split(' ').filter_map(|x| x.parse().ok()).collect();
      Vec3::new(v[0], v[1], v[2])
    })
    .unwrap_or(Vec3::new(0.0, 0.0, 0.0));
  LevelExportOptions {
    name: name.into(),
    nickname: level["nickname"].as_str().unwrap_or_default().into(),
    iso_name: level["iso_name"].as_str().unwrap_or_default().into(),
    base_id: level["base_id"].as_u64().unwrap_or(0) as u32,
    start,
    replaces,
    ..Default::default()
  }
}

/// The text files of an export (README, .jsonc, .gd, level-info.gc, the level's code): path in
/// the export -> text
fn export_texts(export: &Path) -> BTreeMap<String, String> {
  fn walk(dir: &Path, top: &Path, out: &mut BTreeMap<String, String>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
      let p = e.path();
      if p.is_dir() {
        walk(&p, top, out);
      } else if p.extension().is_some_and(|x| x != "glb") {
        out.insert(from_path(p.strip_prefix(top).unwrap()), read_text(&p));
      }
    }
  }
  let mut out = BTreeMap::new();
  walk(export, export, &mut out);
  out
}

/// The first line two texts differ at, for a message
fn first_difference(a: &str, b: &str) -> Option<String> {
  if a == b {
    return None;
  }
  let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
  // a line of JSON (the regions of the .jsonc): the same values, though nlohmann's numbers are
  // not always the shortest (-150.27688598632813 for -150.27688598632812, 4.6e-06 for 4.6e-6)
  fn value_of(line: &str) -> Option<Value> {
    let v = line.trim().trim_end_matches(',');
    let v = v.split_once("\": ").map_or(v, |(_, v)| v);
    serde_json::from_str(v).ok()
  }
  fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
      (Value::Number(x), Value::Number(y)) => {
        let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
        (x - y).abs() <= 1e-12 * x.abs().max(1.0)
      }
      (Value::Array(x), Value::Array(y)) => {
        x.len() == y.len() && x.iter().zip(y).all(|(x, y)| close(x, y))
      }
      (Value::Object(x), Value::Object(y)) => {
        x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w)))
      }
      _ => a == b,
    }
  }
  let same = |x: &&str, y: &&str| {
    x == y || matches!((value_of(x), value_of(y)), (Some(v), Some(w)) if close(&v, &w))
  };
  if a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| same(x, y)) {
    return None;
  }
  let at = a
    .iter()
    .zip(&b)
    .position(|(x, y)| !same(x, y))
    .unwrap_or(a.len().min(b.len()));
  Some(format!(
    "line {}\n  C++:  {}\n  Rust: {}",
    at + 1,
    a.get(at).unwrap_or(&"<end>"),
    b.get(at).unwrap_or(&"<end>")
  ))
}

/// The user's projects exported by the C++ editor (library/export/<project>/, made from the
/// project as saved in library/projects/<project>/<project>.glb): exported again, the same text
/// files and the same nodes in the .glb.
#[test]
fn the_projects_exported_like_the_cpp_editor() {
  let Some(root) = real_library() else {
    return;
  };
  crate::i18n::i18n_init(&crate::i18n::lang_dir(), "en");
  let data = real_data();
  let mut library = AssetLibrary::default();
  library
    .scan(&from_path(&root.join("jak3")))
    .expect("library");
  let tmp = temp_dir("game-export-projects");
  let mut compared = 0;
  for project in ["demo-oasis", "wascitybis"] {
    let (export, file) = (
      root.join("export").join(project),
      root
        .join("projects")
        .join(project)
        .join(format!("{project}.glb")),
    );
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    // the export made from the project as saved
    if !file.is_file() || mtime(&export.join("README.md")) < mtime(&file) {
      continue;
    }
    let mut scene = Scene::new();
    let read = ImportOptions {
      read_settings: true,
      ..Default::default()
    };
    import_gltf(&mut scene, &file, &read).expect("project");
    apply_story(&mut scene, Some(&data), &StoryView::default());
    let mut o = options_of_export(&export, project);
    o.folder = from_path(&tmp.join(project));
    // what App::export_for_game adds: the nav meshes, the levels as extracted, the regions
    (o.navmeshes, o.navmesh_names) = crate::navmesh::world_navmeshes(&scene);
    for level in collision_levels(&scene) {
      if let Some(entry) = library.level(&level) {
        let original = read_original(Path::new(&entry.background)).expect("level");
        o.originals.insert(level, Arc::new(original));
      }
    }
    if let Some(entry) = library.level(&o.replaces) {
      o.region_trees = entry.region_trees.clone();
    }
    scene.begin_frame();
    export_level(&scene, Some(&data), &o).expect("exported");
    let (theirs, mut ours) = (export_texts(&export), export_texts(&tmp.join(project)));
    // the level's code for what OpenGOAL's builder does not write (level_login_code), called by the
    // level's definition: added since; a code file of nothing else, and its line in the DGO, when
    // theirs had none
    for text in ours.values_mut() {
      for start in [
        ";; its nav meshes, laid out",
        ";; What OpenGOAL's level builder does not write in the level",
      ] {
        if let Some(i) = text.find(start) {
          text.truncate(i);
        }
      }
    }
    let obs = format!("goal_src/jak3/levels/{project}/{project}-obs.gc");
    if !theirs.contains_key(&obs) {
      ours.remove(&obs);
      let gd = format!("custom_assets/jak3/levels/{project}/{project}.gd");
      if let Some(t) = ours.get_mut(&gd) {
        *t = t.replace(&format!("  \"{project}-obs.o\"\n"), "");
      }
    }
    assert_eq!(
      theirs.keys().collect::<Vec<_>>(),
      ours.keys().collect::<Vec<_>>(),
      "{project}: the files"
    );
    for (path, text) in &theirs {
      // the README: rewritten since (whozghiar/jak-project, the steps by hand)
      if path == "README.md" {
        continue;
      }
      // the editor's name in the comments: renamed since
      let text = text.replace("the OpenGOAL Level Editor", "Jak Level Editor");
      // the bounce on foot: Jak standing still goes up a hair, allowed since
      let text = text.replace("(>= 0.0 (vector-dot", "(>= 2048.0 (vector-dot");
      // the sky and the city's mood of a new level: added since
      let text = text.replace(
        ":mood-func 'update-mood-default",
        ":mood-func 'update-mood-ctyindb ;; the city's: the level's lights lit as the editor shows them",
      );
      // the level's code called when the game births it: added since
      let text = text.replace(
        ":callback-list '()",
        &format!(
          ":callback-list '((33 . {project}-login)) ;; what the level builder does not write (the level's code)"
        ),
      );
      // the region that does nothing, which OpenGOAL's builder needs: added since
      let regions_added = !text.contains("region_trees");
      let ours: String = ours[path]
        .lines()
        .filter(|l| !l.contains(":level-flags (level-flags sky)"))
        .filter(|l| {
          !(regions_added
            && (l.contains("no region of its own") || l.starts_with("  \"region_trees\": ")))
        })
        .map(|l| format!("{l}\n"))
        .collect();
      if let Some(d) = first_difference(&text, &ours) {
        panic!("{project}/{path}: {d}");
      }
    }
    // the .glb: the same nodes with the same extras
    let glb = |dir: &Path| {
      glb_extras(&read_glb_json(
        &dir.join(format!("custom_assets/jak3/levels/{project}/{project}.glb")),
      ))
    };
    let (theirs, mut ours) = (glb(&export), glb(&tmp.join(project)));
    // the collision of the decor colliding by its shape written apart since (the builder made it
    // from the decor, from both sides with double_sided): the element drawn without it
    ours
      .retain(|name, _| !name.contains("-collision-shape-") && !name.ends_with("-collision-span"));
    assert_eq!(theirs.len(), ours.len(), "{project}: the .glb's nodes");
    for (name, e) in &theirs {
      let mut e = e.clone();
      if e.get("double_sided").is_some() {
        let invisible = e["set_invisible"] == 1;
        e = json!({"set_collision": 1, "ignore": 1});
        if invisible {
          e["set_invisible"] = 1.into();
        }
      }
      assert_eq!(Some(&e), ours.get(name), "{project}: the extras of {name}");
    }
    compared += 1;
  }
  eprintln!("projects compared with the C++ editor's exports: {compared}");
}

#[test]
fn gameplay_markers_exported() {
  // a floor, Jak's start, a checkpoint facing +x with its flag (the editor's only), a death zone
  let tmp = temp_dir("game-export-gameplay");
  let mut s = Scene::new();
  s.add(render("floor", make_floor(-1), 0, Vec3::new(0.0, 0.0, 0.0)));
  let marker = |s: &mut Scene, name: &str, etype: &str, at: Vec3, yaw: f32| {
    let mut a = Node {
      name: name.into(),
      kind: NodeKind::Actor,
      extras: json!({"etype": etype}),
      ..Default::default()
    };
    a.local.t = at;
    a.local.r = Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), yaw);
    let id = s.add(a);
    s.add(render(
      &format!("{name}-flag"),
      make_box("flag", -1),
      id,
      Vec3::default(),
    ));
  };
  marker(&mut s, "jak-start", K_START, Vec3::new(3.0, 1.0, 4.0), 0.0);
  marker(
    &mut s,
    "checkpoint",
    K_CHECKPOINT,
    Vec3::new(20.0, 1.0, 0.0),
    std::f32::consts::FRAC_PI_2,
  );
  let mut zone = render(
    "death-zone",
    make_box("zone", -1),
    0,
    Vec3::new(0.0, -30.0, 0.0),
  );
  zone.extras = json!({"og_decor": {"collision": {"kind": "invisible", "event": "endlessfall"}}});
  s.add(zone);
  // the start the export proposes (its README, its answer): the flag's
  assert_eq!(export_start_point(&mut s, None), Vec3::new(3.0, 1.0, 4.0));
  s.begin_frame();
  let out = tmp.join("export");
  let o = LevelExportOptions {
    name: "my-cp".into(),
    nickname: default_nickname("my-cp"),
    iso_name: default_iso_name("my-cp"),
    folder: from_path(&out),
    start: Vec3::new(100.0, 100.0, 100.0),
    ..Default::default()
  };
  let r = export_level(&s, None, &o).expect("exported");
  let dir = out.join("custom_assets/jak3/levels/my-cp");
  let info = read_text(&dir.join("level-info.gc"));
  let code = read_text(&out.join("goal_src/jak3/levels/my-cp/my-cp-obs.gc"));
  let jsonc = read_text(&dir.join("my-cp.jsonc"));
  // the start where its flag is, a continue point for the checkpoint, Jak facing +x there
  assert!(
    info.contains(":name \"my-cp-start\"")
      && info.contains("(static-vectorm 3.0 1.0 4.0)")
      && info.contains(":name \"my-cp-checkpoint-1\"")
      && info.contains("(static-vectorm 20.0 1.5 0.0)")
      && info.contains("4 0 #x5a82 0 #x5a82"),
    "{info}"
  );
  // its actor makes it Jak's continue point when he passes by
  assert!(
    code.contains("(set-continue! *game-info* \"my-cp-checkpoint-1\" #f)")
      && jsonc.contains("my-cp-checkpoint-continue")
      && r.behaviors == 1,
    "{code}"
  );
  // what OpenGOAL's builder does not write, set by the level's code when the game births it: its
  // visibility (no translucent surface here, no nav mesh)
  assert!(
    info.contains(":callback-list '((33 . my-cp-login))")
      && code.contains("(defun my-cp-login ((lev level))")
      && code.contains("(set! (-> bsp visible-list-length) 2048)")
      && !code.contains("drawable-tree-tfrag-trans")
      && !code.contains("nav-words"),
    "{code}"
  );
  // the flags are not in the level; the death zone is, invisible, an endless fall (event 2)
  let extras = glb_extras(&read_glb_json(&dir.join("my-cp.glb")));
  assert!(
    !extras.keys().any(|k| k.contains("flag"))
      && extras["death-zone"]["set_invisible"] == 1
      && extras
        .values()
        .any(|e| e["collide_event"] == 2 && e["set_invisible"] == 1),
    "{extras:?}"
  );
  let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn creatures_on_their_nav_mesh() {
  // a grunt on the nav mesh of a floor, another off it: the first written with its nav mesh's id,
  // the second left out with a note
  let data = real_data();
  let tmp = temp_dir("game-export-creatures");
  let mut s = Scene::new();
  s.add(render("floor", make_floor(-1), 0, Vec3::new(0.0, 0.0, 0.0)));
  for (name, at) in [
    ("grunt-1", Vec3::new(0.0, 0.0, 0.0)),
    ("grunt-2", Vec3::new(50.0, 0.0, 0.0)),
  ] {
    let mut a = Node {
      name: name.into(),
      kind: NodeKind::Actor,
      extras: json!({"etype": "grunt", K_SPAWN: true}),
      ..Default::default()
    };
    a.local.t = at;
    s.add(a);
  }
  s.begin_frame();
  let mesh = crate::navmesh::NavMeshData {
    origin: Vec3::default(),
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
  let out = tmp.join("export");
  let o = LevelExportOptions {
    name: "my-ai".into(),
    nickname: default_nickname("my-ai"),
    iso_name: default_iso_name("my-ai"),
    folder: from_path(&out),
    navmeshes: vec![mesh],
    ..Default::default()
  };
  let r = export_level(&s, Some(&data), &o).expect("exported");
  let jsonc = read_jsonc(&out.join("custom_assets/jak3/levels/my-ai/my-ai.jsonc"));
  let actors = jsonc["actors"].as_array().unwrap();
  let grunts: Vec<&Value> = actors.iter().filter(|a| a["etype"] == "grunt").collect();
  assert_eq!(grunts.len(), 1, "{actors:?}");

  let base = if o.base_id != 0 {
    o.base_id
  } else {
    K_DEFAULT_BASE_ID
  };

  assert_eq!(
    grunts[0]["lump"]["nav-mesh-actor"],
    json!(["uint32", base + 9000])
  );
  assert!(
    format!("{:?}", r.notes).contains("grunt-2"),
    "{:?}",
    r.notes
  );
  // its code and its model's art group in the level's DGO
  let gd = read_text(&out.join("custom_assets/jak3/levels/my-ai/my-ai.gd"));
  assert!(gd.contains("grunt-ag.go"), "{gd}");
  let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn objects_needing_their_own_level() {
  // the volcano's breaking ground gets the level volcanoa by its name: in a level of its own it
  // is left out with a note (the game would die of it); a level replacing volcanoa keeps it
  let data = real_data();
  assert_eq!(data.needs_levels("vol-break-ground"), ["volcanoa"]);
  let tmp = temp_dir("game-export-needs-level");
  let mut s = Scene::new();
  s.add(render("floor", make_floor(-1), 0, Vec3::new(0.0, 0.0, 0.0)));
  s.add(Node {
    name: "vol-break-ground-1".into(),
    kind: NodeKind::Actor,
    extras: json!({"etype": "vol-break-ground"}),
    ..Default::default()
  });
  // and a battle (its creatures' models not in the level): always left out
  s.add(Node {
    name: "battle-92".into(),
    kind: NodeKind::Actor,
    extras: json!({"etype": "battle", "lump": {"extra-id": ["int32", 7]}}),
    ..Default::default()
  });
  s.begin_frame();
  for (replaces, kept) in [("", false), ("volcanoa", true)] {
    let out = tmp.join(format!("export-{kept}"));
    let o = LevelExportOptions {
      name: "my-lava".into(),
      nickname: default_nickname("my-lava"),
      iso_name: default_iso_name("my-lava"),
      folder: from_path(&out),
      replaces: replaces.into(),
      ..Default::default()
    };
    let r = export_level(&s, Some(&data), &o).expect("exported");
    let jsonc = read_jsonc(&out.join("custom_assets/jak3/levels/my-lava/my-lava.jsonc"));
    let there = jsonc["actors"]
      .as_array()
      .unwrap()
      .iter()
      .any(|a| a["etype"] == "vol-break-ground");
    assert_eq!(there, kept, "{:?}", r.notes);
    assert!(
      !jsonc["actors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["etype"] == "battle")
        && format!("{:?}", r.notes).contains("battle-92"),
      "{:?}",
      r.notes
    );
    assert_eq!(
      format!("{:?}", r.notes).contains("export.note.needs_level"),
      !kept
    );
  }
  let _ = std::fs::remove_dir_all(&tmp);
}

// the decor's coordinates as the level builder packs them: within 4.4 km of the origin all kept;
// farther, the ones its check (in float) would stop the build on moved by a few units to ones it
// packs, never more than 2 mm within 25 km
#[test]
fn coordinates_the_builder_packs() {
  use crate::io::gltf::{packable, packs};
  let (mut far, mut moved) = (0, 0);
  for i in 0..400_000 {
    let x = -4000.0 + i as f32 * 0.0725;
    let y = packable(x);
    assert!(packs(y), "{x} m cannot be packed (moved to {y})");
    if x.abs() < 4380.0 {
      assert_eq!(x, y);
    }
    if x > 4400.0 {
      far += 1;
      moved += (x != y) as usize;
      assert!((y - x).abs() <= 8.0 / 4096.0 + 1e-3, "{x} moved to {y}");
    }
  }
  // the check fails often enough there that it matters (the temple of Jak 3 stopped its build)
  assert!(moved * 100 > far, "{moved} of {far}");
}
