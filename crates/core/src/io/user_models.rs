//! The user's own models, made in Blender or another program and imported as glTF: copied into
//! the library (next to prefabricated/), each in a folder of its own, listed first in the catalog's
//! Models ("My models") and placed as decor; what could go wrong is checked when one is placed
//! (its node's og_import: {"game": "user", "file", "checks"}).
//!
//!   models/<name>/<name>.glb     (or <name>.gltf with the files it names: its .bin, its images)
//!
//! Importing a model of the same name again replaces it (the model edited in Blender); what was
//! placed of it stays as it was.

use std::path::Path;

use serde_json::{Value, json};

use crate::io::asset_library::{LoadedMesh, from_path};
use crate::io::catalog::CatalogModel;
use crate::io::decor_behaviors::{DecorBehavior, decor_behavior_json};
use crate::io::gltf::{FileSummary, file_summary};
use crate::scene::Texture;

/// The group of the user's models in the catalog's Models
pub const MY_MODELS: &str = "my-models";

/// A file name of `stem`: what Windows and the game's tools accept
fn clean_name(stem: &str) -> String {
  let s: String = stem
    .trim()
    .chars()
    .map(|c| {
      if c.is_alphanumeric() || c == '-' || c == '_' {
        c
      } else {
        '-'
      }
    })
    .collect();
  if s.is_empty() { "model".into() } else { s }
}

/// Copies the model at `src` (.glb, or .gltf with the files it names) into `folder`: its path in
/// the library. A model of the library given again is left as it is.
pub fn import_model_file(folder: &str, src: &Path) -> Result<String, String> {
  let ext = src
    .extension()
    .map(|e| e.to_string_lossy().to_lowercase())
    .unwrap_or_default();
  if ext != "glb" && ext != "gltf" {
    return Err(format!("{}: not a glTF file (.glb, .gltf)", src.display()));
  }
  if folder.is_empty() {
    return Err("no library: choose it first (File > Library)".into());
  }
  let summary = file_summary(src)?;
  let name = clean_name(&src.file_stem().unwrap_or_default().to_string_lossy());
  let dir = Path::new(folder).join(&name);
  let to = dir.join(format!("{name}.{ext}"));
  let same =
    |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b);
  if same(src, &to) {
    return Ok(from_path(&to));
  }
  // the files it names first (a .gltf whose .bin is missing is not imported half)
  let from_dir = src.parent().unwrap_or(Path::new("."));
  for f in &summary.files {
    if Path::new(f).is_absolute() || f.split(['/', '\\']).any(|p| p == "..") {
      return Err(format!(
        "{}: names a file outside its folder ({f})",
        src.display()
      ));
    }
    if !from_dir.join(f).is_file() {
      return Err(format!(
        "{}: the file it names is missing ({f})",
        src.display()
      ));
    }
  }
  // the model edited and imported again: the old copy goes
  if dir.is_dir() {
    std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  }
  let copy = |from: &Path, to: &Path| -> Result<(), String> {
    if let Some(parent) = to.parent() {
      std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::copy(from, to)
      .map(|_| ())
      .map_err(|e| format!("{}: {e}", to.display()))
  };
  copy(src, &to)?;
  for f in &summary.files {
    copy(&from_dir.join(f), &dir.join(f))?;
  }
  Ok(from_path(&to))
}

/// The models of `folder`, by name
pub fn list_user_models(folder: &str) -> Vec<CatalogModel> {
  let mut out = Vec::new();
  let Ok(entries) = std::fs::read_dir(folder) else {
    return out;
  };
  for e in entries.flatten() {
    let name = e.file_name().to_string_lossy().into_owned();
    let found = ["glb", "gltf"]
      .iter()
      .map(|ext| e.path().join(format!("{name}.{ext}")))
      .find(|p| p.is_file());
    if let Some(path) = found {
      out.push(CatalogModel {
        name,
        path: from_path(&path),
        group: MY_MODELS.into(),
        game: "user".into(),
      });
    }
  }
  out.sort_by(|a, b| a.name.cmp(&b.name));
  out
}

/// An image made of a plain color (asset_library::plain_colors_as_textures)
fn plain(t: &Texture) -> bool {
  t.name.ends_with("-color") && t.width == 8 && t.height == 8
}

/// The triangles of collision the game gathers around Jak (docs/user-guide.md's appendix: 461, over
/// about 40 m² of ground)
const K_AROUND_JAK: f32 = 460.0;
const K_GROUND_AROUND_JAK: f32 = 40.0;

/// About how many triangles of a collision made from its shape Jak meets at once (the builder
/// doubles the decor's faces): over K_AROUND_JAK, he may fall through it
fn collision_around_jak(triangles: usize, ground: f32) -> f32 {
  2.0 * triangles as f32 * (K_GROUND_AROUND_JAK / ground.max(1.0)).min(1.0)
}

/// What the inspector shows of a model of the user's when it is placed (og_import's checks): its
/// mesh, its size, its materials of one color, its textures, its parts and animations, its
/// collision
pub fn user_model_checks(lm: &LoadedMesh, path: &str) -> Value {
  let s = file_summary(path).unwrap_or(FileSummary {
    parts: 1,
    ..Default::default()
  });
  let mesh = &lm.mesh;
  let triangles = mesh.tri_count();
  // a prim drawn with an image but without coordinates on it (a plain color needs none)
  let no_uv = mesh
    .prims
    .iter()
    .filter(|p| {
      let image = usize::try_from(p.material)
        .ok()
        .and_then(|m| lm.materials.get(m))
        .and_then(|m| usize::try_from(m.texture).ok())
        .and_then(|t| lm.textures.get(t))
        .is_some_and(|t| !plain(t));
      image && p.uv.len() != p.pos.len()
    })
    .count();
  let b = mesh.bounds();
  let size = if b.valid() {
    b.hi - b.lo
  } else {
    Default::default()
  };
  let largest = size.x.max(size.y).max(size.z);
  let plains = lm.textures.iter().filter(|t| plain(t)).count();
  let images: Vec<_> = lm.textures.iter().filter(|t| !plain(t)).collect();
  let biggest = images
    .iter()
    .map(|t| t.width.max(t.height))
    .max()
    .unwrap_or(0);
  let around = collision_around_jak(triangles, size.x * size.z);
  let mut checks = vec![
    json!({"check": "user_mesh", "triangles": triangles, "no_uv": no_uv, "ok": no_uv == 0}),
    json!({"check": "size", "x": size.x, "y": size.y, "z": size.z,
      "ok": (0.05..=500.0).contains(&largest)}),
  ];
  if !images.is_empty() {
    checks.push(
      json!({"check": "user_textures", "count": images.len(), "largest": biggest,
      "ok": biggest <= 1024}),
    );
  }
  if plains > 0 {
    checks.push(json!({"check": "plain", "count": plains, "ok": true}));
  }
  checks.push(json!({"check": "parts", "count": s.parts.max(1), "ok": true}));
  if s.animations > 0 || s.joints > 0 {
    checks
      .push(json!({"check": "animations", "count": s.animations, "joints": s.joints, "ok": true}));
  }
  checks.push(
    json!({"check": "collision", "triangles": triangles, "around": around,
    "box": around > K_AROUND_JAK, "ok": true}),
  );
  Value::Array(checks)
}

/// What the game does with a model of the user's when it is placed (its og_decor): it collides
/// (the game's small details do not, but a model of one's own is there to be met), by its shape,
/// or by the box around it when its shape would be more than the game gathers around Jak (he
/// would fall through it)
pub fn user_model_decor(checks: &Value) -> Value {
  let boxed = checks
    .as_array()
    .into_iter()
    .flatten()
    .any(|c| c["check"] == "collision" && c["box"] == true);
  let mut b = DecorBehavior::default();
  b.collision.set = true;
  b.collision.kind = if boxed { "box" } else { "shape" }.into();
  decor_behavior_json(&b)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::asset_library::load_model_asset;
  use crate::io::gltf::{ExportOptions, export_glb};
  use crate::io::test_library::make_box;
  use crate::scene::{Material, Node, NodeKind, Scene};

  #[test]
  fn imported_listed_checked_and_replaced() {
    let tmp = std::env::temp_dir().join(format!("jle-user-models-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // a model as Blender writes one: two objects, a material of one color (no image)
    let mut doc = Scene::new();
    let red = doc.add_material(Material {
      name: "Red".into(),
      base_color: crate::math::Vec4::new(0.8, 0.05, 0.05, 1.0),
      ..Default::default()
    });
    for (i, name) in ["Plank", "Post"].iter().enumerate() {
      let mut m = (*make_box(name, red)).clone();
      for p in &mut m.prims {
        for v in &mut p.pos {
          v.x += i as f32 * 2.0;
        }
      }
      doc.add(Node {
        name: name.to_string(),
        kind: NodeKind::Render,
        mesh: Some(std::sync::Arc::new(m)),
        ..Default::default()
      });
    }
    let src = tmp.join("My Fence.glb");
    export_glb(&doc, &src, &ExportOptions::default()).unwrap();
    let folder = from_path(&tmp.join("models"));
    let path = import_model_file(&folder, &src).unwrap();
    assert!(path.ends_with("models/My-Fence/My-Fence.glb"), "{path}");
    // the library's copy given again: left as it is
    assert_eq!(import_model_file(&folder, Path::new(&path)).unwrap(), path);
    let listed = list_user_models(&folder);
    assert_eq!(listed.len(), 1);
    assert_eq!(
      (listed[0].name.as_str(), listed[0].game.as_str()),
      ("My-Fence", "user")
    );
    // its color made an image (the game draws a material without one as a checker)
    let lm = load_model_asset(&path, false).unwrap();
    let m = &lm.materials[0];
    let t = &lm.textures[m.texture as usize];
    assert_eq!((t.width, m.base_color.x), (8, 1.0));
    assert!(t.rgba[0] > 200 && t.rgba[1] < 80, "{:?}", &t.rgba[..4]);
    let checks = user_model_checks(&lm, &path);
    let find = |k: &str| {
      checks
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check"] == k)
        .cloned()
    };
    assert_eq!(find("parts").unwrap()["count"], 2);
    assert_eq!(find("plain").unwrap()["count"], 1);
    assert!(find("animations").is_none() && find("user_textures").is_none());
    // it collides by its shape (24 triangles), a dense one by the box around it
    assert_eq!(user_model_decor(&checks)["collision"]["kind"], "shape");
    assert!(collision_around_jak(62976, 9.6) > K_AROUND_JAK);
    assert!(collision_around_jak(96, 3.6) < K_AROUND_JAK);
    let dense = json!([{"check": "collision", "box": true}]);
    assert_eq!(user_model_decor(&dense)["collision"]["kind"], "box");
    // not glTF
    assert!(import_model_file(&folder, &tmp.join("x.obj")).is_err());
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
