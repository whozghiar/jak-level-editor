//! The ground of the extracted levels: which pieces of a level's shell are ground (locked, so that
//! clicks go to what stands on it), the walls the game merged with it, and the floor of a piece.

use std::sync::Arc;

use serde_json::json;

use super::{Mesh, NodeKind, Primitive, Scene, TodColors};
use crate::math::{Mat4, cross, fmax, length};

/// Whether a piece of a level's shell is ground, which the editor locks so that clicks go to what
/// stands on it: 60 m across or more, most of its surface facing up or down. A facade, a wall or a
/// door of a level's shell as large is not (they stay selectable).
pub fn ground_piece(mesh: &Mesh) -> bool {
  let b = mesh.bounds();
  if !b.valid() || fmax(b.size().x, b.size().z) < 60.0 {
    return false;
  }
  let (mut flat, mut all) = (0f64, 0f64);
  for p in &mesh.prims {
    for t in p.idx.as_chunks::<3>().0 {
      let a = p.pos[t[0] as usize];
      let n = cross(p.pos[t[1] as usize] - a, p.pos[t[2] as usize] - a);
      let area = length(n);
      all += area as f64;
      if n.y.abs() > 0.7 * area {
        flat += area as f64;
      }
    }
  }
  all > 0.0 && flat >= 0.5 * all
}

/// A piece whose texture is of natural ground (sand, grass, rock, dirt, mud, snow, a beach, a
/// mountain, a dune, a cliff, lava, "ground"): ground even over other ground (a ledge, a terrace);
/// the level's pieces are named after their texture
pub fn natural_ground(name: &str) -> bool {
  let n = name.to_ascii_lowercase();
  [
    "sand", "grass", "rock", "dirt", "mud", "snow", "beach", "mount", "dune", "cliff", "lava",
    "ground",
  ]
  .iter()
  .any(|w| n.contains(w))
}

/// ... and of a structure (a roof, a bridge, a pillar, the palace, a tower's side, a monitor, a
/// beam, a pipe, a brace): never ground, however large and flat
pub fn structure_texture(name: &str) -> bool {
  let n = name.to_ascii_lowercase();
  [
    "roof",
    "bridge",
    "pillar",
    "palace",
    "tower-side",
    "monitor",
    "beam",
    "pipe",
    "brace",
  ]
  .iter()
  .any(|w| n.contains(w))
}

/// The height of a piece's floor: of its flat surfaces (by area) the lowest fifth's, in the world
/// (`world` its node's matrix), and their area.
pub fn piece_floor(mesh: &Mesh, world: &Mat4) -> (f32, f32) {
  let mut flats: Vec<(f32, f32)> = Vec::new(); // height, area
  let mut total = 0f64;
  for p in &mesh.prims {
    for t in p.idx.as_chunks::<3>().0 {
      let (a, b, c) = (
        world.point(p.pos[t[0] as usize]),
        world.point(p.pos[t[1] as usize]),
        world.point(p.pos[t[2] as usize]),
      );
      let n = cross(b - a, c - a);
      let l = length(n);
      if l < 1e-8 || n.y.abs() < 0.7 * l {
        continue;
      }
      flats.push(((a.y + b.y + c.y) / 3.0, l * 0.5));
      total += (l * 0.5) as f64;
    }
  }
  let area = total as f32;
  if flats.is_empty() {
    return (0.0, area);
  }
  sort_pairs(&mut flats);
  let mut acc = 0f64;
  for &(y, a) in &flats {
    acc += a as f64;
    if acc >= 0.2 * total {
      return (y, area);
    }
  }
  (flats.last().unwrap().0, area)
}

/// std::sort of (height, area) pairs
fn sort_pairs(v: &mut [(f32, f32)]) {
  v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
}

/// A piece of ground (ground_piece) the game made one with walls standing on it (a street and the
/// facade along it): what stands more than 3 m above its ground, mostly upright (walls, and the roofs
/// on them), moved to a piece of its own, <name>-walls, selectable; a hill is left whole. Its light
/// (the game's palettes) goes with each part. Returns the new node's index, None when there is
/// nothing to split.
pub fn split_ground_piece(scene: &mut Scene, index: usize) -> Option<usize> {
  let src = scene.nodes.get(index)?.clone();
  if src.kind != NodeKind::Render {
    return None;
  }
  let mesh = src.mesh.clone()?;
  // the height of its ground: the low fifth of its flat surfaces (by area)
  struct Tri {
    prim: usize,
    first: usize,
    area: f32,
    ny: f32,
    top: f32,
    mid: f32,
  }
  let mut tris = Vec::new();
  let mut flat: Vec<(f32, f32)> = Vec::new(); // height, area
  let mut flat_area = 0f64;
  for (pi, p) in mesh.prims.iter().enumerate() {
    for (k, t) in p.idx.as_chunks::<3>().0.iter().enumerate() {
      let (a, b, c) = (
        p.pos[t[0] as usize],
        p.pos[t[1] as usize],
        p.pos[t[2] as usize],
      );
      let n = cross(b - a, c - a);
      let l = length(n);
      if l < 1e-8 {
        continue;
      }
      let tri = Tri {
        prim: pi,
        first: k * 3,
        area: l * 0.5,
        ny: n.y / l,
        top: fmax(fmax(a.y, b.y), c.y),
        mid: (a.y + b.y + c.y) / 3.0,
      };
      if tri.ny.abs() >= 0.7 {
        flat.push((tri.mid, tri.area));
        flat_area += tri.area as f64;
      }
      tris.push(tri);
    }
  }
  if flat.is_empty() {
    return None;
  }
  sort_pairs(&mut flat);
  let mut base = flat[0].0;
  let mut acc = 0f64;
  for &(y, a) in &flat {
    base = y;
    acc += a as f64;
    if acc >= 0.2 * flat_area {
      break;
    }
  }
  // what stands more than 3 m above it: walls (and the roofs on them) rather than the ground
  let above = base + 3.0;
  let mut up: Vec<Vec<bool>> = mesh
    .prims
    .iter()
    .map(|p| vec![false; p.idx.len() / 3])
    .collect();
  let (mut area, mut upright) = (0f64, 0f64);
  let mut high = base;
  for t in &tris {
    let is_flat = t.ny.abs() >= 0.7;
    if !(if is_flat {
      t.mid > above
    } else {
      t.top > above
    }) {
      continue;
    }
    up[t.prim][t.first / 3] = true;
    area += t.area as f64;
    if t.ny.abs() < 0.3 {
      upright += t.area as f64;
    }
    high = fmax(high, t.top);
  }
  // not a hill (its high slopes are not upright), not a few posts
  if area < 50.0 || upright < 0.4 * area || high - base < 3.0 {
    return None;
  }

  // the two meshes, each vertex of a part once (and its palettes of the game's light)
  let tod = src.tod.clone().filter(|t| t.fits(&mesh));
  let part = |walls: bool, tod_index: &mut Vec<Vec<u16>>| {
    let mut out = Mesh::new();
    out.name = mesh.name.clone() + if walls { "-walls" } else { "" };
    out.extras = mesh.extras.clone();
    for (pi, p) in mesh.prims.iter().enumerate() {
      let mut q = Primitive {
        material: p.material,
        ..Default::default()
      };
      let mut remap = vec![u32::MAX; p.pos.len()];
      let mut idx_tod = Vec::new();
      for (k, t) in p.idx.as_chunks::<3>().0.iter().enumerate() {
        if up[pi][k] != walls {
          continue;
        }
        for &v in t {
          let vi = v as usize;
          if remap[vi] == u32::MAX {
            remap[vi] = q.pos.len() as u32;
            q.pos.push(p.pos[vi]);
            if vi < p.nrm.len() {
              q.nrm.push(p.nrm[vi]);
            }
            if vi < p.uv.len() {
              q.uv.push(p.uv[vi]);
            }
            if vi < p.col.len() {
              q.col.push(p.col[vi]);
            }
            for (name, values) in &p.extra_attrs {
              if vi < values.len() {
                q.extra_attrs
                  .entry(name.clone())
                  .or_default()
                  .push(values[vi]);
              }
            }
            if let Some(tod) = &tod {
              idx_tod.push(tod.index[pi][vi]);
            }
          }
          q.idx.push(remap[vi]);
        }
        if k < p.tri_pat.len() {
          q.tri_pat.push(p.tri_pat[k]);
        }
      }
      if q.idx.is_empty() {
        continue;
      }
      out.prims.push(q);
      if tod.is_some() {
        tod_index.push(idx_tod);
      }
    }
    out
  };
  let (mut ground_tod, mut walls_tod) = (Vec::new(), Vec::new());
  let ground = part(false, &mut ground_tod);
  let walls = part(true, &mut walls_tod);
  if ground.prims.is_empty() || walls.prims.is_empty() {
    return None;
  }
  let mut w = src;
  w.id = 0;
  w.name += "-walls";
  w.mesh = Some(Arc::new(walls));
  w.extras["og_terrain"] = json!(false); // a piece of the shell, not its ground
  w.locked = false;
  if let Some(tod) = &tod {
    let palette = tod.palette.clone();
    scene.nodes[index].tod = Some(Arc::new(TodColors {
      palette: palette.clone(),
      index: ground_tod,
    }));
    w.tod = Some(Arc::new(TodColors {
      palette,
      index: walls_tod,
    }));
  } else {
    w.tod = None;
  }
  scene.nodes[index].mesh = Some(Arc::new(ground));
  scene.add(w);
  Some(scene.nodes.len() - 1)
}
