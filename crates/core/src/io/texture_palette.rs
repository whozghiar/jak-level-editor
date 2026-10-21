//! The palette of materials: the textures of Jak 3's decor that its levels use again and again
//! (city walls, metal floors, Spargus' sandstone, wood planks...), to give any element of a level
//! on the fly. Found by the library's index (how many pieces of each level draw each texture);
//! their images read from the levels' files alone (the extractor writes them as data URIs in the
//! glTF's JSON: no geometry is loaded), on a worker thread; the interface draws them as thumbnails.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

use crate::io::asset_library::AssetLibrary;
use crate::io::library_index::{LibraryIndex, glb_json_chunk};
use crate::scene::Texture;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PaletteTexture {
  /// the game's name of the texture ("city-ind-wall-05")
  pub name: String,
  /// "" Jak 3's, "jak2" Jak II's (named "jak2-<name>" in a document)
  pub game: String,
  /// the first word of its name, what it comes from ("city", "wascity", "des")
  pub theme: String,
  /// the level that draws it most: where its image is read
  pub background: String,
  /// the levels whose decor draws it
  pub levels: i32,
  /// the pieces drawing it in all the levels
  pub uses: i32,
}

/// Every texture the listed levels' decor draws, but the animated textures' slots and the far
/// scenery's (no material to build with), each with the level that draws it most.
fn level_textures(
  index: &LibraryIndex,
  library: &AssetLibrary,
  game: &str,
) -> BTreeMap<String, PaletteTexture> {
  let mut all: BTreeMap<String, PaletteTexture> = BTreeMap::new();
  // the most pieces one level draws it with
  let mut most: BTreeMap<&str, i32> = BTreeMap::new();
  for li in index.levels() {
    if !library.level(&li.level).is_some_and(|l| l.listed) {
      continue;
    }
    for (name, &n) in &li.textures {
      if name.starts_with("anim-slot") || name.contains("lowres") {
        continue;
      }
      let t = all.entry(name.clone()).or_default();
      t.name = name.clone();
      t.game = game.to_string();
      t.levels += 1;
      t.uses += n;
      let m = most.entry(name).or_default();
      if n > *m {
        *m = n;
        t.background = li.background.clone();
      }
    }
  }
  for (name, t) in &mut all {
    t.theme = name.split('-').next().unwrap_or_default().to_string();
  }
  all
}

/// the most shared first
fn by_sharing(a: &PaletteTexture, b: &PaletteTexture) -> std::cmp::Ordering {
  (b.levels, b.uses, &a.name).cmp(&(a.levels, a.uses, &b.name))
}

/// The textures drawn by the decor of at least two levels, or by many pieces of one, the most
/// shared first (at most `max`, 600 in the editor).
pub fn recurring_textures(
  index: &LibraryIndex,
  library: &AssetLibrary,
  max: usize,
) -> Vec<PaletteTexture> {
  // by theme (the sewers' many small levels would fill it alone): the most shared of each theme,
  // the themes most drawn first
  let mut themes: BTreeMap<String, Vec<PaletteTexture>> = BTreeMap::new();
  let mut drawn: BTreeMap<String, i64> = BTreeMap::new();
  for (_, t) in level_textures(index, library, "") {
    if t.levels < 2 && t.uses < 30 {
      continue;
    }
    *drawn.entry(t.theme.clone()).or_default() += t.uses as i64;
    themes.entry(t.theme.clone()).or_default().push(t);
  }
  let mut order: Vec<(i64, String)> = drawn.into_iter().map(|(theme, n)| (-n, theme)).collect();
  order.sort();
  let mut out = Vec::new();
  for (_, theme) in order {
    let mut list = themes.remove(&theme).unwrap_or_default();
    list.sort_by(by_sharing);
    let room = max.saturating_sub(out.len()).min(30);
    out.extend(list.into_iter().take(room));
  }
  out
}

/// Every texture the decor of the levels draws (but the animated textures' slots and the far
/// scenery's), the most shared first; `game`: "" for Jak 3's library, "jak2" for Jak II's.
pub fn all_textures(
  index: &LibraryIndex,
  library: &AssetLibrary,
  game: &str,
) -> Vec<PaletteTexture> {
  let mut out: Vec<PaletteTexture> = level_textures(index, library, game).into_values().collect();
  out.sort_by(by_sharing);
  out
}

pub(crate) fn base64_decode(input: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(input.len() * 3 / 4);
  let (mut acc, mut bits) = (0u32, 0);
  for &c in input {
    let v = match c {
      b'A'..=b'Z' => c - b'A',
      b'a'..=b'z' => c - b'a' + 26,
      b'0'..=b'9' => c - b'0' + 52,
      b'+' => 62,
      b'/' => 63,
      _ => continue, // padding, line breaks
    };
    acc = (acc << 6) | v as u32;
    bits += 6;
    if bits >= 8 {
      bits -= 8;
      out.push((acc >> bits) as u8);
    }
  }
  out
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ImagesDoc {
  images: Option<Vec<DocImage>>,
  #[serde(rename = "bufferViews")]
  buffer_views: Vec<DocBufferView>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocImage {
  name: String,
  uri: String,
  #[serde(rename = "bufferView")]
  buffer_view: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DocBufferView {
  #[serde(rename = "byteOffset")]
  byte_offset: usize,
  #[serde(rename = "byteLength")]
  byte_length: usize,
}

/// The images named `names` of a level's background file, read from its JSON (RGBA). Those it
/// does not have are left out.
pub fn read_level_textures(background: &str, names: &[String]) -> Result<Vec<Texture>, String> {
  let (text, mut file) = glb_json_chunk(background)?;
  let doc: ImagesDoc = serde_json::from_slice(&text).unwrap_or_default();
  drop(text);
  let Some(images) = doc.images else {
    return Err(format!("no images in {background}"));
  };
  // the binary chunk, for images kept there (the extractor writes data URIs)
  let mut bin: Option<Vec<u8>> = None;
  let wanted: BTreeSet<&str> = names.iter().map(String::as_str).collect();
  let mut out = Vec::new();
  for im in images {
    if !wanted.contains(im.name.as_str()) {
      continue;
    }
    let bytes = if let Some(rest) = im.uri.strip_prefix("data:") {
      rest.find(',').map_or_else(Vec::new, |comma| {
        base64_decode(&rest.as_bytes()[comma + 1..])
      })
    } else if let Some(bv) = im.buffer_view.and_then(|i| doc.buffer_views.get(i)) {
      let b = bin.get_or_insert_with(|| {
        let mut chunk = [0u8; 8];
        let mut data = Vec::new();
        if file.read_exact(&mut chunk).is_ok()
          && u32::from_le_bytes(chunk[4..8].try_into().unwrap()) == 0x004E4942
        {
          data.resize(
            u32::from_le_bytes(chunk[0..4].try_into().unwrap()) as usize,
            0,
          );
          if file.read_exact(&mut data).is_err() {
            data.clear();
          }
        }
        data
      });
      b.get(bv.byte_offset..bv.byte_offset + bv.byte_length)
        .map_or_else(Vec::new, <[u8]>::to_vec)
    } else {
      Vec::new()
    };
    let Ok(img) = image::load_from_memory(&bytes) else {
      continue;
    };
    let rgba = img.into_rgba8();
    out.push(Texture {
      name: im.name,
      width: rgba.width() as i32,
      height: rgba.height() as i32,
      rgba: rgba.into_raw(),
      ..Default::default()
    });
  }
  Ok(out)
}

struct Wanted {
  background: String,
  name: String,
  game: String,
}

/// The images of the palette, read when asked for (a level's at once).
#[derive(Default)]
pub struct TexturePalette {
  wanted: BTreeMap<String, Wanted>,
  /// by key; an empty one when its level did not have it
  images: BTreeMap<String, Texture>,
  work: Option<JoinHandle<Vec<Texture>>>,
  /// the keys being read
  working: Vec<String>,
  /// their game
  working_game: String,
}

impl TexturePalette {
  /// by key: the name, "jak2:" before Jak II's (both games have textures of the same names)
  fn key(name: &str, game: &str) -> String {
    if game.is_empty() {
      name.to_string()
    } else {
      format!("{game}:{name}")
    }
  }

  /// The image of a texture of the palette: None while it is read (it is asked for: `update`).
  pub fn image(&mut self, t: &PaletteTexture) -> Option<&Texture> {
    let k = Self::key(&t.name, &t.game);
    if !self.images.contains_key(&k) {
      if !self.working.contains(&k) {
        let w = Wanted {
          background: t.background.clone(),
          name: t.name.clone(),
          game: t.game.clone(),
        };
        self.wanted.insert(k, w);
      }
      return None;
    }
    self.images.get(&k).filter(|i| i.width > 0) // read (or not there)
  }

  /// Regularly: takes the images read, starts reading the next level's.
  pub fn update(&mut self) {
    if self.work.as_ref().is_some_and(JoinHandle::is_finished) {
      let read = self.work.take().unwrap().join().unwrap_or_default();
      for t in read {
        self
          .images
          .insert(Self::key(&t.name, &self.working_game), t);
      }
      // what the level did not have: not asked again
      for k in self.working.drain(..) {
        self.images.entry(k).or_default();
      }
    }
    if self.work.is_some() {
      return;
    }
    // the images of one level at once: its file is read once
    let Some(first) = self.wanted.values().next() else {
      return;
    };
    let background = first.background.clone();
    self.working_game = first.game.clone();
    let mut names = Vec::new();
    self.wanted.retain(|k, w| {
      if w.background != background {
        return true;
      }
      self.working.push(k.clone());
      names.push(std::mem::take(&mut w.name));
      false
    });
    self.work = Some(std::thread::spawn(move || {
      read_level_textures(&background, &names).unwrap_or_default()
    }));
  }

  /// images being read
  pub fn busy(&self) -> bool {
    self.work.is_some() || !self.wanted.is_empty()
  }

  /// another library
  pub fn clear(&mut self) {
    if let Some(work) = self.work.take() {
      let _ = work.join();
    }
    self.working.clear();
    self.wanted.clear();
    self.images.clear();
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;
  use crate::io::asset_library::game_folder;
  use crate::io::test_library::{make_box, temp_dir, write_rip_level, write_text};
  use crate::scene::{Material, Node, NodeKind, Scene};

  /// a level whose shell draws `textures` (a piece each, `copies` times the first one)
  fn level(game: &std::path::Path, name: &str, textures: &[&str], copies: usize) -> bool {
    let mut bg = Scene::new();
    let tfrag = bg.add(Node {
      name: format!("{name}-tfrag-0"),
      ..Default::default()
    });
    let mut extras = json!({});
    for (i, t) in textures.iter().enumerate() {
      let texture = bg.add_texture(Texture {
        name: t.to_string(),
        width: 2,
        height: 2,
        rgba: vec![i as u8 * 50 + 10; 16],
        ..Default::default()
      });
      let material = bg.add_material(Material {
        name: t.to_string(),
        texture,
        ..Default::default()
      });
      for c in 0..if i == 0 { copies } else { 1 } {
        let piece = format!("{name}-{t}-{c}");
        extras[&piece] = json!({"og_terrain": false});
        bg.add(Node {
          name: piece,
          kind: NodeKind::Render,
          parent: tfrag,
          mesh: Some(make_box(t, material)),
          ..Default::default()
        });
      }
    }
    write_rip_level(game, name, &bg, &json!([]), 2, &extras, &json!({}))
  }

  #[test]
  fn palette_of_the_levels() {
    let root = temp_dir("texture-palette");
    let game = root.join("jak3");
    write_text(&game.join("library.json"), r#"{"game": "jak3"}"#);
    assert!(level(
      &game,
      "lva",
      &["city-wall-01", "anim-slot-1", "far-lowres-sky", "lva-only"],
      2
    ));
    assert!(level(&game, "lvb", &["city-wall-01", "des-sand-02"], 40));
    let mut library = AssetLibrary::default();
    library.scan(&game_folder(root.to_str().unwrap())).unwrap();
    let mut index = LibraryIndex::default();
    index.start(&library, "", false);
    index.wait();
    assert_eq!(index.levels()[0].textures["city-wall-01"], 2);
    // shared by two levels, or drawn by 30 pieces of one: the palette; every texture apart
    let palette = recurring_textures(&index, &library, 600);
    let names: Vec<&str> = palette.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["city-wall-01"]);
    let wall = &palette[0];
    assert!(
      wall.theme == "city"
        && wall.levels == 2
        && wall.uses == 42
        && wall.background.contains("lvb"),
      "{wall:?}"
    );
    let all = all_textures(&index, &library, "jak2");
    let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["city-wall-01", "des-sand-02", "lva-only"]);
    assert!(all.iter().all(|t| t.game == "jak2"));

    // their images, read when asked for
    let read =
      read_level_textures(&wall.background, &["des-sand-02".into(), "none".into()]).unwrap();
    assert!(read.len() == 1 && read[0].width == 2 && read[0].rgba[0] == 60);
    let mut p = TexturePalette::default();
    let missing = PaletteTexture {
      name: "none".into(),
      ..wall.clone()
    };
    assert!(p.image(wall).is_none() && p.image(&missing).is_none() && p.busy());
    while p.busy() {
      p.update();
      std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let img = p.image(wall).unwrap();
    assert!(img.width == 2 && img.height == 2 && img.rgba[..4] == [10, 10, 10, 10]);
    assert!(
      p.image(&missing).is_none() && !p.busy(),
      "what a level lacks is not asked again"
    );
    // Jak II's apart from Jak 3's of the same name
    let jak2 = PaletteTexture {
      game: "jak2".into(),
      ..wall.clone()
    };
    assert!(p.image(&jak2).is_none() && p.busy());
    p.clear();
    assert!(!p.busy() && p.image(wall).is_none());
  }
}
