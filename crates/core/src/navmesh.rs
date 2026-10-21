//! Navigation meshes: where the creatures of a level walk (the enemies and citizens of Jak 3 follow
//! them). Generated from the level's walkable surfaces with Recast (rerecast, its Rust port, and
//! Recast's own steps where rerecast differs: recast.rs): the surfaces are cut in voxels, kept
//! where they are not too steep, where a creature has room above them and can step from one to the
//! next, shrunk by the creature's radius, then outlined and cut in convex polygons of at most 4
//! corners: the same polygons as the C++ editor's. Then the limits of the game apply (the game's
//! nav-mesh, see goal_src/jak3/engine/nav/nav-mesh-h.gc): at most 255 polygons per mesh, each mesh
//! in one piece (its route table must reach every polygon), polygons facing up.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use rerecast::{AreaType, BuildContoursFlags, HeightfieldBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::history::{HistoryEntry, NodeChange};
use crate::i18n::tr;
use crate::math::{AABB, K_PI, Vec3, cross, length};
use crate::scene::{AlphaMode, BlendMode, Material, Mesh, Node, NodeKind, Primitive, Scene};
use crate::strf;

mod recast;

/// the game's poly-count is a byte, 255 marks "no polygon"
const K_MAX_POLYS: usize = 255;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavParams {
  /// m: the voxels' width
  pub cell_size: f32,
  /// m: their height
  pub cell_height: f32,
  /// m: the room a creature needs above the floor
  pub agent_height: f32,
  /// m: the walls keep this far
  pub agent_radius: f32,
  /// m: the highest step
  pub max_climb: f32,
  /// degrees
  pub max_slope: f32,
  /// m²: walkable islands smaller than this are left out
  pub min_region: f32,
  /// m²: smaller regions join their neighbors
  pub merge_region: f32,
  /// m: the longest polygon edge along walls
  pub max_edge: f32,
  /// m: how far an outline may stray from the walls
  pub max_error: f32,
  /// when not empty, only the pieces of walkable ground next to one of these points are kept (the
  /// streets where the creatures go; not the roofs, ledges and closed rooms)
  pub keep_from: Vec<Vec3>,
}

impl Default for NavParams {
  fn default() -> Self {
    Self {
      cell_size: 0.5,
      cell_height: 0.2,
      agent_height: 2.0,
      agent_radius: 0.6,
      max_climb: 0.8,
      max_slope: 45.0,
      min_region: 16.0,
      merge_region: 60.0,
      max_edge: 60.0,
      max_error: 0.8,
      keep_from: Vec::new(),
    }
  }
}

/// A polygon: corners (indices in its mesh's vertices), and for each edge (corner i to i+1) the
/// polygon across it in the same mesh, -1 at a border. `pat`: the game's flags of the polygon (0 for
/// the editor's; with bit 0, the game keeps it out of the mesh's grid).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavPoly {
  pub count: usize,
  pub v: [usize; 4],
  pub adj: [i32; 4],
  pub pat: u8,
}

impl Default for NavPoly {
  fn default() -> Self {
    Self {
      count: 0,
      v: [0; 4],
      adj: [-1; 4],
      pat: 0,
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavMeshData {
  /// world position the vertices are relative to
  pub origin: Vec3,
  /// meters, relative to origin
  pub verts: Vec<Vec3>,
  pub polys: Vec<NavPoly>,
}

impl NavMeshData {
  /// m², the polygons' surface
  pub fn area(&self) -> f32 {
    let mut a = 0.0;
    for p in &self.polys {
      for i in 1..p.count.saturating_sub(1) {
        let v0 = self.verts[p.v[0]];
        a += 0.5 * length(cross(self.verts[p.v[i]] - v0, self.verts[p.v[i + 1]] - v0));
      }
    }
    a
  }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NavBuildStats {
  /// triangles of the level read
  pub triangles: usize,
  pub grid_width: i32,
  pub grid_depth: i32,
  /// polygons Recast made
  pub recast_polys: usize,
  pub meshes: usize,
  pub polys: usize,
  /// pieces over 255 polygons cut in several meshes
  pub split: usize,
  /// pieces of ground away from the points of keep_from
  pub left_out: usize,
  pub ms: f64,
}

// ------------------------------------------------------------------------------------------------
// polygons
// ------------------------------------------------------------------------------------------------

/// Twice the signed area of a polygon seen from above (x, z). The game wants its polygons the way
/// round where (v1 - v0) x (v2 - v0) points up, which is a negative value here.
fn signed_area2(verts: &[Vec3], p: &NavPoly) -> f32 {
  let mut s = 0.0;
  for i in 0..p.count {
    let a = verts[p.v[i]];
    let b = verts[p.v[(i + 1) % p.count]];
    s += a.x * b.z - b.x * a.z;
  }
  s
}

/// The game's own test (nav-mesh initialize-mesh!): the normal of the first three corners.
fn first_normal_y(verts: &[Vec3], p: &NavPoly) -> f32 {
  let a = verts[p.v[1]] - verts[p.v[0]];
  let c = verts[p.v[2]] - verts[p.v[0]];
  a.z * c.x - a.x * c.z
}

fn reverse_poly(p: &mut NavPoly) {
  let n = p.count;
  let mut r = *p;
  for k in 0..n {
    r.v[k] = p.v[n - 1 - k];
    // edge k of the reversed polygon is edge n-2-k of the original, the other way
    r.adj[k] = p.adj[(n + n - 2 - k) % n];
  }
  *p = r;
}

fn rotate_poly(p: &mut NavPoly, s: usize) {
  let mut r = *p;
  for k in 0..p.count {
    r.v[k] = p.v[(k + s) % p.count];
    r.adj[k] = p.adj[(k + s) % p.count];
  }
  *p = r;
}

fn centroid(verts: &[Vec3], p: &NavPoly) -> Vec3 {
  let mut c = Vec3::new(0.0, 0.0, 0.0);
  for i in 0..p.count {
    c += verts[p.v[i]];
  }
  c / p.count as f32
}

/// How far `p` is from the polygon, across (x, z): 0 inside.
fn distance_xz(verts: &[Vec3], q: &NavPoly, p: Vec3) -> f32 {
  let (mut inside_cw, mut inside_ccw) = (true, true);
  let mut best = 1e30f32;
  for k in 0..q.count {
    let a = verts[q.v[k]];
    let b = verts[q.v[(k + 1) % q.count]];
    let (ex, ez, px, pz) = (b.x - a.x, b.z - a.z, p.x - a.x, p.z - a.z);
    let cross = ex * pz - ez * px;
    inside_cw = inside_cw && cross <= 0.0;
    inside_ccw = inside_ccw && cross >= 0.0;
    let len2 = ex * ex + ez * ez;
    let t = if len2 > 0.0 {
      ((px * ex + pz * ez) / len2).clamp(0.0, 1.0)
    } else {
      0.0
    };
    best = best.min((px - t * ex).hypot(pz - t * ez));
  }
  if inside_cw || inside_ccw { 0.0 } else { best }
}

/// The connected groups of `ids` (polygons of `polys`), through the adjacencies inside `ids`.
fn components(polys: &[NavPoly], ids: &[usize]) -> Vec<Vec<usize>> {
  // -2: not in ids, -1: not reached yet, else its group
  let mut group = vec![-2i32; polys.len()];
  for &id in ids {
    group[id] = -1;
  }
  let mut out: Vec<Vec<usize>> = Vec::new();
  for &start in ids {
    if group[start] != -1 {
      continue;
    }
    let g = out.len();
    out.push(Vec::new());
    let mut stack = vec![start];
    group[start] = g as i32;
    while let Some(i) = stack.pop() {
      out[g].push(i);
      for k in 0..polys[i].count {
        let j = polys[i].adj[k];
        if j >= 0 && group[j as usize] == -1 {
          group[j as usize] = g as i32;
          stack.push(j as usize);
        }
      }
    }
  }
  out
}

// ------------------------------------------------------------------------------------------------
// generation
// ------------------------------------------------------------------------------------------------

/// A surface nothing walks on, left out of the nav meshes: a glow or a light shaft (added to or
/// subtracted from what is behind), or water and dark eco as the editor draws them (translucent).
/// The levels' own materials are blended with their texture's alpha almost everywhere: that alone
/// does not make them see-through.
pub fn navmesh_ignores(m: &Material) -> bool {
  m.blend != BlendMode::Alpha
    || (m.alpha == AlphaMode::Blend && (m.translucent || m.base_color.w < 0.95))
}

/// The nav meshes of a triangle soup (world space, meters, Y up) inside `region`: one per piece of
/// walkable ground, several for a piece over 255 polygons.
pub fn build_navmeshes(
  verts: &[Vec3],
  tris: &[u32],
  region: &AABB,
  params: &NavParams,
) -> Result<(Vec<NavMeshData>, NavBuildStats), String> {
  let t0 = Instant::now();
  let mut st = NavBuildStats::default();
  if !region.valid() {
    return Err("empty region".into());
  }
  // the triangles that reach the region (with room for the creature around it), walkable when not
  // steeper than the slope, whichever way the triangle is wound (the game's meshes do not always
  // face up)
  let reach = region.expanded(params.agent_radius + params.cell_size);
  let min_up = (params.max_slope * K_PI / 180.0).cos();
  let mut fv: Vec<[f32; 3]> = Vec::new();
  let mut ft: Vec<[u32; 3]> = Vec::new();
  let mut areas = Vec::new();
  let mut remap = vec![u32::MAX; verts.len()];
  let mut used = AABB::default();
  for t in tris.chunks_exact(3) {
    let mut tb = AABB::default();
    for &k in t {
      tb.add(verts[k as usize]);
    }
    if tb.hi.x < reach.lo.x
      || tb.lo.x > reach.hi.x
      || tb.hi.z < reach.lo.z
      || tb.lo.z > reach.hi.z
      || tb.hi.y < reach.lo.y
      || tb.lo.y > reach.hi.y
    {
      continue;
    }
    let mut tri = [0u32; 3];
    for (k, &v) in t.iter().enumerate() {
      if remap[v as usize] == u32::MAX {
        remap[v as usize] = fv.len() as u32;
        fv.push(verts[v as usize].into());
      }
      tri[k] = remap[v as usize];
    }
    let (a, b, c) = (
      verts[t[0] as usize],
      verts[t[1] as usize],
      verts[t[2] as usize],
    );
    let n = cross(b - a, c - a);
    let len = length(n);
    ft.push(tri);
    areas.push(if len > 0.0 && n.y.abs() / len >= min_up {
      AreaType::DEFAULT_WALKABLE
    } else {
      AreaType::NOT_WALKABLE
    });
    used.add_box(&tb);
  }
  st.triangles = ft.len();
  if ft.is_empty() {
    return Err("no surface in the region".into());
  }
  // the grid: the region where there is something, a little wider than the creature needs
  let bmin = glam::Vec3::new(
    reach.lo.x.max(used.lo.x),
    reach.lo.y.max(used.lo.y) - 1.0,
    reach.lo.z.max(used.lo.z),
  );
  let bmax = glam::Vec3::new(
    reach.hi.x.min(used.hi.x),
    reach.hi.y.min(used.hi.y) + params.agent_height,
    reach.hi.z.min(used.hi.z),
  );
  let mut cs = params.cell_size.max(0.05);
  let ch = params.cell_height.max(0.05);
  // rcCalcGridSize
  let grid = |cs: f32| {
    (
      ((bmax.x - bmin.x) / cs + 0.5) as i32,
      ((bmax.z - bmin.z) / cs + 0.5) as i32,
    )
  };
  let (mut w, mut d) = grid(cs);
  // a very large region: coarser voxels (Recast's spans would not fit in memory; rerecast counts
  // the cells of a side in 16 bits)
  while w as f64 * d as f64 > 16e6 || w > u16::MAX as i32 || d > u16::MAX as i32 {
    cs *= 1.25;
    (w, d) = grid(cs);
  }
  st.grid_width = w;
  st.grid_depth = d;

  // rcConfig (rerecast counts in u16 what Recast counts in int: a float cast saturates, an area of
  // more than 65535 cells, a region of 1000 m² of voxels of 0.1 m, is 65535)
  let walkable_height = (params.agent_height / ch).ceil() as u16;
  let walkable_climb = (params.max_climb / ch).floor() as u16;
  let walkable_radius = (params.agent_radius / cs).ceil() as u16;
  // ponytail: at most 255 cells (127 m with the voxels of 0.5 m), rerecast's contours square it
  // in 16 bits; longer edges need its build_contours fixed or ported
  let max_edge_len = ((params.max_edge / cs) as u16).min(255);
  let min_region_area = (params.min_region / (cs * cs)).ceil() as u16;
  let merge_region_area = (params.merge_region / (cs * cs)).ceil() as u16;
  let max_verts_per_poly = 4usize;

  let mut hf = HeightfieldBuilder {
    aabb: rerecast::Aabb3d {
      min: bmin,
      max: bmax,
    },
    cell_size: cs,
    cell_height: ch,
  }
  .build()
  .map_err(|_| "not enough memory for the voxels".to_string())?;
  recast::rasterize_triangles(&mut hf, &fv, &ft, &areas, walkable_climb as i32);
  drop((fv, ft, areas));
  // steps, ledges, and floors without head room are not walkable
  hf.filter_low_hanging_walkable_obstacles(walkable_climb);
  hf.filter_ledge_spans(walkable_height, walkable_climb);
  hf.filter_walkable_low_height_spans(walkable_height);
  let regions_failed = "the walkable regions could not be built";
  let mut chf = hf
    .into_compact(walkable_height, walkable_climb)
    .map_err(|_| regions_failed.to_string())?;
  chf.erode_walkable_area(walkable_radius);
  chf.build_distance_field();
  chf
    .build_regions(0, min_region_area, merge_region_area)
    .map_err(|_| regions_failed.to_string())?;
  let mut cset = chf.build_contours(
    params.max_error,
    max_edge_len,
    BuildContoursFlags::TESSELLATE_SOLID_WALL_EDGES,
  );
  drop(chf);
  recast::merge_holes(&mut cset);
  let nvp = max_verts_per_poly;
  let (pm_verts, pm_polys) = recast::build_poly_mesh(&cset, nvp)
    .map_err(|_| "the polygons could not be built".to_string())?;
  let npolys = pm_polys.len() / (2 * nvp);
  st.recast_polys = npolys;

  // Recast's polygons, in meters
  let all_verts: Vec<Vec3> = pm_verts
    .iter()
    .map(|v| {
      Vec3::new(
        cset.aabb.min.x + v[0] as f32 * cset.cell_size,
        cset.aabb.min.y + v[1] as f32 * cset.cell_height,
        cset.aabb.min.z + v[2] as f32 * cset.cell_size,
      )
    })
    .collect();
  drop(cset);
  let mut all = vec![NavPoly::default(); npolys];
  let mut dead = vec![false; npolys];
  for i in 0..npolys {
    let p = &pm_polys[i * 2 * nvp..(i * 2 + 1) * nvp];
    let nb = &pm_polys[(i * 2 + 1) * nvp..(i + 1) * 2 * nvp];
    let q = &mut all[i];
    for j in 0..nvp {
      if p[j] == recast::NULL_IDX {
        break;
      }
      q.v[q.count] = p[j] as usize;
      q.adj[q.count] = if nb[j] == recast::NULL_IDX || nb[j] & 0x8000 != 0 {
        -1
      } else {
        nb[j] as i32
      };
      q.count += 1;
    }
    if q.count < 3 || signed_area2(&all_verts, q).abs() < 1e-4 {
      dead[i] = true;
    }
  }
  for i in 0..npolys {
    if dead[i] {
      continue;
    }
    let mut q = all[i];
    for k in 0..q.count {
      if q.adj[k] >= 0 && dead[q.adj[k] as usize] {
        q.adj[k] = -1;
      }
    }
    // facing up as the game counts it, and not starting on three corners in a line
    if signed_area2(&all_verts, &q) > 0.0 {
      reverse_poly(&mut q);
    }
    let mut s = 0;
    while s < q.count && first_normal_y(&all_verts, &q).abs() < 1e-6 {
      rotate_poly(&mut q, 1);
      s += 1;
    }
    all[i] = q;
  }

  // the pieces: connected groups, cut in halves (along their longest side) until each has at most
  // 255 polygons
  let alive: Vec<usize> = (0..npolys).filter(|&i| !dead[i]).collect();
  let mut todo = components(&all, &alive);
  let mut pieces: Vec<Vec<usize>> = Vec::new();
  if !params.keep_from.is_empty() {
    // the pieces next to a point: the polygon nearest to it across, at its height (a point on an
    // element standing on the ground is over a hole of the nav mesh, the ground around is near)
    const K_REACH: f32 = 12.0;
    const K_HEIGHT: f32 = 3.0;
    let mut piece_of = vec![usize::MAX; all.len()];
    for (g, piece) in todo.iter().enumerate() {
      for &id in piece {
        piece_of[id] = g;
      }
    }
    let mut reached = vec![false; todo.len()];
    for p in &params.keep_from {
      let mut nearest = None;
      let mut nearest_d = K_REACH;
      for &id in &alive {
        let q = &all[id];
        let (mut lo, mut hi) = (1e30f32, -1e30f32);
        for k in 0..q.count {
          lo = lo.min(all_verts[q.v[k]].y);
          hi = hi.max(all_verts[q.v[k]].y);
        }
        if p.y < lo - K_HEIGHT || p.y > hi + K_HEIGHT {
          continue;
        }
        let d = distance_xz(&all_verts, q, *p);
        if d < nearest_d {
          nearest = Some(id);
          nearest_d = d;
        }
      }
      if let Some(id) = nearest {
        reached[piece_of[id]] = true;
      }
    }
    let before = todo.len();
    todo = todo
      .into_iter()
      .zip(&reached)
      .filter_map(|(g, &r)| r.then_some(g))
      .collect();
    st.left_out = before - todo.len();
    if todo.is_empty() {
      return Err("no walkable ground next to the points given".into());
    }
  }
  let centers: Vec<Vec3> = all.iter().map(|q| centroid(&all_verts, q)).collect();
  while let Some(mut ids) = todo.pop() {
    if ids.len() <= K_MAX_POLYS {
      pieces.push(ids);
      continue;
    }
    st.split += 1;
    let mut cb = AABB::default();
    for &id in &ids {
      cb.add(centers[id]);
    }
    let along_x = cb.size().x >= cb.size().z;
    // ponytail: a stable sort, where C++'s std::sort orders polygons of the same center its own way
    ids.sort_by(|&a, &b| {
      let (ca, cb) = (centers[a], centers[b]);
      if along_x {
        ca.x.total_cmp(&cb.x)
      } else {
        ca.z.total_cmp(&cb.z)
      }
    });
    let (lo, hi) = ids.split_at(ids.len() / 2);
    for side in [lo, hi] {
      todo.extend(components(&all, side));
    }
  }
  // the cuts leave slivers behind: a small piece joins the smallest neighbor it touches when both
  // fit in one mesh (touching, they stay in one piece)
  if st.split > 0 {
    const K_SLIVER: usize = 64;
    let mut piece_of = vec![usize::MAX; all.len()];
    for (g, piece) in pieces.iter().enumerate() {
      for &id in piece {
        piece_of[id] = g;
      }
    }
    let mut merged = true;
    while merged {
      merged = false;
      for g in 0..pieces.len() {
        if pieces[g].is_empty() || pieces[g].len() > K_SLIVER {
          continue;
        }
        let mut best: Option<usize> = None;
        for &id in &pieces[g] {
          for k in 0..all[id].count {
            let nb = all[id].adj[k];
            if nb < 0 {
              continue;
            }
            let h = piece_of[nb as usize];
            if h == usize::MAX || h == g || pieces[h].len() + pieces[g].len() > K_MAX_POLYS {
              continue;
            }
            if best.is_none_or(|b| pieces[h].len() < pieces[b].len()) {
              best = Some(h);
            }
          }
        }
        let Some(best) = best else { continue };
        let moved = std::mem::take(&mut pieces[g]);
        for &id in &moved {
          piece_of[id] = best;
        }
        pieces[best].extend(moved);
        merged = true;
      }
    }
    pieces.retain(|p| !p.is_empty());
  }
  // largest first, like the game's levels list their meshes
  pieces.sort_by_key(|p| Reverse(p.len()));

  let mut out = Vec::with_capacity(pieces.len());
  let mut local_poly = vec![-1i32; all.len()];
  let mut local_vert = vec![usize::MAX; all_verts.len()];
  for ids in &pieces {
    let mut m = NavMeshData::default();
    for &id in ids {
      local_poly[id] = m.polys.len() as i32;
      m.polys.push(all[id]);
    }
    let mut vb = AABB::default();
    for p in &mut m.polys {
      for k in 0..p.count {
        let lv = &mut local_vert[p.v[k]];
        if *lv == usize::MAX {
          *lv = m.verts.len();
          m.verts.push(all_verts[p.v[k]]);
          vb.add(all_verts[p.v[k]]);
        }
        p.v[k] = *lv;
        p.adj[k] = if p.adj[k] >= 0 {
          local_poly[p.adj[k] as usize]
        } else {
          -1
        };
      }
    }
    // the next piece's numbering starts again (only this piece's entries were set)
    for &id in ids {
      for k in 0..all[id].count {
        local_vert[all[id].v[k]] = usize::MAX;
      }
      local_poly[id] = -1;
    }
    // the origin is the center of its corners, as in the game's meshes
    m.origin = vb.center();
    for v in &mut m.verts {
      *v -= m.origin;
    }
    st.polys += m.polys.len();
    out.push(m);
  }
  st.meshes = out.len();
  st.ms = t0.elapsed().as_secs_f64() * 1000.0;
  if out.is_empty() {
    return Err(
      "no walkable ground (the creature does not fit anywhere: try a smaller radius or height)"
        .into(),
    );
  }
  Ok((out, st))
}

// ------------------------------------------------------------------------------------------------
// the game's limits
// ------------------------------------------------------------------------------------------------

/// The route table of a mesh as the game reads it (nav-mesh route): 2 bits per (from, to) pair, bit
/// index (to + from * count) * 2, the edge of `from` to cross towards `to` on the shortest path.
pub fn navmesh_route_table(mesh: &NavMeshData) -> Vec<u8> {
  let n = mesh.polys.len();
  let mut table = vec![0u8; (n * n * 2).div_ceil(8)];
  let center: Vec<Vec3> = mesh
    .polys
    .iter()
    .map(|p| centroid(&mesh.verts, p))
    .collect();
  // the length of a step from a polygon to its neighbor: through the middle of their edge
  let step = |i: usize, k: usize| {
    let p = &mesh.polys[i];
    let mid = (mesh.verts[p.v[k]] + mesh.verts[p.v[(k + 1) % p.count]]) * 0.5;
    length(mid - center[i]) + length(center[p.adj[k] as usize] - mid)
  };
  let mut dist = vec![f32::INFINITY; n];
  for t in 0..n {
    // shortest distances to t (the graph is the same both ways); the distances are not negative:
    // their bits sort as they do
    dist.fill(f32::INFINITY);
    let mut open = BinaryHeap::new();
    dist[t] = 0.0;
    open.push(Reverse((0f32.to_bits(), t)));
    while let Some(Reverse((dd, i))) = open.pop() {
      let dd = f32::from_bits(dd);
      if dd > dist[i] {
        continue;
      }
      let p = &mesh.polys[i];
      for k in 0..p.count {
        let j = p.adj[k];
        if j < 0 {
          continue;
        }
        let nd = dd + step(i, k);
        if nd < dist[j as usize] {
          dist[j as usize] = nd;
          open.push(Reverse((nd.to_bits(), j as usize)));
        }
      }
    }
    for s in 0..n {
      if s == t {
        continue;
      }
      let p = &mesh.polys[s];
      let mut best = 0u8;
      let mut best_d = f32::INFINITY;
      for k in 0..p.count {
        if p.adj[k] < 0 {
          continue;
        }
        let dk = step(s, k) + dist[p.adj[k] as usize];
        if dk < best_d {
          best_d = dk;
          best = k as u8;
        }
      }
      let bit = (t + s * n) * 2;
      table[bit >> 3] |= best << (bit & 7);
    }
  }
  table
}

/// Checks a mesh against what the game needs; None when it is fine.
pub fn navmesh_problem(mesh: &NavMeshData) -> Option<&'static str> {
  let n = mesh.polys.len();
  if n == 0 {
    return Some("no polygon");
  }
  if n > K_MAX_POLYS {
    return Some("more than 255 polygons");
  }
  for (i, p) in mesh.polys.iter().enumerate() {
    if p.count < 3 || p.count > 4 {
      return Some("a polygon without 3 or 4 corners");
    }
    if first_normal_y(&mesh.verts, p) <= 0.0 {
      return Some("a polygon facing down or without area");
    }
    for k in 0..p.count {
      // convex: every turn the same way
      let a = mesh.verts[p.v[k]];
      let b = mesh.verts[p.v[(k + 1) % p.count]];
      let c = mesh.verts[p.v[(k + 2) % p.count]];
      let turn = (b.x - a.x) * (c.z - b.z) - (b.z - a.z) * (c.x - b.x);
      let scale = (b.x - a.x).hypot(b.z - a.z) * (c.x - b.x).hypot(c.z - b.z);
      if turn > 1e-4 * scale + 1e-6 {
        return Some("a polygon that is not convex");
      }
      let j = p.adj[k];
      if j < 0 {
        continue;
      }
      if j as usize >= n {
        return Some("an adjacency outside the mesh");
      }
      let q = &mesh.polys[j as usize];
      let back = (0..q.count).any(|e| {
        q.adj[e] == i as i32 && q.v[e] == p.v[(k + 1) % p.count] && q.v[(e + 1) % q.count] == p.v[k]
      });
      if !back {
        return Some("two polygons that do not share their edge both ways");
      }
    }
  }
  let ids: Vec<usize> = (0..n).collect();
  if components(&mesh.polys, &ids).len() != 1 {
    return Some("polygons that cannot reach each other");
  }
  None
}

// ------------------------------------------------------------------------------------------------
// in the document
// ------------------------------------------------------------------------------------------------

/// A nav mesh as a document node (NodeKind::NavMesh): its polygons in extras.og_navmesh, a fan of
/// triangles as its mesh (bounds, picking), its origin as its position.
pub fn navmesh_node(mesh: &NavMeshData, name: &str) -> Node {
  let mut m = Mesh::new();
  m.name = name.to_string();
  let mut prim = Primitive {
    pos: mesh.verts.clone(),
    nrm: vec![Vec3::new(0.0, 1.0, 0.0); mesh.verts.len()],
    ..Default::default()
  };
  let verts: Vec<Value> = mesh.verts.iter().map(|v| json!([v.x, v.y, v.z])).collect();
  let mut polys = Vec::with_capacity(mesh.polys.len());
  let mut adj = Vec::with_capacity(mesh.polys.len());
  for p in &mesh.polys {
    polys.push(json!(p.v[..p.count]));
    adj.push(json!(p.adj[..p.count]));
    for k in 1..p.count.saturating_sub(1) {
      prim
        .idx
        .extend([p.v[0] as u32, p.v[k] as u32, p.v[k + 1] as u32]);
    }
  }
  m.prims.push(prim);
  let mut n = Node {
    name: name.to_string(),
    kind: NodeKind::NavMesh,
    mesh: Some(Arc::new(m)),
    extras: json!({ "og_navmesh": { "verts": verts, "polys": polys, "adj": adj } }),
    ..Default::default()
  };
  n.local.t = mesh.origin;
  n
}

/// The polygons of such a node, relative to its origin (its transform is not applied).
pub fn navmesh_of_node(n: &Node) -> Option<NavMeshData> {
  if n.kind != NodeKind::NavMesh {
    return None;
  }
  let j = n.extras.get("og_navmesh")?.as_object()?;
  let mut m = NavMeshData::default();
  for v in j.get("verts")?.as_array()? {
    let c = |i: usize| v.get(i).and_then(Value::as_f64).map(|f| f as f32);
    m.verts.push(Vec3::new(c(0)?, c(1)?, c(2)?));
  }
  let polys = j.get("polys")?.as_array()?;
  let adj = j.get("adj")?.as_array()?;
  if polys.len() != adj.len() {
    return None;
  }
  for (pv, pa) in polys.iter().zip(adj) {
    let (pv, pa) = (pv.as_array()?, pa.as_array()?);
    let mut p = NavPoly {
      count: pv.len().min(4),
      ..Default::default()
    };
    for k in 0..p.count {
      let v = pv[k].as_i64()?;
      if v < 0 || v as usize >= m.verts.len() {
        return None;
      }
      p.v[k] = v as usize;
      p.adj[k] = match pa.get(k) {
        Some(a) => a.as_i64()? as i32,
        None => -1,
      };
    }
    m.polys.push(p);
  }
  Some(m)
}

/// The document's nav meshes.
pub fn navmesh_nodes(scene: &Scene) -> Vec<u32> {
  scene
    .nodes
    .iter()
    .filter(|n| n.kind == NodeKind::NavMesh)
    .map(|n| n.id)
    .collect()
}

/// The walkable surfaces of a generation in `region` (invalid: the whole level), App::
/// start_navmesh_generation's: the decor shown, in world space, without the see-through surfaces
/// (water, glows: navmesh_ignores) nor the objects (their models move, open, break). Returns their
/// vertices, triangles, and the region to generate in (the one given, else the surfaces' box); no
/// triangle: the document has nothing to walk on ("navmesh.nothing").
pub fn navmesh_surfaces(
  scene: &Scene,
  region: &AABB,
  params: &NavParams,
) -> (Vec<Vec3>, Vec<u32>, AABB) {
  let cache = scene.cache();
  let reach = if region.valid() {
    region.expanded(params.agent_radius + 2.0)
  } else {
    AABB::default()
  };
  let mut verts = Vec::new();
  let mut tris = Vec::new();
  let mut all = AABB::default();
  for (i, n) in scene.nodes.iter().enumerate() {
    let Some(mesh) = &n.mesh else { continue };
    if n.kind != NodeKind::Render || !cache.visible[i] || cache.actor[i] != 0 || cache.reference[i]
    {
      continue;
    }
    let nb = &cache.bounds[i];
    if reach.valid()
      && (nb.hi.x < reach.lo.x
        || nb.lo.x > reach.hi.x
        || nb.hi.z < reach.lo.z
        || nb.lo.z > reach.hi.z
        || nb.hi.y < reach.lo.y
        || nb.lo.y > reach.hi.y)
    {
      continue;
    }
    let w = &cache.worlds[i];
    for p in &mesh.prims {
      if p.material >= 0
        && (p.material as usize) < scene.materials.len()
        && navmesh_ignores(&scene.materials[p.material as usize])
      {
        continue;
      }
      let base = verts.len() as u32;
      verts.extend(p.pos.iter().map(|&v| w.point(v)));
      tris.extend(p.idx.iter().map(|&ix| base + ix));
      all.add_box(nb);
    }
  }
  let region = if region.valid() { *region } else { all };
  (verts, tris, region)
}

fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
  let mut name = base.to_string();
  let mut i = 2;
  while used.contains(&name) {
    name = format!("{base}-{i}");
    i += 1;
  }
  used.insert(name.clone());
  name
}

/// A generation's meshes in the document, App::poll_navmesh_generation's: they replace the nav
/// meshes of the region, as one undo step. Returns the nodes added (the editor selects them and
/// shows the nav meshes) and the step for the history.
pub fn replace_navmeshes(
  scene: &mut Scene,
  region: &AABB,
  meshes: &[NavMeshData],
) -> (Vec<u32>, HistoryEntry) {
  let mut c = NodeChange::new(scene);
  let within = region.expanded(1.0);
  let doomed: Vec<u32> = scene
    .nodes
    .iter()
    .filter(|n| n.kind == NodeKind::NavMesh && within.contains(scene.world_bounds(n).center()))
    .map(|n| n.id)
    .collect();
  c.save_subtrees(scene, &doomed);
  scene.remove(&doomed);
  let mut names: HashSet<String> = scene.nodes.iter().map(|n| n.name.clone()).collect();
  let added: Vec<u32> = meshes
    .iter()
    .map(|m| scene.add(navmesh_node(m, &unique_name("navmesh", &mut names))))
    .collect();
  let entry = c.commit(scene, strf!(tr("undo.navmesh"), added.len()), None);
  (added, entry)
}

/// The document's nav meshes for the game, App::world_navmeshes's: each where it is now (moved,
/// turned), relative to the center of its corners; and their names.
pub fn world_navmeshes(scene: &Scene) -> (Vec<NavMeshData>, Vec<String>) {
  let mut meshes = Vec::new();
  let mut names = Vec::new();
  for n in &scene.nodes {
    let Some(mut m) = navmesh_of_node(n) else {
      continue;
    };
    let w = scene.world(n);
    let mut b = AABB::default();
    for v in &mut m.verts {
      *v = w.point(*v);
      b.add(*v);
    }
    m.origin = b.center();
    for v in &mut m.verts {
      *v -= m.origin;
    }
    meshes.push(m);
    names.push(n.name.clone());
  }
  (meshes, names)
}
