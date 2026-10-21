//! Editing a mesh by its vertices and faces (the topology tool). A mesh keeps a vertex once per
//! face or piece that draws it (its texture coordinates, its colors differ): the vertices at the
//! same place make one point, which moves as one so that the surface never tears. Faces are the
//! triangles of its pieces, (piece << 32 | triangle).

use std::collections::{BTreeSet, HashMap};

use super::{Mesh, Primitive};
use crate::math::{Vec3, cross, length, normalize};

/// (primitive << 32) | vertex or triangle
pub type MeshRef = u64;

pub fn mesh_ref(prim: usize, index: usize) -> MeshRef {
  ((prim as u64) << 32) | index as u32 as u64
}

pub fn ref_prim(r: MeshRef) -> usize {
  (r >> 32) as usize
}

pub fn ref_index(r: MeshRef) -> u32 {
  r as u32
}

/// The points of a mesh: its vertices at the same place (within 0.1 mm) together
#[derive(Clone, Debug, Default)]
pub struct MeshPoints {
  /// per point
  pub pos: Vec<Vec3>,
  /// per point: its vertices
  pub verts: Vec<Vec<MeshRef>>,
}

/// a position to 0.1 mm
fn point_key(v: Vec3) -> (i64, i64, i64) {
  let r = |x: f32| (x as f64 * 10000.0).round() as i64;
  (r(v.x), r(v.y), r(v.z))
}

pub fn mesh_points(mesh: &Mesh) -> MeshPoints {
  let mut out = MeshPoints::default();
  let mut at: HashMap<(i64, i64, i64), usize> = HashMap::new(); // a position -> its point
  for (pi, p) in mesh.prims.iter().enumerate() {
    for (v, &pos) in p.pos.iter().enumerate() {
      let point = *at.entry(point_key(pos)).or_insert_with(|| {
        out.pos.push(pos);
        out.verts.push(Vec::new());
        out.pos.len() - 1
      });
      out.verts[point].push(mesh_ref(pi, v));
    }
  }
  out
}

/// triangle `f`'s three vertex indices, when it is one of the mesh's
fn triangle(mesh: &Mesh, f: MeshRef) -> Option<(usize, [u32; 3])> {
  let (pi, t) = (ref_prim(f), ref_index(f) as usize);
  let idx = &mesh.prims.get(pi)?.idx;
  (t * 3 + 2 < idx.len()).then(|| (pi, [idx[t * 3], idx[t * 3 + 1], idx[t * 3 + 2]]))
}

/// The vertices of the faces `faces`
pub fn face_vertices(mesh: &Mesh, faces: &BTreeSet<MeshRef>) -> BTreeSet<MeshRef> {
  let mut out = BTreeSet::new();
  for &f in faces {
    if let Some((pi, tri)) = triangle(mesh, f) {
      out.extend(tri.map(|v| mesh_ref(pi, v as usize)));
    }
  }
  out
}

/// The faces using one of the vertices `verts`
pub fn faces_of_vertices(mesh: &Mesh, verts: &BTreeSet<MeshRef>) -> BTreeSet<MeshRef> {
  let mut out = BTreeSet::new();
  for (pi, p) in mesh.prims.iter().enumerate() {
    for (t, tri) in p.idx.chunks_exact(3).enumerate() {
      if tri
        .iter()
        .any(|&v| verts.contains(&mesh_ref(pi, v as usize)))
      {
        out.insert(mesh_ref(pi, t));
      }
    }
  }
  out
}

/// Removes the faces `faces` (the vertices stay); returns how many went
pub fn delete_faces(mesh: &mut Mesh, faces: &BTreeSet<MeshRef>) -> usize {
  let mut gone = 0;
  for (pi, p) in mesh.prims.iter_mut().enumerate() {
    let mut kept = Vec::new();
    let mut pats = Vec::new();
    for (t, tri) in p.idx.chunks_exact(3).enumerate() {
      if faces.contains(&mesh_ref(pi, t)) {
        gone += 1;
        continue;
      }
      kept.extend_from_slice(tri);
      if let Some(&pat) = p.tri_pat.get(t) {
        pats.push(pat);
      }
    }
    p.idx = kept;
    if !p.tri_pat.is_empty() {
      p.tri_pat = pats;
    }
  }
  mesh.touch();
  gone
}

/// Every attribute of vertex v of p copied as a new vertex; its index
pub fn copy_vertex(p: &mut Primitive, v: u32) -> u32 {
  let (n, v) = (p.pos.len(), v as usize);
  p.pos.push(p.pos[v]);
  if p.nrm.len() == n {
    p.nrm.push(p.nrm[v]);
  }
  if p.uv.len() == n {
    p.uv.push(p.uv[v]);
  }
  if p.col.len() == n {
    p.col.push(p.col[v]);
  }
  for a in p.extra_attrs.values_mut() {
    if a.len() == n {
      a.push(a[v]);
    }
  }
  n as u32
}

/// Every attribute of the middle of a and b of p as a new vertex; its index
pub fn middle_vertex(p: &mut Primitive, a: u32, b: u32) -> u32 {
  let (n, a, b) = (p.pos.len(), a as usize, b as usize);
  p.pos.push((p.pos[a] + p.pos[b]) * 0.5);
  if p.nrm.len() == n {
    p.nrm.push(normalize(p.nrm[a] + p.nrm[b]));
  }
  if p.uv.len() == n {
    p.uv.push((p.uv[a] + p.uv[b]) * 0.5);
  }
  if p.col.len() == n {
    p.col.push((p.col[a] + p.col[b]) * 0.5);
  }
  for x in p.extra_attrs.values_mut() {
    if x.len() == n {
      x.push(x[a]); // palettes, joints: the first one's (they are indices or bytes)
    }
  }
  n as u32
}

/// Pulls the faces `faces` out by `distance` meters along their mean normal: new faces where they
/// were moved, walls along their outer edges joining them to the rest. Returns the new faces (the
/// ones moved), to keep working on.
pub fn extrude_faces(
  mesh: &mut Mesh,
  faces: &BTreeSet<MeshRef>,
  distance: f32,
) -> BTreeSet<MeshRef> {
  let mut out = BTreeSet::new();
  // their mean normal
  let mut normal = Vec3::default();
  for &f in faces {
    if let Some((pi, [a, b, c])) = triangle(mesh, f) {
      let pos = &mesh.prims[pi].pos;
      let (a, b, c) = (pos[a as usize], pos[b as usize], pos[c as usize]);
      normal += cross(b - a, c - a);
    }
  }
  if length(normal) < 1e-9 {
    return out;
  }
  let shift = normalize(normal) * distance;
  for (pi, p) in mesh.prims.iter_mut().enumerate() {
    let tris: Vec<usize> = (0..p.idx.len() / 3)
      .filter(|&t| faces.contains(&mesh_ref(pi, t)))
      .collect();
    if tris.is_empty() {
      continue;
    }
    // the edges of the faces by their points: the outer ones belong to one face of them
    type P = (i64, i64, i64);
    let pkey = |p: &Primitive, v: u32| -> P {
      let q = p.pos[v as usize];
      let r = |x: f32| (x * 10000.0).round() as i64;
      (r(q.x), r(q.y), r(q.z))
    };
    let edge = |a: P, b: P| if a < b { (a, b) } else { (b, a) };
    let mut edge_count: HashMap<(P, P), i32> = HashMap::new();
    for &t in &tris {
      for k in 0..3 {
        let (a, b) = (
          pkey(p, p.idx[t * 3 + k]),
          pkey(p, p.idx[t * 3 + (k + 1) % 3]),
        );
        *edge_count.entry(edge(a, b)).or_default() += 1;
      }
    }
    // the faces moved: their vertices copied once, and moved
    let mut moved: HashMap<u32, u32> = HashMap::new();
    let mut moved_of = |p: &mut Primitive, v: u32| -> u32 {
      *moved.entry(v).or_insert_with(|| {
        let c = copy_vertex(p, v);
        p.pos[c as usize] += shift;
        c
      })
    };
    let mut walls: Vec<[u32; 2]> = Vec::new(); // outer edges, in the faces' order
    for &t in &tris {
      let old = [p.idx[t * 3], p.idx[t * 3 + 1], p.idx[t * 3 + 2]];
      for k in 0..3 {
        let (a, b) = (pkey(p, old[k]), pkey(p, old[(k + 1) % 3]));
        if edge_count[&edge(a, b)] == 1 {
          walls.push([old[k], old[(k + 1) % 3]]);
        }
      }
      for k in 0..3 {
        p.idx[t * 3 + k] = moved_of(p, old[k]);
      }
      out.insert(mesh_ref(pi, t));
    }
    // a wall along each outer edge: from the edge where it was to where it is (its own vertices:
    // its texture and normal are its own)
    for e in walls {
      let a = copy_vertex(p, e[0]);
      let b = copy_vertex(p, e[1]);
      let moved_b = moved_of(p, e[1]);
      let c = copy_vertex(p, moved_b);
      let moved_a = moved_of(p, e[0]);
      let d = copy_vertex(p, moved_a);
      let n = normalize(cross(p.pos[b as usize] - p.pos[a as usize], shift));
      for v in [a, b, c, d] {
        if let Some(nrm) = p.nrm.get_mut(v as usize) {
          *nrm = n;
        }
      }
      p.idx.extend([a, b, c, a, c, d]);
      if let Some(&pat) = p.tri_pat.first() {
        p.tri_pat.extend([pat, pat]);
      }
    }
  }
  mesh.touch();
  out
}

/// Cuts each face of `faces` in four (at the middle of its edges); returns the new faces
pub fn subdivide_faces(mesh: &mut Mesh, faces: &BTreeSet<MeshRef>) -> BTreeSet<MeshRef> {
  let mut out = BTreeSet::new();
  for (pi, p) in mesh.prims.iter_mut().enumerate() {
    let count = p.idx.len() / 3;
    let mut middles: HashMap<(u32, u32), u32> = HashMap::new(); // an edge's middle, made once
    let mut middle = |p: &mut Primitive, a: u32, b: u32| -> u32 {
      let k = if a < b { (a, b) } else { (b, a) };
      *middles.entry(k).or_insert_with(|| middle_vertex(p, a, b))
    };
    for t in 0..count {
      if !faces.contains(&mesh_ref(pi, t)) {
        continue;
      }
      let (a, b, c) = (p.idx[t * 3], p.idx[t * 3 + 1], p.idx[t * 3 + 2]);
      let (ab, bc, ca) = (middle(p, a, b), middle(p, b, c), middle(p, c, a));
      p.idx[t * 3 + 1] = ab;
      p.idx[t * 3 + 2] = ca;
      out.insert(mesh_ref(pi, t));
      for tri in [[ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
        out.insert(mesh_ref(pi, p.idx.len() / 3));
        p.idx.extend(tri);
        if let Some(&pat) = p.tri_pat.get(t) {
          p.tri_pat.push(pat);
        }
      }
    }
  }
  mesh.touch();
  out
}

/// Turns the faces `faces` the other way (their front becomes their back)
pub fn flip_faces(mesh: &mut Mesh, faces: &BTreeSet<MeshRef>) {
  for &f in faces {
    let Some((pi, _)) = triangle(mesh, f) else {
      continue;
    };
    let t = ref_index(f) as usize;
    let p = &mut mesh.prims[pi];
    p.idx.swap(t * 3 + 1, t * 3 + 2);
    for k in 0..3 {
      let v = p.idx[t * 3 + k] as usize;
      if let Some(n) = p.nrm.get_mut(v) {
        *n *= -1.0;
      }
    }
  }
  mesh.touch();
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::math::{Vec2, Vec4};
  use crate::scene::shapes::{ShapeParams, make_shape};

  #[test]
  fn extrude_subdivide_delete() {
    // topology: a box's points welded, a face extruded with its walls, cut in four, deleted
    let p = ShapeParams {
      size: Vec3::new(2.0, 2.0, 2.0),
      ..Default::default()
    };
    let bx = make_shape(&p, -1);
    let pts = mesh_points(&bx);
    let q = &bx.prims[0];
    let top: BTreeSet<MeshRef> = (0..q.idx.len() / 3)
      .filter(|&t| {
        let v = |k: usize| q.pos[q.idx[t * 3 + k] as usize];
        normalize(cross(v(1) - v(0), v(2) - v(0))).y > 0.9
      })
      .map(|t| mesh_ref(0, t))
      .collect();
    let mut m = (*bx).clone();
    let before = m.prims[0].idx.len() / 3;
    let moved = extrude_faces(&mut m, &top, 1.0);
    let mut high = -1e9f32;
    for &f in &moved {
      let p = &m.prims[0];
      for k in 0..3 {
        high = high.max(p.pos[p.idx[ref_index(f) as usize * 3 + k] as usize].y);
      }
    }
    let extruded = m.prims[0].idx.len() / 3;
    let cut = subdivide_faces(&mut m, &moved);
    let subdivided = m.prims[0].idx.len() / 3;
    let gone = delete_faces(&mut m, &cut);
    assert!(
      pts.pos.len() == 8
        && top.len() == 2
        && moved.len() == 2
        && (high - 3.0).abs() < 1e-4
        && extruded == before + 8
        && subdivided == extruded + 6
        && gone == 8
        && m.prims[0].idx.len() / 3 == extruded - 2
        && m.prims[0].nrm.len() == m.prims[0].pos.len(),
      "topology: a box's 8 points, its top extruded 1 m with 4 walls, cut in four, deleted"
    );
  }

  #[test]
  fn faces_vertices_flip_and_attributes() {
    // two triangles sharing an edge, with uvs, colors and a palette attribute
    let mut m = Mesh::new();
    let mut p = Primitive {
      pos: vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 1.0),
      ],
      idx: vec![0, 2, 1, 1, 2, 3],
      uv: vec![Vec2::new(0.0, 0.0); 4],
      ..Default::default()
    };
    p.compute_normals();
    p.col = vec![Vec4::default(); 4];
    p.col[0] = Vec4::new(1.0, 0.0, 0.0, 1.0);
    p.col[1] = Vec4::new(0.0, 0.0, 1.0, 1.0);
    let mut palette = vec![Vec4::default(); 4];
    palette[0].x = 7.0;
    palette[1].x = 9.0;
    p.extra_attrs.insert("_SUNRISE".into(), palette);
    m.prims.push(p);
    let first: BTreeSet<MeshRef> = [mesh_ref(0, 0)].into();
    assert_eq!(
      face_vertices(&m, &first),
      [0, 1, 2].map(|v| mesh_ref(0, v)).into()
    );
    // the faces of the shared edge's vertices: both; of vertex 3: the second
    let shared: BTreeSet<MeshRef> = [mesh_ref(0, 1), mesh_ref(0, 2)].into();
    assert_eq!(faces_of_vertices(&m, &shared).len(), 2);
    assert_eq!(
      faces_of_vertices(&m, &[mesh_ref(0, 3)].into()),
      [mesh_ref(0, 1)].into()
    );
    // flipped: the other way round, its normals too
    let version = m.version;
    flip_faces(&mut m, &first);
    let p = &m.prims[0];
    assert!(p.idx[..3] == [0, 1, 2] && p.nrm[0].y < 0.0 && p.nrm[3].y > 0.0 && m.version > version);
    // a middle vertex: halfway, its color mixed, the palette the first one's
    let p = &mut m.prims[0];
    let mid = middle_vertex(p, 0, 1) as usize;
    assert!(
      p.pos[mid] == Vec3::new(0.5, 0.0, 0.0)
        && p.col[mid] == Vec4::new(0.5, 0.0, 0.5, 1.0)
        && p.extra_attrs["_SUNRISE"][mid].x == 7.0
        && p.nrm.len() == p.pos.len()
        && p.uv.len() == p.pos.len()
    );
    let copy = copy_vertex(p, 1) as usize;
    assert!(p.pos[copy] == p.pos[1] && p.extra_attrs["_SUNRISE"][copy].x == 9.0);
  }
}
