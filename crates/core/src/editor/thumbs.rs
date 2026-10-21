//! The pictures of the panels (render/thumbnails.cpp): the engine gives the interface what to draw
//! and keeps what it drew. `preview/<kind>?...` is a package of one mesh with its own materials
//! and textures (a model or a prefab, a decor part of a level, an object's icon, a shape); the
//! interface draws it offscreen (ui/render/thumbnails.ts) and stores the PNG with
//! `thumb_store {name, png}` in <preferences>/thumbs, served again as `thumb/<name>` (404 until
//! then), so each is drawn once. `palette?...` is the image of a texture of the games (the texture
//! browser's). docs/rust-architecture.md §4.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use super::protocol::{encode_mesh, encode_texture, materials_json};
use super::{Editor, lock};
use crate::io::asset_library::{load_model_asset, merge_mesh};
use crate::io::gltf::{ImportOptions, import_gltf};
use crate::io::texture_palette::PaletteTexture;
use crate::math::{AABB, Mat4};
use crate::prefs::Prefs;
use crate::scene::shapes::{make_shape, object_icon, shape_preset};
use crate::scene::{Material, Mesh, NodeKind, Scene, Texture};

/// the levels read for their parts' pictures (each a few hundred MB at most: two kept)
static LEVELS: Mutex<Vec<(PathBuf, Arc<Scene>)>> = Mutex::new(Vec::new());

/// a level's background, read once while its parts are drawn (one read at a time: the requests
/// of the same level wait for it)
fn level_scene(path: &PathBuf) -> Option<Arc<Scene>> {
  let mut levels = LEVELS.lock().unwrap_or_else(|e| e.into_inner());
  if let Some(i) = levels.iter().position(|(p, _)| p == path) {
    let l = levels.remove(i);
    levels.push(l.clone());
    return Some(l.1);
  }
  let mut s = Scene::new();
  import_gltf(&mut s, path, &ImportOptions::default()).ok()?;
  let s = Arc::new(s);
  levels.push((path.clone(), s.clone()));
  if levels.len() > 2 {
    levels.remove(0);
  }
  Some(s)
}

/// `JLEP` 1: `u32 "JLEP", u32 1, u32 json bytes`, the JSON (padded with spaces to 4 bytes:
/// `{materials, textures: [{version}], bounds: {lo, hi}, draws: [{mesh, world}]}`, the materials as
/// `materials`), `u32 meshes`, each `u32 bytes` and the mesh (JLEM), `u32 textures`, then each
/// texture as `texture/<i>` (`u32 width, u32 height`, RGBA). The meshes' materials index the
/// package's own; `draws` places the meshes (a level's), `bounds` is what the picture frames.
pub fn encode_package(
  draws: &[(&Arc<Mesh>, Mat4)],
  textures: &[Texture],
  materials: &[Material],
  bounds: Option<AABB>,
) -> Vec<u8> {
  // only what the meshes draw, renumbered; each mesh once
  let mut own = Scene::new();
  let mut meshes: Vec<(usize, Arc<Mesh>)> = Vec::new();
  let mut placed = Vec::with_capacity(draws.len());
  let mut b = AABB::default();
  for (mesh, world) in draws {
    let key = Arc::as_ptr(mesh) as usize;
    let i = match meshes.iter().position(|(k, _)| *k == key) {
      Some(i) => i,
      None => {
        meshes.push((key, merge_mesh(&mut own, mesh, textures, materials)));
        meshes.len() - 1
      }
    };
    b.add_box(&mesh.bounds().transformed(world));
    placed.push(json!({"mesh": i, "world": world}));
  }
  let b = bounds.filter(AABB::valid).unwrap_or(b);
  let mut j = materials_json(&own);
  j["bounds"] = json!({"lo": b.lo, "hi": b.hi});
  j["draws"] = json!(placed);
  let mut text = j.to_string().into_bytes();
  while text.len() % 4 != 0 {
    text.push(b' ');
  }
  let put = |out: &mut Vec<u8>, v: usize| out.extend_from_slice(&(v as u32).to_le_bytes());
  let mut out = Vec::with_capacity(16 + text.len());
  out.extend_from_slice(b"JLEP");
  put(&mut out, 1);
  put(&mut out, text.len());
  out.extend_from_slice(&text);
  put(&mut out, meshes.len());
  for (_, m) in &meshes {
    let bytes = encode_mesh(m);
    put(&mut out, bytes.len());
    out.extend_from_slice(&bytes);
  }
  put(&mut out, own.textures.len());
  for t in &own.textures {
    out.extend_from_slice(&encode_texture(t));
  }
  out
}

/// a texture at most `max` pixels across (a level's picture: hundreds of them, seen small)
fn shrunk(t: &Texture, max: i32) -> Texture {
  let k = (t.width.max(t.height) + max - 1) / max;
  if k <= 1 {
    return t.clone();
  }
  let (w, h) = ((t.width / k).max(1), (t.height / k).max(1));
  let mut rgba = Vec::with_capacity((w * h * 4) as usize);
  for y in 0..h {
    for x in 0..w {
      // the middle texel of the block
      let (sx, sy) = (
        (x * k + k / 2).min(t.width - 1),
        (y * k + k / 2).min(t.height - 1),
      );
      let at = ((sy * t.width + sx) * 4) as usize;
      rgba.extend_from_slice(t.rgba.get(at..at + 4).unwrap_or(&[255; 4]));
    }
  }
  Texture {
    width: w,
    height: h,
    rgba,
    ..t.clone()
  }
}

/// `preview/<kind>`: `model?path=` (a model or a prefab of the library), `part?path=<level
/// background>&node=<the node whose mesh it is>`, `level?path=<level background>` (all of it,
/// framed on where most of it is), `icon?etype=&family=` (an object without a model:
/// scene/shapes object_icon), `shape?preset=`. None when there is no such thing.
pub fn preview_package(
  editor: &Mutex<Editor>,
  kind: &str,
  param: &dyn Fn(&str) -> Option<String>,
) -> Option<Vec<u8>> {
  let file = |editor: &Mutex<Editor>| lock(editor).asset_path(&param("path")?);
  match kind {
    "model" => {
      let m = load_model_asset(&file(editor)?.to_string_lossy(), false).ok()?;
      Some(encode_package(
        &[(&m.mesh, Mat4::identity())],
        &m.textures,
        &m.materials,
        None,
      ))
    }
    "part" => {
      let level = level_scene(&file(editor)?)?;
      let node = param("node")?;
      let n = level
        .nodes
        .iter()
        .find(|n| n.name == node && n.mesh.is_some())?;
      Some(encode_package(
        &[(n.mesh.as_ref()?, Mat4::identity())],
        &level.textures,
        &level.materials,
        None,
      ))
    }
    "level" => {
      let level = level_scene(&file(editor)?)?;
      let draws: Vec<(&Arc<Mesh>, Mat4)> = level
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Render)
        .filter_map(|n| Some((n.mesh.as_ref()?, level.world(n))))
        .collect();
      let textures: Vec<Texture> = level.textures.iter().map(|t| shrunk(t, 32)).collect();
      let focus = level.focus_bounds();
      Some(encode_package(
        &draws,
        &textures,
        &level.materials,
        Some(focus),
      ))
    }
    "icon" => {
      let mesh = object_icon(&param("etype")?, &param("family").unwrap_or_default(), 0);
      Some(encode_package(&[(&mesh, Mat4::identity())], &[], &[], None))
    }
    "shape" => {
      let p = shape_preset(&param("preset")?)?;
      let mesh = make_shape(&p.params, -1);
      Some(encode_package(&[(&mesh, Mat4::identity())], &[], &[], None))
    }
    _ => None,
  }
}

/// a thumbnail's file: its name is the interface's hash of what it shows (hex digits)
fn thumb_path(name: &str) -> Option<PathBuf> {
  (!name.is_empty() && name.len() <= 32 && name.bytes().all(|b| b.is_ascii_hexdigit())).then(|| {
    PathBuf::from(Prefs::dir())
      .join("thumbs")
      .join(format!("{name}.png"))
  })
}

/// `thumb/<name>`: a PNG drawn before
pub fn thumb_file(name: &str) -> Option<Vec<u8>> {
  std::fs::read(thumb_path(name)?).ok()
}

/// `thumb_store {name, png (base64)}`: a thumbnail the interface drew, kept (not by a session
/// that leaves the preferences alone)
pub fn store_thumb(name: &str, png_base64: &str) -> Result<(), String> {
  let path = thumb_path(name).ok_or("name: hex digits")?;
  if crate::prefs::READ_ONLY.load(std::sync::atomic::Ordering::Relaxed) {
    return Ok(());
  }
  let png = crate::io::texture_palette::base64_decode(png_base64.as_bytes());
  if !png.starts_with(b"\x89PNG") {
    return Err("png: a PNG in base64".into());
  }
  if let Some(dir) = path.parent() {
    let _ = std::fs::create_dir_all(dir);
  }
  std::fs::write(&path, png).map_err(|e| e.to_string())
}

/// `palette?game=&background=&name=`: the image of a texture of the games, as `texture/<i>`;
/// waits while the palette reads it (its level's file once for all its textures), None when the
/// level has no such texture
pub fn palette_image(editor: &Mutex<Editor>, t: &PaletteTexture) -> Option<Vec<u8>> {
  let start = Instant::now();
  loop {
    {
      let mut ed = lock(editor);
      let palette = &mut ed.catalog.palette;
      if let Some(img) = palette.image(t) {
        return Some(encode_texture(img));
      }
      palette.update();
      if let Some(img) = palette.image(t) {
        return Some(encode_texture(img));
      }
      if !palette.busy() || start.elapsed() > Duration::from_secs(20) {
        return None;
      }
    }
    std::thread::sleep(Duration::from_millis(40));
  }
}

/// The plugin's routes of this file: `preview/<kind>`, `thumb/<name>`, `palette`: (media type,
/// bytes), None for a 404.
pub fn serve(
  editor: &Mutex<Editor>,
  route: &str,
  rest: &str,
  param: &dyn Fn(&str) -> Option<String>,
) -> Option<(&'static str, Vec<u8>)> {
  let bin = "application/octet-stream";
  match route {
    "preview" => preview_package(editor, rest, param).map(|b| (bin, b)),
    "thumb" => thumb_file(rest).map(|b| ("image/png", b)),
    "palette" => {
      let t = PaletteTexture {
        name: param("name")?,
        game: param("game").unwrap_or_default(),
        background: param("background")?,
        ..Default::default()
      };
      // a file of the library only
      lock(editor).asset_path(&t.background)?;
      palette_image(editor, &t).map(|b| (bin, b))
    }
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::test_library::make_box;

  #[test]
  fn a_package_holds_each_mesh_once_with_what_it_draws() {
    let mut s = Scene::new();
    let t = Texture {
      name: "big".into(),
      width: 64,
      height: 64,
      rgba: vec![200; 64 * 64 * 4],
      ..Default::default()
    };
    let ti = s.add_texture(t.clone());
    s.add_texture(Texture {
      name: "unused".into(),
      ..t.clone()
    });
    let m = s.add_material(Material {
      name: "stone".into(),
      texture: ti,
      ..Default::default()
    });
    let mesh = make_box("rock", m);
    let far = Mat4::translate(crate::math::Vec3::new(10.0, 0.0, 0.0));
    let small: Vec<Texture> = s.textures.iter().map(|t| shrunk(t, 32)).collect();
    let p = encode_package(
      &[(&mesh, Mat4::identity()), (&mesh, far)],
      &small,
      &s.materials,
      None,
    );
    let u = |o: usize| u32::from_le_bytes(p[o..o + 4].try_into().unwrap()) as usize;
    assert_eq!(&p[0..4], b"JLEP");
    let j: serde_json::Value = serde_json::from_slice(&p[12..12 + u(8)]).unwrap();
    let at = 12 + u(8);
    assert_eq!(u(at), 1, "one mesh for both draws");
    let tex = at + 4 + u(at + 4) + 4;
    assert_eq!(u(tex), 1, "only the texture it draws");
    assert_eq!((u(tex + 4), u(tex + 8)), (32, 32), "shrunk");
    assert!(
      j["draws"].as_array().unwrap().len() == 2 && j["bounds"]["hi"][0].as_f64() > Some(10.0)
    );
  }
}
