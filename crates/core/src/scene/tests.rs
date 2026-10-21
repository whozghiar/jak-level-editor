// The selftest's checks of the scene (src/app/selftest.cpp) that need only the scene, and the
// cache's rules.

use std::sync::Arc;

use super::pick::{K_PICK_ALL_MAX, PickFilter, covered_below, pick, pick_all};
use super::*;
use crate::math::{Quat, Ray};

// a box of size 2, from y = 0 to 2
fn make_box(name: &str, material: i32) -> Arc<Mesh> {
  let mut mesh = Mesh::new();
  mesh.name = name.into();
  let mut p = Primitive::default();
  for i in 0..8 {
    p.pos.push(Vec3::new(
      if i & 1 != 0 { 1.0 } else { -1.0 },
      if i & 2 != 0 { 2.0 } else { 0.0 },
      if i & 4 != 0 { 1.0 } else { -1.0 },
    ));
  }
  let faces = [
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

fn quad(w: f32, h: f32, upright: bool) -> Mesh {
  let mut mesh = Mesh::new();
  let pos = if upright {
    vec![
      Vec3::new(0.0, 0.0, 0.0),
      Vec3::new(w, 0.0, 0.0),
      Vec3::new(w, h, 0.0),
      Vec3::new(0.0, h, 0.0),
    ]
  } else {
    vec![
      Vec3::new(0.0, 0.0, 0.0),
      Vec3::new(w, 0.0, 0.0),
      Vec3::new(w, 0.0, h),
      Vec3::new(0.0, 0.0, h),
    ]
  };
  mesh.prims.push(Primitive {
    pos,
    idx: vec![0, 1, 2, 0, 2, 3],
    ..Default::default()
  });
  mesh
}

// the document and its history go to worker threads
#[test]
fn shared_with_threads() {
  fn send_sync<T: Send + Sync>() {}
  send_sync::<Scene>();
  send_sync::<Mesh>();
  send_sync::<SceneCache>();
  send_sync::<crate::history::History>();
  send_sync::<crate::history::NodeChange>();
}

fn render(mesh: Mesh) -> Node {
  Node {
    kind: NodeKind::Render,
    mesh: Some(Arc::new(mesh)),
    ..Default::default()
  }
}

// a large selection: the descendants of thousands of roots in one pass, right and fast
#[test]
fn large_selection() {
  let mut s = Scene::new();
  let (mut groups, mut leaves) = (Vec::new(), Vec::new());
  for g in 0..1000 {
    let gid = s.add(Node {
      name: format!("g{g}"),
      ..Default::default()
    });
    groups.push(gid);
    for _ in 0..99 {
      leaves.push(s.add(Node {
        parent: gid,
        kind: NodeKind::Render,
        ..Default::default()
      }));
    }
  }
  let mut roots = groups[..500].to_vec();
  roots.push(leaves[700 * 99]); // a leaf of a group not among the roots
  let t0 = std::time::Instant::now();
  let below = s.descendants_of(&roots);
  let mut tree = Vec::new();
  for &g in &groups {
    tree.clear();
    s.subtree(s.index_of(g).unwrap(), &mut tree);
  }
  let ms = t0.elapsed().as_secs_f64() * 1000.0;
  let mut right = below.len() == 501 && below[500].is_empty();
  for r in 0..500 {
    right = right && below[r].len() == 99 && s.find(below[r][0]).unwrap().parent == groups[r];
  }
  assert!(
    right && tree.len() == 100 && ms < 500.0,
    "large selection: the descendants of 500 groups ({ms:.0} ms)"
  );
  assert_eq!(s.descendants(groups[3]).len(), 99);
  s.remove(&[groups[0]]);
  assert!(s.nodes.len() == 99_900 && s.find(leaves[0]).is_none() && s.find(groups[1]).is_some());
}

// the ground of a level locked, not its large walls
#[test]
fn ground_pieces() {
  assert!(
    ground_piece(&quad(100.0, 100.0, false))
      && !ground_piece(&quad(81.0, 30.0, true))
      && !ground_piece(&quad(20.0, 20.0, false)),
    "level pieces: a large flat piece is ground (locked), a facade as wide is not, nor a small slab"
  );

  // a ground the game made one with a 20 m wall standing on it: the wall becomes a piece of its
  // own; a hill stays whole
  let mut gs = Scene::new();
  let mut yard = quad(100.0, 100.0, false);
  let mut wall = quad(60.0, 20.0, true).prims.remove(0);
  for p in &mut wall.pos {
    p.z += 50.0;
  }
  yard.prims.push(wall);
  gs.add(Node {
    name: "yard".into(),
    ..render(yard)
  });
  let mut hill = quad(100.0, 100.0, false);
  for p in &mut hill.prims[0].pos {
    p.y = p.z * 0.2;
  }
  gs.add(render(hill));
  let walls = split_ground_piece(&mut gs, 0);
  let n = &gs.nodes;
  let split = walls == Some(2)
    && n[2].name == "yard-walls"
    && n[0].mesh.as_ref().unwrap().prims.len() == 1
    && n[2].mesh.as_ref().unwrap().prims.len() == 1
    && n[2].mesh.as_ref().unwrap().prims[0].pos[2].y == 20.0
    && ground_piece(n[0].mesh.as_ref().unwrap())
    && !ground_piece(n[2].mesh.as_ref().unwrap())
    && n[2].extras["og_terrain"] == false;
  assert!(
    split && split_ground_piece(&mut gs, 1).is_none() && gs.nodes.len() == 3,
    "level pieces: a wall the game merged with the ground is split off (selectable), a hill is not cut"
  );

  // a large flat piece is ground only with nothing under it: a roof 10 m over a yard is not, a road
  // laid 2 cm over it is still (an overlay), the yard itself is
  let mut cs = Scene::new();
  let mut piece = |x0: f32, x1: f32, z0: f32, z1: f32, y: f32| {
    let mut n = render(quad(x1 - x0, z1 - z0, false));
    n.local.t = Vec3::new(x0, y, z0);
    cs.add(n)
  };
  piece(0.0, 100.0, 0.0, 100.0, 0.0);
  piece(10.0, 80.0, 10.0, 80.0, 10.0);
  piece(20.0, 90.0, 5.0, 95.0, 0.02);
  cs.begin_frame();
  let (yard, roof, road) = (
    covered_below(&cs, 0, 1.0),
    covered_below(&cs, 1, 1.0),
    covered_below(&cs, 2, 1.0),
  );
  assert!(
    structure_texture("city-port-bridge-brace01-18")
      && structure_texture("sewer-brick-roof-02-94")
      && !structure_texture("city-port-grnd-cobl-01-5")
      && natural_ground("des-mount-sand-trans-7")
      && natural_ground("farm-grass-ground-02-20")
      && !natural_ground("freehq-monitor01-4"),
    "level pieces: the textures of structures are never ground, those of natural ground are"
  );
  assert!(
    yard == 0.0 && roof > 0.95 && road < 0.05,
    "level pieces: ground is what has nothing under it (yard {yard:.2}, roof over it {roof:.2}, road on it {road:.2})"
  );
  let (floor, area) = piece_floor(
    &quad(10.0, 10.0, false),
    &Mat4::translate(Vec3::new(0.0, 3.0, 0.0)),
  );
  assert!(floor == 3.0 && (area - 100.0).abs() < 1e-3);
}

// the glows do not take the click
#[test]
fn picking_glows() {
  // a box behind a glow (an additive light beam): the click goes to the box, a second click to the
  // glow
  let mut ps = Scene::new();
  let glow_material = ps.add_material(Material {
    alpha: AlphaMode::Blend,
    blend: BlendMode::Add,
    ..Default::default()
  });
  let box_id = ps.add(Node {
    kind: NodeKind::Render,
    mesh: Some(make_box("box", -1)),
    ..Default::default()
  });
  let mut beam = quad(4.0, 4.0, true);
  beam.prims[0].material = glow_material;
  let mut beam = render(beam);
  beam.local.t = Vec3::new(-2.0, 0.0, 3.0);
  let beam_id = ps.add(beam);
  ps.begin_frame();
  let ray = Ray::new(Vec3::new(0.0, 1.0, 10.0), Vec3::new(0.0, 0.0, -1.0));
  let hit = pick(&ps, &ray, &PickFilter::default());
  let all = pick_all(&ps, &ray, &PickFilter::default(), K_PICK_ALL_MAX);
  let beam_alone = pick(
    &ps,
    &Ray::new(Vec3::new(1.5, 3.5, 10.0), Vec3::new(0.0, 0.0, -1.0)),
    &PickFilter::default(),
  );
  assert!(
    hit.node == box_id
      && all.len() == 2
      && all[0].node == box_id
      && all[1].node == beam_id
      && beam_alone.node == beam_id,
    "picking: a glow in front does not take the click, a second click reaches it, alone it is picked"
  );
  assert!((hit.t - 9.0).abs() < 1e-5 && hit.normal == Vec3::new(0.0, 0.0, 1.0) && hit.prim == 0);

  // locked nodes, excluded subtrees, hidden nodes and actors' markers
  let actor = ps.add(Node {
    kind: NodeKind::Actor,
    local: Transform {
      t: Vec3::new(0.0, 1.0, 6.0),
      ..Default::default()
    },
    ..Default::default()
  });
  ps.begin_frame();
  assert_eq!(
    pick(&ps, &ray, &PickFilter::default()).node,
    actor,
    "an actor's marker in front"
  );
  let no_actors = PickFilter {
    actors: false,
    ..Default::default()
  };
  assert_eq!(pick(&ps, &ray, &no_actors).node, box_id);
  ps.find_mut(box_id).unwrap().locked = true;
  ps.begin_frame();
  assert_eq!(
    pick(&ps, &ray, &no_actors).node,
    beam_id,
    "the locked box is not picked"
  );
  assert_eq!(
    pick(
      &ps,
      &ray,
      &PickFilter {
        include_locked: true,
        ..no_actors.clone()
      }
    )
    .node,
    box_id
  );
  let exclude = PickFilter {
    include_locked: true,
    exclude_list: vec![box_id],
    ..no_actors.clone()
  };
  assert_eq!(pick(&ps, &ray, &exclude).node, beam_id);
  ps.find_mut(beam_id).unwrap().visible = false;
  ps.begin_frame();
  assert!(!pick(&ps, &ray, &no_actors).hit());
}

// a level loaded apart, appended to a document: new ids, its parents kept, the textures and
// materials the document has shared, its meshes on the document's materials
#[test]
fn append() {
  let texture = |name: &str, value: u8| Texture {
    name: name.into(),
    width: 2,
    height: 2,
    rgba: vec![value; 16],
    ..Default::default()
  };
  let (mut doc, mut other) = (Scene::new(), Scene::new());
  doc.add_texture(texture("wall", 10));
  let wall_mat = Material {
    name: "wall".into(),
    texture: 0,
    ..Default::default()
  };
  doc.add_material(wall_mat.clone());
  doc.add(Node {
    name: "a".into(),
    kind: NodeKind::Render,
    mesh: Some(make_box("a", 0)),
    ..Default::default()
  });
  other.add_texture(texture("roof", 20));
  other.add_texture(texture("wall", 10));
  other.add_material(Material {
    name: "roof".into(),
    texture: 0,
    ..Default::default()
  });
  other.add_material(Material {
    texture: 1,
    ..wall_mat
  });
  let g = other.add(Node {
    name: "group".into(),
    ..Default::default()
  });
  let wall_mesh = make_box("wall", 1);
  let render = |name: &str, mesh: Arc<Mesh>| Node {
    name: name.into(),
    kind: NodeKind::Render,
    parent: g,
    mesh: Some(mesh),
    ..Default::default()
  };
  other.add(render("roof", make_box("roof", 0)));
  other.add(render("wall", wall_mesh.clone()));
  other.add(render("wall2", wall_mesh)); // a second instance of the wall: they keep sharing their mesh
  let roots = doc.append(other);
  let ids: std::collections::BTreeSet<u32> = doc.nodes.iter().map(|n| n.id).collect();
  let by_name = |name: &str| doc.nodes.iter().find(|n| n.name == name).unwrap();
  let (new_group, new_roof, new_wall) = (
    doc.find(roots[0]).unwrap(),
    by_name("roof"),
    by_name("wall"),
  );
  let roof_mat = new_roof.mesh.as_ref().unwrap().prims[0].material;
  let appended = roots.len() == 1
    && ids.len() == 5
    && new_roof.parent == new_group.id
    && new_wall.parent == new_group.id
    && doc.textures.len() == 2
    && doc.materials.len() == 2
    && new_wall.mesh.as_ref().unwrap().prims[0].material == 0
    && doc.materials[roof_mat as usize].name == "roof"
    && doc.textures[doc.materials[roof_mat as usize].texture as usize].name == "roof"
    && Arc::ptr_eq(
      new_wall.mesh.as_ref().unwrap(),
      by_name("wall2").mesh.as_ref().unwrap(),
    );
  assert!(
    appended,
    "levels: a level appended to a document, its textures and materials shared or renumbered"
  );
}

// the cache: made again at a new frame or when nodes come or go, not otherwise
#[test]
fn cache_rules() {
  let mut s = Scene::new();
  let actor = s.add(Node {
    kind: NodeKind::Actor,
    local: Transform {
      t: Vec3::new(1.0, 0.0, 0.0),
      ..Default::default()
    },
    ..Default::default()
  });
  let shared = make_box("model", -1);
  let model = s.add(Node {
    parent: actor,
    kind: NodeKind::Render,
    mesh: Some(shared.clone()),
    ..Default::default()
  });
  let other = s.add(Node {
    kind: NodeKind::Render,
    mesh: Some(shared.clone()),
    ..Default::default()
  });
  let c = s.cache();
  assert_eq!(c.version, 1);
  let (im, io) = (s.index_of(model).unwrap(), s.index_of(other).unwrap());
  assert!(c.actor[im] == actor && c.actor[io] == 0 && c.visible[im]);
  assert_eq!(c.worlds[im].col3(3), Vec3::new(1.0, 0.0, 0.0));
  assert!((c.bounds[im].lo.x - 0.0).abs() < 1e-6 && (c.bounds[im].hi.y - 2.0).abs() < 1e-6);
  assert!(
    c.mesh_groups.len() == 1 && c.mesh_groups[0].1 == [im as u32, io as u32],
    "the instances of a mesh grouped"
  );
  assert_eq!(
    &c.kids[c.kid_start[0] as usize..c.kid_start[1] as usize],
    [im as u32]
  );

  // a transform changed in the same frame: the old matrices; the next frame: the new ones
  s.find_mut(actor).unwrap().local.t.x = 5.0;
  assert!(Arc::ptr_eq(&c, &s.cache()));
  s.begin_frame();
  let c2 = s.cache();
  assert!(
    c2.version == 2 && c2.worlds[im].col3(3).x == 5.0 && c.worlds[im].col3(3).x == 1.0,
    "the old cache still held"
  );
  // a node added in the same frame: made again
  s.find_mut(actor).unwrap().visible = false;
  let hidden = s.add(Node {
    parent: actor,
    ..Default::default()
  });
  let c3 = s.cache();
  assert!(c3.version == 3 && !c3.visible[im] && !c3.visible[s.index_of(hidden).unwrap()]);
  assert_eq!(c3.kid_start[1] - c3.kid_start[0], 2);
  // a node given another mesh: regrouped
  s.find_mut(other).unwrap().mesh = Some(make_box("other", -1));
  s.begin_frame();
  assert_eq!(s.cache().mesh_groups.len(), 2);
  // a reference level: under a group marked og_reference
  let reference = s.add(Node {
    extras: json!({"og_reference": true}),
    ..Default::default()
  });
  let inside = s.add(Node {
    parent: reference,
    ..Default::default()
  });
  let c4 = s.cache();
  assert!(c4.reference[s.index_of(inside).unwrap()] && !c4.reference[im]);
  assert!(
    s.effectively_visible(s.find(other).unwrap()) && !s.effectively_visible(s.find(model).unwrap())
  );
  assert_eq!(s.actor_of(s.find(model).unwrap()), actor);

  // the copy on write of a shared mesh: a new uid
  let uid = shared.uid;
  let mut mesh = s.find(model).unwrap().mesh.clone().unwrap();
  Arc::make_mut(&mut mesh).touch();
  assert!(mesh.uid != uid && mesh.version == 2);
  // set_world: the node's world matrix becomes the one asked for
  let w = Mat4::translate(Vec3::new(3.0, 4.0, 5.0))
    * Mat4::rotate(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.5));
  s.set_world(model, &w);
  let got = s.world_of(model);
  assert!(
    got
      .m
      .iter()
      .zip(w.m.iter())
      .all(|(a, b)| (a - b).abs() < 1e-5)
  );
  assert!(s.index_of(12345).is_none() && s.find(0).is_none());
}

#[test]
fn names_and_settings() {
  for k in NodeKind::ALL {
    assert_eq!(kind_from_name(kind_name(k), NodeKind::Group), k);
    assert_eq!(serde_json::to_value(k).unwrap(), kind_name(k));
  }
  assert_eq!(blend_mode_from_name("add_alpha"), BlendMode::AddAlpha);
  assert_eq!(blend_mode_from_name(""), BlendMode::Alpha);
  let s = LevelSettings {
    level: "desert".into(),
    source_group: "desert".into(),
    replaces: String::new(),
  };
  assert_eq!(
    s.to_json(),
    json!({"level": "desert", "source_group": "desert"})
  );
  let r = LevelSettings {
    replaces: "ctyindb".into(),
    ..s
  };
  assert_eq!(LevelSettings::from_json(&r.to_json()), r);
  assert!(
    instance_tree("ctyinda-tie-2")
      && instance_tree("des-shrub-10")
      && !instance_tree("des-stone-shrub-01.mb")
      && !instance_tree("-tie-1")
  );
  let palette = Arc::new(TodPalette {
    rgba: vec![64; 32],
    local: false,
  });
  let mesh = make_box("lit", -1);
  let tod = TodColors {
    palette: Some(palette),
    index: vec![vec![0; 8]],
  };
  assert!(tod.fits(&mesh));
  let mut scene = Scene::new();
  let id = scene.add(Node {
    kind: NodeKind::Render,
    mesh: Some(mesh),
    tod: Some(Arc::new(tod)),
    ..Default::default()
  });
  let ratios = tod_ratios(&scene, &[id]);
  assert!(ratios.iter().all(|&r| r == 1.0));
  let rgba = tod_from_color(Vec4::new(1.0, 0.5, 0.0, 1.0), &ratios);
  assert_eq!(rgba[..4], [128, 64, 0, 128]);
  let mut boxes: Vec<AABB> = (0..100)
    .map(|i| AABB {
      lo: Vec3::new(i as f32, 0.0, 0.0),
      hi: Vec3::new(i as f32 + 1.0, 1.0, 1.0),
    })
    .collect();
  boxes.push(AABB {
    lo: Vec3::new(1e6, 0.0, 0.0),
    hi: Vec3::new(1e6 + 1.0, 1.0, 1.0),
  }); // a stray piece
  let focus = focus_box(&boxes);
  assert!(focus.hi.x < 200.0 && focus.lo.x >= 0.0);
}
