//! Holes in the decor: where a player walking the level would see the void (nothing drawn behind,
//! the screen's background), a visual bug in the game. From points of the ground every few meters
//! (where Jak's eyes are), rays go around and downward; a ray that meets nothing is a view into the
//! void. The rays around it tell where the hole is: a gap between pieces (something on both sides
//! of it, the void right behind), or an open edge (the ground ends: seen directly, or through a
//! passage between pieces, then where that ground ends).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::{AlphaMode, BlendMode, NodeKind, Scene, SceneCache};
use crate::math::{AABB, Mat4, Ray, Vec3, cross, dot, normalize, ray_aabb, transform_ray};

#[derive(Clone, Debug)]
pub struct GapOptions {
  /// where the player can go (its ground is found inside)
  pub region: AABB,
  /// meters between the points looked from (0: so that there are ~400)
  pub spacing: f32,
  /// meters above the ground
  pub eye: f32,
  /// meters: what is farther counts as nothing
  pub reach: f32,
  /// the ocean's height when the level has one: it hides what is under it
  pub ocean: f32,
  /// elements not looked at (and what is under them)
  pub ignore: Vec<u32>,
}

impl Default for GapOptions {
  fn default() -> Self {
    Self {
      region: AABB::default(),
      spacing: 0.0,
      eye: 1.6,
      reach: 300.0,
      ocean: -1e30,
      ignore: Vec::new(),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GapSpot {
  /// about where the hole is (the surfaces around it, or the edge crossed)
  pub at: Vec3,
  /// a point of the ground it is seen from (the eye's height)
  pub seen_from: Vec3,
  /// looking that way
  pub direction: Vec3,
  /// how many rays went through it
  pub rays: i32,
  /// an open edge of the level (no surface around), not a gap between pieces
  pub edge: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GapReport {
  /// the most seen first
  pub spots: Vec<GapSpot>,
  pub viewpoints: usize,
  pub rays: usize,
  pub escaped: usize,
}

const K_CELL: f32 = 16.0;

struct Item {
  index: u32,
  bx: AABB,
  inverse: Mat4,
}

/// The surfaces the player sees, in a grid of 16 m cells over the ground plan: a ray tests the
/// elements of the cells it crosses, the nearest first, instead of every element of the level.
struct Caster<'a> {
  items: Vec<Item>,
  /// into items
  cells: HashMap<(i32, i32), Vec<u32>>,
  stamp: Vec<u32>,
  ray_id: u32,
  scene: &'a Scene,
  cache: Arc<SceneCache>,
  ocean: f32,
}

fn cell_of(p: Vec3) -> (i32, i32) {
  ((p.x / K_CELL).floor() as i32, (p.z / K_CELL).floor() as i32)
}

impl<'a> Caster<'a> {
  fn new(s: &'a Scene, area: &AABB, ignore: &[u32], ocean: f32) -> Self {
    let cache = s.cache();
    let mut skip: HashSet<u32> = HashSet::new();
    for &id in ignore {
      let Some(i) = s.index_of(id) else { continue };
      let mut sub = Vec::new();
      s.subtree(i, &mut sub);
      skip.extend(sub);
    }
    let mut items = Vec::new();
    let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    for (i, n) in s.nodes.iter().enumerate().take(cache.count) {
      if n.kind != NodeKind::Render
        || n.mesh.is_none()
        || !cache.visible[i]
        || skip.contains(&(i as u32))
      {
        continue;
      }
      if cache.reference.len() == cache.count && cache.reference[i] {
        continue;
      }
      let b = cache.bounds[i];
      if !b.valid()
        || b.hi.x < area.lo.x
        || b.lo.x > area.hi.x
        || b.hi.z < area.lo.z
        || b.lo.z > area.hi.z
      {
        continue;
      }
      let k = items.len() as u32;
      items.push(Item {
        index: i as u32,
        bx: b,
        inverse: cache.worlds[i].inverse(),
      });
      let (lo, hi) = (cell_of(b.lo), cell_of(b.hi));
      for x in lo.0..=hi.0 {
        for z in lo.1..=hi.1 {
          cells.entry((x, z)).or_default().push(k);
        }
      }
    }
    let stamp = vec![0; items.len()];
    Self {
      items,
      cells,
      stamp,
      ray_id: 0,
      scene: s,
      cache,
      ocean,
    }
  }

  /// the nearest surface the ray meets before `reach` (glass, water and glows are seen through)
  fn cast(&mut self, ray: &Ray, reach: f32, mut normal: Option<&mut Vec3>) -> f32 {
    self.ray_id += 1;
    let mut best = reach;
    // the ocean hides what is under it
    if ray.d.y < 0.0 && ray.o.y > self.ocean {
      best = best.min((self.ocean - ray.o.y) / ray.d.y);
    }
    // the cells under the ray, in order (a walk along its ground plan, half a cell at a time)
    let flat_len = (ray.d.x * ray.d.x + ray.d.z * ray.d.z).sqrt();
    let step = if flat_len > 1e-4 {
      K_CELL * 0.5 / flat_len
    } else {
      reach
    };
    let mut last = (i32::MIN, i32::MIN);
    let mut t = 0.0;
    while t <= best + step {
      let p = ray.at(t.min(best));
      let here = cell_of(p);
      for dx in -1..=1 {
        for dz in -1..=1 {
          let c = (here.0 + dx, here.1 + dz);
          if c == last && dx == 0 && dz == 0 {
            continue;
          }
          let Some(list) = self.cells.get(&c) else {
            continue;
          };
          for &k in list {
            if self.stamp[k as usize] == self.ray_id {
              continue;
            }
            self.stamp[k as usize] = self.ray_id;
            let item = &self.items[k as usize];
            if ray_aabb(ray, &item.bx.expanded(0.01), best).is_none() {
              continue;
            }
            let n = &self.scene.nodes[item.index as usize];
            let Some(mesh) = &n.mesh else { continue };
            let local = transform_ray(&item.inverse, ray);
            for (pi, prim) in mesh.prims.iter().enumerate() {
              if prim.idx.is_empty() {
                continue;
              }
              if let Some(m) = usize::try_from(prim.material)
                .ok()
                .and_then(|m| self.scene.materials.get(m))
                && m.alpha == AlphaMode::Blend
                && (m.blend != BlendMode::Alpha || m.translucent)
              {
                continue;
              }
              let Some((th, tri)) = mesh.bvh(pi).raycast(&local, &prim.pos, &prim.idx, best) else {
                continue;
              };
              best = th;
              if let Some(normal) = normal.as_deref_mut() {
                let w = &self.cache.worlds[item.index as usize];
                let i = tri as usize * 3;
                let a = w.point(prim.pos[prim.idx[i] as usize]);
                let b = w.point(prim.pos[prim.idx[i + 1] as usize]);
                let cc = w.point(prim.pos[prim.idx[i + 2] as usize]);
                *normal = normalize(cross(b - a, cc - a));
                if dot(*normal, ray.d) > 0.0 {
                  *normal = -*normal;
                }
              }
            }
          }
        }
      }
      last = here;
      if t >= best {
        break;
      }
      t += step;
    }
    best
  }
}

fn direction(yaw: f32, pitch: f32) -> Vec3 {
  Vec3::new(
    pitch.cos() * yaw.sin(),
    pitch.sin(),
    pitch.cos() * yaw.cos(),
  )
}

pub fn find_gaps(scene: &Scene, o: &GapOptions) -> GapReport {
  let mut report = GapReport::default();
  if !o.region.valid() {
    return report;
  }
  let r = &o.region;
  // what a ray may meet: the region and as far around as it reaches
  let area = r.expanded(o.reach);
  let mut caster = Caster::new(scene, &area, &o.ignore, o.ocean);
  let (sx, sz) = (r.hi.x - r.lo.x, r.hi.z - r.lo.z);
  let spacing = if o.spacing > 0.0 {
    o.spacing
  } else {
    2f32.max(((sx * sz).max(1.0) / 300.0).sqrt())
  };
  // radians down: from under the feet to the horizon (a slit of a floor is seen at one angle only)
  let pitches = [-1.2f32, -0.8, -0.55, -0.4, -0.3, -0.22, -0.15, -0.09, -0.04];
  let yaws = 32;
  let down = Vec3::new(0.0, -1.0, 0.0);
  let mut spots: BTreeMap<(i32, i32), GapSpot> = BTreeMap::new(); // by 2 m cell of where the hole is
  let mut x = r.lo.x + spacing * 0.5;
  while x < r.hi.x {
    let mut z = r.lo.z + spacing * 0.5;
    while z < r.hi.z {
      // the ground there: walkable (mostly facing up), inside the region
      let mut normal = Vec3::new(0.0, 1.0, 0.0);
      let below = Ray::new(Vec3::new(x, r.hi.y + 2.0, z), down);
      let depth = r.hi.y - r.lo.y + 4.0;
      let t = caster.cast(&below, depth, Some(&mut normal));
      z += spacing;
      if t >= depth || normal.y < 0.7 {
        continue;
      }
      let eye = below.at(t) + Vec3::new(0.0, o.eye, 0.0);
      report.viewpoints += 1;
      for k in 0..yaws {
        for &pitch in &pitches {
          let yaw = k as f32 / yaws as f32 * std::f32::consts::TAU;
          let ray = Ray::new(eye, direction(yaw, pitch));
          report.rays += 1;
          if caster.cast(&ray, o.reach, None) < o.reach {
            continue;
          }
          report.escaped += 1;
          // the rays on each side of it: a gap has something on both sides (left and right of a
          // slit between two walls, before and after a hole in a floor), an open edge on one side
          // at most (a little farther each time: a hole can be wider than the first step)
          let mut hit = [o.reach; 4];
          for a in [0.03f32, 0.08, 0.16] {
            let around = [
              (yaw - a, pitch),
              (yaw + a, pitch),
              (yaw, pitch - a),
              (yaw, pitch + a),
            ];
            for (h, (yw, tilt)) in hit.iter_mut().zip(around) {
              if *h >= o.reach {
                *h = caster.cast(&Ray::new(eye, direction(yw, tilt)), o.reach, None);
              }
            }
          }
          let across = hit[0] < o.reach && hit[1] < o.reach;
          let along = hit[2] < o.reach && hit[3] < o.reach;
          let near = (if across { hit[0].min(hit[1]) } else { o.reach }).min(if along {
            hit[2].min(hit[3])
          } else {
            o.reach
          });
          let mut s = GapSpot {
            seen_from: eye,
            direction: ray.d,
            edge: !across && !along,
            ..Default::default()
          };
          if !s.edge {
            // what lies behind the opening: ground (a passage between two pieces: the void is
            // farther, where that ground ends: an open edge there) or nothing (a hole)
            let mut ground_behind = false;
            let mut t = near + 1.0;
            while t < o.reach {
              if caster.cast(&Ray::new(ray.at(t), down), o.eye + 3.0, None) >= o.eye + 3.0 {
                break;
              }
              ground_behind = true;
              t += 2.0;
            }
            s.edge = ground_behind;
            s.at = ray.at(if ground_behind { t } else { near });
          } else {
            // an open edge: where the ray leaves the region
            let big = r.expanded(1.0);
            let mut tt = 0.0;
            while tt < o.reach && big.contains(ray.at(tt)) {
              tt += 1.0;
            }
            s.at = ray.at(tt);
          }
          let key = ((s.at.x / 2.0).floor() as i32, (s.at.z / 2.0).floor() as i32);
          spots.entry(key).or_insert(s).rays += 1;
        }
      }
    }
    x += spacing;
  }
  report.spots = spots.into_values().collect();
  // the gaps between pieces first: the edges may be wanted
  report
    .spots
    .sort_by(|a, b| a.edge.cmp(&b.edge).then(b.rays.cmp(&a.rays)));
  report
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::{Mesh, Node, Primitive};

  // a box of size 2, from y = 0 to 2
  fn make_box(name: &str) -> Arc<Mesh> {
    let mut mesh = Mesh::new();
    mesh.name = name.into();
    let mut p = Primitive::default();
    for i in 0..8 {
      let f = |bit: i32, a: f32, b: f32| if i & bit != 0 { b } else { a };
      p.pos
        .push(Vec3::new(f(1, -1.0, 1.0), f(2, 0.0, 2.0), f(4, -1.0, 1.0)));
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
    p.compute_normals();
    mesh.prims.push(p);
    Arc::new(mesh)
  }

  fn slab(sc: &mut Scene, name: &str, at: Vec3, size: Vec3) -> u32 {
    let mut n = Node {
      name: name.into(),
      kind: NodeKind::Render,
      mesh: Some(make_box(name)),
      ..Default::default()
    };
    n.local.t = at;
    n.local.s = size * 0.5;
    sc.add(n)
  }

  fn walls_around(sc: &mut Scene) {
    slab(
      sc,
      "wall-n",
      Vec3::new(0.0, -1.0, -21.0),
      Vec3::new(44.0, 12.0, 2.0),
    );
    slab(
      sc,
      "wall-s",
      Vec3::new(0.0, -1.0, 21.0),
      Vec3::new(44.0, 12.0, 2.0),
    );
    slab(
      sc,
      "wall-w",
      Vec3::new(-21.0, -1.0, 0.0),
      Vec3::new(2.0, 12.0, 44.0),
    );
    slab(
      sc,
      "wall-e",
      Vec3::new(21.0, -1.0, 0.0),
      Vec3::new(2.0, 12.0, 44.0),
    );
  }

  #[test]
  fn holes_between_slabs() {
    // building without visual bugs: holes in the decor
    // holes: two slabs of a floor 1 m apart, walls all around: the gap between them is found and
    // where it is; the floor whole: none
    let mut g = Scene::new();
    slab(
      &mut g,
      "left",
      Vec3::new(-10.25, -0.2, 0.0),
      Vec3::new(19.5, 0.2, 40.0),
    );
    slab(
      &mut g,
      "right",
      Vec3::new(10.25, -0.2, 0.0),
      Vec3::new(19.5, 0.2, 40.0),
    );
    walls_around(&mut g);
    let mut go = GapOptions::default();
    go.region.add(Vec3::new(-18.0, -1.0, -18.0));
    go.region.add(Vec3::new(18.0, 4.0, 18.0));
    go.spacing = 4.0;
    let holes = find_gaps(&g, &go);
    let gaps: Vec<&GapSpot> = holes.spots.iter().filter(|s| !s.edge).collect();
    let at_gap = gaps.iter().filter(|s| s.at.x.abs() < 1.5).count();
    let mut whole = Scene::new();
    slab(
      &mut whole,
      "floor",
      Vec3::new(0.0, -0.2, 0.0),
      Vec3::new(40.0, 0.2, 40.0),
    );
    walls_around(&mut whole);
    let none = find_gaps(&whole, &go);
    assert!(
      holes.viewpoints > 0 && !gaps.is_empty() && at_gap == gaps.len() && none.spots.is_empty(),
      "holes in the decor: the slit between two slabs found where it is, a whole floor walled around has none"
    );
    // what is ignored is not looked at: the right slab ignored, its side is the void
    go.ignore = vec![g.nodes[1].id];
    let open = find_gaps(&g, &go);
    assert!(open.escaped > holes.escaped);
  }
}
