//! Prefabs: elements of a level saved together (a building and its props, a group of crates...)
//! to be put again in any level. Each prefab is a .glb in the prefab folder
//! (<library>/prefabricated/): the elements with their hierarchy, around a pivot at the bottom
//! center of their bounds, so that a prefab sits on the surface it is placed on.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::io::asset_library::{file_mtime, from_path, merge_mesh};
use crate::io::gltf::{ExportMode, ExportOptions, ImportOptions, export_glb, import_gltf};
use crate::math::{AABB, Mat4, Transform, Vec3};
use crate::scene::{Mesh, NodeKind, Scene};

#[derive(Clone, Debug)]
pub struct PrefabInfo {
  /// file name without .glb (a game prefab: its recipe's name)
  pub name: String,
  pub path: String,
  pub mtime: i64,
  /// a prefab of the game (io/prefab_recipes): its category and what it is, its game ("" Jak 3,
  /// "jak2") and the level it comes from; made: its file is there (they are made level by level,
  /// when they are shown or placed)
  pub category: String,
  pub description: String,
  pub game: String,
  pub level: String,
  pub made: bool,
}

impl Default for PrefabInfo {
  fn default() -> Self {
    Self {
      name: String::new(),
      path: String::new(),
      mtime: 0,
      category: String::new(),
      description: String::new(),
      game: String::new(),
      level: String::new(),
      made: true,
    }
  }
}

pub fn list_prefabs(folder: &str) -> Vec<PrefabInfo> {
  let mut out = Vec::new();
  let Ok(entries) = std::fs::read_dir(folder) else {
    return out;
  };
  for e in entries.flatten() {
    let path = e.path();
    if !e.file_type().is_ok_and(|t| t.is_file()) || path.extension().is_none_or(|x| x != "glb") {
      continue;
    }
    out.push(PrefabInfo {
      name: path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned(),
      path: from_path(&path),
      mtime: file_mtime(&path),
      ..Default::default()
    });
  }
  out.sort_by(|a, b| a.name.cmp(&b.name));
  out
}

/// A file name for a prefab name ("Pipes & crates" -> "Pipes_crates.glb"), not taken in `folder`.
pub fn new_prefab_path(folder: &str, name: &str) -> String {
  let mut base = String::new();
  for c in name.chars() {
    if c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii() {
      base.push(c);
    } else if !base.is_empty() && !base.ends_with('_') {
      base.push('_');
    }
  }
  let base = match base.trim_end_matches('_') {
    "" => "prefab",
    b => b,
  };
  let mut path = format!("{folder}/{base}.glb");
  let mut i = 2;
  while Path::new(&path).exists() {
    path = format!("{folder}/{base}-{i}.glb");
    i += 1;
  }
  path
}

/// Writes `roots` (and their descendants) to a prefab file.
pub fn save_prefab(scene: &Scene, roots: &[u32], path: &str) -> Result<(), String> {
  if roots.is_empty() {
    return Err("nothing selected".into());
  }
  // the pivot: bottom center of everything the prefab holds
  let mut b = AABB::default();
  for (&id, below) in roots.iter().zip(scene.descendants_of(roots)) {
    for id in std::iter::once(id).chain(below) {
      if let Some(n) = scene.find(id) {
        b.add_box(&scene.world_bounds(n));
      }
    }
  }
  let mut o = ExportOptions {
    mode: ExportMode::Prefab,
    only: roots.to_vec(),
    ..Default::default()
  };
  if b.valid() {
    o.pivot = Mat4::translate(Vec3::new(b.center().x, b.lo.y, b.center().z));
  }
  if let Some(dir) = Path::new(path).parent() {
    let _ = std::fs::create_dir_all(dir);
  }
  export_glb(scene, path, &o)
}

pub fn load_prefab(path: &str, out: &mut Scene) -> Result<(), String> {
  import_gltf(out, path, &ImportOptions::default()).map(|_| ())
}

/// Puts a prefab in a document at `m`: new nodes, the roots at the top of the document, named
/// apart from the document's (its actors like the game names them: <type>-<number>, in their data
/// too). Meshes are merged into the document once (`merged`, per prefab mesh: `mesh_key`) and
/// shared by later placements. Returns the new roots.
pub fn place_prefab(
  doc: &mut Scene,
  prefab: &Scene,
  m: &Mat4,
  merged: &mut HashMap<usize, Arc<Mesh>>,
) -> Vec<u32> {
  let mut ids: HashMap<u32, u32> = HashMap::with_capacity(prefab.nodes.len());
  for n in &prefab.nodes {
    ids.insert(n.id, doc.next_id);
    doc.next_id += 1;
  }
  let mut names: HashSet<String> = doc.nodes.iter().map(|n| n.name.clone()).collect();
  let mut free_name = |base: &str, numbered: bool| {
    let mut name = if numbered {
      format!("{base}-1")
    } else {
      base.to_string()
    };
    let mut i = 2;
    while names.contains(&name) {
      name = format!("{base}-{i}");
      i += 1;
    }
    names.insert(name.clone());
    name
  };
  let mut roots = Vec::new();
  for src in &prefab.nodes {
    let mut n = src.clone();
    n.id = ids[&src.id];
    n.parent = match ids.get(&src.parent) {
      Some(&p) if src.parent != 0 => p,
      _ => 0,
    };
    if n.parent == 0 {
      n.local = Transform::from_matrix(&(*m * src.local.matrix()));
      roots.push(n.id);
    }
    // two actors of a level cannot share a name (the game finds them by it): a placed actor is
    // named like the game names them, <type>-<number>; the other roots apart from the document's
    let etype = if n.kind == NodeKind::Actor {
      n.extras
        .get("etype")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
    } else {
      String::new()
    };
    if !etype.is_empty() {
      n.name = free_name(&etype, true);
      if let Some(lump) = n.extras.get_mut("lump").and_then(|l| l.as_object_mut()) {
        lump.insert("name".into(), n.name.clone().into());
      }
    } else if n.parent == 0 {
      n.name = free_name(&n.name, false);
    }
    if let Some(mesh) = &src.mesh {
      let shared = merged
        .entry(Arc::as_ptr(mesh) as usize)
        .or_insert_with(|| merge_mesh(doc, mesh, &prefab.textures, &prefab.materials));
      n.mesh = Some(shared.clone());
    }
    if let Some(extras) = n.extras.as_object_mut() {
      // a placed actor is a new one: the game gives it its id
      if n.kind == NodeKind::Actor {
        extras.remove("aid");
      }
      // the groups it was made of: the prefab placed is one (the editor's place_at)
      extras.remove("og_group");
    }
    doc.add(n);
  }
  roots
}
