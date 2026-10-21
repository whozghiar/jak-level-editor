//! Ray picking against the scene: meshes (BVH per primitive, local space) and actor markers. The
//! glows and light beams of the levels (additive blends) do not take the click from what is behind
//! them: they are hit only where nothing else is.

use super::{AlphaMode, BlendMode, K_MARKER_RADIUS, NodeKind, Scene};
use crate::math::{Ray, Vec3, cross, dot, length, normalize, ray_aabb, ray_sphere, transform_ray};

#[derive(Clone, Debug)]
pub struct PickFilter {
  pub render: bool,
  pub actors: bool,
  pub collision: bool,
  /// the nav meshes drawn over the level
  pub navmesh: bool,
  pub include_locked: bool,
  /// these nodes and their descendants
  pub exclude_list: Vec<u32>,
}

impl Default for PickFilter {
  fn default() -> Self {
    Self {
      render: true,
      actors: true,
      collision: false,
      navmesh: false,
      include_locked: false,
      exclude_list: Vec::new(),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickHit {
  pub node: u32,
  pub t: f32,
  pub point: Vec3,
  pub normal: Vec3,
  pub prim: i32,
  pub tri: u32,
}

impl Default for PickHit {
  fn default() -> Self {
    Self {
      node: 0,
      t: f32::MAX,
      point: Vec3::default(),
      normal: Vec3::new(0.0, 1.0, 0.0),
      prim: -1,
      tri: 0,
    }
  }
}

impl PickHit {
  pub fn hit(&self) -> bool {
    self.node != 0
  }
}

fn excluded(scene: &Scene, f: &PickFilter, id: u32) -> bool {
  f.exclude_list
    .iter()
    .any(|&e| id == e || scene.is_ancestor(e, id))
}

/// a glow or a light beam (the PS2's additive and subtractive blends): drawn faintly over what is
/// behind it, so it does not take the click from it
fn glow(scene: &Scene, material: i32) -> bool {
  material >= 0
    && (material as usize) < scene.materials.len()
    && scene.materials[material as usize].alpha == AlphaMode::Blend
    && scene.materials[material as usize].blend != BlendMode::Alpha
}

/// Every node the ray hits before `tmax`, its nearest point each, given to `hit` (which returns the
/// distance the next nodes must beat). Glows go to `glow_hit` instead.
fn cast(
  scene: &Scene,
  ray: &Ray,
  f: &PickFilter,
  mut hit: impl FnMut(&PickHit) -> f32,
  mut glow_hit: impl FnMut(&PickHit),
  mut tmax: f32,
) {
  let cache = scene.cache();
  for (i, n) in scene.nodes.iter().enumerate().take(cache.count) {
    if !cache.visible[i] {
      continue;
    }
    if cache.reference[i] {
      continue; // a level shown to compare with: not part of the document
    }
    if n.locked && !f.include_locked {
      continue;
    }
    let allowed = (n.kind == NodeKind::Render && f.render)
      || (n.kind == NodeKind::Collision && f.collision)
      || (n.kind == NodeKind::NavMesh && f.navmesh)
      || (n.kind == NodeKind::Actor && f.actors);
    if !allowed {
      continue;
    }
    let w = &cache.worlds[i];
    // cheap rejection first: most nodes of a level are far from the ray
    if n.mesh.is_some() && ray_aabb(ray, &cache.bounds[i].expanded(0.01), tmax).is_none() {
      continue;
    }
    if excluded(scene, f, n.id) {
      continue;
    }
    let mut h = PickHit::default();
    if n.kind == NodeKind::Actor {
      let t = ray_sphere(ray, w.col3(3), K_MARKER_RADIUS * 1.3);
      if t > 0.0 && t < tmax {
        h.node = n.id;
        h.t = t;
        h.point = ray.at(t);
        h.normal = Vec3::new(0.0, 1.0, 0.0);
        h.prim = -1;
        tmax = hit(&h);
      }
      continue;
    }
    let Some(mesh) = &n.mesh else { continue };
    let local = transform_ray(&w.inverse(), ray);
    let mut soft = PickHit::default();
    for (p, prim) in mesh.prims.iter().enumerate() {
      // the see-through surfaces of the terrain (water, glass) do not hide what is behind them
      if n.locked
        && prim.material >= 0
        && (prim.material as usize) < scene.materials.len()
        && scene.materials[prim.material as usize].alpha == AlphaMode::Blend
      {
        continue;
      }
      let best = if glow(scene, prim.material) {
        &mut soft
      } else {
        &mut h
      };
      if let Some((t, tri)) = mesh
        .bvh(p)
        .raycast(&local, &prim.pos, &prim.idx, tmax.min(best.t))
      {
        let k = tri as usize * 3;
        let a = w.point(prim.pos[prim.idx[k] as usize]);
        let b = w.point(prim.pos[prim.idx[k + 1] as usize]);
        let c = w.point(prim.pos[prim.idx[k + 2] as usize]);
        let mut nrm = normalize(cross(b - a, c - a));
        if dot(nrm, ray.d) > 0.0 {
          nrm = -nrm;
        }
        best.node = n.id;
        best.t = t;
        best.point = ray.at(t);
        best.normal = nrm;
        best.prim = p as i32;
        best.tri = tri;
      }
    }
    if h.hit() {
      tmax = hit(&h);
    } else if soft.hit() {
      glow_hit(&soft);
    }
  }
}

pub fn pick(scene: &Scene, ray: &Ray, f: &PickFilter) -> PickHit {
  let mut best = PickHit::default();
  let mut glow_best = PickHit::default();
  cast(
    scene,
    ray,
    f,
    |h| {
      if h.t < best.t {
        best = *h;
      }
      best.t
    },
    |h| {
      if h.t < glow_best.t {
        glow_best = *h;
      }
    },
    f32::MAX,
  );
  // a glow is chosen when nothing else is under it
  if best.hit() { best } else { glow_best }
}

/// Every node along the ray, the nearest first (one hit each), the glows last: what a click at the
/// same place goes through, one after the other.
pub fn pick_all(scene: &Scene, ray: &Ray, f: &PickFilter, max: usize) -> Vec<PickHit> {
  let mut hits = Vec::new();
  let mut glows = Vec::new();
  cast(
    scene,
    ray,
    f,
    |h| {
      hits.push(*h);
      f32::MAX
    },
    |h| glows.push(*h),
    f32::MAX,
  );
  let nearest = |a: &PickHit, b: &PickHit| a.t.total_cmp(&b.t);
  hits.sort_by(nearest);
  glows.sort_by(nearest);
  // the glows after what they are in front of: they are reached by clicking again
  hits.extend(glows);
  hits.truncate(max);
  hits
}

/// `pick_all`'s default: 32 hits at most.
pub const K_PICK_ALL_MAX: usize = 32;

/// How much of the flat surfaces of the piece at `index` (0..1, by area) has another piece more than
/// `gap` meters under it (the backdrops left out): a ground has nothing under it, a roof, a ceiling,
/// a bridge or a slab lying on the ground has. Down rays from points spread over them, whichever way
/// their triangles face. (C++ default `gap`: 1 m.)
pub fn covered_below(scene: &Scene, index: usize, gap: f32) -> f32 {
  let Some(n) = scene.nodes.get(index) else {
    return 0.0;
  };
  let Some(mesh) = &n.mesh else { return 0.0 };
  let Some(&w) = scene.cache().worlds.get(index) else {
    return 0.0;
  };
  // up to 64 points, one every 64th of the flat area: the middle of the triangle reached
  struct Flat {
    mid: Vec3,
    area: f32,
  }
  let mut flats = Vec::new();
  let mut total = 0f64;
  for p in &mesh.prims {
    for t in p.idx.as_chunks::<3>().0 {
      let (a, b, c) = (
        w.point(p.pos[t[0] as usize]),
        w.point(p.pos[t[1] as usize]),
        w.point(p.pos[t[2] as usize]),
      );
      let nr = cross(b - a, c - a);
      let l = length(nr);
      if l < 1e-8 || nr.y.abs() < 0.7 * l {
        continue;
      }
      flats.push(Flat {
        mid: (a + b + c) * (1.0 / 3.0),
        area: l * 0.5,
      });
      total += (l * 0.5) as f64;
    }
  }
  if flats.is_empty() {
    return 0.0;
  }
  let f = PickFilter {
    actors: false,
    include_locked: true,
    exclude_list: vec![n.id],
    ..Default::default()
  };
  let step = total / 64.0;
  let (mut acc, mut next) = (0f64, step * 0.5);
  let (mut samples, mut covered) = (0, 0);
  for fl in &flats {
    acc += fl.area as f64;
    if acc < next {
      continue;
    }
    while next <= acc {
      next += step;
    }
    samples += 1;
    let h = pick(
      scene,
      &Ray::new(
        fl.mid - Vec3::new(0.0, 0.05, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
      ),
      &f,
    );
    if !h.hit() || h.t <= gap || h.t > 80.0 {
      continue;
    }
    if let Some(under) = scene.find(h.node)
      && let Some(m) = &under.mesh
      && m.extras.get("og_backdrop").is_some()
    {
      continue;
    }
    covered += 1;
  }
  if samples > 0 {
    covered as f32 / samples as f32
  } else {
    0.0
  }
}
