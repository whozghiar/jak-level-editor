//! The editor's side of its MCP server (src/app/remote_tools.cpp; remote.rs, crates/mcp): the
//! tools an assistant calls, carried out under the editor's lock like the user's actions (shown
//! as they happen, undoable). Positions are in meters, Y up, like the editor's view. The answers'
//! shapes and texts are the C++ editor's: the MCP guide and the user's scripts rely on them.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::export::GapsFound;
use super::levels::{LevelRole, copy_nodes, paste_nodes};
use super::placement::PlacementKind;
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
use crate::scene::gaps::{GapOptions, find_gaps};
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
      "find_gaps" => self.tool_find_gaps(a).into(),
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

  // ----------------------------------------------------------------------------------------------
  // levels and documents
  // ----------------------------------------------------------------------------------------------

  fn tool_list_levels(&mut self, a: &Value) -> R {
    let f = lower(str_arg(a, "filter"));
    // near: only the levels the game displays together with that one
    let near = str_arg(a, "near");
    let around: Option<Vec<String>> = if near.is_empty() {
      None
    } else {
      let data = self.game_data();
      let n = data
        .as_ref()
        .and_then(|d| d.level(near))
        .map(|l| l.neighbors.clone());
      if n.is_none() {
        return Err(format!("near: no game data for the level '{near}'"));
      }
      n
    };
    let in_document: HashSet<String> = self
      .document_levels()
      .iter()
      .filter(|l| l.role != LevelRole::Layer)
      .map(|l| l.name.clone())
      .collect();
    let jak2 = str_arg(a, "game") == "jak2";
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    let levels: Vec<Value> = lib
      .levels()
      .iter()
      .filter(|l| l.listed && (f.is_empty() || l.name.contains(&f) || lower(&l.dgo).contains(&f)))
      .filter(|l| around.as_ref().is_none_or(|n| n.contains(&l.name)))
      .map(|l| {
        let mut e = json!({"name": l.name, "dgo": l.dgo});
        let name = if jak2 {
          format!("jak2-{}", l.name)
        } else {
          l.name.clone()
        };
        if in_document.contains(&name) {
          e["in_document"] = json!(true);
        }
        e
      })
      .collect();
    Ok(json!({ "levels": levels }))
  }

  fn tool_open_level(&mut self, a: &Value) -> Reply {
    let name = str_arg(a, "name").to_string();
    if self.levels.opening.is_some() {
      return Err("a level is already loading".to_string()).into();
    }
    if self.remote_unsaved(a) {
      return Err(UNSAVED.to_string()).into();
    }
    // Jak II's (game "jak2"): its decor made Jak 3's, the document's level named jak2-<name>
    let jak2 = str_arg(a, "game") == "jak2";
    if jak2 && self.library_jak2.is_empty() {
      return Err(NO_JAK2.to_string()).into();
    }
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    if lib.level(&name).is_none() {
      return Err(format!(
        "no level '{name}' in the library (see list_levels)"
      ))
      .into();
    }
    let before = self.load_actors;
    self.load_actors = flag(a, "actors", true);
    let started = self.load_level(&name, jak2);
    self.load_actors = before;
    if let Err(e) = started {
      return Err(e).into();
    }
    let opened = if jak2 {
      format!("jak2-{name}")
    } else {
      name.clone()
    };
    Reply::wait(move |ed| {
      ed.levels.opening.is_none().then(|| {
        if ed.scene.settings.source_group != opened {
          Err(format!(
            "the level {name} could not be opened (see the editor's log)"
          ))
          .into()
        } else {
          Ok(ed.remote_status()).into()
        }
      })
    })
  }

  fn tool_add_level(&mut self, a: &Value) -> Reply {
    let name = str_arg(a, "name").to_string();
    let jak2 = str_arg(a, "game") == "jak2";
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    if lib.level(&name).is_none() {
      return Err(format!(
        "no level '{name}' in the library (see list_levels)"
      ))
      .into();
    }
    let before = self.levels.levels_added;
    let with_actors = self.load_actors;
    self.load_actors = flag(a, "actors", true);
    let started = self.add_level_to_document(&name, flag(a, "reference", false), jak2);
    self.load_actors = with_actors;
    if let Err(e) = started {
      return Err(e).into();
    }
    // Jak II's is named jak2-<name> in the document
    let shown = if jak2 { format!("jak2-{name}") } else { name };
    Reply::wait(move |ed| {
      (ed.levels.levels_added != before).then(|| {
        if ed.levels.add_error.is_empty() {
          Ok(ed.level_info(&shown)).into()
        } else {
          Err(ed.levels.add_error.clone()).into()
        }
      })
    })
  }

  fn tool_remove_level(&mut self, a: &Value) -> R {
    let name = str_arg(a, "name");
    let reference = flag(a, "reference", false);
    let group = self
      .document_levels()
      .iter()
      .rfind(|l| l.name == name && l.role == LevelRole::Added && l.reference == reference)
      .map(|l| l.group)
      .ok_or_else(|| format!("no level '{name}' added to the document (the level opened stays)"))?;
    self.remove_document_level(group)?;
    Ok(self.remote_status())
  }

  fn tool_merge_level(&mut self, a: &Value) -> R {
    // a level added made part of the document's own: no longer a level next to it (the export
    // writes it with the document's, with its collision: a level built from another's pieces)
    let name = str_arg(a, "name");
    let group = self
      .document_levels()
      .iter()
      .rfind(|l| l.name == name && l.role == LevelRole::Added && !l.reference)
      .map(|l| l.group)
      .ok_or("name: a level added to the document (add_level), not a reference")?;
    let mut c = NodeChange::new(&self.scene);
    c.save(&self.scene, group);
    if let Some(o) = self
      .scene
      .find_mut(group)
      .and_then(|g| g.extras.as_object_mut())
    {
      o.remove("og_level");
    }
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.merge_level"), name), None));
    self.story_document_changed();
    Ok(json!({"group": group, "status": self.remote_status()}))
  }

  fn tool_replace_level(&mut self, a: &Value) -> Reply {
    if !self.document_open() {
      return Err("no level open: open_level, open_project or new_level first".to_string()).into();
    }
    if let Some(level) = a.get("level") {
      let level = level.as_str().unwrap_or_default();
      let data = self.document_game_data();
      let info = data.as_ref().and_then(|d| d.level(level));
      if !level.is_empty() && info.is_none_or(|i| i.dgo.is_empty() || i.layer) {
        return Err(
          "level: a level of the game (see list_levels), or \"\" to replace none".to_string(),
        )
        .into();
      }
      self.set_replaces(level);
    }
    let level = self.scene.settings.replaces.clone();
    let shown = self
      .document_levels()
      .iter()
      .any(|l| l.reference && l.name == level);
    if flag(a, "show_original", false) && !level.is_empty() && !shown {
      if self.library.level(&level).is_none() {
        return Err(format!("the level {level} is not in the library")).into();
      }
      let before = self.levels.levels_added;
      if let Err(e) = self.add_level_to_document(&level, true, false) {
        return Err(e).into();
      }
      // what is asked once the original is there: placed where it is, turned
      let a = a.clone();
      return Reply::wait(move |ed| {
        (ed.levels.levels_added != before).then(|| {
          if ed.levels.add_error.is_empty() {
            ed.replace_level_finish(&a, &level).into()
          } else {
            Err(ed.levels.add_error.clone()).into()
          }
        })
      });
    }
    self.replace_level_finish(a, &level).into()
  }

  fn replace_level_finish(&mut self, a: &Value, level: &str) -> R {
    if flag(a, "align", false) {
      self.align_to_replaced()?;
    }
    if let Some(t) = num_opt(a, "turn_deg") {
      self.turn_own_level(t);
    }
    self.scene.begin_frame();
    let data = self.document_game_data();
    let checks: Vec<Value> = check_continues(&self.scene, data.as_deref(), level)
      .iter()
      .map(|c| {
        let ground = if c.ok() {
          json!(round3(c.ground))
        } else {
          Value::Null
        };
        json!({"name": c.name, "position": v3(c.position), "ground": ground})
      })
      .collect();
    let mut r = json!({"replaces": level, "continues": checks});
    let own = self.own_level_bounds();
    if own.valid() {
      r["bounds"] = min_max(own);
    }
    for l in self.document_levels().iter() {
      if l.reference && l.name == level {
        let b = self.document_level_bounds(l.group);
        if b.valid() {
          r["original_bounds"] = min_max(b);
        }
      }
    }
    Ok(r)
  }

  fn tool_save_project(&mut self, a: &Value) -> R {
    // a name: its folder in <library>/projects; a path: that file; else where it was saved
    let name = str_arg(a, "name");
    if !name.is_empty() {
      self
        .save_project_as(name)
        .map_err(|e| format!("cannot save: {e}"))?;
    } else {
      let mut path = match str_arg(a, "path") {
        "" => self.project_path.clone(),
        p => p.to_string(),
      };
      if path.is_empty() {
        return Err("name: the project's name (saved in <library>/projects/<name>/)".into());
      }
      if Path::new(&path).extension().is_none_or(|e| e != "glb") {
        path += ".glb";
      }
      self
        .save_project_files(&path)
        .map_err(|e| format!("cannot save: {e}"))?;
    }
    Ok(json!({"path": self.project_path}))
  }

  fn tool_set_story(&mut self, a: &Value) -> R {
    let data = self
      .document_game_data()
      .filter(|d| d.has_story())
      .ok_or("no story view for this document (a new level has none)")?;
    let mut v = self.story.clone();
    // which level: the one opened (the default), or a level added, at its own point of the story
    let mut level = str_arg(a, "level").to_string();
    if level == self.scene.settings.level {
      level.clear();
    }
    if !level.is_empty() && !self.level_story_steps.contains_key(&level) {
      return Err("level: the level opened or a level added (get_status: document.levels)".into());
    }
    let set_step = |v: &mut crate::scene::story::StoryView, s: i32| {
      if level.is_empty() {
        v.step = s;
      } else {
        v.level_steps.insert(level.clone(), s);
      }
    };
    if let Some(e) = a.get("enabled").and_then(Value::as_bool) {
      v.enabled = e;
    }
    if let Some(s) = a.get("step").and_then(Value::as_i64) {
      set_step(&mut v, s as i32);
    }
    if let Some(s) = a.get("state").and_then(Value::as_i64) {
      let states = self.story_states(&level);
      if s < 1 || s as usize > states.len() {
        return Err(format!("state: 1 to {} for this level", states.len()));
      }
      set_step(&mut v, states[s as usize - 1]);
      v.enabled = true;
    }
    if let Some(node) = a.get("after_node").and_then(Value::as_str) {
      let found = (0..data.step_count().max(0) as usize)
        .rev()
        .find(|&i| data.node(i).name == node)
        .ok_or_else(|| format!("no task node '{node}'"))?;
      set_step(&mut v, found as i32 + 1);
      v.enabled = true;
    }
    self.story_level = level;
    self.set_story_view(v);
    Ok(self.remote_status()["story"].clone())
  }

  // ----------------------------------------------------------------------------------------------
  // finding things
  // ----------------------------------------------------------------------------------------------

  fn tool_find_nodes(&mut self, a: &Value) -> R {
    let name = lower(str_arg(a, "name"));
    let etype = str_arg(a, "etype");
    let kind = str_arg(a, "kind");
    let include_locked = flag(a, "include_locked", false);
    let near = vec3_arg(a, "near");
    let radius = num(a, "radius", 20.0);
    let limit = int(a, "limit", 50).clamp(1, 20000) as usize;
    // inside a box (its middle), or outside it; under a group (a level of the document)
    let bx = a.get("box").and_then(box_arg);
    let outside = flag(a, "outside", false);
    let under = uint(a, "under");
    let cache = self.scene.cache();
    let mut found: Vec<(f32, u32)> = Vec::new();
    for (i, n) in self.scene.nodes.iter().enumerate() {
      // an actor's model: the actor stands for it
      if n.extras.get("og_actor_visual").is_some() || n.kind == NodeKind::Collision {
        continue;
      }
      if (n.locked && !include_locked) || (!kind.is_empty() && kind != kind_name(n.kind)) {
        continue;
      }
      if !name.is_empty() && !lower(&n.name).contains(&name) {
        continue;
      }
      if !etype.is_empty() && (n.kind != NodeKind::Actor || str_arg(&n.extras, "etype") != etype) {
        continue;
      }
      if n.kind == NodeKind::Group && name.is_empty() && etype.is_empty() && kind.is_empty() {
        continue;
      }
      if under != 0 {
        let mut p = self.scene.find(n.parent);
        let mut guard = 0;
        while let Some(x) = p.filter(|x| x.id != under && guard < 1000) {
          p = self.scene.find(x.parent);
          guard += 1;
        }
        if p.is_none_or(|x| x.id != under) {
          continue;
        }
      }
      let in_cache = i < cache.count;
      let nb = if n.mesh.is_some() && in_cache {
        cache.bounds[i]
      } else {
        AABB::default()
      };
      let mid = if nb.valid() {
        nb.center()
      } else if in_cache {
        cache.worlds[i].col3(3)
      } else {
        self.scene.world(n).col3(3)
      };
      if let Some(b) = &bx {
        let inside = mid.x >= b.lo.x
          && mid.x <= b.hi.x
          && mid.y >= b.lo.y
          && mid.y <= b.hi.y
          && mid.z >= b.lo.z
          && mid.z <= b.hi.z;
        if inside == outside {
          continue;
        }
      }
      let mut d = 0.0;
      if let Some(p) = near {
        d = length(mid - p);
        if d > radius {
          continue;
        }
      }
      found.push((d, n.id));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let list: Vec<Value> = found
      .iter()
      .take(limit)
      .map(|&(_, id)| self.node_json(id))
      .collect();
    Ok(json!({"total": found.len(), "nodes": list}))
  }

  fn tool_get_node(&mut self, a: &Value) -> R {
    let id = uint(a, "id");
    let n = self
      .scene
      .find(id)
      .cloned()
      .ok_or("no element with that id")?;
    let mut j = self.node_json(id);
    // what the inspector's Preview shows of it (an actor, a model of the catalog)
    if let Some(p) = self.preview_summary(id) {
      j["preview"] = p;
    }
    j["parent"] = json!(n.parent);
    j["children"] = json!(self.scene.children(id));
    j["local"] = json!({
      "position": v3(n.local.t),
      "rotation_deg": v3(n.local.r.to_euler_deg()),
      "scale": v3(n.local.s),
    });
    if let Some(mesh) = &n.mesh {
      j["mesh"] = json!({"name": mesh.name, "triangles": mesh.tri_count()});
      // its materials (set_material), their textures
      let mut seen = BTreeSet::new();
      let mut mats = Vec::new();
      for p in &mesh.prims {
        let Some(m) = usize::try_from(p.material)
          .ok()
          .and_then(|i| self.scene.materials.get(i))
        else {
          continue;
        };
        if !seen.insert(p.material) {
          continue;
        }
        let c = m.base_color;
        let mut e = json!({"material": p.material, "color": [c.x, c.y, c.z, c.w]});
        if let Some(t) = usize::try_from(m.texture)
          .ok()
          .and_then(|i| self.scene.textures.get(i))
        {
          e["texture"] = json!(t.name);
        }
        mats.push(e);
      }
      j["materials"] = json!(mats);
    }
    if let Some(shape) = node_shape(&n) {
      j["shape"] = shape.to_json();
    }
    // a structure of Jak II: where it comes from and what was checked when it came in
    if let Some(im) = n.extras.get("og_import") {
      j["import"] = im.clone();
    }
    // what the game does with it (collision, bounce, launch)
    if let Some(d) = n.extras.get("og_decor") {
      j["decor"] = d.clone();
    }
    if n.kind == NodeKind::Actor {
      j["game_data"] = n.extras.clone();
      // what configures it: the lumps its type's code reads (set_actor), and its behavior
      let data = self.game_data();
      let etype = str_arg(&n.extras, "etype");
      if let Some(ps) = data.as_ref().and_then(|d| d.params(etype)) {
        j["params"] = ps
          .iter()
          .map(|p| json!({"tag": p.tag, "kind": p.kind, "default": p.def}))
          .collect();
      }
      j["behavior_kind"] =
        json!(format!("{:?}", behavior_kind(data.as_deref(), etype)).to_lowercase());
      for c in self.scene.children(id) {
        if let Some(cn) = self
          .scene
          .find(c)
          .filter(|c| c.extras.get("og_actor_visual").is_some())
        {
          j["model"] = json!(cn.name);
        }
      }
    }
    Ok(j)
  }

  fn tool_raycast(&mut self, a: &Value) -> R {
    let origin = vec3_arg(a, "origin").ok_or("origin: [x, y, z]")?;
    let direction = vec3_arg(a, "direction").unwrap_or(Vec3::new(0.0, -1.0, 0.0));
    if length(direction) < 1e-6 {
      return Err("direction: a vector".into());
    }
    let f = PickFilter {
      actors: flag(a, "actors", false),
      include_locked: true,
      ..Default::default()
    };
    let hit = pick(&self.scene, &Ray::new(origin, normalize(direction)), &f);
    if !hit.hit() {
      return Ok(json!({"hit": false}));
    }
    let mut j = json!({
      "hit": true,
      "point": v3(hit.point),
      "normal": v3(hit.normal),
      "distance": round3(hit.t),
    });
    if self.scene.find(hit.node).is_some() {
      j["node"] = self.node_json(hit.node);
    }
    Ok(j)
  }

  fn tool_find_gaps(&mut self, a: &Value) -> R {
    // where the player would see the void: from the ground every few meters, rays around and down
    if !self.document_open() {
      return Err("no document".into());
    }
    let region = if let Some(b) = a
      .get("box")
      .filter(|b| b.as_array().is_some_and(|l| l.len() == 2))
    {
      box_arg(b).ok_or("box: [[x, y, z], [x, y, z]]")?
    } else if a.get("ids").is_some() {
      let mut r = AABB::default();
      for id in ids_arg(a, "ids") {
        r.add_box(&self.subtree_bounds(id).0);
      }
      r.expanded(num(a, "margin", 12.0))
    } else {
      self.own_level_bounds()
    };
    if !region.valid() {
      return Err("no region: box, ids, or a document with decor".into());
    }
    let mut o = GapOptions {
      region,
      spacing: num(a, "spacing", 0.0).max(0.0),
      eye: num(a, "eye", 1.6),
      ..Default::default()
    };
    if let Some(h) = self.ocean_height().filter(|_| self.view.ocean) {
      o.ocean = h;
    }
    let r = find_gaps(&self.scene, &o);
    let limit = int(a, "limit", 40).clamp(1, 500) as usize;
    let gaps = r.spots.iter().filter(|s| !s.edge).count();
    let spots: Vec<Value> = r
      .spots
      .iter()
      .take(limit)
      .map(|s| {
        json!({
          "kind": if s.edge { "edge" } else { "gap" },
          "at": v3(s.at),
          "seen_from": v3(s.seen_from),
          "direction": v3(s.direction),
          "rays": s.rays,
        })
      })
      .collect();
    let answer = json!({
      "gaps": gaps,
      "edges": r.spots.len() - gaps,
      "viewpoints": r.viewpoints,
      "rays": r.rays,
      "region": [v3(region.lo), v3(region.hi)],
      "spots": spots,
    });
    // listed and drawn in the editor too: the Holes window, opened unless show is false (its spots
    // are drawn while it is open, in the captures too)
    self.gaps = Some(GapsFound { report: r, region });
    let show = flag(a, "show", true);
    self.emit("jle:changed", json!({"gaps": true, "show_gaps": show}));
    Ok(answer)
  }

  fn tool_list_assemblies(&mut self, a: &Value) -> R {
    let min_elements = int(a, "min_elements", 2).max(1) as usize;
    let min_size = num(a, "min_size", 4.0);
    let max_size = num(a, "max_size", 60.0).max(10.0);
    let limit = int(a, "limit", 200).max(1) as usize;
    let assemblies = find_assemblies(&mut self.scene, max_size);
    let c = self.scene.cache();
    // seed: the largest element whose name no other has (a recipe finds the assembly by it)
    let mut named: HashMap<&str, usize> = HashMap::new();
    for n in &self.scene.nodes {
      *named.entry(n.name.as_str()).or_default() += 1;
    }
    let mut list = Vec::new();
    let mut total = 0;
    for ids in &assemblies {
      if ids.len() < min_elements {
        continue;
      }
      let mut bx = AABB::default();
      let (mut main, mut seed) = ("", "");
      let (mut main_volume, mut seed_volume) = (-1.0f32, -1.0f32);
      // a part's name without its number: how many, their volume
      let mut parts: BTreeMap<&str, (i32, f32)> = BTreeMap::new();
      let mut triangles = 0;
      // the largest element's size (meters)
      let mut largest = 0.0f32;
      for &id in ids {
        let Some(i) = self.scene.index_of(id).filter(|&i| i < c.count) else {
          continue;
        };
        let b = c.bounds[i];
        bx.add_box(&b);
        let s = b.size();
        let volume = s.x * s.y * s.z;
        let name = self.scene.nodes[i].name.as_str();
        if volume > main_volume {
          main_volume = volume;
          main = name;
        }
        if named[name] == 1 && volume > seed_volume {
          seed_volume = volume;
          seed = name;
        }
        let stem = match name.rfind('-') {
          Some(dash)
            if dash + 1 < name.len() && name[dash + 1..].bytes().all(|b| b.is_ascii_digit()) =>
          {
            &name[..dash]
          }
          _ => name,
        };
        largest = largest.max(s.x).max(s.y).max(s.z);
        let p = parts.entry(stem).or_default();
        p.0 += 1;
        p.1 += volume;
        if let Some(m) = &self.scene.nodes[i].mesh {
          triangles += m.tri_count();
        }
      }
      let size = bx.size();
      if !bx.valid() || size.x.max(size.y).max(size.z) < min_size {
        continue;
      }
      total += 1;
      if list.len() >= limit {
        continue;
      }
      let mut by_count: Vec<(i32, &str)> = parts.iter().map(|(&s, n)| (-n.0, s)).collect();
      by_count.sort();
      let names: Vec<Value> = by_count
        .iter()
        .take(6)
        .map(|&(n, s)| json!([s, -n, parts[s].1.round()]))
        .collect();
      list.push(json!({
        "ids": ids,
        "elements": ids.len(),
        "triangles": triangles,
        "largest": largest,
        "box": [[bx.lo.x, bx.lo.y, bx.lo.z], [bx.hi.x, bx.hi.y, bx.hi.z]],
        "main": main,
        "seed": seed,
        "parts": names,
      }));
    }
    Ok(json!({"assemblies": list, "count": total}))
  }

  /// For the studies of the level pieces (not offered to the assistant): the large pieces of the
  /// levels' shell (og_terrain), the ground locked or not, what stands more than 3 m above their
  /// floor (flat: roofs, upright: walls), the walls split off
  fn ground_report(&mut self) -> Value {
    struct Tri {
      area: f32,
      ny: f32,
      top: f32,
      mid: f32,
    }
    let cache = self.scene.cache();
    let mut pieces = Vec::new();
    for (i, n) in self.scene.nodes.iter().enumerate().take(cache.count) {
      let Some(mesh) = n
        .mesh
        .as_ref()
        .filter(|_| n.kind == NodeKind::Render && n.extras.get("og_terrain").is_some())
      else {
        continue;
      };
      let w = cache.worlds[i];
      let mut flats: Vec<(f32, f32)> = Vec::new(); // height, area
      let (mut all, mut flat, mut walk) = (0.0f64, 0.0f64, 0.0f64);
      let mut tris = Vec::new();
      let mut b = AABB::default();
      for pr in &mesh.prims {
        for t in pr.idx.chunks_exact(3) {
          let [p, q, r] = [0, 1, 2].map(|k| w.point(pr.pos[t[k] as usize]));
          b.add(p);
          b.add(q);
          b.add(r);
          let nr = crate::math::cross(q - p, r - p);
          let l = length(nr);
          if l < 1e-8 {
            continue;
          }
          let tri = Tri {
            area: l * 0.5,
            ny: nr.y.abs() / l,
            top: p.y.max(q.y).max(r.y),
            mid: (p.y + q.y + r.y) / 3.0,
          };
          all += tri.area as f64;
          if tri.ny > 0.7 {
            flat += tri.area as f64;
            flats.push((tri.mid, tri.area));
          }
          if tri.ny > 0.5 {
            walk += tri.area as f64;
          }
          tris.push(tri);
        }
      }
      if all <= 0.0 {
        continue;
      }
      flats.sort_by(|a, b| a.0.total_cmp(&b.0));
      let mut base = flats.first().map_or(b.lo.y, |f| f.0);
      let mut acc = 0.0f64;
      for &(y, ar) in &flats {
        base = y;
        acc += ar as f64;
        if acc >= 0.2 * flat {
          break;
        }
      }
      let (mut above_flat, mut above_up) = (0.0f64, 0.0f64);
      let mut high = base;
      for t in &tris {
        let is_flat = t.ny >= 0.7;
        if !(if is_flat {
          t.mid > base + 3.0
        } else {
          t.top > base + 3.0
        }) {
          continue;
        }
        if is_flat {
          above_flat += t.area as f64;
        } else if t.ny < 0.3 {
          above_up += t.area as f64;
        }
        high = high.max(t.top);
      }
      pieces.push(json!({
        "id": n.id,
        "name": n.name,
        "locked": n.locked,
        "split": n.extras.get("og_split").is_some(),
        "walls_piece": n.name.len() > 6 && n.name.ends_with("-walls"),
        "box": [v3(b.lo), v3(b.hi)],
        "area": all,
        "flat": flat / all,
        "walkable": walk / all,
        "base": base,
        "above_flat": above_flat,
        "above_upright": above_up,
        "high": high - base,
        "covered1": covered_below(&self.scene, i, 1.0),
        "covered3": covered_below(&self.scene, i, 3.0),
      }));
    }
    json!({"level": self.scene.settings.level, "pieces": pieces})
  }

  // ----------------------------------------------------------------------------------------------
  // changing things
  // ----------------------------------------------------------------------------------------------

  fn tool_set_shape(&mut self, a: &Value) -> R {
    let id = uint(a, "id");
    let p = self
      .scene
      .find(id)
      .and_then(node_shape)
      .ok_or("id: a shape (place kind 'shape')")?;
    let mut j = p.to_json();
    for k in ["size", "tile", "steps", "sides", "opening", "bend", "round"] {
      if let Some(v) = a.get(k) {
        j[k] = v.clone();
      }
    }
    let mut c = NodeChange::new(&self.scene);
    c.save(&self.scene, id);
    reshape(&mut self.scene, id, &ShapeParams::from_json(&j));
    self
      .history
      .push(c.commit(&self.scene, tr("undo.shape"), None));
    Ok(self.node_json(id))
  }

  fn tool_set_material(&mut self, a: &Value) -> R {
    let ids = ids_arg(a, "ids");
    if ids.is_empty() {
      return Err("ids: the elements".into());
    }
    let tex = str_arg(a, "texture");
    if tex.is_empty() {
      return Err("texture: an image file, or a texture name (list_textures)".into());
    }
    let mut texture = self
      .scene
      .textures
      .iter()
      .position(|t| t.name == tex)
      .map_or(-1, |i| i as i32);
    // a texture of the games' levels (Jak II's named jak2-<name>), read now
    let c = &self.catalog;
    let games = if c.game_textures.is_empty() {
      &c.palette_textures
    } else {
      &c.game_textures
    };
    let found = games.iter().find(|t| t.name == tex).or_else(|| {
      c.game_textures_jak2
        .iter()
        .find(|t| format!("jak2-{}", t.name) == tex)
    });
    if texture < 0
      && let Some(t) = found.cloned()
    {
      texture = self.palette_texture(&t, true);
    }
    let texture = match texture {
      i if i >= 0 => i,
      _ => texture_from_image(&mut self.scene, tex).map_err(|e| {
        format!(
          "texture: no texture '{tex}' in the document or the games' levels (list_textures), nor \
           an image file ({e})"
        )
      })?,
    };
    let mut c = NodeChange::new(&self.scene);
    for &id in &ids {
      c.save(&self.scene, id);
    }
    let material = int(a, "material", -1) as i32;
    let changed = edit_material(&mut self.scene, &ids, material, |m| m.texture = texture);
    if changed > 0 {
      self
        .history
        .push(c.commit(&self.scene, tr("undo.texture"), None));
    }
    Ok(json!({ "changed": changed }))
  }

  fn tool_list_textures(&mut self, a: &Value) -> R {
    let f = lower(str_arg(a, "filter"));
    let offset = int(a, "offset", 0).max(0) as usize;
    let limit = int(a, "limit", 100).clamp(1, 1000) as usize;
    let mut list = Vec::new();
    let mut matching = 0;
    if flag(a, "palette", false) {
      // the textures Jak 3's levels use again and again, the most shared first
      for t in &self.catalog.palette_textures {
        if !f.is_empty() && !lower(&t.name).contains(&f) && t.theme != f {
          continue;
        }
        matching += 1;
        if matching - 1 < offset || list.len() >= limit {
          continue;
        }
        list.push(json!({"name": t.name, "theme": t.theme, "levels": t.levels, "pieces": t.uses}));
      }
      return Ok(json!({"total": matching, "offset": offset, "textures": list}));
    }
    for t in &self.scene.textures {
      if !f.is_empty() && !lower(&t.name).contains(&f) {
        continue;
      }
      matching += 1;
      if matching - 1 < offset || list.len() >= limit {
        continue;
      }
      list.push(json!({"name": t.name, "size": [t.width, t.height]}));
    }
    Ok(json!({"total": matching, "offset": offset, "textures": list}))
  }

  fn tool_set_actor(&mut self, a: &Value) -> R {
    // an actor's behavior (merged into og_behavior) and parameters (its lumps, typed by its
    // type's code)
    let id = uint(a, "id");
    let extras = self
      .scene
      .find(id)
      .filter(|n| n.kind == NodeKind::Actor)
      .map(|n| n.extras.clone())
      .ok_or("id: an actor (find_nodes kind 'actor')")?;
    let object = |k: &str| {
      extras
        .get(k)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
    };
    let (mut b, mut lump, mut types) =
      (object("og_behavior"), object("lump"), object("lump_types"));
    let etype = str_arg(&extras, "etype");
    let data = self.game_data();
    let ps = data.as_ref().and_then(|d| d.params(etype));
    if let Some(behavior) = a.get("behavior") {
      let behavior = behavior
        .as_object()
        .ok_or("behavior: an object (see the tool's description)")?;
      // checked here: a value of the wrong kind would stop the inspector and the export
      for (k, v) in behavior {
        if v.is_null() {
          continue;
        }
        let condition = v.get("when").is_some_and(Value::is_string);
        if k == "open"
          && !v
            .as_str()
            .is_some_and(|s| ["approach", "locked", "always", "when"].contains(&s))
        {
          return Err(
            "behavior.open: \"approach\", \"locked\", \"always\" or \"when\" (with \"when\": a \
             condition, beside it: {\"open\": \"when\", \"when\": {\"when\": \"gone\", \"arg\": \
             \"crate-1\"}, \"distance\": 10})"
              .into(),
          );
        }
        if ["when", "active", "leave", "present"].contains(&k.as_str()) && !condition {
          return Err(format!(
            "behavior.{k}: a condition {{\"when\": always|never|task|pressed|gone, \"arg\": ...}}"
          ));
        }
        if k == "distance" && !v.is_number() {
          return Err("behavior.distance: meters".into());
        }
        if k == "loads" && !v.is_string() {
          return Err("behavior.loads: a level's name".into());
        }
      }
      for (k, v) in behavior {
        if v.is_null() {
          b.remove(k);
        } else {
          b.insert(k.clone(), v.clone());
        }
      }
    }
    let mut unknown = Vec::new();
    if let Some(params) = a.get("params") {
      let params = params
        .as_object()
        .ok_or("params: an object, tag -> value (get_node: params)")?;
      for (tag, v) in params {
        let Some(p) = ps.and_then(|ps| ps.iter().rfind(|q| &q.tag == tag)) else {
          unknown.push(json!(tag));
          continue;
        };
        if v.is_null() {
          lump.remove(tag);
          types.remove(tag);
          continue;
        }
        let mut value = v.clone();
        let ty = match p.kind.as_str() {
          "ints" | "eco-info" | "int" => "int32",
          k if k.starts_with("enum:") => "uint32",
          "vector" => "vector",
          "string" => "string",
          "actor" => "actor-name",
          "symbol" => {
            if let Some(s) = v.as_str().filter(|s| !s.starts_with('\'')) {
              value = json!(format!("'{s}"));
            }
            "symbol"
          }
          "float" | "floats" => "float",
          kind => {
            unknown.push(json!(format!("{tag} ({kind}: not settable)")));
            continue;
          }
        };
        lump.insert(tag.clone(), value);
        types.insert(tag.clone(), json!(ty));
      }
    }
    let mut c = NodeChange::new(&self.scene);
    c.save(&self.scene, id);
    let n = self.scene.find_mut(id).unwrap();
    if !n.extras.is_object() {
      n.extras = json!({});
    }
    let extras = n.extras.as_object_mut().unwrap();
    if b.is_empty() {
      extras.remove("og_behavior");
    } else {
      extras.insert("og_behavior".into(), json!(b));
    }
    extras.insert("lump".into(), json!(lump));
    extras.insert("lump_types".into(), json!(types));
    self
      .history
      .push(c.commit(&self.scene, tr("undo.behavior"), None));
    let mut out = json!({"behavior": b, "lump": lump});
    if !unknown.is_empty() {
      out["not_set"] = json!(unknown);
    }
    Ok(out)
  }

  fn tool_set_visibility(&mut self, a: &Value) -> R {
    let ids = ids_arg(a, "ids");
    let visible = a.get("visible").and_then(Value::as_bool);
    let (false, Some(visible)) = (ids.is_empty(), visible) else {
      return Err("ids and visible (true or false)".into());
    };
    let mut c = NodeChange::new(&self.scene);
    let mut changed = 0;
    for id in ids {
      if self.scene.find(id).is_some_and(|n| n.visible != visible) {
        c.save(&self.scene, id);
        self.scene.find_mut(id).unwrap().visible = visible;
        changed += 1;
      }
    }
    if changed > 0 {
      self
        .history
        .push(c.commit(&self.scene, tr("undo.visibility"), None));
      // a level's group shown or hidden: the ocean follows
      self.update_ocean();
    }
    Ok(json!({ "changed": changed }))
  }

  fn tool_match_lighting(&mut self, a: &Value) -> R {
    // the inspector's Match the lighting: the vertex colors of the pieces (and those under them)
    // scaled to the brightness of the document's Jak 3 decor, or by `factor`
    let ids = ids_arg(a, "ids");
    if ids.is_empty() {
      return Err("ids: the elements".into());
    }
    let factor = num(a, "factor", 0.0);
    let mut all = ids.clone();
    all.extend(self.scene.descendants_of(&ids).into_iter().flatten());
    let mut c = NodeChange::new(&self.scene);
    let mut changed = 0;
    for id in all {
      let Some(mesh) = self
        .scene
        .find(id)
        .filter(|n| n.kind == NodeKind::Render)
        .and_then(|n| n.mesh.clone())
        .filter(|m| mesh_lighting(m) != 0.0)
      else {
        continue;
      };
      let k = if factor > 0.0 {
        factor
      } else {
        1.0 / lighting_ratio(&mesh, &self.scene)
      };
      if k <= 0.0 || (k - 1.0).abs() < 1e-3 {
        continue;
      }
      c.save(&self.scene, id);
      self.scene.find_mut(id).unwrap().mesh = Some(scaled_colors(&mesh, k));
      changed += 1;
    }
    if changed > 0 {
      self
        .history
        .push(c.commit(&self.scene, tr("undo.match_lighting"), None));
    }
    Ok(json!({ "changed": changed }))
  }

  fn tool_set_decor(&mut self, a: &Value) -> R {
    // what the game does with elements of the decor: their collision, bounce, launch; a part
    // given false goes back to the game's
    let ids = ids_arg(a, "ids");
    if ids.is_empty() {
      return Err("ids: elements of the decor".into());
    }
    let mut c = NodeChange::new(&self.scene);
    let mut changed = Vec::new();
    for id in ids {
      let Some(n) = self.scene.find(id) else {
        continue;
      };
      if n.kind != NodeKind::Render || n.mesh.is_none() || self.scene.actor_of(n) != 0 {
        continue;
      }
      c.save(&self.scene, id);
      let mut d = n
        .extras
        .get("og_decor")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
      for part in ["collision", "bounce", "launch"] {
        match a.get(part) {
          Some(v) if v.is_object() => {
            d.insert(part.into(), v.clone());
          }
          Some(_) => {
            d.remove(part);
          }
          None => {}
        }
      }
      // read back as the editor keeps them (names checked, values clamped)
      let clean = decor_behavior_json(&decor_behavior_of(&json!({ "og_decor": d })));
      let n = self.scene.find_mut(id).unwrap();
      if !n.extras.is_object() {
        n.extras = json!({});
      }
      let extras = n.extras.as_object_mut().unwrap();
      if clean.is_null() {
        extras.remove("og_decor");
      } else {
        extras.insert("og_decor".into(), clean.clone());
      }
      changed.push(json!({"id": id, "decor": clean}));
    }
    if changed.is_empty() {
      return Err("ids: elements of the decor (not actors)".into());
    }
    self
      .history
      .push(c.commit(&self.scene, tr("undo.decor"), None));
    Ok(json!({ "nodes": changed }))
  }

  fn tool_export_navmesh(&mut self, a: &Value) -> R {
    let path = str_arg(a, "path");
    if path.is_empty() {
      return Err("path: the nav_data file to write (.json)".into());
    }
    let first = a
      .get("first_id")
      .and_then(Value::as_u64)
      .map_or(self.nav.first_aid, |v| v as u32);
    export_navmeshes(&self.scene, Path::new(path), first)?;
    Ok(json!({"path": path, "meshes": navmesh_nodes(&self.scene).len(), "first_id": first}))
  }

  fn tool_transform_nodes(&mut self, a: &Value) -> R {
    let ids = ids_arg(a, "ids");
    let position = vec3_arg(a, "position");
    let translate = vec3_arg(a, "translate");
    let rotation = vec3_arg(a, "rotation_deg");
    let turn = num(a, "rotate_y_deg", 0.0);
    // scale: [x, y, z] or one number for the three; scale_by multiplies the scale they have
    let scale_arg = |k: &str| {
      a.get(k).and_then(|v| {
        v.as_f64()
          .map(|k| Vec3::new(k as f32, k as f32, k as f32))
          .or_else(|| vec3_of(v))
      })
    };
    let (scale, scale_by) = (scale_arg("scale"), scale_arg("scale_by"));
    if ids.is_empty() {
      return Err("ids: the elements to move".into());
    }
    let mut c = NodeChange::new(&self.scene);
    let mut done = Vec::new();
    for &id in &ids {
      if self.scene.find(id).is_none() {
        continue;
      }
      c.save(&self.scene, id);
      if let Some(r) = rotation {
        self.scene.find_mut(id).unwrap().local.r = Quat::from_euler_deg(r);
      }
      let mut w = self.scene.world_of(id);
      if turn != 0.0 {
        let p = w.col3(3);
        let up = Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), radians(turn));
        w = Mat4::translate(p) * Mat4::rotate(up) * Mat4::translate(-p) * w;
      }
      if let Some(p) = position {
        w = Mat4::translate(p - w.col3(3)) * w;
      }
      if let Some(t) = translate {
        w = Mat4::translate(t) * w;
      }
      self.scene.set_world(id, &w);
      let n = self.scene.find_mut(id).unwrap();
      if let Some(s) = scale {
        n.local.s = s;
      }
      if let Some(k) = scale_by {
        n.local.s = Vec3::new(n.local.s.x * k.x, n.local.s.y * k.y, n.local.s.z * k.z);
      }
      let s = n.local.s;
      n.local.s = Vec3::new(s.x.max(0.01), s.y.max(0.01), s.z.max(0.01));
      done.push(id);
    }
    let moved: Vec<Value> = done.iter().map(|&id| self.node_json(id)).collect();
    if !done.is_empty() {
      self.relight_moved(&mut c, &done);
      let label = if scale.is_some() || scale_by.is_some() {
        "undo.scale"
      } else if rotation.is_some() || turn != 0.0 {
        "undo.rotate"
      } else {
        "undo.move"
      };
      self.history.push(c.commit(&self.scene, tr(label), None));
    }
    Ok(json!({ "nodes": moved }))
  }

  fn tool_delete_nodes(&mut self, a: &Value) -> R {
    let ids: Vec<u32> = ids_arg(a, "ids")
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    if ids.is_empty() {
      return Err("ids: the elements to delete".into());
    }
    let mut c = NodeChange::new(&self.scene);
    c.save_subtrees(&self.scene, &ids);
    self.scene.remove(&ids);
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.delete"), ids.len()), None));
    let gone: Vec<u32> = self
      .selection
      .iter()
      .copied()
      .filter(|&id| self.scene.find(id).is_none())
      .collect();
    for id in gone {
      self.selection.remove(&id);
    }
    self.fix_primary();
    Ok(json!({ "deleted": ids.len() }))
  }

  fn tool_duplicate_nodes(&mut self, a: &Value) -> R {
    let ids: Vec<u32> = ids_arg(a, "ids")
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    if ids.is_empty() {
      return Err("ids: the elements to copy".into());
    }
    let offset = vec3_arg(a, "offset").unwrap_or_default();
    let c = NodeChange::new(&self.scene);
    let clip = copy_nodes(&self.scene, &ids);
    let copies = paste_nodes(&mut self.scene, &clip);
    for &id in &copies {
      let w = Mat4::translate(offset) * self.scene.world_of(id);
      self.scene.set_world(id, &w);
    }
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.duplicate"), copies.len()), None));
    let list: Vec<Value> = copies.iter().map(|&id| self.node_json(id)).collect();
    Ok(json!({ "nodes": list }))
  }

  fn tool_group(&mut self, a: &Value) -> R {
    let name = str_arg(a, "name");
    if name.is_empty() {
      return Err("name: the group's".into());
    }
    let ids: Vec<u32> = ids_arg(a, "ids")
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    if ids.is_empty() {
      self.selection.clear();
      self.fix_primary();
      return Err("ids: the elements to group".into());
    }
    self.selection = ids.iter().copied().collect();
    self.fix_primary();
    let gid = self.group_selection(name);
    Ok(json!({"group": gid, "name": name, "elements": self.selection_roots().len()}))
  }

  fn tool_select(&mut self, a: &Value) -> R {
    if a.get("group").is_some() {
      let gid = uint(a, "group");
      if self.group_info(gid).is_none() {
        return Err("group: a group's id (list_groups, or 'group' of an element)".into());
      }
      self.select_group(gid);
      return Ok(self.selection_json());
    }
    if let Some(level) = a.get("level") {
      // a whole level: its group, or the root elements of the level opened
      let group = self
        .document_level_group(level, &[LevelRole::Main, LevelRole::Added])
        .ok_or("level: the level opened or a level added (get_status: document.levels)")?;
      self.select_level(group);
      return Ok(self.selection_json());
    }
    if a.get("assembly").is_some() {
      let id = uint(a, "assembly");
      if self.scene.find(id).is_none() {
        return Err("assembly: an element's id".into());
      }
      self.select_assembly(id);
      return Ok(self.selection_json());
    }
    self.selection = ids_arg(a, "ids")
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    self.primary = self.selection.first().copied().unwrap_or(0);
    Ok(self.selection_json())
  }

  // ----------------------------------------------------------------------------------------------
  // the view
  // ----------------------------------------------------------------------------------------------

  fn tool_set_view(&mut self, a: &Value) -> R {
    // the game view (the game's renderer), its hour and fog, the effects
    if let Some(on) = a.get("game_view").and_then(Value::as_bool) {
      if on && !self.view.game_view_ready {
        return Err(format!("no game view: {}", self.view.game_view_error));
      }
      self.view.game_view = on;
    }
    if let Some(h) = num_opt(a, "hour") {
      self.weather.mood.hour = h.clamp(0.0, 24.0);
    }
    // the weather: one of the game's by its name, or its cloud, fog and rain
    if let Some(w) = a.get("weather").and_then(Value::as_str)
      && !self.weather.set_weather(w)
    {
      let names: Vec<&str> = weather_presets().iter().map(|p| p.id).collect();
      return Err(format!("weather: one of {}", names.join(", ")));
    }
    let mood = &mut self.weather.mood;
    for (k, v) in [
      ("cloud", &mut mood.cloud),
      ("fog", &mut mood.fog),
      ("rain", &mut mood.rain),
    ] {
      if let Some(x) = num_opt(a, k) {
        *v = x.clamp(0.0, 1.0);
      }
    }
    if let Some(l) = a.get("level_lights").and_then(Value::as_bool) {
      mood.level_lights = l;
    }
    if let Some(l) = a.get("lightning").and_then(Value::as_bool) {
      self.weather.lightning = l;
    }
    if let Some(e) = a.get("effects").and_then(Value::as_bool) {
      self.view.effects = e;
    }
    Ok(self.view_json())
  }

  fn tool_set_camera(&mut self, a: &Value) -> R {
    // the camera computed here (render/camera.h), then given to the viewport
    let mut cam = self.camera;
    let mut framed = AABB::default();
    if let Some(f) = a.get("frame") {
      let roles = [LevelRole::Main, LevelRole::Added, LevelRole::Companion];
      if f.as_str() == Some("all") {
        // app.cpp frame_all
        cam.yaw = 0.6;
        cam.pitch = -0.45;
        framed = self.scene.focus_bounds();
      } else if let Some(level) = f.get("level") {
        let g = self
          .document_level_group(level, &roles)
          .ok_or("frame.level: a level of the document (get_status: document.levels)")?;
        framed = self.document_level_bounds(g);
        if !framed.valid() {
          return Err("frame.level: that level has nothing to frame".into());
        }
      } else if let Some(pair) = f.get("junction") {
        let group = |k: usize| {
          pair
            .as_array()
            .filter(|p| p.len() == 2)
            .and_then(|p| self.document_level_group(&p[k], &roles))
        };
        let (Some(ga), Some(gb)) = (group(0), group(1)) else {
          return Err("frame.junction: two levels of the document".into());
        };
        if ga == gb {
          return Err("frame.junction: two levels of the document".into());
        }
        framed = self.document_junction(ga, gb);
        if !framed.valid() {
          return Err("frame.junction: these two levels do not meet".into());
        }
        cam.pitch = -0.6;
      } else {
        let ids: Vec<u32> = if f.is_array() {
          ids_arg(a, "frame")
        } else {
          self.selection.iter().copied().collect()
        };
        for id in ids {
          framed.add_box(&self.subtree_bounds(id).0);
        }
        if framed.valid() {
          framed = framed.expanded(2.0);
        }
      }
      cam.frame(framed);
    }
    if let Some(p) = vec3_arg(a, "position") {
      cam.pos = p;
    }
    if let Some(t) = vec3_arg(a, "look_at") {
      cam.look_at(t);
    }
    if let Some(y) = num_opt(a, "yaw_deg") {
      cam.yaw = radians(y);
    }
    if let Some(p) = num_opt(a, "pitch_deg") {
      cam.pitch = radians(p).clamp(-1.55, 1.55);
    }
    self.camera = cam;
    let mut request = json!({"kind": "camera", "pos": cam.pos, "yaw": cam.yaw, "pitch": cam.pitch});
    if framed.valid() {
      // the viewport's speed for a box that size (the position given after it)
      request["frame"] = json!({"lo": framed.lo, "hi": framed.hi});
    }
    self.request(request);
    Ok(camera_json(&cam))
  }

  fn tool_capture_view(&mut self, a: &Value) -> Reply {
    // the picture is the interface's: asked once it listens (frontend_ready), a few seconds after
    // the editor started
    if !self.interface_ready {
      let (a, start) = (a.clone(), Instant::now());
      return Reply::wait(move |ed| {
        if ed.interface_ready {
          return Some(ed.tool_capture_view(&a));
        }
        (start.elapsed() > K_INTERFACE_WAIT).then(|| {
          Err(
            "the view could not be captured: the editor's interface is not ready (its window is              still starting, or there is none)"
              .to_string(),
          )
          .into()
        })
      });
    }
    let w = int(a, "width", 960).clamp(64, 2048);
    let h = int(a, "height", 600).clamp(64, 2048);
    // what the user's view shows by default: the game rendering at its hour when it is on
    let shown = if self.view.game_view && self.view.game_view_ready {
      self.weather.mood.hour
    } else {
      -1.0
    };
    let hour = num(a, "game_hour", shown);
    if hour >= 0.0 && !self.view.game_view_ready {
      return Err(format!("no game view: {}", self.view.game_view_error)).into();
    }
    // the picture is the viewport's: {kind: "capture", width, height, game_hour (-1: the
    // editor's own view), only: these elements and what is under them alone}
    let mut request = json!({"kind": "capture", "width": w, "height": h, "game_hour": hour});
    if a.get("only").is_some() {
      let mut keep = BTreeSet::new();
      for id in ids_arg(a, "only") {
        keep.insert(id);
        keep.extend(self.scene.descendants(id));
      }
      request["only"] = json!(keep);
    }
    let id = self.request(request);
    let start = Instant::now();
    Reply::wait(move |ed| {
      let Some(r) = ed.replies.remove(&id) else {
        return (start.elapsed() > K_CAPTURE_WAIT).then(|| {
          Err("the view could not be captured (the editor's view did not answer)".to_string())
            .into()
        });
      };
      let png = str_arg(&r, "png");
      if png.is_empty() {
        let why = str_arg(&r, "error");
        return Some(
          Err(if why.is_empty() {
            "the view could not be captured".to_string()
          } else {
            format!("the view could not be captured: {why}")
          })
          .into(),
        );
      }
      if let Some(c) = r
        .get("camera")
        .and_then(|c| serde_json::from_value::<CameraState>(c.clone()).ok())
      {
        ed.camera = c;
      }
      Some(Reply::Now(json!({
        "ok": true,
        "result": {"width": w, "height": h, "camera": camera_json(&ed.camera)},
        "image": png,
      })))
    })
  }
}

// ------------------------------------------------------------------------------------------------
// the catalog, prefabs, placing, nav meshes, the export: what waits for the library's workers
// ------------------------------------------------------------------------------------------------

/// the shape presets' names, as the tool says them
const SHAPES: &str = "name: box, floor, wall, doorway, window, pillar, ramp, stairs, roof, roof-hip, cylinder or ground";

impl Editor {
  /// `f` now, else asked again until it answers (the library's workers polled in between: the
  /// plugin's timer does it too)
  fn now_or_wait(
    &mut self,
    mut f: impl FnMut(&mut Editor) -> Option<Reply> + Send + 'static,
  ) -> Reply {
    match f(self) {
      Some(r) => r,
      None => Reply::wait(move |ed| {
        ed.poll_library_work();
        f(ed)
      }),
    }
  }

  /// the library's index merged into the catalog (its variants, effects, decor parts)
  fn indexed(&self, jak2: bool) -> bool {
    self.catalog_state()[if jak2 { "ready_jak2" } else { "ready" }] == true
  }

  /// Some(the tool asked again once the index is merged) while it is not
  fn when_indexed(&mut self, jak2: bool, tool: &str, a: &Value) -> Option<Reply> {
    if self.indexed(jak2) {
      return None;
    }
    let (tool, a) = (tool.to_string(), a.clone());
    Some(self.now_or_wait(move |ed| ed.indexed(jak2).then(|| ed.remote_tool(&tool, &a))))
  }

  fn tool_list_catalog(&mut self, a: &Value) -> Reply {
    if let Some(w) = self.when_indexed(false, "list_catalog", a) {
      return w;
    }
    let family = str_arg(a, "family");
    let f = lower(str_arg(a, "filter"));
    let offset = int(a, "offset", 0).max(0) as usize;
    let limit = int(a, "limit", 100).clamp(1, 1000) as usize;
    // the objects that make up a place (props, doors, water, effects), or the interactive ones
    let group = str_arg(a, "group");
    let mut objects = Vec::new();
    let mut matching = 0;
    for o in &self.catalog.objects {
      let effect = o.family == "effect";
      if (!family.is_empty() && o.family != family)
        || (group == "objects" && (o.interactive || effect))
        || (group == "interactive" && !o.interactive)
        || (group == "effects" && !effect)
        || (!f.is_empty() && !o.name.contains(&f) && !o.etype.contains(&f))
      {
        continue;
      }
      matching += 1;
      if matching - 1 < offset || objects.len() >= limit {
        continue;
      }
      let g = if effect {
        "effects"
      } else if o.interactive {
        "interactive"
      } else {
        "objects"
      };
      let mut j = json!({"name": o.name, "etype": o.etype, "family": o.family, "group": g});
      if effect {
        j["effect_kind"] = json!(effect_kind(o));
      }
      if o.model.is_empty() {
        j["drawn_without_model"] = json!(true);
      } else {
        j["model"] = json!(o.model);
      }
      if o.count != 0 {
        j["in_game"] = json!(o.count);
      }
      objects.push(j);
    }
    let mut families: Vec<&str> = Vec::new();
    for o in &self.catalog.objects {
      if families.last() != Some(&o.family.as_str()) {
        families.push(&o.family);
      }
    }
    Ok(json!({"total": matching, "offset": offset, "objects": objects, "families": families}))
      .into()
  }

  fn tool_list_models(&mut self, a: &Value) -> Reply {
    if let Some(w) = self.when_indexed(false, "list_models", a) {
      return w;
    }
    // Jak II's objects by their model (game: "jak2"), placed as decor, or Jak 3's models
    let jak2 = str_arg(a, "game") == "jak2";
    if jak2 && self.library_jak2.is_empty() {
      return Err(NO_JAK2.to_string()).into();
    }
    let f = lower(str_arg(a, "filter"));
    let offset = int(a, "offset", 0).max(0) as usize;
    let limit = int(a, "limit", 100).clamp(1, 1000) as usize;
    let mut models = Vec::new();
    let mut matching = 0;
    let all = if jak2 {
      &self.catalog.models_jak2
    } else {
      &self.catalog.models
    };
    for m in all {
      if !f.is_empty() && !m.name.contains(&f) && !m.group.contains(&f) {
        continue;
      }
      matching += 1;
      if matching - 1 < offset || models.len() >= limit {
        continue;
      }
      models.push(json!({"name": m.name, "from": m.group}));
    }
    Ok(json!({"total": matching, "offset": offset, "models": models})).into()
  }

  fn tool_list_decor(&mut self, a: &Value) -> Reply {
    // Jak II's structures (game: "jak2"), brought into Jak 3 levels, or Jak 3's decor
    let jak2 = str_arg(a, "game") == "jak2";
    if jak2 && self.library_jak2.is_empty() {
      return Err(NO_JAK2.to_string()).into();
    }
    let name = str_arg(a, "level").to_string();
    let f = lower(str_arg(a, "filter"));
    let offset = int(a, "offset", 0).max(0) as usize;
    let limit = int(a, "limit", 100).clamp(1, 1000) as usize;
    let kind = |p: &CatalogPart| {
      if p.kind == PartKind::Prototype {
        "prototype"
      } else {
        "piece"
      }
    };
    if name.is_empty() || name == "all" {
      // the decor of every level: once the editor has read them
      if let Some(w) = self.when_indexed(jak2, "list_decor", a) {
        return w;
      }
      let mut parts = Vec::new();
      let mut matching = 0;
      let all = if jak2 {
        &self.catalog.all_parts_jak2
      } else {
        &self.catalog.all_parts
      };
      for e in all {
        if !f.is_empty() && !e.key.contains(&f) {
          continue;
        }
        matching += 1;
        if matching - 1 < offset || parts.len() >= limit {
          continue;
        }
        let mut j = json!({"name": e.part.name, "level": e.level, "kind": kind(&e.part),
          "triangles": e.part.triangles});
        if e.levels > 1 {
          j["in_levels"] = json!(e.levels);
        }
        parts.push(j);
      }
      return Ok(json!({"total": matching, "offset": offset, "parts": parts})).into();
    }
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    let Some(background) = lib.level(&name).map(|l| l.background.clone()) else {
      return Err(format!(
        "no level '{name}' in the library (see list_levels)"
      ))
      .into();
    };
    self.now_or_wait(move |ed| {
      let Some(scene) = ed.decor_scene_of(&background) else {
        return ed
          .catalog
          .store
          .failed_level(&background)
          .then(|| Err(format!("the decor of {name} cannot be read")).into());
      };
      let structures_only = ed.library_jak2.level_of(&background).is_some();
      let mut parts = Vec::new();
      let mut matching = 0;
      for p in list_parts(&scene) {
        if (!f.is_empty() && !lower(&p.name).contains(&f))
          || (structures_only && !jak2_structure(&p))
        {
          continue;
        }
        matching += 1;
        if matching - 1 < offset || parts.len() >= limit {
          continue;
        }
        let mut j = json!({"name": p.name, "kind": kind(&p), "triangles": p.triangles});
        if p.kind == PartKind::Prototype {
          j["copies_in_level"] = json!(p.copies);
        }
        parts.push(j);
      }
      Some(Ok(json!({"level": name, "total": matching, "offset": offset, "parts": parts})).into())
    })
  }

  fn tool_list_prefabs(&mut self, a: &Value) -> Reply {
    self.refresh_prefabs();
    // make: the game's prefabs not made yet made first from the extracted levels (waits); else
    // each is made with its level's when it is placed
    let rebuild = flag(a, "rebuild", false);
    if rebuild || flag(a, "make", false) {
      self.start_game_prefabs(rebuild, &BTreeSet::new());
    }
    let runs = self.catalog.game_prefab_runs;
    let waiting = self.game_prefabs_busy();
    self.now_or_wait(move |ed| {
      if waiting && ed.catalog.game_prefab_runs == runs && ed.game_prefabs_busy() {
        return None;
      }
      let mine: Vec<&str> = ed.catalog.prefabs.iter().map(|p| p.name.as_str()).collect();
      let game: Vec<Value> = ed
        .catalog
        .game_prefabs
        .iter()
        .map(|p| {
          let g = if p.game.is_empty() { "jak3" } else { &p.game };
          json!({"name": p.name, "category": p.category, "description": p.description,
            "game": g, "level": p.level, "made": p.made})
        })
        .collect();
      Some(Ok(json!({"prefabs": mine, "game_prefabs": game})).into())
    })
  }

  fn tool_create_prefab(&mut self, a: &Value) -> R {
    let ids = ids_arg(a, "ids");
    let name = str_arg(a, "name");
    if ids.is_empty() || name.is_empty() {
      return Err("ids and name".into());
    }
    self.selection = ids
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    self.fix_primary();
    if self.selection.is_empty() {
      return Err("no element with those ids".into());
    }
    // the small pieces left out (bolts, debris, stains): 0.5 m by default
    let mut left_out = Vec::new();
    let min_size = num(a, "min_size", 0.5).max(0.0);
    let path = self.create_prefab(name, min_size, Some(&mut left_out));
    if path.is_empty() {
      return Err("the prefab could not be saved (see the editor's log)".into());
    }
    // its name (<folder>/<name>.glb): the one asked, or with -2, -3... when a prefab has it already
    let stem = Path::new(&path)
      .file_stem()
      .map(|s| s.to_string_lossy().into_owned())
      .unwrap_or_default();
    let prefabs: Vec<&str> = self
      .catalog
      .prefabs
      .iter()
      .map(|p| p.name.as_str())
      .collect();
    Ok(json!({"name": stem, "left_out": left_out, "prefabs": prefabs}))
  }

  /// place: what to put by its kind and name, where (remote_place)
  fn remote_place(&mut self, a: &Value) -> Reply {
    let fail = |e: String| -> Reply { Err(e).into() };
    let kind = a.get("kind").and_then(Value::as_str).unwrap_or("object");
    let name = str_arg(a, "name");
    let Some(position) = vec3_arg(a, "position") else {
      return fail("position: [x, y, z] in meters".into());
    };
    if !self.document_open() {
      return fail("no document: open_level or new_level first".into());
    }
    let yaw = radians(num(a, "yaw_deg", 0.0));
    let on_ground = flag(a, "on_ground", false);
    // a full rotation (degrees around x, y, z) instead of the yaw, a scale ([x, y, z] or one
    // number)
    let rotation = vec3_arg(a, "rotation_deg");
    let s = match a.get("scale").and_then(Value::as_f64) {
      Some(k) => Vec3::new(k as f32, k as f32, k as f32),
      None => vec3_arg(a, "scale").unwrap_or(Vec3::new(1.0, 1.0, 1.0)),
    };
    let scale = Vec3::new(s.x.max(0.01), s.y.max(0.01), s.z.max(0.01));
    let jak2 = str_arg(a, "game") == "jak2";

    // what to place: by name
    match kind {
      "model" if a.get("path").is_some() => {
        // a model made elsewhere
        let path = str_arg(a, "path");
        let file = Path::new(path);
        if !file.is_file() {
          return fail(format!("path: no file {path}"));
        }
        let stem = file
          .file_stem()
          .map(|s| s.to_string_lossy().into_owned())
          .unwrap_or_default();
        let model = CatalogModel {
          name: if name.is_empty() { stem } else { name.into() },
          path: path.into(),
          ..Default::default()
        };
        self.start_model_placement(&model, false);
      }
      "model" => {
        let models = if jak2 {
          &self.catalog.models_jak2
        } else {
          &self.catalog.models
        };
        let Some(found) = models.iter().rfind(|m| m.name == name).cloned() else {
          return fail(format!(
            "no model '{name}' in the catalog (see list_models)"
          ));
        };
        self.start_model_placement(&found, false);
      }
      "shape" => {
        let Some(preset) = shape_preset(name) else {
          return fail(SHAPES.into());
        };
        // in the style of a level: its floors' (a floor, a ground) or its walls' texture, size
        // and light, its ground's bumps; measured once its decor is read
        let style = str_arg(a, "style");
        if !style.is_empty() {
          let Some(bg) = self.library.level(style).map(|l| l.background.clone()) else {
            return fail(format!("style: no level '{style}' (see list_levels)"));
          };
          if self.level_style(style).is_none() {
            if self.catalog.store.failed_level(&bg) {
              return fail(format!("style: the decor of {style} cannot be read"));
            }
            let (a, style) = (a.clone(), style.to_string());
            return self.now_or_wait(move |ed| {
              (ed.level_style(&style).is_some() || ed.catalog.store.failed_level(&bg))
                .then(|| ed.remote_place(&a))
            });
          }
          self.start_style_placement(style, name == "floor" || name == "ground", name);
        }
        let mut j = if style.is_empty() {
          preset.params.to_json()
        } else {
          self.placement().shape.to_json()
        };
        for k in [
          "size",
          "tile",
          "steps",
          "sides",
          "opening",
          "bend",
          "round",
          "bumps",
          "bump_size",
          "seed",
        ] {
          if let Some(v) = a.get(k) {
            j[k] = v.clone();
          }
        }
        let mut p = preset.clone();
        p.params = ShapeParams::from_json(&j);
        let material = if style.is_empty() {
          -1
        } else {
          self.placement().material
        };
        self.start_shape_placement(&p, false);
        self.placement_mut().material = material;
      }
      "object" => {
        let objects = &self.catalog.objects;
        let Some(found) = objects
          .iter()
          .rfind(|o| o.name == name)
          .or_else(|| objects.iter().find(|o| o.etype == name))
          .cloned()
        else {
          return fail(format!(
            "no object '{name}' in the catalog (see list_catalog)"
          ));
        };
        self.start_object_placement(&found, false);
      }
      "prefab" => {
        self.refresh_prefabs();
        let c = &self.catalog;
        let Some(found) = c
          .prefabs
          .iter()
          .chain(&c.game_prefabs)
          .rfind(|p| p.name == name)
          .cloned()
        else {
          return fail(format!("no prefab '{name}' (see list_prefabs)"));
        };
        if !found.made {
          // a game's prefab not made yet: made with its level's, then placed
          let runs = self.catalog.game_prefab_runs;
          self.want_game_prefabs(&found.game, &found.level);
          let a = a.clone();
          return Reply::wait(move |ed| {
            ed.poll_library_work();
            if ed.catalog.game_prefab_runs == runs || ed.game_prefabs_busy() {
              return None;
            }
            let made = ed
              .catalog
              .game_prefabs
              .iter()
              .any(|p| p.name == found.name && p.made);
            Some(if made {
              ed.remote_place(&a)
            } else {
              Err("the prefab could not be made from its level (see the editor's log)".to_string())
                .into()
            })
          });
        }
        self.start_placement(&found.path, &found.name, false);
      }
      "decor" => {
        // a structure of Jak II (game: "jak2"): converted for Jak 3 (io/jak2_import)
        let mut level_name = str_arg(a, "level").to_string();
        let mb = format!("{name}.mb");
        if level_name.is_empty() {
          // the first level with a part of that name
          if let Some(w) = self.when_indexed(jak2, "place", a) {
            return w;
          }
          let all = if jak2 {
            &self.catalog.all_parts_jak2
          } else {
            &self.catalog.all_parts
          };
          let Some(e) = all
            .iter()
            .find(|e| e.part.name == name || e.part.name == mb)
          else {
            return fail(format!(
              "no decor part '{name}' in the levels (see list_decor)"
            ));
          };
          level_name = e.level.clone();
        }
        let lib = if jak2 {
          &self.library_jak2
        } else {
          &self.library
        };
        let Some(level) = lib.level(&level_name).cloned() else {
          return fail("level: the level the decor part comes from (see list_decor)".into());
        };
        let Some(source) = self.decor_scene_of(&level.background) else {
          if self.catalog.store.failed_level(&level.background) {
            return fail("the decor of that level cannot be read".into());
          }
          // loading: tried again
          let a = a.clone();
          return self.now_or_wait(move |ed| {
            let bg = &level.background;
            (ed.decor_scene_of(bg).is_some() || ed.catalog.store.failed_level(bg))
              .then(|| ed.remote_place(&a))
          });
        };
        let parts = list_parts(&source);
        let Some(found) = parts.iter().rfind(|p| p.name == name || p.name == mb) else {
          return fail(format!(
            "no decor part '{name}' in {} (see list_decor)",
            level.name
          ));
        };
        self.start_part_placement(&level.background, found, false);
      }
      _ => return fail("kind: object, model, decor, prefab or shape".into()),
    }
    self.placement_mut().yaw = yaw;

    // where: on the surface below, when asked
    let mut p = position;
    if on_ground {
      let f = PickFilter {
        actors: false,
        include_locked: true,
        ..Default::default()
      };
      let ray = Ray::new(p + Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
      let hit = pick(&self.scene, &ray, &f);
      if hit.hit() {
        p = hit.point;
      }
    }
    let rot = rotation.map_or(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), yaw), |r| {
      Quat::from_euler_deg(r)
    });
    let m = Mat4::translate(p) * Mat4::rotate(rot) * Mat4::scale(scale);
    let prefab = kind == "prefab";
    // its model or its prefab may still be loading
    self.now_or_wait(move |ed| {
      let pl = ed.placement();
      let markerless = pl.kind == PlacementKind::Object && pl.path.is_empty();
      if !prefab && !markerless && ed.placement_mesh().is_none() {
        if !ed.placement_failed() {
          return None;
        }
        ed.stop_placement();
        return Some(Err("its model cannot be read".to_string()).into());
      }
      let roots = ed.place_at(&m);
      ed.stop_placement();
      if roots.is_empty() {
        return Some(Err("nothing was placed".to_string()).into());
      }
      let nodes: Vec<Value> = roots.iter().map(|&id| ed.node_json(id)).collect();
      Some(Ok(json!({ "nodes": nodes })).into())
    })
  }

  fn tool_place_many(&mut self, a: &Value) -> Reply {
    let Some(items) = a
      .get("items")
      .and_then(Value::as_array)
      .filter(|l| !l.is_empty())
      .cloned()
    else {
      return Err("items: a list of placements".to_string()).into();
    };
    if items.len() > 2000 {
      return Err("items: at most 2000 at once".to_string()).into();
    }
    if !self.document_open() {
      return Err("no document: open_level or new_level first".to_string()).into();
    }
    // one undo entry for the whole batch, lit like the level around it at its end
    self.begin_place_batch();
    let mut placed: Vec<Value> = Vec::new();
    let mut next = 0;
    let mut current: Option<Reply> = None;
    self.now_or_wait(move |ed| {
      loop {
        let reply = match current.take() {
          Some(r) => r,
          None if next < items.len() => ed.remote_place(&items[next]),
          None => {
            ed.end_place_batch();
            let count = placed.iter().filter(|p| p.get("id").is_some()).count();
            let nodes = std::mem::take(&mut placed);
            return Some(Ok(json!({"placed": count, "nodes": nodes})).into());
          }
        };
        match reply {
          Reply::Now(answer) => {
            if answer["ok"] == true {
              placed.extend(
                answer["result"]["nodes"]
                  .as_array()
                  .cloned()
                  .unwrap_or_default(),
              );
            } else {
              placed.push(json!({"index": next, "error": answer["error"]}));
            }
            next += 1;
          }
          Reply::Wait(mut w) => match w(ed) {
            Some(r) => current = Some(r),
            None => {
              current = Some(Reply::Wait(w));
              return None;
            }
          },
        }
      }
    })
  }

  fn tool_generate_navmesh(&mut self, a: &Value) -> Reply {
    // the parameters given, the others as the Nav meshes window has them
    let mut params = Map::new();
    for k in [
      "cell_size",
      "agent_radius",
      "agent_height",
      "max_climb",
      "max_slope",
      "max_edge",
      "max_error",
      "min_region",
    ] {
      if let Some(v) = a.get(k).filter(|v| v.is_number()) {
        params.insert(k.into(), v.clone());
      }
    }
    let mut call = json!({ "params": params });
    for k in ["region", "from"] {
      if let Some(v) = a.get(k) {
        call[k] = v.clone();
      }
    }
    let generation = self.nav.generation;
    if let Err(e) = self.generate_navmesh_call(&call) {
      return Err(e).into();
    }
    self.now_or_wait(move |ed| {
      if ed.nav.generation == generation {
        return None;
      }
      let r = match &ed.nav.last {
        Some(Ok(s)) => {
          // the new meshes are the selection
          let meshes: Vec<Value> = ed.selection.iter().map(|&id| ed.node_json(id)).collect();
          Ok(json!({
            "meshes": meshes,
            "polygons": s.polys,
            "pieces_split": s.split,
            "pieces_left_out": s.left_out,
            "triangles_read": s.triangles,
            "grid": [s.grid_width, s.grid_depth],
            "seconds": (s.ms / 100.0).round() / 10.0,
          }))
        }
        Some(Err(e)) => Err(format!("nav mesh not generated: {e}")),
        None => Err("nav mesh not generated".to_string()),
      };
      Some(r.into())
    })
  }

  fn tool_export_level(&mut self, a: &Value) -> Reply {
    if !self.document_open() {
      return Err("no level open: open_level or open_project first".to_string()).into();
    }
    let mut o = self.default_export_options();
    o.name = str_arg(a, "name").to_string();
    // the default nickname is one no level of the game has
    let data = self.document_game_data();
    o.nickname = match a.get("nickname").and_then(Value::as_str) {
      Some(n) => n.to_string(),
      None => untaken_nickname(&default_nickname(&o.name), data.as_deref()),
    };
    o.iso_name = match a.get("iso_name").and_then(Value::as_str) {
      Some(n) => n.to_string(),
      None => default_iso_name(&o.name),
    };
    o.replaces = match a.get("replaces").and_then(Value::as_str) {
      Some(r) => r.to_string(),
      None => self.scene.settings.replaces.clone(),
    };
    if let Some(id) = a.get("base_id").and_then(Value::as_u64) {
      o.base_id = id as u32;
    }
    if let Some(f) = a.get("folder").and_then(Value::as_str) {
      o.folder = f.to_string();
    }
    o.actors = flag(a, "actors", true);
    let collision = a.get("collision").and_then(Value::as_str).unwrap_or("game");
    if collision != "game" && collision != "decor" {
      return Err("collision: 'game' or 'decor'".to_string()).into();
    }
    o.game_collision = collision == "game";
    if let Some(s) = vec3_arg(a, "start") {
      o.start = s;
    }
    if let Some(bad) = self.export_check(&o) {
      return Err(tr_en(bad).to_string()).into();
    }
    let start = o.start;
    let generation = self.export.generation;
    if let Err(e) = self.export_for_game(o, false) {
      return Err(e).into();
    }
    self.now_or_wait(move |ed| {
      if ed.export.generation == generation {
        return None;
      }
      let Some(r) = ed
        .export
        .report
        .as_ref()
        .filter(|_| ed.export.error.is_empty())
      else {
        return Some(Err(ed.export.error.clone()).into());
      };
      let notes: Vec<Value> = r
        .notes
        .iter()
        .map(|n| {
          json!({"key": n.key, "count": n.count, "detail": n.detail,
            "text": strf!(tr_en(&n.key), n.count, &n.detail)})
        })
        .collect();
      let dense: Vec<Value> = r
        .densest
        .iter()
        .map(|&(n, at)| json!({"triangles": n, "position": v3(at)}))
        .collect();
      Some(
        Ok(json!({
          "folder": r.dir,
          "dense_places": r.dense_places,
          "densest": dense,
          "level_folder": r.level_dir,
          "files": r.files,
          "meshes": r.meshes,
          "triangles": r.triangles,
          "collision_triangles": r.collision_triangles,
          "meshes_colliding_by_shape": r.collision_from_decor,
          "actors": r.actors,
          "art_groups": r.art_groups,
          "navmeshes": r.navmeshes,
          "start": v3(start),
          "notes": notes,
        }))
        .into(),
      )
    })
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Mutex;

  use super::*;
  use crate::editor::remote::run_tool;
  use crate::io::test_library::real_library;
  use crate::scene::Node;
  use crate::scene::shapes::{make_shape, shape_preset};

  fn editor() -> Mutex<Editor> {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    Mutex::new(ed)
  }

  fn ok(ed: &Mutex<Editor>, tool: &str, args: Value) -> Value {
    let a = run_tool(ed, tool, &args);
    assert_eq!(a["ok"], true, "{tool}: {a}");
    a["result"].clone()
  }

  fn fail(ed: &Mutex<Editor>, tool: &str, args: Value) -> String {
    let a = run_tool(ed, tool, &args);
    assert_eq!(a["ok"], false, "{tool}: {a}");
    a["error"].as_str().unwrap().to_string()
  }

  /// a new level holding a floor shape and a box shape on it
  fn test_document(ed: &Mutex<Editor>) -> (u32, u32) {
    ok(ed, "new_level", json!({}));
    let mut e = ed.lock().unwrap();
    let material = crate::scene::shapes::shape_default_material(&mut e.scene);
    let mut add = |name: &str, preset: &str, size: Vec3, at: Vec3| {
      let mut p = shape_preset(preset).unwrap().params.clone();
      p.size = size;
      let mut n = Node {
        name: name.into(),
        kind: NodeKind::Render,
        mesh: Some(make_shape(&p, material)),
        ..Default::default()
      };
      n.local.t = at;
      n.extras["og_shape"] = p.to_json();
      e.scene.add(n)
    };
    let floor = add(
      "floor-1",
      "floor",
      Vec3::new(20.0, 0.5, 20.0),
      Vec3::default(),
    );
    let crate_box = add(
      "box-1",
      "box",
      Vec3::new(2.0, 2.0, 2.0),
      Vec3::new(3.0, 0.5, 4.0),
    );
    e.scene.begin_frame();
    (floor, crate_box)
  }

  #[test]
  fn find_transform_undo_on_a_test_document() {
    let ed = editor();
    let (floor, b) = test_document(&ed);
    let status = ok(&ed, "get_status", json!({}));
    assert_eq!(status["document"]["open"], true);
    assert_eq!(status["document"]["elements"], 2);
    assert_eq!(status["document"]["levels"][0]["role"], "opened");

    // by name, near a point
    let found = ok(&ed, "find_nodes", json!({"name": "BOX"}));
    assert_eq!(found["total"], 1);
    assert_eq!(found["nodes"][0]["id"], b);
    assert_eq!(found["nodes"][0]["kind"], "render");
    assert_eq!(found["nodes"][0]["size"], json!([2.0, 2.0, 2.0]));
    let near = ok(&ed, "find_nodes", json!({"near": [3, 1.5, 4], "radius": 1}));
    assert_eq!(near["nodes"].as_array().unwrap().len(), 1);
    let boxed = ok(
      &ed,
      "find_nodes",
      json!({"box": [[-1, -1, -1], [1, 1, 1]], "outside": true}),
    );
    assert_eq!(boxed["nodes"][0]["id"], b);

    // the floor under a point
    let hit = ok(&ed, "raycast", json!({"origin": [-5, 10, -5]}));
    assert_eq!(hit["hit"], true);
    assert_eq!(hit["node"]["id"], floor);
    assert_eq!(hit["point"][1], 0.5);

    // moved, turned, scaled: one undo step each
    let moved = ok(
      &ed,
      "transform_nodes",
      json!({"ids": [b], "position": [10, 0.5, 0], "scale_by": 2}),
    );
    assert_eq!(moved["nodes"][0]["position"], json!([10.0, 0.5, 0.0]));
    assert_eq!(moved["nodes"][0]["size"], json!([4.0, 4.0, 4.0]));
    let undone = ok(&ed, "undo", json!({}));
    assert_eq!(undone["done"], true);
    assert_eq!(undone["change"], tr("undo.scale"));
    let back = ok(&ed, "get_node", json!({"id": b}));
    assert_eq!(back["position"], json!([3.0, 0.5, 4.0]));
    assert_eq!(back["shape"]["kind"], "box");
    ok(&ed, "redo", json!({}));
    assert_eq!(
      ok(&ed, "get_node", json!({"id": b}))["position"],
      json!([10.0, 0.5, 0.0])
    );

    // a shape resized, copied, deleted, grouped
    let resized = ok(&ed, "set_shape", json!({"id": b, "size": [1, 3, 1]}));
    assert_eq!(resized["size"][1], 6.0); // scaled by 2
    let copies = ok(
      &ed,
      "duplicate_nodes",
      json!({"ids": [b], "offset": [0, 0, 5]}),
    );
    let copy = copies["nodes"][0]["id"].as_u64().unwrap() as u32;
    assert_eq!(copies["nodes"][0]["position"], json!([10.0, 0.5, 5.0]));
    let g = ok(&ed, "group", json!({"ids": [b, copy], "name": "pair"}));
    let groups = ok(&ed, "list_groups", json!({}));
    assert_eq!(groups["groups"][0]["group"], g["group"]);
    assert_eq!(groups["groups"][0]["ids"].as_array().unwrap().len(), 2);
    let sel = ok(&ed, "select", json!({"group": g["group"]}));
    assert_eq!(sel["selection"].as_array().unwrap().len(), 2);
    assert_eq!(
      ok(&ed, "delete_nodes", json!({"ids": [copy]}))["deleted"],
      1
    );
    assert!(fail(&ed, "get_node", json!({"id": copy})).contains("no element"));
    ok(&ed, "undo", json!({}));
    assert_eq!(ok(&ed, "get_node", json!({"id": copy}))["id"], copy);

    // hidden, then shown back
    assert_eq!(
      ok(
        &ed,
        "set_visibility",
        json!({"ids": [floor], "visible": false})
      )["changed"],
      1
    );
    assert_eq!(
      ok(&ed, "find_nodes", json!({"name": "floor"}))["nodes"][0]["hidden"],
      true
    );

    // the camera computed here, given to the viewport
    let cam = ok(
      &ed,
      "set_camera",
      json!({"position": [0, 10, 10], "look_at": [0, 0, 0]}),
    );
    assert_eq!(cam["position"], json!([0.0, 10.0, 10.0]));
    assert_eq!(cam["pitch_deg"], -45.0);
    assert!(fail(&ed, "set_view", json!({"weather": "snow"})).starts_with("weather: one of clear"));
    assert_eq!(ok(&ed, "set_view", json!({"hour": 30}))["hour"], 24.0);

    // the arguments checked as the C++ did
    assert_eq!(
      fail(&ed, "transform_nodes", json!({})),
      "ids: the elements to move"
    );
    assert_eq!(fail(&ed, "raycast", json!({})), "origin: [x, y, z]");
    assert_eq!(
      fail(&ed, "set_shape", json!({"id": floor + 100})),
      "id: a shape (place kind 'shape')"
    );
    assert_eq!(
      fail(&ed, "place", json!({"kind": "shape", "name": "box"})),
      "position: [x, y, z] in meters"
    );
  }

  // what was opened is framed at once (app.cpp frame_all): the answer gives the camera the C++
  // gave, not the one before the viewport framed it
  #[test]
  fn a_project_opened_is_framed() {
    let ed = editor();
    test_document(&ed);
    let dir = crate::io::test_library::temp_dir("remote-framed");
    let path = dir.join("framed.glb").to_string_lossy().replace('\\', "/");
    ok(&ed, "save_project", json!({ "path": path }));
    let status = ok(&ed, "open_project", json!({ "path": path }));
    let cam = &status["camera"];
    assert_eq!(cam["yaw_deg"], json!(round3(0.6f32.to_degrees())));
    assert_eq!(cam["pitch_deg"], json!(round3((-0.45f32).to_degrees())));
    // the middle of what is open straight ahead
    let f = |v: &Value, k: usize| v[k].as_f64().unwrap();
    let b = &status["document"]["bounds"];
    let to: Vec<f64> = (0..3)
      .map(|k| (f(&b["min"], k) + f(&b["max"], k)) / 2.0 - f(&cam["position"], k))
      .collect();
    let d = to.iter().map(|x| x * x).sum::<f64>().sqrt();
    let along: f64 = (0..3).map(|k| to[k] / d * f(&cam["forward"], k)).sum();
    assert!(along > 0.999, "{along}");
    let _ = std::fs::remove_dir_all(dir);
  }

  // the land around the ocean still being made: the document is loading (the C++ waited on it)
  #[test]
  fn loading_while_the_ocean_land_is_made() {
    let ed = editor();
    test_document(&ed);
    assert_eq!(
      ok(&ed, "get_status", json!({}))["document"]["loading"],
      false
    );
    let job = Default::default();
    ed.lock().unwrap().jobs.insert("ocean_land".into(), job);
    assert_eq!(
      ok(&ed, "get_status", json!({}))["document"]["loading"],
      true
    );
  }

  // the export writes the models as they are: an animation preview going on is stopped first
  // (App::export_for_game's stop_preview)
  #[test]
  fn export_stops_the_animation_preview() {
    let ed = editor();
    test_document(&ed);
    let model = {
      let mut e = ed.lock().unwrap();
      let a = e.scene.add(Node {
        name: "fac-elevator".into(),
        kind: NodeKind::Actor,
        extras: json!({"etype": "fac-elevator"}),
        ..Default::default()
      });
      let m = e.scene.add(Node {
        name: "fac-elevator-model".into(),
        parent: a,
        kind: NodeKind::Render,
        mesh: Some(crate::io::test_library::make_box("fac-elevator", -1)),
        extras: json!({"og_actor_visual": "fac-elevator"}),
        ..Default::default()
      });
      e.scene.begin_frame();
      e.call("start_preview", &json!({"id": a, "what": "motion"}))
        .unwrap();
      m
    };
    assert_eq!(ed.lock().unwrap().get_state()["preview"]["node"], model);
    let dir = crate::io::test_library::temp_dir("remote-export-preview");
    let folder = dir.to_string_lossy().replace('\\', "/");
    run_tool(
      &ed,
      "export_level",
      &json!({"name": "qatest", "folder": folder}),
    );
    assert_eq!(ed.lock().unwrap().get_state()["preview"]["node"], 0);
    let _ = std::fs::remove_dir_all(dir);
  }

  // the region's error names the keys the tool's description gives (the C++ text)
  #[test]
  fn navmesh_region_error_says_min_max() {
    let ed = editor();
    test_document(&ed);
    assert_eq!(
      fail(
        &ed,
        "generate_navmesh",
        json!({"region": {"min": [0, 0, 0]}})
      ),
      "region: \"all\", \"selection\" or {\"min\": [x, y, z], \"max\": [x, y, z]}"
    );
  }

  // the game's numbers read as the C++ read them (strtod, correctly rounded): an actor's bsphere of
  // hiphog was one ulp off in the projects saved (serde_json's float_roundtrip)
  #[test]
  fn numbers_read_correctly_rounded() {
    let text = "13.193374633789063";
    let v: Value = serde_json::from_str(text).unwrap();
    assert_eq!(v.as_f64(), text.parse::<f64>().ok());
  }

  #[test]
  fn capture_view_answers_the_viewports_picture() {
    let ed = std::sync::Arc::new(editor());
    test_document(&ed);
    assert_eq!(ok(&ed, "get_status", json!({}))["interface_ready"], false);
    // the viewport: ready a moment later (the capture waits for it), then answers the request it
    // was sent (jle:request {kind: "capture", id: 1})
    let viewport = {
      let ed = ed.clone();
      std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(ed.lock().unwrap().next_request, 1);
        ed.lock()
          .unwrap()
          .call("frontend_ready", &Value::Null)
          .unwrap();
        for _ in 0..2000 {
          {
            let mut e = ed.lock().unwrap();
            if e.next_request > 1 {
              let id = e.next_request - 1;
              let camera = json!({"pos": [1, 2, 3], "yaw": 0.0, "pitch": 0.0, "fov": 60.0});
              e.call(
                "frontend_reply",
                &json!({"id": id, "png": "iVBORw0KGgo=", "width": 320, "height": 200, "camera": camera}),
              )
              .unwrap();
              return;
            }
          }
          std::thread::sleep(Duration::from_millis(2));
        }
      })
    };
    let a = run_tool(&ed, "capture_view", &json!({"width": 320, "height": 200}));
    viewport.join().unwrap();
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(a["image"], "iVBORw0KGgo=");
    assert_eq!(a["result"]["width"], 320);
    assert_eq!(a["result"]["camera"]["position"], json!([1.0, 2.0, 3.0]));
    assert_eq!(ok(&ed, "get_status", json!({}))["interface_ready"], true);
  }

  #[test]
  fn levels_of_the_real_library() {
    let Some(root) = real_library() else {
      return;
    };
    let ed = editor();
    ed.lock().unwrap().set_library(&root.to_string_lossy());
    let levels = ok(&ed, "list_levels", json!({"filter": "ctyinda"}));
    assert_eq!(levels["levels"][0]["name"], "ctyinda");
    assert!(fail(&ed, "open_level", json!({"name": "nowhere"})).starts_with("no level 'nowhere'"));
    let status = ok(
      &ed,
      "open_level",
      json!({"name": "ctyinda", "actors": true}),
    );
    assert_eq!(status["document"]["level"], "ctyinda");
    assert!(status["document"]["actors"].as_u64().unwrap() > 0);
    assert!(status["document"]["bounds"]["min"].is_array());
    let info = ok(&ed, "get_level_info", json!({}));
    assert_eq!(info["in_document"], "opened");
    let crates = ok(&ed, "find_nodes", json!({"kind": "actor", "limit": 5}));
    assert!(crates["total"].as_u64().unwrap() > 0);
    let id = crates["nodes"][0]["id"].clone();
    let node = ok(&ed, "get_node", json!({"id": id}));
    assert!(node["game_data"]["etype"].is_string());
    let asm = ok(&ed, "list_assemblies", json!({"limit": 3}));
    assert!(asm["count"].as_u64().unwrap() > 0);
    let gaps = ok(&ed, "find_gaps", json!({"ids": [id], "margin": 6}));
    assert!(gaps["spots"].is_array() && gaps["region"].is_array());
    assert!(ed.lock().unwrap().gaps.is_some()); // shown in the Holes window
    // an object of the catalog on the ground, a level's decor (read by the store's workers)
    // above an actor of the level: something is under it
    let at = &crates["nodes"][0]["position"];
    let c = |k: usize| at[k].as_f64().unwrap();
    let p = [c(0), c(1) + 30.0, c(2)];
    let placed = ok(
      &ed,
      "place",
      json!({"kind": "object", "name": "crate", "position": p, "on_ground": true}),
    );
    assert_eq!(placed["nodes"][0]["etype"], "crate");
    assert!(placed["nodes"][0]["position"][1].as_f64().unwrap() < p[1]);
    let decor = ok(&ed, "list_decor", json!({"level": "ctyinda", "limit": 3}));
    assert!(decor["total"].as_u64().unwrap() > 0);
    let part = decor["parts"][0]["name"].clone();
    let placed = ok(
      &ed,
      "place",
      json!({"kind": "decor", "name": part, "level": "ctyinda", "position": p}),
    );
    assert_eq!(placed["nodes"][0]["kind"], "render");
  }

  #[test]
  fn shapes_placed_one_and_many() {
    let ed = editor();
    let (floor, _) = test_document(&ed);
    // dropped onto the floor (its top at 0.5 m)
    let one = ok(
      &ed,
      "place",
      json!({"kind": "shape", "name": "pillar", "position": [-5, 8, -5], "on_ground": true}),
    );
    let pillar = &one["nodes"][0];
    assert_eq!(pillar["position"], json!([-5.0, 0.5, -5.0]));
    assert!(pillar["name"].as_str().unwrap().starts_with("pillar"));
    assert_eq!(
      fail(
        &ed,
        "place",
        json!({"kind": "shape", "name": "blob", "position": [0, 0, 0]})
      ),
      SHAPES
    );
    assert_eq!(
      fail(
        &ed,
        "place",
        json!({"kind": "thing", "name": "x", "position": [0, 0, 0]})
      ),
      "kind: object, model, decor, prefab or shape"
    );
    // many at once, one undo step; an item refused says why
    let items = json!([
      {"kind": "shape", "name": "box", "position": [1, 0.5, 1], "size": [1, 1, 1]},
      {"kind": "shape", "name": "wall", "position": [6, 0.5, 1], "yaw_deg": 90},
      {"kind": "object", "name": "nothing-of-the-sort", "position": [0, 0.5, 0]},
    ]);
    let many = ok(&ed, "place_many", json!({ "items": items }));
    assert_eq!(many["placed"], 2);
    assert_eq!(many["nodes"][2]["index"], 2);
    assert!(
      many["nodes"][2]["error"]
        .as_str()
        .unwrap()
        .starts_with("no object")
    );
    assert_eq!(ok(&ed, "get_status", json!({}))["document"]["elements"], 5);
    let undone = ok(&ed, "undo", json!({}));
    assert_eq!(undone["change"], strf!(tr("undo.place_many"), 2));
    assert_eq!(ok(&ed, "get_status", json!({}))["document"]["elements"], 3);
    assert_eq!(ok(&ed, "get_node", json!({"id": floor}))["id"], floor);
  }
}
