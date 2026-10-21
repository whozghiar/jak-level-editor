//! The bones of an animated model of the game (a door, a button, a platform: the extractor writes
//! its skeleton and the animations of its art group into its .glb), and their poses: the model's
//! vertices moved by its joints (skinning) on the CPU, for the inspector's preview.

use crate::math::{Mat4, Quat, Vec3, Vec4, length, normalize};
use crate::scene::Mesh;

#[derive(Clone, Debug, Default)]
pub struct Channel {
  pub node: i32,
  /// 0 translation, 1 rotation, 2 scale
  pub path: i32,
  pub step: bool,
  pub times: Vec<f32>,
  pub values: Vec<Vec4>,
}

#[derive(Clone, Debug, Default)]
pub struct Animation {
  pub name: String,
  /// seconds; 0: a still pose
  pub duration: f32,
  pub channels: Vec<Channel>,
}

#[derive(Clone, Debug, Default)]
pub struct Skeleton {
  /// per node of the file (-1: a root)
  pub parent: Vec<i32>,
  /// per node: its own transform
  pub rest: Vec<Mat4>,
  /// per joint of the skin: its node
  pub joints: Vec<i32>,
  /// per joint
  pub inverse_bind: Vec<Mat4>,
  pub animations: Vec<Animation>,
}

/// A matrix's translation, rotation and scale
fn decompose(m: &Mat4) -> (Vec3, Quat, Vec3) {
  let t = m.col3(3);
  let (c0, c1, c2) = (m.col3(0), m.col3(1), m.col3(2));
  let s = Vec3::new(length(c0), length(c1), length(c2));
  let x = if s.x > 0.0 {
    c0 * (1.0 / s.x)
  } else {
    Vec3::new(1.0, 0.0, 0.0)
  };
  let y = if s.y > 0.0 {
    c1 * (1.0 / s.y)
  } else {
    Vec3::new(0.0, 1.0, 0.0)
  };
  let z = if s.z > 0.0 {
    c2 * (1.0 / s.z)
  } else {
    Vec3::new(0.0, 0.0, 1.0)
  };
  // the rotation matrix's quaternion
  let tr = x.x + y.y + z.z;
  let q = if tr > 0.0 {
    let k = (tr + 1.0).sqrt() * 2.0;
    Quat::new((y.z - z.y) / k, (z.x - x.z) / k, (x.y - y.x) / k, 0.25 * k)
  } else if x.x > y.y && x.x > z.z {
    let k = (1.0 + x.x - y.y - z.z).sqrt() * 2.0;
    Quat::new(0.25 * k, (y.x + x.y) / k, (z.x + x.z) / k, (y.z - z.y) / k)
  } else if y.y > z.z {
    let k = (1.0 + y.y - x.x - z.z).sqrt() * 2.0;
    Quat::new((y.x + x.y) / k, 0.25 * k, (z.y + y.z) / k, (z.x - x.z) / k)
  } else {
    let k = (1.0 + z.z - x.x - y.y).sqrt() * 2.0;
    Quat::new((z.x + x.z) / k, (z.y + y.z) / k, 0.25 * k, (x.y - y.x) / k)
  };
  (t, q.normalized(), s)
}

fn slerp(a: Quat, mut b: Quat, t: f32) -> Quat {
  let mut d = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
  if d < 0.0 {
    b = Quat::new(-b.x, -b.y, -b.z, -b.w);
    d = -d;
  }
  if d > 0.9995 {
    return Quat::new(
      a.x + (b.x - a.x) * t,
      a.y + (b.y - a.y) * t,
      a.z + (b.z - a.z) * t,
      a.w + (b.w - a.w) * t,
    )
    .normalized();
  }
  let th = d.acos();
  let s = th.sin();
  let (ka, kb) = (((1.0 - t) * th).sin() / s, (t * th).sin() / s);
  Quat::new(
    a.x * ka + b.x * kb,
    a.y * ka + b.y * kb,
    a.z * ka + b.z * kb,
    a.w * ka + b.w * kb,
  )
}

/// A channel's value at t (its keys held before the first and after the last)
fn sample(c: &Channel, t: f32) -> Vec4 {
  let (Some(&first), Some(&last)) = (c.values.first(), c.values.last()) else {
    return Vec4::default();
  };
  if c.times.len() < 2 || t <= c.times[0] {
    return first;
  }
  if t >= *c.times.last().unwrap() {
    return c.values[c.values.len().min(c.times.len()) - 1];
  }
  let i = c.times.partition_point(|&k| k <= t) - 1;
  if i + 1 >= c.values.len() {
    return last;
  }
  let (a, b) = (c.values[i], c.values[i + 1]);
  if c.step {
    return a;
  }
  let k = (t - c.times[i]) / (c.times[i + 1] - c.times[i]).max(1e-6);
  if c.path == 1 {
    let q = slerp(
      Quat::new(a.x, a.y, a.z, a.w),
      Quat::new(b.x, b.y, b.z, b.w),
      k,
    );
    return Vec4::new(q.x, q.y, q.z, q.w);
  }
  Vec4::new(
    a.x + (b.x - a.x) * k,
    a.y + (b.y - a.y) * k,
    a.z + (b.z - a.z) * k,
    a.w + (b.w - a.w) * k,
  )
}

/// Every node's world matrix at `t` seconds into animation `animation` (-1: the rest pose)
pub fn skeleton_pose(s: &Skeleton, animation: i32, t: f32) -> Vec<Mat4> {
  let n = s.rest.len();
  let mut local = s.rest.clone();
  if animation >= 0 && (animation as usize) < s.animations.len() {
    // the animated nodes: their translation, rotation and scale from the channels, the rest from
    // their own transform
    let mut trs: Vec<Option<(Vec3, Quat, Vec3)>> = vec![None; n];
    for c in &s.animations[animation as usize].channels {
      if c.node < 0 || c.node as usize >= n {
        continue;
      }
      let node = c.node as usize;
      let (tr, rot, sc) = trs[node].get_or_insert_with(|| decompose(&s.rest[node]));
      let v = sample(c, t);
      match c.path {
        0 => *tr = v.xyz(),
        1 => *rot = Quat::new(v.x, v.y, v.z, v.w).normalized(),
        _ => *sc = v.xyz(),
      }
    }
    for (i, m) in trs.into_iter().enumerate() {
      if let Some((tr, rot, sc)) = m {
        local[i] = Mat4::translate(tr) * Mat4::rotate(rot) * Mat4::scale(sc);
      }
    }
  }
  // down from the roots
  let parent = |k: usize| s.parent.get(k).copied().unwrap_or(-1);
  let mut world = vec![Mat4::identity(); n];
  let mut done = vec![false; n];
  let mut chain = Vec::new();
  for i in 0..n {
    chain.clear();
    let mut k = i as i32;
    while k >= 0 && (k as usize) < n && !done[k as usize] && chain.len() <= n {
      chain.push(k as usize);
      k = parent(k as usize);
    }
    for &k in chain.iter().rev() {
      let p = parent(k);
      world[k] = if p >= 0 && (p as usize) < n {
        world[p as usize] * local[k]
      } else {
        local[k]
      };
      done[k] = true;
    }
  }
  world
}

/// Per joint, from the nodes' world matrices: the matrices that move the vertices
pub fn joint_matrices(s: &Skeleton, worlds: &[Mat4]) -> Vec<Mat4> {
  s.joints
    .iter()
    .enumerate()
    .map(|(j, &node)| {
      let w = if node >= 0 {
        worlds
          .get(node as usize)
          .copied()
          .unwrap_or(Mat4::identity())
      } else {
        Mat4::identity()
      };
      match s.inverse_bind.get(j) {
        Some(ib) => w * *ib,
        None => w,
      }
    })
    .collect()
}

/// `out` (made from `bind`: the same primitives and vertices) with its positions and normals moved
/// by the joints; each vertex's joints and weights are bind's JOINTS_0 and WEIGHTS_0 (extra_attrs).
/// False when they do not match.
pub fn skin_mesh(bind: &Mesh, joints: &[Mat4], out: &mut Mesh) -> bool {
  if bind.prims.len() != out.prims.len() {
    return false;
  }
  for (b, o) in bind.prims.iter().zip(out.prims.iter_mut()) {
    if o.pos.len() != b.pos.len() {
      return false;
    }
    let (Some(j), Some(w)) = (
      b.extra_attrs.get("JOINTS_0"),
      b.extra_attrs.get("WEIGHTS_0"),
    ) else {
      continue; // a piece that does not move
    };
    o.nrm.resize(o.pos.len(), Vec3::default());
    for v in 0..b.pos.len() {
      let (mut p, mut nr) = (Vec3::default(), Vec3::default());
      let mut total = 0.0;
      let n = b.nrm.get(v).copied().unwrap_or(Vec3::new(0.0, 1.0, 0.0));
      for k in 0..4 {
        let weight = w[v][k];
        let joint = j[v][k] as i32;
        if weight <= 0.0 || joint < 0 || joint as usize >= joints.len() {
          continue;
        }
        p += joints[joint as usize].point(b.pos[v]) * weight;
        nr += joints[joint as usize].dir(n) * weight;
        total += weight;
      }
      o.pos[v] = if total > 0.0 {
        p * (1.0 / total)
      } else {
        b.pos[v]
      };
      o.nrm[v] = if total > 0.0 { normalize(nr) } else { n };
    }
  }
  out.touch();
  true
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::Primitive;

  #[test]
  fn skinning_follows_the_joints() {
    // the preview's skinning: a joint moved moves its vertices, the others stay; the rest pose is
    // the bind pose
    let s = Skeleton {
      parent: vec![-1, 0],
      rest: vec![Mat4::identity(), Mat4::translate(Vec3::new(0.0, 1.0, 0.0))],
      joints: vec![0, 1],
      inverse_bind: vec![Mat4::identity(), Mat4::translate(Vec3::new(0.0, -1.0, 0.0))],
      animations: vec![Animation {
        name: "lift".into(),
        duration: 1.0,
        channels: vec![Channel {
          node: 1,
          path: 0,
          step: false,
          times: vec![0.0, 1.0],
          values: vec![Vec4::new(0.0, 1.0, 0.0, 0.0), Vec4::new(0.0, 3.0, 0.0, 0.0)],
        }],
      }],
    };
    let mut bind = Mesh::new();
    let mut p = Primitive {
      pos: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)],
      nrm: vec![Vec3::new(0.0, 1.0, 0.0); 2],
      ..Default::default()
    };
    let joints = vec![Vec4::new(0.0, 0.0, 0.0, 0.0), Vec4::new(1.0, 0.0, 0.0, 0.0)];
    let weights = vec![Vec4::new(1.0, 0.0, 0.0, 0.0); 2];
    p.extra_attrs.insert("JOINTS_0".into(), joints);
    p.extra_attrs.insert("WEIGHTS_0".into(), weights);
    bind.prims.push(p);
    let mut out = bind.clone();
    let rest = skin_mesh(
      &bind,
      &joint_matrices(&s, &skeleton_pose(&s, -1, 0.0)),
      &mut out,
    ) && length(out.prims[0].pos[1] - Vec3::new(0.0, 1.0, 0.0)) < 1e-5;
    skin_mesh(
      &bind,
      &joint_matrices(&s, &skeleton_pose(&s, 0, 0.5)),
      &mut out,
    );
    assert!(rest);
    assert!(length(out.prims[0].pos[0]) < 1e-5);
    assert!(length(out.prims[0].pos[1] - Vec3::new(0.0, 2.0, 0.0)) < 1e-4);
  }
}
