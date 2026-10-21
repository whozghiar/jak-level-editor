//! The inspector's sections of a surface (surface_panel.cpp): the size of a shape made in the
//! editor (scene/shapes.rs), and the materials of any piece: an image file, or a texture of the
//! games (the Textures window). A change gives the pieces selected a new material and copies of
//! their meshes: the other pieces drawing the same material keep it, and undo puts the meshes back.
//! Their color is the game's (not edited: the game's materials are drawn twice as bright, their
//! vertex colors at half). And the checks of a model imported from a .glb.

use serde_json::{Value, json};

use super::Editor;
use crate::history::NodeChange;
use crate::i18n::tr;
use crate::io::jak2_import::{lighting_ratio, scaled_colors};
use crate::log_error;
use crate::math::Vec3;
use crate::scene::shapes::{ShapeParams, edit_material, node_shape, reshape, texture_from_image};
use crate::scene::{Mesh, NodeKind};

/// the materials the inspector lists, the others counted
const K_SHOWN: usize = 12;

/// the materials a mesh draws, in the order of its pieces
fn mesh_materials(mesh: &Mesh) -> Vec<i32> {
  let mut out = Vec::new();
  for p in &mesh.prims {
    if !out.contains(&p.material) {
      out.push(p.material);
    }
  }
  out
}

fn ids_of(a: &Value) -> Vec<u32> {
  a.get("ids")
    .and_then(|v| serde_json::from_value(v.clone()).ok())
    .unwrap_or_default()
}

impl Editor {
  /// `shape {id}`: {params (ShapeParams), scale} of a shape made in the editor, null otherwise
  pub fn shape_info(&self, id: u32) -> Option<Value> {
    let n = self.scene.find(id).filter(|n| n.kind == NodeKind::Render)?;
    let p = node_shape(n)?;
    Some(json!({"params": p.to_json(), "scale": n.local.s}))
  }

  /// `set_shape {ids, params (merged into each one's), apply_scale?, live?}`: the shapes made
  /// again, undoable (undo.shape). `apply_scale`: stretched with the scale tool, the scale made its
  /// size (its texture repeats again instead of stretching).
  pub fn set_shape(&mut self, a: &Value) -> Result<(), String> {
    let ids: Vec<u32> = ids_of(a)
      .into_iter()
      .filter(|&id| self.shape_info(id).is_some())
      .collect();
    if ids.is_empty() {
      return Err("ids: shapes made in the editor".into());
    }
    let over = a
      .get("params")
      .and_then(Value::as_object)
      .cloned()
      .unwrap_or_default();
    let apply_scale = a.get("apply_scale").and_then(Value::as_bool) == Some(true);
    let live = a.get("live").and_then(Value::as_bool).unwrap_or(false);
    self.inspector_edit(&ids, live, "undo.shape", |scene| {
      for &id in &ids {
        let Some(n) = scene.find_mut(id) else {
          continue;
        };
        let mut j = node_shape(n).unwrap_or_default().to_json();
        for (k, v) in &over {
          j[k] = v.clone();
        }
        let mut p = ShapeParams::from_json(&j);
        if apply_scale {
          let s = n.local.s;
          p.size = Vec3::new(p.size.x * s.x, p.size.y * s.y, p.size.z * s.z);
          n.local.s = Vec3::new(1.0, 1.0, 1.0);
        }
        reshape(scene, id, &ShapeParams::from_json(&p.to_json()));
      }
    });
    Ok(())
  }

  /// `materials {id}` (the pieces selected drawing the same change with it) or `{ids}` (those, the
  /// first one's materials): {targets, slots: [{material, texture: {index, name, width, height} |
  /// null}] (the first 12), more}. Null when the first is not a mesh.
  pub fn materials_info(&self, a: &Value) -> Option<Value> {
    let mut targets = ids_of(a);
    if let Some(id) = a.get("id").and_then(Value::as_u64) {
      targets = self.selection.iter().copied().collect();
      if !targets.contains(&(id as u32)) {
        targets.push(id as u32);
      }
      // the one inspected first
      targets.retain(|&t| t != id as u32);
      targets.insert(0, id as u32);
    }
    let n = self
      .scene
      .find(*targets.first()?)
      .filter(|n| n.kind == NodeKind::Render)?;
    let materials = mesh_materials(n.mesh.as_ref()?);
    let slots: Vec<Value> = materials
      .iter()
      .take(K_SHOWN)
      .map(|&mi| {
        let t = usize::try_from(mi)
          .ok()
          .and_then(|i| self.scene.materials.get(i))
          .and_then(|m| usize::try_from(m.texture).ok())
          .and_then(|ti| Some((ti, self.scene.textures.get(ti)?)));
        json!({
          "material": mi,
          "texture": t.map(|(i, t)| json!({"index": i, "name": t.name, "width": t.width, "height": t.height})),
        })
      })
      .collect();
    Some(json!({
      "targets": targets,
      "slots": slots,
      "more": materials.len().saturating_sub(K_SHOWN),
    }))
  }

  /// `set_material {ids, slot, texture}`: the texture (an index of the document's) of the material
  /// `slot` of the first one's mesh (none: all of them), on those of `ids` drawing it, undoable
  /// (undo.texture). Answers {changed}.
  pub fn set_material(&mut self, a: &Value) -> Result<Value, String> {
    let texture = a.get("texture").and_then(Value::as_i64).unwrap_or(-1) as i32;
    if texture < 0 || texture as usize >= self.scene.textures.len() {
      return Err(format!("texture: no texture {texture} in the document"));
    }
    Ok(json!({"changed": self.give_texture(a, texture)}))
  }

  /// `material_image {ids, slot, path}`: an image file (PNG, JPG, TGA, BMP) as the texture of the
  /// material `slot`, as set_material
  pub fn material_image(&mut self, a: &Value) -> Value {
    let path = a.get("path").and_then(Value::as_str).unwrap_or_default();
    match texture_from_image(&mut self.scene, path) {
      Ok(ti) => json!({"changed": self.give_texture(a, ti)}),
      Err(e) => {
        log_error!(tr("log.texture_failed"), path, e);
        json!({"changed": 0})
      }
    }
  }

  fn give_texture(&mut self, a: &Value, texture: i32) -> usize {
    let ids = ids_of(a);
    let slot = a.get("slot").and_then(Value::as_i64).unwrap_or(-1);
    let material = ids
      .first()
      .and_then(|&id| self.scene.find(id)?.mesh.as_ref())
      .map(|m| mesh_materials(m))
      .and_then(|m| m.get(usize::try_from(slot).ok()?).copied())
      .unwrap_or(-1);
    let mut c = NodeChange::new(&self.scene);
    for &id in &ids {
      c.save(&self.scene, id);
    }
    let changed = edit_material(&mut self.scene, &ids, material, |m| m.texture = texture);
    if changed > 0 {
      self.scene.begin_frame();
      self
        .history
        .push(c.commit(&self.scene, tr("undo.texture"), None));
    }
    changed
  }

  /// `import_match_lighting {id}`: the vertex colors of a model imported scaled to the brightness
  /// of the document's Jak 3 decor (its lighting check passed), undoable (undo.match_lighting)
  pub fn import_match_lighting(&mut self, id: u32) {
    let Some(n) = self.scene.find(id).filter(|n| n.kind == NodeKind::Render) else {
      return;
    };
    let Some(mesh) = n.mesh.clone() else { return };
    let ratio = lighting_ratio(&mesh, &self.scene);
    if ratio <= 0.0 {
      return;
    }
    let before = n.clone();
    let n = self.scene.find_mut(id).unwrap();
    n.mesh = Some(scaled_colors(&mesh, 1.0 / ratio));
    let checks = n
      .extras
      .get_mut("og_import")
      .and_then(|i| i.get_mut("checks"));
    if let Some(checks) = checks.and_then(Value::as_array_mut) {
      for k in checks {
        if k.get("check").and_then(Value::as_str) == Some("lighting") {
          k["ok"] = true.into();
          k["ratio"] = 1.0.into();
        }
      }
    }
    self.commit_node_edit(before, "undo.match_lighting");
  }
}
