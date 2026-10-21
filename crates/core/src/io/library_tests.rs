//! The library, glTF, prefabs and projects (src/app/selftest.cpp: the library, the level, prefabs,
//! the project round trip), and the real library when it is there.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{Value, json};

use super::asset_library::{
  AssetLibrary, from_path, game_folder, library_root_of, load_model_asset, merge_mesh, strip_lod,
};
use super::gltf::{
  ExportMode, ExportOptions, ImportOptions, export_glb, import_gltf, load_skeleton,
};
use super::prefabs::{list_prefabs, load_prefab, new_prefab_path, place_prefab, save_prefab};
use super::test_library::*;
use crate::math::{Mat4, Vec3, Vec4, length};
use crate::scene::skeleton::{joint_matrices, skeleton_pose, skin_mesh};
use crate::scene::{
  AlphaMode, BlendMode, Material, Mesh, Node, NodeKind, Scene, Texture, TodColors, TodPalette,
};

fn kinds(s: &Scene) -> [usize; 5] {
  let mut k = [0; 5];
  for n in &s.nodes {
    k[n.kind as usize] += 1;
  }
  k
}

fn named<'a>(s: &'a Scene, name: &str) -> &'a Node {
  s.nodes.iter().find(|n| n.name == name).unwrap()
}

/// the level of the small library, as opened (its decor, actor and collision)
fn small_level(dir: &Path) -> (AssetLibrary, Scene) {
  assert!(
    write_library(dir),
    "library written in ogle-extract's layout"
  );
  let mut library = AssetLibrary::default();
  library.scan(&game_folder(dir.to_str().unwrap())).unwrap();
  let mut scene = Scene::new();
  import_gltf(
    &mut scene,
    &library.level("lev").unwrap().background,
    &ImportOptions::default(),
  )
  .unwrap();
  scene.settings.level = "lev".into();
  scene.settings.source_group = "lev".into();
  (library, scene)
}

#[test]
fn library_scan() {
  let tmp = temp_dir("library");
  let lib_root = tmp.join("library");
  assert!(write_library(&lib_root));
  let folder = game_folder(lib_root.to_str().unwrap());
  let abs = std::path::absolute(lib_root.join("jak3")).unwrap();
  assert_eq!(Path::new(&folder), abs);
  assert!(!folder.contains('\\'));
  // a game folder chosen means its parent
  assert_eq!(game_folder(&folder), folder);
  assert_eq!(
    library_root_of(&folder),
    library_root_of(lib_root.to_str().unwrap())
  );
  assert!(game_folder(tmp.join("nothing").to_str().unwrap()).is_empty());

  let mut library = AssetLibrary::default();
  library.scan(&folder).unwrap();
  let level = library.level("lev").unwrap();
  // one level with its level.json, _disc skipped
  assert_eq!(library.levels().len(), 1);
  assert_eq!(level.dgo, "LEV.DGO");
  assert_eq!(level.actors, 1);
  assert_eq!(level.collision_triangles, -1);
  assert!(level.mtime > 0 && level.listed);
  assert!(std::ptr::eq(
    library
      .level_of(&level.background.replace('/', "\\"))
      .unwrap(),
    level
  ));
  assert_eq!(
    library.prefab_folder(),
    format!("{}/prefabricated", from_root(&folder))
  );
  // actor models: by name start and end, lowest lod first
  let model = |etype: &str| {
    library
      .model_for_etype(etype, "lev")
      .map(|m| format!("{}:{}", m.name, m.lod))
      .unwrap_or_default()
  };
  assert_eq!(model("crate"), "crate-krimson:0");
  assert_eq!(model("skill"), "collectables-skill:0");
  assert_eq!(model("babak"), "");
  assert_eq!(library.models().len(), 3);
  assert_eq!(library.models_named("crate-krimson").len(), 1);
  assert_eq!(
    library.model_named("crate-krimson", "other").unwrap().lod,
    0
  );
  assert_eq!(
    strip_lod("collectables-skill-lod12"),
    ("collectables-skill", 12)
  );
  assert_eq!(strip_lod("door-lod"), ("door-lod", 0));
  library.set_listed(|l| l.name != "lev");
  assert!(!library.levels()[0].listed);
}

fn from_root(game_folder: &str) -> &str {
  &game_folder[..game_folder.rfind('/').unwrap()]
}

#[test]
fn level_read() {
  let tmp = temp_dir("level");
  let (library, scene) = small_level(&tmp);
  let floor = named(&scene, "floor-0");
  let instances: Vec<&Node> = scene
    .nodes
    .iter()
    .filter(|n| n.name.starts_with("rock.mb-"))
    .collect();
  assert!(floor.mesh.is_some() && floor.kind == NodeKind::Render);
  // decor instances sharing their mesh
  assert_eq!(instances.len(), 2);
  assert!(Arc::ptr_eq(
    instances[0].mesh.as_ref().unwrap(),
    instances[1].mesh.as_ref().unwrap()
  ));
  // actor with its data and position
  let actor = scene
    .nodes
    .iter()
    .find(|n| n.kind == NodeKind::Actor)
    .unwrap();
  assert_eq!(actor.extras["etype"], "crate");
  assert_eq!(actor.extras["aid"], 123);
  assert!(actor.extras.get("kind").is_none());
  assert!(length(actor.local.t - Vec3::new(1.0, 2.0, 3.0)) < 1e-5);
  // collision kept with its surface, the builder's keys gone
  let col = scene
    .nodes
    .iter()
    .find(|n| n.kind == NodeKind::Collision)
    .unwrap();
  assert_eq!(
    col.mesh.as_ref().unwrap().prims[0].tri_pat,
    vec![4660u32; 2]
  );
  assert_eq!(col.extras, json!({}));
  assert_eq!(kinds(&scene), [3, 3, 1, 1, 0]);

  let mut again = Scene::new();
  let r = import_gltf(
    &mut again,
    &library.level("lev").unwrap().background,
    &ImportOptions::default(),
  )
  .unwrap();
  assert_eq!((r.nodes, r.triangles, r.roots.len()), (8, 16, 4));
  assert_eq!(r.generator, "jak-level-editor");
  assert_eq!(r.asset_extras["opengoal"]["level"], "lev");

  // a model of the library, baked, merged into a document twice: its materials once
  let model = library.model_for_etype("crate", "lev").unwrap();
  let lm = load_model_asset(&model.path, false).unwrap();
  assert_eq!(lm.mesh.name, "crate-krimson");
  assert_eq!(lm.mesh.tri_count(), 12);
  let mut doc = Scene::new();
  let m = Material {
    name: "stone".into(),
    ..Default::default()
  };
  let lm = super::asset_library::LoadedMesh {
    mesh: make_box("x", 0),
    textures: vec![Texture {
      name: "t".into(),
      width: 1,
      height: 1,
      rgba: vec![1, 2, 3, 4],
      ..Default::default()
    }],
    materials: vec![Material { texture: 0, ..m }],
  };
  let a = merge_mesh(&mut doc, &lm.mesh, &lm.textures, &lm.materials);
  let b = merge_mesh(&mut doc, &lm.mesh, &lm.textures, &lm.materials);
  assert_eq!((doc.textures.len(), doc.materials.len()), (1, 1));
  assert_eq!((a.prims[0].material, b.prims[0].material), (0, 0));
  assert_ne!(a.uid, lm.mesh.uid);
}

#[test]
fn prefabs() {
  let tmp = temp_dir("prefabs");
  let (_, mut scene) = small_level(&tmp);
  let instances: Vec<u32> = scene
    .nodes
    .iter()
    .filter(|n| n.name.starts_with("rock.mb-"))
    .map(|n| n.id)
    .collect();
  let folder = tmp.join("prefabs");
  let folder = folder.to_str().unwrap();
  let prefab_path = new_prefab_path(folder, "Rocks & crate");
  assert!(
    prefab_path.ends_with("/Rocks_crate.glb"),
    "a file name from its name"
  );
  save_prefab(&scene, &instances, &prefab_path).unwrap();
  // the next one of that name gets another file
  assert!(new_prefab_path(folder, "Rocks & crate").ends_with("/Rocks_crate-2.glb"));
  assert!(new_prefab_path(folder, "&&").ends_with("/prefab.glb"));
  assert!(new_prefab_path(folder, "Café 2").ends_with("/Café_2.glb"));
  assert!(save_prefab(&scene, &[], &prefab_path).is_err());
  let listed = list_prefabs(folder);
  assert_eq!(listed.len(), 1);
  assert_eq!(listed[0].name, "Rocks_crate");
  assert!(listed[0].made && listed[0].mtime > 0);
  let mut prefab = Scene::new();
  load_prefab(&prefab_path, &mut prefab).unwrap();
  // its elements around a pivot at the bottom center
  let pb = prefab.bounds();
  assert_eq!(prefab.nodes.len(), 2);
  assert!(
    pb.valid() && pb.lo.y.abs() < 1e-4 && pb.center().x.abs() < 1e-4 && pb.center().z.abs() < 1e-4
  );

  // placed twice: on the point given, meshes shared
  let mut merged = HashMap::new();
  let before = scene.nodes.len();
  let a = place_prefab(
    &mut scene,
    &prefab,
    &Mat4::translate(Vec3::new(100.0, 5.0, 0.0)),
    &mut merged,
  );
  let b = place_prefab(
    &mut scene,
    &prefab,
    &Mat4::translate(Vec3::new(200.0, 5.0, 0.0)),
    &mut merged,
  );
  let mut placed = crate::math::AABB::default();
  for id in &a {
    placed.add_box(&scene.world_bounds(scene.find(*id).unwrap()));
  }
  assert_eq!((a.len(), b.len(), scene.nodes.len()), (2, 2, before + 4));
  assert!(Arc::ptr_eq(
    scene.find(a[0]).unwrap().mesh.as_ref().unwrap(),
    scene.find(b[0]).unwrap().mesh.as_ref().unwrap()
  ));
  assert!((placed.lo.y - 5.0).abs() < 1e-4 && (placed.center().x - 100.0).abs() < 1e-3);
  // the roots named apart from the document's
  let names: HashSet<&str> = scene.nodes.iter().map(|n| n.name.as_str()).collect();
  assert_eq!(names.len(), scene.nodes.len());
}

#[test]
fn prefab_actors_named_like_the_game() {
  // an actor of a prefab placed twice: two actors of a level cannot share a name
  let mut with_actor = Scene::new();
  with_actor.add(Node {
    name: "crate-1".into(),
    kind: NodeKind::Actor,
    extras: json!({"etype": "crate", "aid": 5, "og_group": {"id": 1}, "lump": {"name": "crate-1"}}),
    ..Default::default()
  });
  let mut doc = Scene::new();
  let mut merged = HashMap::new();
  let first = place_prefab(&mut doc, &with_actor, &Mat4::identity(), &mut merged);
  let second = place_prefab(
    &mut doc,
    &with_actor,
    &Mat4::translate(Vec3::new(4.0, 0.0, 0.0)),
    &mut merged,
  );
  let (x, y) = (doc.find(first[0]).unwrap(), doc.find(second[0]).unwrap());
  assert_eq!((x.name.as_str(), y.name.as_str()), ("crate-1", "crate-2"));
  assert_eq!(y.extras["lump"]["name"], "crate-2");
  assert!(y.extras.get("aid").is_none() && y.extras.get("og_group").is_none());
}

fn lit(n_colors: usize, local: bool) -> Arc<TodPalette> {
  let mut rgba = Vec::new();
  for c in 0..n_colors {
    for k in 0..8u8 {
      rgba.extend([10 + k, 20 + k, 30 + c as u8, 128]);
    }
  }
  Arc::new(TodPalette { rgba, local })
}

#[test]
fn project_round_trip() {
  let tmp = temp_dir("project");
  let (_, mut scene) = small_level(&tmp);
  scene.settings.replaces = "ctyindb".into();
  scene
    .nodes
    .iter_mut()
    .find(|n| n.name == "floor-0")
    .unwrap()
    .locked = true;
  let glow = Material {
    name: "glow".into(),
    alpha: AlphaMode::Blend,
    blend: BlendMode::Add,
    cutoff: 0.15, // the alpha test of the PS2, kept by blended surfaces too
    ..Default::default()
  };
  let mut n = Node {
    name: "glow".into(),
    kind: NodeKind::Render,
    mesh: Some(make_box("glow", scene.add_material(glow))),
    ..Default::default()
  };
  n.local.s = Vec3::new(3.0, 1.0, 0.5); // stretched along x, flattened along z
  scene.add(n);
  // a texture, a material see through, two lit nodes sharing their palette, palettes of a file
  let tex = scene.add_texture(Texture {
    name: "checker".into(),
    width: 2,
    height: 1,
    rgba: vec![255, 0, 0, 255, 0, 255, 0, 100],
    ..Default::default()
  });
  let glass = scene.add_material(Material {
    name: "glass".into(),
    texture: tex,
    alpha: AlphaMode::Blend,
    translucent: true,
    double_sided: false,
    base_color: Vec4::new(2.0, 2.0, 2.0, 2.0),
    ..Default::default()
  });
  let palette = lit(2, true);
  let mut lit_floor = make_floor(glass);
  Arc::make_mut(&mut lit_floor).prims[0]
    .extra_attrs
    .insert("_SUNRISE".into(), vec![Vec4::new(0.5, 0.25, 1.0, 1.0); 4]);
  for (i, name) in ["lit-a", "lit-b"].into_iter().enumerate() {
    scene.add(Node {
      name: name.into(),
      kind: NodeKind::Render,
      mesh: Some(lit_floor.clone()),
      tod: Some(Arc::new(TodColors {
        palette: Some(palette.clone()),
        index: vec![vec![i as u16, 1, 0, 1]],
      })),
      visible: i == 0,
      extras: json!({"og_group": {"id": 3, "name": "g"}, "big": 4294905854u64}),
      ..Default::default()
    });
  }
  scene.add(Node {
    name: "walk".into(),
    kind: NodeKind::NavMesh,
    mesh: Some(make_floor(-1)),
    ..Default::default()
  });

  let project = tmp.join("project.glb");
  export_glb(&scene, &project, &ExportOptions::default()).unwrap();
  let mut back = Scene::new();
  let r = import_gltf(
    &mut back,
    &project,
    &ImportOptions {
      read_settings: true,
      ..Default::default()
    },
  )
  .unwrap();
  // saved and opened again, same nodes and level
  assert_eq!(kinds(&scene), kinds(&back));
  assert_eq!(back.settings, scene.settings);
  assert_eq!(r.nodes, scene.nodes.len());
  for (a, b) in scene.nodes.iter().zip(&back.nodes) {
    assert_eq!(
      (&a.name, a.kind, a.visible, a.locked),
      (&b.name, b.kind, b.visible, b.locked)
    );
    assert_eq!(a.extras, b.extras, "{}", a.name);
    assert!(length(a.local.t - b.local.t) < 1e-5);
  }
  // instancing, locks, collision surfaces, actor data, additive material and its alpha test
  let rocks: HashSet<usize> = back
    .nodes
    .iter()
    .filter(|x| x.name.starts_with("rock.mb-"))
    .map(|x| Arc::as_ptr(x.mesh.as_ref().unwrap()) as usize)
    .collect();
  assert_eq!(rocks.len(), 1);
  assert!(named(&back, "floor-0").locked);
  let col = back
    .nodes
    .iter()
    .find(|x| x.kind == NodeKind::Collision)
    .unwrap();
  assert_eq!(
    col.mesh.as_ref().unwrap().prims[0].tri_pat,
    vec![4660u32; 2]
  );
  assert!(
    col
      .mesh
      .as_ref()
      .unwrap()
      .extras
      .get("og_tri_pat_views")
      .is_none()
  );
  let actor = back
    .nodes
    .iter()
    .find(|x| x.kind == NodeKind::Actor)
    .unwrap();
  assert_eq!(actor.extras["aid"], 123);
  let g = named(&back, "glow");
  let m = &back.materials[g.mesh.as_ref().unwrap().prims[0].material as usize];
  assert!(
    m.blend == BlendMode::Add && m.alpha == AlphaMode::Blend && (m.cutoff - 0.15).abs() < 1e-6
  );
  // the scale of a stretched element kept (and its bounds)
  let b = back.world_bounds(g);
  assert!(length(g.local.s - Vec3::new(3.0, 1.0, 0.5)) < 1e-4);
  assert!((b.hi.x - b.lo.x - 6.0).abs() < 1e-3 && (b.hi.z - b.lo.z - 1.0).abs() < 1e-3);
  // the lighting: one palette for both nodes, each its own colors
  let (a, b) = (named(&back, "lit-a"), named(&back, "lit-b"));
  let (ta, tb) = (a.tod.as_ref().unwrap(), b.tod.as_ref().unwrap());
  assert!(Arc::ptr_eq(
    ta.palette.as_ref().unwrap(),
    tb.palette.as_ref().unwrap()
  ));
  assert_eq!(ta.palette.as_ref().unwrap().rgba, palette.rgba);
  assert!(ta.palette.as_ref().unwrap().local);
  // (the vertices read in the order the triangles use them: 0, 2, 1, 3, their colors with them)
  assert_eq!(
    (ta.index.clone(), tb.index.clone()),
    (vec![vec![0, 0, 1, 1]], vec![vec![1, 0, 1, 1]])
  );
  assert!(Arc::ptr_eq(
    a.mesh.as_ref().unwrap(),
    b.mesh.as_ref().unwrap()
  ));
  let p = &a.mesh.as_ref().unwrap().prims[0];
  assert_eq!(p.extra_attrs["_SUNRISE"][3], Vec4::new(0.5, 0.25, 1.0, 1.0));
  assert_eq!(p.uv.len(), 4);
  // the material and its texture
  let m = &back.materials[p.material as usize];
  assert!(m.translucent && !m.double_sided && m.base_color == Vec4::new(2.0, 2.0, 2.0, 2.0));
  let t = &back.textures[m.texture as usize];
  assert_eq!(
    (t.name.as_str(), t.width, t.height, &t.rgba),
    ("checker", 2, 1, &scene.textures[tex as usize].rgba)
  );
  assert_eq!(named(&back, "walk").kind, NodeKind::NavMesh);
  // the file: og_tod written once, the editor's ids
  let glb = read_glb_json(&project);
  assert_eq!(
    glb["asset"]["extras"]["ogle"]["settings"]["replaces"],
    "ctyindb"
  );
  assert!(
    glb["nodes"]
      .as_array()
      .unwrap()
      .iter()
      .all(|n| n["extras"]["og_id"].is_u64())
  );
  let palettes: HashSet<u64> = glb["nodes"]
    .as_array()
    .unwrap()
    .iter()
    .filter_map(|n| n["extras"]["og_tod"]["palette"].as_u64())
    .collect();
  assert_eq!(palettes.len(), 1);
  let acc = &glb["accessors"][glb["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
    .as_u64()
    .unwrap() as usize];
  assert!(acc["min"].is_array() && acc["max"].is_array());

  // an empty document saved and opened again
  let empty = tmp.join("empty.glb");
  export_glb(&Scene::new(), &empty, &ExportOptions::default()).unwrap();
  let mut empty_back = Scene::new();
  import_gltf(&mut empty_back, &empty, &ImportOptions::default()).unwrap();
  assert!(empty_back.nodes.is_empty());
  assert!(read_glb_json(&empty).get("buffers").is_none());

  // a prefab: its roots around the pivot, no ids
  let prefab = tmp.join("prefab.glb");
  let lit_a = named(&scene, "lit-a").id;
  let o = ExportOptions {
    mode: ExportMode::Prefab,
    only: vec![named(&scene, "lev-tie-0").id, lit_a],
    pivot: Mat4::translate(Vec3::new(0.0, -1.0, 0.0)),
    ..Default::default()
  };
  export_glb(&scene, &prefab, &o).unwrap();
  let mut p = Scene::new();
  let r = import_gltf(&mut p, &prefab, &ImportOptions::default()).unwrap();
  assert_eq!((r.nodes, r.roots.len()), (5, 2));
  assert!((named(&p, "lev-tie-0").local.t.y - 1.0).abs() < 1e-5);
  assert!(
    read_glb_json(&prefab)["nodes"][0]["extras"]
      .get("og_id")
      .is_none()
  );
  assert!(p.nodes.iter().all(|n| n.extras.get("og_kind").is_none()));
}

#[test]
fn builder_export() {
  let tmp = temp_dir("builder");
  let mut s = Scene::new();
  let tex = s.add_texture(Texture {
    name: "t".into(),
    width: 1,
    height: 1,
    rgba: vec![10, 20, 30, 100],
    ..Default::default()
  });
  let game = Material {
    name: "wall".into(),
    texture: tex,
    alpha: AlphaMode::Blend,
    cutoff: 0.3,
    base_color: Vec4::new(2.0, 2.0, 2.0, 2.0),
    ..Default::default()
  };
  let glass = Material {
    name: "glass".into(),
    translucent: true,
    ..game.clone()
  };
  let (wall, glass) = (s.add_material(game), s.add_material(glass));
  let group = s.add(Node {
    name: "g".into(),
    local: crate::math::Transform {
      t: Vec3::new(5.0, 0.0, 0.0),
      ..Default::default()
    },
    ..Default::default()
  });
  let floor = s.add(Node {
    name: "floor".into(),
    kind: NodeKind::Render,
    parent: group,
    mesh: Some(make_floor(wall)),
    tod: Some(Arc::new(TodColors {
      palette: Some(lit(1, false)),
      index: vec![vec![0; 4]],
    })),
    ..Default::default()
  });
  let window = s.add(Node {
    name: "window".into(),
    kind: NodeKind::Render,
    mesh: Some(make_box("window", glass)),
    extras: json!({"og_kind": "render"}),
    ..Default::default()
  });
  let mut extra = Mesh::new();
  extra.name = "more".into();
  extra.prims = make_box("x", -1).prims.clone();
  let o = ExportOptions {
    mode: ExportMode::Builder,
    only: vec![floor, window],
    node_extras: HashMap::from([(floor, json!({"set_collision": 1})), (window, json!({}))]),
    extra_meshes: vec![(Arc::new(extra), json!({"kind": "x"}))],
    ..Default::default()
  };
  let path = tmp.join("builder.glb");
  export_glb(&s, &path, &o).unwrap();
  let glb = read_glb_json(&path);
  let nodes = glb["nodes"].as_array().unwrap();
  assert_eq!(nodes.len(), 3);
  // each at its world matrix, the builder's extras only
  assert_eq!(nodes[0]["translation"], json!([5.0, 0.0, 0.0]));
  assert_eq!(nodes[0]["extras"], json!({"set_collision": 1}));
  assert!(nodes[1].get("extras").is_none());
  assert_eq!(nodes[2]["extras"], json!({"kind": "x"}));
  assert!(nodes[2].get("translation").is_none());
  // the lighting as the 8 palettes per vertex, the alpha doubled
  let attrs =
    &glb["meshes"][nodes[0]["mesh"].as_u64().unwrap() as usize]["primitives"][0]["attributes"];
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
    assert!(attrs[k].is_u64(), "{k}");
  }
  let noon = glb_accessor_bytes(&path, &glb, attrs["_NOON"].as_u64().unwrap() as usize);
  assert_eq!(&noon[..4], &[12, 22, 30, 255]);
  // the unlit window lit like new geometry: its palettes too
  let attrs =
    &glb["meshes"][nodes[1]["mesh"].as_u64().unwrap() as usize]["primitives"][0]["attributes"];
  let sunrise = glb_accessor_bytes(&path, &glb, attrs["_SUNRISE"].as_u64().unwrap() as usize);
  assert_eq!(sunrise.len(), 8 * 4);
  assert_eq!(sunrise[3], 255);
  // the materials: a wall cut out, the glass see through; the texture's alpha doubled
  let mats = glb["materials"].as_array().unwrap();
  let mat = |name: &str| mats.iter().find(|m| m["name"] == name).unwrap();
  assert_eq!(mat("wall")["alphaMode"], "MASK");
  assert!((mat("wall")["alphaCutoff"].as_f64().unwrap() - 0.6).abs() < 1e-6);
  assert_eq!(mat("glass")["alphaMode"], "BLEND");
  assert_eq!(mat("glass")["extras"], json!({"og_blend": "alpha"}));
  assert_eq!(glb["samplers"].as_array().unwrap().len(), 1);
  let mut back = Scene::new();
  import_gltf(&mut back, &path, &ImportOptions::default()).unwrap();
  assert_eq!(back.textures[0].rgba, vec![10, 20, 30, 200]);
  assert!(back.nodes.iter().all(|n| n.extras.get("og_kind").is_none()));
  // the builder's palettes read back like those of a file
  assert!(
    back.nodes[0].mesh.as_ref().unwrap().prims[0]
      .extra_attrs
      .contains_key("_SUNRISE")
  );
}

// ------------------------------------------------------------------------------------------------
// files made by hand: strips and fans, other component types, .gltf with its buffer in a data
// URI, a skeleton
// ------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Bin {
  bytes: Vec<u8>,
  views: Vec<Value>,
  accessors: Vec<Value>,
}

impl Bin {
  fn add(&mut self, data: &[u8], ctype: u32, count: usize, ty: &str, normalized: bool) -> usize {
    while !self.bytes.len().is_multiple_of(4) {
      self.bytes.push(0);
    }
    self
      .views
      .push(json!({"buffer": 0, "byteOffset": self.bytes.len(), "byteLength": data.len()}));
    self.bytes.extend_from_slice(data);
    let mut a = json!({"bufferView": self.views.len() - 1, "componentType": ctype, "count": count, "type": ty});
    if normalized {
      a["normalized"] = json!(true);
    }
    self.accessors.push(a);
    self.accessors.len() - 1
  }

  fn floats(&mut self, v: &[f32], count: usize, ty: &str) -> usize {
    let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
    self.add(&bytes, 5126, count, ty, false)
  }

  fn finish(self, mut doc: Value) -> (Value, Vec<u8>) {
    doc["bufferViews"] = Value::Array(self.views);
    doc["accessors"] = Value::Array(self.accessors);
    doc["buffers"] = json!([{"byteLength": self.bytes.len()}]);
    (doc, self.bytes)
  }
}

fn base64(data: &[u8]) -> String {
  const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut out = String::new();
  for c in data.chunks(3) {
    let n =
      (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
    for k in 0..4 {
      out.push(if k <= c.len() {
        T[(n >> (18 - 6 * k) & 63) as usize] as char
      } else {
        '='
      });
    }
  }
  out
}

fn write_glb(path: &Path, doc: &Value, bin: &[u8]) {
  let mut text = serde_json::to_vec(doc).unwrap();
  while !text.len().is_multiple_of(4) {
    text.push(b' ');
  }
  let mut bin = bin.to_vec();
  while !bin.len().is_multiple_of(4) {
    bin.push(0);
  }
  let mut out = Vec::new();
  for v in [
    0x46546C67u32,
    2,
    (28 + text.len() + bin.len()) as u32,
    text.len() as u32,
    0x4E4F534A,
  ] {
    out.extend_from_slice(&v.to_le_bytes());
  }
  out.extend_from_slice(&text);
  out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
  out.extend_from_slice(&0x004E4942u32.to_le_bytes());
  out.extend_from_slice(&bin);
  std::fs::write(path, out).unwrap();
}

#[test]
fn gltf_text_strips_and_component_types() {
  let tmp = temp_dir("gltf-text");
  let mut bin = Bin::default();
  let pos = bin.floats(&[0., 0., 0., 1., 0., 0., 0., 0., 1., 1., 0., 1.], 4, "VEC3");
  let col = bin.add(
    &[
      255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 51, 51, 51, 0,
    ],
    5121,
    4,
    "VEC4",
    false,
  );
  let uv: Vec<u8> = [0u16, 0, 65535, 0, 0, 65535, 65535, 65535]
    .iter()
    .flat_map(|v| v.to_le_bytes())
    .collect();
  let uv = bin.add(&uv, 5123, 4, "VEC2", false);
  let strip = bin.add(&[0, 1, 2, 3], 5121, 4, "SCALAR", false);
  let fan: Vec<u8> = [0u16, 1, 2, 3]
    .iter()
    .flat_map(|v| v.to_le_bytes())
    .collect();
  let fan = bin.add(&fan, 5123, 4, "SCALAR", false);
  let bad = bin.add(&[0, 1, 9], 5121, 3, "SCALAR", false);
  let mut png = Vec::new();
  image::ImageEncoder::write_image(
    image::codecs::png::PngEncoder::new(&mut png),
    &[1, 2, 3, 4, 5, 6, 7, 8],
    2,
    1,
    image::ExtendedColorType::Rgba8,
  )
  .unwrap();
  let mut jpeg = Vec::new();
  image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
    .encode(&[200u8; 8 * 4 * 3], 8, 4, image::ExtendedColorType::Rgb8)
    .unwrap();
  let attrs = json!({"POSITION": pos, "COLOR_0": col, "TEXCOORD_0": uv});
  let doc = json!({
    "asset": {"version": "2.0", "generator": "test", "extras": {"ogle": {"settings": {"level": "x"}}}},
    "nodes": [
      {"matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 0, 0, 1], "children": [1]},
      {"mesh": 0, "extras": {"og_kind": "render", "og_hidden": true}},
      {"name": "alone", "translation": [0, 1, 0], "rotation": [0, 0, 0, 2], "scale": [2, 2, 2]}
    ],
    "meshes": [{"primitives": [
      {"attributes": attrs, "indices": strip, "mode": 5, "material": 0},
      {"attributes": attrs, "indices": fan, "mode": 6, "material": 1},
      {"attributes": attrs, "indices": bad},
      {"attributes": attrs},
      {"attributes": attrs, "mode": 0},
      {"attributes": {"NORMAL": pos}}
    ]}],
    "materials": [
      {"pbrMetallicRoughness": {"baseColorFactor": [0.5, 0.5, 0.5, 1], "baseColorTexture": {"index": 0}},
       "alphaMode": "MASK", "alphaCutoff": 0.25, "extras": {"og_blend": "subtract"}},
      {"name": "broken", "pbrMetallicRoughness": {"baseColorTexture": {"index": 1}}, "doubleSided": true}
    ],
    "textures": [{"source": 0}, {"source": 1}],
    "images": [{"uri": format!("data:image/png;base64,{}", base64(&png))}, {"name": "gone", "uri": "missing.png"},
               {"name": "photo", "uri": format!("data:image/jpeg;base64,{}", base64(&jpeg))}]
  });
  let (mut doc, bytes) = bin.finish(doc);
  doc["buffers"][0]["uri"] = json!(format!(
    "data:application/octet-stream;base64,{}",
    base64(&bytes)
  ));
  let path = tmp.join("hand.gltf");
  std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();

  let mut s = Scene::new();
  let r = import_gltf(
    &mut s,
    &path,
    &ImportOptions {
      read_settings: true,
      ..Default::default()
    },
  )
  .unwrap();
  // no scene: the nodes nobody has as a child are the roots
  assert_eq!(
    (r.nodes, r.roots.len(), r.generator.as_str()),
    (3, 2, "test")
  );
  assert_eq!(s.settings.level, "x");
  assert_eq!(s.nodes[0].name, "node-0");
  assert!(length(s.nodes[0].local.t - Vec3::new(5.0, 0.0, 0.0)) < 1e-6);
  let alone = named(&s, "alone");
  assert!((alone.local.r.w - 1.0).abs() < 1e-6 && alone.local.s == Vec3::new(2.0, 2.0, 2.0));
  let n = &s.nodes[1];
  assert!(!n.visible && n.extras == json!({}));
  let mesh = n.mesh.as_ref().unwrap();
  assert_eq!(mesh.name, "mesh-0");
  // the strip, the fan; out of range indices and points left out
  assert_eq!(mesh.prims.len(), 3);
  assert_eq!(mesh.prims[0].idx, vec![0, 1, 2, 2, 1, 3]);
  assert_eq!(mesh.prims[1].idx, vec![0, 1, 2, 0, 2, 3]);
  assert_eq!(r.triangles, 5);
  let p = &mesh.prims[0];
  assert_eq!(p.col[3], Vec4::new(0.2, 0.2, 0.2, 0.0));
  assert_eq!(p.uv[3], crate::math::Vec2::new(1.0, 1.0));
  assert!(p.nrm.len() == 4 && (p.nrm[0].y.abs() - 1.0).abs() < 1e-6);
  // a primitive without indices: every vertex (whole triangles); NORMAL alone: left out
  assert_eq!(mesh.prims[2].idx, vec![0, 1, 2]);
  // materials: their texture, alpha, blend; an image that cannot be read is a checker
  let m = &s.materials[p.material as usize];
  assert_eq!(
    (m.name.as_str(), m.alpha, m.blend, m.cutoff, m.double_sided),
    (
      "material-0",
      AlphaMode::Blend,
      BlendMode::Subtract,
      0.25,
      false
    )
  );
  assert_eq!(m.base_color, Vec4::new(0.5, 0.5, 0.5, 1.0));
  assert_eq!(
    s.textures[m.texture as usize].rgba,
    vec![1, 2, 3, 4, 5, 6, 7, 8]
  );
  assert_eq!(s.textures[m.texture as usize].name, "image-0");
  let broken = &s.textures[s.materials[1].texture as usize];
  assert_eq!(
    (broken.name.as_str(), broken.width, broken.rgba[0]),
    ("gone", 2, 255)
  );
  // a JPEG, as RGBA
  let photo = &s.textures[2];
  assert_eq!(
    (photo.width, photo.height, photo.rgba.len()),
    (8, 4, 8 * 4 * 4)
  );
  assert!(photo.rgba[0].abs_diff(200) < 3 && photo.rgba[3] == 255);
  assert!(s.materials[1].double_sided);

  // the same as a .glb (its buffer the BIN chunk) and as a .gltf with the buffer beside it
  let mut glb_doc = doc.clone();
  glb_doc["buffers"][0].as_object_mut().unwrap().remove("uri");
  write_glb(&tmp.join("hand.glb"), &glb_doc, &bytes);
  let mut side = doc.clone();
  side["buffers"][0]["uri"] = json!("hand%20data.bin");
  std::fs::write(tmp.join("hand data.bin"), &bytes).unwrap();
  std::fs::write(tmp.join("side.gltf"), serde_json::to_vec(&side).unwrap()).unwrap();
  for file in ["hand.glb", "side.gltf"] {
    let mut t = Scene::new();
    let r = import_gltf(&mut t, tmp.join(file), &ImportOptions::default()).unwrap();
    assert_eq!((r.nodes, r.triangles), (3, 5), "{file}");
    assert_eq!(t.nodes[1].mesh.as_ref().unwrap().prims[0].col, p.col);
  }
  // a damaged file: an error, not a crash
  std::fs::write(tmp.join("bad.glb"), b"glTF\x02\0\0\0").unwrap();
  assert!(
    import_gltf(
      &mut Scene::new(),
      tmp.join("bad.glb"),
      &ImportOptions::default()
    )
    .is_err()
  );
  assert!(
    import_gltf(
      &mut Scene::new(),
      tmp.join("none.glb"),
      &ImportOptions::default()
    )
    .is_err()
  );
  assert!(load_skeleton(tmp.join("hand.glb")).is_err());
}

#[test]
fn skeleton_and_skin() {
  let tmp = temp_dir("skeleton");
  let mut bin = Bin::default();
  let pos = bin.floats(&[0., 0., 0., 1., 0., 0., 0., 1., 0.], 3, "VEC3");
  let joints = bin.add(
    &[0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0],
    5121,
    3,
    "VEC4",
    false,
  );
  let weights = bin.add(
    &[255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0],
    5121,
    3,
    "VEC4",
    true,
  );
  let mut ibm = Mat4::identity().m.to_vec();
  ibm.extend(Mat4::translate(Vec3::new(0.0, -1.0, 0.0)).m);
  let ibm = bin.floats(&ibm, 2, "MAT4");
  let times = bin.floats(&[0.0, 2.0], 2, "SCALAR");
  let values = bin.floats(&[0., 1., 0., 0., 3., 0.], 2, "VEC3");
  let doc = json!({
    "asset": {"version": "2.0"},
    "scene": 0,
    "scenes": [{"nodes": [0, 2]}],
    "nodes": [
      {"name": "root", "children": [1]},
      {"name": "bone", "translation": [0, 1, 0]},
      {"name": "body", "mesh": 0, "skin": 0}
    ],
    "meshes": [{"primitives": [{"attributes": {"POSITION": pos, "JOINTS_0": joints, "WEIGHTS_0": weights}}]}],
    "skins": [{"joints": [0, 1], "inverseBindMatrices": ibm}],
    "animations": [{"name": "lift",
      "channels": [{"sampler": 0, "target": {"node": 1, "path": "translation"}},
                   {"sampler": 0, "target": {"node": 1, "path": "weights"}}],
      "samplers": [{"input": times, "output": values}]}]
  });
  let (doc, bytes) = bin.finish(doc);
  let path = tmp.join("door-lod0.glb");
  write_glb(&path, &doc, &bytes);

  let s = load_skeleton(&path).unwrap();
  assert_eq!(
    (s.parent.clone(), s.joints.clone()),
    (vec![-1, 0, -1], vec![0, 1])
  );
  assert_eq!(s.inverse_bind.len(), 2);
  assert_eq!(s.animations.len(), 1);
  assert_eq!(
    (s.animations[0].name.as_str(), s.animations[0].duration),
    ("lift", 2.0)
  );
  assert_eq!(s.animations[0].channels.len(), 1);
  // the model with its joints and weights, moved by the animation
  let plain = load_model_asset(path.to_str().unwrap(), false).unwrap();
  assert!(plain.mesh.prims[0].extra_attrs.is_empty());
  let lm = load_model_asset(path.to_str().unwrap(), true).unwrap();
  assert_eq!(lm.mesh.name, "door");
  let bind = &lm.mesh;
  assert_eq!(
    bind.prims[0].extra_attrs["WEIGHTS_0"][1],
    Vec4::new(1.0, 0.0, 0.0, 0.0)
  );
  assert_eq!(
    bind.prims[0].extra_attrs["JOINTS_0"][1],
    Vec4::new(1.0, 0.0, 0.0, 0.0)
  );
  let mut out = (**bind).clone();
  assert!(skin_mesh(
    bind,
    &joint_matrices(&s, &skeleton_pose(&s, -1, 0.0)),
    &mut out
  ));
  assert!(length(out.prims[0].pos[1] - Vec3::new(1.0, 0.0, 0.0)) < 1e-5);
  skin_mesh(
    bind,
    &joint_matrices(&s, &skeleton_pose(&s, 0, 1.0)),
    &mut out,
  );
  assert!(length(out.prims[0].pos[0]) < 1e-5);
  assert!(length(out.prims[0].pos[1] - Vec3::new(1.0, 1.0, 0.0)) < 1e-4);
}

// ------------------------------------------------------------------------------------------------
// the real library (skipped without it)
// ------------------------------------------------------------------------------------------------

fn distinct_palettes(s: &Scene) -> usize {
  s.nodes
    .iter()
    .filter_map(|n| n.tod.as_ref())
    .map(|t| Arc::as_ptr(t.palette.as_ref().unwrap()) as usize)
    .collect::<HashSet<_>>()
    .len()
}

#[test]
fn real_level_reads_and_round_trips() {
  let Some(lib) = real_library() else {
    eprintln!("skipped: no library/jak3/library.json");
    return;
  };
  let path = lib.join("jak3/ctyinda/decor/ctyinda-background.glb");
  if !path.is_file() {
    return;
  }
  let mut s = Scene::new();
  let t = Instant::now();
  let r = import_gltf(&mut s, &path, &ImportOptions::default()).unwrap();
  eprintln!(
    "ctyinda: {} nodes, {} triangles read in {:?}",
    r.nodes,
    r.triangles,
    t.elapsed()
  );
  // the counts of the file (the C++ editor's: tools/selftest's reading of the same glTF)
  assert_eq!((r.nodes, r.triangles), (9451, 36735));
  assert_eq!(kinds(&s), [102, 8925, 13, 411, 0]);
  assert_eq!(s.nodes.iter().filter(|n| n.tod.is_some()).count(), 8925);
  assert_eq!(distinct_palettes(&s), 4);
  assert_eq!((s.textures.len(), s.materials.len()), (81, 256));
  // every image read (none replaced by the checker)
  assert!(s.textures.iter().all(
    |t| t.rgba.len() == (t.width * t.height * 4) as usize && t.rgba[..4] != [255, 0, 255, 255]
  ));
  assert_eq!(r.asset_extras["opengoal"]["level"], "ctyinda");
  for n in &s.nodes {
    match n.kind {
      NodeKind::Collision => {
        let m = n.mesh.as_ref().unwrap();
        assert!(
          m.prims
            .iter()
            .all(|p| p.tri_pat.len() == p.tri_count() && p.tri_count() > 0)
        );
        assert!(n.extras.get("collide_pat").is_none() && n.extras.get("set_collision").is_none());
      }
      NodeKind::Actor => {
        assert!(
          n.extras["etype"].is_string()
            && n.extras.get("kind").is_none()
            && n.extras["lump"].is_object()
        );
      }
      _ => {}
    }
  }

  let tmp = temp_dir("real-level");
  let out = tmp.join("ctyinda.glb");
  let t = Instant::now();
  export_glb(&s, &out, &ExportOptions::default()).unwrap();
  eprintln!("ctyinda: written in {:?}", t.elapsed());
  let mut back = Scene::new();
  let r2 = import_gltf(&mut back, &out, &ImportOptions::default()).unwrap();
  assert_eq!((r2.nodes, r2.triangles), (r.nodes, r.triangles));
  assert_eq!(kinds(&back), kinds(&s));
  assert_eq!(distinct_palettes(&back), 4);
  for (a, b) in s.nodes.iter().zip(&back.nodes) {
    assert_eq!((&a.name, a.kind, &a.extras), (&b.name, b.kind, &b.extras));
    assert!(length(a.local.t - b.local.t) < 1e-3);
    match (&a.tod, &b.tod) {
      (Some(x), Some(y)) => {
        assert_eq!(x.index, y.index);
        assert_eq!(
          x.palette.as_ref().unwrap().rgba,
          y.palette.as_ref().unwrap().rgba
        );
      }
      (None, None) => {}
      _ => panic!("{}: lighting lost", a.name),
    }
    if let (Some(ma), Some(mb)) = (&a.mesh, &b.mesh) {
      assert_eq!(ma.prims.len(), mb.prims.len());
      for (pa, pb) in ma.prims.iter().zip(&mb.prims) {
        assert_eq!(
          (&pa.pos, &pa.idx, &pa.uv, &pa.col, &pa.tri_pat),
          (&pb.pos, &pb.idx, &pb.uv, &pb.col, &pb.tri_pat)
        );
        let name = |s: &Scene, m: i32| (m >= 0).then(|| s.materials[m as usize].name.clone());
        assert_eq!(name(&s, pa.material), name(&back, pb.material));
      }
    }
  }
}

#[test]
fn real_library_scan_and_largest_level() {
  let Some(lib) = real_library() else {
    eprintln!("skipped: no library/jak3/library.json");
    return;
  };
  let mut library = AssetLibrary::default();
  library.scan(&game_folder(lib.to_str().unwrap())).unwrap();
  assert!(library.levels().len() > 50 && library.models().len() > 100);
  assert!(
    library
      .level("ctyinda")
      .is_some_and(|l| l.collision_triangles != -1 && l.actor_types.is_object())
  );
  let largest = library
    .levels()
    .iter()
    .max_by_key(|l| std::fs::metadata(&l.background).map_or(0, |m| m.len()))
    .unwrap();
  let size = std::fs::metadata(&largest.background).unwrap().len();
  let mut s = Scene::new();
  let t = Instant::now();
  let r = import_gltf(&mut s, &largest.background, &ImportOptions::default()).unwrap();
  eprintln!(
    "largest level {} ({:.1} MB): {} nodes, {} triangles, {} textures read in {:?}",
    largest.name,
    size as f64 / 1e6,
    r.nodes,
    r.triangles,
    s.textures.len(),
    t.elapsed()
  );
  assert!(r.nodes > 0 && r.triangles > 0);
}

#[test]
fn editor_tree_defaults() {
  // this editor's own tree when the program runs from it (the tests do): never jak-project's,
  // which it neither builds against nor runs; an installed editor has none (its host's library)
  use super::asset_library::{default_iso_data_dir, default_library_root, editor_tree};
  let Some(tree) = editor_tree() else {
    return; // a target folder outside the tree: an installed editor's case
  };
  let (library, iso) = (default_library_root(), default_iso_data_dir());
  for p in [&library, &iso] {
    assert!(!p.contains('\\') && !p.contains("/jak-project/"), "{p}");
  }
  let (library, iso) = (Path::new(&library), Path::new(&iso));
  assert_eq!(iso.parent(), library.parent());
  assert_eq!(iso.file_name().unwrap(), "iso_data");
  assert!(tree.join("crates/core/Cargo.toml").is_file());
  assert_eq!(library, tree.join("library"));
  // the editor's library before one is chosen: the host's, else this tree's
  let mut ed = crate::editor::Editor::new();
  assert_eq!(ed.default_library(), from_path(library));
  assert_eq!(Path::new(&ed.projects_root()), library.join("projects"));
  ed.host.default_library = Some("C:/Users/x/Documents/Jak Level Editor".into());
  assert_eq!(
    ed.default_library(),
    "C:/Users/x/Documents/Jak Level Editor"
  );
}

#[test]
fn asset_store_loads_on_threads() {
  fn shareable<T: Send + Sync>() {}
  shareable::<AssetLibrary>();
  shareable::<super::asset_library::LoadedMesh>();
  let tmp = temp_dir("store");
  let (library, _) = small_level(&tmp);
  let model = library
    .model_for_etype("crate", "lev")
    .unwrap()
    .path
    .clone();
  let level = library.level("lev").unwrap().background.clone();
  let missing = tmp.join("missing.glb").to_str().unwrap().to_string();
  let mut store = super::asset_store::AssetStore::new();
  let start = Instant::now();
  let (mut m, mut l) = (None, None);
  while (m.is_none() || l.is_none() || !store.failed_model(&missing))
    && start.elapsed().as_secs() < 20
  {
    m = store.model(&model, false);
    l = store.level(&level, true);
    store.model(&missing, false);
    store.update();
    std::thread::sleep(std::time::Duration::from_millis(5));
  }
  assert_eq!(m.unwrap().mesh.tri_count(), 12);
  assert_eq!(l.unwrap().nodes.len(), 8);
  assert!(store.failed_model(&missing) && !store.failed_level(&level));
  assert_eq!(store.pending(), 0);
  store.forget_model(&model);
  assert!(store.model(&model, false).is_none() && store.pending() == 1);
  store.clear();
  assert_eq!(store.pending(), 0);
}

/// A whole city (every level of the city appended: about 150 000 nodes) read, written and read
/// again: `cargo test --release -- --ignored city_scale --nocapture`
#[test]
#[ignore]
fn city_scale() {
  let Some(lib) = real_library() else { return };
  let mut library = AssetLibrary::default();
  library.scan(&game_folder(lib.to_str().unwrap())).unwrap();
  let mut city = Scene::new();
  let t = Instant::now();
  for l in library
    .levels()
    .iter()
    .filter(|l| l.name.starts_with("cty"))
  {
    let mut s = Scene::new();
    import_gltf(&mut s, &l.background, &ImportOptions::default()).unwrap();
    city.append(s);
  }
  eprintln!("city: {} nodes read in {:?}", city.nodes.len(), t.elapsed());
  let out = temp_dir("city").join("city.glb");
  let t = Instant::now();
  export_glb(&city, &out, &ExportOptions::default()).unwrap();
  let size = std::fs::metadata(&out).unwrap().len();
  eprintln!(
    "city: written in {:?} ({:.1} MB)",
    t.elapsed(),
    size as f64 / 1e6
  );
  let t = Instant::now();
  let mut back = Scene::new();
  let r = import_gltf(&mut back, &out, &ImportOptions::default()).unwrap();
  eprintln!("city: {} nodes read again in {:?}", r.nodes, t.elapsed());
  assert_eq!(r.nodes, city.nodes.len());
}
