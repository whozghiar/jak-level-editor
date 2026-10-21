//! The decor of a level as the assemblies its artists built: the elements touching each other, a
//! house with its facades, its roof, its windows and what hangs on them, a bridge with its pillars,
//! a tower with its balconies. What selects a whole structure in one click and what the game's
//! prefabs are made of (io/prefab_recipes).

use super::{NodeKind, Scene};
use crate::math::AABB;

/// The widest assembly (meters) find_assemblies keeps whole: the C++ default.
pub const K_ASSEMBLY_MAX_SIZE: f32 = 60.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
  Solid,
  Flat,
  Thin,
}

/// meters between two boxes that touch
const K_TOUCH: f32 = 0.1;

// the ground's details: never what holds an assembly together, nor a part of one
fn ground_detail(name: &str) -> bool {
  const K_WORDS: [&str; 20] = [
    "ground", "dirt", "pebble", "rubble", "grass", "decal", "stain", "blotch", "shadow", "puddle",
    "crack", "moss", "road", "sand", "debris", "weed", "blend", "cattail", "leaves", "foliage",
  ];
  let l = name.to_ascii_lowercase();
  K_WORDS.iter().any(|w| l.contains(w))
}

fn shape_of(b: &AABB) -> Shape {
  let s = b.size();
  let mut a = [s.x, s.y, s.z];
  a.sort_by(f32::total_cmp);
  if a[2] > 6.0 * a[1].max(0.3) {
    return Shape::Thin;
  }
  if s.y < 1.0 && s.x.max(s.z) > 3.0 * s.y.max(0.1) {
    return Shape::Flat;
  }
  Shape::Solid
}

fn touch(a: &AABB, b: &AABB) -> bool {
  a.lo.x <= b.hi.x + K_TOUCH
    && b.lo.x <= a.hi.x + K_TOUCH
    && a.lo.y <= b.hi.y + K_TOUCH
    && b.lo.y <= a.hi.y + K_TOUCH
    && a.lo.z <= b.hi.z + K_TOUCH
    && b.lo.z <= a.hi.z + K_TOUCH
}

// what lies flat links to what holds it, not to what stands on it
fn links(a: &AABB, sa: Shape, b: &AABB, sb: Shape) -> bool {
  if sa == Shape::Flat && b.center().y > a.center().y {
    return false;
  }
  if sb == Shape::Flat && a.center().y > b.center().y {
    return false;
  }
  true
}

struct Elements {
  bx: Vec<AABB>,
  shape: Vec<Shape>,
}

// the elements `items` (indices of `e`) touching each other, in groups
fn components(e: &Elements, mut items: Vec<u32>) -> Vec<Vec<u32>> {
  items.sort_by(|&a, &b| e.bx[a as usize].lo.x.total_cmp(&e.bx[b as usize].lo.x));
  let mut root: Vec<u32> = (0..items.len() as u32).collect();
  fn find(root: &mut [u32], mut a: u32) -> u32 {
    while root[a as usize] != a {
      root[a as usize] = root[root[a as usize] as usize];
      a = root[a as usize];
    }
    a
  }
  // positions in items whose box reaches this far along x
  let mut active: Vec<u32> = Vec::new();
  for k in 0..items.len() as u32 {
    let a = &e.bx[items[k as usize] as usize];
    active.retain(|&j| e.bx[items[j as usize] as usize].hi.x + K_TOUCH >= a.lo.x);
    for &j in &active {
      let b = &e.bx[items[j as usize] as usize];
      if touch(a, b)
        && links(
          a,
          e.shape[items[k as usize] as usize],
          b,
          e.shape[items[j as usize] as usize],
        )
      {
        let (rk, rj) = (find(&mut root, k), find(&mut root, j));
        root[rk as usize] = rj;
      }
    }
    active.push(k);
  }
  let mut out: Vec<Vec<u32>> = Vec::new();
  let mut group: Vec<Option<usize>> = vec![None; items.len()];
  for k in 0..items.len() as u32 {
    let r = find(&mut root, k) as usize;
    let g = *group[r].get_or_insert_with(|| {
      out.push(Vec::new());
      out.len() - 1
    });
    out[g].push(items[k as usize]);
  }
  out
}

// Where to cut `items` in two along `axis` (0: x, 2: z): the plane the fewest of their boxes cross,
// between the 20th and 80th percentiles of their middles (a street, the seam between two buildings,
// with elements on both sides), the nearest the middle among equals. Returns its cost (boxes
// crossing it) and the plane.
fn best_cut(e: &Elements, items: &[u32], axis: usize) -> (f32, f32) {
  let get = |v: crate::math::Vec3| if axis == 0 { v.x } else { v.z };
  let mut los: Vec<f32> = items
    .iter()
    .map(|&i| get(e.bx[i as usize].lo) + K_TOUCH)
    .collect();
  let mut his: Vec<f32> = items
    .iter()
    .map(|&i| get(e.bx[i as usize].hi) - K_TOUCH)
    .collect();
  let mut mids: Vec<f32> = items
    .iter()
    .map(|&i| get(e.bx[i as usize].center()))
    .collect();
  los.sort_by(f32::total_cmp);
  his.sort_by(f32::total_cmp);
  mids.sort_by(f32::total_cmp);
  let (from, to) = (mids[mids.len() / 5], mids[mids.len() * 4 / 5]);
  let mid = (from + to) * 0.5;
  let mut best = 1e30f32;
  let mut at = 0.0;
  let mut p = from;
  while p <= to {
    // crossing: starts before p and ends after it
    let n = los.partition_point(|&x| x < p) as f32 - his.partition_point(|&x| x <= p) as f32;
    let cost = n + (p - mid).abs() / (to - from).max(1.0); // the middle among equals
    if cost < best {
      best = cost;
      at = p;
    }
    p += 0.5;
  }
  (best, at)
}

// the groups of `items`; the ones wider than max_size cut in two where the fewest elements cross
// (each element on the side of its middle), again until they fit
fn split(e: &Elements, items: Vec<u32>, max_size: f32, out: &mut Vec<Vec<u32>>, depth: i32) {
  for c in components(e, items) {
    let mut b = AABB::default();
    for &i in &c {
      b.add_box(&e.bx[i as usize]);
    }
    let s = b.size();
    if (s.x.max(s.z) <= max_size && s.y <= 2.0 * max_size) || c.len() < 2 || depth > 24 {
      out.push(c);
      continue;
    }
    let (cost_x, at_x) = if s.x > max_size * 0.5 {
      best_cut(e, &c, 0)
    } else {
      (1e30, 0.0)
    };
    let (cost_z, at_z) = if s.z > max_size * 0.5 {
      best_cut(e, &c, 2)
    } else {
      (1e30, 0.0)
    };
    // the cheaper cut; along the longer side when both cost the same
    let along_x = cost_x < cost_z || (cost_x == cost_z && s.x >= s.z);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for &i in &c {
      let m = e.bx[i as usize].center();
      let below = if along_x { m.x < at_x } else { m.z < at_z };
      if below { &mut left } else { &mut right }.push(i);
    }
    if left.is_empty() || right.is_empty() {
      out.push(c);
      continue;
    }
    split(e, left, max_size, out, depth + 1);
    split(e, right, max_size, out, depth + 1);
  }
}

/// Every assembly of the scene's decor (the render nodes that are not the ground, an actor's
/// model, the far scenery nor a reference level), the ones with the most elements first, each
/// one's elements by id. Two elements link when their boxes touch, but:
/// - what lies flat (a sidewalk, a roof, a floor plate) links only to what holds it, under it: a
///   house standing on a sidewalk is not linked to the next house by it;
/// - what is long and thin (a pipe or a cable from a house to the next, a lamp post) and the
///   ground's details (dirt, pebbles, grass, decals...) do not link;
///
/// they join the assembly whose box holds their middle (the smallest), else stay alone. In a city
/// everything touches (the houses of a block side by side, a wall along a canal): an assembly
/// wider than `max_size` meters (K_ASSEMBLY_MAX_SIZE) is cut in two where the fewest elements
/// cross (a street, the seam between two buildings), again until its parts fit.
pub fn find_assemblies(scene: &mut Scene, max_size: f32) -> Vec<Vec<u32>> {
  scene.begin_frame();
  let c = scene.cache();
  let mut e = Elements {
    bx: Vec::new(),
    shape: Vec::new(),
  };
  let (mut ids, mut linking, mut loose, mut details) =
    (Vec::new(), Vec::new(), Vec::new(), Vec::new());
  for (i, n) in scene.nodes.iter().enumerate() {
    let Some(mesh) = n.mesh.as_ref() else {
      continue;
    };
    if n.kind != NodeKind::Render || n.locked || !c.visible[i] || c.actor[i] != 0 || c.reference[i]
    {
      continue;
    }
    // the far scenery
    if mesh
      .extras
      .get("og_backdrop")
      .and_then(|v| v.as_bool())
      .unwrap_or(false)
    {
      continue;
    }
    let b = c.bounds[i];
    if !b.valid() || b.size().x.max(b.size().y).max(b.size().z) > 3.0 * max_size {
      continue;
    }
    let k = ids.len() as u32;
    ids.push(n.id);
    e.bx.push(b);
    e.shape.push(shape_of(&b));
    if ground_detail(&n.name) {
      details.push(k);
    } else if e.shape[k as usize] == Shape::Thin {
      loose.push(k);
    } else {
      linking.push(k);
    }
  }
  let mut groups: Vec<Vec<u32>> = Vec::new();
  split(&e, linking, max_size, &mut groups, 0);
  // the loose elements join the smallest group whose box holds their middle, and is not much
  // smaller than them
  let group_box: Vec<AABB> = groups
    .iter()
    .map(|g| {
      let mut b = AABB::default();
      for &i in g {
        b.add_box(&e.bx[i as usize]);
      }
      b
    })
    .collect();
  let volume: Vec<f32> = group_box
    .iter()
    .map(|b| {
      let s = b.size();
      (s.x + 1.0) * (s.y + 1.0) * (s.z + 1.0)
    })
    .collect();
  let mut joined: Vec<Vec<u32>> = vec![Vec::new(); groups.len()];
  for i in loose {
    let (m, s) = (e.bx[i as usize].center(), e.bx[i as usize].size());
    let mut best: Option<usize> = None;
    for (g, gb) in group_box.iter().enumerate() {
      let gs = gb.size();
      if m.x < gb.lo.x - 1.0
        || m.x > gb.hi.x + 1.0
        || m.y < gb.lo.y - 1.0
        || m.y > gb.hi.y + 1.0
        || m.z < gb.lo.z - 1.0
        || m.z > gb.hi.z + 1.0
      {
        continue;
      }
      if s.x > gs.x * 1.25 + 1.0 || s.y > gs.y * 1.25 + 1.0 || s.z > gs.z * 1.25 + 1.0 {
        continue;
      }
      if best.is_none_or(|b| volume[g] < volume[b]) {
        best = Some(g);
      }
    }
    match best {
      Some(b) => joined[b].push(i),
      None => groups.push(vec![i]),
    }
  }
  groups.extend(details.into_iter().map(|i| vec![i]));
  let mut out: Vec<Vec<u32>> = groups
    .iter()
    .enumerate()
    .map(|(g, members)| {
      let mut m: Vec<u32> = members
        .iter()
        .chain(joined.get(g).into_iter().flatten())
        .map(|&i| ids[i as usize])
        .collect();
      m.sort_unstable();
      m
    })
    .collect();
  out.sort_by_key(|a| std::cmp::Reverse(a.len()));
  out
}

/// The assembly of `assemblies` holding the element `id` (empty when none does).
pub fn assembly_of(assemblies: &[Vec<u32>], id: u32) -> Vec<u32> {
  assemblies
    .iter()
    .find(|a| a.binary_search(&id).is_ok())
    .cloned()
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeSet;
  use std::sync::Arc;

  use super::*;
  use crate::math::Vec3;
  use crate::scene::{Mesh, Node, Primitive};

  // a box of size 2, from y = 0 to 2
  fn make_box(name: &str) -> Arc<Mesh> {
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
    for f in [
      [0, 1, 3, 2],
      [4, 6, 7, 5],
      [0, 4, 5, 1],
      [2, 3, 7, 6],
      [0, 2, 6, 4],
      [1, 5, 7, 3],
    ] {
      p.idx.extend([f[0], f[1], f[2], f[0], f[2], f[3]]);
    }
    mesh.prims.push(p);
    Arc::new(mesh)
  }

  #[test]
  fn assemblies() {
    // two houses (four walls and a roof each) on one sidewalk, a pipe from one to the other, a
    // lamp post in the first, dirt on the ground; a row of blocks 200 m long
    let mut s = Scene::new();
    // a box of that size, its base at `at`
    let mut put = |name: &str, at: [f32; 3], size: [f32; 3]| {
      let mut n = Node {
        name: name.into(),
        kind: NodeKind::Render,
        mesh: Some(make_box(name)),
        ..Default::default()
      };
      n.local.t = Vec3::new(at[0], at[1], at[2]);
      n.local.s = Vec3::new(size[0], size[1], size[2]) * 0.5;
      s.add(n)
    };
    let mut house: [Vec<u32>; 2] = Default::default();
    for (h, walls) in house.iter_mut().enumerate() {
      let x = h as f32 * 30.0;
      let p = if h == 1 { "B-" } else { "A-" };
      *walls = vec![
        put(&format!("{p}front"), [x, 0.0, 0.0], [10.0, 6.0, 1.0]),
        put(&format!("{p}back"), [x, 0.0, 8.0], [10.0, 6.0, 1.0]),
        put(&format!("{p}left"), [x - 5.0, 0.0, 4.0], [1.0, 6.0, 9.0]),
        put(&format!("{p}right"), [x + 5.0, 0.0, 4.0], [1.0, 6.0, 9.0]),
        put(&format!("{p}roof"), [x, 6.0, 4.0], [11.0, 0.5, 10.0]),
      ];
    }
    house[0].push(put("A-lamp-post", [2.0, 0.0, 4.0], [0.3, 4.0, 0.3]));
    let sidewalk = put("sidewalk", [15.0, -0.3, -1.0], [40.0, 0.3, 10.0]);
    let pipe = put("pipe", [15.0, 3.0, -0.75], [30.0, 0.5, 0.5]);
    let dirt = put("street-dirt-01", [0.0, 0.0, 4.0], [2.0, 0.05, 2.0]);
    let row: Vec<u32> = (0..20)
      .map(|b| {
        put(
          &format!("block-{b}"),
          [100.0 + b as f32 * 10.0, 0.0, 50.0],
          [10.0, 5.0, 4.0],
        )
      })
      .collect();
    let found = find_assemblies(&mut s, K_ASSEMBLY_MAX_SIZE);
    let sorted = |mut v: Vec<u32>| {
      v.sort();
      v
    };
    let (a, b) = (
      assembly_of(&found, house[0][0]),
      assembly_of(&found, house[1][0]),
    );
    let (mut row_ok, mut row_seen, mut row_parts) = (true, BTreeSet::new(), 0);
    let cache = s.cache();
    for part in found.iter().filter(|p| row.contains(&p[0])) {
      row_parts += 1;
      let mut bx = AABB::default();
      for id in part {
        row_ok &= row.contains(id) && row_seen.insert(*id);
        bx.add_box(&cache.bounds[s.index_of(*id).unwrap()]);
      }
      row_ok &= bx.size().x <= 60.5;
    }
    assert!(
      a == sorted(house[0].clone())
        && b == sorted(house[1].clone())
        && assembly_of(&found, sidewalk) == [sidewalk]
        && assembly_of(&found, pipe) == [pipe]
        && assembly_of(&found, dirt) == [dirt]
        && row_ok
        && row_seen.len() == row.len()
        && row_parts >= 4,
      "assemblies: a house its walls, roof and lamp, not the sidewalk under both nor the pipe \
       between them nor the dirt; a row 200 m long cut in parts of 60 m at most"
    );
    assert!(assembly_of(&found, 9999).is_empty());
  }
}
