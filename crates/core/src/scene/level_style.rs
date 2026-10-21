//! The look of a level's ground and walls, measured on its decor, to build in its style: the
//! texture its floors draw most (and its walls), how many meters one repeat of it covers there, the
//! light on it, and how uneven its ground is (the slums' worn cobbles, the desert's dunes, the
//! industrial zone's flat slabs). The Build panel makes a floor (a "ground" shape, its bumps as the
//! level's) and walls with them.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use super::{AlphaMode, BlendMode, Material, NodeKind, Scene};
use crate::math::{Vec3, Vec4, cross, length};

#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
  /// in the level's scene
  pub material: i32,
  /// its name
  pub texture: String,
  /// meters of the surface per repeat of its texture
  pub tile: f32,
  /// the vertex colors on it (0.5: the texture as it is), not darker than 0.3 on its brightest
  /// channel (a cave's floor keeps its hue, not its darkness)
  pub color: Vec4,
  /// square meters of it
  pub area: f64,
}

impl Default for Surface {
  fn default() -> Self {
    Self {
      material: -1,
      texture: String::new(),
      tile: 2.0,
      color: Vec4::new(0.5, 0.5, 0.5, 1.0),
      area: 0.0,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LevelStyle {
  pub floor: Surface,
  pub wall: Surface,
  /// meters up and down of its ground
  pub bumps: f32,
  /// meters across a bump
  pub bump_size: f32,
}

impl Default for LevelStyle {
  fn default() -> Self {
    Self {
      floor: Surface::default(),
      wall: Surface::default(),
      bumps: 0.0,
      bump_size: 2.0,
    }
  }
}

impl LevelStyle {
  pub fn valid(&self) -> bool {
    self.floor.material >= 0 || self.wall.material >= 0
  }
}

#[derive(Clone, Copy, Debug, Default)]
struct Bin {
  area: f64,
  /// area-weighted
  log_tile: f64,
  tile_area: f64,
  /// area-weighted
  color: Vec4,
}

type Bins = BTreeMap<i32, Bin>;

/// a material's texture's name (else its own), in lower case
fn texture_name(level: &Scene, m: &Material) -> String {
  let name = match usize::try_from(m.texture)
    .ok()
    .and_then(|t| level.textures.get(t))
  {
    Some(t) => &t.name,
    None => &m.name,
  };
  name.to_lowercase()
}

/// Measured on the decor of a level (its scene as the library has it); the ground under the water
/// of its decor (a canal's bed) is not its floor
pub fn measure_style(level: &Scene) -> LevelStyle {
  let cache = level.cache();
  // the surfaces of water in the decor (a canal's, a pool's: a texture of water; the streams and
  // the sea are actors, not here): the ground under them is a bed, not the ground one walks on.
  // Their height on a grid of 4 m cells.
  let is_water = |m: &Material| texture_name(level, m).contains("water");
  const K_CELL: f32 = 4.0;
  let mut water: HashMap<u64, f32> = HashMap::new();
  let cell_of = |v: Vec3| {
    (((v.x / K_CELL).floor() as i32 as u32 as u64) << 32)
      | (v.z / K_CELL).floor() as i32 as u32 as u64
  };
  let material = |m: i32| usize::try_from(m).ok().and_then(|m| level.materials.get(m));
  for (i, n) in level.nodes.iter().enumerate() {
    let Some(mesh) = &n.mesh else { continue };
    if n.kind != NodeKind::Render || cache.actor[i] != 0 {
      continue;
    }
    let w = &cache.worlds[i];
    for p in &mesh.prims {
      if !material(p.material).is_some_and(is_water) {
        continue;
      }
      for t in p.idx.chunks_exact(3) {
        let pa = w.point(p.pos[t[0] as usize]);
        let pb = w.point(p.pos[t[1] as usize]);
        let pc = w.point(p.pos[t[2] as usize]);
        let cr = cross(pb - pa, pc - pa);
        let len = length(cr);
        if len < 1e-4 || cr.y.abs() < 0.95 * len {
          continue;
        }
        // every cell the triangle covers (its box)
        let (x0, x1) = (pa.x.min(pb.x).min(pc.x), pa.x.max(pb.x).max(pc.x));
        let (z0, z1) = (pa.z.min(pb.z).min(pc.z), pa.z.max(pb.z).max(pc.z));
        if (x1 - x0) * (z1 - z0) > 4096.0 * K_CELL * K_CELL {
          continue;
        }
        let y = (pa.y + pb.y + pc.y) / 3.0;
        let mut x = x0;
        while x <= x1 + K_CELL {
          let mut z = z0;
          while z <= z1 + K_CELL {
            let h = water
              .entry(cell_of(Vec3::new(x.min(x1), 0.0, z.min(z1))))
              .or_insert(y);
            *h = h.max(y);
            z += K_CELL;
          }
          x += K_CELL;
        }
      }
    }
  }
  let under_water = |v: Vec3| {
    water
      .get(&cell_of(v))
      .is_some_and(|&h| v.y < h && v.y > h - 20.0)
  };
  // the floors: the level's ground (its terrain pieces) when it has some, not its roofs
  // by material; the walls of the level's shell (its pieces) apart from those of its instanced
  // decor (the trees' sides, the crates')
  let (mut floors, mut ground, mut walls, mut shell_walls, mut ground_walls) = (
    Bins::new(),
    Bins::new(),
    Bins::new(),
    Bins::new(),
    Bins::new(),
  );
  let mut tilt_area = [0f64; 2];
  let mut tilt_sum = [0f64; 2];
  let mut edges: [Vec<f32>; 2] = [Vec::new(), Vec::new()];
  for (i, n) in level.nodes.iter().enumerate() {
    let Some(mesh) = &n.mesh else { continue };
    if n.kind != NodeKind::Render || cache.actor[i] != 0 {
      continue;
    }
    let w = &cache.worlds[i];
    let terrain = n
      .extras
      .get("og_terrain")
      .and_then(Value::as_bool)
      .unwrap_or(false) as usize;
    let shell = n.extras.get("og_terrain").is_some(); // a piece of the level's shell (tfrag)
    for p in &mesh.prims {
      let Some(m) = material(p.material) else {
        continue;
      };
      // the decor's own surfaces, those the game blends over the rest too (the forest's grass,
      // drawn in its translucent tree), not its glows and light shafts (added or taken away)
      if m.texture < 0 || (m.alpha == AlphaMode::Blend && m.blend != BlendMode::Alpha) {
        continue;
      }
      let uv = p.uv.len() == p.pos.len();
      let col = p.col.len() == p.pos.len();
      for t in p.idx.chunks_exact(3) {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (pa, pb, pc) = (w.point(p.pos[a]), w.point(p.pos[b]), w.point(p.pos[c]));
        let cr = cross(pb - pa, pc - pa);
        let area = length(cr) * 0.5;
        if area < 1e-4 {
          continue;
        }
        let nr = cr * (0.5 / area);
        let up = nr.y.abs();
        if up > 0.85 && under_water((pa + pb + pc) * (1.0 / 3.0)) {
          continue; // a bed under water
        }
        // the steep faces: of the ground itself (its banks and cliffs) first, else of the shell
        let bins = if up > 0.85 {
          Some(if terrain != 0 {
            &mut ground
          } else {
            &mut floors
          })
        } else if up < 0.7 {
          Some(if terrain != 0 {
            &mut ground_walls
          } else if shell {
            &mut shell_walls
          } else {
            &mut walls
          })
        } else {
          None
        };
        if up > 0.5 {
          // the ground's unevenness: how far its faces tilt, how large they are
          tilt_area[terrain] += area as f64;
          tilt_sum[terrain] += (area * up.min(1.0).acos()) as f64;
          if edges[terrain].len() < 200000 {
            edges[terrain].push((area * 2.0).sqrt());
          }
        }
        let Some(bins) = bins else { continue };
        let bin = bins.entry(p.material).or_default();
        bin.area += area as f64;
        if uv {
          let (ua, ub, uc) = (p.uv[a], p.uv[b], p.uv[c]);
          let uv_area = ((ub.x - ua.x) * (uc.y - ua.y) - (uc.x - ua.x) * (ub.y - ua.y)).abs() * 0.5;
          if uv_area > 1e-6 {
            let tile = (area / uv_area).sqrt();
            if tile > 0.05 && tile < 200.0 {
              bin.log_tile += (area * tile.ln()) as f64;
              bin.tile_area += area as f64;
            }
          }
        }
        if col {
          let cc = (p.col[a] + p.col[b] + p.col[c]) * (1.0 / 3.0);
          bin.color = bin.color + cc * area;
        }
      }
    }
  }
  let mut s = LevelStyle::default();
  let mat = |m: i32| &level.materials[m as usize];
  // by texture: a texture drawn by several materials (the forest's grass, split among four) counts
  // as one, its material the one drawing the most of it
  let by_texture = |bins: &Bins| -> Bins {
    let mut merged = Bins::new(); // the texture (else -1 - material) -> its bins summed
    let mut main: BTreeMap<i32, (f64, i32)> = BTreeMap::new(); // -> the material drawing the most of it
    for (&m, b) in bins {
      let t = if mat(m).texture >= 0 {
        mat(m).texture
      } else {
        -1 - m
      };
      let e = merged.entry(t).or_default();
      e.area += b.area;
      e.log_tile += b.log_tile;
      e.tile_area += b.tile_area;
      e.color = e.color + b.color;
      let best = main.entry(t).or_insert((0.0, 0));
      if b.area > best.0 {
        *best = (b.area, m);
      }
    }
    merged.into_iter().map(|(t, b)| (main[&t].1, b)).collect()
  };
  // what a floor or a wall of a new place cannot be: a texture mostly see-through (an overlay the
  // game lays over the ground: its alpha under a quarter), a black filler, and by its name a pipe,
  // a cable, a trim, a roof, a bridge's planks... (only while something else is there)
  let see_through: RefCell<HashMap<i32, bool>> = RefCell::new(HashMap::new());
  let overlay = |m: i32| {
    let t = mat(m).texture;
    let Some(tex) = usize::try_from(t).ok().and_then(|t| level.textures.get(t)) else {
      return false;
    };
    *see_through.borrow_mut().entry(t).or_insert_with(|| {
      let px = &tex.rgba;
      let alpha: f64 = px.iter().skip(3).step_by(4).map(|&a| a as f64).sum();
      !px.is_empty() && (alpha / (px.len() / 4) as f64) < 0.25 * 128.0 // the PS2's 128: opaque
    })
  };
  const K_NEVER: [&str; 22] = [
    "black", "pipe", "tube", "cable", "bark", "trim", "rim", "bridge", "roof", "overhang", "tent",
    "glass", "awning", "window", "light", "screen", "monitor", "skull", "siding", "door", "stand",
    "beam",
  ];
  let unwanted = |m: i32, floor: bool| {
    if overlay(m) {
      return true;
    }
    let name = texture_name(level, mat(m));
    if K_NEVER.iter().any(|w| name.contains(w)) {
      return true;
    }
    floor && name.contains("wall")
  };
  // `strict`: only what is not unwanted; else anything not an overlay
  let pick = |all: &Bins, skip: i32, out: &mut Surface, strict: bool| {
    let floor = skip < 0;
    let per_material: Bins = all
      .iter()
      .filter(|&(&m, _)| {
        if strict {
          !unwanted(m, floor)
        } else {
          !overlay(m)
        }
      })
      .map(|(&m, &b)| (m, b))
      .collect();
    let skip_texture = if skip >= 0 { mat(skip).texture } else { -2 };
    for (m, b) in by_texture(&per_material) {
      if m == skip || (skip_texture >= 0 && mat(m).texture == skip_texture) || b.area <= out.area {
        continue;
      }
      out.material = m;
      out.area = b.area;
      out.tile = if b.tile_area > 0.0 {
        (b.log_tile / b.tile_area).exp() as f32
      } else {
        2.0
      };
      out.color = if b.color.w > 0.0 {
        b.color * (1.0 / b.area) as f32
      } else {
        Vec4::new(0.5, 0.5, 0.5, 1.0)
      };
      out.color.w = 1.0;
      // its hue, not the darkness of a cave or a temple's halls (their lamps light them in the
      // game, a new place has none): the brightest channel at least 0.3 (0.5: the texture as it is)
      let c = out.color;
      let bright = c.x.max(c.y).max(c.z);
      if bright < 0.3 {
        out.color = if bright > 1e-3 {
          let k = 0.3 / bright;
          Vec4::new(c.x * k, c.y * k, c.z * k, 1.0)
        } else {
          Vec4::new(0.3, 0.3, 0.3, 1.0)
        };
      }
    }
    if out.material >= 0 {
      let m = mat(out.material);
      out.texture = match usize::try_from(m.texture)
        .ok()
        .and_then(|t| level.textures.get(t))
      {
        Some(t) => t.name.clone(),
        None => m.name.clone(),
      };
    }
  };
  // the ground, then the other floors; what is unwanted only when nothing else is
  for strict in [true, false] {
    for bins in [&ground, &floors] {
      if s.floor.material < 0 {
        pick(bins, -1, &mut s.floor, strict);
      }
    }
  }
  // the slopes and walls of a terrain: the level's ground's own banks and cliffs, else the walls
  // of its shell, else of its decor (not the trees' sides when there is better)
  // a texture drawn mostly flat (a floor's, on the edge of a bank) is not one of walls
  let mut flat_by_texture: HashMap<i32, f64> = HashMap::new();
  let mut steep_by_texture: HashMap<i32, f64> = HashMap::new();
  for bins in [&ground, &floors] {
    for (&m, b) in bins {
      *flat_by_texture.entry(mat(m).texture).or_default() += b.area;
    }
  }
  for bins in [&ground_walls, &shell_walls, &walls] {
    for (&m, b) in bins {
      *steep_by_texture.entry(mat(m).texture).or_default() += b.area;
    }
  }
  // nor a roof's (the farms' market roofs set in their ground are steep, not slopes of it)
  let roof = |m: i32| texture_name(level, mat(m)).contains("roof");
  let steep_only = |bins: &Bins| -> Bins {
    bins
      .iter()
      .filter(|&(&m, _)| {
        let t = mat(m).texture;
        let flat = flat_by_texture.get(&t).copied().unwrap_or(0.0);
        let steep = steep_by_texture.get(&t).copied().unwrap_or(0.0);
        flat <= steep && !roof(m)
      })
      .map(|(&m, &b)| (m, b))
      .collect()
  };
  let (gw, sw, aw) = (
    steep_only(&ground_walls),
    steep_only(&shell_walls),
    steep_only(&walls),
  );
  let ground_walls_area: f64 = gw
    .iter()
    .map(|(&m, b)| if m == s.floor.material { 0.0 } else { b.area })
    .sum();
  let wall_sources = [
    if ground_walls_area > 200.0 { &gw } else { &sw },
    &sw,
    &aw,
    &walls,
  ];
  for strict in [true, false] {
    for bins in wall_sources {
      if s.wall.material < 0 {
        pick(bins, s.floor.material, &mut s.wall, strict);
      }
    }
  }
  let g = if tilt_area[1] > 0.0 { 1 } else { 0 };
  if tilt_area[g] > 0.0 && !edges[g].is_empty() {
    let e = &mut edges[g];
    let mid = e.len() / 2;
    let edge = (*e.select_nth_unstable_by(mid, f32::total_cmp).1).clamp(0.5, 6.0);
    let tilt = (tilt_sum[g] / tilt_area[g]) as f32;
    s.bump_size = edge * 2.0;
    s.bumps = (tilt.tan() * edge * 0.5).clamp(0.0, 1.5);
  }
  s
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use serde_json::json;

  use super::*;
  use crate::math::Vec2;
  use crate::scene::{Mesh, Node, Primitive, Texture};

  #[test]
  fn floor_and_walls_of_a_level() {
    // a level's style: its floor's texture and its size (1 repeat over 4 m), its walls'
    let mut s = Scene::new();
    let mut t = Texture {
      name: "floor-tex".into(),
      width: 1,
      height: 1,
      rgba: vec![1, 2, 3, 255],
      ..Default::default()
    };
    let mf = Material {
      name: "floor".into(),
      texture: s.add_texture(t.clone()),
      ..Default::default()
    };
    let floor_mat = s.add_material(mf);
    t.name = "wall-tex".into();
    let mw = Material {
      name: "wall".into(),
      texture: s.add_texture(t),
      ..Default::default()
    };
    let wall_mat = s.add_material(mw);
    let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3, uv: f32, mat: i32| {
      let mut p = Primitive {
        pos: vec![a, b, c, d],
        uv: vec![
          Vec2::new(0.0, 0.0),
          Vec2::new(uv, 0.0),
          Vec2::new(uv, uv),
          Vec2::new(0.0, uv),
        ],
        col: vec![Vec4::new(0.3, 0.3, 0.3, 1.0); 4],
        idx: vec![0, 2, 1, 0, 3, 2],
        material: mat,
        ..Default::default()
      };
      p.compute_normals();
      let mut m = Mesh::new();
      m.prims.push(p);
      Arc::new(m)
    };
    let v = Vec3::new;
    s.add(Node {
      kind: NodeKind::Render,
      mesh: Some(quad(
        v(0.0, 0.0, 0.0),
        v(8.0, 0.0, 0.0),
        v(8.0, 0.0, 8.0),
        v(0.0, 0.0, 8.0),
        2.0,
        floor_mat,
      )),
      extras: json!({"og_terrain": true}),
      ..Default::default()
    });
    s.add(Node {
      kind: NodeKind::Render,
      mesh: Some(quad(
        v(0.0, 0.0, 0.0),
        v(6.0, 0.0, 0.0),
        v(6.0, 6.0, 0.0),
        v(0.0, 6.0, 0.0),
        2.0,
        wall_mat,
      )),
      ..Default::default()
    });
    let st = measure_style(&s);
    assert!(
      st.floor.texture == "floor-tex"
        && (st.floor.tile - 4.0).abs() < 0.01
        && st.wall.texture == "wall-tex"
        && (st.wall.tile - 3.0).abs() < 0.01
        && (st.floor.color.x - 0.3).abs() < 0.01
        && st.bumps < 0.01
        && st.valid(),
      "build: a level's style, its floor's and walls' textures, their size, its light, a flat ground"
    );
  }
}
