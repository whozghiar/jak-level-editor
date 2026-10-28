//! The models of the library (`<level>/models/<model>-lodN.glb`), written from the level's .fr3
//! and its art groups (art.rs) as OpenGOAL's extractor writes them (fr3_to_gltf.cpp, add_merc):
//! the mesh (a primitive per draw), its skeleton, its animations, its blend shapes; with only the
//! model's own vertices, in the order its triangles first use them. A material keeps the PS2
//! blend glTF cannot say (`og_blend`) and whether it is seen through (`og_translucent`), as the
//! decor's.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Value, json};

use super::art::{Anim, Art, Joint};
use super::decor::Writer;
use super::fr3::{Level, MercEffect, MercModel, MercVertex};
use crate::io::gltf::{ARRAY_BUFFER, FLOAT};

const RESTART: u32 = u32::MAX;

type Vec3 = [f32; 3];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn length(a: Vec3) -> f32 {
  dot(a, a).sqrt()
}

/// Whether a strip faces away from its vertices' normals (merc_should_swap_tris): below 0
fn facing(tris: &[u32], v: &[MercVertex]) -> f32 {
  let mut sum = 0.0;
  for t in tris.chunks_exact(3) {
    let (a, b, c) = (&v[t[0] as usize], &v[t[1] as usize], &v[t[2] as usize]);
    let (e1, e2) = (sub(b.pos, a.pos), sub(c.pos, a.pos));
    let geometric = [
      e1[1] * e2[2] - e1[2] * e2[1],
      e1[2] * e2[0] - e1[0] * e2[2],
      e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let n = std::array::from_fn(|i| a.normal[i] + b.normal[i] + c.normal[i]);
    let scale = length(geometric) * length(n);
    if scale > 0.0 {
      sum += dot(geometric, n) / scale;
    }
  }
  sum
}

/// The model's draws as triangles of the level's vertices (unstrip_merc_draws): each strip turned
/// to face as its normals, every other triangle of it turned back; and per effect, per draw,
/// where its triangles are
fn triangles(level: &Level, model: &MercModel) -> (Vec<u32>, Vec<Vec<(usize, usize)>>) {
  let (idx, verts) = (&level.merc_indices, &level.merc_vertices);
  let mut out = Vec::new();
  let mut strip: Vec<u32> = Vec::new();
  let flush = |strip: &mut Vec<u32>, out: &mut Vec<u32>| {
    if facing(strip, verts) < 0.0 {
      for t in strip.chunks_exact_mut(3) {
        t.swap(0, 1);
      }
    }
    out.append(strip);
  };
  let mut ranges = Vec::new();
  for e in &model.effects {
    let mut r = Vec::new();
    for d in &e.draws {
      let start = out.len();
      let mut toggle = false;
      for i in 2..d.index_count as usize {
        let at = i + d.first_index as usize;
        let (a, b, c) = (idx[at], idx[at - 1], idx[at - 2]);
        if a == RESTART || b == RESTART || c == RESTART {
          flush(&mut strip, &mut out);
          toggle = false;
          continue;
        }
        strip.extend(if toggle { [a, b, c] } else { [a, c, b] });
        toggle = !toggle;
      }
      flush(&mut strip, &mut out);
      r.push((start, out.len() - start));
    }
    ranges.push(r);
  }
  (out, ranges)
}

/// The level's vertex each mod vertex of an effect copies (mod_to_global_vertex_idx, which the
/// .fr3 does not keep). The extractor numbers them in the order its modifiable draws first use
/// them, each made of the strips of one of the effect's draws that hold vertices the game changes
/// (all of the draw when they all do): each is matched with its draw, strip by strip.
fn mod_to_global(level: &Level, e: &MercEffect) -> Vec<u32> {
  let idx = &level.merc_indices;
  let stream = |first: u32, count: u32| -> &[u32] {
    idx
      .get(first as usize..(first + count) as usize)
      .unwrap_or(&[])
  };
  let strips = |s: &[u32]| -> Vec<Vec<u32>> {
    s.split(|&i| i == RESTART)
      .filter(|x| !x.is_empty())
      .map(<[u32]>::to_vec)
      .collect()
  };
  // a mod vertex `m` copying the level's vertex `g`, if it can (map grows only: undone by length)
  let take = |map: &mut Vec<u32>, m: u32, g: u32| -> bool {
    let (m, gi) = (m as usize, g as usize);
    if m < map.len() {
      return map[m] == g;
    }
    m == map.len()
      && e.mod_vertices.get(m).is_some()
      && e.mod_vertices.get(m) == level.merc_vertices.get(gi)
      && {
        map.push(g);
        true
      }
  };
  let mut map: Vec<u32> = Vec::new();
  let mut next = 0;
  for md in &e.mod_draws {
    let s = stream(md.first_index, md.index_count);
    let found = (next..e.draws.len()).find(|&k| {
      let d = &e.draws[k];
      if (d.mode, d.tex, d.eye_id, d.num_triangles)
        != (md.mode, md.tex, md.eye_id, md.num_triangles)
      {
        return false;
      }
      let g = stream(d.first_index, d.index_count);
      let before = map.len();
      // all of the draw
      if s.len() == g.len()
        && s.iter().zip(g).all(|(&m, &gi)| {
          (m == RESTART) == (gi == RESTART) && (m == RESTART || take(&mut map, m, gi))
        })
      {
        return true;
      }
      map.truncate(before);
      // some of its strips
      let mut source = strips(g).into_iter();
      for ms in strips(s) {
        let ok = source.by_ref().any(|gs| {
          let at = map.len();
          let ok =
            gs.len() == ms.len() && ms.iter().zip(&gs).all(|(&m, &gi)| take(&mut map, m, gi));
          if !ok {
            map.truncate(at);
          }
          ok
        });
        if !ok {
          map.truncate(before);
          return false;
        }
      }
      true
    });
    match found {
      Some(k) => next = k + 1,
      None => break,
    }
  }
  map
}

/// The blend shapes of a model (add_blerc_targets): per target, per vertex of the level, the
/// offsets of its position and normal
fn blend_targets(level: &Level, model: &MercModel) -> Vec<(Vec<Vec3>, Vec<Vec3>)> {
  // the targets: the highest index the effects name
  let mut count = 0;
  for e in &model.effects {
    let mut skip = false;
    for &v in &e.blerc_ints {
      if skip {
        skip = false;
      } else if v == RESTART {
        skip = true;
      } else {
        count = count.max(v as usize + 1);
      }
    }
  }
  let n = level.merc_vertices.len();
  let mut targets = vec![(vec![[0.0; 3]; n], vec![[0.0; 3]; n]); count];
  for e in &model.effects {
    if e.blerc_ints.is_empty() {
      continue;
    }
    let map = mod_to_global(level, e);
    if map.is_empty() {
      continue;
    }
    let ints = &e.blerc_ints;
    let (mut f, mut i) = (0, 0);
    while i < ints.len() {
      f += 1; // the vertex's base
      let mut of_vertex = Vec::new();
      while let Some(&t) = ints.get(i).filter(|&&t| t != RESTART) {
        of_vertex.push((t as usize, f));
        i += 1;
        f += 1;
      }
      let Some(&dest) = ints.get(i + 1) else { break };
      i += 2;
      let Some(&g) = map.get(dest as usize) else {
        continue;
      };
      for (t, f) in of_vertex {
        if let (Some(target), Some(d)) = (targets.get_mut(t), e.blerc_floats.get(f)) {
          target.0[g as usize] = [d[0] * 2.0, d[1] * 2.0, d[2] * 2.0];
          target.1[g as usize] = [d[4] * 8192.0, d[5] * 8192.0, d[6] * 8192.0];
        }
      }
    }
  }
  targets
}

/// The level's texture of each animated texture slot the level has (slots: the names of the
/// slots, data/<game>/texture-slots.json)
pub fn slot_textures(level: &Level, slots: &[String]) -> HashMap<usize, usize> {
  let slot_of: HashMap<&str, usize> = slots
    .iter()
    .enumerate()
    .map(|(i, s)| (s.as_str(), i))
    .collect();
  let mut out = HashMap::new();
  for (i, t) in level.textures.iter().enumerate() {
    if let Some(&s) = slot_of.get(t.name.as_str()) {
      out.insert(s, i);
    }
  }
  out
}

/// A 4x4 matrix as the game stores it: m(r, c) at r + 4c (its rows are the game's vectors)
type Mat = [f32; 16];

/// The bind pose with its translation in meters (unscale_translation)
fn unscaled(m: &Mat) -> Mat {
  let mut out = *m;
  for v in &mut out[12..15] {
    *v /= 4096.0;
  }
  out
}

fn mul(x: &Mat, y: &Mat) -> Mat {
  let mut r = [0.0; 16];
  for rx in 0..4 {
    for cx in 0..4 {
      for yi in 0..4 {
        r[rx + 4 * yi] += x[rx + 4 * cx] * y[cx + 4 * yi];
      }
    }
  }
  r
}

/// The inverse of a 4x4 matrix (common/math/geometry.h, inverse)
fn inverse(m: &Mat) -> Mat {
  let m = |r: usize, c: usize| m[r + 4 * c];
  let a2323 = m(2, 2) * m(3, 3) - m(2, 3) * m(3, 2);
  let a1323 = m(2, 1) * m(3, 3) - m(2, 3) * m(3, 1);
  let a1223 = m(2, 1) * m(3, 2) - m(2, 2) * m(3, 1);
  let a0323 = m(2, 0) * m(3, 3) - m(2, 3) * m(3, 0);
  let a0223 = m(2, 0) * m(3, 2) - m(2, 2) * m(3, 0);
  let a0123 = m(2, 0) * m(3, 1) - m(2, 1) * m(3, 0);
  let a2313 = m(1, 2) * m(3, 3) - m(1, 3) * m(3, 2);
  let a1313 = m(1, 1) * m(3, 3) - m(1, 3) * m(3, 1);
  let a1213 = m(1, 1) * m(3, 2) - m(1, 2) * m(3, 1);
  let a2312 = m(1, 2) * m(2, 3) - m(1, 3) * m(2, 2);
  let a1312 = m(1, 1) * m(2, 3) - m(1, 3) * m(2, 1);
  let a1212 = m(1, 1) * m(2, 2) - m(1, 2) * m(2, 1);
  let a0313 = m(1, 0) * m(3, 3) - m(1, 3) * m(3, 0);
  let a0213 = m(1, 0) * m(3, 2) - m(1, 2) * m(3, 0);
  let a0312 = m(1, 0) * m(2, 3) - m(1, 3) * m(2, 0);
  let a0212 = m(1, 0) * m(2, 2) - m(1, 2) * m(2, 0);
  let a0113 = m(1, 0) * m(3, 1) - m(1, 1) * m(3, 0);
  let a0112 = m(1, 0) * m(2, 1) - m(1, 1) * m(2, 0);
  let det = m(0, 0) * (m(1, 1) * a2323 - m(1, 2) * a1323 + m(1, 3) * a1223)
    - m(0, 1) * (m(1, 0) * a2323 - m(1, 2) * a0323 + m(1, 3) * a0223)
    + m(0, 2) * (m(1, 0) * a1323 - m(1, 1) * a0323 + m(1, 3) * a0123)
    - m(0, 3) * (m(1, 0) * a1223 - m(1, 1) * a0223 + m(1, 2) * a0123);
  let det = 1.0 / det;
  let mut im = [0.0; 16];
  let mut set = |r: usize, c: usize, v: f32| im[r + 4 * c] = v;
  set(
    0,
    0,
    det * (m(1, 1) * a2323 - m(1, 2) * a1323 + m(1, 3) * a1223),
  );
  set(
    0,
    1,
    det * -(m(0, 1) * a2323 - m(0, 2) * a1323 + m(0, 3) * a1223),
  );
  set(
    0,
    2,
    det * (m(0, 1) * a2313 - m(0, 2) * a1313 + m(0, 3) * a1213),
  );
  set(
    0,
    3,
    det * -(m(0, 1) * a2312 - m(0, 2) * a1312 + m(0, 3) * a1212),
  );
  set(
    1,
    0,
    det * -(m(1, 0) * a2323 - m(1, 2) * a0323 + m(1, 3) * a0223),
  );
  set(
    1,
    1,
    det * (m(0, 0) * a2323 - m(0, 2) * a0323 + m(0, 3) * a0223),
  );
  set(
    1,
    2,
    det * -(m(0, 0) * a2313 - m(0, 2) * a0313 + m(0, 3) * a0213),
  );
  set(
    1,
    3,
    det * (m(0, 0) * a2312 - m(0, 2) * a0312 + m(0, 3) * a0212),
  );
  set(
    2,
    0,
    det * (m(1, 0) * a1323 - m(1, 1) * a0323 + m(1, 3) * a0123),
  );
  set(
    2,
    1,
    det * -(m(0, 0) * a1323 - m(0, 1) * a0323 + m(0, 3) * a0123),
  );
  set(
    2,
    2,
    det * (m(0, 0) * a1313 - m(0, 1) * a0313 + m(0, 3) * a0113),
  );
  set(
    2,
    3,
    det * -(m(0, 0) * a1312 - m(0, 1) * a0312 + m(0, 3) * a0112),
  );
  set(
    3,
    0,
    det * -(m(1, 0) * a1223 - m(1, 1) * a0223 + m(1, 2) * a0123),
  );
  set(
    3,
    1,
    det * (m(0, 0) * a1223 - m(0, 1) * a0223 + m(0, 2) * a0123),
  );
  set(
    3,
    2,
    det * -(m(0, 0) * a1213 - m(0, 1) * a0213 + m(0, 2) * a0113),
  );
  set(
    3,
    3,
    det * (m(0, 0) * a1212 - m(0, 1) * a0212 + m(0, 2) * a0112),
  );
  im
}

/// Floats, N per element, in a view of their own (`target`: 0 for none), with their bounds when
/// `minmax`
fn floats<const N: usize>(w: &mut Writer, data: &[[f32; N]], target: u32, minmax: bool) -> usize {
  let view = w.w.add_view(target, |bin| {
    for v in data {
      for x in v {
        bin.extend_from_slice(&x.to_le_bytes());
      }
    }
  });
  let ty = [
    "", "SCALAR", "VEC2", "VEC3", "VEC4", "", "", "", "", "", "", "", "", "", "", "", "MAT4",
  ][N];
  let a = w.w.accessor(view, data.len(), FLOAT, ty);
  if minmax && !data.is_empty() {
    let (lo, hi) = data
      .iter()
      .fold(([f32::MAX; N], [f32::MIN; N]), |(lo, hi), v| {
        (
          std::array::from_fn(|i| lo[i].min(v[i])),
          std::array::from_fn(|i| hi[i].max(v[i])),
        )
      });
    w.w.accessors[a]["min"] = json!(lo.to_vec());
    w.w.accessors[a]["max"] = json!(hi.to_vec());
  }
  a
}

/// The skeleton's nodes after the model's (each bone's matrix relative to its parent): the skin,
/// and the roots of the skeleton
fn skeleton(w: &mut Writer, joints: &[Joint]) -> (Value, Vec<usize>) {
  let first = w.nodes.len();
  let mut children = vec![Vec::new(); joints.len()];
  for (i, j) in joints.iter().enumerate() {
    if j.parent >= 0 {
      children[j.parent as usize].push(first + i);
    }
  }
  for (i, j) in joints.iter().enumerate() {
    let matrix = if j.parent >= 0 {
      mul(
        &unscaled(&joints[j.parent as usize].bind),
        &inverse(&unscaled(&j.bind)),
      )
    } else {
      std::array::from_fn(|k| if k % 5 == 0 { 1.0 } else { 0.0 })
    };
    let mut node = json!({ "matrix": matrix.to_vec() });
    if !j.name.is_empty() {
      node["name"] = json!(j.name);
    }
    if !children[i].is_empty() {
      node["children"] = json!(children[i]);
    }
    w.nodes.push(node);
  }
  let inverse_binds: Vec<[f32; 16]> = joints.iter().map(|j| unscaled(&j.bind)).collect();
  let skin = json!({
    "inverseBindMatrices": floats(w, &inverse_binds, 0, false),
    "joints": (first..first + joints.len()).collect::<Vec<_>>(),
    "skeleton": first,
  });
  let roots = (0..joints.len())
    .filter(|&i| joints[i].parent < 0)
    .map(|i| first + i)
    .collect();
  (skin, roots)
}

/// An animation of the skeleton whose bones are the nodes `bones` (add_animation_to_gltf), with
/// the weights of the model's `targets` blend shapes
fn animation(w: &mut Writer, a: &Anim, bones: &[usize], targets: usize) -> Value {
  let times: Vec<[f32; 1]> = (0..a.frames).map(|i| [i as f32 / a.framerate]).collect();
  let input = floats(w, &times, 0, true);
  let (mut samplers, mut channels) = (Vec::new(), Vec::new());
  let mut channel = |output: usize, node: usize, path: &str| {
    channels.push(json!({"sampler": samplers.len(), "target": {"node": node, "path": path}}));
    samplers.push(json!({"input": input, "interpolation": "LINEAR", "output": output}));
  };
  // the animation's bones are the skeleton's from its third (align and prejoint have no keys)
  for (k, &node) in a.joints.iter().zip(bones.iter().skip(2)) {
    if k.trans.len() == a.frames {
      channel(floats(w, &k.trans, 0, false), node, "translation");
    }
    if k.quat.len() == a.frames {
      channel(floats(w, &k.quat, 0, false), node, "rotation");
    }
    if k.scale.len() == a.frames {
      channel(floats(w, &k.scale, 0, false), node, "scale");
    }
  }
  if targets > 0 && a.blend.len() >= a.frames * targets {
    let weights: Vec<[f32; 1]> = a.blend[..a.frames * targets]
      .iter()
      .map(|&b| [(b as f32 - 64.0) / 128.0])
      .collect();
    // the model's node
    channel(floats(w, &weights, 0, false), 0, "weights");
  }
  json!({"name": a.name, "samplers": samplers, "channels": channels})
}

/// Writes the level's model `model` to `path`, its skeleton and animations from `art`; `slots`:
/// the level's textures of the animated texture slots (slot_textures)
pub fn write_model(
  level: &Level,
  model: &MercModel,
  art: &Art,
  slots: &HashMap<usize, usize>,
  path: &Path,
) -> Result<(), String> {
  let (tris, ranges) = triangles(level, model);
  // the model's vertices, in the order its triangles first use them
  let mut order: Vec<u32> = Vec::new();
  let mut new_of: HashMap<u32, u32> = HashMap::new();
  let tris: Vec<u32> = tris
    .iter()
    .map(|&v| {
      *new_of.entry(v).or_insert_with(|| {
        order.push(v);
        order.len() as u32 - 1
      })
    })
    .collect();
  if order.is_empty() && !level.merc_vertices.is_empty() {
    order.push(0); // an accessor holds one element at least
  }
  let verts: Vec<&MercVertex> = order
    .iter()
    .map(|&v| &level.merc_vertices[v as usize])
    .collect();

  let mut w = Writer::new(level);
  let pos: Vec<Vec3> = verts.iter().map(|v| v.pos.map(|x| x / 4096.0)).collect();
  let st: Vec<[f32; 2]> = verts.iter().map(|v| v.st).collect();
  let rgba: Vec<[f32; 4]> = verts
    .iter()
    .map(|v| {
      let c = |i: usize| v.rgba[i] as f32 / 255.0;
      [c(0), c(1), c(2), 1.0]
    })
    .collect();
  // the bones of each vertex (0: none, else the bone + 1)
  let bones: Vec<u8> = verts
    .iter()
    .flat_map(|v| [v.mats[0], v.mats[1], v.mats[2], 0].map(|m| m.saturating_sub(1)))
    .collect();
  let weights: Vec<[f32; 4]> = verts
    .iter()
    .map(|v| [v.weights[0], v.weights[1], v.weights[2], 0.0])
    .collect();
  let normals: Vec<Vec3> = verts
    .iter()
    .map(|v| {
      let len = length(v.normal);
      if len > 0.0 {
        v.normal.map(|x| x / len)
      } else {
        [0.0, 1.0, 0.0]
      }
    })
    .collect();
  let attributes = json!({
    "POSITION": floats(&mut w, &pos, ARRAY_BUFFER, false),
    "TEXCOORD_0": floats(&mut w, &st, ARRAY_BUFFER, false),
    "COLOR_0": floats(&mut w, &rgba, ARRAY_BUFFER, false),
    "JOINTS_0": w.w.add_u8(&bones, verts.len(), ARRAY_BUFFER, "VEC4"),
    "WEIGHTS_0": floats(&mut w, &weights, ARRAY_BUFFER, false),
    "NORMAL": floats(&mut w, &normals, ARRAY_BUFFER, false),
  });
  if let Some(normal) = w.w.accessors.last_mut() {
    normal["name"] = json!("NORMAL");
  }

  let targets = blend_targets(level, model);
  w.nodes.push(json!({"name": model.name, "mesh": 0}));
  let mut scene = vec![0];
  let mut skins = Vec::new();
  let mut animations = Vec::new();
  if let Some(data) = art.get(&model.name).filter(|d| !d.joints.is_empty()) {
    let (skin, roots) = skeleton(&mut w, &data.joints);
    let bones: Vec<usize> = (1..=data.joints.len()).collect();
    w.nodes[0]["skin"] = json!(0);
    skins.push(skin);
    scene.extend(roots);
    for a in data
      .anims
      .iter()
      .filter(|a| a.frames > 0 && !a.joints.is_empty())
    {
      animations.push(animation(&mut w, a, &bones, targets.len()));
    }
  }
  if !targets.is_empty() {
    // the empty node Blender adds for animated blend shapes takes the model's name
    w.nodes[0]["name"] = json!(format!("{}_blerc", model.name));
  }

  let mut prims = Vec::new();
  for (e, effect) in model.effects.iter().enumerate() {
    let envmap = effect
      .envmap
      .filter(|&(_, t)| (t as usize) < level.textures.len())
      .map(|(mode, t)| (t as i32, mode));
    for (d, draw) in effect.draws.iter().enumerate() {
      // an animated texture: the texture of its slot
      let tex = if draw.tex < 0 {
        slots
          .get(&(-(draw.tex + 1) as usize))
          .map_or(draw.tex, |&t| t as i32)
      } else {
        draw.tex
      };
      let material = w.material(tex, draw.mode, envmap);
      let (start, count) = ranges[e][d];
      let indices = w.indices(&tris[start..start + count]);
      prims.push(json!({
        "attributes": attributes, "indices": indices, "material": material, "mode": 4,
      }));
    }
  }
  if !targets.is_empty() {
    let pick = |v: &Vec<Vec3>| -> Vec<Vec3> { order.iter().map(|&g| v[g as usize]).collect() };
    let mut list = Vec::new();
    for (p, n) in &targets {
      list.push(json!({
        "POSITION": floats(&mut w, &pick(p), ARRAY_BUFFER, false),
        "NORMAL": floats(&mut w, &pick(n), ARRAY_BUFFER, false),
      }));
    }
    for p in &mut prims {
      p["targets"] = json!(list);
    }
  }
  let mut mesh = json!({"name": model.name, "primitives": prims});
  if !targets.is_empty() {
    mesh["weights"] = json!(vec![0.0; targets.len()]);
  }

  let mut doc = json!({
    "asset": {"generator": "opengoal", "version": "2.0"},
    "scenes": [{"nodes": scene}],
    "nodes": w.nodes,
    "meshes": [mesh],
    "materials": w.materials,
  });
  for (key, list) in [
    ("images", w.images),
    ("textures", w.textures),
    ("samplers", w.samplers),
    ("skins", skins),
    ("animations", animations),
  ] {
    if !list.is_empty() {
      doc[key] = Value::Array(list);
    }
  }
  if w.specular {
    doc["extensionsUsed"] = json!(["KHR_materials_specular"]);
  }
  if let Some(dir) = path.parent() {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  }
  w.w.write(doc, path)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::extraction::art::{ArtData, JointKeys};
  use crate::io::extraction::fr3::{DrawMode, MercDraw, Texture};

  fn vertex(i: u32) -> MercVertex {
    let i = i as f32;
    MercVertex {
      pos: [i * 4096.0, 0.0, (i % 2.0) * 4096.0],
      normal: [0.0, 1.0, 0.0],
      weights: [1.0, 0.0, 0.0],
      mats: [3, 0, 0],
      ..Default::default()
    }
  }

  fn draw(first: u32, count: u32, tris: u32, tex: i32) -> MercDraw {
    MercDraw {
      mode: DrawMode(0),
      tex,
      eye_id: 0xff,
      first_index: first,
      index_count: count,
      num_triangles: tris,
    }
  }

  #[test]
  fn model_written_with_its_skeleton_animation_and_blend_shapes() {
    // vertices 0 to 9; the model's strips: 2 3 4 5, then 6 7 8 after a restart; the game changes
    // the second strip only (its modifiable draw: its strip, as copies of 6 7 8)
    let mut level = Level {
      merc_vertices: (0..10).map(vertex).collect(),
      merc_indices: vec![2, 3, 4, 5, RESTART, 6, 7, 8, 0, 1, 2, RESTART],
      textures: vec![Texture {
        w: 1,
        h: 1,
        rgba: vec![255; 4],
        name: "jakc-face".into(),
        ..Default::default()
      }],
      ..Default::default()
    };
    let effect = MercEffect {
      draws: vec![draw(0, 8, 3, -2)],
      mod_draws: vec![draw(8, 4, 3, -2)],
      mod_vertices: vec![vertex(6), vertex(7), vertex(8)],
      // one blend shape, moving mod vertex 1 (the level's 7) one unit up
      blerc_ints: vec![0, RESTART, 1],
      blerc_floats: vec![[0.0; 8], [0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]],
      envmap: None,
    };
    assert_eq!(mod_to_global(&level, &effect), [6, 7, 8]);
    level.models.push(MercModel {
      name: "box-lod0".into(),
      effects: vec![effect],
    });
    let joint = |name: &str, parent: i32| Joint {
      name: name.into(),
      parent,
      bind: std::array::from_fn(|k| if k % 5 == 0 { 1.0 } else { 0.0 }),
    };
    let keys = JointKeys {
      trans: vec![[0.0; 3]; 2],
      quat: vec![[0.0, 0.0, 0.0, 1.0]; 2],
      scale: vec![[1.0; 3]; 2],
    };
    let mut art = Art::new();
    art.insert(
      "box-lod0".into(),
      ArtData {
        joints: vec![joint("align", -1), joint("prejoint", -1), joint("lid", 1)],
        anims: vec![Anim {
          name: "box-open".into(),
          framerate: 60.0,
          frames: 2,
          joints: vec![keys],
          blend: vec![64, 192],
        }],
      },
    );
    // the second slot is the texture's
    let slots = slot_textures(&level, &["skull-gem-dest".into(), "jakc-face".into()]);
    let path = crate::io::test_library::temp_dir("merc-model").join("box-lod0.glb");
    write_model(&level, &level.models[0], &art, &slots, &path).unwrap();

    let j = crate::io::test_library::read_glb_json(&path);
    // the model's 7 vertices only (of 10), its 3 triangles as 3 primitives' worth of indices
    let prim = &j["meshes"][0]["primitives"][0];
    let count = |a: &Value| j["accessors"][a.as_u64().unwrap() as usize]["count"].clone();
    assert_eq!(count(&prim["attributes"]["POSITION"]), 7);
    assert_eq!(count(&prim["indices"]), 9);
    assert_eq!(
      j["materials"][prim["material"].as_u64().unwrap() as usize]["name"],
      "jakc-face"
    );
    // the skeleton after the model's node, the blend shape's weights in the animation
    assert_eq!(j["nodes"][0]["name"], "box-lod0_blerc");
    assert_eq!(j["skins"][0]["joints"], json!([1, 2, 3]));
    assert_eq!(j["scenes"][0]["nodes"], json!([0, 1, 2]));
    assert_eq!(j["nodes"][2]["children"], json!([3]));
    let paths: Vec<&str> = j["animations"][0]["channels"]
      .as_array()
      .unwrap()
      .iter()
      .map(|c| c["target"]["path"].as_str().unwrap())
      .collect();
    assert_eq!(paths, ["translation", "rotation", "scale", "weights"]);
    assert_eq!(prim["targets"].as_array().unwrap().len(), 1);
    assert_eq!(j["meshes"][0]["weights"], json!([0.0]));
  }

  #[test]
  fn matrices_inverted() {
    let m: Mat = [
      2.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 8.0, 0.0, 4096.0, 0.0, 0.0, 1.0,
    ];
    let i = mul(&m, &inverse(&m));
    assert!(
      i.iter()
        .enumerate()
        .all(|(k, &v)| v == if k % 5 == 0 { 1.0 } else { 0.0 })
    );
    assert_eq!(unscaled(&m)[12], 1.0);
  }
}
