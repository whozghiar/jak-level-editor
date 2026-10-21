//! Bounding volume hierarchy over the triangles of one primitive, in the primitive's local space.
//! Used for picking (ray casts) and for area queries (collision brush, snapping).

use crate::math::{AABB, Ray, Vec3, ray_aabb, ray_triangle};

const K_LEAF_SIZE: u32 = 4;

#[derive(Clone, Debug, Default)]
pub struct BvhNode {
  pub bbox: AABB,
  /// leaf: first index into tri_order; inner: index of left child
  pub first: u32,
  /// leaf: number of triangles; inner: 0 (right child = left + 1)
  pub count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Bvh {
  pub nodes: Vec<BvhNode>,
  pub tri_order: Vec<u32>,
}

impl Bvh {
  pub fn build(pos: &[Vec3], idx: &[u32]) -> Bvh {
    let mut bvh = Bvh::default();
    let ntri = (idx.len() / 3) as u32;
    if ntri == 0 {
      return bvh;
    }
    let mut tri_box = vec![AABB::default(); ntri as usize];
    let mut centroid = vec![Vec3::default(); ntri as usize];
    for t in 0..ntri as usize {
      let (a, b, c) = (
        pos[idx[t * 3] as usize],
        pos[idx[t * 3 + 1] as usize],
        pos[idx[t * 3 + 2] as usize],
      );
      tri_box[t].add(a);
      tri_box[t].add(b);
      tri_box[t].add(c);
      centroid[t] = (a + b + c) / 3.0;
    }
    bvh.tri_order = (0..ntri).collect();
    bvh.nodes.reserve((ntri * 2 / K_LEAF_SIZE + 2) as usize);

    struct Task {
      node: u32,
      first: u32,
      count: u32,
    }
    bvh.nodes.push(BvhNode::default());
    let mut stack = vec![Task {
      node: 0,
      first: 0,
      count: ntri,
    }];
    while let Some(task) = stack.pop() {
      let (mut bx, mut cbox) = (AABB::default(), AABB::default());
      let range = task.first as usize..(task.first + task.count) as usize;
      for &t in &bvh.tri_order[range.clone()] {
        bx.add_box(&tri_box[t as usize]);
        cbox.add(centroid[t as usize]);
      }
      let node = &mut bvh.nodes[task.node as usize];
      node.bbox = bx;
      if task.count <= K_LEAF_SIZE {
        node.first = task.first;
        node.count = task.count;
        continue;
      }
      let ext = cbox.size();
      let axis = if ext.x > ext.y {
        if ext.x > ext.z { 0 } else { 2 }
      } else if ext.y > ext.z {
        1
      } else {
        2
      };
      let mid = task.first + task.count / 2;
      bvh.tri_order[range].select_nth_unstable_by((mid - task.first) as usize, |&a, &b| {
        centroid[a as usize][axis].total_cmp(&centroid[b as usize][axis])
      });
      let left = bvh.nodes.len() as u32;
      bvh.nodes.push(BvhNode::default());
      bvh.nodes.push(BvhNode::default());
      let node = &mut bvh.nodes[task.node as usize];
      node.first = left;
      node.count = 0;
      stack.push(Task {
        node: left,
        first: task.first,
        count: mid - task.first,
      });
      stack.push(Task {
        node: left + 1,
        first: mid,
        count: task.first + task.count - mid,
      });
    }
    bvh
  }

  pub fn is_empty(&self) -> bool {
    self.nodes.is_empty()
  }

  /// nearest hit with t < tmax: its distance and its triangle index
  pub fn raycast(&self, ray: &Ray, pos: &[Vec3], idx: &[u32], tmax: f32) -> Option<(f32, u32)> {
    if self.nodes.is_empty() {
      return None;
    }
    let mut best = tmax;
    let mut hit = None;
    let mut stack = [0u32; 128];
    let mut sp = 1;
    while sp > 0 {
      sp -= 1;
      let n = &self.nodes[stack[sp] as usize];
      if ray_aabb(ray, &n.bbox, best).is_none() {
        continue;
      }
      if n.count > 0 {
        for &tri in &self.tri_order[n.first as usize..(n.first + n.count) as usize] {
          let i = tri as usize * 3;
          let t = ray_triangle(
            ray,
            pos[idx[i] as usize],
            pos[idx[i + 1] as usize],
            pos[idx[i + 2] as usize],
          );
          if t > 0.0 && t < best {
            best = t;
            hit = Some(tri);
          }
        }
      } else if sp + 2 <= 128 {
        stack[sp] = n.first;
        stack[sp + 1] = n.first + 1;
        sp += 2;
      }
    }
    hit.map(|tri| (best, tri))
  }

  /// triangles whose bounds overlap the box
  pub fn query(&self, bx: &AABB, pos: &[Vec3], idx: &[u32], out: &mut Vec<u32>) {
    if self.nodes.is_empty() {
      return;
    }
    let mut stack = vec![0u32];
    while let Some(i) = stack.pop() {
      let n = &self.nodes[i as usize];
      if !n.bbox.overlaps(bx) {
        continue;
      }
      if n.count > 0 {
        for &tri in &self.tri_order[n.first as usize..(n.first + n.count) as usize] {
          let i = tri as usize * 3;
          let mut tb = AABB::default();
          tb.add(pos[idx[i] as usize]);
          tb.add(pos[idx[i + 1] as usize]);
          tb.add(pos[idx[i + 2] as usize]);
          if tb.overlaps(bx) {
            out.push(tri);
          }
        }
      } else {
        stack.push(n.first);
        stack.push(n.first + 1);
      }
    }
  }
}
