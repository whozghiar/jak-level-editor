//! Recast's own steps where rerecast (its Rust port) differs from it, from
//! third-party/recast/Source:
//! - RecastRasterization.cpp, rcRasterizeTriangles: the triangles cut in voxels. Rerecast's
//!   divides an empty polygon without saying it is empty: the last row of a triangle is voxelized
//!   again in the next row (spans where nothing is), and it keeps heights over Recast's 13 bits;
//! - RecastContour.cpp, the end of rcBuildContours: the holes of a region merged into its outline,
//!   a step rerecast's build_contours does not have (its holes stay apart, wound backwards: no
//!   polygon can be made of them);
//! - RecastMesh.cpp, rcBuildPolyMesh: the contours cut in convex polygons. Rerecast's gives up on a
//!   contour it cannot triangulate, where Recast keeps the triangles found (its "Bad
//!   triangulation" warning), and compares the lengths of the diagonals in 16 bits (other polygons
//!   than Recast's on large levels).
//!
//! Recast: Copyright (c) 2009-2010 Mikko Mononen memon@inside.org
//!
//! This software is provided 'as-is', without any express or implied warranty. In no event will the
//! authors be held liable for any damages arising from the use of this software. Permission is
//! granted to anyone to use this software for any purpose, including commercial applications, and
//! to alter it and redistribute it freely, subject to the following restrictions:
//! 1. The origin of this software must not be misrepresented; you must not claim that you wrote the
//!    original software. If you use this software in a product, an acknowledgment in the product
//!    documentation would be appreciated but is not required.
//! 2. Altered source versions must be plainly marked as such, and must not be misrepresented as
//!    being the original software.
//! 3. This notice may not be removed or altered from any source distribution.
//!
//! Altered: ported to Rust, on rerecast's contours; without the polygons' regions and areas (the
//! editor does not use them) nor the removal of the vertices on a tile's border (no tiles here: the
//! regions are built without border, so no contour has such a vertex); the lengths and areas
//! computed in 64 bits and the edges counted in usize (Recast's int and unsigned short overflow
//! there).

use glam::U16Vec3;
use rerecast::{AreaType, Contour, ContourSet, Heightfield, Span, SpanKey};

pub const NULL_IDX: u16 = 0xffff;
const VERTEX_BUCKET_COUNT: usize = 1 << 12;
const CAN_REMOVE: u32 = 0x80000000;
const INDEX_MASK: u32 = 0x0fffffff;

/// a contour's vertex: x, y, z (cells)
type P = [i32; 3];

fn compute_vertex_hash(x: i32, y: i32, z: i32) -> usize {
  // Large multiplicative constants; here arbitrarily chosen primes
  let (h1, h2, h3) = (0x8da6b343u32, 0xd8163841u32, 0xcb1ab31fu32);
  let n = h1
    .wrapping_mul(x as u32)
    .wrapping_add(h2.wrapping_mul(y as u32))
    .wrapping_add(h3.wrapping_mul(z as u32));
  (n & (VERTEX_BUCKET_COUNT as u32 - 1)) as usize
}

fn add_vertex(
  v: [u16; 3],
  verts: &mut Vec<[u16; 3]>,
  first_vert: &mut [i32],
  next_vert: &mut [i32],
) -> u16 {
  let bucket = compute_vertex_hash(v[0] as i32, 0, v[2] as i32);
  let mut i = first_vert[bucket];
  while i != -1 {
    let w = verts[i as usize];
    if w[0] == v[0] && (w[1] as i32 - v[1] as i32).abs() <= 2 && w[2] == v[2] {
      return i as u16;
    }
    i = next_vert[i as usize]; // next
  }
  // Could not find, create new.
  let i = verts.len();
  verts.push(v);
  next_vert[i] = first_vert[bucket];
  first_vert[bucket] = i as i32;
  i as u16
}

fn prev(i: usize, n: usize) -> usize {
  if i >= 1 { i - 1 } else { n - 1 }
}
fn next(i: usize, n: usize) -> usize {
  if i + 1 < n { i + 1 } else { 0 }
}

fn area2(a: &P, b: &P, c: &P) -> i32 {
  (b[0] - a[0]) * (c[2] - a[2]) - (c[0] - a[0]) * (b[2] - a[2])
}

/// Returns true iff c is strictly to the left of the directed line through a to b.
fn left(a: &P, b: &P, c: &P) -> bool {
  area2(a, b, c) < 0
}

fn left_on(a: &P, b: &P, c: &P) -> bool {
  area2(a, b, c) <= 0
}

fn collinear(a: &P, b: &P, c: &P) -> bool {
  area2(a, b, c) == 0
}

/// Returns true iff ab properly intersects cd: they share a point interior to both segments. The
/// properness of the intersection is ensured by using strict leftness.
fn intersect_prop(a: &P, b: &P, c: &P, d: &P) -> bool {
  // Eliminate improper cases.
  if collinear(a, b, c) || collinear(a, b, d) || collinear(c, d, a) || collinear(c, d, b) {
    return false;
  }
  (left(a, b, c) ^ left(a, b, d)) && (left(c, d, a) ^ left(c, d, b))
}

/// Returns T iff (a,b,c) are collinear and point c lies on the closed segement ab.
fn between(a: &P, b: &P, c: &P) -> bool {
  if !collinear(a, b, c) {
    return false;
  }
  // If ab not vertical, check betweenness on x; else on y.
  if a[0] != b[0] {
    return (a[0] <= c[0] && c[0] <= b[0]) || (a[0] >= c[0] && c[0] >= b[0]);
  }
  (a[2] <= c[2] && c[2] <= b[2]) || (a[2] >= c[2] && c[2] >= b[2])
}

/// Returns true iff segments ab and cd intersect, properly or improperly.
fn intersect(a: &P, b: &P, c: &P, d: &P) -> bool {
  intersect_prop(a, b, c, d)
    || between(a, b, c)
    || between(a, b, d)
    || between(c, d, a)
    || between(c, d, b)
}

fn vequal(a: &P, b: &P) -> bool {
  a[0] == b[0] && a[2] == b[2]
}

fn vert<'a>(verts: &'a [P], indices: &[u32], i: usize) -> &'a P {
  &verts[(indices[i] & INDEX_MASK) as usize]
}

/// Returns T iff (v_i, v_j) is a proper internal *or* external diagonal of P, *ignoring edges
/// incident to v_i and v_j*. `loose`: only the proper intersections count (diagonalieLoose).
fn diagonalie(i: usize, j: usize, n: usize, verts: &[P], indices: &[u32], loose: bool) -> bool {
  let d0 = vert(verts, indices, i);
  let d1 = vert(verts, indices, j);
  // For each edge (k,k+1) of P
  for k in 0..n {
    let k1 = next(k, n);
    // Skip edges incident to i or j
    if !(k == i || k1 == i || k == j || k1 == j) {
      let p0 = vert(verts, indices, k);
      let p1 = vert(verts, indices, k1);
      if vequal(d0, p0) || vequal(d1, p0) || vequal(d0, p1) || vequal(d1, p1) {
        continue;
      }
      let crosses = if loose {
        intersect_prop(d0, d1, p0, p1)
      } else {
        intersect(d0, d1, p0, p1)
      };
      if crosses {
        return false;
      }
    }
  }
  true
}

/// Returns true iff the diagonal (i,j) is strictly internal to the polygon P in the neighborhood of
/// the i endpoint. `loose`: or on its sides (inConeLoose).
fn in_cone(i: usize, j: usize, n: usize, verts: &[P], indices: &[u32], loose: bool) -> bool {
  let pi = vert(verts, indices, i);
  let pj = vert(verts, indices, j);
  let pi1 = vert(verts, indices, next(i, n));
  let pin1 = vert(verts, indices, prev(i, n));
  // If P[i] is a convex vertex [ i+1 left or on (i-1,i) ].
  if left_on(pin1, pi, pi1) {
    return if loose {
      left_on(pi, pj, pin1) && left_on(pj, pi, pi1)
    } else {
      left(pi, pj, pin1) && left(pj, pi, pi1)
    };
  }
  // Assume (i-1,i,i+1) not collinear.
  // else P[i] is reflex.
  !(left_on(pi, pj, pi1) && left_on(pj, pi, pin1))
}

/// Returns T iff (v_i, v_j) is a proper internal diagonal of P.
fn diagonal(i: usize, j: usize, n: usize, verts: &[P], indices: &[u32]) -> bool {
  in_cone(i, j, n, verts, indices, false) && diagonalie(i, j, n, verts, indices, false)
}

fn diagonal_loose(i: usize, j: usize, n: usize, verts: &[P], indices: &[u32]) -> bool {
  in_cone(i, j, n, verts, indices, true) && diagonalie(i, j, n, verts, indices, true)
}

fn length2(p0: &P, p2: &P) -> i64 {
  let dx = (p2[0] - p0[0]) as i64;
  let dy = (p2[2] - p0[2]) as i64;
  dx * dx + dy * dy
}

/// The triangles of a contour (positions in `verts`); a contour it cannot cut entirely gives the
/// triangles found until then (Recast's negative count).
fn triangulate(mut n: usize, verts: &[P], indices: &mut [u32], tris: &mut Vec<[usize; 3]>) {
  // The last bit of the index is used to indicate if the vertex can be removed.
  for i in 0..n {
    let i1 = next(i, n);
    let i2 = next(i1, n);
    if diagonal(i, i2, n, verts, indices) {
      indices[i1] |= CAN_REMOVE;
    }
  }
  while n > 3 {
    let mut min_len = -1i64;
    let mut mini = None;
    for i in 0..n {
      let i1 = next(i, n);
      if indices[i1] & CAN_REMOVE != 0 {
        let len = length2(vert(verts, indices, i), vert(verts, indices, next(i1, n)));
        if min_len < 0 || len < min_len {
          min_len = len;
          mini = Some(i);
        }
      }
    }
    if mini.is_none() {
      // We might get here because the contour has overlapping segments, like this:
      //
      //  A o-o=====o---o B
      //   /  |C   D|    \.
      //  o   o     o     o
      //  :   :     :     :
      // We'll try to recover by loosing up the inCone test a bit so that a diagonal
      // like A-B or C-D can be found and we can continue.
      min_len = -1;
      for i in 0..n {
        let i1 = next(i, n);
        let i2 = next(i1, n);
        if diagonal_loose(i, i2, n, verts, indices) {
          let len = length2(vert(verts, indices, i), vert(verts, indices, next(i2, n)));
          if min_len < 0 || len < min_len {
            min_len = len;
            mini = Some(i);
          }
        }
      }
    }
    // The contour is messed up. This sometimes happens if the contour simplification is too
    // aggressive.
    let Some(i) = mini else { return };
    let mut i1 = next(i, n);
    let i2 = next(i1, n);
    tris.push([
      (indices[i] & INDEX_MASK) as usize,
      (indices[i1] & INDEX_MASK) as usize,
      (indices[i2] & INDEX_MASK) as usize,
    ]);

    // Removes P[i1] by copying P[i+1]...P[n-1] left one index.
    n -= 1;
    indices.copy_within(i1 + 1..n + 1, i1);
    if i1 >= n {
      i1 = 0;
    }
    let i = prev(i1, n);
    // Update diagonal flags.
    if diagonal(prev(i, n), i1, n, verts, indices) {
      indices[i] |= CAN_REMOVE;
    } else {
      indices[i] &= INDEX_MASK;
    }
    if diagonal(i, next(i1, n), n, verts, indices) {
      indices[i1] |= CAN_REMOVE;
    } else {
      indices[i1] &= INDEX_MASK;
    }
  }
  // Append the remaining triangle.
  tris.push([
    (indices[0] & INDEX_MASK) as usize,
    (indices[1] & INDEX_MASK) as usize,
    (indices[2] & INDEX_MASK) as usize,
  ]);
}

// ------------------------------------------------------------------------------------------------
// RecastRasterization.cpp: the triangles cut in voxels
// ------------------------------------------------------------------------------------------------

/// Recast keeps the spans' heights in 13 bits
const RC_SPAN_MAX_HEIGHT: i32 = (1 << 13) - 1;

fn rc_clamp(v: i32, mn: i32, mx: i32) -> i32 {
  if v < mn {
    mn
  } else if v > mx {
    mx
  } else {
    v
  }
}

/// Adds a span to the heightfield, merging it with those it overlaps (addSpan).
fn add_span(
  hf: &mut Heightfield,
  column: usize,
  min: u16,
  max: u16,
  area: AreaType,
  flag_merge_threshold: i32,
) {
  // Create the new span (its heights in 13 bits, as Recast's: a max of 8192 is 0).
  let mut new = Span {
    min: min & 0x1fff,
    max: max & 0x1fff,
    area,
    next: None,
  };
  let mut previous: Option<SpanKey> = None;
  let mut current = hf.spans[column];
  // Insert the new span, possibly merging it with existing spans.
  while let Some(key) = current {
    let cur = hf.span(key).clone();
    if cur.min > new.max {
      // Current span is completely after the new span, break.
      break;
    }
    if cur.max < new.min {
      // Current span is completely before the new span.  Keep going.
      previous = Some(key);
      current = cur.next;
      continue;
    }
    // The new span overlaps with an existing span.  Merge them.
    new.min = new.min.min(cur.min);
    new.max = new.max.max(cur.max);
    // Merge flags.
    if (new.max as i32 - cur.max as i32).abs() <= flag_merge_threshold {
      // Higher area ID numbers indicate higher resolution priority.
      new.area = AreaType(new.area.0.max(cur.area.0));
    }
    // Remove the current span since it's now merged with newSpan.
    // Keep going because there might be other overlapping spans that also need to be merged.
    hf.allocated_spans.remove(key);
    match previous {
      Some(p) => hf.span_mut(p).next = cur.next,
      None => hf.spans[column] = cur.next,
    }
    current = cur.next;
  }
  // Insert new span after prev
  match previous {
    Some(p) => {
      new.next = hf.span(p).next;
      let key = hf.allocated_spans.insert(new);
      hf.span_mut(p).next = Some(key);
    }
    None => {
      // This span should go before the others in the list
      new.next = hf.spans[column];
      let key = hf.allocated_spans.insert(new);
      hf.spans[column] = Some(key);
    }
  }
}

/// Divides a convex polygon of max 12 vertices into two convex polygons across a separating axis
/// (0: x, 2: z); returns their counts.
fn divide_poly(
  input: &[[f32; 3]],
  out1: &mut [[f32; 3]; 12],
  out2: &mut [[f32; 3]; 12],
  axis_offset: f32,
  axis: usize,
) -> (usize, usize) {
  // How far positive or negative away from the separating axis is each vertex.
  let mut delta = [0f32; 12];
  for (d, v) in delta.iter_mut().zip(input) {
    *d = axis_offset - v[axis];
  }
  let (mut n1, mut n2) = (0, 0);
  let n = input.len();
  let mut b = n.wrapping_sub(1);
  for a in 0..n {
    // If the two vertices are on the same side of the separating axis
    let same_side = (delta[a] >= 0.0) == (delta[b] >= 0.0);
    if !same_side {
      let s = delta[b] / (delta[b] - delta[a]);
      for k in 0..3 {
        out1[n1][k] = input[b][k] + (input[a][k] - input[b][k]) * s;
      }
      out2[n2] = out1[n1];
      n1 += 1;
      n2 += 1;
      // add the inVertA point to the right polygon. Do NOT add points that are on the dividing line
      // since these were already added above
      if delta[a] > 0.0 {
        out1[n1] = input[a];
        n1 += 1;
      } else if delta[a] < 0.0 {
        out2[n2] = input[a];
        n2 += 1;
      }
    } else {
      // add the inVertA point to the right polygon. Addition is done even for points on the dividing
      // line
      let mut both = true;
      if delta[a] >= 0.0 {
        out1[n1] = input[a];
        n1 += 1;
        both = delta[a] == 0.0;
      }
      if both {
        out2[n2] = input[a];
        n2 += 1;
      }
    }
    b = a;
  }
  (n1, n2)
}

/// Rasterize a single triangle to the heightfield (rasterizeTri).
fn rasterize_tri(
  v: [[f32; 3]; 3],
  area: AreaType,
  hf: &mut Heightfield,
  inverse_cell_size: f32,
  inverse_cell_height: f32,
  flag_merge_threshold: i32,
) {
  let (bmin, bmax) = (hf.aabb.min.to_array(), hf.aabb.max.to_array());
  let cs = hf.cell_size;
  // Calculate the bounding box of the triangle.
  let mut tri_min = v[0];
  let mut tri_max = v[0];
  for p in &v[1..] {
    for k in 0..3 {
      tri_min[k] = if p[k] < tri_min[k] { p[k] } else { tri_min[k] };
      tri_max[k] = if p[k] > tri_max[k] { p[k] } else { tri_max[k] };
    }
  }
  // If the triangle does not touch the bounding box of the heightfield, skip the triangle.
  if !(0..3).all(|k| tri_min[k] <= bmax[k] && tri_max[k] >= bmin[k]) {
    return;
  }
  let w = hf.width as i32;
  let h = hf.height as i32;
  let by = bmax[1] - bmin[1];

  // Calculate the footprint of the triangle on the grid's z-axis
  let z0 = ((tri_min[2] - bmin[2]) * inverse_cell_size) as i32;
  let z1 = ((tri_max[2] - bmin[2]) * inverse_cell_size) as i32;
  // use -1 rather than 0 to cut the polygon properly at the start of the tile
  let z0 = rc_clamp(z0, -1, h - 1);
  let z1 = rc_clamp(z1, 0, h - 1);

  // Clip the triangle into all grid cells it touches.
  let mut input = [[0f32; 3]; 12];
  let (mut in_row, mut p1, mut p2) = ([[0f32; 3]; 12], [[0f32; 3]; 12], [[0f32; 3]; 12]);
  input[..3].copy_from_slice(&v);
  let mut nv_in = 3;
  for z in z0..=z1 {
    // Clip polygon to row. Store the remaining polygon as well
    let cell_z = bmin[2] + z as f32 * cs;
    let (nv_row, rest) = divide_poly(&input[..nv_in], &mut in_row, &mut p1, cell_z + cs, 2);
    std::mem::swap(&mut input, &mut p1);
    nv_in = rest;
    if nv_row < 3 || z < 0 {
      continue;
    }
    // find X-axis bounds of the row
    let (mut min_x, mut max_x) = (in_row[0][0], in_row[0][0]);
    for p in &in_row[1..nv_row] {
      if min_x > p[0] {
        min_x = p[0];
      }
      if max_x < p[0] {
        max_x = p[0];
      }
    }
    let x0 = ((min_x - bmin[0]) * inverse_cell_size) as i32;
    let x1 = ((max_x - bmin[0]) * inverse_cell_size) as i32;
    if x1 < 0 || x0 >= w {
      continue;
    }
    let x0 = rc_clamp(x0, -1, w - 1);
    let x1 = rc_clamp(x1, 0, w - 1);

    let mut nv2 = nv_row;
    for x in x0..=x1 {
      // Clip polygon to column. store the remaining polygon as well
      let cx = bmin[0] + x as f32 * cs;
      let (nv, rest) = divide_poly(&in_row[..nv2], &mut p1, &mut p2, cx + cs, 0);
      std::mem::swap(&mut in_row, &mut p2);
      nv2 = rest;
      if nv < 3 || x < 0 {
        continue;
      }
      // Calculate min and max of the span.
      let (mut span_min, mut span_max) = (p1[0][1], p1[0][1]);
      for p in &p1[1..nv] {
        span_min = if p[1] < span_min { p[1] } else { span_min };
        span_max = if p[1] > span_max { p[1] } else { span_max };
      }
      span_min -= bmin[1];
      span_max -= bmin[1];
      // Skip the span if it's completely outside the heightfield bounding box
      if span_max < 0.0 || span_min > by {
        continue;
      }
      // Clamp the span to the heightfield bounding box.
      if span_min < 0.0 {
        span_min = 0.0;
      }
      if span_max > by {
        span_max = by;
      }
      // Snap the span to the heightfield height grid.
      let smin = rc_clamp(
        (span_min * inverse_cell_height).floor() as i32,
        0,
        RC_SPAN_MAX_HEIGHT,
      ) as u16;
      let smax = rc_clamp(
        (span_max * inverse_cell_height).ceil() as i32,
        smin as i32 + 1,
        RC_SPAN_MAX_HEIGHT,
      ) as u16;
      add_span(
        hf,
        (x + z * w) as usize,
        smin,
        smax,
        area,
        flag_merge_threshold,
      );
    }
  }
}

/// The triangles (of `verts`) cut in voxels, each with its area.
pub fn rasterize_triangles(
  hf: &mut Heightfield,
  verts: &[[f32; 3]],
  tris: &[[u32; 3]],
  areas: &[AreaType],
  flag_merge_threshold: i32,
) {
  let inverse_cell_size = 1.0 / hf.cell_size;
  let inverse_cell_height = 1.0 / hf.cell_height;
  for (t, &area) in tris.iter().zip(areas) {
    let v = t.map(|i| verts[i as usize]);
    rasterize_tri(
      v,
      area,
      hf,
      inverse_cell_size,
      inverse_cell_height,
      flag_merge_threshold,
    );
  }
}

// ------------------------------------------------------------------------------------------------
// RecastContour.cpp: the holes merged into the outlines
// ------------------------------------------------------------------------------------------------

fn points(c: &Contour) -> Vec<P> {
  c.vertices
    .iter()
    .map(|(v, _)| [v.x as i32, v.y as i32, v.z as i32])
    .collect()
}

fn calc_area_of_polygon_2d(verts: &[(U16Vec3, u32)]) -> i64 {
  let mut area = 0i64;
  let mut j = verts.len().wrapping_sub(1);
  for i in 0..verts.len() {
    let (vi, vj) = (verts[i].0, verts[j].0);
    area += vi.x as i64 * vj.z as i64 - vj.x as i64 * vi.z as i64;
    j = i;
  }
  (area + 1) / 2
}

/// `i`: the vertex whose edges are skipped (-1: none)
fn intersect_seg_contour(d0: &P, d1: &P, i: i64, verts: &[P]) -> bool {
  // For each edge (k,k+1) of P
  let n = verts.len();
  for k in 0..n {
    let k1 = next(k, n);
    // Skip edges incident to i.
    if i == k as i64 || i == k1 as i64 {
      continue;
    }
    let (p0, p1) = (&verts[k], &verts[k1]);
    if vequal(d0, p0) || vequal(d1, p0) || vequal(d0, p1) || vequal(d1, p1) {
      continue;
    }
    if intersect(d0, d1, p0, p1) {
      return true;
    }
  }
  false
}

fn in_cone_contour(i: usize, verts: &[P], pj: &P) -> bool {
  let n = verts.len();
  let pi = &verts[i];
  let pi1 = &verts[next(i, n)];
  let pin1 = &verts[prev(i, n)];
  // If P[i] is a convex vertex [ i+1 left or on (i-1,i) ].
  if left_on(pin1, pi, pi1) {
    return left(pi, pj, pin1) && left(pj, pi, pi1);
  }
  // Assume (i-1,i,i+1) not collinear.
  // else P[i] is reflex.
  !(left_on(pi, pj, pi1) && left_on(pj, pi, pin1))
}

fn merge_contours(contours: &mut [Contour], ca: usize, cb: usize, ia: usize, ib: usize) {
  let (a, b) = (&contours[ca].vertices, &contours[cb].vertices);
  let mut verts = Vec::with_capacity(a.len() + b.len() + 2);
  // Copy contour A.
  for i in 0..=a.len() {
    verts.push(a[(ia + i) % a.len()]);
  }
  // Copy contour B
  for i in 0..=b.len() {
    verts.push(b[(ib + i) % b.len()]);
  }
  contours[ca].vertices = verts;
  contours[cb].vertices.clear();
}

/// The C runtime's qsort as Microsoft's does it (the C++ editor's Recast sorts with it, on Windows):
/// the holes and diagonals of the same distance end in the same order, so the holes join their
/// outline where they did.
fn crt_qsort<T>(v: &mut [T], cmp: impl Fn(&T, &T) -> std::cmp::Ordering) {
  use std::cmp::Ordering::{Equal, Greater};
  const CUTOFF: usize = 8;
  if v.len() < 2 {
    return;
  }
  let mut stack: Vec<(usize, usize)> = Vec::new();
  let (mut lo, mut hi) = (0usize, v.len() - 1);
  loop {
    let size = hi - lo + 1;
    if size <= CUTOFF {
      // shortsort: the largest to the end, again and again
      let mut h = hi;
      while h > lo {
        let mut max = lo;
        for p in lo + 1..=h {
          if cmp(&v[p], &v[max]) == Greater {
            max = p;
          }
        }
        v.swap(max, h);
        h -= 1;
      }
    } else {
      let mut mid = lo + size / 2;
      if cmp(&v[lo], &v[mid]) == Greater {
        v.swap(lo, mid);
      }
      if cmp(&v[lo], &v[hi]) == Greater {
        v.swap(lo, hi);
      }
      if cmp(&v[mid], &v[hi]) == Greater {
        v.swap(mid, hi);
      }
      let (mut loguy, mut higuy) = (lo, hi);
      loop {
        if mid > loguy {
          loop {
            loguy += 1;
            if !(loguy < mid && cmp(&v[loguy], &v[mid]) != Greater) {
              break;
            }
          }
        }
        if mid <= loguy {
          loop {
            loguy += 1;
            if !(loguy <= hi && cmp(&v[loguy], &v[mid]) != Greater) {
              break;
            }
          }
        }
        loop {
          higuy -= 1;
          if !(higuy > mid && cmp(&v[higuy], &v[mid]) == Greater) {
            break;
          }
        }
        if higuy < loguy {
          break;
        }
        v.swap(loguy, higuy);
        if mid == higuy {
          mid = loguy;
        }
      }
      higuy += 1;
      if mid < higuy {
        loop {
          higuy -= 1;
          if !(higuy > mid && cmp(&v[higuy], &v[mid]) == Equal) {
            break;
          }
        }
      }
      if mid >= higuy {
        loop {
          higuy -= 1;
          if !(higuy > lo && cmp(&v[higuy], &v[mid]) == Equal) {
            break;
          }
        }
      }
      // the smaller part first, the larger one later (lo..=higuy, loguy..=hi)
      if higuy - lo >= hi - loguy {
        if lo < higuy {
          stack.push((lo, higuy));
        }
        if loguy < hi {
          lo = loguy;
          continue;
        }
      } else {
        if loguy < hi {
          stack.push((loguy, hi));
        }
        if lo < higuy {
          hi = higuy;
          continue;
        }
      }
    }
    match stack.pop() {
      Some((l, h)) => (lo, hi) = (l, h),
      None => return,
    }
  }
}

struct ContourHole {
  contour: usize,
  minx: i32,
  minz: i32,
  leftmost: usize,
}

fn merge_region_holes(contours: &mut [Contour], outline: usize, holes: &mut [ContourHole]) {
  // Sort holes from left to right: by their lowest leftmost vertex.
  for h in holes.iter_mut() {
    let v = points(&contours[h.contour]);
    (h.minx, h.minz, h.leftmost) = (v[0][0], v[0][2], 0);
    for (i, p) in v.iter().enumerate().skip(1) {
      if p[0] < h.minx || (p[0] == h.minx && p[2] < h.minz) {
        (h.minx, h.minz, h.leftmost) = (p[0], p[2], i);
      }
    }
  }
  crt_qsort(holes, |a, b| (a.minx, a.minz).cmp(&(b.minx, b.minz)));

  // the potential diagonals (vertex, distance), kept from one hole to the next as Recast's: it
  // reads the entry of the hole's own index below
  let max_verts = contours[outline].vertices.len()
    + holes
      .iter()
      .map(|h| contours[h.contour].vertices.len())
      .sum::<usize>();
  let mut diags = vec![(0usize, 0i64); max_verts];

  // Merge holes into the outline one by one.
  for i in 0..holes.len() {
    let hole = points(&contours[holes[i].contour]);
    let mut index = None;
    let mut best_vertex = holes[i].leftmost;
    for _ in 0..hole.len() {
      // Find potential diagonals.
      // The 'best' vertex must be in the cone described by 3 consecutive vertices of the outline.
      // ..o j-1
      //   |
      //   |   * best
      //   |
      // j o-----o j+1
      //         :
      let outline_verts = points(&contours[outline]);
      let corner = hole[best_vertex];
      let mut ndiags = 0;
      for j in 0..outline_verts.len() {
        if in_cone_contour(j, &outline_verts, &corner) {
          let dx = (outline_verts[j][0] - corner[0]) as i64;
          let dz = (outline_verts[j][2] - corner[2]) as i64;
          let d = (j, dx * dx + dz * dz);
          if ndiags < diags.len() {
            diags[ndiags] = d;
          } else {
            diags.push(d);
          }
          ndiags += 1;
        }
      }
      // Sort potential diagonals by distance, we want to make the connection as short as possible.
      crt_qsort(&mut diags[..ndiags], |a, b| a.1.cmp(&b.1));

      // Find a diagonal that is not intersecting the outline not the remaining holes.
      index = None;
      for j in 0..ndiags {
        let pt = outline_verts[diags[j].0];
        let mut crossing = intersect_seg_contour(&pt, &corner, diags[i].0 as i64, &outline_verts);
        for h in &holes[i..] {
          if crossing {
            break;
          }
          crossing |= intersect_seg_contour(&pt, &corner, -1, &points(&contours[h.contour]));
        }
        if !crossing {
          index = Some(diags[j].0);
          break;
        }
      }
      // If found non-intersecting diagonal, stop looking.
      if index.is_some() {
        break;
      }
      // All the potential diagonals for the current vertex were intersecting, try next vertex.
      best_vertex = (best_vertex + 1) % hole.len();
    }
    // none: Recast's "Failed to find merge points" warning
    if let Some(index) = index {
      merge_contours(contours, outline, holes[i].contour, index, best_vertex);
    }
  }
}

/// The holes of each region (its contours wound backwards) merged into its outline.
pub fn merge_holes(cset: &mut ContourSet) {
  // If the contour is wound backwards, it is a hole.
  let hole: Vec<bool> = cset
    .contours
    .iter()
    .map(|c| calc_area_of_polygon_2d(&c.vertices) < 0)
    .collect();
  if !hole.contains(&true) {
    return;
  }
  // Collect outline contour and holes contours per region. We assume that there is one outline
  // and multiple holes.
  let nregions = cset
    .contours
    .iter()
    .map(|c| c.region.bits() as usize + 1)
    .max()
    .unwrap_or(0);
  let mut outlines = vec![None; nregions];
  let mut holes: Vec<Vec<ContourHole>> = (0..nregions).map(|_| Vec::new()).collect();
  for (i, c) in cset.contours.iter().enumerate() {
    let r = c.region.bits() as usize;
    // Positively would contours are outlines, negative holes (several outlines: Recast's
    // "Multiple outlines" error, the last kept).
    if hole[i] {
      holes[r].push(ContourHole {
        contour: i,
        minx: 0,
        minz: 0,
        leftmost: 0,
      });
    } else {
      outlines[r] = Some(i);
    }
  }
  // Finally merge each regions holes into the outline (without one: Recast's "Bad outline" error,
  // the contour simplification is likely too aggressive).
  for (r, region_holes) in holes.iter_mut().enumerate() {
    if let Some(outline) = outlines[r]
      && !region_holes.is_empty()
    {
      merge_region_holes(&mut cset.contours, outline, region_holes);
    }
  }
}

// ------------------------------------------------------------------------------------------------
// RecastMesh.cpp: the polygons
// ------------------------------------------------------------------------------------------------

fn count_poly_verts(p: &[u16]) -> usize {
  p.iter().position(|&v| v == NULL_IDX).unwrap_or(p.len())
}

fn uleft(a: [u16; 3], b: [u16; 3], c: [u16; 3]) -> bool {
  (b[0] as i32 - a[0] as i32) * (c[2] as i32 - a[2] as i32)
    - (c[0] as i32 - a[0] as i32) * (b[2] as i32 - a[2] as i32)
    < 0
}

/// How good merging pa and pb is (the length of their common edge), with their edges; None when
/// they cannot.
fn get_poly_merge_value(
  pa: &[u16],
  pb: &[u16],
  verts: &[[u16; 3]],
  nvp: usize,
) -> Option<(i64, usize, usize)> {
  let na = count_poly_verts(pa);
  let nb = count_poly_verts(pb);
  // If the merged polygon would be too big, do not merge.
  if na + nb - 2 > nvp {
    return None;
  }
  // Check if the polygons share an edge.
  let mut shared = None;
  for i in 0..na {
    let (mut va0, mut va1) = (pa[i], pa[(i + 1) % na]);
    if va0 > va1 {
      std::mem::swap(&mut va0, &mut va1);
    }
    for j in 0..nb {
      let (mut vb0, mut vb1) = (pb[j], pb[(j + 1) % nb]);
      if vb0 > vb1 {
        std::mem::swap(&mut vb0, &mut vb1);
      }
      if va0 == vb0 && va1 == vb1 {
        shared = Some((i, j));
        break;
      }
    }
  }
  // No common edge, cannot merge.
  let (ea, eb) = shared?;
  // Check to see if the merged polygon would be convex.
  let v = |i: u16| verts[i as usize];
  if !uleft(v(pa[(ea + na - 1) % na]), v(pa[ea]), v(pb[(eb + 2) % nb])) {
    return None;
  }
  if !uleft(v(pb[(eb + nb - 1) % nb]), v(pb[eb]), v(pa[(ea + 2) % na])) {
    return None;
  }
  let (va, vb) = (v(pa[ea]), v(pa[(ea + 1) % na]));
  let dx = va[0] as i64 - vb[0] as i64;
  let dy = va[2] as i64 - vb[2] as i64;
  Some((dx * dx + dy * dy, ea, eb))
}

fn merge_poly_verts(pa: &[u16], pb: &[u16], ea: usize, eb: usize, nvp: usize) -> Vec<u16> {
  let na = count_poly_verts(pa);
  let nb = count_poly_verts(pb);
  // Merge polygons.
  let mut tmp = vec![NULL_IDX; nvp];
  let mut n = 0;
  // Add pa
  for i in 0..na - 1 {
    tmp[n] = pa[(ea + 1 + i) % na];
    n += 1;
  }
  // Add pb
  for i in 0..nb - 1 {
    tmp[n] = pb[(eb + 1 + i) % nb];
    n += 1;
  }
  tmp
}

fn build_mesh_adjacency(polys: &mut [u16], npolys: usize, nverts: usize, nvp: usize) {
  // Based on code by Eric Lengyel from:
  // https://web.archive.org/web/20080704083314/http://www.terathon.com/code/edges.php
  struct Edge {
    vert: [u16; 2],
    poly_edge: [usize; 2],
    poly: [usize; 2],
  }
  let mut first_edge = vec![usize::MAX; nverts];
  let mut next_edge = Vec::with_capacity(npolys * nvp);
  let mut edges: Vec<Edge> = Vec::with_capacity(npolys * nvp);
  let ends = |t: &[u16], j: usize| {
    let v1 = if j + 1 >= nvp || t[j + 1] == NULL_IDX {
      t[0]
    } else {
      t[j + 1]
    };
    (t[j], v1)
  };
  for i in 0..npolys {
    let t = &polys[i * nvp * 2..];
    for j in 0..nvp {
      if t[j] == NULL_IDX {
        break;
      }
      let (v0, v1) = ends(t, j);
      if v0 < v1 {
        edges.push(Edge {
          vert: [v0, v1],
          poly: [i, i],
          poly_edge: [j, 0],
        });
        // Insert edge
        next_edge.push(first_edge[v0 as usize]);
        first_edge[v0 as usize] = edges.len() - 1;
      }
    }
  }
  for i in 0..npolys {
    let t = &polys[i * nvp * 2..];
    for j in 0..nvp {
      if t[j] == NULL_IDX {
        break;
      }
      let (v0, v1) = ends(t, j);
      if v0 > v1 {
        let mut e = first_edge[v1 as usize];
        while e != usize::MAX {
          let edge = &mut edges[e];
          if edge.vert[1] == v0 && edge.poly[0] == edge.poly[1] {
            edge.poly[1] = i;
            edge.poly_edge[1] = j;
            break;
          }
          e = next_edge[e];
        }
      }
    }
  }
  // Store adjacency
  for e in &edges {
    if e.poly[0] != e.poly[1] {
      polys[e.poly[0] * nvp * 2 + nvp + e.poly_edge[0]] = e.poly[1] as u16;
      polys[e.poly[1] * nvp * 2 + nvp + e.poly_edge[1]] = e.poly[0] as u16;
    }
  }
}

/// The polygons of the contours, of at most `nvp` corners: the vertices (cells, as the contours'),
/// and per polygon `nvp` corners then `nvp` neighbors (NULL_IDX: none).
pub fn build_poly_mesh(cset: &ContourSet, nvp: usize) -> Result<(Vec<[u16; 3]>, Vec<u16>), String> {
  let mut max_vertices = 0;
  let mut max_tris = 0;
  let mut max_verts_per_cont = 0;
  for c in &cset.contours {
    // Skip null contours.
    if c.vertices.len() < 3 {
      continue;
    }
    max_vertices += c.vertices.len();
    max_tris += c.vertices.len() - 2;
    max_verts_per_cont = max_verts_per_cont.max(c.vertices.len());
  }
  if max_vertices >= 0xfffe {
    return Err(format!("too many vertices {max_vertices}"));
  }
  let mut verts: Vec<[u16; 3]> = Vec::with_capacity(max_vertices);
  let mut mesh_polys: Vec<u16> = Vec::new();
  let mut mesh_npolys = 0;
  let mut next_vert = vec![0i32; max_vertices];
  let mut first_vert = vec![-1i32; VERTEX_BUCKET_COUNT];
  let mut indices = vec![0u32; max_verts_per_cont];
  let mut tris = Vec::with_capacity(max_verts_per_cont);
  let mut polys = vec![NULL_IDX; max_verts_per_cont * nvp];

  for cont in &cset.contours {
    let nv = cont.vertices.len();
    // Skip null contours.
    if nv < 3 {
      continue;
    }
    // Triangulate contour
    let cverts: Vec<P> = cont
      .vertices
      .iter()
      .map(|(v, _)| [v.x as i32, v.y as i32, v.z as i32])
      .collect();
    for (j, ix) in indices.iter_mut().enumerate().take(nv) {
      *ix = j as u32;
    }
    tris.clear();
    triangulate(nv, &cverts, &mut indices, &mut tris);

    // Add and merge vertices.
    for (j, (v, _)) in cont.vertices.iter().enumerate() {
      indices[j] = add_vertex([v.x, v.y, v.z], &mut verts, &mut first_vert, &mut next_vert) as u32;
    }

    // Build initial polygons.
    let mut npolys = 0;
    polys.fill(NULL_IDX);
    for t in &tris {
      if t[0] != t[1] && t[0] != t[2] && t[1] != t[2] {
        for k in 0..3 {
          polys[npolys * nvp + k] = indices[t[k]] as u16;
        }
        npolys += 1;
      }
    }
    if npolys == 0 {
      continue;
    }

    // Merge polygons.
    if nvp > 3 {
      loop {
        // Find best polygons to merge.
        let mut best: Option<(i64, usize, usize, usize, usize)> = None;
        for j in 0..npolys - 1 {
          for k in j + 1..npolys {
            let pj = &polys[j * nvp..(j + 1) * nvp];
            let pk = &polys[k * nvp..(k + 1) * nvp];
            if let Some((v, ea, eb)) = get_poly_merge_value(pj, pk, &verts, nvp)
              && v > best.map_or(0, |b| b.0)
            {
              best = Some((v, j, k, ea, eb));
            }
          }
        }
        // Could not merge any polygons, stop.
        let Some((_, pa, pb, ea, eb)) = best else {
          break;
        };
        // Found best, merge.
        let merged = merge_poly_verts(
          &polys[pa * nvp..(pa + 1) * nvp],
          &polys[pb * nvp..(pb + 1) * nvp],
          ea,
          eb,
          nvp,
        );
        polys[pa * nvp..(pa + 1) * nvp].copy_from_slice(&merged);
        let last = npolys - 1;
        if pb != last {
          polys.copy_within(last * nvp..(last + 1) * nvp, pb * nvp);
        }
        npolys -= 1;
      }
    }

    // Store polygons.
    for j in 0..npolys {
      mesh_polys.extend_from_slice(&polys[j * nvp..(j + 1) * nvp]);
      mesh_polys.extend(std::iter::repeat_n(NULL_IDX, nvp));
      mesh_npolys += 1;
      if mesh_npolys > max_tris {
        return Err(format!("too many polygons {mesh_npolys} (max:{max_tris})"));
      }
    }
  }

  // Calculate adjacency.
  build_mesh_adjacency(&mut mesh_polys, mesh_npolys, verts.len(), nvp);
  Ok((verts, mesh_polys))
}

#[cfg(test)]
mod tests {
  use super::crt_qsort;

  #[test]
  fn qsort_as_the_crt() {
    // the ids in the order the Microsoft C runtime's qsort leaves them, keys with many ties
    let keys = [
      3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7, 9, 3, 2, 3, 8, 4, 6, 2, 6, 4, 3, 3, 8, 3, 2, 7, 9,
      5, 0, 2, 8, 8, 4, 1, 9, 7,
    ];
    let mut v: Vec<(i32, usize)> = keys.into_iter().zip(0..).collect();
    let ids = |v: &[(i32, usize)]| v.iter().map(|e| e.1).collect::<Vec<_>>();
    crt_qsort(&mut v, |a, b| a.0.cmp(&b.0));
    assert_eq!(
      ids(&v),
      [
        32, 37, 3, 1, 28, 16, 6, 21, 33, 17, 25, 9, 24, 0, 15, 27, 36, 2, 19, 23, 31, 4, 10, 8, 20,
        7, 22, 13, 29, 39, 35, 11, 18, 26, 34, 30, 5, 38, 12, 14
      ]
    );
    // a short one, sorted already
    crt_qsort(&mut v[..7], |a, b| a.0.cmp(&b.0));
    assert_eq!(ids(&v[..7]), [32, 3, 1, 37, 16, 6, 28]);
  }
}
