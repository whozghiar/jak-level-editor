//! What the catalog needs of every level of the library without loading them: the decor parts of
//! each level (the decor of all the levels, searched by name) and the art-names of its actors (the
//! models and particle effects they choose by name). Read from the JSON of the levels' background
//! files only, not their geometry, on a worker thread, and kept in a cache folder (the editor's:
//! <preferences>/index) so that a level is read again only when its file changes.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::i18n::tr;
use crate::io::asset_library::AssetLibrary;
use crate::io::catalog::{CatalogPart, PartKind, add_piece, piece_key, same_piece};
use crate::scene::instance_tree;
use crate::{log_info, log_warn};

/// a new one reads every level again
const K_INDEX_VERSION: i64 = 8;

#[derive(Clone, Debug, Default)]
pub struct LevelIndex {
  /// the folder ("desert")
  pub level: String,
  /// its background file
  pub background: String,
  /// the file's date when it was read
  pub stamp: i64,
  /// as list_parts() gives them for the loaded level
  pub parts: Vec<CatalogPart>,
  /// per actor type: the art-names of its actors, how many have each and the data (lump) of the
  /// first one
  pub art_names: BTreeMap<String, BTreeMap<String, i32>>,
  pub art_lumps: BTreeMap<String, BTreeMap<String, Value>>,
  /// the textures of its decor: how many pieces draw each (the textures used again and again by
  /// the levels make the palette of materials: io/texture_palette)
  pub textures: BTreeMap<String, i32>,
}

/// A decor part of a level of the library, for the decor of all the levels.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DecorEntry {
  /// the folder it comes from
  pub level: String,
  /// that level's background file
  pub background: String,
  /// its date (thumbnails)
  pub stamp: i64,
  pub part: CatalogPart,
  /// the name in lower case, for the filter
  pub key: String,
  /// the levels that have a prototype of that name
  pub levels: i32,
}

fn cache_file(dir: &str, background: &str) -> String {
  let mut h: u64 = 1469598103934665603;
  for &c in background.as_bytes() {
    h = (h ^ c as u64).wrapping_mul(1099511628211);
  }
  format!("{dir}/{h:016x}.json")
}

fn to_json(l: &LevelIndex) -> Value {
  let parts: Vec<Value> = l
    .parts
    .iter()
    .map(|p| {
      json!([
        p.name,
        p.mesh_node,
        (p.kind == PartKind::Piece) as i32,
        p.triangles,
        p.copies,
        p.key,
        p.surface,
        p.edges
      ])
    })
    .collect();
  json!({"version": K_INDEX_VERSION, "background": l.background, "stamp": l.stamp,
         "parts": parts, "art_names": l.art_names, "art_lumps": l.art_lumps,
         "textures": l.textures})
}

/// what a cache file holds (None when it is not this version's or is damaged)
fn from_json(j: &Value) -> Option<LevelIndex> {
  if j.get("version").and_then(Value::as_i64) != Some(K_INDEX_VERSION) {
    return None;
  }
  let mut out = LevelIndex::default();
  for p in j.get("parts")?.as_array()? {
    let p = p.as_array().filter(|p| p.len() == 8)?;
    let s = |i: usize| p[i].as_str().map(str::to_string);
    out.parts.push(CatalogPart {
      name: s(0)?,
      mesh_node: s(1)?,
      kind: if p[2].as_i64()? != 0 {
        PartKind::Piece
      } else {
        PartKind::Prototype
      },
      triangles: p[3].as_u64()? as usize,
      copies: p[4].as_i64()? as i32,
      key: s(5)?,
      surface: p[6].as_f64()?,
      edges: p[7].as_f64()?,
    });
  }
  for (etype, names) in j.get("art_names")?.as_object()? {
    for (name, n) in names.as_object()? {
      out
        .art_names
        .entry(etype.clone())
        .or_default()
        .insert(name.clone(), n.as_i64()? as i32);
    }
  }
  if let Some(lumps) = j.get("art_lumps").and_then(Value::as_object) {
    for (etype, names) in lumps {
      for (name, lump) in names.as_object()? {
        out
          .art_lumps
          .entry(etype.clone())
          .or_default()
          .insert(name.clone(), lump.clone());
      }
    }
  }
  if let Some(textures) = j.get("textures").and_then(Value::as_object) {
    for (name, n) in textures {
      out.textures.insert(name.clone(), n.as_i64()? as i32);
    }
  }
  Some(out)
}

/// The JSON chunk of a .glb (header: magic, version, length, then the chunk's length and type), and
/// the file left where the next chunk starts (the binary one)
pub(crate) fn glb_json_chunk(path: &str) -> Result<(Vec<u8>, File), String> {
  let not_glb = || format!("not a glTF binary file: {path}");
  let mut f = File::open(path).map_err(|_| not_glb())?;
  let mut head = [0u8; 20];
  f.read_exact(&mut head).map_err(|_| not_glb())?;
  let word = |i: usize| u32::from_le_bytes(head[i * 4..i * 4 + 4].try_into().unwrap());
  if word(0) != 0x46546C67 || word(4) != 0x4E4F534A {
    return Err(not_glb());
  }
  let mut text = vec![0u8; word(3) as usize];
  f.read_exact(&mut text)
    .map_err(|_| format!("truncated file: {path}"))?;
  while text.last().is_some_and(|&c| c == 0 || c == b' ') {
    text.pop(); // the chunk's padding
  }
  Ok((text, f))
}

// what the index reads of a level's glTF (the rest, its images first, skipped unread)
#[derive(Deserialize, Default)]
#[serde(default)]
struct Doc {
  nodes: Vec<DocNode>,
  meshes: Vec<DocMesh>,
  accessors: Vec<DocAccessor>,
  materials: Vec<DocMaterial>,
  textures: Vec<DocTexture>,
  images: Vec<DocImage>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocNode {
  name: String,
  mesh: Option<i64>,
  children: Vec<i64>,
  extras: Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocMesh {
  primitives: Vec<DocPrimitive>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocPrimitive {
  indices: Option<i64>,
  attributes: Option<DocAttributes>,
  material: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocAttributes {
  #[serde(rename = "POSITION")]
  position: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocAccessor {
  count: usize,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocMaterial {
  #[serde(rename = "pbrMetallicRoughness")]
  pbr: Option<DocPbr>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocPbr {
  #[serde(rename = "baseColorTexture")]
  base_color_texture: Option<DocTextureRef>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocTextureRef {
  index: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocTexture {
  source: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocImage {
  name: String,
}

fn at<T>(v: &[T], i: i64) -> Option<&T> {
  usize::try_from(i).ok().and_then(|i| v.get(i))
}

/// Reads a level's background file (its JSON chunk only): its parts, the art-names of its actors,
/// the textures of its decor (`level`, `background` and `stamp` are left to the caller).
pub fn index_level(background: &str) -> Result<LevelIndex, String> {
  let (text, _) = glb_json_chunk(background)?;
  let j: Doc =
    serde_json::from_slice(&text).map_err(|_| format!("unreadable glTF JSON: {background}"))?;
  drop(text);
  let mut out = LevelIndex::default();
  let n = j.nodes.len();

  // the texture each material draws (by its image's name), and how many pieces draw each
  let material_texture: Vec<&str> = j
    .materials
    .iter()
    .map(|m| {
      let ti = m
        .pbr
        .as_ref()
        .and_then(|p| p.base_color_texture.as_ref())
        .and_then(|t| t.index)
        .unwrap_or(-1);
      at(&j.textures, ti)
        .and_then(|t| at(&j.images, t.source.unwrap_or(-1)))
        .map_or("", |im| im.name.as_str())
    })
    .collect();
  for node in &j.nodes {
    let Some(mesh) = node.mesh.and_then(|mi| at(&j.meshes, mi)) else {
      continue;
    };
    for prim in &mesh.primitives {
      if let Some(&t) = at(&material_texture, prim.material.unwrap_or(-1))
        && !t.is_empty()
      {
        *out.textures.entry(t.to_string()).or_default() += 1;
      }
    }
  }

  // the names the importer gives, the hierarchy, the triangles of a mesh
  let names: Vec<String> = (0..n)
    .map(|i| match j.nodes[i].name.as_str() {
      "" => format!("node-{i}"),
      name => name.to_string(),
    })
    .collect();
  let mut parent = vec![-1i64; n];
  let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
  for (i, node) in j.nodes.iter().enumerate() {
    for &c in &node.children {
      if c < 0 || c as usize >= n {
        continue;
      }
      parent[c as usize] = i as i64;
      children[i].push(c as usize);
    }
  }
  let mesh_of = |i: usize| j.nodes[i].mesh.unwrap_or(-1);
  let triangles = |mesh: i64| -> usize {
    let Some(mesh) = at(&j.meshes, mesh) else {
      return 0;
    };
    let mut count = 0;
    for prim in &mesh.primitives {
      let mut acc = prim.indices.unwrap_or(-1);
      if acc < 0
        && let Some(a) = &prim.attributes
      {
        acc = a.position.unwrap_or(-1);
      }
      if let Some(a) = at(&j.accessors, acc) {
        count += a.count / 3;
      }
    }
    count
  };

  // the parts, as list_parts() finds them in the loaded level (a piece's copies once, by the
  // measures the extractor writes: og_measure)
  let (mut prototypes, mut pieces) = (Vec::new(), Vec::new());
  let mut piece_of: BTreeMap<String, Vec<usize>> = BTreeMap::new();
  for i in 0..n {
    let extras = &j.nodes[i].extras;
    if extras.get("kind").and_then(Value::as_str) == Some("actor") {
      let etype = extras.get("etype").and_then(Value::as_str).unwrap_or("");
      if let Some(lump) = extras.get("lump").filter(|l| l.is_object())
        && !etype.is_empty()
        && let Some(name) = lump.get("art-name").and_then(Value::as_str)
      {
        let count = out
          .art_names
          .entry(etype.to_string())
          .or_default()
          .entry(name.to_string())
          .or_default();
        if *count == 0 {
          out
            .art_lumps
            .entry(etype.to_string())
            .or_default()
            .insert(name.to_string(), lump.clone());
        }
        *count += 1;
      }
    }
    let Ok(p) = usize::try_from(parent[i]) else {
      continue;
    };
    let pname = &names[p];
    let mesh = mesh_of(i);
    if instance_tree(pname) {
      let mut part = CatalogPart {
        kind: PartKind::Prototype,
        name: names[i].clone(),
        ..Default::default()
      };
      if mesh >= 0 {
        part.mesh_node = names[i].clone();
        part.triangles = triangles(mesh);
        part.copies = 1;
      } else {
        for &c in &children[i] {
          let cm = mesh_of(c);
          if cm < 0 {
            continue;
          }
          if part.mesh_node.is_empty() {
            part.mesh_node = names[c].clone();
            part.triangles = triangles(cm);
          }
          part.copies += 1;
        }
      }
      if !part.mesh_node.is_empty() {
        prototypes.push(part);
      }
    } else if pname.contains("-tfrag-") && mesh >= 0 {
      if extras.get("og_terrain").and_then(Value::as_bool) != Some(false) {
        continue;
      }
      let mut part = CatalogPart {
        kind: PartKind::Piece,
        name: names[i].clone(),
        mesh_node: names[i].clone(),
        triangles: triangles(mesh),
        copies: 1,
        ..Default::default()
      };
      let measure = extras
        .get("og_measure")
        .and_then(Value::as_array)
        .filter(|m| m.len() == 2);
      let Some(measure) = measure else {
        pieces.push(part); // an older extraction: no measures, every piece shown
        continue;
      };
      part.key = piece_key(&names[i], part.triangles);
      part.surface = measure[0].as_f64().unwrap_or(0.0);
      part.edges = measure[1].as_f64().unwrap_or(0.0);
      add_piece(&mut pieces, &mut piece_of, part);
    }
  }
  prototypes.sort_by(|a, b| a.name.cmp(&b.name));
  pieces.sort_by(|a, b| a.name.cmp(&b.name));
  prototypes.append(&mut pieces);
  out.parts = prototypes;
  Ok(out)
}

/// The index of a library, made on a worker thread (`start`), taken once it is done (`update`).
#[derive(Default)]
pub struct LibraryIndex {
  work: Option<JoinHandle<Option<Vec<LevelIndex>>>>,
  cancel: Arc<AtomicBool>,
  done: Arc<AtomicUsize>,
  total: usize,
  levels: Vec<LevelIndex>,
  ready: bool,
}

impl Drop for LibraryIndex {
  fn drop(&mut self) {
    self.stop();
  }
}

impl LibraryIndex {
  /// Reads the levels of `library` on a worker thread (the ones cached in `cache_dir` from there,
  /// "" for no cache), after forgetting the previous library's. The levels read are added to the
  /// cache when `write_cache` (not by captures and tests: their libraries are temporary).
  pub fn start(&mut self, library: &AssetLibrary, cache_dir: &str, write_cache: bool) {
    self.stop();
    self.levels.clear();
    let mut levels: Vec<LevelIndex> = library
      .levels()
      .iter()
      .map(|l| LevelIndex {
        level: l.name.clone(),
        background: l.background.clone(),
        stamp: l.mtime,
        ..Default::default()
      })
      .collect();
    self.total = levels.len();
    self.done = Arc::new(AtomicUsize::new(0));
    self.cancel = Arc::new(AtomicBool::new(false));
    let (done, cancel, cache_dir) = (
      self.done.clone(),
      self.cancel.clone(),
      cache_dir.to_string(),
    );
    self.work = Some(std::thread::spawn(move || {
      if !cache_dir.is_empty() && write_cache {
        let _ = std::fs::create_dir_all(&cache_dir);
      }
      let mut read = 0usize;
      for l in &mut levels {
        if cancel.load(Ordering::Relaxed) {
          return None;
        }
        let cache = if cache_dir.is_empty() {
          String::new()
        } else {
          cache_file(&cache_dir, &l.background)
        };
        let cached = (!cache.is_empty())
          .then(|| std::fs::read(&cache).ok())
          .flatten()
          .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
          .filter(|j| {
            j.get("background").and_then(Value::as_str).unwrap_or("") == l.background
              && j.get("stamp").and_then(Value::as_i64).unwrap_or(0) == l.stamp
          })
          .and_then(|j| from_json(&j));
        let fresh = cached.is_none();
        match cached.map_or_else(|| index_level(&l.background), Ok) {
          Ok(c) => {
            l.parts = c.parts;
            l.art_names = c.art_names;
            l.art_lumps = c.art_lumps;
            l.textures = c.textures;
            if fresh {
              if !cache.is_empty() && write_cache {
                let _ = std::fs::write(&cache, to_json(l).to_string());
              }
              read += 1;
            }
          }
          Err(e) => log_warn!("%s", e),
        }
        done.fetch_add(1, Ordering::Relaxed);
      }
      if read > 0 {
        log_info!(tr("log.index_read"), read);
      }
      Some(levels)
    }));
    self.ready = false;
  }

  /// Takes the worker's result once it is done: true when the index just became ready.
  pub fn update(&mut self) -> bool {
    if !self.work.as_ref().is_some_and(JoinHandle::is_finished) {
      return false;
    }
    self.wait();
    self.ready
  }

  /// Waits for the worker (tests, the remote control).
  pub fn wait(&mut self) {
    if let Some(work) = self.work.take()
      && let Ok(Some(levels)) = work.join()
    {
      self.levels = levels;
      self.ready = true;
    }
  }

  pub fn stop(&mut self) {
    self.cancel.store(true, Ordering::Relaxed);
    if let Some(work) = self.work.take() {
      let _ = work.join();
    }
    self.ready = false;
  }

  pub fn ready(&self) -> bool {
    self.ready
  }

  /// the levels read so far
  pub fn done(&self) -> usize {
    self.done.load(Ordering::Relaxed)
  }

  pub fn total(&self) -> usize {
    self.total
  }

  /// Once ready: every level of the library, in its order.
  pub fn levels(&self) -> &[LevelIndex] {
    &self.levels
  }
}

/// The decor parts of the levels `library` lists, in its order: a prototype once (from the first
/// level that has one), the pieces of every level.
pub fn all_decor(index: &LibraryIndex, library: &AssetLibrary) -> Vec<DecorEntry> {
  let mut out: Vec<DecorEntry> = Vec::new();
  if !index.ready() {
    return out;
  }
  // prototype -> its entry
  let mut first: BTreeMap<&str, usize> = BTreeMap::new();
  // a piece's key -> the entries of its copies
  let mut pieces: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
  for li in index.levels() {
    let Some(level) = library.level(&li.level).filter(|l| l.listed) else {
      continue;
    };
    for p in &li.parts {
      if p.kind == PartKind::Prototype {
        if let Some(&i) = first.get(p.name.as_str()) {
          out[i].levels += 1;
          continue;
        }
        first.insert(&p.name, out.len());
      } else if !p.key.is_empty() {
        // a copy of a piece another level has: once
        let alike = pieces.entry(&p.key).or_default();
        if let Some(&i) = alike.iter().find(|&&i| same_piece(&out[i].part, p)) {
          out[i].levels += 1;
          continue;
        }
        alike.push(out.len());
      }
      out.push(DecorEntry {
        level: li.level.clone(),
        background: li.background.clone(),
        stamp: level.mtime,
        part: p.clone(),
        key: p.name.to_ascii_lowercase(),
        levels: 1,
      });
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeSet;
  use std::time::Instant;

  use super::*;
  use crate::io::asset_library::file_mtime;
  use crate::io::catalog::{build_catalog, jak2_creatures, list_catalog_models};
  use crate::io::game_data::{GameData, game_data_path, level_listed};
  use crate::io::jak2_import::jak2_structure;
  use crate::io::test_library::real_library;
  use crate::io::texture_palette::{all_textures, recurring_textures};
  use crate::prefs::Prefs;

  /// JSON values alike but for the last digits of their numbers (two parsers)
  fn alike(a: &Value, b: &Value) -> bool {
    match (a, b) {
      (Value::Number(x), Value::Number(y)) => {
        let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
        (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)
      }
      (Value::Array(x), Value::Array(y)) => {
        x.len() == y.len() && x.iter().zip(y).all(|(x, y)| alike(x, y))
      }
      (Value::Object(x), Value::Object(y)) => {
        x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| alike(v, w)))
      }
      _ => a == b,
    }
  }

  /// The index made of a game's folder (`game`: "jak3", "jak2"), compared with the C++ editor's
  /// cache of it (<its preferences>/index/<game>) for every level whose file has not changed since.
  fn index_of_real(game: &str) -> Option<(AssetLibrary, LibraryIndex)> {
    let root = real_library()?;
    let mut library = AssetLibrary::default();
    library.scan(root.join(game).to_str().unwrap()).ok()?;
    let data = GameData::load(&game_data_path()).ok();
    let data = data.as_ref().filter(|_| game == "jak3");
    library.set_listed(|l| level_listed(&l.name, &l.level, l.collision_triangles as usize, data));
    let mut index = LibraryIndex::default();
    let t = Instant::now();
    index.start(&library, "", false);
    index.wait();
    let time = t.elapsed();
    assert!(index.ready() && index.levels().len() == library.levels().len());
    let parts: usize = index.levels().iter().map(|l| l.parts.len()).sum();
    eprintln!(
      "{game}: {} levels indexed in {:.2} s ({parts} parts)",
      index.levels().len(),
      time.as_secs_f64()
    );
    let cpp = format!("{}index/{game}", Prefs::old_dir());
    let (mut compared, mut stale) = (0, 0);
    for l in index.levels() {
      // its path as the C++ editor had it (the library's real folder)
      let real = std::fs::canonicalize(&l.background).unwrap();
      let real = real.to_string_lossy().replace('\\', "/");
      let Some(j) = std::fs::read(cache_file(&cpp, real.trim_start_matches("//?/")))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
      else {
        continue;
      };
      // the C++ editor's dates: 100 ns since 1601
      let stamp = file_mtime(&l.background) / 100 + 116444736000000000;
      if j["stamp"].as_i64() != Some(stamp) {
        stale += 1;
        continue;
      }
      let c = from_json(&j).unwrap();
      let sorted = |p: &[CatalogPart]| {
        let mut p = p.to_vec();
        p.sort_by(|a, b| {
          (&a.name, &a.mesh_node, &a.key, a.triangles, a.copies).cmp(&(
            &b.name,
            &b.mesh_node,
            &b.key,
            b.triangles,
            b.copies,
          ))
        });
        p
      };
      let (mine, theirs) = (sorted(&l.parts), sorted(&c.parts));
      assert_eq!(mine.len(), theirs.len(), "{}: parts", l.level);
      for (a, b) in mine.iter().zip(&theirs) {
        let close = |x: f64, y: f64| (x - y).abs() <= 1e-9 * x.abs().max(1.0);
        assert!(
          (&a.name, &a.mesh_node, a.kind, a.triangles, a.copies, &a.key)
            == (&b.name, &b.mesh_node, b.kind, b.triangles, b.copies, &b.key)
            && close(a.surface, b.surface)
            && close(a.edges, b.edges),
          "{}: {a:?} {b:?}",
          l.level
        );
      }
      assert_eq!(l.art_names, c.art_names, "{}: art-names", l.level);
      assert_eq!(l.textures, c.textures, "{}: textures", l.level);
      assert!(
        alike(&json!(l.art_lumps), &json!(c.art_lumps)),
        "{}: art lumps",
        l.level
      );
      compared += 1;
    }
    eprintln!(
      "{game}: {compared} levels the same as the C++ editor's index ({stale} changed since)"
    );
    Some((library, index))
  }

  #[test]
  fn real_library_index_and_catalog() {
    let Some((library, index)) = index_of_real("jak3") else {
      eprintln!("skipped: no library/jak3/library.json");
      return;
    };
    let data = GameData::load(&game_data_path()).unwrap();
    let t = Instant::now();
    let objects = build_catalog(&library, Some(&data), Some(&index));
    let models = list_catalog_models(&library, Some(&data), &objects, &BTreeSet::new());
    let decor = all_decor(&index, &library);
    let catalog_time = t.elapsed();
    let prototypes = decor
      .iter()
      .filter(|e| e.part.kind == PartKind::Prototype)
      .count();
    let families: BTreeSet<&str> = objects.iter().map(|o| o.family.as_str()).collect();
    let effects = objects.iter().filter(|o| o.family == "effect").count();
    let palette = recurring_textures(&index, &library, 600);
    let textures = all_textures(&index, &library, "");
    eprintln!(
      "jak3: {} objects ({effects} effects, families {families:?}), {} models, {} decor parts ({prototypes} prototypes, {} pieces), palette {} of {} textures; catalog in {:.2} s",
      objects.len(),
      models.len(),
      decor.len(),
      decor.len() - prototypes,
      palette.len(),
      textures.len(),
      catalog_time.as_secs_f64()
    );
    // the C++ editor's on the same library (2026-10-21): 747 objects (451 effects), 197 models,
    // 6261 decor parts; Jak II's 535 models, 7265 structures
    assert!(
      objects.len() > 500
        && effects > 300
        && families.contains("crate")
        && families.contains("water")
    );
    assert!(models.len() > 150 && prototypes > 1000 && decor.len() > prototypes);
    if let Ok(path) = std::env::var("JLE_DUMP_CATALOG") {
      std::fs::write(path, serde_json::to_string(&objects).unwrap()).unwrap();
    }
    assert!(palette.len() > 100 && palette.len() <= 600 && textures.len() > palette.len());
    assert!(
      objects
        .iter()
        .all(|o| o.lump.is_object() && o.lump.get("name").is_none()),
      "objects without their level's data"
    );

    let Some((library2, index2)) = index_of_real("jak2") else {
      return;
    };
    let structures = all_decor(&index2, &library2)
      .into_iter()
      .filter(|e| jak2_structure(&e.part))
      .count();
    let models2 = list_catalog_models(&library2, None, &[], &jak2_creatures());
    eprintln!("jak2: {structures} structures, {} models", models2.len());
    assert!(structures > 5000 && models2.len() > 300);
  }
}
