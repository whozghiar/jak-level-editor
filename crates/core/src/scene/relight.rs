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

use super::{Mesh, NodeKind, Scene, TodColors, TodPalette, tod_from_color, tod_ratios};
use crate::game::mood::{MoodSettings, game_mood};
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
struct Sample {
  p: Vec3,
  n: Vec3,
  rgba: [u8; 32],
}

/// A piece that keeps the light it has in its level (a structure's parts of the game: its level's
/// buildings' own light), never lit again
pub const K_KEEP_LIGHT: &str = "og_keep_light";

/// the targets: the Render nodes among ids and under them, not an actor's model, not a piece that
/// keeps its light
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
    if n.kind != NodeKind::Render
      || n.mesh.is_none()
      || cache.actor.get(i).is_some_and(|&a| a != 0)
      || n.extras.get(K_KEEP_LIGHT).and_then(Value::as_bool) == Some(true)
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
/// light at once (`max_vertices`, K_LIGHT_MAX_VERTICES by default); unless an outdoor level's
/// light would leave it in the dark (`dark_outdoors`): it is then lit from its colors, as new
/// geometry.
pub fn light_like_surroundings(scene: &mut Scene, ids: &[u32], max_vertices: usize) -> usize {
  scene.begin_frame(); // the world matrices after the last changes
  let lit = lights(scene, ids, max_vertices);
  let mut done = HashSet::new();
  for (t, mesh, tod) in lit {
    let n = &mut scene.nodes[t];
    n.mesh = Some(mesh);
    n.tod = Some(tod);
    n.extras[K_LIT_HERE] = Value::Bool(true);
    done.insert(t);
  }
  let weights = outdoor_weights();
  for t in target_indices(scene, ids) {
    if done.contains(&t) {
      continue;
    }
    let Some(mesh) = dark_outdoors(scene, t, &weights) else {
      // its own light kept: still not the level's (tod_ratios), lit again where it is moved
      if scene.nodes[t].tod.is_some() {
        scene.nodes[t].extras[K_LIT_HERE] = Value::Bool(true);
      }
      continue;
    };
    let n = &mut scene.nodes[t];
    n.mesh = Some(mesh);
    n.tod = None;
    n.extras[K_LIT_HERE] = Value::Bool(true);
    done.insert(t);
  }
  done.len()
}

/// how much an outdoor level lights each of the 8 palettes at noon (the editor's mood, the city's
/// lights: what a new level gets in the game, update-mood-ctyindb)
fn outdoor_weights() -> [f32; 8] {
  let m = game_mood(&MoodSettings::default());
  std::array::from_fn(|k| m.times[k][3] * (m.times[k][0] + m.times[k][1] + m.times[k][2]) / 3.0)
}

/// A piece keeping its own light whose light an outdoor level leaves in the dark: its colors as the
/// editor's own view shows them (each channel's brightest palette), to be lit from them. A piece of
/// an indoor level is lit by its lamps (the palettes its level's mood keeps on: Onin's tent by its
/// flames), which an outdoor level drives as the sun of other hours or leaves off; the pieces lit
/// by the ambient light and the sun, as outdoor levels light them, keep their own.
fn dark_outdoors(scene: &Scene, t: usize, weights: &[f32; 8]) -> Option<Arc<Mesh>> {
  let n = &scene.nodes[t];
  let (mesh, tod) = (n.mesh.as_ref()?, n.tod.as_ref()?);
  if !tod.fits(mesh) {
    return None;
  }
  let rgba = &tod.palette.as_ref()?.rgba;
  // each vertex: lit at noon, against its brightest palette
  let (mut lit, mut brightest) = (0f64, 0f64);
  for prim in &tod.index {
    for &color in prim {
      let Some(c) = rgba.get(color as usize * 32..color as usize * 32 + 32) else {
        continue;
      };
      let mut best = 0f32;
      for k in 0..8 {
        let v = (c[k * 4] as f32 + c[k * 4 + 1] as f32 + c[k * 4 + 2] as f32) / 3.0;
        lit += (v * weights[k]) as f64;
        best = best.max(v);
      }
      brightest += best as f64;
    }
  }
  // ponytail: one hour (noon) and half of its brightest as the line; a piece lit by palettes 1..4
  // only at other hours (an indoor lamp stored there) would need the day averaged
  if brightest <= 0.0 || lit >= 0.5 * brightest {
    return None;
  }
  Some(Arc::new(shown_colors(scene, mesh, rgba, &tod.index)))
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
  // what is being lit is never its own surroundings, its pieces that keep their light included (a
  // prefab's parts of a level, a structure's): they would light all of it like themselves
  let mut placed: HashSet<usize> = ids.iter().filter_map(|&id| scene.index_of(id)).collect();
  for below in scene.descendants_of(ids) {
    placed.extend(below.into_iter().filter_map(|id| scene.index_of(id)));
  }

  // the lit vertices of the level around them: the game's light of its decor, and the light the
  // new geometry (the terrain, the shapes) gets from its color in the game view and the export
  // (game_export's): a new level's own light
  let ratios = std::cell::OnceCell::new();
  let mut samples: Vec<Sample> = Vec::new();
  let mut grid: HashMap<u64, Vec<u32>> = HashMap::new();
  let count = scene.nodes.len().min(cache.visible.len());
  for i in 0..count {
    if is_target.contains(&i) || placed.contains(&i) {
      continue;
    }
    let n = &scene.nodes[i];
    let Some(mesh) = &n.mesh else {
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
    let tod = n.tod.as_ref().filter(|t| t.fits(mesh));
    let w = &cache.worlds[i];
    for (pi, p) in mesh.prims.iter().enumerate() {
      let base = usize::try_from(p.material)
        .ok()
        .and_then(|m| scene.materials.get(m))
        .map(|m| m.base_color);
      for (v, &pos) in p.pos.iter().enumerate() {
        let q = w.point(pos);
        if !region.contains(q) {
          continue;
        }
        let rgba: [u8; 32] = match tod {
          Some(tod) => {
            let color = tod.index[pi][v] as usize * 32;
            let rgba = &tod.palette.as_ref().unwrap().rgba;
            let Some(c) = rgba.get(color..color + 32) else {
              continue;
            };
            c.try_into().unwrap()
          }
          None => {
            let mut shown = p
              .col
              .get(v)
              .copied()
              .unwrap_or(Vec4::new(1.0, 1.0, 1.0, 1.0));
            if let Some(b) = base {
              shown = Vec4::new(shown.x * b.x, shown.y * b.y, shown.z * b.z, shown.w);
            }
            // the level's (what is being lit is not the level's)
            let ratios = ratios.get_or_init(|| {
              let ids: Vec<u32> = (0..scene.nodes.len())
                .filter(|i| !is_target.contains(i))
                .map(|i| scene.nodes[i].id)
                .collect();
              tod_ratios(scene, &ids)
            });
            tod_from_color(shown, ratios)
          }
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
    let mesh = shown_colors(scene, src, &palette.rgba, &tod.index);
    tod.palette = Some(Arc::new(palette));
    out.push((t, Arc::new(mesh), Arc::new(tod)));
  }
  out
}

/// `src` with its colors in the editor's own view, as the extracted levels': each channel's
/// brightest palette of its colors (`index` into `rgba`; the editor's materials, at 1 where the
/// game's are at 2, twice that)
fn shown_colors(scene: &Scene, src: &Mesh, rgba: &[u8], index: &[Vec<u16>]) -> Mesh {
  let mut mesh = src.clone();
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
      let c = &rgba[index[pi][v] as usize * 32..][..32];
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
  mesh
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

  #[test]
  fn a_placed_prefab_is_not_its_own_surroundings() {
    // a prefab placed alone: a part of it that keeps its light (a level's piece of a structure)
    // does not light the rest of it; outside of it, the same lit piece does
    let mut s = Scene::new();
    let quad = |y: f32| {
      let mut m = Mesh::new();
      m.prims.push(Primitive {
        pos: vec![
          Vec3::new(-2.0, y, -2.0),
          Vec3::new(2.0, y, -2.0),
          Vec3::new(2.0, y, 2.0),
          Vec3::new(-2.0, y, 2.0),
        ],
        nrm: vec![Vec3::new(0.0, 1.0, 0.0); 4],
        idx: vec![0, 2, 1, 0, 3, 2],
        ..Default::default()
      });
      Arc::new(m)
    };
    let mut palette = TodPalette::default();
    for _ in 0..8 {
      palette.rgba.extend([20, 20, 20, 128]);
    }
    let kept = Node {
      kind: NodeKind::Render,
      name: "kept".into(),
      mesh: Some(quad(0.0)),
      tod: Some(Arc::new(TodColors {
        palette: Some(Arc::new(palette)),
        index: vec![vec![0, 0, 0, 0]],
      })),
      extras: serde_json::json!({ K_KEEP_LIGHT: true }),
      ..Default::default()
    };
    let wall = Node {
      kind: NodeKind::Render,
      name: "wall".into(),
      mesh: Some(quad(0.5)),
      ..Default::default()
    };
    let (k, w) = (s.add(kept.clone()), s.add(wall.clone()));
    assert_eq!(
      light_like_surroundings(&mut s, &[k, w], K_LIGHT_MAX_VERTICES),
      0
    );
    assert!(s.find(w).unwrap().tod.is_none());
    let mut s = Scene::new();
    s.add(kept);
    let w = s.add(wall);
    assert_eq!(
      light_like_surroundings(&mut s, &[w], K_LIGHT_MAX_VERTICES),
      1
    );
  }

  #[test]
  fn a_piece_on_new_geometry_takes_its_light() {
    // a new level: a floor made in the editor (lit from its color, no palettes) under a piece of
    // an indoor level: the piece takes the floor's light, as the game view and the export give it
    let mut s = Scene::new();
    let mut floor = Mesh::new();
    floor.prims.push(Primitive {
      pos: vec![
        Vec3::new(-6.0, 0.0, -6.0),
        Vec3::new(6.0, 0.0, -6.0),
        Vec3::new(6.0, 0.0, 6.0),
        Vec3::new(-6.0, 0.0, 6.0),
      ],
      nrm: vec![Vec3::new(0.0, 1.0, 0.0); 4],
      col: vec![Vec4::new(0.5, 0.5, 0.5, 1.0); 4],
      idx: vec![0, 2, 1, 0, 3, 2],
      ..Default::default()
    });
    s.add(Node {
      kind: NodeKind::Render,
      mesh: Some(Arc::new(floor)),
      ..Default::default()
    });
    let mut m = Mesh::new();
    m.prims.push(Primitive {
      pos: vec![
        Vec3::new(0.0, 0.5, 0.0),
        Vec3::new(1.0, 0.5, 0.0),
        Vec3::new(0.0, 0.5, 1.0),
      ],
      nrm: vec![Vec3::new(0.0, 1.0, 0.0); 3],
      idx: vec![0, 2, 1],
      ..Default::default()
    });
    let mut palette = TodPalette::default();
    palette.rgba = vec![0; 32];
    palette.rgba[20..24].copy_from_slice(&[25, 25, 25, 128]); // a lamp of its level, dim
    let id = s.add(Node {
      kind: NodeKind::Render,
      mesh: Some(Arc::new(m)),
      tod: Some(Arc::new(TodColors {
        palette: Some(Arc::new(palette)),
        index: vec![vec![0, 0, 0]],
      })),
      ..Default::default()
    });
    let lit = light_like_surroundings(&mut s, &[id], K_LIGHT_MAX_VERTICES);
    let n = s.find(id).unwrap();
    let rgba = &n.tod.as_ref().unwrap().palette.as_ref().unwrap().rgba;
    let floor_ambient = tod_from_color(Vec4::new(0.5, 0.5, 0.5, 1.0), &tod_ratios(&s, &[]))[0];
    assert!(
      lit == 1 && rgba[0] == floor_ambient && rgba[0] > 25,
      "placed on new geometry: the piece takes its light ({} against {floor_ambient})",
      rgba[0]
    );
  }

  #[test]
  fn an_indoor_piece_is_lit_from_its_colors_outdoors() {
    // nothing lit around: a piece lit by a lamp of its indoor level (palette 5, which an outdoor
    // level lights at night only) is lit from its colors; one lit by the ambient light keeps its
    // own
    let mut s = Scene::new();
    let piece = |s: &mut Scene, pal: usize| {
      let mut m = Mesh::new();
      m.prims.push(Primitive {
        pos: vec![
          Vec3::new(0.0, 0.0, 0.0),
          Vec3::new(1.0, 0.0, 0.0),
          Vec3::new(0.0, 1.0, 0.0),
        ],
        nrm: vec![Vec3::new(0.0, 0.0, 1.0); 3],
        idx: vec![0, 1, 2],
        ..Default::default()
      });
      let mut palette = TodPalette::default();
      palette.rgba = vec![0; 32];
      palette.rgba[pal * 4..pal * 4 + 4].copy_from_slice(&[200, 150, 100, 128]);
      s.add(Node {
        kind: NodeKind::Render,
        mesh: Some(Arc::new(m)),
        tod: Some(Arc::new(TodColors {
          palette: Some(Arc::new(palette)),
          index: vec![vec![0, 0, 0]],
        })),
        ..Default::default()
      })
    };
    let indoor = piece(&mut s, 5);
    let outdoor = piece(&mut s, 0);
    let lit = light_like_surroundings(&mut s, &[indoor, outdoor], K_LIGHT_MAX_VERTICES);
    let (a, b) = (s.find(indoor).unwrap(), s.find(outdoor).unwrap());
    assert!(
      lit == 1
        && a.tod.is_none()
        && a.extras[K_LIT_HERE] == true
        && (a.mesh.as_ref().unwrap().prims[0].col[0].x - 200.0 / 255.0).abs() < 0.01
        && b.tod.is_some(),
      "an indoor piece outdoors: lit from its colors; an outdoor one keeps its light"
    );
  }
}
