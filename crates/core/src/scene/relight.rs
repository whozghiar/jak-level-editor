//! The light of the level around elements, given to them. Jak 3's decor carries its light: 8
//! palettes per vertex (the ambient light, the sun from 4 directions, the lamps), baked by the
//! game's artists for each place. An element placed or moved elsewhere keeps the light of where it
//! came from: too dark or too bright, too blue or too warm next to its new neighbors. Here each of
//! its vertices takes the palettes of the level's vertices around it that face the same way (the
//! nearer, the more); the element gets palettes of its own (TodPalette::local: the game view draws
//! it with the new geometry, the export writes them) and its vertex colors (the editor's own view)
//! from them.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;

use super::{Mesh, NodeKind, Scene, TodColors, TodPalette};
use crate::math::{AABB, Mat4, Vec3, Vec4, dot, length, length2};

/// Marks a node lit where it stands: moved, it is lit again where it goes (the level's own pieces
/// keep the light the game gave them unless asked).
pub const K_LIT_HERE: &str = "og_lit_here";

/// The most vertices lit at once (light_like_surroundings' `max_vertices`)
pub const K_LIGHT_MAX_VERTICES: usize = 400_000;

/// meters: the grid of the level's vertices
const K_CELL: f32 = 2.0;
/// searched in turn until enough are found
const K_RADII: [f32; 4] = [2.0, 4.0, 8.0, 16.0];
const K_ENOUGH: usize = 4;

fn cell_key(x: i32, y: i32, z: i32) -> u64 {
  let f = |v: i32| (v.wrapping_add(1048576) as u32 as u64) & 0x1fffff;
  f(x) | (f(y) << 21) | (f(z) << 42)
}

fn cell_of(q: Vec3) -> (i32, i32, i32) {
  (
    (q.x / K_CELL).floor() as i32,
    (q.y / K_CELL).floor() as i32,
    (q.z / K_CELL).floor() as i32,
  )
}

/// a lit vertex of the level: where it is, the way it faces, its 8 palettes
struct Sample<'a> {
  p: Vec3,
  n: Vec3,
  rgba: &'a [u8],
}

/// the targets: the Render nodes among ids and under them, not an actor's model
fn target_indices(scene: &Scene, ids: &[u32]) -> Vec<usize> {
  let cache = scene.cache();
  let mut out = Vec::new();
  let mut seen = HashSet::new();
  let mut add = |id: u32| {
    if !seen.insert(id) {
      return;
    }
    let Some(i) = scene.index_of(id) else { return };
    let n = &scene.nodes[i];
    if n.kind != NodeKind::Render || n.mesh.is_none() || cache.actor.get(i).is_some_and(|&a| a != 0)
    {
      return;
    }
    out.push(i);
  };
  for &id in ids {
    add(id);
  }
  for below in scene.descendants_of(ids) {
    for id in below {
      add(id);
    }
  }
  out
}

/// The nodes among `ids` and under them that are marked K_LIT_HERE.
pub fn lit_here_nodes(scene: &Scene, ids: &[u32]) -> Vec<u32> {
  target_indices(scene, ids)
    .into_iter()
    .map(|i| &scene.nodes[i])
    .filter(|n| {
      n.extras
        .get(K_LIT_HERE)
        .and_then(Value::as_bool)
        .unwrap_or(false)
    })
    .map(|n| n.id)
    .collect()
}

/// a normal in the world, of length 1 (0 when it has none)
fn world_normal(w: &Mat4, nrm: Option<&Vec3>) -> Vec3 {
  let Some(&n) = nrm else {
    return Vec3::default();
  };
  let n = w.dir(n);
  let l = length(n);
  if l > 1e-6 {
    n * (1.0 / l)
  } else {
    Vec3::default()
  }
}

/// Lights the Render nodes among `ids` and under them (not the actors' models: the game lights them
/// itself) like the level around them, and marks them K_LIT_HERE. Returns how many got it: a node
/// with no lit vertex of the level within 16 m keeps its light, as does a selection too large to
/// light at once (`max_vertices`, K_LIGHT_MAX_VERTICES by default).
pub fn light_like_surroundings(scene: &mut Scene, ids: &[u32], max_vertices: usize) -> usize {
  scene.begin_frame(); // the world matrices after the last changes
  let lit = lights(scene, ids, max_vertices);
  let count = lit.len();
  for (t, mesh, tod) in lit {
    let n = &mut scene.nodes[t];
    n.mesh = Some(mesh);
    n.tod = Some(tod);
    n.extras[K_LIT_HERE] = Value::Bool(true);
  }
  count
}

/// the targets lit: their index, their mesh with its new colors, their lighting
fn lights(
  scene: &Scene,
  ids: &[u32],
  max_vertices: usize,
) -> Vec<(usize, Arc<Mesh>, Arc<TodColors>)> {
  let mut out = Vec::new();
  let cache = scene.cache();
  let targets = target_indices(scene, ids);
  if targets.is_empty() {
    return out;
  }
  let mut vertices = 0;
  let mut region = AABB::default();
  let is_target: HashSet<usize> = targets.iter().copied().collect();
  for &t in &targets {
    for p in &scene.nodes[t].mesh.as_ref().unwrap().prims {
      vertices += p.pos.len();
    }
    if t < cache.bounds.len() && cache.bounds[t].valid() {
      region.add_box(&cache.bounds[t]);
    }
  }
  if !region.valid() || vertices > max_vertices {
    return out;
  }
  let region = region.expanded(K_RADII[3]);

  // the lit vertices of the level around them
  let mut samples: Vec<Sample> = Vec::new();
  let mut grid: HashMap<u64, Vec<u32>> = HashMap::new();
  let count = scene.nodes.len().min(cache.visible.len());
  for i in 0..count {
    if is_target.contains(&i) {
      continue;
    }
    let n = &scene.nodes[i];
    let (Some(mesh), Some(tod)) = (&n.mesh, &n.tod) else {
      continue;
    };
    if n.kind != NodeKind::Render || !cache.visible[i] || cache.actor[i] != 0 {
      continue;
    }
    if i < cache.reference.len() && cache.reference[i] {
      continue;
    }
    if i >= cache.bounds.len() || !cache.bounds[i].valid() || !cache.bounds[i].overlaps(&region) {
      continue;
    }
    if !tod.fits(mesh) {
      continue;
    }
    let w = &cache.worlds[i];
    let rgba = &tod.palette.as_ref().unwrap().rgba;
    for (pi, p) in mesh.prims.iter().enumerate() {
      for (v, &pos) in p.pos.iter().enumerate() {
        let q = w.point(pos);
        if !region.contains(q) {
          continue;
        }
        let color = tod.index[pi][v] as usize * 32;
        let Some(rgba) = rgba.get(color..color + 32) else {
          continue;
        };
        let (x, y, z) = cell_of(q);
        grid
          .entry(cell_key(x, y, z))
          .or_default()
          .push(samples.len() as u32);
        samples.push(Sample {
          p: q,
          n: world_normal(w, p.nrm.get(v)),
          rgba,
        });
      }
    }
  }
  if samples.is_empty() {
    return out;
  }

  for &t in &targets {
    let n = &scene.nodes[t];
    let src = n.mesh.as_ref().unwrap();
    let w = &cache.worlds[t];
    let mut palette = TodPalette {
      local: true,
      ..Default::default()
    };
    let mut tod = TodColors::default();
    let mut colors: HashMap<[u8; 32], u16> = HashMap::new();
    let (mut found, mut all) = (0usize, 0usize);
    let mut sum = [0f64; 32];
    tod.index = vec![Vec::new(); src.prims.len()];
    // the vertices with no sample near: the node's average
    let mut missing: Vec<Vec<usize>> = vec![Vec::new(); src.prims.len()];
    for (pi, p) in src.prims.iter().enumerate() {
      tod.index[pi] = vec![0; p.pos.len()];
      for (v, &pos) in p.pos.iter().enumerate() {
        all += 1;
        let q = w.point(pos);
        let nrm = world_normal(w, p.nrm.get(v));
        // the samples around, the nearer and the more alike in facing the more
        let mut acc = [0f64; 32];
        let mut total = 0f64;
        let mut near = 0usize;
        for r in K_RADII {
          acc = [0.0; 32];
          total = 0.0;
          near = 0;
          let lo = cell_of(q - Vec3::new(r, r, r));
          let hi = cell_of(q + Vec3::new(r, r, r));
          for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
              for z in lo.2..=hi.2 {
                let Some(list) = grid.get(&cell_key(x, y, z)) else {
                  continue;
                };
                for &s in list {
                  let sm = &samples[s as usize];
                  let d2 = length2(sm.p - q);
                  if d2 > r * r {
                    continue;
                  }
                  let mut facing = 1.0;
                  if length2(nrm) > 0.0 && length2(sm.n) > 0.0 {
                    facing = dot(nrm, sm.n).max(0.0) + 0.05;
                  }
                  let wgt = (facing * facing) as f64 / (d2 as f64 + 0.5);
                  for k in 0..32 {
                    acc[k] += wgt * sm.rgba[k] as f64;
                  }
                  total += wgt;
                  near += 1;
                }
              }
            }
          }
          if near >= K_ENOUGH && total > 0.0 {
            break;
          }
        }
        if near == 0 || total <= 0.0 {
          missing[pi].push(v);
          continue;
        }
        found += 1;
        let mut col = [0u8; 32];
        for k in 0..32 {
          let c = acc[k] / total;
          sum[k] += c;
          col[k] = (c.round() as i64).clamp(0, 255) as u8;
        }
        tod.index[pi][v] = add_color(&mut colors, &mut palette, col);
      }
    }
    // too far from the level's lit decor: it keeps its own light
    if found == 0 || found * 2 < all || colors.len() >= 65535 {
      continue;
    }
    if missing.iter().any(|m| !m.is_empty()) {
      let mut avg = [0u8; 32];
      for k in 0..32 {
        avg[k] = ((sum[k] / found as f64).round() as i64).clamp(0, 255) as u8;
      }
      let at = add_color(&mut colors, &mut palette, avg);
      for (pi, m) in missing.iter().enumerate() {
        for &v in m {
          tod.index[pi][v] = at;
        }
      }
    }
    // its colors in the editor's own view, as the extracted levels': each channel's brightest
    // palette (the editor's materials, at 1 where the game's are at 2, twice that)
    let mut mesh = (**src).clone();
    for (pi, p) in mesh.prims.iter_mut().enumerate() {
      let mat = usize::try_from(p.material)
        .ok()
        .and_then(|m| scene.materials.get(m));
      let k = if mat.is_some_and(|m| m.base_color.x <= 1.5) {
        2.0
      } else {
        1.0
      };
      let had = p.col.len() == p.pos.len();
      p.col.resize(p.pos.len(), Vec4::new(1.0, 1.0, 1.0, 0.5));
      for v in 0..p.pos.len() {
        let c = &palette.rgba[tod.index[pi][v] as usize * 32..][..32];
        let mut best = [0f32; 3];
        for pal in 0..8 {
          for ch in 0..3 {
            best[ch] = best[ch].max(c[pal * 4 + ch] as f32);
          }
        }
        let shown = |ch: usize| (best[ch] / 255.0 * k).min(1.0);
        p.col[v] = Vec4::new(
          shown(0),
          shown(1),
          shown(2),
          if had { p.col[v].w } else { 0.5 },
        );
      }
    }
    mesh.touch();
    tod.palette = Some(Arc::new(palette));
    out.push((t, Arc::new(mesh), Arc::new(tod)));
  }
  out
}

/// a color's index in the palette, added when new
fn add_color(colors: &mut HashMap<[u8; 32], u16>, palette: &mut TodPalette, col: [u8; 32]) -> u16 {
  let next = colors.len() as u16;
  *colors.entry(col).or_insert_with(|| {
    palette.rgba.extend_from_slice(&col);
    next
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::{Node, Primitive};

  #[test]
  fn placed_element_takes_the_light_around_it() {
    // an element placed on a lit floor takes the floor's light (its 8 palettes), its own palette;
    // one placed far from any lit decor keeps its own light
    let mut s = Scene::new();
    let quad = |y: f32, half: f32| {
      let mut m = Mesh::new();
      m.prims.push(Primitive {
        pos: vec![
          Vec3::new(-half, y, -half),
          Vec3::new(half, y, -half),
          Vec3::new(half, y, half),
          Vec3::new(-half, y, half),
        ],
        nrm: vec![Vec3::new(0.0, 1.0, 0.0); 4],
        idx: vec![0, 2, 1, 0, 3, 2],
        ..Default::default()
      });
      Arc::new(m)
    };
    let mut palette = TodPalette::default();
    for k in 0..8 {
      palette.rgba.extend([100, 50, 10 * k, 128]);
    }
    let palette = Arc::new(palette);
    let tod = TodColors {
      palette: Some(palette.clone()),
      index: vec![vec![0, 0, 0, 0]],
    };
    s.add(Node {
      kind: NodeKind::Render,
      name: "floor".into(),
      mesh: Some(quad(0.0, 6.0)),
      tod: Some(Arc::new(tod)),
      ..Default::default()
    });
    let mut piece = Node {
      kind: NodeKind::Render,
      name: "piece".into(),
      mesh: Some(quad(0.5, 1.0)),
      ..Default::default()
    };
    let near_id = s.add(piece.clone());
    piece.local.t = Vec3::new(200.0, 0.0, 0.0);
    let far_id = s.add(piece);
    let lit = light_like_surroundings(&mut s, &[near_id, far_id], K_LIGHT_MAX_VERTICES);
    let a = s.find(near_id).unwrap();
    let b = s.find(far_id).unwrap();
    let a_tod = a.tod.as_ref().unwrap();
    let a_mesh = a.mesh.as_ref().unwrap();
    let a_palette = a_tod.palette.as_ref().unwrap();
    let same = a_tod.fits(a_mesh)
      && a_palette.local
      && a_palette.size() == 1
      && a_palette.rgba[..32] == palette.rgba[..32];
    assert!(
      lit == 1
        && same
        && a.extras[K_LIT_HERE] == true
        && a_mesh.prims[0].col.len() == 4
        && (a_mesh.prims[0].col[0].x - 100.0 / 255.0).abs() < 0.01
        && b.tod.is_none()
        && lit_here_nodes(&s, &[near_id, far_id]) == vec![near_id],
      "placing: an element takes the light of the level around it, one far from it keeps its own"
    );
  }
}
