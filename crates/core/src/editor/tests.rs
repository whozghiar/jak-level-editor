//! The editor's tests (src/app/selftest.cpp): the level opened (the terrain locked, the actors'
//! models), copy and paste, a project's round trip, a level with the game's data (its companions,
//! layers, story, the levels added), the ground's strips, the ocean, groups; the editor driven by
//! its calls; and on the user's extracted game when there is one (library/jak3/library.json).

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use super::protocol::{ProtocolState, encode_mesh, encode_texture, materials_json};
use super::*;
use crate::history::NodeChange;
use crate::io::asset_library::{AssetLibrary, game_folder};
use crate::io::game_data::{GameData, OceanMap};
use crate::io::prefabs::place_prefab;
use crate::io::test_library::{
  make_box, real_library, temp_dir, write_library, write_story_library,
};
use crate::math::{Mat4, Vec3, length};
use crate::scene::story::{StoryView, apply_story, story_changes};
use crate::scene::{AlphaMode, BlendMode, Material, Mesh, Node, NodeKind, Primitive, Scene};

fn named<'a>(s: &'a Scene, name: &str) -> Option<&'a Node> {
  s.nodes.iter().find(|n| n.name == name)
}

fn library_of(root: &Path) -> AssetLibrary {
  let mut lib = AssetLibrary::default();
  lib.scan(&game_folder(root.to_str().unwrap())).unwrap();
  lib
}

/// an editor on its own (its jobs run in the call), the preferences left as they are
fn editor() -> Editor {
  let mut ed = Editor::new();
  ed.write_prefs = false;
  ed
}

#[test]
fn level_opened_copy_paste_and_project() {
  let tmp = temp_dir("editor-level");
  let root = tmp.join("library");
  assert!(write_library(&root));
  let library = library_of(&root);
  let level = library.level("lev").unwrap();

  // the level
  let mut scene = Scene::new();
  open_level(&mut scene, &library, None, level, true).unwrap();
  let actor = scene
    .nodes
    .iter()
    .find(|n| n.kind == NodeKind::Actor)
    .unwrap()
    .clone();
  let collision = scene
    .nodes
    .iter()
    .find(|n| n.kind == NodeKind::Collision)
    .unwrap()
    .id;
  let floor = named(&scene, "floor-0").unwrap().id;
  let instances: Vec<u32> = scene
    .nodes
    .iter()
    .filter(|n| n.name.starts_with("rock.mb-"))
    .map(|n| n.id)
    .collect();
  assert_eq!(scene.settings.level, "lev", "level opened: its name");
  let find = |id| scene.find(id).unwrap();
  assert!(
    find(floor).locked
      && instances.len() == 2
      && !find(instances[0]).locked
      && Arc::ptr_eq(
        find(instances[0]).mesh.as_ref().unwrap(),
        find(instances[1]).mesh.as_ref().unwrap()
      ),
    "level: terrain locked, decor instances selectable and sharing their mesh"
  );
  let model = scene
    .children(actor.id)
    .into_iter()
    .any(|c| find(c).extras["og_actor_visual"] == "crate-krimson");
  assert!(
    actor.extras["etype"] == "crate"
      && actor.extras["aid"] == 123
      && model
      && length(actor.local.t - Vec3::new(1.0, 2.0, 3.0)) < 1e-5,
    "level: actor with its data, position and model"
  );
  assert!(
    actor.extras["og_origin"]
      .as_array()
      .is_some_and(|o| o.len() == 7),
    "level: where its actors are in the game"
  );
  assert_eq!(
    find(collision).mesh.as_ref().unwrap().prims[0].tri_pat,
    vec![4660u32; 2],
    "level: collision kept with its surface"
  );

  // copy and paste
  let change = NodeChange::new(&scene);
  let before = scene.nodes.len();
  let pasted = {
    let clip = copy_nodes(&scene, &[instances[0], actor.id]);
    paste_nodes(&mut scene, &clip)
  };
  let (copy, actor_copy, original) = (
    scene.find(pasted[0]).unwrap(),
    scene.find(pasted[1]).unwrap(),
    scene.find(instances[0]).unwrap(),
  );
  assert!(
    Arc::ptr_eq(copy.mesh.as_ref().unwrap(), original.mesh.as_ref().unwrap())
      && copy.parent == original.parent
      && copy.name != original.name
      && length(scene.world(copy).col3(3) - scene.world(original).col3(3)) < 1e-5,
    "paste: a copy in place, sharing the mesh, under the same parent"
  );
  assert!(
    actor_copy.kind == NodeKind::Actor
      && actor_copy.extras.get("aid").is_none()
      && scene.children(actor_copy.id).len() == 1
      && actor_copy.name == "crate-1"
      && actor_copy.extras["lump"]["name"] == "crate-1",
    "paste: an actor copy with its model, without the original's id, named like its type"
  );
  let e = change.commit(&scene, "paste", None);
  (e.undo)(&mut scene);
  assert_eq!(scene.nodes.len(), before, "paste undone");
  (e.redo)(&mut scene);
  assert_eq!(
    scene.nodes.len(),
    before + 3,
    "paste redone (the instance, the actor and its model)"
  );

  // project round trip, through save_project and open_project
  let glow = scene.add_material(Material {
    name: "glow".into(),
    alpha: AlphaMode::Blend,
    blend: BlendMode::Add,
    cutoff: 0.15, // the alpha test of the PS2, kept by blended surfaces too
    ..Default::default()
  });
  let mut n = Node {
    name: "glow".into(),
    kind: NodeKind::Render,
    mesh: Some(make_box("glow", glow)),
    ..Default::default()
  };
  n.local.s = Vec3::new(3.0, 1.0, 0.5); // stretched along x, flattened along z
  scene.add(n);
  let project = tmp.join("project.glb");
  let project = project.to_str().unwrap();
  save_project(&scene, project).unwrap();
  let mut back = Scene::new();
  open_project(&mut back, project).unwrap();
  let kinds = |s: &Scene| {
    let mut k = [0usize; 5];
    for n in &s.nodes {
      k[n.kind as usize] += 1;
    }
    k
  };
  assert!(
    kinds(&scene) == kinds(&back) && back.settings.level == "lev",
    "project: saved and opened again, same nodes and level"
  );
  let rocks: BTreeSet<usize> = back
    .nodes
    .iter()
    .filter(|x| x.name.starts_with("rock.mb") && x.kind == NodeKind::Render && x.parent != 0)
    .map(|x| Arc::as_ptr(x.mesh.as_ref().unwrap()) as usize)
    .collect();
  let g = named(&back, "glow").unwrap();
  let m = &back.materials[g.mesh.as_ref().unwrap().prims[0].material as usize];
  let b = back.world_bounds(g);
  assert!(
    length(g.local.s - Vec3::new(3.0, 1.0, 0.5)) < 1e-4
      && (b.hi.x - b.lo.x - 6.0).abs() < 1e-3
      && (b.hi.z - b.lo.z - 1.0).abs() < 1e-3,
    "project: the scale of a stretched element kept (and its bounds)"
  );
  let collision_back = back
    .nodes
    .iter()
    .find(|x| x.kind == NodeKind::Collision)
    .unwrap();
  assert!(
    rocks.len() == 1
      && named(&back, "floor-0").unwrap().locked
      && collision_back.mesh.as_ref().unwrap().prims[0].tri_pat == vec![4660u32; 2]
      && back
        .nodes
        .iter()
        .any(|x| x.kind == NodeKind::Actor && x.extras["aid"] == 123)
      && m.blend == BlendMode::Add
      && (m.cutoff - 0.15).abs() < 1e-6,
    "project: instancing, locks, collision surfaces, actor data, additive material and its alpha test kept"
  );
  let empty = tmp.join("empty.glb");
  save_project(&Scene::new(), empty.to_str().unwrap()).unwrap();
  let mut empty_back = Scene::new();
  open_project(&mut empty_back, empty.to_str().unwrap()).unwrap();
  assert!(
    empty_back.nodes.is_empty(),
    "project: an empty document saved and opened again"
  );
  assert!(
    open_project(&mut Scene::new(), "x.txt").is_err(),
    "project: only a .glb"
  );
}

#[test]
fn level_with_game_data_and_levels_added() {
  let tmp = temp_dir("editor-story");
  assert!(write_story_library(&tmp));
  let data = GameData::load(&tmp.join("game-data.json")).unwrap();
  let lib = library_of(&tmp);
  let town_entry = lib.level("town").unwrap();
  let mut town = Scene::new();
  open_level(&mut town, &lib, Some(&data), town_entry, true).unwrap();
  let actors: Vec<&str> = town
    .nodes
    .iter()
    .filter(|n| n.kind == NodeKind::Actor)
    .map(|n| n.name.as_str())
    .collect();
  assert!(
    !actors
      .iter()
      .any(|a| a.starts_with("grunt") || a.starts_with("jak-highres"))
      && actors.iter().any(|a| a.starts_with("part-spawner"))
      && named(&town, "crate-1").is_some()
      && named(&town, "statue-broken-6").is_some(),
    "level with game data: creatures and cutscene characters left out, its particle effects kept"
  );
  let pool = named(&town, "pool-5").unwrap().id;
  assert!(
    town
      .children(pool)
      .into_iter()
      .any(|c| town.find(c).unwrap().extras["og_actor_visual"] == "water-anim-test-pool"),
    "water actor: the model of its look"
  );
  let hut = town
    .nodes
    .iter()
    .find(|x| x.extras["og_companion"] == "hut")
    .unwrap();
  assert!(
    town
      .nodes
      .iter()
      .any(|x| x.extras["og_layer"] == "lmission")
      && !town.children(hut.id).is_empty()
      && named(&town, "lcrate-1").is_some(),
    "level with game data: its companion and the layer of its mission under their groups"
  );
  // the story: step 1 (the mission under way), step 4 (the mission over)
  let shown = |s: &Scene, name: &str| named(s, name).is_some_and(|x| s.effectively_visible(x));
  let mut view = StoryView {
    step: 1,
    ..Default::default()
  };
  apply_story(&mut town, Some(&data), &view);
  let start = ["crate-1", "rock.mb", "lcrate-1", "hut"]
    .iter()
    .all(|n| shown(&town, n))
    && !shown(&town, "statue-broken-6")
    && !shown(&town, "rubble.mb");
  view.step = 4;
  apply_story(&mut town, Some(&data), &view);
  let after = !shown(&town, "crate-1")
    && shown(&town, "statue-broken-6")
    && !shown(&town, "rock.mb")
    && shown(&town, "rubble.mb")
    && !shown(&town, "lcrate-1")
    && shown(&town, "hut");
  assert!(start && after, "story: the level at two steps");
  assert_eq!(
    story_changes(&town, &data, ""),
    vec![4],
    "story: the steps where the level changes"
  );
  // the shell split in pieces: the terrain locked, the others free, around their own origin
  let floor = named(&town, "town-floor").unwrap();
  let wall = named(&town, "town-wall").unwrap();
  let wb = wall.mesh.as_ref().unwrap().bounds();
  assert!(
    floor.locked
      && !wall.locked
      && length(wall.local.t - Vec3::new(20.0, 0.0, 10.0)) < 1e-4
      && wb.center().x.abs() < 1e-4
      && wb.lo.y.abs() < 1e-4,
    "level pieces: the terrain locked, a piece of the shell selectable, its origin at its base"
  );
  // a copied actor is named like the others of its type, in its data too
  let c = named(&town, "crate-1").unwrap().id;
  let copies = {
    let clip = copy_nodes(&town, &[c]);
    paste_nodes(&mut town, &clip)
  };
  let copy = town.find(copies[0]).unwrap();
  assert!(
    copy.name == "crate-2" && copy.extras["lump"]["name"] == "crate-2",
    "paste: an actor copy named <type>-<number>, in its data too"
  );

  // several levels: port opened, town added at its place with its companion and its mission's
  // layer under its group
  let mut doc = Scene::new();
  open_level(
    &mut doc,
    &lib,
    Some(&data),
    lib.level("port").unwrap(),
    true,
  )
  .unwrap();
  let group = add_level(&mut doc, &lib, Some(&data), town_entry, true).unwrap();
  let in_group = |key: &str, v: &str| {
    doc
      .nodes
      .iter()
      .find(|x| x.extras[key] == v)
      .is_some_and(|x| doc.is_ancestor(group, x.id))
  };
  let wall_added = named(&doc, "town-wall").unwrap();
  let alone = named(&town, "town-wall").unwrap();
  assert!(
    doc.find(group).unwrap().extras["og_level"] == "town"
      && in_group("og_companion", "hut")
      && in_group("og_layer", "lmission")
      && length(doc.world_bounds(wall_added).center() - town.world_bounds(alone).center()) < 1e-4
      && (doc
        .world_bounds(named(&doc, "port-wall").unwrap())
        .center()
        .x
        - 120.0)
        .abs()
        < 1e-3,
    "levels: one added at its place, its companion and mission layer under its group"
  );
  let mut bare = Scene::new();
  open_level(&mut bare, &lib, Some(&data), town_entry, false).unwrap();
  assert!(
    !bare.nodes.iter().any(|x| x.kind == NodeKind::Actor)
      && bare.nodes.iter().any(|x| x.kind == NodeKind::Render),
    "levels: opened without their actors, the decor alone"
  );
}

#[test]
fn raised_strips_are_not_ground() {
  // a wall's top 150 m long, 8 m wide, 17 m over a large ground: not ground; a strip of the
  // ground: it is
  let quad = |w: f32, h: f32| {
    let mut mesh = Mesh::new();
    mesh.prims.push(Primitive {
      pos: vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(w, 0.0, 0.0),
        Vec3::new(w, 0.0, h),
        Vec3::new(0.0, 0.0, h),
      ],
      idx: vec![0, 1, 2, 0, 2, 3],
      ..Default::default()
    });
    Arc::new(mesh)
  };
  let mut ws = Scene::new();
  let mut ground = |x0: f32, x1: f32, z0: f32, z1: f32, y: f32| {
    let mut n = Node {
      kind: NodeKind::Render,
      mesh: Some(quad(x1 - x0, z1 - z0)),
      locked: true,
      extras: json!({"og_terrain": true}),
      ..Default::default()
    };
    n.local.t = Vec3::new(x0, y, z0);
    ws.add(n)
  };
  let yard = ground(0.0, 200.0, 0.0, 200.0, 0.0);
  let top = ground(0.0, 150.0, 210.0, 218.0, 17.0);
  let strip = ground(0.0, 64.0, 220.0, 227.0, 0.0);
  ws.begin_frame();
  free_raised_strips(&mut ws, 0);
  assert!(
    !ws.find(top).unwrap().locked
      && ws.find(strip).unwrap().locked
      && ws.find(yard).unwrap().locked,
    "level pieces: a wall's top standing over the ground is not ground, a strip of the ground is"
  );
}

#[test]
fn ocean() {
  let map = OceanMap {
    cell: 96.0,
    size: 2, // 64 squares of 3 m a side
    ..Default::default()
  };
  let slab = |x0: f32, x1: f32, z0: f32, z1: f32, y: f32| Primitive {
    pos: vec![
      Vec3::new(x0, y, z0),
      Vec3::new(x1, y, z0),
      Vec3::new(x1, y, z1),
      Vec3::new(x0, y, z1),
    ],
    idx: vec![0, 1, 2, 0, 2, 3],
    ..Default::default()
  };
  let mut mesh = Mesh::new();
  mesh.prims = vec![
    slab(30.0, 60.0, 30.0, 60.0, 2.0),
    slab(90.0, 120.0, 90.0, 120.0, -1.0),
  ];
  mesh.prims.push(Primitive {
    pos: vec![
      Vec3::new(0.0, 0.0, 150.0),
      Vec3::new(90.0, 0.0, 150.0),
      Vec3::new(90.0, 20.0, 150.0),
      Vec3::new(0.0, 20.0, 150.0),
    ],
    idx: vec![0, 1, 2, 0, 2, 3],
    ..Default::default()
  });
  let mut land = Scene::new();
  land.add(Node {
    kind: NodeKind::Render,
    mesh: Some(Arc::new(mesh)),
    ..Default::default()
  });
  let mask = ocean_land(&map, 0.0, &land);
  let covered: usize = mask.iter().map(|&v| v as usize).sum();
  assert!(
    mask.len() == 64 * 64 && covered == 100 && mask[15 * 64 + 15] != 0 && mask[35 * 64 + 35] == 0,
    "ocean: the squares under a slab above the water are land (not under one below it, nor a wall)"
  );

  let side = 64;
  let block = |m: &mut Vec<u8>, x0: usize, x1: usize, z0: usize, z1: usize| {
    if m.is_empty() {
      m.resize(side * side, 0);
    }
    for z in z0..=z1 {
      for x in x0..=x1 {
        m[z * side + x] = 1;
      }
    }
  };
  let (mut doc, mut placed, mut moved, mut around) = (vec![], vec![], vec![], vec![]);
  block(&mut doc, 20, 29, 20, 29);
  block(&mut placed, 20, 29, 20, 29);
  block(&mut placed, 30, 63, 0, 63); // the city on, from the document's land
  block(&mut placed, 0, 9, 50, 59); // an island
  block(&mut moved, 40, 49, 40, 49);
  block(&mut moved, 0, 15, 30, 45); // land of a model moved away: not taken
  block(&mut around, 0, 15, 0, 15); // a neighbor
  block(&mut around, 0, 19, 47, 47); // a wall in the sea, one square thick
  let out = ocean_land_around(side, &[doc.clone()], &[around], &[moved, placed], &[]);
  let at = |x: usize, z: usize| out[z * side + x] != 0;
  assert!(
    at(40, 10) && at(8, 8) && !at(25, 25) && !at(5, 55) && !at(10, 47) && !at(8, 38),
    "ocean: the city seen from afar hides it beyond the level (not an island, not a moved model), \
     a neighbor too, not a thin wall nor the level's own box"
  );
  // a pond the city closes in (water of the map under it) goes; the sea open to the edge, the
  // water of the level's own box stay
  let mut pond_city = vec![];
  block(&mut pond_city, 20, 29, 20, 29);
  block(&mut pond_city, 30, 63, 0, 63);
  for z in 20..=29 {
    for x in 45..=54 {
      pond_city[z * side + x] = 0; // a pond 30 m across in the city
    }
  }
  let open = ocean_land_around(side, &[doc.clone()], &[], &[pond_city.clone()], &[]);
  let closed = ocean_land_around(side, &[doc], &[], &[pond_city], &vec![1; side * side]);
  let shut = |x: usize, z: usize| closed[z * side + x] != 0;
  assert!(
    open[25 * side + 50] == 0
      && shut(50, 25)
      && shut(40, 10)
      && !shut(10, 30)
      && !shut(25, 25)
      && !shut(5, 55),
    "ocean: the water the land around closes in is left out, the sea open to the edge and the \
     level's own kept"
  );
}

#[test]
fn groups() {
  // a copy of a group is a group of its own; a prefab placed forgets the groups it was made of
  let mut d = Scene::new();
  let mut a = Node {
    name: "stall".into(),
    kind: NodeKind::Render,
    mesh: Some(make_box("stall", -1)),
    ..Default::default()
  };
  set_group_tag(&mut a, 1, "market", false);
  let ida = d.add(a.clone());
  let copies = {
    let clip = copy_nodes(&d, &[ida]);
    paste_nodes(&mut d, &clip)
  };
  let mut pre = Scene::new();
  pre.add(a);
  let placed = place_prefab(&mut d, &pre, &Mat4::identity(), &mut HashMap::new());
  let copy = d.find(copies[0]).unwrap();
  assert!(
    group_tag(copy) == 2
      && group_tag(d.find(ida).unwrap()) == 1
      && copy.extras["og_group"]["name"] == "market"
      && group_tag(d.find(placed[0]).unwrap()) == 0
      && new_group_id(&d) == 3,
    "groups: a copy of a group is a new group of the same name, a prefab placed forgets the groups \
     it was made of"
  );
  // the editor's: grouped, selected whole, renamed, ungrouped, each undoable
  let mut ed = editor();
  ed.scene = d;
  ed.select_ids(&[ida, copies[0]], selection::SelectMode::Replace);
  let g = ed.group_selection("pair");
  ed.select_only(ida);
  assert!(ed.select_whole(ida) && ed.selection.len() == 2 && ed.group_of(ida) == g);
  ed.rename_group(g, "duo");
  assert_eq!(ed.group_info(g).unwrap().name, "duo");
  ed.ungroup(g);
  assert!(ed.group_info(g).is_none());
  ed.call("undo", &json!({})).unwrap();
  assert_eq!(
    ed.group_info(g).unwrap().name,
    "duo",
    "groups: ungrouping undone"
  );
}

#[test]
fn editor_calls() {
  let tmp = temp_dir("editor-calls");
  assert!(write_story_library(&tmp));
  let mut ed = editor();
  ed.game_data = Some(
    GameData::load(&tmp.join("game-data.json"))
      .ok()
      .map(Arc::new),
  );
  ed.call("set_library", &json!({"path": tmp.to_str().unwrap()}))
    .unwrap();
  let levels = ed.call("list_levels", &json!({})).unwrap();
  let listed: Vec<&str> = levels["levels"]
    .as_array()
    .unwrap()
    .iter()
    .filter(|l| l["listed"] == true)
    .map(|l| l["name"].as_str().unwrap())
    .collect();
  assert_eq!(
    listed,
    ["hut", "port", "town"],
    "levels listed: the layer of a mission is not"
  );

  // a level opened (its job runs now: an editor on its own), the story, the ocean
  ed.call("open_level", &json!({"name": "town", "actors": true}))
    .unwrap();
  let st = ed.call("get_state", &json!({})).unwrap();
  assert!(
    st["document"]["open"] == true
      && st["document"]["title"] == "town"
      && st["document"]["unsaved"] == false,
    "a level opened: the document"
  );
  assert!(
    st["story"]["available"] == true && st["story"]["states"].as_array().unwrap().len() == 2,
    "the story bar: the level's two states"
  );
  assert!(
    st["ocean"]["mesh"].as_u64().unwrap() > 0
      && (st["ocean"]["height"].as_f64().unwrap() + 1.5).abs() < 1e-5,
    "the ocean of the level, at its height"
  );
  for k in [
    "projects_root",
    "recent_projects",
    "history",
    "selection",
    "clipboard_empty",
    "tool",
    "gizmo",
    "brush",
    "view",
    "loading",
    "remote_clients",
    "library_empty",
    "language",
    "log",
    "jobs",
  ] {
    assert!(st.get(k).is_some(), "get_state: {k}");
  }
  ed.call("set_story", &json!({"state": 2})).unwrap();
  let crate1 = named(&ed.scene, "crate-1").unwrap();
  assert!(
    ed.story.step == 4 && !ed.scene.effectively_visible(crate1),
    "the story bar's second state: after the mission"
  );

  // the selection: a level whole, an actor picked through its model, deleted and back
  ed.call("select_level", &json!({"group": 0})).unwrap();
  assert!(
    ed.selection.len() > 1,
    "select_level: the roots of the level opened"
  );
  let crate_id = named(&ed.scene, "statue-broken-6").unwrap().id;
  let at = ed.scene.world_of(crate_id).col3(3);
  ed.scene.begin_frame();
  let hit = ed
    .call(
      "pick",
      &json!({"origin": [at.x, at.y + 50.0, at.z], "dir": [0, -1, 0]}),
    )
    .unwrap();
  assert!(hit.is_object(), "pick: something under the ray");
  ed.call("select_only", &json!({"id": crate_id})).unwrap();
  let before = ed.scene.nodes.len();
  ed.call("delete_selection", &json!({})).unwrap();
  assert!(ed.scene.nodes.len() < before && ed.selection.is_empty());
  ed.call("undo", &json!({})).unwrap();
  assert_eq!(ed.scene.nodes.len(), before, "deleted, then undone");

  // moved by the viewport's gizmo, one undo step
  let w = Mat4::translate(Vec3::new(5.0, 0.0, 0.0)) * ed.scene.world_of(crate_id);
  ed.call(
    "transform_nodes",
    &json!({"ids": [crate_id], "worlds": [w.m], "label": "undo.move"}),
  )
  .unwrap();
  assert!(length(ed.scene.world_of(crate_id).col3(3) - w.col3(3)) < 1e-4);
  ed.call(
    "set_node",
    &json!({"id": crate_id, "name": "statue", "visible": false}),
  )
  .unwrap();
  assert!(ed.call("node", &json!({"id": crate_id})).unwrap()["name"] == "statue");
  ed.call("undo", &json!({})).unwrap();
  assert_eq!(
    ed.scene.find(crate_id).unwrap().name,
    "statue-broken-6",
    "set_node undone"
  );

  // a level added next to it, then removed: undoable, the story follows
  ed.call("add_level", &json!({"name": "port"})).unwrap();
  let added = ed
    .document_levels()
    .iter()
    .find(|l| l.name == "port")
    .map(|l| l.group);
  let group = added.expect("port added");
  assert!(
    ed.call("add_level", &json!({"name": "port"})).is_err(),
    "a level is in the document once"
  );
  ed.call("remove_level", &json!({"group": group})).unwrap();
  assert!(!ed.level_in_document("port"));
  ed.call("undo", &json!({})).unwrap();
  assert!(ed.level_in_document("port"), "removing a level undone");

  // saved as a project in <library>/projects/<name>/, opened again
  let r = ed.call("save", &json!({})).unwrap();
  assert_eq!(r["ask_name"], "my-town", "the first save asks for a name");
  let r = ed
    .call("save_project_as", &json!({"name": "My town"}))
    .unwrap();
  let path = r["path"].as_str().unwrap().to_string();
  assert!(
    path
      .replace('\\', "/")
      .ends_with("projects/My-town/My-town.glb")
      && Path::new(&path).is_file()
  );
  assert!(ed.call("get_state", &json!({})).unwrap()["document"]["unsaved"] == false);
  assert_eq!(ed.call("save", &json!({})).unwrap()["saved"], true);
  let nodes = ed.scene.nodes.len();
  ed.call("new_level", &json!({})).unwrap();
  assert!(ed.scene.nodes.is_empty() && ed.scene.settings.level == "custom-level");
  ed.call("open_project_file", &json!({"path": path}))
    .unwrap();
  assert!(
    ed.scene.nodes.len() == nodes && ed.level_in_document("port"),
    "the project opened again"
  );

  // the view and the gizmo, merged
  ed.call(
    "set_gizmo",
    &json!({"mode": "rotate", "snap": {"enabled": true}}),
  )
  .unwrap();
  ed.call("set_view", &json!({"grid": false, "nonsense": 3}))
    .unwrap();
  assert!(
    ed.gizmo.mode == GizmoMode::Rotate
      && ed.gizmo.snap.enabled
      && ed.gizmo.snap.translate == 0.5
      && !ed.view.grid
  );
  // the terrain brush on a document, off again; an unknown call refused
  assert_eq!(ed.call("start_terrain_brush", &json!({})), Ok(json!(true)));
  assert_eq!(ed.tool, ToolMode::Terrain);
  ed.call("stop_terrain_brush", &json!({})).unwrap();
  assert!(ed.tool == ToolMode::Select && ed.call("nope", &json!({})).is_err());
}

#[test]
fn change_notification() {
  let events: Arc<std::sync::Mutex<Vec<(String, Value)>>> = Arc::default();
  let seen = events.clone();
  let mut ed = editor();
  ed.notify = Some(Arc::new(move |e, p| {
    seen.lock().unwrap().push((e.into(), p))
  }));
  ed.scene.add(Node {
    name: "a".into(),
    kind: NodeKind::Render,
    mesh: Some(make_box("a", -1)),
    ..Default::default()
  });
  ed.apply_story_view(); // as a document opened has it
  ed.call("get_state", &json!({})).unwrap();
  assert!(events.lock().unwrap().is_empty(), "a query changes nothing");
  ed.call("select_only", &json!({"id": 1})).unwrap();
  let last = events.lock().unwrap().last().cloned().unwrap();
  assert!(
    last.0 == "jle:changed"
      && last.1["selection"] == true
      && last.1["scene"] == false
      && last.1["state"] == true
  );
  ed.call("delete_selection", &json!({})).unwrap();
  let last = events.lock().unwrap().last().cloned().unwrap();
  assert!(last.1["scene"] == true && last.1["document"] == false);
  ed.call("new_level", &json!({})).unwrap();
  let last = events.lock().unwrap().last().cloned().unwrap();
  assert!(last.1["document"] == true);
  let n = events.lock().unwrap().len();
  ed.call("select_only", &json!({"id": 0})).unwrap();
  assert_eq!(events.lock().unwrap().len(), n, "nothing changed: no event");

  // the protocol: a full answer, then only what changed (a selected flag too), the ids removed
  let mut p = ProtocolState::default();
  let id = ed.scene.add(Node {
    name: "b".into(),
    kind: NodeKind::Render,
    mesh: Some(make_box("b", -1)),
    ..Default::default()
  });
  ed.scene.add(Node::default());
  let full = p.encode_scene(ed.scene_snapshot(), true);
  assert_eq!(full.len(), 16 + 2 * 84 + 4);
  assert_eq!(
    p.encode_scene(ed.scene_snapshot(), false).len(),
    20,
    "nothing changed"
  );
  ed.select_only(id);
  let delta = p.encode_scene(ed.scene_snapshot(), false);
  assert!(
    delta.len() == 16 + 84 + 4 && delta[16 + 9] & 32 != 0,
    "the selected flag changed"
  );
  ed.scene.remove(&[id]);
  let delta = p.encode_scene(ed.scene_snapshot(), false);
  assert!(delta.len() == 24 && u32::from_le_bytes(delta[20..24].try_into().unwrap()) == id);
}

// ---- the user's extracted game (skipped without it)

#[test]
fn real_level_through_the_editor() {
  let Some(root) = real_library() else { return };
  let mut ed = editor();
  ed.set_library(root.to_str().unwrap());
  let t = std::time::Instant::now();
  ed.call("open_level", &json!({"name": "ctyinda"})).unwrap();
  let opened = t.elapsed();
  assert!(ed.scene.nodes.len() > 100 && ed.scene.settings.level == "ctyinda");
  let snap = ed.scene_snapshot();
  let (count, meshes) = (snap.records.len(), snap.meshes.clone());
  let mut p = ProtocolState::default();
  let scene = p.encode_scene(snap, true);
  assert_eq!(scene.len(), 16 + count * 84 + 4, "scene: every node");
  let mut bytes = 0;
  for m in meshes.values() {
    let e = encode_mesh(m);
    let expected: usize = 12
      + m
        .prims
        .iter()
        .map(|p| {
          let n = p.pos.len();
          16 + n * 24
            + if p.uv.len() == n && n > 0 { n * 8 } else { 0 }
            + if p.col.len() == n && n > 0 { n * 4 } else { 0 }
            + p.idx.len() * 4
        })
        .sum::<usize>();
    assert_eq!(e.len(), expected, "mesh {}", m.name);
    bytes += e.len();
  }
  for t in &ed.scene.textures {
    assert_eq!(
      encode_texture(t).len(),
      8 + (t.width * t.height * 4) as usize
    );
  }
  let mats = materials_json(&ed.scene);
  assert_eq!(
    mats["materials"].as_array().unwrap().len(),
    ed.scene.materials.len()
  );
  eprintln!(
    "ctyinda: opened in {:?}, {} nodes, {} meshes ({:.1} MB), {} textures, ocean {}",
    opened,
    count,
    meshes.len(),
    bytes as f64 / 1e6,
    ed.scene.textures.len(),
    ed.ocean_mesh().is_some()
  );
  // a neighbor added, saved as a project in a temp folder, opened again
  let neighbor = ed.document_neighbors().first().cloned();
  if let Some(n) = &neighbor {
    ed.call("add_level", &json!({"name": n})).unwrap();
    assert!(ed.level_in_document(n), "neighbor {n} added");
  }
  let tmp = temp_dir("editor-real");
  let path = tmp.join("real.glb");
  ed.save_project_files(path.to_str().unwrap()).unwrap();
  let nodes = ed.scene.nodes.len();
  ed.call(
    "open_project_file",
    &json!({"path": path.to_str().unwrap()}),
  )
  .unwrap();
  assert_eq!(ed.scene.nodes.len(), nodes, "the project opened again");
  if let Some(n) = neighbor {
    assert!(ed.level_in_document(&n));
  }
}
