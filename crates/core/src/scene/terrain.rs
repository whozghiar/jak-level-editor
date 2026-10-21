//! A terrain drawn with the brush (Build panel > Terrain brush): a grid of square cells, the
//! ground's height at each of their corners, which cells are ground and which are walls (a column
//! of the cell, its height above the ground), kept in its node's extras (og_terrain_grid) and its
//! mesh made again from them at each stroke. Each cell keeps the look it was painted with (a
//! level's style, scene/level_style): what is flat draws its floor material, the slopes steeper
//! than 45 degrees, the walls and the edges its wall material.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use super::shapes::smooth_noise;
use super::{Mesh, Node, NodeKind, Primitive, Scene};
use crate::math::{Vec2, Vec3, Vec4, cross, dot, length};

/// What the cells painted with a look are drawn with (TerrainGrid::Look)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainLook {
  /// in the scene; -1: the other one, or none
  pub floor_material: i32,
  pub wall_material: i32,
  /// meters per repeat of their textures
  pub floor_tile: f32,
  pub wall_tile: f32,
  /// vertex colors (a level's light); w 0: none
  pub color: Vec4,
}

impl Default for TerrainLook {
  fn default() -> Self {
    Self {
      floor_material: -1,
      wall_material: -1,
      floor_tile: 2.0,
      wall_tile: 2.0,
      color: Vec4::new(0.0, 0.0, 0.0, 0.0),
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainGrid {
  /// meters across a cell
  pub cell: f32,
  /// the first cell, counted from the node's origin (cell i covers x in [i, i + 1] * cell)
  pub x0: i32,
  pub z0: i32,
  /// cells along x and z
  pub w: i32,
  pub d: i32,
  /// (w + 1) * (d + 1) corners: the ground's height (meters)
  pub height: Vec<f32>,
  /// w * d: NONE, GROUND or WALL
  pub cells: Vec<u8>,
  /// w * d: a wall cell's height above its ground (meters)
  pub wall: Vec<f32>,
  /// the looks painted with (64 at most)
  pub looks: Vec<TerrainLook>,
  /// w * d: a cell's, in `looks`
  pub look: Vec<u8>,
}

impl Default for TerrainGrid {
  fn default() -> Self {
    Self {
      cell: 1.0,
      x0: 0,
      z0: 0,
      w: 0,
      d: 0,
      height: Vec::new(),
      cells: Vec::new(),
      wall: Vec::new(),
      looks: vec![TerrainLook::default()],
      look: Vec::new(),
    }
  }
}

fn num(j: &Value, key: &str) -> Option<f64> {
  j.get(key).and_then(Value::as_f64)
}

impl TerrainGrid {
  pub const NONE: u8 = 0;
  pub const GROUND: u8 = 1;
  pub const WALL: u8 = 2;

  pub fn empty(&self) -> bool {
    self.cells.iter().all(|&c| c == Self::NONE)
  }

  /// the index of cell (i, k) in `cells`, `wall`, `look` (inside the grid)
  pub fn cell_index(&self, i: i32, k: i32) -> usize {
    (k - self.z0) as usize * self.w as usize + (i - self.x0) as usize
  }

  /// the index of corner (i, k) in `height` (inside the grid)
  pub fn corner_index(&self, i: i32, k: i32) -> usize {
    (k - self.z0) as usize * (self.w + 1) as usize + (i - self.x0) as usize
  }

  /// a cell, NONE outside
  pub fn at(&self, i: i32, k: i32) -> u8 {
    let (i, k) = (i - self.x0, k - self.z0);
    if i < 0 || k < 0 || i >= self.w || k >= self.d {
      return Self::NONE;
    }
    self.cells[k as usize * self.w as usize + i as usize]
  }

  /// a corner's height, 0 outside
  pub fn corner(&self, i: i32, k: i32) -> f32 {
    let (i, k) = (i - self.x0, k - self.z0);
    if i < 0 || k < 0 || i > self.w || k > self.d {
      return 0.0;
    }
    self.height[k as usize * (self.w + 1) as usize + i as usize]
  }

  /// one of the 4 cells around it is ground or wall
  pub fn corner_on_ground(&self, i: i32, k: i32) -> bool {
    self.at(i - 1, k - 1) != 0
      || self.at(i, k - 1) != 0
      || self.at(i - 1, k) != 0
      || self.at(i, k) != 0
  }

  /// holds the cells [i0, i1] x [k0, k1]
  pub fn grow(&mut self, mut i0: i32, mut k0: i32, mut i1: i32, mut k1: i32) {
    let (x0, z0, w, d) = (self.x0, self.z0, self.w, self.d);
    if w > 0 && d > 0 {
      if i0 >= x0 && k0 >= z0 && i1 < x0 + w && k1 < z0 + d {
        return;
      }
      i0 = i0.min(x0);
      k0 = k0.min(z0);
      i1 = i1.max(x0 + w - 1);
      k1 = k1.max(z0 + d - 1);
    }
    let (nw, nd) = (i1 - i0 + 1, k1 - k0 + 1);
    let cells = nw as usize * nd as usize;
    let mut nh = vec![0f32; (nw + 1) as usize * (nd + 1) as usize];
    let mut nwall = vec![0f32; cells];
    let mut nc = vec![Self::NONE; cells];
    let mut nl = vec![0u8; cells];
    if w > 0 && d > 0 {
      for k in 0..=d {
        for i in 0..=w {
          nh[(k + z0 - k0) as usize * (nw + 1) as usize + (i + x0 - i0) as usize] =
            self.height[k as usize * (w + 1) as usize + i as usize];
        }
      }
      for k in 0..d {
        for i in 0..w {
          let to = (k + z0 - k0) as usize * nw as usize + (i + x0 - i0) as usize;
          let from = k as usize * w as usize + i as usize;
          nc[to] = self.cells[from];
          nl[to] = self.look[from];
          nwall[to] = self.wall[from];
        }
      }
    }
    self.x0 = i0;
    self.z0 = k0;
    self.w = nw;
    self.d = nd;
    self.height = nh;
    self.cells = nc;
    self.look = nl;
    self.wall = nwall;
  }

  /// its index in `looks` (added when new)
  pub fn add_look(&mut self, l: &TerrainLook) -> i32 {
    if let Some(i) = self.looks.iter().position(|o| o == l) {
      return i as i32;
    }
    if self.looks.len() >= 64 {
      return 0;
    }
    self.looks.push(*l);
    self.looks.len() as i32 - 1
  }

  pub fn to_json(&self) -> Value {
    // heights in centimeters, the cells one digit each (their looks one character each), the walls
    // [cell, centimeters]
    let h: Vec<i32> = self
      .height
      .iter()
      .map(|v| (v * 100.0).round() as i32)
      .collect();
    let c: String = self.cells.iter().map(|&c| (b'0' + c) as char).collect();
    let lk: String = self.look.iter().map(|&l| (b'0' + l) as char).collect();
    let walls: Vec<Value> = (0..self.cells.len())
      .filter(|&i| self.cells[i] == Self::WALL)
      .map(|i| json!([i, (self.wall[i] * 100.0).round() as i32]))
      .collect();
    let jl: Vec<Value> = self
      .looks
      .iter()
      .map(|l| {
        json!({
          "floor_material": l.floor_material,
          "wall_material": l.wall_material,
          "floor_tile": l.floor_tile,
          "wall_tile": l.wall_tile,
          "color": [l.color.x, l.color.y, l.color.z, l.color.w],
        })
      })
      .collect();
    json!({
      "cell": self.cell,
      "x0": self.x0,
      "z0": self.z0,
      "w": self.w,
      "d": self.d,
      "heights": h,
      "cells": c,
      "walls": walls,
      "looks": jl,
      "look": lk,
    })
  }

  pub fn from_json(j: &Value) -> Option<TerrainGrid> {
    if !j.is_object() {
      return None;
    }
    let int = |k: &str| num(j, k).map_or(0, |v| v as i32);
    let mut g = TerrainGrid {
      cell: (num(j, "cell").unwrap_or(1.0) as f32).clamp(0.25, 8.0),
      x0: int("x0"),
      z0: int("z0"),
      w: int("w"),
      d: int("d"),
      ..Default::default()
    };
    if g.w < 0 || g.d < 0 || g.w > 4096 || g.d > 4096 {
      return None;
    }
    if g.w == 0 || g.d == 0 {
      // nothing painted yet
      g.w = 0;
      g.d = 0;
    }
    let empty = Vec::new();
    let h = j.get("heights").map_or(Some(&empty), Value::as_array)?;
    let c = j
      .get("cells")
      .and_then(Value::as_str)
      .unwrap_or("")
      .as_bytes();
    let corners = if g.w != 0 {
      (g.w + 1) as usize * (g.d + 1) as usize
    } else {
      0
    };
    if h.len() != corners || c.len() != g.w as usize * g.d as usize {
      return None;
    }
    g.height = h
      .iter()
      .map(|v| v.as_f64().map_or(0.0, |v| v as f32 / 100.0))
      .collect();
    g.cells = c
      .iter()
      .map(|&b| (b as i32 - b'0' as i32).clamp(0, 2) as u8)
      .collect();
    g.wall = vec![0.0; c.len()];
    for wv in j.get("walls").and_then(Value::as_array).unwrap_or(&empty) {
      if let Some([at, cm]) = wv.as_array().map(Vec::as_slice)
        && let Some(at) = at.as_u64()
        && (at as usize) < g.wall.len()
      {
        g.wall[at as usize] = cm.as_f64().unwrap_or(0.0) as f32 / 100.0;
      }
    }
    // the looks (a terrain of before them: one, its fields beside the grid's)
    let read_look = |o: &Value| {
      let mut l = TerrainLook {
        floor_material: num(o, "floor_material").map_or(-1, |v| v as i32),
        wall_material: num(o, "wall_material").map_or(-1, |v| v as i32),
        floor_tile: (num(o, "floor_tile").unwrap_or(2.0) as f32).max(0.05),
        wall_tile: (num(o, "wall_tile").unwrap_or(2.0) as f32).max(0.05),
        ..Default::default()
      };
      if let Some(col) = o.get("color").and_then(Value::as_array)
        && col.len() == 4
      {
        let f = |i: usize| col[i].as_f64().unwrap_or(0.0) as f32;
        l.color = Vec4::new(f(0), f(1), f(2), f(3));
      }
      l
    };
    g.looks.clear();
    for o in j.get("looks").and_then(Value::as_array).unwrap_or(&empty) {
      if o.is_object() && g.looks.len() < 64 {
        g.looks.push(read_look(o));
      }
    }
    if g.looks.is_empty() {
      g.looks.push(read_look(j));
    }
    let lk = j
      .get("look")
      .and_then(Value::as_str)
      .unwrap_or("")
      .as_bytes();
    g.look = vec![0; c.len()];
    if lk.len() == c.len() {
      let last = g.looks.len() as i32 - 1;
      for (to, &b) in g.look.iter_mut().zip(lk) {
        *to = (b as i32 - b'0' as i32).clamp(0, last) as u8;
      }
    }
    Some(g)
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrushMode {
  #[default]
  Paint,
  Erase,
  Raise,
  Dig,
  Smooth,
  Flatten,
  Wall,
  Unwall,
}

/// One application of the square brush, in the terrain's node space
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushStroke {
  pub mode: BrushMode,
  pub center: Vec3,
  /// meters: the square's side
  pub size: f32,
  /// raise, dig: meters at its middle; smooth, flatten: 0..1 of the way; wall: its height above
  /// the ground
  pub amount: f32,
  /// paint, wall, flatten: the ground's height (where the stroke started)
  pub height: f32,
  /// paint: the relief of the new ground (a level's: its bumps up and down, their size)
  pub bumps: f32,
  pub bump_size: f32,
  pub seed: i32,
  /// paint, wall: the look given to the cells (in the grid's `looks`)
  pub look: i32,
  /// raise, dig: ground laid first where there is none under its middle
  pub lay: bool,
}

impl Default for BrushStroke {
  fn default() -> Self {
    Self {
      mode: BrushMode::Paint,
      center: Vec3::default(),
      size: 4.0,
      amount: 0.0,
      height: 0.0,
      bumps: 0.0,
      bump_size: 3.0,
      seed: 1,
      look: 0,
      lay: true,
    }
  }
}

fn smoothstep01(t: f32) -> f32 {
  let t = t.clamp(0.0, 1.0);
  t * t * (3.0 - 2.0 * t)
}

/// the brush's weight at (dx, dz) from its middle: 1 there, fading to 0 at the square's edge along
/// a cosine (a hill, a hollow, not a step)
fn weight(dx: f32, dz: f32, half: f32) -> f32 {
  let t = (dx.abs().max(dz.abs()) / half.max(1e-3)).min(1.0);
  0.5 + 0.5 * (t * std::f32::consts::PI).cos()
}

/// Applies the brush: the cells whose middle is in the square, the corners in it (for raise, dig,
/// smooth and flatten the most in its middle, fading to its edges; raise and dig lay ground first
/// where there is none under its middle). Paint over ground gives it the stroke's look. Returns
/// whether the terrain changed.
pub fn apply_brush(g: &mut TerrainGrid, s: &BrushStroke) -> bool {
  let c = g.cell;
  let half = (s.size * 0.5).max(c * 0.5);
  let m = s.center;
  // the cells whose middle is in the square (the one under the brush at least), the corners in it
  let mut ci0 = ((m.x - half) / c - 0.5).ceil() as i32;
  let mut ci1 = ((m.x + half) / c - 0.5).floor() as i32;
  let mut ck0 = ((m.z - half) / c - 0.5).ceil() as i32;
  let mut ck1 = ((m.z + half) / c - 0.5).floor() as i32;
  if ci0 > ci1 {
    ci0 = (m.x / c).floor() as i32;
    ci1 = ci0;
  }
  if ck0 > ck1 {
    ck0 = (m.z / c).floor() as i32;
    ck1 = ck0;
  }
  let pi0 = ((m.x - half) / c).ceil() as i32;
  let pi1 = ((m.x + half) / c).floor() as i32;
  let pk0 = ((m.z - half) / c).ceil() as i32;
  let pk1 = ((m.z + half) / c).floor() as i32;
  let mut changed = false;
  if (s.mode == BrushMode::Raise || s.mode == BrushMode::Dig)
    && s.lay
    && g.at((m.x / c).floor() as i32, (m.z / c).floor() as i32) == TerrainGrid::NONE
  {
    let ground = BrushStroke {
      mode: BrushMode::Paint,
      ..*s
    };
    changed |= apply_brush(g, &ground);
  }
  match s.mode {
    BrushMode::Paint | BrushMode::Wall => {
      g.grow(ci0, ck0, ci1, ck1);
      // the corners of new ground: at the stroke's height, with the relief
      for k in ck0..=ck1 + 1 {
        for i in ci0..=ci1 + 1 {
          if g.corner_on_ground(i, k) {
            continue;
          }
          let mut y = s.height;
          if s.mode == BrushMode::Paint && s.bumps > 0.0 {
            let u = i as f32 * c / s.bump_size;
            let v = k as f32 * c / s.bump_size;
            y += s.bumps
              * (smooth_noise(u, v, s.seed)
                + 0.35 * smooth_noise(u * 2.3 + 17.0, v * 2.3 + 5.0, s.seed));
          }
          let at = g.corner_index(i, k);
          g.height[at] = y;
          changed = true;
        }
      }
      for k in ck0..=ck1 {
        for i in ci0..=ci1 {
          let at = g.cell_index(i, k);
          let t = g.cells[at];
          if g.look[at] != s.look as u8 && (t != TerrainGrid::NONE || s.mode == BrushMode::Wall) {
            changed = true;
          }
          g.look[at] = s.look as u8;
          if s.mode == BrushMode::Paint && t == TerrainGrid::NONE {
            g.cells[at] = TerrainGrid::GROUND;
            changed = true;
          } else if s.mode == BrushMode::Wall {
            if t != TerrainGrid::WALL || g.wall[at] != s.amount {
              changed = true;
            }
            g.cells[at] = TerrainGrid::WALL;
            g.wall[at] = s.amount;
          }
        }
      }
    }
    BrushMode::Erase | BrushMode::Unwall => {
      for k in ck0..=ck1 {
        for i in ci0..=ci1 {
          let t = g.at(i, k);
          if t == TerrainGrid::NONE || (s.mode == BrushMode::Unwall && t != TerrainGrid::WALL) {
            continue;
          }
          let at = g.cell_index(i, k);
          g.cells[at] = if s.mode == BrushMode::Erase {
            TerrainGrid::NONE
          } else {
            TerrainGrid::GROUND
          };
          changed = true;
        }
      }
    }
    BrushMode::Raise | BrushMode::Dig | BrushMode::Flatten | BrushMode::Smooth => {
      let before = g.height.clone(); // smoothing reads the heights before this step
      let old = |g: &TerrainGrid, i: i32, k: i32| before[g.corner_index(i, k)];
      for k in pk0.max(g.z0)..=pk1.min(g.z0 + g.d) {
        for i in pi0.max(g.x0)..=pi1.min(g.x0 + g.w) {
          if !g.corner_on_ground(i, k) {
            continue;
          }
          let wgt = weight(i as f32 * c - m.x, k as f32 * c - m.z, half);
          if wgt <= 0.0 {
            continue;
          }
          let at = g.corner_index(i, k);
          let was = g.height[at];
          let mut h = was;
          match s.mode {
            BrushMode::Raise => h += s.amount * wgt,
            BrushMode::Dig => h -= s.amount * wgt,
            BrushMode::Flatten => h += (s.height - h) * (s.amount * wgt).clamp(0.0, 1.0),
            _ => {
              let mut sum = 0.0;
              let mut n = 0;
              for (di, dk) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (ni, nk) = (i + di, k + dk);
                if ni < g.x0
                  || nk < g.z0
                  || ni > g.x0 + g.w
                  || nk > g.z0 + g.d
                  || !g.corner_on_ground(ni, nk)
                {
                  continue;
                }
                sum += old(g, ni, nk);
                n += 1;
              }
              if n != 0 {
                h += (sum / n as f32 - old(g, i, k)) * (s.amount * wgt).clamp(0.0, 1.0);
              }
            }
          }
          g.height[at] = h;
          changed |= h != was;
        }
      }
    }
  }
  changed
}

/// A cell of [i0, i1] x [k0, k1] that a change from `before` to `after` changed: laid, raised or
/// dug, walled (drawn from `after`), or taken away (`removed`: drawn from `before`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainChange {
  pub i: i32,
  pub k: i32,
  pub removed: bool,
}

pub fn terrain_changes(
  before: &TerrainGrid,
  after: &TerrainGrid,
  i0: i32,
  k0: i32,
  i1: i32,
  k1: i32,
) -> Vec<TerrainChange> {
  let mut out = Vec::new();
  let wall = |g: &TerrainGrid, i: i32, k: i32| g.wall[g.cell_index(i, k)];
  for k in k0..=k1 {
    for i in i0..=i1 {
      let (a, b) = (before.at(i, k), after.at(i, k));
      if b == TerrainGrid::NONE {
        if a != TerrainGrid::NONE {
          out.push(TerrainChange {
            i,
            k,
            removed: true,
          });
        }
        continue;
      }
      let mut changed =
        a != b || (b == TerrainGrid::WALL && wall(before, i, k) != wall(after, i, k));
      for dk in 0..=1 {
        if changed {
          break;
        }
        for di in 0..=1 {
          changed |= (before.corner(i + di, k + dk) - after.corner(i + di, k + dk)).abs() > 1e-3;
        }
      }
      if changed {
        out.push(TerrainChange {
          i,
          k,
          removed: false,
        });
      }
    }
  }
  out
}

/// Hills over a rectangle around `center` (x, z: `size` meters), made by a fractal noise: `hills`
/// meters high at most, `roughness` 0..1 (smooth slopes to broken ground); its new ground painted
/// with `look` (0 by default)
pub fn generate_terrain(
  g: &mut TerrainGrid,
  center: Vec3,
  size: Vec2,
  hills: f32,
  roughness: f32,
  seed: i32,
  look: i32,
) {
  let c = g.cell;
  let i0 = ((center.x - size.x * 0.5) / c).floor() as i32;
  let i1 = ((center.x + size.x * 0.5) / c).ceil() as i32 - 1;
  let k0 = ((center.z - size.y * 0.5) / c).floor() as i32;
  let k1 = ((center.z + size.y * 0.5) / c).ceil() as i32 - 1;
  if i1 < i0 || k1 < k0 {
    return;
  }
  g.grow(i0, k0, i1, k1);
  // a few hills across, finer and finer octaves (more of them the rougher)
  let scale = (size.x.max(size.y) / 3.0).max(4.0);
  let persistence = 0.3 + 0.35 * roughness.clamp(0.0, 1.0);
  for k in k0..=k1 + 1 {
    for i in i0..=i1 + 1 {
      let (x, z) = (i as f32 * c, k as f32 * c);
      let (mut sum, mut amp, mut total, mut freq) = (0.0, 1.0, 0.0, 1.0 / scale);
      for o in 0..5 {
        let of = o as f32;
        sum += amp * smooth_noise(x * freq + of * 31.7, z * freq - of * 17.3, seed + o);
        total += amp;
        amp *= persistence;
        freq *= 2.0;
      }
      // 0 at its border, to meet the flat ground around it
      let bx = (i - i0).min(i1 + 1 - i) as f32 / 1f32.max((i1 + 1 - i0) as f32 * 0.15);
      let bz = (k - k0).min(k1 + 1 - k) as f32 / 1f32.max((k1 + 1 - k0) as f32 * 0.15);
      let edge = smoothstep01(bx.min(bz));
      let at = g.corner_index(i, k);
      g.height[at] = center.y + hills * (sum / total + 0.5) * edge;
    }
  }
  for k in k0..=k1 {
    for i in i0..=i1 {
      let at = g.cell_index(i, k);
      if g.cells[at] == TerrainGrid::NONE {
        g.cells[at] = TerrainGrid::GROUND;
        g.look[at] = look as u8;
      }
    }
  }
}

/// The magnet brush: the top of the decor nearest to `at` (world space) beside it, not under it: of
/// the surfaces facing up of the visible pieces (not props: 4 m2 of them at least; `skip` left out:
/// the terrain painted), within `reach` meters across and `window` meters up or down, the one whose
/// edge is nearest. The point of that edge nearest to `at`, at the decor's height there, and the
/// node; None when there is none (or only decor under `at`).
pub fn nearest_decor_edge(
  scene: &Scene,
  at: Vec3,
  reach: f32,
  window: f32,
  skip: u32,
) -> Option<(Vec3, u32)> {
  let cache = scene.cache();
  let mut best = f32::MAX;
  let mut found = None;
  for (i, n) in scene.nodes.iter().enumerate().take(cache.bounds.len()) {
    let Some(mesh) = &n.mesh else { continue };
    if n.kind != NodeKind::Render || n.id == skip || !cache.visible[i] || cache.actor[i] != 0 {
      continue;
    }
    let b = &cache.bounds[i];
    if !b.valid()
      || b.lo.x > at.x + reach
      || b.hi.x < at.x - reach
      || b.lo.z > at.z + reach
      || b.hi.z < at.z - reach
      || b.lo.y > at.y + window
      || b.hi.y < at.y - window
    {
      continue;
    }
    // of this piece: the point of its top nearest to `at` across; none when `at` is over it
    let mut near = f32::MAX;
    let mut top = 0.0; // its surface facing up (m2)
    let mut point = Vec3::default();
    let mut under = false;
    let w = &cache.worlds[i];
    'prims: for p in &mesh.prims {
      for t in p.idx.chunks_exact(3) {
        let a = w.point(p.pos[t[0] as usize]);
        let bb = w.point(p.pos[t[1] as usize]);
        let c = w.point(p.pos[t[2] as usize]);
        let nrm = cross(bb - a, c - a);
        let len = length(nrm);
        if len < 1e-6 || nrm.y.abs() < 0.7 * len {
          continue; // walls and slopes are no top
        }
        if a.y.max(bb.y).max(c.y) < at.y - window || a.y.min(bb.y).min(c.y) > at.y + window {
          continue;
        }
        top += len * 0.5;
        if a.x.min(bb.x).min(c.x) > at.x + reach
          || a.x.max(bb.x).max(c.x) < at.x - reach
          || a.z.min(bb.z).min(c.z) > at.z + reach
          || a.z.max(bb.z).max(c.z) < at.z - reach
        {
          continue;
        }
        // the nearest point of the triangle across (x, z), and its height there
        let d = (bb.x - a.x) * (c.z - a.z) - (c.x - a.x) * (bb.z - a.z);
        let height = |x: f32, z: f32| {
          let u = ((bb.x - x) * (c.z - z) - (c.x - x) * (bb.z - z)) / d;
          let v = ((c.x - x) * (a.z - z) - (a.x - x) * (c.z - z)) / d;
          u * a.y + v * bb.y + (1.0 - u - v) * c.y
        };
        let u = ((bb.x - at.x) * (c.z - at.z) - (c.x - at.x) * (bb.z - at.z)) / d;
        let v = ((c.x - at.x) * (a.z - at.z) - (a.x - at.x) * (c.z - at.z)) / d;
        if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
          under = true; // it is under (or over) the square's middle: not beside it
          break 'prims;
        }
        let tri = [a, bb, c];
        for e in 0..3 {
          let (p0, p1) = (tri[e], tri[(e + 1) % 3]);
          let (ex, ez) = (p1.x - p0.x, p1.z - p0.z);
          let l2 = ex * ex + ez * ez;
          let s = if l2 > 1e-12 {
            (((at.x - p0.x) * ex + (at.z - p0.z) * ez) / l2).clamp(0.0, 1.0)
          } else {
            0.0
          };
          let (qx, qz) = (p0.x + ex * s, p0.z + ez * s);
          let dist = (at.x - qx).hypot(at.z - qz);
          let y = if d.abs() > 1e-9 {
            height(qx, qz)
          } else {
            p0.y + (p1.y - p0.y) * s
          };
          // as near: the highest (a box's top, not its bottom)
          if dist < near - 1e-3 || (dist < near + 1e-3 && y > point.y) {
            near = near.min(dist);
            point = Vec3::new(qx, y, qz);
          }
        }
      }
    }
    // not a prop (a plate, a crate's lid): a top of 4 m2 at least (a box's bottom counted too)
    if under || near > reach || near >= best || top < 8.0 {
      continue;
    }
    best = near;
    found = Some((point, n.id));
  }
  found
}

/// The ground of a terrain nearest to `at` (its space) within `reach` meters across: the middle of
/// that cell at the height of its ground (a wall's foot for a wall); None when there is none
pub fn terrain_level_near(g: &TerrainGrid, at: Vec3, reach: f32) -> Option<Vec3> {
  let c = g.cell;
  let mut best = f32::MAX;
  let mut point = None;
  let i0 = ((at.x - reach) / c).floor() as i32;
  let i1 = ((at.x + reach) / c).floor() as i32;
  let k0 = ((at.z - reach) / c).floor() as i32;
  let k1 = ((at.z + reach) / c).floor() as i32;
  for k in k0.max(g.z0)..=k1.min(g.z0 + g.d - 1) {
    for i in i0.max(g.x0)..=i1.min(g.x0 + g.w - 1) {
      if g.at(i, k) == TerrainGrid::NONE {
        continue;
      }
      let (x, z) = ((i as f32 + 0.5) * c, (k as f32 + 0.5) * c);
      let d = (x - at.x).hypot(z - at.z);
      if d > reach || d >= best {
        continue;
      }
      best = d;
      let y =
        (g.corner(i, k) + g.corner(i + 1, k) + g.corner(i, k + 1) + g.corner(i + 1, k + 1)) * 0.25;
      point = Some(Vec3::new(x, y, z));
    }
  }
  point
}

/// The terrain's height at (x, z) (bilinear over its cell); None where there is no ground
pub fn terrain_height(g: &TerrainGrid, x: f32, z: f32) -> Option<f32> {
  let (fi, fk) = (x / g.cell, z / g.cell);
  let (i, k) = (fi.floor() as i32, fk.floor() as i32);
  if g.at(i, k) == TerrainGrid::NONE {
    return None;
  }
  let (u, v) = (fi - i as f32, fk - k as f32);
  let a = g.corner(i, k) + (g.corner(i + 1, k) - g.corner(i, k)) * u;
  let b = g.corner(i, k + 1) + (g.corner(i + 1, k + 1) - g.corner(i, k + 1)) * u;
  Some(a + (b - a) * v)
}

/// a triangle pair a b c d (a quad: a and d opposite), facing `out`
fn add_quad(p: &mut Primitive, a: u32, mut b: u32, mut c: u32, d: u32, out: Vec3) {
  let (pa, pb, pc) = (p.pos[a as usize], p.pos[b as usize], p.pos[c as usize]);
  if dot(cross(pb - pa, pc - pa), out) < 0.0 {
    std::mem::swap(&mut b, &mut c);
  }
  p.idx.extend([a, b, c, c, b, d]);
}

/// a triangle a b c, facing `out`
fn add_tri(p: &mut Primitive, a: u32, mut b: u32, mut c: u32, out: Vec3) {
  let (pa, pb, pc) = (p.pos[a as usize], p.pos[b as usize], p.pos[c as usize]);
  if dot(cross(pb - pa, pc - pa), out) < 0.0 {
    std::mem::swap(&mut b, &mut c);
  }
  p.idx.extend([a, b, c]);
}

fn add_vertex(p: &mut Primitive, pos: Vec3, uv: Vec2) -> u32 {
  p.pos.push(pos);
  p.uv.push(uv);
  p.pos.len() as u32 - 1
}

/// an upright face from (a, b) at the bottom to (a, b) at the top, its texture along it
fn upright(steep: &mut Primitive, wt: f32, a: Vec3, b: Vec3, a_top: f32, b_top: f32, out: Vec3) {
  let along_a = if out.x.abs() > 0.5 { a.z } else { a.x };
  let along_b = if out.x.abs() > 0.5 { b.z } else { b.x };
  let v0 = add_vertex(steep, a, Vec2::new(along_a / wt, a.y / wt));
  let v1 = add_vertex(steep, b, Vec2::new(along_b / wt, b.y / wt));
  let v2 = add_vertex(
    steep,
    Vec3::new(a.x, a_top, a.z),
    Vec2::new(along_a / wt, a_top / wt),
  );
  let v3 = add_vertex(
    steep,
    Vec3::new(b.x, b_top, b.z),
    Vec2::new(along_b / wt, b_top / wt),
  );
  add_quad(steep, v0, v1, v2, v3, out);
}

/// Its mesh: the floor, the slopes and the walls, the sides of its edges down below its lowest
/// point
pub fn make_terrain_mesh(g: &TerrainGrid) -> Arc<Mesh> {
  let mut mesh = Mesh::new();
  mesh.name = "terrain".into();
  let c = g.cell;
  // per look: its flat ground, its slopes and walls
  let mut flats: Vec<Primitive> = Vec::with_capacity(g.looks.len());
  let mut steeps: Vec<Primitive> = Vec::with_capacity(g.looks.len());
  for lk in &g.looks {
    flats.push(Primitive {
      material: if lk.floor_material >= 0 {
        lk.floor_material
      } else {
        lk.wall_material
      },
      ..Default::default()
    });
    steeps.push(Primitive {
      material: if lk.wall_material >= 0 {
        lk.wall_material
      } else {
        lk.floor_material
      },
      ..Default::default()
    });
  }
  // the lowest of the ground: the edges go down below it
  let mut lowest = f32::MAX;
  for k in g.z0..=g.z0 + g.d {
    for i in g.x0..=g.x0 + g.w {
      if g.corner_on_ground(i, k) {
        lowest = lowest.min(g.corner(i, k));
      }
    }
  }
  if lowest == f32::MAX {
    return Arc::new(mesh);
  }
  let bottom = lowest - 1.0;
  // the ground smoothed over about 3 m (the cells within; at least one each side): what tells a
  // steep slope from a bump
  let r = ((1.5 / c).round() as i32).max(1);
  let mut smoothed = vec![0f32; (g.w + 1) as usize * (g.d + 1) as usize];
  for k in g.z0..=g.z0 + g.d {
    for i in g.x0..=g.x0 + g.w {
      let mut sum = 0.0;
      let mut n = 0;
      for dk in -r..=r {
        for di in -r..=r {
          let (ni, nk) = (i + di, k + dk);
          if ni < g.x0
            || nk < g.z0
            || ni > g.x0 + g.w
            || nk > g.z0 + g.d
            || !g.corner_on_ground(ni, nk)
          {
            continue;
          }
          sum += g.corner(ni, nk);
          n += 1;
        }
      }
      smoothed[g.corner_index(i, k)] = if n != 0 {
        sum / n as f32
      } else {
        g.corner(i, k)
      };
    }
  }
  let smooth_corner = |i: i32, k: i32| smoothed[g.corner_index(i, k)];
  // the flat ground of a look shares its corners (smooth light over it)
  let mut shared: Vec<HashMap<u64, u32>> = vec![HashMap::new(); g.looks.len()];
  for k in g.z0..g.z0 + g.d {
    for i in g.x0..g.x0 + g.w {
      let t = g.at(i, k);
      if t == TerrainGrid::NONE {
        continue;
      }
      let cur = g.look[g.cell_index(i, k)] as usize;
      let flat = &mut flats[cur];
      let steep = &mut steeps[cur];
      let shared = &mut shared[cur];
      let ft = g.looks[cur].floor_tile;
      let wt = g.looks[cur].wall_tile;
      let mut flat_corner = |flat: &mut Primitive, i: i32, k: i32| {
        let key = ((i as u32 as u64) << 32) | k as u32 as u64;
        *shared.entry(key).or_insert_with(|| {
          let p = Vec3::new(i as f32 * c, g.corner(i, k), k as f32 * c);
          add_vertex(flat, p, Vec2::new(p.x / ft, p.z / ft))
        })
      };
      let (h00, h10, h01, h11) = (
        g.corner(i, k),
        g.corner(i + 1, k),
        g.corner(i, k + 1),
        g.corner(i + 1, k + 1),
      );
      let (x, z) = (i as f32 * c, k as f32 * c);
      // the sides of each edge: down below the ground where it ends, from the ground up a wall
      struct Side {
        di: i32,
        dk: i32,
        a: Vec3,
        b: Vec3,
        ha: f32,
        hb: f32,
        out: Vec3,
      }
      #[rustfmt::skip]
      let sides = [
        Side { di: -1, dk: 0, a: Vec3::new(x, 0.0, z), b: Vec3::new(x, 0.0, z + c), ha: h00, hb: h01, out: Vec3::new(-1.0, 0.0, 0.0) },
        Side { di: 1, dk: 0, a: Vec3::new(x + c, 0.0, z), b: Vec3::new(x + c, 0.0, z + c), ha: h10, hb: h11, out: Vec3::new(1.0, 0.0, 0.0) },
        Side { di: 0, dk: -1, a: Vec3::new(x, 0.0, z), b: Vec3::new(x + c, 0.0, z), ha: h00, hb: h10, out: Vec3::new(0.0, 0.0, -1.0) },
        Side { di: 0, dk: 1, a: Vec3::new(x, 0.0, z + c), b: Vec3::new(x + c, 0.0, z + c), ha: h01, hb: h11, out: Vec3::new(0.0, 0.0, 1.0) },
      ];
      let wall_top = |i: i32, k: i32| {
        g.corner(i, k)
          .max(g.corner(i + 1, k))
          .max(g.corner(i, k + 1))
          .max(g.corner(i + 1, k + 1))
          + g.wall[g.cell_index(i, k)]
      };
      if t == TerrainGrid::WALL {
        // a column: from under the ground to its top, flat
        let top = wall_top(i, k);
        let base = h00.min(h10).min(h01).min(h11) - 0.3;
        let v0 = add_vertex(steep, Vec3::new(x, top, z), Vec2::new(x / wt, z / wt));
        let v1 = add_vertex(
          steep,
          Vec3::new(x + c, top, z),
          Vec2::new((x + c) / wt, z / wt),
        );
        let v2 = add_vertex(
          steep,
          Vec3::new(x, top, z + c),
          Vec2::new(x / wt, (z + c) / wt),
        );
        let v3 = add_vertex(
          steep,
          Vec3::new(x + c, top, z + c),
          Vec2::new((x + c) / wt, (z + c) / wt),
        );
        add_quad(steep, v0, v1, v2, v3, Vec3::new(0.0, 1.0, 0.0));
        for s in &sides {
          let mut from = base;
          if g.at(i + s.di, k + s.dk) == TerrainGrid::WALL {
            let ntop = wall_top(i + s.di, k + s.dk);
            if ntop >= top {
              continue;
            }
            from = ntop;
          }
          upright(
            steep,
            wt,
            Vec3::new(s.a.x, from, s.a.z),
            Vec3::new(s.b.x, from, s.b.z),
            top,
            top,
            s.out,
          );
        }
        continue;
      }
      // its two triangles, each with the floor texture or, steeper than 45 degrees, the wall
      // texture along its slope (rock showing where the ground is steep, its edges diagonal, not
      // square cell by cell)
      const TRI_CORNERS: [[[i32; 2]; 3]; 2] = [[[0, 0], [0, 1], [1, 0]], [[1, 0], [0, 1], [1, 1]]];
      for t in &TRI_CORNERS {
        let mut p3 = [Vec3::default(); 3];
        let mut q3 = [Vec3::default(); 3];
        for v in 0..3 {
          let (ci, ck) = (i + t[v][0], k + t[v][1]);
          p3[v] = Vec3::new(ci as f32 * c, g.corner(ci, ck), ck as f32 * c);
          q3[v] = Vec3::new(ci as f32 * c, smooth_corner(ci, ck), ck as f32 * c);
        }
        // its slope on the ground smoothed over a few meters: the rock of the broad steep slopes,
        // not of every small bump of a rough ground
        let n = cross(q3[1] - q3[0], q3[2] - q3[0]);
        let len = length(n);
        #[allow(clippy::approx_constant)] // the C++'s threshold: a slope of 45 degrees is a floor
        let is_steep = len > 0.0 && n.y.abs() < 0.7071 * len;
        let mut v = [0u32; 3];
        if !is_steep {
          for q in 0..3 {
            v[q] = flat_corner(flat, i + t[q][0], k + t[q][1]);
          }
          add_tri(flat, v[0], v[1], v[2], Vec3::new(0.0, 1.0, 0.0));
        } else {
          // its texture along the slope, not stretched: projected on the side it faces most
          let along_z = n.x.abs() > n.z.abs();
          for q in 0..3 {
            let uv = if along_z {
              Vec2::new(p3[q].z / wt, p3[q].y / wt)
            } else {
              Vec2::new(p3[q].x / wt, p3[q].y / wt)
            };
            v[q] = add_vertex(steep, p3[q], uv);
          }
          add_tri(steep, v[0], v[1], v[2], Vec3::new(0.0, 1.0, 0.0));
        }
      }
      for s in &sides {
        if g.at(i + s.di, k + s.dk) == TerrainGrid::NONE {
          upright(
            steep,
            wt,
            Vec3::new(s.a.x, bottom, s.a.z),
            Vec3::new(s.b.x, bottom, s.b.z),
            s.ha,
            s.hb,
            s.out,
          );
        }
      }
    }
  }
  for (l, (flat, steep)) in flats.into_iter().zip(steeps).enumerate() {
    for mut p in [flat, steep] {
      if p.idx.is_empty() {
        continue;
      }
      p.compute_normals();
      if g.looks[l].color.w > 0.0 {
        p.col = vec![g.looks[l].color; p.pos.len()];
      }
      mesh.prims.push(p);
    }
  }
  Arc::new(mesh)
}

/// A terrain node's grid, when it is one
pub fn node_terrain(n: &Node) -> Option<TerrainGrid> {
  n.extras
    .get("og_terrain_grid")
    .and_then(TerrainGrid::from_json)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn stroke(mode: BrushMode, center: Vec3, size: f32, amount: f32, height: f32) -> BrushStroke {
    BrushStroke {
      mode,
      center,
      size,
      amount,
      height,
      ..Default::default()
    }
  }

  #[test]
  fn brush_paint_raise_dig_walls_looks_hills() {
    // the terrain brush: ground painted at the stroke's height, raised and dug in its middle, walls
    // raised and taken away, the grid kept through its node's extras, hills generated
    let mut g = TerrainGrid::default();
    g.looks[0].floor_material = 0;
    g.looks[0].wall_material = 1;
    let origin = Vec3::default();
    let mut st = stroke(BrushMode::Paint, origin, 4.0, 0.0, 1.0);
    let painted = apply_brush(&mut g, &st);
    let cells = g
      .cells
      .iter()
      .filter(|&&c| c == TerrainGrid::GROUND)
      .count();
    st.mode = BrushMode::Raise;
    st.amount = 2.0;
    apply_brush(&mut g, &st);
    let (raised, edge) = (g.corner(0, 0), g.corner(-2, -2));
    st.mode = BrushMode::Dig;
    st.amount = 0.5;
    apply_brush(&mut g, &st);
    let middle = terrain_height(&g, 0.0, 0.0);
    let off = terrain_height(&g, 20.0, 0.0);
    let mut st = stroke(BrushMode::Wall, Vec3::new(10.0, 0.0, 0.0), 2.0, 3.0, 0.0);
    apply_brush(&mut g, &st);
    let walls = g.cells.iter().filter(|&&c| c == TerrainGrid::WALL).count();
    let mesh = make_terrain_mesh(&g);
    let top = mesh
      .prims
      .iter()
      .filter(|p| p.material == 1)
      .flat_map(|p| p.pos.iter().map(|v| v.y))
      .fold(-1e9f32, f32::max);
    let back = TerrainGrid::from_json(&g.to_json());
    let same = back.as_ref().is_some_and(|b| {
      b.cells == g.cells
        && b.height.len() == g.height.len()
        && b
          .height
          .iter()
          .zip(&g.height)
          .all(|(a, b)| (a - b).abs() < 0.006)
    });
    // another style painted over a part: its own floor and walls, kept in the node; raise where
    // there is no ground lays some
    let mut two = g.clone();
    let other = TerrainLook {
      floor_material: 2,
      wall_material: 3,
      ..Default::default()
    };
    let mut ps = stroke(BrushMode::Paint, origin, 2.0, 0.0, 1.0);
    ps.look = two.add_look(&other);
    let repainted = apply_brush(&mut two, &ps) && two.add_look(&other) == ps.look;
    let mesh_two = make_terrain_mesh(&two);
    let floor2 = mesh_two.prims.iter().any(|p| p.material == 2);
    let floor0 = mesh_two.prims.iter().any(|p| p.material == 0);
    let looks_kept = TerrainGrid::from_json(&two.to_json())
      .is_some_and(|b| b.look == two.look && b.looks.len() == 2 && b.looks[1].wall_material == 3);
    let rs = stroke(BrushMode::Raise, Vec3::new(30.0, 0.0, 30.0), 2.0, 1.0, 0.0);
    let raised_new = apply_brush(&mut two, &rs)
      && two.at(30, 30) == TerrainGrid::GROUND
      && two.corner(30, 30) > 0.9;
    st.mode = BrushMode::Erase;
    apply_brush(&mut g, &st);
    let walls_after = g.cells.iter().filter(|&&c| c == TerrainGrid::WALL).count();
    let mut hills = TerrainGrid::default();
    generate_terrain(
      &mut hills,
      Vec3::new(0.0, 5.0, 0.0),
      Vec2::new(32.0, 32.0),
      10.0,
      0.5,
      3,
      0,
    );
    let lo = hills.height.iter().copied().fold(1e9f32, f32::min);
    let hi = hills.height.iter().copied().fold(-1e9f32, f32::max);
    let border = (hills.corner(hills.x0, hills.z0) - 5.0).abs() < 0.01;
    assert!(
      painted
        && cells == 16
        && (raised - 3.0).abs() < 0.01
        && edge < raised
        && middle.is_some_and(|m| (m - 2.5).abs() < 0.01)
        && off.is_none()
        && walls == 4
        && (top - 3.0).abs() < 0.01
        && mesh.prims.len() == 2
        && same
        && walls_after == 0
        && lo >= 4.99
        && hi <= 15.01
        && hi - lo > 2.0
        && border
        && repainted
        && floor2
        && floor0
        && looks_kept
        && raised_new,
      "terrain brush: ground painted at the stroke's height, raised and dug in the square's middle, walls up and \
       away, kept in the node, hills generated meeting the ground around, another style painted over a part, \
       raise laying ground where there is none"
    );
    // the same seed, the same hills (the noise is the editor's own)
    let mut again = TerrainGrid::default();
    generate_terrain(
      &mut again,
      Vec3::new(0.0, 5.0, 0.0),
      Vec2::new(32.0, 32.0),
      10.0,
      0.5,
      3,
      0,
    );
    assert_eq!(again, hills);
  }

  #[test]
  fn brush_preview_changes() {
    // what a stamp would change, shown before it is done: the cells laid, raised, walled, taken away
    let none = TerrainGrid::default();
    let mut laid = none.clone();
    let mut stamp = stroke(BrushMode::Paint, Vec3::new(2.0, 0.0, 2.0), 4.0, 0.0, 0.0);
    apply_brush(&mut laid, &stamp);
    let (mut raised, mut walled, mut erased, mut repainted) =
      (laid.clone(), laid.clone(), laid.clone(), laid.clone());
    stamp.mode = BrushMode::Raise;
    stamp.amount = 1.0;
    apply_brush(&mut raised, &stamp);
    stamp.mode = BrushMode::Wall;
    stamp.amount = 3.0;
    apply_brush(&mut walled, &stamp);
    stamp.mode = BrushMode::Erase;
    apply_brush(&mut erased, &stamp);
    stamp.mode = BrushMode::Paint;
    apply_brush(&mut repainted, &stamp);
    let new_cells = terrain_changes(&none, &laid, -2, -2, 6, 6);
    let up = terrain_changes(&laid, &raised, -2, -2, 6, 6);
    let walls_up = terrain_changes(&laid, &walled, -2, -2, 6, 6);
    let gone = terrain_changes(&laid, &erased, -2, -2, 6, 6);
    assert!(
      new_cells.len() == 16
        && !new_cells[0].removed
        && !up.is_empty()
        && up.len() <= 16
        && walls_up.len() == 16
        && gone.len() == 16
        && gone[0].removed
        && terrain_changes(&laid, &repainted, -2, -2, 6, 6).is_empty(),
      "terrain brush preview: the cells a stamp lays, raises, walls or takes away, none painted over again"
    );
  }

  #[test]
  fn magnet_and_raise_without_laying() {
    // the magnet brush: beside a platform (its top at 2 m, its bottom at 0) on a large ground, the
    // square sticks to the platform's edge at its top, going under it; over it, or too far, nothing
    let mut ms = Scene::new();
    let flat = |x0: f32, x1: f32, z0: f32, z1: f32, y: f32| Primitive {
      pos: vec![
        Vec3::new(x0, y, z0),
        Vec3::new(x1, y, z0),
        Vec3::new(x1, y, z1),
        Vec3::new(x0, y, z1),
      ],
      idx: vec![0, 1, 2, 0, 2, 3],
      ..Default::default()
    };
    let render = |prims: Vec<Primitive>| {
      let mut mesh = Mesh::new();
      mesh.prims = prims;
      Node {
        kind: NodeKind::Render,
        mesh: Some(Arc::new(mesh)),
        ..Default::default()
      }
    };
    ms.add(render(vec![flat(-50.0, 50.0, -50.0, 50.0, 0.0)]));
    let platform_id = ms.add(render(vec![
      flat(0.0, 10.0, 0.0, 10.0, 0.0),
      flat(0.0, 10.0, 0.0, 10.0, 2.0),
    ]));
    ms.begin_frame();
    let beside = nearest_decor_edge(&ms, Vec3::new(12.0, 0.0, 5.0), 4.0, 8.0, 0);
    let over = nearest_decor_edge(&ms, Vec3::new(5.0, 2.0, 5.0), 4.0, 8.0, 0);
    let far = nearest_decor_edge(&ms, Vec3::new(20.0, 0.0, 5.0), 4.0, 8.0, 0);
    // ... and of a terrain painted, the level of its ground nearest to the square (3 m up there)
    let mut painted = TerrainGrid::default();
    let lay = stroke(BrushMode::Paint, Vec3::new(2.0, 0.0, 2.0), 4.0, 0.0, 3.0);
    apply_brush(&mut painted, &lay);
    let near_terrain = terrain_level_near(&painted, Vec3::new(6.0, 0.0, 2.0), 4.0);
    let far_terrain = terrain_level_near(&painted, Vec3::new(12.0, 0.0, 2.0), 4.0);
    assert!(
      beside.is_some_and(|(top, on)| on == platform_id
        && (top.x - 10.0).abs() < 1e-3
        && (top.y - 2.0).abs() < 1e-3)
        && over.is_none()
        && far.is_none()
        && near_terrain.is_some_and(|p| (p.y - 3.0).abs() < 1e-4)
        && far_terrain.is_none(),
      "magnet brush: the level of the platform beside the square (its top), of the terrain painted next to it; not over, not far"
    );
    // the right button erasing raises the ground there is, never lays any
    let mut none2 = painted.clone();
    let mut rise = stroke(BrushMode::Raise, Vec3::new(20.0, 0.0, 20.0), 4.0, 1.0, 0.0);
    rise.lay = false;
    let laid_nothing = !apply_brush(&mut none2, &rise) && none2.at(20, 20) == TerrainGrid::NONE;
    rise.lay = true;
    assert!(
      laid_nothing && apply_brush(&mut none2, &rise) && none2.at(20, 20) == TerrainGrid::GROUND,
      "terrain brush: raising without laying ground (the right button erasing) leaves the void as it is"
    );
  }

  #[test]
  fn grid_grows_keeping_its_cells_and_reads_old_terrains() {
    let mut g = TerrainGrid::default();
    apply_brush(
      &mut g,
      &stroke(BrushMode::Wall, Vec3::new(0.5, 0.0, 0.5), 1.0, 2.0, 1.0),
    );
    assert!(g.w == 1 && g.d == 1 && g.at(0, 0) == TerrainGrid::WALL && !g.empty());
    g.grow(-3, 2, 4, 5);
    assert!(g.x0 == -3 && g.z0 == 0 && g.w == 8 && g.d == 6);
    assert!(
      g.at(0, 0) == TerrainGrid::WALL && g.wall[g.cell_index(0, 0)] == 2.0 && g.corner(1, 1) == 1.0
    );
    assert!(g.at(4, 5) == TerrainGrid::NONE && g.corner(-3, 6) == 0.0);
    // a node's grid; a terrain of before the looks: one look, its fields beside the grid's
    let node = Node {
      extras: json!({"og_terrain_grid": {"cell": 2, "w": 1, "d": 1, "heights": [0, 100, 0, 100],
        "cells": "1", "floor_material": 4, "floor_tile": 3}}),
      ..Default::default()
    };
    let old = node_terrain(&node).unwrap();
    assert!(old.cell == 2.0 && old.looks.len() == 1 && old.looks[0].floor_material == 4);
    assert!(old.looks[0].floor_tile == 3.0 && old.look == vec![0] && old.corner(1, 0) == 1.0);
    assert!(
      TerrainGrid::from_json(&json!({"w": 2, "d": 1, "heights": [0], "cells": "11"})).is_none()
    );
  }
}
