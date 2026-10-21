//! The ocean of the level (src/app/actions.cpp update_ocean): like the game, the ocean of a level
//! shown with an ocean map, less where the land of the levels around hides it in the game. A flat
//! mesh at height 0 (the viewport draws it at the ocean's height, its waves moving), served on
//! `mesh/<uid>` like the document's; get_state's `ocean` tells the viewport (setOcean).

use std::sync::Arc;

use serde_json::{Value, json};

use super::Editor;
use super::levels::LevelRole;
use crate::io::game_data::OceanMap;
use crate::io::gltf::{ImportOptions, import_gltf};
use crate::math::Vec3;
use crate::scene::{Mesh, NodeKind, Primitive, Scene};
use crate::strf;

/// Where the land of other levels hides the ocean of `map` in the game (its water at `ocean_y`):
/// per square of 3 m of the map (rows along z, map.size * 32 squares a side), 1 under a surface of
/// a render piece of `land` lying above the water (walls left out).
pub fn ocean_land(map: &OceanMap, ocean_y: f32, land: &Scene) -> Vec<u8> {
  let side = map.size * 32;
  let sq = map.cell / 32.0;
  let mut out = vec![0u8; side * side];
  let cache = land.cache();
  let last = side as i32 - 1;
  for (i, n) in land.nodes.iter().enumerate() {
    let Some(mesh) = &n.mesh else { continue };
    if n.kind != NodeKind::Render {
      continue;
    }
    let w = &cache.worlds[i];
    for p in &mesh.prims {
      for t in p.idx.chunks_exact(3) {
        let (a, b, c) = (
          w.point(p.pos[t[0] as usize]),
          w.point(p.pos[t[1] as usize]),
          w.point(p.pos[t[2] as usize]),
        );
        if a.y.max(b.y).max(c.y) <= ocean_y + 0.3 {
          continue;
        }
        // the squares whose middle it covers, seen from above (a wall covers none)
        let d = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
        if d.abs() < 1e-6 {
          continue;
        }
        let first = |v: f32, o: f32| (((v - o) / sq - 0.5).ceil() as i32).max(0);
        let last = |v: f32, o: f32| (((v - o) / sq - 0.5).floor() as i32).min(last);
        let x0 = first(a.x.min(b.x).min(c.x), map.corner.x);
        let x1 = last(a.x.max(b.x).max(c.x), map.corner.x);
        let z0 = first(a.z.min(b.z).min(c.z), map.corner.z);
        let z1 = last(a.z.max(b.z).max(c.z), map.corner.z);
        for z in z0..=z1 {
          for x in x0..=x1 {
            let px = map.corner.x + (x as f32 + 0.5) * sq;
            let pz = map.corner.z + (z as f32 + 0.5) * sq;
            let u = ((b.x - px) * (c.z - pz) - (c.x - px) * (b.z - pz)) / d;
            let v = ((c.x - px) * (a.z - pz) - (a.x - px) * (c.z - pz)) / d;
            if u < 0.0 || v < 0.0 || u + v > 1.0 {
              continue;
            }
            if u * a.y + v * b.y + (1.0 - u - v) * c.y > ocean_y + 0.3 {
              out[z as usize * side + x as usize] = 1;
            }
          }
        }
      }
    }
  }
  out
}

/// The squares of the map with water (its mid ocean's cells, its near ocean's squares), as above.
pub fn ocean_water(map: &OceanMap) -> Vec<u8> {
  let size = map.size;
  let side = size * 32;
  let mut out = vec![0u8; side * side];
  for r in 0..size {
    for c in 0..size {
      let near_rows = map.near.get(&(r * size + c));
      let near = near_rows.is_some_and(|rows| rows.len() >= 32);
      let all = match near_rows {
        Some(rows) => rows.is_empty(),
        None => map.cells.get(r * size + c).is_some_and(|&v| v != 0),
      };
      for row in 0..32 {
        for x in 0..32 {
          let bit = near && (near_rows.unwrap()[row] >> x & 1) != 0;
          out[(r * 32 + row) * side + c * 32 + x] = (all || bit) as u8;
        }
      }
    }
  }
  out
}

/// The land hiding the ocean around the document's levels, from such masks (side x side): `doc`
/// its levels', `around` the levels the game shows with them, `far` views of the map from afar
/// (the one placed like the document's levels, if one is); with `water` (ocean_water), the water
/// the land closes in too. Never in the box of the document's own land.
pub fn ocean_land_around(
  side: usize,
  doc: &[Vec<u8>],
  around: &[Vec<u8>],
  far: &[Vec<u8>],
  water: &[u8],
) -> Vec<u8> {
  let count = side * side;
  let (mut own, mut out) = (vec![0u8; count], vec![0u8; count]);
  for d in doc.iter().filter(|d| d.len() == count) {
    for i in 0..count {
      own[i] |= d[i];
    }
  }
  for a in around.iter().filter(|a| a.len() == count) {
    for i in 0..count {
      out[i] |= a[i];
    }
  }
  // the document's area: the box of its land
  let (mut x0, mut x1, mut z0, mut z1) = (side as i32, -1i32, side as i32, -1i32);
  for z in 0..side {
    for x in 0..side {
      if own[z * side + x] != 0 {
        x0 = x0.min(x as i32);
        x1 = x1.max(x as i32);
        z0 = z0.min(z as i32);
        z1 = z1.max(z as i32);
      }
    }
  }
  let in_box = |f: &mut dyn FnMut(usize)| {
    for z in z0.max(0)..=z1 {
      for x in x0.max(0)..=x1 {
        f(z as usize * side + x as usize);
      }
    }
  };
  // the view from afar placed like the document's levels (some are models moved or scaled): the
  // one whose land matches theirs best in their box, if it does
  // ponytail: a match of 0.35, from the city's (0.41 placed, 0.30 scaled, 0.16 moved)
  let mut best: Option<&Vec<u8>> = None;
  let mut best_match = 0.35;
  for f in far.iter().filter(|f| f.len() == count) {
    let (mut both, mut either) = (0usize, 0usize);
    in_box(&mut |i| {
      both += (own[i] != 0 && f[i] != 0) as usize;
      either += (own[i] != 0 || f[i] != 0) as usize;
    });
    if either > 0 && both as f64 / either as f64 > best_match {
      best_match = both as f64 / either as f64;
      best = Some(f);
    }
  }
  // of it, the land the document's stands on, as far as it goes (not islands the game does not
  // show from there)
  if let Some(best) = best {
    let mut seen = vec![0u8; count];
    let mut todo: Vec<usize> = Vec::new();
    for i in 0..count {
      if own[i] != 0 {
        seen[i] = 1;
        todo.push(i);
      }
    }
    while let Some(i) = todo.pop() {
      if best[i] != 0 {
        out[i] = 1;
      }
      let (x, z) = ((i % side) as i32, (i / side) as i32);
      for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let (xx, zz) = (x + dx, z + dz);
        if xx < 0 || xx >= side as i32 || zz < 0 || zz >= side as i32 {
          continue;
        }
        let j = zz as usize * side + xx as usize;
        if seen[j] == 0 && best[j] != 0 {
          seen[j] = 1;
          todo.push(j);
        }
      }
    }
  }
  // in their box, the document's own pieces hide the water, or show it (under a bridge, where the
  // user took a piece away)
  let clear_box = |out: &mut Vec<u8>| {
    for z in z0.max(0)..=z1 {
      for x in x0.max(0)..=x1 {
        out[z as usize * side + x as usize] = 0;
      }
    }
  };
  clear_box(&mut out);
  // land thinner than 27 m (a wall in the sea, a bridge) keeps the water around it, a gap as thin
  // in the land (a street of a view from afar) none
  let grow = |out: &mut Vec<u8>, land: bool| {
    // by 4 squares: along x, then along z
    let pass = |from: &[u8], along_x: bool| {
      let mut to = vec![0u8; count];
      let clamp = |v: usize, k: i32| (v as i32 + k).clamp(0, side as i32 - 1) as usize;
      for z in 0..side {
        for x in 0..side {
          let mut v = !land;
          for k in -4..=4 {
            let (xx, zz) = if along_x {
              (clamp(x, k), z)
            } else {
              (x, clamp(z, k))
            };
            if (from[zz * side + xx] != 0) == land {
              v = land;
            }
          }
          to[z * side + x] = v as u8;
        }
      }
      to
    };
    let tmp = pass(out, true);
    *out = pass(&tmp, false);
  };
  grow(&mut out, false);
  grow(&mut out, true);
  grow(&mut out, true);
  grow(&mut out, false);
  clear_box(&mut out);
  // the water the land closes in (squares of the map's ocean under the city, a canal cut off):
  // none; the sea goes on to the map's edge, or into the document's own box
  if water.len() == count {
    let mut seen = vec![0u8; count];
    let mut todo: Vec<usize> = Vec::new();
    let reach = |x: usize, z: usize, seen: &mut Vec<u8>, todo: &mut Vec<usize>| {
      let i = z * side + x;
      if water[i] != 0 && out[i] == 0 && seen[i] == 0 {
        seen[i] = 1;
        todo.push(i);
      }
    };
    for k in 0..side {
      reach(k, 0, &mut seen, &mut todo);
      reach(k, side - 1, &mut seen, &mut todo);
      reach(0, k, &mut seen, &mut todo);
      reach(side - 1, k, &mut seen, &mut todo);
    }
    for z in z0.max(0)..=z1 {
      for x in x0.max(0)..=x1 {
        reach(x as usize, z as usize, &mut seen, &mut todo);
      }
    }
    while let Some(i) = todo.pop() {
      let (x, z) = (i % side, i / side);
      if x > 0 {
        reach(x - 1, z, &mut seen, &mut todo);
      }
      if x < side - 1 {
        reach(x + 1, z, &mut seen, &mut todo);
      }
      if z > 0 {
        reach(x, z - 1, &mut seen, &mut todo);
      }
      if z < side - 1 {
        reach(x, z + 1, &mut seen, &mut todo);
      }
    }
    for i in 0..count {
      if water[i] != 0 && out[i] == 0 && seen[i] == 0 {
        out[i] = 1;
      }
    }
  }
  out
}

/// The ocean of the document (App's m_ocean...).
#[derive(Default)]
pub(crate) struct OceanState {
  /// a mesh at height 0, drawn at the ocean's height
  pub mesh: Option<Arc<Mesh>>,
  /// what the mesh was made for: the map's name, with the land's key when masked
  map: String,
  pub height: f32,
  /// the far ocean drawn: a level of the document asks for it
  pub far: bool,
  pub far_color: Vec3,
  /// its colors: the map's, lifted; the far ocean's
  pub color: Vec3,
  pub deep: Vec3,
  /// the land of the levels the game shows around the document's, read on a worker: no water
  /// drawn there (in the game, their ground hides it); its key: the map, the height, the levels
  land: Vec<u8>,
  land_key: String,
  land_want: String,
}

impl Editor {
  pub fn update_ocean(&mut self) {
    let data = self.document_game_data();
    // like the game: the ocean of a level shown with an ocean map (the level opened first, then
    // the others of the document), the far ocean when one of them asks for it
    let mut ocean_level = self.scene.settings.level.clone();
    let mut info = data.as_ref().and_then(|d| d.level(&ocean_level));
    self.ocean.far = false;
    let levels = self.document_levels();
    if let Some(d) = &data {
      for l in levels.iter().filter(|l| l.role != LevelRole::Layer) {
        if l.group != 0 && self.scene.find(l.group).is_some_and(|g| !g.visible) {
          continue;
        }
        let Some(li) = d.level(&l.name) else { continue };
        self.ocean.far |= li.ocean_far;
        if info.is_none_or(|i| i.ocean.is_empty()) && !li.ocean.is_empty() {
          info = Some(li);
          ocean_level = l.name.clone();
        }
      }
    }
    let (Some(d), Some(info)) = (&data, info) else {
      return self.no_ocean();
    };
    let Some(map) = d.ocean(&info.ocean).filter(|m| m.size > 0) else {
      return self.no_ocean();
    };
    let map_name = info.ocean.clone();
    self.ocean.far_color = map.far_color;
    let step = if self.story.enabled {
      self.story.step_of(&ocean_level)
    } else {
      1
    };
    self.ocean.height = d.ocean_height(&ocean_level, step);
    // the land of the levels the game shows around the document's (their neighbors, and the city
    // seen from afar on the same map) hides the water there in the game; read on a worker from the
    // library, with the document's levels as extracted (where they are)
    let in_document = |name: &str| levels.iter().any(|l| l.name == name);
    // the document's levels (=), around them (+), from afar (~)
    let mut lists: [Vec<String>; 3] = Default::default();
    let mut want = strf!("%s@%.2f", &map_name, self.ocean.height);
    let library = self.library.clone();
    let mut add = |to: usize, name: &str, lists: &mut [Vec<String>; 3]| {
      // a level the game loads under several names (ctywide-ff, ctywide-kg): its library's
      let e = library
        .level(name)
        .or_else(|| library.level(name.split('-').next().unwrap_or_default()));
      let Some(e) = e else { return };
      if to != 0 && (in_document(&e.name) || in_document(name)) {
        return;
      }
      if lists.iter().any(|v| v.contains(&e.background)) {
        return;
      }
      lists[to].push(e.background.clone());
      want.push_str(&format!(" {}{}", ["=", "+", "~"][to], e.name));
    };
    for l in levels.iter().filter(|l| l.role != LevelRole::Layer) {
      add(0, &l.name, &mut lists);
    }
    for l in levels.iter().filter(|l| l.role != LevelRole::Layer) {
      for n in d
        .level(&l.name)
        .map(|i| i.neighbors.clone())
        .unwrap_or_default()
      {
        add(1, &n, &mut lists);
      }
    }
    for (name, li) in d.levels() {
      if li.ocean == map_name {
        add(2, name, &mut lists);
      }
    }
    let [doc, around, far] = lists;
    if want != self.ocean.land_key && want != self.ocean.land_want && !self.busy("ocean_land") {
      self.ocean.land_want = want.clone();
      let (m, y) = (map.clone(), self.ocean.height);
      self.run_job(
        "ocean_land",
        String::new(),
        move || {
          let read = |paths: &[String], afar: bool| -> Vec<Vec<u8>> {
            let mut out = Vec::new();
            for path in paths {
              let mut s = Scene::new();
              if import_gltf(&mut s, path, &ImportOptions::default()).is_err() {
                continue;
              }
              // a view from afar has hills kilometers wide around: a backdrop, not the land there
              if afar {
                for i in 0..s.nodes.len() {
                  let b = if s.nodes[i].mesh.is_some() {
                    s.world_bounds(&s.nodes[i])
                  } else {
                    Default::default()
                  };
                  if b.valid() && (b.hi.x - b.lo.x).max(b.hi.z - b.lo.z) > 2000.0 {
                    s.nodes[i].mesh = None;
                  }
                }
              }
              out.push(ocean_land(&m, y, &s));
            }
            out
          };
          ocean_land_around(
            m.size * 32,
            &read(&doc, false),
            &read(&around, false),
            &read(&far, true),
            &ocean_water(&m),
          )
        },
        move |ed, land| {
          ed.ocean.land = land;
          ed.ocean.land_key = std::mem::take(&mut ed.ocean.land_want);
          ed.update_ocean();
        },
      );
    }
    let masked = want == self.ocean.land_key && !self.ocean.land.is_empty();
    let key = if masked { want } else { map_name };
    if self.ocean.mesh.is_some() && self.ocean.map == key {
      return;
    }
    self.ocean.map = key;
    let mut water = ocean_water(map);
    if masked && self.ocean.land.len() == water.len() {
      for (w, l) in water.iter_mut().zip(&self.ocean.land) {
        *w &= (*l == 0) as u8;
      }
    }
    self.ocean.mesh = Some(ocean_mesh(map, &water));
    // its colors: the map's (0x80 is 1.0), lifted, and the far ocean's
    let lift = |v: f32| (v * 1.6 + 0.08).min(1.0);
    self.ocean.color = Vec3::new(lift(map.color.x), lift(map.color.y), lift(map.color.z));
    self.ocean.deep = map.far_color;
  }

  fn no_ocean(&mut self) {
    self.ocean.mesh = None;
    self.ocean.map.clear();
    self.ocean.far = false;
  }

  /// get_state's `ocean` (the viewport's setOcean), null without one
  pub(crate) fn ocean_state(&self) -> Value {
    let Some(mesh) = self
      .ocean
      .mesh
      .as_ref()
      .filter(|_| !self.scene.nodes.is_empty())
    else {
      return Value::Null;
    };
    let o = &self.ocean;
    json!({
      "mesh": mesh.uid as u32,
      "version": mesh.version,
      "height": o.height,
      "look": {
        "color": o.color,
        "deep": o.deep,
        "sky": [0.5, 0.62, 0.72],
        "sun_dir": [0.32, 0.92, 0.23],
        "sun_color": [1.0, 0.95, 0.85],
        "alpha": 0.62,
      },
      "far": o.far,
      "far_color": o.far_color,
    })
  }

  pub fn ocean_mesh(&self) -> Option<&Arc<Mesh>> {
    self.ocean.mesh.as_ref()
  }

  pub fn ocean_height(&self) -> Option<f32> {
    self.ocean.mesh.as_ref().map(|_| self.ocean.height)
  }
}

// the water: the mid ocean's cells, and the near ocean's water by the shores (3 m squares) where
// the game draws it instead, less the land around; a quad per run of water along x, at height 0
// (drawn at the ocean's height)
fn ocean_mesh(map: &OceanMap, water: &[u8]) -> Arc<Mesh> {
  let mut mesh = Mesh::new();
  mesh.name = "ocean".into();
  let mut p = Primitive::default();
  let size = map.size;
  let side = size * 32;
  let mut quad = |x0: f32, x1: f32, z0: f32, z1: f32| {
    let base = p.pos.len() as u32;
    p.pos.extend([
      Vec3::new(x0, 0.0, z0),
      Vec3::new(x1, 0.0, z0),
      Vec3::new(x1, 0.0, z1),
      Vec3::new(x0, 0.0, z1),
    ]);
    p.nrm.extend([Vec3::new(0.0, 1.0, 0.0); 4]);
    p.idx
      .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
  };
  // the 32 rows of water squares of a cell (bit x of row z); true when it is water everywhere
  let rows = |r: usize, c: usize, out: &mut [u32; 32]| {
    let mut whole = true;
    for (row, bits) in out.iter_mut().enumerate() {
      let w = &water[(r * 32 + row) * side + c * 32..][..32];
      *bits = w
        .iter()
        .enumerate()
        .fold(0u32, |b, (x, &v)| b | (v as u32) << x);
      whole &= *bits == u32::MAX;
    }
    whole
  };
  let sq = map.cell / 32.0;
  let mut bits = [0u32; 32];
  for r in 0..size {
    let mut c = 0;
    while c < size {
      if !rows(r, c, &mut bits) {
        // part of the cell: its rows of water squares
        let cx = map.corner.x + c as f32 * map.cell;
        let cz = map.corner.z + r as f32 * map.cell;
        for (row, &b) in bits.iter().enumerate() {
          let mut x = 0;
          while x < 32 {
            if b >> x & 1 == 0 {
              x += 1;
              continue;
            }
            let x0 = x;
            while x < 32 && b >> x & 1 != 0 {
              x += 1;
            }
            quad(
              cx + x0 as f32 * sq,
              cx + x as f32 * sq,
              cz + row as f32 * sq,
              cz + (row + 1) as f32 * sq,
            );
          }
        }
        c += 1;
        continue;
      }
      let c0 = c;
      while c < size && rows(r, c, &mut bits) {
        c += 1;
      }
      let z0 = map.corner.z + r as f32 * map.cell;
      quad(
        map.corner.x + c0 as f32 * map.cell,
        map.corner.x + c as f32 * map.cell,
        z0,
        z0 + map.cell,
      );
    }
  }
  p.material = 0;
  mesh.prims.push(p);
  Arc::new(mesh)
}
