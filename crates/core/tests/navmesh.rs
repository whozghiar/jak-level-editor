// The selftest's nav mesh checks (src/app/selftest.cpp): a floor around a block, in the game's
// limits; a floor too big for one mesh cut in several; the route table reaches every polygon; a
// node and a project keep them; the blob for the game reads back; the game's own mesh (OGLE_NAV_REFERENCE: a
// decompiler_out/jak3/entities/<level>-nav.json) passes the checks and gets the same polygon grid.

use std::sync::Arc;

use jle_core::history::History;
use jle_core::io::gltf::{ExportOptions, ImportOptions, export_glb, import_gltf};
use jle_core::io::nav_export::{
  NavExportEntry, navmesh_blob, navmesh_grid, navmeshes_of_blob, read_navmesh_blob,
  write_navmesh_blob,
};
use jle_core::math::{AABB, Vec3, Vec4, length};
use jle_core::navmesh::*;
use jle_core::scene::{AlphaMode, BlendMode, Material, Mesh, Node, NodeKind, Primitive, Scene};

/// a floor of 2 * half, with blocks 4 m wide, 3 m high, on a grid (one block in the middle when
/// blocks == 1)
fn floor_with_blocks(half: f32, blocks: usize) -> (Vec<Vec3>, Vec<u32>) {
  let mut v = vec![
    Vec3::new(-half, 0.0, -half),
    Vec3::new(half, 0.0, -half),
    Vec3::new(half, 0.0, half),
    Vec3::new(-half, 0.0, half),
  ];
  let mut t = vec![0, 2, 1, 0, 3, 2];
  let faces = [
    [0, 1, 3, 2],
    [4, 6, 7, 5],
    [0, 4, 5, 1],
    [2, 3, 7, 6],
    [0, 2, 6, 4],
    [1, 5, 7, 3],
  ];
  let step = if blocks > 1 {
    2.0 * half / (blocks + 1) as f32
  } else {
    0.0
  };
  for bx in 0..blocks {
    for bz in 0..blocks {
      let (cx, cz) = if blocks > 1 {
        (
          -half + step * (bx + 1) as f32,
          -half + step * (bz + 1) as f32,
        )
      } else {
        (0.0, 0.0)
      };
      let b = v.len() as u32;
      for i in 0..8 {
        v.push(Vec3::new(
          cx + if i & 1 != 0 { 2.0 } else { -2.0 },
          if i & 2 != 0 { 3.0 } else { 0.0 },
          cz + if i & 4 != 0 { 2.0 } else { -2.0 },
        ));
      }
      for f in faces {
        t.extend([b + f[0], b + f[1], b + f[2], b + f[0], b + f[2], b + f[3]]);
      }
    }
  }
  (v, t)
}

fn route_reaches_all(m: &NavMeshData) -> bool {
  let route = navmesh_route_table(m);
  let n = m.polys.len();
  for from in 0..n {
    for to in 0..n {
      let (mut cur, mut steps) = (from, 0);
      while cur != to && steps < n {
        steps += 1;
        let bit = (to + cur * n) * 2;
        let edge = ((route[bit >> 3] >> (bit & 7)) & 3) as usize;
        if edge >= m.polys[cur].count || m.polys[cur].adj[edge] < 0 {
          return false;
        }
        cur = m.polys[cur].adj[edge] as usize;
      }
      if cur != to {
        return false;
      }
    }
  }
  true
}

fn region(lo: [f32; 3], hi: [f32; 3]) -> AABB {
  let mut r = AABB::default();
  r.add(lo.into());
  r.add(hi.into());
  r
}

fn floor_meshes() -> Vec<NavMeshData> {
  let (v, t) = floor_with_blocks(20.0, 1);
  let r = region([-20.0, -1.0, -20.0], [20.0, 4.0, 20.0]);
  build_navmeshes(&v, &t, &r, &NavParams::default())
    .expect("built")
    .0
}

#[test]
fn floor_around_a_block() {
  let meshes = floor_meshes();
  assert!(!meshes.is_empty());
  let mut area = 0.0;
  for m in &meshes {
    assert_eq!(navmesh_problem(m), None);
    area += m.area();
    for p in &m.polys {
      let mut c = Vec3::default();
      for k in 0..p.count {
        c += m.verts[p.v[k]];
      }
      c = c / p.count as f32 + m.origin;
      assert!(
        c.x.abs() >= 2.0 || c.z.abs() >= 2.0,
        "a polygon in the block"
      );
    }
  }
  // the floor without its border and the block, both widened by the creature's radius
  assert!(area > 1300.0 && area < 1520.0, "area {area}");
  assert!(
    route_reaches_all(&meshes[0]),
    "the route table leads from every polygon to every other"
  );
}

#[test]
fn walkable_materials() {
  // what the extractor writes for the decor (blended with its texture's alpha, color x2) is
  // walked on; the water the editor draws and the glows are not
  let decor = Material {
    alpha: AlphaMode::Blend,
    base_color: Vec4::new(2.0, 2.0, 2.0, 2.0),
    ..Default::default()
  };
  let water = Material {
    alpha: AlphaMode::Blend,
    base_color: Vec4::new(1.1, 1.6, 2.0, 0.75),
    ..Default::default()
  };
  let glow = Material {
    blend: BlendMode::Add,
    ..decor.clone()
  };
  assert!(!navmesh_ignores(&decor) && navmesh_ignores(&water) && navmesh_ignores(&glow));
}

#[test]
fn only_the_ground_next_to_a_point() {
  // a roof apart from the floor: kept, unless only the ground next to a point of the floor is
  // wanted (a point in the block, over a hole of the nav mesh, finds the floor around it)
  let (mut v, mut t) = floor_with_blocks(20.0, 1);
  let b = v.len() as u32;
  v.extend([
    Vec3::new(30.0, 6.0, -10.0),
    Vec3::new(50.0, 6.0, -10.0),
    Vec3::new(50.0, 6.0, 10.0),
    Vec3::new(30.0, 6.0, 10.0),
  ]);
  t.extend([b, b + 2, b + 1, b, b + 3, b + 2]);
  let r = region([-20.0, -1.0, -20.0], [50.0, 8.0, 20.0]);
  let (both, _) = build_navmeshes(&v, &t, &r, &NavParams::default()).unwrap();
  let from_floor = NavParams {
    keep_from: vec![Vec3::new(0.0, 0.0, 0.0)],
    ..Default::default()
  };
  let (floor_only, st) = build_navmeshes(&v, &t, &r, &from_floor).unwrap();
  assert_eq!(both.len(), 2);
  assert_eq!(floor_only.len(), 1);
  assert_eq!(st.left_out, 1);
  assert!(floor_only[0].origin.y < 1.0);
}

#[test]
fn big_floor_cut_in_several_meshes() {
  let (v, t) = floor_with_blocks(150.0, 12);
  let r = region([-150.0, -1.0, -150.0], [150.0, 4.0, 150.0]);
  let params = NavParams {
    max_edge: 4.0,
    ..Default::default()
  };
  let (big, st) = build_navmeshes(&v, &t, &r, &params).unwrap();
  assert!(
    big.len() >= 2 && st.split >= 1,
    "{} meshes, {st:?}",
    big.len()
  );
  for m in &big {
    assert!(m.polys.len() <= 255);
    assert_eq!(navmesh_problem(m), None);
  }
}

#[test]
fn kept_by_its_node() {
  let meshes = floor_meshes();
  let node = navmesh_node(&meshes[0], "navmesh-test");
  let back = navmesh_of_node(&node).expect("read back");
  assert_eq!(back.verts.len(), meshes[0].verts.len());
  assert_eq!(back.polys.len(), meshes[0].polys.len());
  for (a, b) in back.polys.iter().zip(&meshes[0].polys) {
    assert_eq!((a.count, a.v, a.adj), (b.count, b.v, b.adj));
  }
  assert_eq!(node.local.t, meshes[0].origin);
  assert_eq!(node.mesh.as_ref().unwrap().prims[0].tri_count(), {
    meshes[0].polys.iter().map(|p| p.count - 2).sum::<usize>()
  });

  // in the document: the meshes of the region replaced in one undo step, each exported where it
  // is now
  let mut scene = Scene::new();
  let old = scene.add(navmesh_node(&meshes[0], "navmesh"));
  let mut far = meshes[0].clone();
  far.origin = Vec3::new(500.0, 0.0, 0.0);
  let away = scene.add(navmesh_node(&far, "navmesh-2"));
  let mut history = History::default();
  let r = region([-20.0, -1.0, -20.0], [20.0, 4.0, 20.0]);
  let (added, entry) = replace_navmeshes(&mut scene, &r, &meshes);
  history.push(entry);
  assert_eq!(added.len(), meshes.len());
  assert!(scene.find(old).is_none() && scene.find(away).is_some());
  assert_eq!(scene.find(added[0]).unwrap().name, "navmesh");
  assert_eq!(navmesh_nodes(&scene).len(), 1 + meshes.len());
  scene.find_mut(added[0]).unwrap().local.t += Vec3::new(10.0, 0.0, 0.0);
  let (world, names) = world_navmeshes(&scene);
  let i = names.iter().position(|n| n == "navmesh").unwrap();
  assert!(length(world[i].origin - (meshes[0].origin + Vec3::new(10.0, 0.0, 0.0))) < 1e-3);
  history.undo(&mut scene);
  assert!(scene.find(old).is_some() && scene.find(added[0]).is_none());
}

#[test]
fn kept_by_a_project() {
  // what save_project and open_project do (a Project glb, read with its settings)
  let meshes = floor_meshes();
  let mut scene = Scene::new();
  scene.add(navmesh_node(&meshes[0], "navmesh-test"));
  let path = std::env::temp_dir().join(format!("jle-navmesh-{}.glb", std::process::id()));
  export_glb(&scene, &path, &ExportOptions::default()).unwrap();
  let mut back = Scene::new();
  let read = import_gltf(
    &mut back,
    &path,
    &ImportOptions {
      read_settings: true,
      ..Default::default()
    },
  );
  let _ = std::fs::remove_file(&path);
  read.unwrap();
  let node = back
    .nodes
    .iter()
    .find(|n| n.kind == NodeKind::NavMesh)
    .expect("a nav mesh node");
  let reopened = navmesh_of_node(node).expect("its polygons");
  assert_eq!(reopened.polys, meshes[0].polys);
  assert!(length(back.world(node).col3(3) - meshes[0].origin) < 1e-3);
}

#[test]
fn walkable_surfaces_of_the_document() {
  // a floor and water over it: the water is left out
  let mut scene = Scene::new();
  let water = scene.add_material(Material {
    alpha: AlphaMode::Blend,
    base_color: Vec4::new(1.1, 1.6, 2.0, 0.75),
    ..Default::default()
  });
  let quad = |y: f32, material: i32| {
    let mut m = Mesh::new();
    m.prims.push(Primitive {
      pos: vec![
        Vec3::new(-5.0, y, -5.0),
        Vec3::new(5.0, y, -5.0),
        Vec3::new(5.0, y, 5.0),
        Vec3::new(-5.0, y, 5.0),
      ],
      idx: vec![0, 2, 1, 0, 3, 2],
      material,
      ..Default::default()
    });
    Node {
      kind: NodeKind::Render,
      mesh: Some(Arc::new(m)),
      ..Default::default()
    }
  };
  let floor = scene.add(quad(0.0, -1));
  scene.add(quad(1.0, water));
  scene.find_mut(floor).unwrap().local.t = Vec3::new(100.0, 0.0, 0.0);
  // a death zone (an invisible fall) and decor Jak goes through: no creature walks on them
  let mut zone = quad(-20.0, -1);
  zone.extras =
    serde_json::json!({"og_decor": {"collision": {"kind": "invisible", "event": "endlessfall"}}});
  scene.add(zone);
  let mut ghost = quad(3.0, -1);
  ghost.extras = serde_json::json!({"og_decor": {"collision": {"kind": "none"}}});
  scene.add(ghost);
  let (v, t, r) = navmesh_surfaces(&scene, &AABB::default(), &NavParams::default());
  assert_eq!((v.len(), t.len()), (4, 6));
  assert!(v.iter().all(|p| p.x >= 95.0) && r.lo.x >= 95.0 - 1e-3);
  // a region away from the floor: nothing
  let (_, t, _) = navmesh_surfaces(
    &scene,
    &region([-5.0, 0.0, -5.0], [5.0, 1.0, 5.0]),
    &NavParams::default(),
  );
  assert!(t.is_empty());
  // a level of the game with its collision: a roof of its decor the game gave no collision (or one
  // of a wall's mode) is no ground; its collision's street is; the collision of a platform deleted
  // from its decor is not
  let mut scene = Scene::new();
  let group = |scene: &mut Scene, name: &str, parent: u32| {
    scene.add(Node {
      name: name.into(),
      kind: NodeKind::Group,
      parent,
      ..Default::default()
    })
  };
  let level = group(&mut scene, "ctysluma", 0);
  let decor = group(&mut scene, "ctysluma-tfrag-0", level);
  for y in [10.0, 0.0] {
    let mut drawn = quad(y, -1);
    drawn.parent = decor;
    scene.add(drawn);
  }
  let collision = group(&mut scene, "ctysluma-collision", level);
  for y in [0.0, 5.0] {
    let mut floor = quad(y, -1);
    floor.kind = NodeKind::Collision;
    floor.parent = collision;
    scene.add(floor);
  }
  // the roof's flat top in the collision, but of a wall's mode (pat mode 1): no floor in the game
  let mut top = quad(10.0, -1);
  top.kind = NodeKind::Collision;
  top.parent = collision;
  let mut m = (**top.mesh.as_ref().unwrap()).clone();
  m.prims[0].tri_pat = vec![1 << 7, 1 << 7];
  top.mesh = Some(Arc::new(m));
  scene.add(top);
  let (v, t, _) = navmesh_surfaces(&scene, &AABB::default(), &NavParams::default());
  assert_eq!(t.len(), 6);
  assert!(t.iter().all(|&i| v[i as usize].y.abs() < 1e-3), "{v:?}");
}

#[test]
fn blob_for_the_game_reads_back() {
  let meshes = floor_meshes();
  let entries = [NavExportEntry {
    mesh: &meshes[0],
    aid: 61000,
    name: "navmesh-test".into(),
  }];
  let view = read_navmesh_blob(&navmesh_blob(&entries)).expect("blob read");
  let from_blob = navmeshes_of_blob(&view).expect("meshes read");
  assert_eq!(from_blob.len(), 1);
  assert_eq!(from_blob[0].polys.len(), meshes[0].polys.len());
  let ent = view.roots[0].1;
  let mesh_ptr = view.ptr(ent + 48).unwrap();
  assert_eq!(view.u32(ent + 44), 61000);
  assert_eq!(view.u8(mesh_ptr + 9) as usize, meshes[0].polys.len());
  assert!(view.ptr(mesh_ptr + 12).is_some() && view.ptr(mesh_ptr + 16).is_some());
  assert!(length(from_blob[0].origin - meshes[0].origin) < 1e-3);
  for (a, b) in from_blob[0].polys.iter().zip(&meshes[0].polys) {
    for k in 0..a.count {
      assert_eq!(a.adj[k], b.adj[k]);
      assert!(length(from_blob[0].verts[a.v[k]] - meshes[0].verts[b.v[k]]) < 1e-3);
    }
  }
  // res tags sorted as the game looks them up
  let tags: Vec<&str> = view
    .symbols
    .iter()
    .map(|(_, s)| s.as_str())
    .filter(|s| *s != "#f")
    .collect();
  assert_eq!(tags, ["name", "nearest-y-threshold", "nav-max-users"]);

  // the file: written once the meshes suit the game
  let path = std::env::temp_dir().join(format!("jle-navmesh-{}.json", std::process::id()));
  write_navmesh_blob(&entries, &path).unwrap();
  let written: serde_json::Value =
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
  let _ = std::fs::remove_file(&path);
  assert_eq!(written, navmesh_blob(&entries));
  let bad = NavMeshData::default();
  let bad_entry = [NavExportEntry {
    mesh: &bad,
    aid: 1,
    name: "bad".into(),
  }];
  assert_eq!(
    write_navmesh_blob(&bad_entry, &path),
    Err("bad: no polygon".into())
  );
}

#[test]
fn the_games_own_meshes() {
  let Ok(reference) = std::env::var("OGLE_NAV_REFERENCE") else {
    eprintln!("OGLE_NAV_REFERENCE not set: skipped");
    return;
  };
  let j: serde_json::Value =
    serde_json::from_str(&std::fs::read_to_string(&reference).unwrap()).unwrap();
  let game = read_navmesh_blob(&j).expect("blob read");
  let game_meshes = navmeshes_of_blob(&game).expect("meshes read");
  assert!(!game_meshes.is_empty());
  // every mesh of the level (the roots are in the order navmeshes_of_blob reads them); what
  // differs is said
  let roots: Vec<usize> = game
    .roots
    .iter()
    .filter(|(name, _)| name.starts_with("nav-mesh-"))
    .map(|r| r.1)
    .collect();
  let mut same_grid = true;
  for (m, mesh) in game_meshes.iter().enumerate() {
    // what the editor asks of its own meshes, the game's do not always have (a polygon facing
    // down, a mesh in several pieces): said, not a failure
    let note = navmesh_problem(mesh)
      .or_else(|| (!route_reaches_all(mesh)).then_some("its route does not lead everywhere"));
    if let Some(note) = note {
      println!(
        "    nav mesh {m} of {}, the game's own: {note}",
        game_meshes.len()
      );
    }
    let g = navmesh_grid(mesh);
    let hash = game.ptr(game.ptr(roots[m] + 48).unwrap() + 16).unwrap();
    let buckets = game.ptr(hash - 4 + 32).unwrap();
    let dims = [
      game.u8(hash - 4 + 28) as i32,
      game.u8(hash - 4 + 29) as i32,
      game.u8(hash - 4 + 30) as i32,
    ];
    let mut why = String::new();
    if dims != g.dims {
      why = format!("grid {:?}, the game's {dims:?}", g.dims);
    }
    if let Some(i) = (0..g.buckets.len()).find(|&i| game.u8(buckets + i) != g.buckets[i])
      && why.is_empty()
    {
      why = format!("grid byte {i} differs");
    }
    if !why.is_empty() {
      println!(
        "    nav mesh {m} of {} ({} polygons): {why}",
        game_meshes.len(),
        mesh.polys.len()
      );
      same_grid = false;
    }
  }
  assert!(
    same_grid,
    "the game's own meshes read back, the same grid-hash for each"
  );
}

/// `cargo test --release -p jak-level-editor-core --test navmesh -- --ignored --nocapture`: a real
/// level's walkable surfaces (the industrial district, when the library is there), and a synthetic
/// district
#[test]
#[ignore]
fn generation_time() {
  let library = concat!(env!("CARGO_MANIFEST_DIR"), "/../../library/jak3");
  if std::path::Path::new(library).join("library.json").exists() {
    let mut scene = Scene::new();
    import_gltf(
      &mut scene,
      format!("{library}/ctyinda/decor/ctyinda-background.glb"),
      &ImportOptions::default(),
    )
    .unwrap();
    let (v, t, r) = navmesh_surfaces(&scene, &AABB::default(), &NavParams::default());
    let (meshes, st) = build_navmeshes(&v, &t, &r, &NavParams::default()).unwrap();
    println!(
      "ctyinda: {} triangles, grid {}x{}, {} Recast polygons, {} meshes ({} cut), {} polygons, {:.0} m2, {:.0} ms",
      st.triangles,
      st.grid_width,
      st.grid_depth,
      st.recast_polys,
      meshes.len(),
      st.split,
      st.polys,
      meshes.iter().map(NavMeshData::area).sum::<f32>(),
      st.ms
    );
    for m in &meshes {
      assert_eq!(navmesh_problem(m), None);
    }
  }

  // a district of 600 m with 30 x 30 blocks
  let (v, t) = floor_with_blocks(300.0, 30);
  let r = region([-300.0, -1.0, -300.0], [300.0, 4.0, 300.0]);
  let (meshes, st) = build_navmeshes(&v, &t, &r, &NavParams::default()).unwrap();
  println!(
    "synthetic floor: {} triangles, grid {}x{}, {} Recast polygons, {} meshes ({} cut), {} polygons, {:.0} ms",
    st.triangles,
    st.grid_width,
    st.grid_depth,
    st.recast_polys,
    meshes.len(),
    st.split,
    st.polys,
    st.ms
  );
  for m in &meshes {
    assert_eq!(navmesh_problem(m), None);
  }
}
