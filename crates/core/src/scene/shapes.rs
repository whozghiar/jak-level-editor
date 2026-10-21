//! The shapes made in the editor (Build panel and menu): blocks, floors, walls, ramps, stairs,
//! cylinders, to build what the game's decor has not. A shape keeps its kind and size in its node's
//! extras (og_shape): the inspector changes them and the mesh is made again, its texture repeating
//! every `tile` meters instead of stretching. Its origin is the middle of its base.

use std::collections::{BTreeMap, HashMap};
use std::f32::consts::PI;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};

use super::{Material, Mesh, Node, NodeKind, Primitive, Scene, Texture, mesh_key, same_material};
use crate::math::{Vec2, Vec3, Vec4, cross, dot, length, normalize, radians};

#[derive(Clone, Debug, PartialEq)]
pub struct ShapeParams {
  /// box, ramp, stairs, cylinder (floors, walls and pillars are boxes); roof (two slopes meeting at
  /// a ridge along z), roof-hip (four slopes); doorway and window (a wall with an opening); ground
  /// (a floor with bumps)
  pub kind: String,
  /// meters: width (x), height (y), depth (z); a cylinder's x and z are its diameters
  pub size: Vec3,
  /// meters of the shape per repeat of its texture
  pub tile: f32,
  /// stairs
  pub steps: i32,
  /// cylinder
  pub sides: i32,
  /// doorway, window: the opening's width and height (meters)
  pub opening: Vec2,
  /// bent around a vertical axis, in degrees over its length (along x; ramps and stairs along their
  /// climb, z): a wall bent 90 makes a quarter of a round tower, stairs 180 turn half a turn
  pub bend: f32,
  /// box: the radius of its upright edges (meters), rounded corners
  pub round: f32,
  /// ground: a slab whose top goes up and down by `bumps` meters, its bumps `bump_size` across
  /// (a level's ground measured: scene/level_style); `seed` chooses them
  pub bumps: f32,
  pub bump_size: f32,
  pub seed: i32,
  /// its vertex colors (a level's light on it); w 0: none
  pub color: Vec4,
}

impl Default for ShapeParams {
  fn default() -> Self {
    Self {
      kind: "box".into(),
      size: Vec3::new(4.0, 2.0, 4.0),
      tile: 2.0,
      steps: 8,
      sides: 16,
      opening: Vec2::new(2.0, 3.0),
      bend: 0.0,
      round: 0.0,
      bumps: 0.3,
      bump_size: 3.0,
      seed: 1,
      color: Vec4::new(0.0, 0.0, 0.0, 0.0),
    }
  }
}

fn num(j: &Value, key: &str) -> Option<f32> {
  j.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

/// a JSON array of `n` numbers
fn numbers(j: &Value, key: &str, n: usize) -> Option<Vec<Option<f32>>> {
  let a = j.get(key)?.as_array()?;
  (a.len() == n).then(|| a.iter().map(|v| v.as_f64().map(|v| v as f32)).collect())
}

impl ShapeParams {
  pub fn to_json(&self) -> Value {
    let s = self.size;
    let mut j = json!({"kind": self.kind, "size": [s.x, s.y, s.z], "tile": self.tile});
    if self.kind == "stairs" {
      j["steps"] = json!(self.steps);
    }
    if self.kind == "cylinder" {
      j["sides"] = json!(self.sides);
    }
    if self.kind == "doorway" || self.kind == "window" {
      j["opening"] = json!([self.opening.x, self.opening.y]);
    }
    if self.bend != 0.0 {
      j["bend"] = json!(self.bend);
    }
    if self.round > 0.0 {
      j["round"] = json!(self.round);
    }
    if self.kind == "ground" {
      j["bumps"] = json!(self.bumps);
      j["bump_size"] = json!(self.bump_size);
      j["seed"] = json!(self.seed);
    }
    let c = self.color;
    if c.w > 0.0 {
      j["color"] = json!([c.x, c.y, c.z, c.w]);
    }
    j
  }

  /// clamped to sensible values
  pub fn from_json(j: &Value) -> ShapeParams {
    let mut p = ShapeParams::default();
    if !j.is_object() {
      return p;
    }
    let k = j.get("kind").and_then(Value::as_str).unwrap_or("box");
    for kind in [
      "ramp", "stairs", "cylinder", "roof", "roof-hip", "doorway", "window", "ground",
    ] {
      if k == kind {
        p.kind = k.into();
      }
    }
    if let Some(size) = numbers(j, "size", 3) {
      for (i, v) in size.into_iter().enumerate() {
        if let Some(v) = v {
          p.size[i] = v;
        }
      }
    }
    for i in 0..3 {
      p.size[i] = p.size[i].clamp(0.05, 2000.0);
    }
    if let Some(v) = num(j, "tile") {
      p.tile = v.clamp(0.1, 100.0);
    }
    if let Some(v) = num(j, "steps") {
      p.steps = (v as i32).clamp(1, 64);
    }
    if let Some(v) = num(j, "sides") {
      p.sides = (v as i32).clamp(3, 64);
    }
    if let Some(v) = num(j, "bend")
      && p.kind != "cylinder"
    {
      p.bend = v.clamp(-360.0, 360.0);
    }
    if let Some(v) = num(j, "bumps") {
      p.bumps = v.clamp(0.0, 20.0);
    }
    if let Some(v) = num(j, "bump_size") {
      p.bump_size = v.clamp(0.25, 200.0);
    }
    if let Some(v) = j.get("seed").and_then(Value::as_i64) {
      p.seed = v as i32;
    }
    if let Some(color) = numbers(j, "color", 4) {
      for (i, v) in color.into_iter().enumerate() {
        if let Some(v) = v {
          p.color[i] = v.clamp(0.0, 1.0);
        }
      }
    }
    if let Some(v) = num(j, "round")
      && p.kind == "box"
    {
      p.round = v.clamp(0.0, p.size.x.min(p.size.z) / 2.0);
    }
    if let Some(o) = numbers(j, "opening", 2)
      && let [Some(x), Some(y)] = o[..]
    {
      p.opening = Vec2::new(x, y);
    }
    // an opening within its wall: a post of 0.1 m at least on each side, a lintel above
    p.opening.x = p.opening.x.clamp(0.1, (p.size.x - 0.2).max(0.1));
    p.opening.y = p.opening.y.clamp(0.1, (p.size.y - 0.1).max(0.1));
    p
  }
}

/// What the Build panel and menu offer: a name ("box", "floor", "wall", "doorway", "window",
/// "pillar", "ramp", "stairs", "roof", "roof-hip", "cylinder") and its starting size: walls of 4 m
/// by 4 m, the grid of modular building (with the snap's 0.5 m)
#[derive(Clone, Debug)]
pub struct ShapePreset {
  pub name: &'static str,
  pub params: ShapeParams,
}

pub fn shape_presets() -> &'static [ShapePreset] {
  static PRESETS: OnceLock<Vec<ShapePreset>> = OnceLock::new();
  PRESETS.get_or_init(|| {
    let add = |name: &'static str, kind: &str, size: Vec3| ShapePreset {
      name,
      params: ShapeParams {
        kind: kind.into(),
        size,
        ..Default::default()
      },
    };
    let mut v = vec![
      add("box", "box", v3(4.0, 4.0, 4.0)),
      add("floor", "box", v3(8.0, 0.5, 8.0)),
      add("wall", "box", v3(8.0, 4.0, 0.5)),
      add("doorway", "doorway", v3(4.0, 4.0, 0.5)),
      add("window", "window", v3(4.0, 4.0, 0.5)),
      add("pillar", "box", v3(1.0, 6.0, 1.0)),
      add("ramp", "ramp", v3(4.0, 2.0, 8.0)),
      add("stairs", "stairs", v3(4.0, 3.0, 6.0)),
      add("roof", "roof", v3(8.0, 3.0, 8.0)),
      add("roof-hip", "roof-hip", v3(8.0, 3.0, 10.0)),
      add("cylinder", "cylinder", v3(2.0, 6.0, 2.0)),
      add("ground", "ground", v3(16.0, 0.5, 16.0)),
    ];
    v[4].params.opening = Vec2::new(1.6, 1.4); // the window
    v
  })
}

pub fn shape_preset(name: &str) -> Option<&'static ShapePreset> {
  shape_presets().iter().find(|p| p.name == name)
}

// ------------------------------------------------------------------------------------------------
// faces
// ------------------------------------------------------------------------------------------------

const fn v3(x: f32, y: f32, z: f32) -> Vec3 {
  Vec3::new(x, y, z)
}

/// While a bent shape is made: its faces cut in slices of this many meters along this axis (0 x,
/// 2 z), so that it bends smoothly; 0: not cut
#[derive(Clone, Copy)]
struct Cut {
  slice: f32,
  axis: usize,
}

const NO_CUT: Cut = Cut {
  slice: 0.0,
  axis: 0,
};

/// A convex polygon's part between two planes across `axis` (lo <= v[axis] <= hi)
fn clip_slab(poly: &[Vec3], axis: usize, lo: f32, hi: f32) -> Vec<Vec3> {
  let clip = |input: &[Vec3], at: f32, keep_above: bool| {
    let mut out = Vec::new();
    for i in 0..input.len() {
      let (a, b) = (input[i], input[(i + 1) % input.len()]);
      let ia = if keep_above {
        a[axis] >= at
      } else {
        a[axis] <= at
      };
      let ib = if keep_above {
        b[axis] >= at
      } else {
        b[axis] <= at
      };
      if ia {
        out.push(a);
      }
      if ia != ib {
        let k = (at - a[axis]) / (b[axis] - a[axis]);
        out.push(a + (b - a) * k);
      }
    }
    out
  };
  clip(&clip(poly, lo, true), hi, false)
}

/// A flat convex face (corners in order, either way round), facing `n`: its texture repeats every
/// `tile` meters, horizontal along its width and upright along its slope.
fn add_face(p: &mut Primitive, poly: &[Vec3], n: Vec3, tile: f32, cut: Cut) {
  if cut.slice > 0.0 {
    // in slices across the bending axis (the same planes for every face: no cracks)
    let axis = cut.axis;
    let mut lo = poly[0][axis];
    let mut hi = lo;
    for v in poly {
      lo = lo.min(v[axis]);
      hi = hi.max(v[axis]);
    }
    if hi - lo > cut.slice * 1.01 {
      let slice = cut.slice;
      let mut a = (lo / slice).floor() * slice;
      while a < hi {
        let part = clip_slab(poly, axis, a, a + slice);
        // drop the corners repeated by the cut
        let mut clean: Vec<Vec3> = Vec::new();
        for v in part {
          if clean.last().is_none_or(|&l| length(v - l) > 1e-5) {
            clean.push(v);
          }
        }
        while clean.len() > 1 && length(clean[0] - *clean.last().unwrap()) <= 1e-5 {
          clean.pop();
        }
        if clean.len() >= 3 {
          add_face(p, &clean, n, tile, NO_CUT);
        }
        a += slice;
      }
      return;
    }
  }
  let n = normalize(n);
  let mut poly = poly.to_vec();
  if dot(cross(poly[1] - poly[0], poly[2] - poly[0]), n) < 0.0 {
    poly.reverse();
  }
  let up = v3(0.0, 1.0, 0.0);
  let right = if n.y.abs() > 0.99 {
    v3(1.0, 0.0, 0.0)
  } else {
    normalize(cross(up, n))
  };
  let vup = cross(n, right);
  let first = p.pos.len() as u32;
  for &v in &poly {
    p.pos.push(v);
    p.nrm.push(n);
    p.uv
      .push(Vec2::new(dot(v, right) / tile, -dot(v, vup) / tile));
  }
  for i in 1..poly.len().saturating_sub(1) as u32 {
    p.idx.extend([first, first + i, first + i + 1]);
  }
}

fn add_box(p: &mut Primitive, lo: Vec3, hi: Vec3, tile: f32, cut: Cut) {
  let (x0, y0, z0, x1, y1, z1) = (lo.x, lo.y, lo.z, hi.x, hi.y, hi.z);
  #[rustfmt::skip]
  let faces = [
    ([v3(x1, y0, z0), v3(x1, y1, z0), v3(x1, y1, z1), v3(x1, y0, z1)], v3(1.0, 0.0, 0.0)),
    ([v3(x0, y0, z0), v3(x0, y1, z0), v3(x0, y1, z1), v3(x0, y0, z1)], v3(-1.0, 0.0, 0.0)),
    ([v3(x0, y1, z0), v3(x1, y1, z0), v3(x1, y1, z1), v3(x0, y1, z1)], v3(0.0, 1.0, 0.0)),
    ([v3(x0, y0, z0), v3(x1, y0, z0), v3(x1, y0, z1), v3(x0, y0, z1)], v3(0.0, -1.0, 0.0)),
    ([v3(x0, y0, z1), v3(x1, y0, z1), v3(x1, y1, z1), v3(x0, y1, z1)], v3(0.0, 0.0, 1.0)),
    ([v3(x0, y0, z0), v3(x1, y0, z0), v3(x1, y1, z0), v3(x0, y1, z0)], v3(0.0, 0.0, -1.0)),
  ];
  for (poly, n) in faces {
    add_face(p, &poly, n, tile, cut);
  }
}

/// A cylinder from a to b
fn add_tube(p: &mut Primitive, a: Vec3, b: Vec3, r: f32, sides: i32) {
  let axis = normalize(b - a);
  let side = if axis.y.abs() > 0.9 {
    v3(1.0, 0.0, 0.0)
  } else {
    v3(0.0, 1.0, 0.0)
  };
  let u = normalize(cross(side, axis));
  let v = cross(axis, u);
  let (mut ca, mut cb) = (Vec::new(), Vec::new());
  for i in 0..sides {
    let t = 2.0 * PI * i as f32 / sides as f32;
    let o = (u * t.cos() + v * t.sin()) * r;
    ca.push(a + o);
    cb.push(b + o);
  }
  for i in 0..sides as usize {
    let j = (i + 1) % sides as usize;
    add_face(
      p,
      &[ca[i], ca[j], cb[j], cb[i]],
      (ca[i] + ca[j]) * 0.5 - a,
      1.0,
      NO_CUT,
    );
  }
  add_face(p, &ca, axis * -1.0, 1.0, NO_CUT);
  add_face(p, &cb, axis, 1.0, NO_CUT);
}

/// A cone standing on `base`, pointing along `dir` (its length)
fn add_cone(p: &mut Primitive, base: Vec3, dir: Vec3, r: f32, sides: i32) {
  let axis = normalize(dir);
  let side = if axis.y.abs() > 0.9 {
    v3(1.0, 0.0, 0.0)
  } else {
    v3(0.0, 1.0, 0.0)
  };
  let u = normalize(cross(side, axis));
  let v = cross(axis, u);
  let tip = base + dir;
  let ring: Vec<Vec3> = (0..sides)
    .map(|i| {
      let t = 2.0 * PI * i as f32 / sides as f32;
      base + (u * t.cos() + v * t.sin()) * r
    })
    .collect();
  for i in 0..sides as usize {
    let j = (i + 1) % sides as usize;
    add_face(
      p,
      &[ring[i], ring[j], tip],
      (ring[i] + ring[j]) * 0.5 - base + axis * r,
      1.0,
      NO_CUT,
    );
  }
  add_face(p, &ring, axis * -1.0, 1.0, NO_CUT);
}

/// A line of tubes through points
fn add_path(p: &mut Primitive, points: &[Vec3], r: f32) {
  for w in points.windows(2) {
    add_tube(p, w[0], w[1], r, 6);
  }
}

// ------------------------------------------------------------------------------------------------
// the objects' icons
// ------------------------------------------------------------------------------------------------

/// A picture in 3D of an object of the game that has no model (what the game draws of it is in the
/// level's decor, or made of effects: lightning, vents, beams): what it does, for its thumbnail. By
/// its type, else its family (platform, hazard, water...); vertex colors, no material.
/// `seed`: other zigzags of its lightning (a preview draws it again and again: it crackles); 0 by
/// default
pub fn object_icon(etype: &str, family: &str, seed: i32) -> Arc<Mesh> {
  // a zigzag's offset at point i of bolt b: the same for a seed
  let zig = |b: i32, i: i32| -> f32 {
    let mut k = seed
      .wrapping_mul(92821)
      .wrapping_add(b.wrapping_mul(689287))
      .wrapping_add(i.wrapping_mul(283923481)) as u32;
    k = (k ^ (k >> 15)).wrapping_mul(2246822519);
    if seed != 0 {
      ((k >> 8) & 0xffff) as f32 / 65535.0 - 0.5
    } else if i % 2 != 0 {
      0.5
    } else {
      -0.5
    }
  };
  let mut mesh = Mesh::new();
  mesh.name = format!("icon-{etype}");
  // a piece of one color (the editor's vertex colors are doubled: 0.5 is the color as it is)
  fn part(mesh: &mut Mesh, color: Vec3, make: impl FnOnce(&mut Primitive)) {
    let mut p = Primitive::default();
    make(&mut p);
    p.col = vec![Vec4::new(color.x * 0.5, color.y * 0.5, color.z * 0.5, 1.0); p.pos.len()];
    mesh.prims.push(p);
  }
  let m = &mut mesh;
  let has = |s: &str| etype.contains(s);
  let metal = v3(0.45, 0.47, 0.5);
  let dark = v3(0.2, 0.21, 0.23);
  let bolt = v3(0.45, 0.8, 1.0);
  let lava = v3(1.0, 0.45, 0.1);
  let yellow = v3(0.95, 0.8, 0.2);
  let spiral = |m: &mut Mesh, color: Vec3, floor: Vec3| {
    part(m, floor, |p| {
      add_tube(p, v3(0.0, 0.0, 0.0), v3(0.0, 0.1, 0.0), 2.0, 24)
    });
    part(m, color, |p| {
      let pts: Vec<Vec3> = (0..=48)
        .map(|i| {
          let t = i as f32 / 48.0;
          let a = t * 4.0 * PI;
          v3(a.cos() * 1.8 * (1.0 - t), 0.15, a.sin() * 1.8 * (1.0 - t))
        })
        .collect();
      add_path(p, &pts, 0.08);
    });
  };
  if etype == "climber" {
    // someone climbing (Jak, as a figure)
    part(m, v3(0.2, 0.4, 0.85), |p| {
      add_tube(p, v3(0.0, 0.75, 0.0), v3(0.0, 1.35, 0.0), 0.25, 10);
      for x in [-0.12, 0.12] {
        add_tube(p, v3(x, 0.0, 0.0), v3(x, 0.8, 0.0), 0.09, 10);
      }
    });
    part(m, v3(0.95, 0.75, 0.55), |p| {
      add_tube(p, v3(0.0, 1.4, 0.0), v3(0.0, 1.75, 0.0), 0.18, 10);
      for x in [-0.32, 0.32] {
        add_tube(p, v3(x, 1.3, 0.0), v3(x, 1.9, 0.1), 0.07, 10);
      }
    });
  } else if has("board-vent") || etype == "sew-vent" {
    // a grate that blows upwards
    part(m, dark, |p| {
      add_tube(p, v3(0.0, 0.0, 0.0), v3(0.0, 0.15, 0.0), 1.4, 20)
    });
    part(m, metal, |p| {
      for i in -2..=2 {
        let x = i as f32 * 0.5;
        add_box(
          p,
          v3(x - 0.06, 0.15, -1.1),
          v3(x + 0.06, 0.25, 1.1),
          1.0,
          NO_CUT,
        );
      }
    });
    let color = if etype == "sew-vent" {
      v3(0.9, 0.95, 1.0)
    } else {
      bolt
    };
    part(m, color, |p| {
      add_tube(p, v3(0.0, 0.4, 0.0), v3(0.0, 2.4, 0.0), 0.18, 10);
      add_cone(p, v3(0.0, 2.4, 0.0), v3(0.0, 0.9, 0.0), 0.5, 12);
    });
  } else if etype == "swingpole" {
    // the bar Jak swings around
    part(m, metal, |p| {
      add_tube(p, v3(-2.5, 3.0, 0.0), v3(2.5, 3.0, 0.0), 0.12, 12)
    });
    part(m, dark, |p| {
      for x in [-2.5, 2.5] {
        add_tube(p, v3(x - 0.05, 3.0, 0.0), v3(x + 0.05, 3.0, 0.0), 0.25, 12);
      }
    });
    part(m, yellow, |p| {
      add_tube(p, v3(-0.8, 3.0, 0.0), v3(0.8, 3.0, 0.0), 0.14, 12)
    });
  } else if has("elec-gate") || has("vingate") {
    // two posts and the lightning between them
    part(m, dark, |p| {
      for x in [-2.0, 2.0] {
        add_tube(p, v3(x, 0.0, 0.0), v3(x, 4.0, 0.0), 0.2, 10);
      }
    });
    part(m, bolt, |p| {
      for b in 0..3 {
        let pts: Vec<Vec3> = (0..=8)
          .map(|i| {
            let inner = i % 8 != 0;
            v3(
              -2.0 + i as f32 * 0.5,
              1.0 + b as f32 + if inner { 0.5 * zig(b, i) } else { 0.0 },
              if inner { 0.3 * zig(b + 7, i) } else { 0.0 },
            )
          })
          .collect();
        add_path(p, &pts, 0.05);
      }
    });
  } else if has("conveyor") {
    // a belt and the way it goes
    part(m, dark, |p| {
      add_box(p, v3(-3.0, 0.0, -1.2), v3(3.0, 0.4, 1.2), 1.0, NO_CUT)
    });
    part(m, yellow, |p| {
      for i in -2..=2 {
        let i = i as f32;
        let tri = [
          v3(i - 0.35, 0.42, -0.8),
          v3(i + 0.35, 0.42, 0.0),
          v3(i - 0.35, 0.42, 0.8),
        ];
        add_face(p, &tri, v3(0.0, 1.0, 0.0), 1.0, NO_CUT);
      }
    });
  } else if has("laser") {
    part(m, metal, |p| {
      for x in [-3.0, 3.0] {
        add_box(
          p,
          v3(x - 0.3, 1.2, -0.3),
          v3(x + 0.3, 1.8, 0.3),
          1.0,
          NO_CUT,
        );
      }
    });
    part(m, v3(1.0, 0.15, 0.1), |p| {
      let r = if seed != 0 {
        0.04 + 0.04 * (zig(0, 1) + 0.5)
      } else {
        0.06
      };
      add_tube(p, v3(-2.7, 1.5, 0.0), v3(2.7, 1.5, 0.0), r, 10);
    });
  } else if etype == "comb-turbo" {
    // a ring of energy
    part(m, bolt, |p| {
      let pts: Vec<Vec3> = (0..=24)
        .map(|i| {
          let r = 2.0 + if i % 24 != 0 { 0.2 * zig(0, i) } else { 0.0 };
          let a = 2.0 * PI * i as f32 / 24.0;
          v3(a.cos() * r, 2.2 + a.sin() * r, 0.0)
        })
        .collect();
      add_path(p, &pts, 0.15);
    });
  } else if etype == "propa" {
    // a loudspeaker on its pole
    part(m, dark, |p| {
      add_tube(p, v3(0.0, 0.0, 0.0), v3(0.0, 3.0, 0.0), 0.08, 10)
    });
    part(m, metal, |p| {
      add_box(p, v3(-0.5, 3.0, -0.4), v3(0.5, 3.8, 0.4), 1.0, NO_CUT)
    });
    part(m, v3(0.9, 0.3, 0.25), |p| {
      add_cone(p, v3(0.0, 3.4, 1.1), v3(0.0, 0.0, -0.7), 0.45, 12)
    });
  } else if has("whirlpool") {
    spiral(m, v3(0.5, 0.75, 1.0), v3(0.15, 0.3, 0.5));
  } else if has("spinning-hole") {
    spiral(m, v3(0.85, 0.5, 1.0), dark);
  } else if has("lava") {
    // a spout of lava
    part(m, dark, |p| {
      add_cone(p, v3(0.0, 0.0, 0.0), v3(0.0, 0.8, 0.0), 1.2, 12)
    });
    part(m, lava, |p| {
      add_tube(p, v3(0.0, 0.6, 0.0), v3(0.0, 2.5, 0.0), 0.25, 10);
      for y in [3.0, 3.8] {
        add_box(p, v3(-0.25, y, -0.25), v3(0.25, y + 0.5, 0.25), 1.0, NO_CUT);
      }
    });
  } else if family == "water" {
    part(m, v3(0.3, 0.55, 0.9), |p| {
      add_tube(p, v3(0.0, 0.0, 0.0), v3(0.0, 0.1, 0.0), 2.0, 24)
    });
  } else if family == "platform" {
    part(m, metal, |p| {
      add_box(p, v3(-2.0, 0.0, -2.0), v3(2.0, 0.5, 2.0), 1.0, NO_CUT)
    });
  } else if family == "hazard" {
    part(m, metal, |p| {
      add_box(p, v3(-2.0, 0.0, -2.0), v3(2.0, 0.2, 2.0), 1.0, NO_CUT)
    });
    part(m, v3(0.85, 0.25, 0.2), |p| {
      for x in [-1.0, 0.0, 1.0] {
        for z in [-1.0, 0.0, 1.0] {
          add_cone(p, v3(x, 0.2, z), v3(0.0, 0.9, 0.0), 0.3, 6);
        }
      }
    });
  } else {
    // something the game does there: a marker with an arrow
    part(m, metal, |p| {
      add_tube(p, v3(0.0, 0.0, 0.0), v3(0.0, 0.15, 0.0), 1.2, 16)
    });
    part(m, bolt, |p| {
      add_tube(p, v3(0.0, 0.15, 0.0), v3(0.0, 1.8, 0.0), 0.15, 10);
      add_cone(p, v3(0.0, 1.8, 0.0), v3(0.0, 0.8, 0.0), 0.45, 12);
    });
  }
  Arc::new(mesh)
}

// ------------------------------------------------------------------------------------------------
// the shapes' meshes
// ------------------------------------------------------------------------------------------------

/// The shape's mesh, every face with `material`, its UVs in meters / tile
pub fn make_shape(s: &ShapeParams, material: i32) -> Arc<Mesh> {
  let (w, h, d, t) = (s.size.x / 2.0, s.size.y, s.size.z / 2.0, s.tile);
  let mut p = Primitive {
    material,
    ..Default::default()
  };
  // bent: cut in slices along its length first, about 4 degrees each
  let axis = if s.kind == "ramp" || s.kind == "stairs" {
    2
  } else {
    0
  };
  let mut cut = Cut { slice: 0.0, axis };
  if s.bend.abs() > 0.5 {
    let length_along = if axis == 0 { s.size.x } else { s.size.z };
    cut.slice = length_along / (s.bend.abs() / 4.0).clamp(4.0, 96.0);
  }
  let face = |p: &mut Primitive, poly: &[Vec3], n: Vec3| add_face(p, poly, n, t, cut);
  let bx = |p: &mut Primitive, lo: Vec3, hi: Vec3| add_box(p, lo, hi, t, cut);
  match s.kind.as_str() {
    "ramp" => {
      // rising towards +z
      face(
        &mut p,
        &[
          v3(-w, 0.0, -d),
          v3(w, 0.0, -d),
          v3(w, 0.0, d),
          v3(-w, 0.0, d),
        ],
        v3(0.0, -1.0, 0.0),
      );
      face(
        &mut p,
        &[v3(-w, 0.0, d), v3(w, 0.0, d), v3(w, h, d), v3(-w, h, d)],
        v3(0.0, 0.0, 1.0),
      );
      face(
        &mut p,
        &[v3(-w, 0.0, -d), v3(w, 0.0, -d), v3(w, h, d), v3(-w, h, d)],
        v3(0.0, 2.0 * d, -h),
      );
      face(
        &mut p,
        &[v3(w, 0.0, -d), v3(w, 0.0, d), v3(w, h, d)],
        v3(1.0, 0.0, 0.0),
      );
      face(
        &mut p,
        &[v3(-w, 0.0, -d), v3(-w, 0.0, d), v3(-w, h, d)],
        v3(-1.0, 0.0, 0.0),
      );
    }
    "stairs" => {
      // rising towards +z: one block under each step
      let n = s.steps.max(1);
      for i in 0..n {
        let z0 = -d + 2.0 * d * i as f32 / n as f32;
        let z1 = -d + 2.0 * d * (i + 1) as f32 / n as f32;
        bx(
          &mut p,
          v3(-w, 0.0, z0),
          v3(w, h * (i + 1) as f32 / n as f32, z1),
        );
      }
    }
    "roof" | "roof-hip" => {
      // two slopes meeting at a ridge along z, height h; a hip roof slopes at its ends too (its
      // ridge shorter by its width, a pyramid when it is square)
      let hip = s.kind == "roof-hip";
      let r = if hip { (d - w).max(0.0) } else { d };
      face(
        &mut p,
        &[
          v3(-w, 0.0, -d),
          v3(0.0, h, -r),
          v3(0.0, h, r),
          v3(-w, 0.0, d),
        ],
        v3(-h, w, 0.0),
      );
      face(
        &mut p,
        &[v3(w, 0.0, -d), v3(0.0, h, -r), v3(0.0, h, r), v3(w, 0.0, d)],
        v3(h, w, 0.0),
      );
      if hip {
        face(
          &mut p,
          &[v3(-w, 0.0, d), v3(w, 0.0, d), v3(0.0, h, r)],
          v3(0.0, d - r, h),
        );
        face(
          &mut p,
          &[v3(-w, 0.0, -d), v3(w, 0.0, -d), v3(0.0, h, -r)],
          v3(0.0, d - r, -h),
        );
      } else {
        face(
          &mut p,
          &[v3(-w, 0.0, d), v3(w, 0.0, d), v3(0.0, h, d)],
          v3(0.0, 0.0, 1.0),
        );
        face(
          &mut p,
          &[v3(-w, 0.0, -d), v3(w, 0.0, -d), v3(0.0, h, -d)],
          v3(0.0, 0.0, -1.0),
        );
      }
      face(
        &mut p,
        &[
          v3(-w, 0.0, -d),
          v3(w, 0.0, -d),
          v3(w, 0.0, d),
          v3(-w, 0.0, d),
        ],
        v3(0.0, -1.0, 0.0),
      );
    }
    "doorway" | "window" => {
      // a wall around its opening: a door from the ground, a window in the middle of its height
      let ow = s.opening.x.min(s.size.x - 0.2) / 2.0;
      let oh = s.opening.y.min(h - 0.1);
      let bottom = if s.kind == "window" {
        (h - oh) / 2.0
      } else {
        0.0
      };
      let top = bottom + oh;
      bx(&mut p, v3(-w, 0.0, -d), v3(-ow, h, d));
      bx(&mut p, v3(ow, 0.0, -d), v3(w, h, d));
      bx(&mut p, v3(-ow, top, -d), v3(ow, h, d));
      if bottom > 0.0 {
        bx(&mut p, v3(-ow, 0.0, -d), v3(ow, bottom, d));
      }
    }
    "cylinder" => {
      let n = s.sides.max(3) as usize;
      let (rx, rz) = (w, d);
      let (mut top, mut bottom) = (Vec::new(), Vec::new());
      for i in 0..n {
        let a = 2.0 * PI * i as f32 / n as f32;
        top.push(v3(rx * a.cos(), h, rz * a.sin()));
        bottom.push(v3(rx * a.cos(), 0.0, rz * a.sin()));
      }
      for i in 0..n {
        let j = (i + 1) % n;
        let mid = (bottom[i] + bottom[j]) * 0.5;
        face(
          &mut p,
          &[bottom[i], bottom[j], top[j], top[i]],
          v3(mid.x, 0.0, mid.z),
        );
      }
      face(&mut p, &top, v3(0.0, 1.0, 0.0));
      face(&mut p, &bottom, v3(0.0, -1.0, 0.0));
    }
    "ground" => make_ground(s, &mut p, cut),
    _ if s.round > 0.01 => {
      // its upright edges rounded: a rounded rectangle, raised
      let r = s.round.min(w).min(d);
      let corners = [
        Vec2::new(w - r, d - r),
        Vec2::new(-(w - r), d - r),
        Vec2::new(-(w - r), -(d - r)),
        Vec2::new(w - r, -(d - r)),
      ];
      let mut ring = Vec::new();
      for (c, corner) in corners.iter().enumerate() {
        for i in 0..=6 {
          let a = PI * 0.5 * (c as f32 + i as f32 / 6.0);
          ring.push(v3(corner.x + r * a.cos(), 0.0, corner.y + r * a.sin()));
        }
      }
      let bottom = ring.clone();
      let top: Vec<Vec3> = ring.iter().map(|v| v3(v.x, h, v.z)).collect();
      for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        if length(ring[j] - ring[i]) < 1e-5 {
          continue;
        }
        let mid = (ring[i] + ring[j]) * 0.5;
        let along = ring[j] - ring[i];
        let out = v3(along.z, 0.0, -along.x);
        let out = out * if dot(out, mid) < 0.0 { -1.0 } else { 1.0 };
        face(&mut p, &[bottom[i], bottom[j], top[j], top[i]], out);
      }
      face(&mut p, &top, v3(0.0, 1.0, 0.0));
      face(&mut p, &bottom, v3(0.0, -1.0, 0.0));
    }
    _ => bx(&mut p, v3(-w, 0.0, -d), v3(w, h, d)),
  }
  if s.bend.abs() > 0.5 {
    // around a vertical axis: x (or z) along an arc, the other across it; the arc's middle stays
    let (a, c) = (axis, 2 - axis);
    let length_along = if a == 0 { s.size.x } else { s.size.z };
    let radius = length_along / radians(s.bend);
    for i in 0..p.pos.len() {
      let v = &mut p.pos[i];
      let th = v[a] / radius;
      let rr = radius - v[c];
      let (ca, sa) = (th.cos(), th.sin());
      let n = p.nrm[i];
      v[a] = rr * sa;
      v[c] = radius - rr * ca;
      let mut m = n;
      m[a] = n[a] * ca - n[c] * sa;
      m[c] = n[a] * sa + n[c] * ca;
      p.nrm[i] = normalize(m);
    }
  }
  if s.color.w > 0.0 {
    p.col = vec![s.color; p.pos.len()];
  }
  let mut mesh = Mesh::new();
  mesh.name = format!("shape-{}", s.kind);
  mesh.prims.push(p);
  Arc::new(mesh)
}

/// its top on a grid, raised and lowered by a smooth noise; its sides follow it
fn make_ground(s: &ShapeParams, p: &mut Primitive, cut: Cut) {
  let (w, h, d, t) = (s.size.x / 2.0, s.size.y, s.size.z / 2.0, s.tile);
  let step = (s.bump_size / 3.0).clamp(0.25, 4.0);
  let cells = |size: f32| {
    if s.bumps > 0.0 {
      ((size / step).ceil() as i32).clamp(1, 128)
    } else {
      1
    }
  };
  let (nx, nz) = (cells(s.size.x), cells(s.size.z));
  let noise = |x: f32, z: f32| smooth_noise(x, z, s.seed);
  let height = |x: f32, z: f32| {
    if s.bumps <= 0.0 {
      return h;
    }
    let (u, v) = (x / s.bump_size, z / s.bump_size);
    h + s.bumps * (noise(u, v) + 0.35 * noise(u * 2.3 + 17.0, v * 2.3 + 5.0))
  };
  let first = 0u32;
  for iz in 0..=nz {
    for ix in 0..=nx {
      let x = -w + 2.0 * w * ix as f32 / nx as f32;
      let z = -d + 2.0 * d * iz as f32 / nz as f32;
      p.pos.push(v3(x, height(x, z), z));
      p.uv.push(Vec2::new(x / t, z / t));
    }
  }
  for iz in 0..nz as u32 {
    for ix in 0..nx as u32 {
      let a = first + iz * (nx as u32 + 1) + ix;
      let (b, c) = (a + 1, a + nx as u32 + 1);
      let e = c + 1;
      p.idx.extend([a, c, b, b, c, e]);
    }
  }
  p.compute_normals();
  // the sides, along its edges, and its bottom
  let mut side = |a: Vec3, b: Vec3, out: Vec3| {
    let (ha, hb) = (height(a.x, a.z), height(b.x, b.z));
    let at = p.pos.len() as u32;
    let along = length(b - a);
    for v in [
      v3(a.x, 0.0, a.z),
      v3(b.x, 0.0, b.z),
      v3(b.x, hb, b.z),
      v3(a.x, ha, a.z),
    ] {
      p.pos.push(v);
      p.nrm.push(out);
    }
    p.uv.extend([
      Vec2::new(0.0, 0.0),
      Vec2::new(along / t, 0.0),
      Vec2::new(along / t, -hb / t),
      Vec2::new(0.0, -ha / t),
    ]);
    if dot(cross(b - a, v3(0.0, 1.0, 0.0)), out) > 0.0 {
      p.idx.extend([at, at + 1, at + 2, at, at + 2, at + 3]);
    } else {
      p.idx.extend([at, at + 2, at + 1, at, at + 3, at + 2]);
    }
  };
  for ix in 0..nx {
    let x0 = -w + 2.0 * w * ix as f32 / nx as f32;
    let x1 = -w + 2.0 * w * (ix + 1) as f32 / nx as f32;
    side(v3(x0, 0.0, -d), v3(x1, 0.0, -d), v3(0.0, 0.0, -1.0));
    side(v3(x0, 0.0, d), v3(x1, 0.0, d), v3(0.0, 0.0, 1.0));
  }
  for iz in 0..nz {
    let z0 = -d + 2.0 * d * iz as f32 / nz as f32;
    let z1 = -d + 2.0 * d * (iz + 1) as f32 / nz as f32;
    side(v3(-w, 0.0, z0), v3(-w, 0.0, z1), v3(-1.0, 0.0, 0.0));
    side(v3(w, 0.0, z0), v3(w, 0.0, z1), v3(1.0, 0.0, 0.0));
  }
  let bottom = [
    v3(-w, 0.0, -d),
    v3(w, 0.0, -d),
    v3(w, 0.0, d),
    v3(-w, 0.0, d),
  ];
  add_face(p, &bottom, v3(0.0, -1.0, 0.0), t, cut);
}

/// A smooth noise over the plane, -0.5..0.5, one bump per unit, the same for the same seed (the
/// ground's bumps, the terrain brush's relief)
pub fn smooth_noise(x: f32, z: f32, seed: i32) -> f32 {
  let hash = |x: i32, z: i32| {
    let mut k = (x as u32).wrapping_mul(73856093)
      ^ (z as u32).wrapping_mul(19349663)
      ^ (seed as u32).wrapping_mul(83492791);
    k = (k ^ (k >> 13)).wrapping_mul(1274126177);
    ((k ^ (k >> 16)) & 0xffff) as f32 / 65535.0 - 0.5
  };
  let (x0, z0) = (x.floor() as i32, z.floor() as i32);
  let mut fx = x - x0 as f32;
  let mut fz = z - z0 as f32;
  fx = fx * fx * (3.0 - 2.0 * fx);
  fz = fz * fz * (3.0 - 2.0 * fz);
  let a = hash(x0, z0) + (hash(x0 + 1, z0) - hash(x0, z0)) * fx;
  let b = hash(x0, z0 + 1) + (hash(x0 + 1, z0 + 1) - hash(x0, z0 + 1)) * fx;
  a + (b - a) * fz
}

/// The node's shape, when it is one
pub fn node_shape(n: &Node) -> Option<ShapeParams> {
  n.extras
    .get("og_shape")
    .filter(|v| v.is_object())
    .map(ShapeParams::from_json)
}

/// The material of new shapes: a grey grid texture (one cell per repeat), added to the scene once
pub fn shape_default_material(scene: &mut Scene) -> i32 {
  if let Some(i) = scene.materials.iter().position(|m| m.name == "ogle-grid") {
    return i as i32;
  }
  // light grey, a darker line around each cell
  let mut t = Texture {
    name: "ogle-grid".into(),
    width: 64,
    height: 64,
    rgba: vec![0; 64 * 64 * 4],
    ..Default::default()
  };
  for y in 0..64 {
    for x in 0..64 {
      let line = x < 2 || y < 2 || x > 61 || y > 61;
      let v = if line { 96 } else { 168 };
      let px = &mut t.rgba[(y * 64 + x) * 4..][..4];
      px.copy_from_slice(&[v, v, v, 255]);
    }
  }
  let m = Material {
    name: "ogle-grid".into(),
    texture: scene.add_texture(t),
    ..Default::default()
  };
  scene.add_material(m)
}

/// A shape node's mesh made again from `p` (its extras too), with its material
pub fn reshape(scene: &mut Scene, id: u32, p: &ShapeParams) {
  let Some(i) = scene.index_of(id) else { return };
  let first = scene.nodes[i]
    .mesh
    .as_ref()
    .and_then(|m| m.prims.first())
    .map(|q| q.material);
  let material = first.unwrap_or_else(|| shape_default_material(scene));
  let n = &mut scene.nodes[i];
  n.mesh = Some(make_shape(p, material));
  n.extras["og_shape"] = p.to_json();
}

/// An image file (PNG, JPG, TGA, BMP) as a texture of the scene (the same image once); the error
/// when unreadable
pub fn texture_from_image(scene: &mut Scene, path: impl AsRef<Path>) -> Result<i32, String> {
  let path = path.as_ref();
  let bytes = std::fs::read(path).unwrap_or_default();
  if bytes.is_empty() {
    return Err("cannot read the file".into());
  }
  let img = image::load_from_memory(&bytes)
    .map_err(|e| e.to_string())?
    .to_rgba8();
  let t = Texture {
    name: path
      .file_stem()
      .map(|s| s.to_string_lossy().into_owned())
      .unwrap_or_default(),
    width: img.width() as i32,
    height: img.height() as i32,
    rgba: img.into_raw(),
    ..Default::default()
  };
  Ok(add_texture_once(scene, t))
}

/// A texture of the scene: the one it has with the same name and image, else this one added
pub fn add_texture_once(scene: &mut Scene, t: Texture) -> i32 {
  if let Some(i) = scene.textures.iter().position(|d| {
    d.name == t.name && d.width == t.width && d.height == t.height && d.rgba == t.rgba
  }) {
    return i as i32;
  }
  scene.add_texture(t)
}

/// Gives the Render nodes among `ids` that draw material `material` (-1: every material of theirs)
/// a new material made by `edit` from it (an identical one is shared). Their meshes are copied
/// (shared again between them): the other nodes drawing the material keep it, and a NodeChange
/// made before undoes it. Returns how many nodes changed.
pub fn edit_material(
  scene: &mut Scene,
  ids: &[u32],
  material: i32,
  edit: impl Fn(&mut Material),
) -> usize {
  // material -> its edited copy
  let mut remapped: BTreeMap<i32, i32> = BTreeMap::new();
  let mut remap = |scene: &mut Scene, old: i32| -> i32 {
    if let Some(&m) = remapped.get(&old) {
      return m;
    }
    let mut m = if old >= 0 && (old as usize) < scene.materials.len() {
      scene.materials[old as usize].clone()
    } else {
      Material::default()
    };
    edit(&mut m);
    let found = match scene.materials.iter().position(|o| same_material(o, &m)) {
      Some(i) => i as i32,
      None => scene.add_material(m),
    };
    remapped.insert(old, found);
    found
  };
  // shared meshes stay shared (the original kept with its copy: its address stays its own)
  let mut copies: HashMap<usize, (Arc<Mesh>, Arc<Mesh>)> = HashMap::new();
  let mut changed = 0;
  for &id in ids {
    let Some(i) = scene.index_of(id) else {
      continue;
    };
    let n = &scene.nodes[i];
    let Some(mesh) = n.mesh.clone().filter(|_| n.kind == NodeKind::Render) else {
      continue;
    };
    let key = mesh_key(&n.mesh);
    let copy = match copies.get(&key) {
      Some((_, copy)) => copy.clone(),
      None => {
        let uses = mesh
          .prims
          .iter()
          .any(|p| material < 0 || p.material == material);
        if !uses {
          continue;
        }
        let mut copy = (*mesh).clone();
        for p in &mut copy.prims {
          if material < 0 || p.material == material {
            p.material = remap(scene, p.material);
          }
        }
        copy.touch();
        let copy = Arc::new(copy);
        copies.insert(key, (mesh, copy.clone()));
        copy
      }
    };
    scene.nodes[i].mesh = Some(copy);
    changed += 1;
  }
  changed
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::history::NodeChange;
  use crate::math::Ray;

  fn near3(a: Vec3, b: Vec3) -> bool {
    length(a - b) < 1e-4
  }

  #[test]
  fn shapes_sizes_and_faces() {
    let bx = ShapeParams {
      size: v3(4.0, 2.0, 6.0),
      tile: 2.0,
      ..Default::default()
    };
    let bm = make_shape(&bx, 0);
    let bb = bm.bounds();
    let bp = &bm.prims[0];
    let (mut umin, mut umax) = (1e9f32, -1e9f32);
    for uv in &bp.uv {
      umin = umin.min(uv.x);
      umax = umax.max(uv.x);
    }
    let mut outward = true;
    for t in bp.idx.chunks_exact(3) {
      let (a, b, c) = (
        bp.pos[t[0] as usize],
        bp.pos[t[1] as usize],
        bp.pos[t[2] as usize],
      );
      let nrm = bp.nrm[t[0] as usize];
      outward &= dot(cross(b - a, c - a), nrm) > 0.0
        && dot(nrm, (a + b + c) / 3.0 - v3(0.0, 1.0, 0.0)) > 0.0;
    }
    assert!(
      bm.tri_count() == 12
        && near3(bb.lo, v3(-2.0, 0.0, -3.0))
        && near3(bb.hi, v3(2.0, 2.0, 3.0))
        && (umax - umin - 3.0).abs() < 1e-4
        && outward,
      "shapes: a box of its size on its base, its texture every 2 m, its faces outwards"
    );
    let stairs = ShapeParams {
      kind: "stairs".into(),
      size: v3(2.0, 2.0, 4.0),
      steps: 4,
      ..Default::default()
    };
    let ramp = ShapeParams {
      kind: "ramp".into(),
      ..Default::default()
    };
    let cyl = ShapeParams {
      kind: "cylinder".into(),
      sides: 16,
      ..Default::default()
    };
    let odd = ShapeParams::from_json(&json!({"kind": "nope", "size": [-1, 5, 5], "tile": 0}));
    assert!(
      make_shape(&stairs, 0).tri_count() == 48
        && (make_shape(&stairs, 0).bounds().hi.y - 2.0).abs() < 1e-4
        && make_shape(&ramp, 0).tri_count() == 8
        && make_shape(&cyl, 0).tri_count() == 60
        && odd.kind == "box"
        && odd.size.x == 0.05
        && odd.tile == 0.1,
      "shapes: stairs, ramp and cylinder; sizes read back kept sensible"
    );
    // modular building: a door and a window go through their wall, a roof's ridge in its middle
    let through = |m: &Mesh, at: Vec3| {
      let r = Ray::new(v3(at.x, at.y, at.z - 5.0), v3(0.0, 0.0, 1.0));
      m.bvh(0)
        .raycast(&r, &m.prims[0].pos, &m.prims[0].idx, 100.0)
        .is_none()
    };
    let door =
      ShapeParams::from_json(&json!({"kind": "doorway", "size": [4, 4, 0.5], "opening": [2, 3]}));
    let window =
      ShapeParams::from_json(&json!({"kind": "window", "size": [4, 4, 0.5], "opening": [2, 2]}));
    let (dm, wm) = (make_shape(&door, 0), make_shape(&window, 0));
    let roof = ShapeParams {
      kind: "roof".into(),
      size: v3(8.0, 3.0, 8.0),
      ..Default::default()
    };
    let hip = ShapeParams {
      kind: "roof-hip".into(),
      size: v3(8.0, 3.0, 10.0),
      ..Default::default()
    };
    let (rm, hm) = (make_shape(&roof, 0), make_shape(&hip, 0));
    let ridge = |m: &Mesh, along: fn(&Vec3) -> f32| {
      m.prims[0]
        .pos
        .iter()
        .filter(|v| (v.y - 3.0).abs() < 1e-4)
        .map(along)
        .fold(0f32, f32::max)
    };
    let (ridge, hip_ridge) = (ridge(&rm, |v| v.x.abs()), ridge(&hm, |v| v.z.abs()));
    assert!(
      through(&dm, v3(0.0, 1.0, 0.0))
        && !through(&dm, v3(1.5, 1.0, 0.0))
        && !through(&dm, v3(0.0, 3.5, 0.0))
        && through(&wm, v3(0.0, 2.0, 0.0))
        && !through(&wm, v3(0.0, 0.5, 0.0))
        && door.opening.x == 2.0
        && ridge < 1e-4
        && (rm.bounds().hi.y - 3.0).abs() < 1e-4
        && (hip_ridge - 1.0).abs() < 1e-4
        && shape_preset("roof").is_some()
        && shape_preset("doorway").is_some()
        && shape_preset("pillar").is_some(),
      "shapes: a door and a window open through their wall, a roof's ridge in its middle, a hip roof's shorter"
    );
  }

  #[test]
  fn image_texture_and_material_on_some_pieces() {
    let bx = ShapeParams {
      size: v3(4.0, 2.0, 6.0),
      tile: 2.0,
      ..Default::default()
    };
    let mut s = Scene::new();
    let grid = shape_default_material(&mut s);
    let piece = Node {
      kind: NodeKind::Render,
      mesh: Some(make_shape(&bx, grid)),
      extras: json!({"og_shape": bx.to_json()}),
      ..Default::default()
    };
    let (ia, ib, ic) = (s.add(piece.clone()), s.add(piece.clone()), s.add(piece));
    let tmp = std::env::temp_dir().join(format!("jle-shapes-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let px = [
      200, 50, 50, 255, 50, 200, 50, 255, 50, 50, 200, 255, 200, 200, 200, 255,
    ];
    let image = tmp.join("brick.png");
    image::RgbaImage::from_raw(2, 2, px.to_vec())
      .unwrap()
      .save(&image)
      .unwrap();
    let ti = texture_from_image(&mut s, &image).unwrap();
    let again = texture_from_image(&mut s, &image).unwrap();
    s.begin_frame();
    s.cache();
    let mut change = NodeChange::new(&s);
    change.save(&s, ia);
    change.save(&s, ib);
    let changed = edit_material(&mut s, &[ia, ib], grid, |m| m.texture = ti);
    // what is drawn: the new meshes, the node set being the same
    s.begin_frame();
    let (mut drawn_new, mut drawn_old) = (false, false);
    for (mesh, idx) in &s.cache().mesh_groups {
      drawn_new |= *mesh == mesh_key(&s.find(ia).unwrap().mesh) && idx.len() == 2;
      drawn_old |= *mesh == mesh_key(&s.find(ic).unwrap().mesh) && idx.len() == 1;
    }
    assert!(
      drawn_new && drawn_old,
      "scene: a node given another mesh is drawn with it"
    );
    let entry = change.commit(&s, "texture", None);
    let material_of =
      |s: &Scene, id: u32| s.find(id).unwrap().mesh.as_ref().unwrap().prims[0].material;
    let textured = material_of(&s, ia);
    let applied = changed == 2
      && Arc::ptr_eq(
        s.find(ia).unwrap().mesh.as_ref().unwrap(),
        s.find(ib).unwrap().mesh.as_ref().unwrap(),
      )
      && s.materials[textured as usize].texture == ti
      && material_of(&s, ic) == grid;
    (entry.undo)(&mut s);
    let undone = material_of(&s, ia) == grid;
    (entry.redo)(&mut s);
    assert!(
      ti >= 0
        && ti == again
        && s.textures[ti as usize].name == "brick"
        && s.textures[ti as usize].width == 2
        && applied
        && undone
        && shape_default_material(&mut s) == grid
        && texture_from_image(&mut s, tmp.join("none.png")).is_err(),
      "materials: an image once as a texture, on the pieces chosen only, undone"
    );
    let bigger = ShapeParams {
      size: v3(10.0, 1.0, 10.0),
      ..bx
    };
    reshape(&mut s, ia, &bigger);
    let read = node_shape(s.find(ia).unwrap());
    let bounds = |s: &Scene, id: u32| s.find(id).unwrap().mesh.as_ref().unwrap().bounds();
    assert!(
      (bounds(&s, ia).hi.x - 5.0).abs() < 1e-4
        && material_of(&s, ia) == textured
        && read.is_some_and(|r| r.size.x == 10.0)
        && bounds(&s, ib).hi.x == 2.0,
      "shapes: resized alone, its texture kept"
    );
    let _ = std::fs::remove_dir_all(&tmp);
  }

  #[test]
  fn bent_round_and_bumpy_shapes() {
    // shapes: a wall bent a quarter turn ends turned 90 degrees; rounded corners; a ground's bumps
    let wall = ShapeParams {
      size: v3(8.0, 4.0, 0.5),
      bend: 90.0,
      ..Default::default()
    };
    let bent = make_shape(&wall, -1);
    let (mut min_x, mut max_x, mut max_z) = (1e9f32, -1e9f32, -1e9f32);
    for v in &bent.prims[0].pos {
      min_x = min_x.min(v.x);
      max_x = max_x.max(v.x);
      max_z = max_z.max(v.z);
    }
    let r = 8.0 / radians(90.0); // the arc's radius
    let pillar = ShapeParams {
      size: v3(2.0, 4.0, 2.0),
      round: 1.0,
      ..Default::default()
    };
    let round = make_shape(&pillar, -1);
    // the farthest point from the axis: 1 when it is round
    let corner = round.prims[0]
      .pos
      .iter()
      .map(|v| (v.x * v.x + v.z * v.z).sqrt())
      .fold(0f32, f32::max);
    let mut ground = shape_preset("ground").unwrap().params.clone();
    ground.bumps = 0.5;
    ground.color = Vec4::new(0.2, 0.3, 0.4, 1.0);
    let g = make_shape(&ground, -1);
    let (mut lo, mut hi) = (1e9f32, -1e9f32);
    for v in g.prims[0].pos.iter().filter(|v| v.y > 0.01) {
      lo = lo.min(v.y);
      hi = hi.max(v.y);
    }
    let back = ShapeParams::from_json(&ground.to_json());
    let s45 = radians(45.0).sin();
    assert!(
      bent.prims[0].pos.len() > 100
        && (max_x - r * s45 - 0.25 * s45).abs() < 0.3
        && (min_x + max_x).abs() < 0.01
        && max_z > 1.0
        && corner < 1.01
        && corner > 0.99
        && hi - lo > 0.2
        && hi - lo < 1.5
        && g.prims[0].col.len() == g.prims[0].pos.len()
        && back.bumps == 0.5
        && back.color.x == 0.2
        && ShapeParams::from_json(&wall.to_json()).bend == 90.0,
      "shapes: a wall bent 90 degrees along an arc, a pillar with round corners, a ground with its bumps and light"
    );
  }

  #[test]
  fn objects_without_a_model() {
    // the catalog's objects without a model: a picture of what they do
    let vent = object_icon("industrial-board-vent", "movement", 0);
    let gate = object_icon("sewer-elec-gate", "hazard", 0);
    let any = object_icon("some-type", "other", 0);
    assert!(
      vent.prims.len() == 3
        && gate.prims.len() == 2
        && !any.prims.is_empty()
        && !vent.prims[0].col.is_empty(),
      "catalog: an object without a model drawn as what it does (a vent blowing up, an electric gate)"
    );
    // a preview's lightning crackles: other zigzags for another seed, the same for the same
    let bolt = |seed| {
      object_icon("sewer-elec-gate", "hazard", seed).prims[1]
        .pos
        .clone()
    };
    assert!(bolt(1) == bolt(1) && bolt(1) != bolt(2));
  }

  #[test]
  fn noise_is_smooth_and_seeded() {
    // the same for the same seed, -0.5..0.5, continuous across the unit cells
    let mut other = false;
    for i in 0..200 {
      let (x, z) = (i as f32 * 0.37 - 30.0, i as f32 * 0.11 - 7.0);
      let n = smooth_noise(x, z, 3);
      assert!(n == smooth_noise(x, z, 3) && (-0.5..=0.5).contains(&n));
      other |= n != smooth_noise(x, z, 4);
      assert!((smooth_noise(x + 1e-3, z, 3) - n).abs() < 0.01);
    }
    assert!(other);
  }
}
