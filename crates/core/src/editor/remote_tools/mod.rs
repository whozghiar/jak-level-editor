//! The editor's side of its MCP server (src/app/remote_tools.cpp; remote.rs, crates/mcp): the
//! tools an assistant calls, carried out under the editor's lock like the user's actions (shown
//! as they happen, undoable). Positions are in meters, Y up, like the editor's view. The answers'
//! shapes and texts are the C++ editor's: the MCP guide and the user's scripts rely on them.
//!
//! The tools by what they do: levels.rs, find.rs, change.rs, view.rs, catalog.rs.

mod catalog;
mod change;
mod find;
mod levels;
#[cfg(test)]
mod tests;
mod view;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::levels::{LevelRole, copy_nodes, paste_nodes};
use super::placement::PlacementKind;
use super::structures::{K_STRUCTURE_KINDS, StructureParams};

/// the first id of a list of new roots ({roots: [...]})
fn id_of(v: &Value) -> u32 {
  v["roots"][0].as_u64().unwrap_or(0) as u32
}
use super::remote::Reply;
use super::{CameraState, Editor};
use crate::game::mood::weather_presets;
use crate::history::NodeChange;
use crate::i18n::{tr, tr_en};
use crate::io::behaviors::behavior_kind;
use crate::io::catalog::{CatalogModel, CatalogPart, PartKind, effect_kind, list_parts};
use crate::io::decor_behaviors::{decor_behavior_json, decor_behavior_of};
use crate::io::game_export::{
  check_continues, default_iso_name, default_nickname, untaken_nickname,
};
use crate::io::jak2_import::{jak2_structure, lighting_ratio, mesh_lighting, scaled_colors};
use crate::io::nav_export::export_navmeshes;
use crate::math::{AABB, Mat4, Quat, Ray, Vec3, length, normalize, radians};
use crate::navmesh::{navmesh_nodes, navmesh_of_node, navmesh_problem};
use crate::scene::assemblies::find_assemblies;
use crate::scene::pick::{PickFilter, covered_below, pick};
use crate::scene::shapes::{
  ShapeParams, edit_material, node_shape, reshape, shape_preset, texture_from_image,
};
use crate::scene::{NodeKind, kind_name};
use crate::{log_info, strf};

type R = Result<Value, String>;

/// how long a capture waits for the interface's picture
const K_CAPTURE_WAIT: Duration = Duration::from_secs(30);
/// how long a capture waits for the interface to start (frontend_ready)
const K_INTERFACE_WAIT: Duration = Duration::from_secs(15);

const UNSAVED: &str =
  "the document has unsaved changes: save_project first, or discard_changes: true";
const NO_JAK2: &str = "Jak II was not extracted (File > Extract game assets with a Jak II disc)";

fn round3(v: f32) -> f32 {
  (v * 1000.0).round() / 1000.0
}

fn v3(v: Vec3) -> Value {
  json!([round3(v.x), round3(v.y), round3(v.z)])
}

fn vec3_of(v: &Value) -> Option<Vec3> {
  let a = v.as_array().filter(|a| a.len() == 3)?;
  let c = |i: usize| a[i].as_f64().map(|x| x as f32);
  Some(Vec3::new(c(0)?, c(1)?, c(2)?))
}

fn vec3_arg(a: &Value, k: &str) -> Option<Vec3> {
  a.get(k).and_then(vec3_of)
}

/// one id or a list of them
fn ids_arg(a: &Value, k: &str) -> Vec<u32> {
  match a.get(k) {
    Some(Value::Array(l)) => l
      .iter()
      .filter_map(Value::as_u64)
      .map(|v| v as u32)
      .collect(),
    Some(v) => v.as_u64().map(|v| vec![v as u32]).unwrap_or_default(),
    None => Vec::new(),
  }
}

fn str_arg<'a>(a: &'a Value, k: &str) -> &'a str {
  a.get(k).and_then(Value::as_str).unwrap_or_default()
}

fn num_opt(a: &Value, k: &str) -> Option<f32> {
  a.get(k).and_then(Value::as_f64).map(|v| v as f32)
}

fn num(a: &Value, k: &str, def: f32) -> f32 {
  num_opt(a, k).unwrap_or(def)
}

fn int(a: &Value, k: &str, def: i64) -> i64 {
  a.get(k).and_then(Value::as_f64).map_or(def, |v| v as i64)
}

fn uint(a: &Value, k: &str) -> u32 {
  a.get(k).and_then(Value::as_u64).unwrap_or(0) as u32
}

fn flag(a: &Value, k: &str, def: bool) -> bool {
  a.get(k).and_then(Value::as_bool).unwrap_or(def)
}

fn lower(s: &str) -> String {
  s.to_ascii_lowercase()
}

/// [[x, y, z], [x, y, z]]
fn box_arg(v: &Value) -> Option<AABB> {
  let l = v.as_array().filter(|l| l.len() == 2)?;
  let mut b = AABB::default();
  b.add(vec3_of(&l[0])?);
  b.add(vec3_of(&l[1])?);
  Some(b)
}

fn min_max(b: AABB) -> Value {
  json!({"min": v3(b.lo), "max": v3(b.hi)})
}

fn role_word(role: LevelRole) -> &'static str {
  match role {
    LevelRole::Main => "opened",
    LevelRole::Added => "added",
    _ => "companion",
  }
}

fn camera_json(c: &CameraState) -> Value {
  json!({
    "position": v3(c.pos),
    "yaw_deg": round3(c.yaw.to_degrees()),
    "pitch_deg": round3(c.pitch.to_degrees()),
    "forward": v3(c.forward()),
  })
}

impl Editor {
  /// A tool of the MCP server by name (remote_dispatch).
  pub fn remote_tool(&mut self, tool: &str, a: &Value) -> Reply {
    match tool {
      "get_status" => Ok(self.remote_status()).into(),
      "list_levels" => self.tool_list_levels(a).into(),
      "get_level_info" => {
        let name = match a.get("name").and_then(Value::as_str) {
          Some(n) => n.to_string(),
          None => self.scene.settings.level.clone(),
        };
        if name.is_empty() {
          return Err("name: a level (see list_levels)".to_string()).into();
        }
        Ok(self.level_info(&name)).into()
      }
      "add_level" => self.tool_add_level(a),
      "remove_level" => self.tool_remove_level(a).into(),
      "merge_level" => self.tool_merge_level(a).into(),
      "set_shape" => self.tool_set_shape(a).into(),
      "set_material" => self.tool_set_material(a).into(),
      "list_textures" => self.tool_list_textures(a).into(),
      "set_actor" => self.tool_set_actor(a).into(),
      "set_visibility" => self.tool_set_visibility(a).into(),
      "open_level" => self.tool_open_level(a),
      "new_level" => {
        if self.remote_unsaved(a) {
          return Err(UNSAVED.to_string()).into();
        }
        self.new_level();
        Ok(self.remote_status()).into()
      }
      // the whole catalog: once the editor has read the levels (their variants and effects)
      "list_catalog" => self.tool_list_catalog(a),
      "list_models" => self.tool_list_models(a),
      "list_decor" => self.tool_list_decor(a),
      "list_prefabs" => self.tool_list_prefabs(a),
      "place" => self.remote_place(a),
      "place_many" => self.tool_place_many(a),
      "build_house" => self.tool_build_house(a),
      "build_structure" => self.tool_build_structure(a),
      "set_structure" => self.tool_set_structure(a),
      "create_prefab_set" => self.tool_create_prefab_set(a),
      "building_kit" => self.tool_building_kit(a),
      "generate_navmesh" => self.tool_generate_navmesh(a),
      "export_navmesh" => self.tool_export_navmesh(a).into(),
      "match_lighting" => self.tool_match_lighting(a).into(),
      "set_decor" => self.tool_set_decor(a).into(),
      "replace_level" => self.tool_replace_level(a),
      "export_level" => self.tool_export_level(a),
      "find_nodes" => self.tool_find_nodes(a).into(),
      "get_node" => self.tool_get_node(a).into(),
      "transform_nodes" => self.tool_transform_nodes(a).into(),
      "delete_nodes" => self.tool_delete_nodes(a).into(),
      "duplicate_nodes" => self.tool_duplicate_nodes(a).into(),
      "raycast" => self.tool_raycast(a).into(),
      "set_view" => self.tool_set_view(a).into(),
      "set_camera" => self.tool_set_camera(a).into(),
      "capture_view" => self.tool_capture_view(a),
      "group" => self.tool_group(a).into(),
      "ungroup" => {
        let gid = uint(a, "group");
        if self.group_info(gid).is_none() {
          return Err("group: a group's id (list_groups)".to_string()).into();
        }
        self.ungroup(gid);
        Ok(json!({"ungrouped": gid})).into()
      }
      "list_groups" => Ok(self.groups_json()).into(),
      "select" => self.tool_select(a).into(),
      "terrain" => self.tool_terrain(a),
      "list_assemblies" => self.tool_list_assemblies(a).into(),
      "set_story" => self.tool_set_story(a).into(),
      "create_prefab" => self.tool_create_prefab(a).into(),
      // for the tests of the C++ interface (not offered to the assistant)
      "check_catalog_thumbnails" => Err(
        "check_catalog_thumbnails: the catalog's pictures are made by the interface now"
          .to_string(),
      )
      .into(),
      "ground_report" => Ok(self.ground_report()).into(),
      "ui_input" => Err(
        "ui_input: not available: the interface is a web view now (drive it through the \
         WebView's DevTools protocol)"
          .to_string(),
      )
      .into(),
      "save_project" => self.tool_save_project(a).into(),
      "open_project" => {
        let path = str_arg(a, "path");
        if path.is_empty() {
          return Err("path: the project (.glb)".to_string()).into();
        }
        if self.remote_unsaved(a) {
          return Err(UNSAVED.to_string()).into();
        }
        if self.open_project_file(path).is_err() {
          return Err(format!("cannot open {path} (see the editor's log)")).into();
        }
        Ok(self.remote_status()).into()
      }
      "restore_effects" => {
        if !self.document_open() {
          return Err("no document open".to_string()).into();
        }
        let (added, left_out) = self.restore_level_effects();
        log_info!(tr("log.effects_restored"), added, left_out);
        Ok(json!({"added": added, "left_out": left_out})).into()
      }
      "undo" | "redo" => {
        let undo = tool == "undo";
        let (can, label) = if undo {
          (self.history.can_undo(), self.history.undo_label())
        } else {
          (self.history.can_redo(), self.history.redo_label())
        };
        if can {
          self.undo_redo(undo);
        }
        Ok(json!({"done": can, "change": label})).into()
      }
      _ => Err(format!("unknown tool '{tool}'")).into(),
    }
  }

  // ----------------------------------------------------------------------------------------------
  // helpers
  // ----------------------------------------------------------------------------------------------

  /// An element as the tools answer it (remote_node_json)
  fn node_json(&self, id: u32) -> Value {
    let Some(n) = self.scene.find(id) else {
      return Value::Null;
    };
    let mut j = json!({"id": n.id, "name": n.name, "kind": kind_name(n.kind)});
    if n.kind == NodeKind::Actor {
      j["etype"] = json!(str_arg(&n.extras, "etype"));
    }
    j["position"] = v3(self.scene.world(n).col3(3));
    let (b, _) = self.subtree_bounds(id);
    if b.valid() {
      j["size"] = v3(b.size());
      j["box"] = json!([v3(b.lo), v3(b.hi)]);
    }
    if n.locked {
      j["locked"] = json!(true);
    }
    if !self.scene.effectively_visible(n) {
      j["hidden"] = json!(true);
    }
    // a prefab placed, a group named
    let g = self.group_of(id);
    if g != 0 {
      j["group"] = json!(g);
    }
    if let Some(nav) = navmesh_of_node(n) {
      j["polygons"] = json!(nav.polys.len());
      j["area_m2"] = json!(nav.area().round());
      if let Some(problem) = navmesh_problem(&nav) {
        j["problem"] = json!(problem);
      }
    }
    j
  }

  fn remote_unsaved(&self, a: &Value) -> bool {
    self.unsaved() && !flag(a, "discard_changes", false)
  }

  fn view_json(&self) -> Value {
    let m = &self.weather.mood;
    let s = &self.game_chunks.stats;
    let name = self
      .weather
      .preset_index()
      .map_or("custom", |i| weather_presets()[i].id);
    json!({
      "game_view": self.view.game_view && self.view.game_view_ready,
      "hour": round3(m.hour),
      "weather": {
        "name": name,
        "cloud": round3(m.cloud),
        "fog": round3(m.fog),
        "rain": round3(m.rain),
        "lightning": self.weather.lightning,
      },
      "level_lights": m.level_lights,
      "effects": self.view.effects,
      "game_chunks": s.chunks,
      "game_triangles": s.triangles,
      "game_build_ms": round3(s.ms_build as f32),
    })
  }

  /// get_status: the library, the document, the camera, the selection, the view, the story
  // (the C++ also gave the frame's time and the drawing's statistics: the interface's now)
  pub fn remote_status(&mut self) -> Value {
    let actors = self
      .scene
      .nodes
      .iter()
      .filter(|n| n.kind == NodeKind::Actor)
      .count();
    let open = self.document_open();
    let mut j = json!({
      "library": self.library_root,
      "levels": self.library.levels().len(),
      "document": {
        "open": open,
        "level": self.scene.settings.level,
        "project": self.project_path,
        "elements": self.scene.nodes.len(),
        "actors": actors,
        "unsaved": self.unsaved(),
        // a level read or added, the land around the ocean (the C++'s m_ocean_land_future)
        "loading": self.levels.opening.is_some()
          || self.levels.adding.is_some()
          || self.busy("ocean_land"),
      },
      "camera": camera_json(&self.camera),
      "selection": self.selection,
      "can_undo": self.history.can_undo(),
      "can_redo": self.history.can_redo(),
      "view": self.view_json(),
      // the interface is there to make the pictures of the view (capture_view)
      "interface_ready": self.interface_ready,
    });
    if let Some(data) = self.document_game_data().filter(|d| d.has_story()) {
      let after_node = |step: i32| data.node(step as usize - 1).name.clone();
      let mut s = json!({
        "enabled": self.story.enabled,
        "step": self.story.step,
        "steps": data.step_count(),
      });
      if self.story.step > 1 {
        s["after_node"] = json!(after_node(self.story.step));
      }
      // each level at its point: its states (the steps where it changes) and the one shown
      let main = self.scene.settings.level.clone();
      let level_json = |name: &str, step: i32| {
        let states = self.story_states(if name == main { "" } else { name });
        let state = 1 + states[1..].iter().filter(|&&s| s <= step).count();
        let mut l = json!({"step": step, "state": state, "states": states});
        if step > 1 {
          l["after_node"] = json!(after_node(step));
        }
        l
      };
      let mut levels = Map::new();
      levels.insert(main.clone(), level_json(&main, self.story.step));
      for name in self.level_story_steps.keys() {
        levels.insert(name.clone(), level_json(name, self.story.step_of(name)));
      }
      s["levels"] = Value::Object(levels);
      j["story"] = s;
    }
    if open {
      let b = self.scene.focus_bounds();
      if b.valid() {
        j["document"]["bounds"] = min_max(b);
      }
      // its levels: the one opened, the ones added, the companions (layers left out)
      let levels: Vec<Value> = self
        .document_levels()
        .iter()
        .filter(|l| l.role != LevelRole::Layer)
        .map(|l| {
          let mut e = json!({"name": l.name, "role": role_word(l.role)});
          if l.group != 0 {
            e["group"] = json!(l.group);
            if let Some(g) = self.scene.find(l.group) {
              e["visible"] = json!(g.visible);
            }
          }
          e
        })
        .collect();
      j["document"]["levels"] = json!(levels);
      if let Some((name, _)) = &self.levels.adding {
        j["document"]["adding"] = json!(name);
      }
    }
    j
  }

  fn groups_json(&self) -> Value {
    let groups: Vec<Value> = self
      .document_groups()
      .iter()
      .map(|g| {
        let mut b = AABB::default();
        for &r in &g.roots {
          b.add_box(&self.subtree_bounds(r).0);
        }
        json!({
          "group": g.id,
          "name": g.name,
          "prefab": g.prefab,
          "ids": g.roots,
          "box": if b.valid() { json!([v3(b.lo), v3(b.hi)]) } else { Value::Null },
        })
      })
      .collect();
    json!({ "groups": groups })
  }

  fn selection_json(&self) -> Value {
    json!({ "selection": self.selection })
  }

  /// the group of a level of the document by its name (0 the level opened)
  fn document_level_group(&self, name: &Value, roles: &[LevelRole]) -> Option<u32> {
    let name = name.as_str()?;
    self
      .document_levels()
      .iter()
      .find(|l| l.name == name && roles.contains(&l.role))
      .map(|l| l.group)
  }
}
