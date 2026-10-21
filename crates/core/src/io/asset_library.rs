//! The library: the folder ogle-extract writes to, the game in its subfolder jak3/.
//!
//!   <game>/library.json                           the game, its version, the levels extracted
//!   <game>/<level>/decor/<level>-background.glb   a level: its decor, collision and actors
//!   <game>/<level>/models/<model>-lodN.glb        the models of its actors
//!   <game>/<level>/level.json                     what the folder holds (dgo, actors, models)
//!   <game>/common/models/                         the models every level uses
//!   prefabricated/                                the prefabs (next to the game's folder): the
//!                                                 user's, and the game's in prefabricated/game/
//!
//! An AssetLibrary is the folder of the game. Scanning only lists file names; geometry is read
//! when a level is opened or a thumbnail is drawn. It holds no lock and no cell: an
//! `Arc<AssetLibrary>` goes to worker threads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::io::gltf::{ImportOptions, import_gltf};
use crate::log_info;
use crate::math::normalize;
use crate::scene::{Material, Mesh, NodeKind, Scene, Texture};

/// A path as the editor keeps it: '/' separated
pub fn from_path(p: &Path) -> String {
  p.to_string_lossy().replace('\\', "/")
}

/// The date of a file (nanoseconds since 1970), 0 when it has none: what tells a thumbnail or an
/// index made from it is still good
pub fn file_mtime(path: impl AsRef<Path>) -> i64 {
  std::fs::metadata(path)
    .and_then(|m| m.modified())
    .ok()
    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
    .map_or(0, |d| d.as_nanos() as i64)
}

fn read_json(path: &Path) -> Value {
  std::fs::read(path)
    .ok()
    .and_then(|b| serde_json::from_slice(&b).ok())
    .unwrap_or(Value::Null)
}

fn fnv(data: &[u8]) -> u64 {
  let mut h: u64 = 1469598103934665603;
  for &b in data {
    h = (h ^ b as u64).wrapping_mul(1099511628211);
  }
  h
}

/// "collectables-skill-lod0" -> ("collectables-skill", 0); (name, 0) without the suffix
pub fn strip_lod(name: &str) -> (&str, i32) {
  if let Some(p) = name.rfind("-lod")
    && p + 4 < name.len()
    && name[p + 4..].bytes().all(|b| b.is_ascii_digit())
  {
    return (&name[..p], name[p + 4..].parse().unwrap_or(0));
  }
  (name, 0)
}

// ------------------------------------------------------------------------------------------------
// the game's folder of a library folder
// ------------------------------------------------------------------------------------------------

fn absolute(p: &Path) -> PathBuf {
  std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The library folder of a path the user chose: the parent of a game folder, else the path itself.
pub fn library_root_of(folder: &str) -> String {
  let p = absolute(Path::new(folder));
  if p.join("library.json").exists() {
    from_path(p.parent().unwrap_or(&p))
  } else {
    from_path(&p)
  }
}

/// This editor's own tree when the program runs from it (a development build: target/<profile>/
/// of the source tree it was built from), else None: an installed editor has no folder of its own
/// to write to (C:\Program Files\... is not writable); its host gives the library's
/// (crate::host::Host::default_library). Never jak-project's, which this editor neither builds
/// against nor runs.
pub fn editor_tree() -> Option<PathBuf> {
  let project = absolute(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2)?);
  let exe = std::env::current_exe().ok()?;
  (project.join("Cargo.toml").is_file()
    && project.join("crates").join("core").is_dir()
    && exe.starts_with(&project))
  .then_some(project)
}

/// library/ in the editor's own tree (a development build), "" otherwise: the library's default
/// is then the host's (Editor::default_library).
pub fn default_library_root() -> String {
  editor_tree().map_or_else(String::new, |t| from_path(&t.join("library")))
}

/// Where the user's own disc images and disc folders go in a development build, so File > Extract
/// has a folder of its own to suggest: iso_data/ next to library/ (its .gitignore keeps them out of
/// this repository); "" otherwise.
pub fn default_iso_data_dir() -> String {
  editor_tree().map_or_else(String::new, |t| from_path(&t.join("iso_data")))
}

/// The folder of the extracted game in a library folder (<library>/jak3, absolute, '/' separated),
/// "" when the game was not extracted there. A folder that is itself the game's (it holds
/// library.json) is taken as the game folder of its parent.
pub fn game_folder(library_root: &str) -> String {
  if library_root.is_empty() {
    return String::new();
  }
  let folder = Path::new(&library_root_of(library_root)).join("jak3");
  if read_json(&folder.join("library.json")).is_object() {
    from_path(&folder)
  } else {
    String::new()
  }
}

// ------------------------------------------------------------------------------------------------
// the game's folder
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct LevelEntry {
  /// "desert" (the folder)
  pub name: String,
  /// the game's name of the level (level.json), "<level>" for a copy "<level>-<dgo>"
  pub level: String,
  /// "DST.DGO" (level.json)
  pub dgo: String,
  /// <level>/decor/<level>-background.glb
  pub background: String,
  /// level.json
  pub actors: i32,
  /// level.json, -1 when it does not say
  pub collision_triangles: i32,
  /// level.json: per actor type, how many and the data of the first one
  pub actor_types: Value,
  /// level.json: where the game runs a script when Jak or the camera goes in or out (the levels it
  /// loads and shows around this one), as the level builder's "region_trees"; null when unknown
  pub region_trees: Value,
  /// date of the background file (file_mtime)
  pub mtime: i64,
  /// shown in the Levels panel (see the editor's set_library)
  pub listed: bool,
}

impl Default for LevelEntry {
  fn default() -> Self {
    Self {
      name: String::new(),
      level: String::new(),
      dgo: String::new(),
      background: String::new(),
      actors: 0,
      collision_triangles: -1,
      actor_types: Value::Null,
      region_trees: Value::Null,
      mtime: 0,
      listed: true,
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelAsset {
  pub path: String,
  /// the level folder it is in ("common" for the shared models)
  pub group: String,
  /// file name without "-lodN"
  pub name: String,
  pub lod: i32,
}

#[derive(Clone, Debug, Default)]
pub struct AssetLibrary {
  root: String,
  levels: Vec<LevelEntry>,
  /// sorted by (name, group, lod)
  models: Vec<ModelAsset>,
}

impl AssetLibrary {
  pub fn scan(&mut self, game_folder: &str) -> Result<(), String> {
    self.clear();
    let base = absolute(Path::new(game_folder));
    if !base.is_dir() {
      return Err(format!("folder not found: {game_folder}"));
    }
    self.root = from_path(&base);
    let entries = std::fs::read_dir(&base).map_err(|e| format!("{game_folder}: {e}"))?;
    for dir in entries.flatten() {
      if !dir.file_type().is_ok_and(|t| t.is_dir()) {
        continue;
      }
      let name = dir.file_name().to_string_lossy().into_owned();
      if name == "prefabs" || name == "_disc" || name == "_work" {
        continue;
      }
      // the models of the folder
      if let Ok(files) = std::fs::read_dir(dir.path().join("models")) {
        for f in files.flatten() {
          let path = f.path();
          if !f.file_type().is_ok_and(|t| t.is_file())
            || path.extension().is_none_or(|e| e != "glb")
          {
            continue;
          }
          let stem = path.file_stem().unwrap_or_default().to_string_lossy();
          let (model, lod) = strip_lod(&stem);
          self.models.push(ModelAsset {
            path: from_path(&path),
            group: name.clone(),
            name: model.to_string(),
            lod,
          });
        }
      }
      // a level: its decor
      let background = dir
        .path()
        .join("decor")
        .join(format!("{name}-background.glb"));
      if !background.is_file() {
        continue;
      }
      let mut level = LevelEntry {
        name: name.clone(),
        background: from_path(&background),
        mtime: file_mtime(&background),
        ..Default::default()
      };
      let info = read_json(&dir.path().join("level.json"));
      if info.is_object() {
        let s = |k: &str| {
          info
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
        };
        let int = |k: &str, d: i32| info.get(k).and_then(Value::as_f64).map_or(d, |v| v as i32);
        level.level = s("level");
        level.dgo = s("dgo");
        level.actors = int("actors", 0);
        level.collision_triangles = int("collision_triangles", -1);
        if let Some(v) = info.get("actor_types") {
          level.actor_types = v.clone();
        }
        if let Some(v) = info.get("region_trees") {
          level.region_trees = v.clone();
        }
      }
      self.levels.push(level);
    }
    self.levels.sort_by(|a, b| a.name.cmp(&b.name));
    self
      .models
      .sort_by(|a, b| (&a.name, &a.group, a.lod).cmp(&(&b.name, &b.group, b.lod)));
    log_info!(
      "library %s: %zu levels, %zu models",
      &self.root,
      self.levels.len(),
      self.models.len()
    );
    Ok(())
  }

  pub fn clear(&mut self) {
    self.root.clear();
    self.levels.clear();
    self.models.clear();
  }

  pub fn is_empty(&self) -> bool {
    self.levels.is_empty()
  }

  pub fn root(&self) -> &str {
    &self.root
  }

  pub fn levels(&self) -> &[LevelEntry] {
    &self.levels
  }

  /// decides which levels the Levels panel lists
  pub fn set_listed(&mut self, listed: impl Fn(&LevelEntry) -> bool) {
    for l in &mut self.levels {
      l.listed = listed(l);
    }
  }

  pub fn level(&self, name: &str) -> Option<&LevelEntry> {
    self.levels.iter().find(|l| l.name == name)
  }

  pub fn level_of(&self, background_path: &str) -> Option<&LevelEntry> {
    let p = background_path.replace('\\', "/");
    self.levels.iter().find(|l| l.background == p)
  }

  /// <library>/prefabricated: the prefabs, kept out of git (they hold the game's decor)
  pub fn prefab_folder(&self) -> String {
    match self.root.rfind('/') {
      _ if self.root.is_empty() => String::new(),
      Some(at) => format!("{}/prefabricated", &self.root[..at]),
      None => format!("{}/prefabricated", self.root),
    }
  }

  /// by name, group, lod
  pub fn models(&self) -> &[ModelAsset] {
    &self.models
  }

  /// the models of that name, lowest lod first in each group
  fn named(&self, name: &str) -> &[ModelAsset] {
    let start = self.models.partition_point(|m| m.name.as_str() < name);
    let end = start + self.models[start..].partition_point(|m| m.name == name);
    &self.models[start..end]
  }

  /// lowest lod of a model name, the copy of `prefer_group` first
  pub fn model_named(&self, name: &str, prefer_group: &str) -> Option<&ModelAsset> {
    let named = self.named(name);
    named
      .iter()
      .find(|m| m.group == prefer_group)
      .or(named.first())
  }

  /// every copy of a model (a level folder each, its lowest lod): the same model saved with the
  /// levels that load it, not all with its animations
  pub fn models_named(&self, name: &str) -> Vec<&ModelAsset> {
    let mut out: Vec<&ModelAsset> = Vec::new();
    for m in self.named(name) {
      if out.last().is_none_or(|l| l.group != m.group) {
        out.push(m);
      }
    }
    out
  }

  /// The model drawn for an actor type, from file names only:
  ///   1. a model named like the type (vin-door-ctyinda),
  ///   2. the only model whose name starts with "<etype>-" (crate -> crate-krimson),
  ///   3. the only model whose name ends with "-<etype>" (skill -> collectables-skill),
  ///
  /// looking in `prefer_group` first, then in the whole game.
  pub fn model_for_etype(&self, etype: &str, prefer_group: &str) -> Option<&ModelAsset> {
    if etype.is_empty() {
      return None;
    }
    if let Some(m) = self.model_named(etype, prefer_group) {
      return Some(m);
    }
    // the only model name matching, in the preferred folder, then anywhere
    let unique = |matches: &dyn Fn(&str) -> bool, only_group: bool| -> Option<&ModelAsset> {
      let mut found: Option<&str> = None;
      for m in &self.models {
        if (only_group && m.group != prefer_group) || !matches(&m.name) {
          continue;
        }
        if found.is_some_and(|f| f != m.name) {
          return None; // ambiguous
        }
        found = Some(&m.name);
      }
      self.model_named(found?, prefer_group)
    };
    let (prefix, suffix) = (format!("{etype}-"), format!("-{etype}"));
    let starts = |n: &str| n.starts_with(&prefix);
    let ends = |n: &str| n.ends_with(&suffix);
    for only_group in [true, false] {
      if only_group && prefer_group.is_empty() {
        continue;
      }
      if let Some(m) = unique(&starts, only_group).or_else(|| unique(&ends, only_group)) {
        return Some(m);
      }
    }
    None
  }
}

// ------------------------------------------------------------------------------------------------
// loading (thread safe: a worker fills these, the editor merges them into a document)
// ------------------------------------------------------------------------------------------------

/// A mesh with its own textures and materials (primitive materials index `materials`, material
/// textures index `textures`).
#[derive(Clone, Debug)]
pub struct LoadedMesh {
  pub mesh: Arc<Mesh>,
  pub textures: Vec<Texture>,
  pub materials: Vec<Material>,
}

/// Every render node of the file baked into one mesh, in the file's space (a model in its bind
/// pose, a prefab around its pivot). `skin`: each vertex's joints and weights kept (an animated
/// model's: scene/skeleton).
pub fn load_model_asset(path: &str, skin: bool) -> Result<LoadedMesh, String> {
  let mut tmp = Scene::new();
  import_gltf(
    &mut tmp,
    path,
    &ImportOptions {
      skin,
      ..Default::default()
    },
  )?;
  let mut mesh = Mesh::new();
  let stem = Path::new(path)
    .file_stem()
    .unwrap_or_default()
    .to_string_lossy();
  mesh.name = strip_lod(&stem).0.to_string();
  let cache = tmp.cache();
  for (i, n) in tmp.nodes.iter().enumerate() {
    let Some(m) = &n.mesh else { continue };
    if n.kind != NodeKind::Render {
      continue;
    }
    let w = cache.worlds[i];
    let nm = w.inverse().transposed();
    for p in &m.prims {
      let mut p = p.clone();
      for v in &mut p.pos {
        *v = w.point(*v);
      }
      for v in &mut p.nrm {
        *v = normalize(nm.dir(*v));
      }
      p.tri_pat.clear();
      mesh.prims.push(p);
    }
  }
  if mesh.prims.is_empty() {
    return Err(format!("no geometry in {path}"));
  }
  Ok(LoadedMesh {
    mesh: Arc::new(mesh),
    textures: std::mem::take(&mut tmp.textures),
    materials: std::mem::take(&mut tmp.materials),
  })
}

/// Copies a mesh into a document: textures (deduplicated by name and content) and materials are
/// appended, primitive materials remapped. The returned mesh belongs to the document.
pub fn merge_mesh(
  doc: &mut Scene,
  mesh: &Mesh,
  textures: &[Texture],
  materials: &[Material],
) -> Arc<Mesh> {
  let mut tex_map: HashMap<i32, i32> = HashMap::new();
  let mut mat_map: HashMap<i32, i32> = HashMap::new();
  let mut texture = |doc: &mut Scene, ti: i32| -> i32 {
    let Some(t) = usize::try_from(ti).ok().and_then(|i| textures.get(i)) else {
      return -1;
    };
    *tex_map.entry(ti).or_insert_with(|| {
      let h = fnv(&t.rgba);
      doc
        .textures
        .iter()
        .position(|d| {
          d.name == t.name && d.width == t.width && d.height == t.height && fnv(&d.rgba) == h
        })
        .map_or_else(|| doc.add_texture(t.clone()), |i| i as i32)
    })
  };
  let mut out = mesh.clone();
  for p in &mut out.prims {
    let mi = p.material;
    p.material = match usize::try_from(mi).ok().and_then(|i| materials.get(i)) {
      None => -1,
      Some(_) if mat_map.contains_key(&mi) => mat_map[&mi],
      Some(src) => {
        let mut m = src.clone();
        m.texture = texture(doc, m.texture);
        let found = doc.materials.iter().position(|d| {
          d.name == m.name
            && d.texture == m.texture
            && d.alpha == m.alpha
            && d.blend == m.blend
            && d.translucent == m.translucent
            && d.cutoff == m.cutoff
            && d.double_sided == m.double_sided
            && d.base_color == m.base_color
        });
        let found = found.map_or_else(|| doc.add_material(m), |i| i as i32);
        mat_map.insert(mi, found);
        found
      }
    };
  }
  out.touch();
  Arc::new(out)
}
