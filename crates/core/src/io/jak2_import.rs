//! The structures of Jak II (buildings, the palace, pillars, cables: the decor parts of its levels)
//! brought into Jak 3 levels. The extraction writes Jak II's levels to <library>/jak2/ with the same
//! writer as Jak 3 (meters, the same axes, vertex colors, materials and collision layout), so a
//! part keeps its geometry as it is; what could break is checked and converted when it comes in:
//!
//!   names      its textures are renamed "jak2-<name>" unless the document has the same image under
//!              that name (the two games share most texture names with slightly different pixels:
//!              one name for two images would make the level builder and texture packs mix them)
//!   mesh       triangles, indices, texture coordinates and vertex colors where the game has them
//!   lighting   its vertex colors (the brightest of its level's light palettes, 0.5 neutral)
//!              against the Jak 3 decor of the document: more than a quarter brighter or darker is
//!              said, and can be matched (its colors scaled)
//!   materials  the alpha and blend modes the export writes for Jak 3 (blended: cut out)
//!   textures   8-bit RGBA, power-of-two sizes up to 256 as the game's
//!
//! A part brought in has no actor, no story, no collision of its own (the level builder makes it
//! from its shape) and keeps in its node extras og_import: {"game": "jak2", "level", "part",
//! "checks": [{"check": "mesh" | "colors" | "lighting" | "materials" | "textures" | "metadata",
//! "ok", and its numbers}]}.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::io::asset_library::merge_mesh;
use crate::io::catalog::CatalogPart;
use crate::scene::{AlphaMode, Material, Mesh, Node, NodeKind, Scene, Texture};

fn prefixed(s: &str) -> bool {
  s.starts_with("jak2-")
}

fn pow2(v: i32) -> bool {
  v > 0 && (v & (v - 1)) == 0
}

// the part of the document that does not come from Jak II
fn from_jak2(n: &Node) -> bool {
  n.extras
    .get("og_import")
    .and_then(|i| i.get("game"))
    .and_then(Value::as_str)
    == Some("jak2")
}

/// Whether a decor part of Jak II is one of its structures, to offer: not the low-detail scenery a
/// level draws far away (its "-lowres" prototypes: mountains, the city seen from the palace).
pub fn jak2_structure(part: &CatalogPart) -> bool {
  !part.name.contains("lowres")
}

/// The mean brightness of a mesh's vertex colors, 0 when it has none.
pub fn mesh_lighting(mesh: &Mesh) -> f32 {
  let (mut sum, mut count) = (0.0f64, 0usize);
  for c in mesh.prims.iter().flat_map(|p| &p.col) {
    sum += (c.x + c.y + c.z) as f64 / 3.0;
    count += 1;
  }
  if count > 0 {
    (sum / count as f64) as f32
  } else {
    0.0
  }
}

/// The mean brightness of the vertex colors of the document's decor that does not come from Jak II
/// (0.5 is neutral), 0 when it has none.
pub fn document_lighting(doc: &Scene) -> f32 {
  let (mut sum, mut count) = (0.0f64, 0usize);
  let mut seen: HashSet<*const Mesh> = HashSet::new();
  for n in &doc.nodes {
    let Some(mesh) = &n.mesh else { continue };
    if n.kind != NodeKind::Render || from_jak2(n) || !seen.insert(Arc::as_ptr(mesh)) {
      continue;
    }
    for c in mesh.prims.iter().flat_map(|p| &p.col) {
      sum += (c.x + c.y + c.z) as f64 / 3.0;
      count += 1;
    }
  }
  if count > 0 {
    (sum / count as f64) as f32
  } else {
    0.0
  }
}

/// The lighting check: the part's brightness against the document's (its ratio, 1 when either has
/// none); within a quarter is fine.
pub fn lighting_ratio(part: &Mesh, doc: &Scene) -> f32 {
  let (a, b) = (mesh_lighting(part), document_lighting(doc));
  if a > 0.0 && b > 0.0 { a / b } else { 1.0 }
}

/// A copy of the mesh with its vertex colors scaled by `k` (kept within 0..1).
pub fn scaled_colors(mesh: &Mesh, k: f32) -> Arc<Mesh> {
  let mut out = mesh.clone();
  for c in out.prims.iter_mut().flat_map(|p| &mut p.col) {
    c.x = (c.x * k).clamp(0.0, 1.0);
    c.y = (c.y * k).clamp(0.0, 1.0);
    c.z = (c.z * k).clamp(0.0, 1.0);
  }
  out.touch();
  Arc::new(out)
}

/// A Jak II mesh and its textures and materials made part of a Jak 3 document; `checks` gets what
/// was checked (og_import's "checks").
pub fn import_jak2_mesh(
  doc: &mut Scene,
  mesh: &Mesh,
  textures: &[Texture],
  materials: &[Material],
  checks: Option<&mut Value>,
) -> Arc<Mesh> {
  let material = |i: i32| usize::try_from(i).ok().and_then(|i| materials.get(i));
  // the textures it draws: renamed unless the document has the very same image under that name
  let (mut used_materials, mut used_textures) = (BTreeSet::new(), BTreeSet::new());
  for p in &mesh.prims {
    if let Some(m) = material(p.material) {
      used_materials.insert(p.material as usize);
      if let Ok(t) = usize::try_from(m.texture)
        && t < textures.len()
      {
        used_textures.insert(t);
      }
    }
  }
  let mut tex = textures.to_vec();
  let (mut renamed, mut odd) = (0usize, 0usize);
  for &ti in &used_textures {
    let t = &mut tex[ti];
    let same = doc
      .textures
      .iter()
      .any(|d| d.name == t.name && d.width == t.width && d.height == t.height && d.rgba == t.rgba);
    if !same && !prefixed(&t.name) {
      t.name = format!("jak2-{}", t.name);
      renamed += 1;
    }
    if !pow2(t.width) || !pow2(t.height) || t.width > 256 || t.height > 256 {
      odd += 1;
    }
  }
  let mut mats = materials.to_vec();
  let mut blended = 0usize;
  for &mi in &used_materials {
    if !prefixed(&mats[mi].name) {
      mats[mi].name = format!("jak2-{}", mats[mi].name);
    }
    blended += (mats[mi].alpha == AlphaMode::Blend) as usize;
  }
  let mut out = merge_mesh(doc, mesh, &tex, &mats);
  let m = Arc::make_mut(&mut out);
  if !prefixed(&m.name) {
    m.name = format!("jak2-{}", m.name);
  }
  if let Some(extras) = m.extras.as_object_mut() {
    extras.remove("og_terrain");
  }

  if let Some(checks) = checks {
    let (mut tris, mut bad, mut no_uv, mut no_color, mut colors_out) = (0, 0, 0, 0, 0);
    for p in &mesh.prims {
      tris += p.tri_count();
      bad += p.idx.iter().filter(|&&i| i as usize >= p.pos.len()).count();
      let textured = material(p.material).is_some_and(|m| m.texture >= 0);
      no_uv += (textured && p.uv.len() != p.pos.len()) as usize;
      no_color += (p.col.len() != p.pos.len()) as usize;
      // vertex colors outside 0..1
      colors_out += p
        .col
        .iter()
        .filter(|c| [c.x, c.y, c.z].iter().any(|&v| v < 0.0 || v > 1.0))
        .count();
    }
    let ratio = lighting_ratio(mesh, doc);
    *checks = json!([
      {"check": "mesh", "ok": bad == 0 && no_uv == 0, "triangles": tris, "bad_indices": bad,
       "no_uv": no_uv},
      {"check": "colors", "ok": colors_out == 0 && no_color == 0, "without": no_color,
       "outside": colors_out},
      {"check": "lighting", "ok": (0.75..=1.0 / 0.75).contains(&ratio),
       "ratio": (ratio * 100.0).round() / 100.0},
      {"check": "materials", "ok": true, "count": used_materials.len(), "blended": blended},
      {"check": "textures", "ok": odd == 0, "count": used_textures.len(), "renamed": renamed,
       "odd": odd},
      {"check": "metadata", "ok": true},
    ]);
  }
  out
}

#[cfg(test)]
mod tests {
  use std::collections::{BTreeMap, BTreeSet};

  use super::*;
  use crate::io::catalog::PartKind;
  use crate::io::test_library::make_box;
  use crate::math::{Vec2, Vec4};

  // a structure of Jak II brought into a Jak 3 level: the image Jak 3 has shared, another renamed,
  // its lighting compared with the level's and matched
  #[test]
  fn jak2_structure_brought_in() {
    let mut doc = Scene::new();
    let wall = Texture {
      name: "wall".into(),
      width: 2,
      height: 2,
      rgba: vec![100; 16],
      ..Default::default()
    };
    let wm = Material {
      name: "wall".into(),
      texture: doc.add_texture(wall.clone()),
      ..Default::default()
    };
    let doc_mat = doc.add_material(wm);
    let mut decor_mesh = make_box("jak3-box", doc_mat);
    let p = &mut Arc::make_mut(&mut decor_mesh).prims[0];
    p.col = vec![Vec4::new(0.3, 0.3, 0.3, 1.0); p.pos.len()];
    doc.add(Node {
      kind: NodeKind::Render,
      mesh: Some(decor_mesh),
      ..Default::default()
    });
    // the Jak II part: the same "wall" image, a "roof" Jak 3 has under that name with other pixels
    let roof = Texture {
      name: "roof".into(),
      rgba: vec![200; 16],
      ..wall.clone()
    };
    doc.add_texture(Texture {
      name: "roof".into(),
      ..wall.clone()
    });
    let textures = vec![wall, roof];
    let materials = vec![
      Material {
        name: "wall".into(),
        texture: 0,
        ..Default::default()
      },
      Material {
        name: "roof".into(),
        texture: 1,
        alpha: AlphaMode::Blend,
        ..Default::default()
      },
    ];
    let mut part = Mesh::new();
    part.name = "palace-pillar".into();
    for m in [0, 1] {
      let mut p = make_box("p", m).prims[0].clone();
      p.uv = vec![Vec2::default(); p.pos.len()];
      p.col = vec![Vec4::new(0.6, 0.6, 0.6, 1.0); p.pos.len()];
      part.prims.push(p);
    }
    let mut checks = Value::Null;
    let mesh = import_jak2_mesh(&mut doc, &part, &textures, &materials, Some(&mut checks));
    let names: BTreeSet<&str> = mesh
      .prims
      .iter()
      .map(|p| {
        let m = &doc.materials[p.material as usize];
        doc.textures[m.texture as usize].name.as_str()
      })
      .collect();
    let by: BTreeMap<&str, &Value> = checks
      .as_array()
      .unwrap()
      .iter()
      .map(|c| (c["check"].as_str().unwrap(), c))
      .collect();
    assert_eq!(mesh.name, "jak2-palace-pillar");
    assert_eq!(names, BTreeSet::from(["wall", "jak2-roof"]));
    assert_eq!(doc.textures.len(), 3);
    assert_eq!(by["textures"]["renamed"], 1);
    assert_eq!(by["materials"]["blended"], 1);
    assert_eq!(by["mesh"]["ok"], true);
    assert!((by["lighting"]["ratio"].as_f64().unwrap() - 2.0).abs() < 1e-6);
    assert_eq!(by["lighting"]["ok"], false);
    let matched = lighting_ratio(&scaled_colors(&mesh, 0.5), &doc);
    assert!(
      (matched - 1.0).abs() < 1e-4,
      "the lighting matched: {matched}"
    );
    assert!(!jak2_structure(&CatalogPart {
      kind: PartKind::Prototype,
      name: "palcab-lowres-city.mb".into(),
      ..Default::default()
    }));
  }
}
