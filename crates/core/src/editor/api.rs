//! The engine's API: every call of the interface (and of the plugin's protocol) by name. A call
//! changing something is followed by one `jle:changed` saying what (Editor::after_change). A new
//! call is one line of `dispatch`, its work in the file of its area.

use serde_json::{Value, json};

use super::levels::story_view_from;
use super::selection::SelectMode;
use super::{Editor, GizmoState, ViewSettings};
use crate::game::mood::{MoodSettings, game_mood, weather_presets};
use crate::i18n::{i18n_languages, tr};
use crate::math::{AABB, Mat4, Ray};
use crate::prefs::Prefs;
use crate::{log_error, log_info};

// the calls that only read: no change to look for after them (a pick at each mouse move)
const QUERIES: &[&str] = &[
  "ping",
  "get_state",
  "pick",
  "pick_all",
  "labels",
  "node",
  "list_levels",
  "get_level_info",
  "get_prefs",
  "weather_presets",
  "extract_state",
  "box_nodes",
  "list_groups",
  "level_bounds",
  "document_junction",
  "effect_emitters",
  "languages",
  "inspector",
  "actor_behavior",
  "decor_behavior",
  "shape",
  "materials",
  "export_state",
  "play_state",
  "runtime_levels",
  "tooling_state",
  "export_check",
  "navmesh_state",
  "navmesh_data",
  "gaps_report",
  "replace_info",
  "project_save_path",
  "catalog",
  "prefabs",
  "placement_preview",
  "prefab_dialog",
  "texture_list",
  "selection_meshes",
  "thumb_store",
  "level_textures",
  "preview_options",
  "library_setup",
];

fn u(a: &Value, k: &str) -> u32 {
  a.get(k).and_then(Value::as_u64).unwrap_or(0) as u32
}

fn s<'a>(a: &'a Value, k: &str) -> &'a str {
  a.get(k).and_then(Value::as_str).unwrap_or_default()
}

fn b(a: &Value, k: &str) -> Option<bool> {
  a.get(k).and_then(Value::as_bool)
}

fn f(a: &Value, k: &str) -> Option<f32> {
  a.get(k).and_then(Value::as_f64).map(|v| v as f32)
}

fn get<T: serde::de::DeserializeOwned>(a: &Value, k: &str) -> Option<T> {
  serde_json::from_value(a.get(k)?.clone()).ok()
}

fn ids(a: &Value, k: &str) -> Vec<u32> {
  get(a, k).unwrap_or_default()
}

/// `{lo, hi}`, null when invalid
fn box_json(b: AABB) -> Value {
  if b.valid() {
    json!({"lo": b.lo, "hi": b.hi})
  } else {
    Value::Null
  }
}

/// an object merged into a serializable value (set_view, set_gizmo, set_prefs, set_weather): the
/// keys it has, of the right type
fn merged<T: serde::Serialize + serde::de::DeserializeOwned>(base: &T, args: &Value) -> T {
  fn merge(base: &mut Value, over: &Value) {
    if let (Some(b), Some(o)) = (base.as_object_mut(), over.as_object()) {
      for (k, v) in o {
        if let Some(slot) = b.get_mut(k) {
          if slot.is_object() {
            merge(slot, v);
          } else if std::mem::discriminant(slot) == std::mem::discriminant(v) {
            *slot = v.clone();
          }
        }
      }
    }
  }
  let mut v = serde_json::to_value(base).unwrap_or_default();
  merge(&mut v, args);
  serde_json::from_value(v).unwrap_or_else(|_| {
    serde_json::from_value(serde_json::to_value(base).unwrap_or_default()).unwrap()
  })
}

impl Editor {
  pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
    if QUERIES.contains(&name) {
      return self.dispatch(name, args);
    }
    let before = self.watch();
    let r = self.dispatch(name, args);
    self.after_change(&before);
    r
  }

  fn dispatch(&mut self, name: &str, a: &Value) -> Result<Value, String> {
    let done = Ok(Value::Null);
    match name {
      "ping" => {
        Ok(json!({ "engine": "jak-level-editor-core", "version": env!("CARGO_PKG_VERSION") }))
      }
      "get_state" => Ok(self.get_state()),
      "languages" => Ok(json!(i18n_languages())),
      "set_language" => match self.set_language(s(a, "code"), b(a, "keep").unwrap_or(true)) {
        true => done,
        false => Err(format!("no language '{}'", s(a, "code"))),
      },
      "get_prefs" => Ok(json!(*Prefs::get())),
      "set_prefs" => {
        let p = merged(&*Prefs::get(), a);
        self.save_prefs(|old| *old = p.clone());
        Ok(json!(p))
      }
      "set_library" => {
        self.set_library(s(a, "path"));
        done
      }
      // the first-run assistant, the settings (library.rs)
      "library_setup" => Ok(self.library_setup()),
      "choose_library" => self.choose_library(s(a, "path")),
      // the window closed without saving where the engine outlives it (the launcher): nothing
      // open, as at the start
      "close_document" => {
        self.new_document();
        done
      }
      // a path dropped on the window (process_event): a folder is taken as the library ("library"),
      // a file is a project the interface opens ("file")
      "drop_path" => {
        let path = s(a, "path");
        if !std::path::Path::new(path).is_dir() {
          return Ok(json!("file"));
        }
        self.set_library(path);
        Ok(json!("library"))
      }

      // ---- documents, projects, levels
      "new_level" => {
        self.new_level();
        done
      }
      "open_project_file" => self.open_project_file(s(a, "path")).map(|_| Value::Null),
      "save" => self.save(b(a, "save_as").unwrap_or(false)),
      "save_project_as" => self
        .save_project_as(s(a, "name"))
        .map(|path| json!({"saved": true, "path": path})),
      "open_level" => {
        if let Some(actors) = b(a, "actors") {
          self.load_actors = actors;
        }
        let jak2 = b(a, "jak2").unwrap_or(false);
        self.load_level(s(a, "name"), jak2).map(|_| Value::Null)
      }
      "add_level" => {
        let (reference, jak2) = (b(a, "reference"), b(a, "jak2"));
        self
          .add_level_to_document(
            s(a, "name"),
            reference.unwrap_or(false),
            jak2.unwrap_or(false),
          )
          .map(|_| Value::Null)
      }
      "remove_level" => self
        .remove_document_level(u(a, "group"))
        .map(|_| Value::Null),
      "set_load_actors" => {
        self.load_actors = b(a, "actors").unwrap_or(false);
        done
      }
      "list_levels" => Ok(self.list_levels(b(a, "jak2").unwrap_or(false))),
      "get_level_info" => Ok(self.level_info(s(a, "name"))),
      "level_bounds" => Ok(box_json(self.document_level_bounds(u(a, "group")))),
      "document_junction" => Ok(box_json(self.document_junction(u(a, "a"), u(a, "b")))),
      "align_to_replaced" => self.align_to_replaced().map(|_| Value::Null),
      "turn_own_level" => {
        self.turn_own_level(f(a, "degrees").unwrap_or(90.0));
        done
      }
      "set_replaces" => {
        self.set_replaces(s(a, "level"));
        done
      }
      // the Replace section and the Save window (panels.rs)
      "replace_info" => Ok(self.replace_info()),
      "project_save_path" => Ok(self.project_save_path(s(a, "name"))),
      "restore_level_effects" => {
        let (added, left_out) = self.restore_level_effects();
        log_info!(tr("log.effects_restored"), added, left_out);
        Ok(json!({"added": added, "left_out": left_out}))
      }
      "restore_level_lighting" => {
        let lit = self.restore_level_lighting();
        if lit > 0 {
          log_info!(tr("log.lighting_restored"), lit);
        }
        Ok(json!({"restored": lit}))
      }
      "set_story" => {
        if let Some(level) = a.get("level").and_then(Value::as_str) {
          self.set_story_level(level);
        }
        let mut v = story_view_from(self.story_view(), a);
        if let Some(state) = a.get("state").and_then(Value::as_i64) {
          v = self.story_view_at_state(state as i32);
          v.enabled = b(a, "enabled").unwrap_or(v.enabled);
        }
        self.set_story_view(v);
        done
      }

      // ---- history, clipboard
      "undo" => {
        self.undo_redo(true);
        done
      }
      "redo" => {
        self.undo_redo(false);
        done
      }
      "copy_selection" => {
        self.copy_selection();
        done
      }
      "paste" => {
        self.paste();
        done
      }
      "duplicate_selection" => {
        self.duplicate_selection();
        done
      }
      "delete_selection" => {
        self.delete_selection();
        done
      }

      // ---- the selection
      "select_only" => {
        self.select_only(u(a, "id"));
        done
      }
      "select" => {
        self.select_ids(&ids(a, "ids"), SelectMode::from_name(s(a, "mode")));
        done
      }
      "toggle_select" => {
        self.toggle_select(u(a, "id"));
        done
      }
      "select_level" => {
        self.select_level(u(a, "group"));
        done
      }
      "select_assembly" => {
        self.select_assembly(u(a, "id"));
        done
      }
      "select_group" => {
        self.select_group(u(a, "id"));
        done
      }
      "select_whole" => Ok(json!(self.select_whole(u(a, "id")))),
      "hover" => {
        self.hovered = u(a, "id");
        done
      }
      "box_nodes" => Ok(json!({ "ids": self.box_nodes_call(a)? })),
      "select_box" => {
        let found = self.box_nodes_call(a)?;
        self.select_ids(&found, SelectMode::from_name(s(a, "mode")));
        Ok(json!({ "ids": found }))
      }
      "pick" => {
        let ray = Ray::new(
          get(a, "origin").unwrap_or_default(),
          get(a, "dir").unwrap_or_default(),
        );
        Ok(self.pick_ray(&ray, a))
      }
      "pick_all" => Ok(self.pick_all_call(a)),
      "labels" => Ok(self.labels_call()),
      "node" => Ok(self.node_info(u(a, "id")).unwrap_or(Value::Null)),
      "set_node" => self.set_node(a).map(|_| Value::Null),
      "edit_begin" => {
        self.edit_begin(u(a, "id"));
        done
      }
      "edit_end" => {
        self.edit_end(
          a.get("label")
            .and_then(Value::as_str)
            .unwrap_or("undo.param"),
        );
        done
      }
      "transform_nodes" => {
        let worlds: Vec<Mat4> = get(a, "worlds").unwrap_or_default();
        let label = a
          .get("label")
          .and_then(Value::as_str)
          .unwrap_or("undo.move");
        self.transform_nodes(&ids(a, "ids"), &worlds, label);
        done
      }

      // ---- the inspector (inspector.rs, behaviors.rs, surfaces.rs)
      "inspector" => Ok(self.inspector_info(u(a, "id"))),
      "actor_behavior" => Ok(self.actor_behavior(u(a, "id")).unwrap_or(Value::Null)),
      "set_actor_behavior" => self.set_actor_behavior(a).map(|_| Value::Null),
      "set_actor_param" => self.set_actor_param(a).map(|_| Value::Null),
      "decor_behavior" => Ok(self.decor_behavior(u(a, "id")).unwrap_or(Value::Null)),
      "set_decor_behavior" => self.set_decor_behavior(a).map(|_| Value::Null),
      "shape" => Ok(self.shape_info(u(a, "id")).unwrap_or(Value::Null)),
      "set_shape" => self.set_shape(a).map(|_| Value::Null),
      "materials" => Ok(self.materials_info(a).unwrap_or(Value::Null)),
      "set_material" => self.set_material(a),
      "material_image" => Ok(self.material_image(a)),
      "import_match_lighting" => {
        self.import_match_lighting(u(a, "id"));
        done
      }

      // ---- groups
      "list_groups" => Ok(json!(*self.document_groups())),
      "group_selection" => Ok(json!({"group": self.group_selection(s(a, "name"))})),
      "ungroup" => {
        self.ungroup(u(a, "id"));
        done
      }
      "rename_group" => {
        self.rename_group(u(a, "id"), s(a, "name"));
        done
      }

      // ---- the view, the tools
      "set_view" => {
        self.view = merged::<ViewSettings>(&self.view, a);
        done
      }
      "set_gizmo" => {
        self.gizmo = merged::<GizmoState>(&self.gizmo, a);
        done
      }
      "set_camera_state" => {
        self.camera = merged(&self.camera, a);
        done
      }
      "frontend_reply" => {
        self.replies.insert(u(a, "id") as u64, a.clone());
        done
      }
      // the viewport listens to the engine's requests (the MCP server's captures can be made)
      "frontend_ready" => {
        self.interface_ready = true;
        done
      }
      "escape" => {
        self.escape();
        done
      }
      "game_mood" => {
        if let Some(dt) = f(a, "dt") {
          self.tick(dt);
        }
        let mood = merged::<MoodSettings>(&self.weather.mood, a);
        Ok(json!(game_mood(&mood)))
      }
      "weather_presets" => Ok(json!(weather_presets())),
      "set_weather" => {
        if let Some(id) = a.get("preset").and_then(Value::as_str)
          && !self.weather.set_weather(id)
        {
          return Err(format!("no weather '{id}'"));
        }
        // what the call gives, the lightning's clock kept (not serialized)
        let w = merged(&self.weather, a);
        self.weather.mood = w.mood;
        self.weather.time = w.time;
        self.weather.time_passes = w.time_passes;
        self.weather.time_speed = w.time_speed;
        self.weather.lightning = w.lightning;
        done
      }
      "effect_emitters" => Ok(self.effect_emitters_call()),
      // nodes left out of the game view's chunks for a while: {hide: ids} (a drag's pieces),
      // {only: ids} (a capture of these alone), {} everything again
      "game_view_filter" => {
        let filter = match (a.get("only"), a.get("hide")) {
          (Some(_), _) => Some((true, ids(a, "only").into_iter().collect())),
          (_, Some(_)) => Some((false, ids(a, "hide").into_iter().collect())),
          _ => None,
        };
        self.game_chunks.set_filter(filter);
        done
      }

      // ---- File > Extract game assets (extract.rs)
      "extract_defaults" => Ok(self.extract_defaults()),
      "extract_fetch" => self.extract_fetch().map(|_| self.extract_defaults()),
      "extract_detect" => {
        self.extraction.detect(s(a, "source"));
        Ok(self.extraction.snapshot())
      }
      "extract_start" => {
        let (source, output) = (s(a, "source"), s(a, "output"));
        self.save_prefs(|p| p.extract_source = source.to_string());
        if self.extraction.start(source, output) {
          log_info!(tr("log.extracting"), output);
        } else {
          // ogle-extract cannot start: why, as the window says it
          let e = &self.extraction.snapshot()["error"];
          let t = super::extract::Text {
            key: s(e, "key").into(),
            args: get(e, "args").unwrap_or_default(),
            detail: s(e, "detail").into(),
            text: s(e, "text").into(),
          };
          log_error!("%s", super::text_string(&t));
        }
        Ok(self.extraction.snapshot())
      }
      "extract_stop" => {
        self.extraction.cancel();
        Ok(self.extraction.snapshot())
      }
      "extract_state" => Ok(self.extraction.snapshot()),

      // ---- the terrain brush, the topology tool, the animation preview (terrain_tool.rs,
      // topology_tool.rs, preview.rs)
      "start_terrain_brush"
      | "stop_terrain_brush"
      | "set_brush"
      | "brush_hover"
      | "brush_press"
      | "brush_drag"
      | "brush_release"
      | "brush_cancel"
      | "brush_sculpt"
      | "terrain_generate"
      | "terrain_apply_style"
      | "terrain_start_new"
      | "level_textures" => self.terrain_call(name, a),
      "start_topology" | "stop_topology" | "topology_delete" | "topology_view"
      | "topology_pick" | "topology_box" | "topology_select" | "topology_drag"
      | "topology_drag_end" | "topology_extrude" | "topology_subdivide" | "topology_flip" => {
        self.topology_call(name, a)
      }
      "preview_options" | "start_preview" | "stop_preview" | "preview_frame" => {
        self.preview_call(name, a)
      }
      // ---- the export for the game, nav meshes, holes (export.rs)
      "export_defaults" => Ok(self.export_defaults()),
      "export_check" => {
        let o = get(a, "options").ok_or("options: the export's")?;
        Ok(json!(self.export_check(&o)))
      }
      "export_start_point" => Ok(json!(self.export_start_point())),
      "check_continues" => Ok(self.check_continues(s(a, "level"))),
      "export_for_game" => {
        let o = get(a, "options").ok_or("options: the export's")?;
        self.export_for_game(o, b(a, "wait").unwrap_or(false))?;
        Ok(self.export_state())
      }
      "export_state" => Ok(self.export_state()),
      // ---- the game runtime of the host: Play (runtime.rs)
      "play_level" => self.play_level(get(a, "options"), b(a, "wait").unwrap_or(false)),
      "play_state" => Ok(self.play_state()),
      "runtime_levels" => self.runtime_levels(),
      "runtime_remove" => self.runtime_remove(s(a, "name"), b(a, "wait").unwrap_or(false)),
      "runtime_launch" => self.runtime_launch(s(a, "name"), b(a, "wait").unwrap_or(false)),
      "runtime_settings" => Ok(self.runtime_settings()),
      // ---- OpenGOAL's tooling, when the runtime installs it itself (runtime.rs, crate::tooling)
      "tooling_state" => Ok(self.tooling_state(b(a, "check").unwrap_or(false))),
      "tooling_install" => self.tooling_install(s(a, "version"), b(a, "wait").unwrap_or(false)),
      "tooling_use" => self.tooling_use(s(a, "version")),
      "tooling_remove" => self.tooling_remove(s(a, "version")),
      "generate_navmesh" => {
        self.generate_navmesh_call(a)?;
        Ok(self.navmesh_state())
      }
      "set_navmesh" => {
        self.set_navmesh(a);
        Ok(self.navmesh_state())
      }
      "navmesh_state" => Ok(self.navmesh_state()),
      "navmesh_data" => Ok(self.navmesh_data(get(a, "ids"))),
      "export_navmeshes" => {
        let first = a.get("first_aid").and_then(Value::as_u64).map(|v| v as u32);
        self.export_navmeshes_call(s(a, "path"), first)
      }
      "find_gaps" => Ok(self.find_gaps_call(a)),
      "gaps_report" => Ok(self.gaps_report()),
      // ---- the Place tool, prefabs, the catalog, the textures of the games (placement.rs,
      // catalog.rs, thumbs.rs)
      "start_shape_placement"
      | "start_model_placement"
      | "start_object_placement"
      | "start_part_placement"
      | "start_prefab_placement"
      | "start_style_placement"
      | "import_model"
      | "stop_placement"
      | "turn_placement"
      | "set_placement"
      | "placement_preview"
      | "place_at"
      | "place_here"
      | "relight_selection"
      | "create_prefab"
      | "prefab_dialog"
      | "delete_prefab"
      | "level_style" => self
        .placement_call(name, a)
        .unwrap_or_else(|| Err(format!("unknown call: {name}"))),
      "catalog" | "prefabs" | "want_game_prefabs" | "game_prefabs" | "texture_list"
      | "palette_texture" | "selection_meshes" => self
        .catalog_call(name, a)
        .unwrap_or_else(|| Err(format!("unknown call: {name}"))),
      "thumb_store" => super::thumbs::store_thumb(s(a, "name"), s(a, "png")).map(|_| Value::Null),
      _ => Err(format!("unknown call: {name}")),
    }
  }

  // undo / redo: the levels of the document come and go with them (the C++ after-apply hooks of
  // the levels added and removed: story_document_changed)
  pub(crate) fn undo_redo(&mut self, undo: bool) {
    let names = |ed: &Editor| -> Vec<String> {
      ed.document_levels()
        .iter()
        .map(|l| l.name.clone())
        .collect()
    };
    let before = names(self);
    if undo {
      self.history.undo(&mut self.scene);
    } else {
      self.history.redo(&mut self.scene);
    }
    self.scene.begin_frame();
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
    if names(self) != before {
      self.story_document_changed();
    }
  }

  // box_nodes {view_proj: 16 floats (column-major), width, height, a: [x, y], b: [x, y]}
  fn box_nodes_call(&self, a: &Value) -> Result<Vec<u32>, String> {
    let vp: Mat4 = get(a, "view_proj").ok_or("view_proj: the view's 16 floats")?;
    let size = [f(a, "width").unwrap_or(1.0), f(a, "height").unwrap_or(1.0)];
    let corner = |k: &str| get::<[f32; 2]>(a, k).unwrap_or_default();
    Ok(self.box_nodes(&vp, size, corner("a"), corner("b")))
  }

  /// the level of the game the document replaces (Level > Replace), undoable
  pub fn set_replaces(&mut self, level: &str) {
    let before = self.scene.settings.replaces.clone();
    if before == level {
      return;
    }
    self.scene.settings.replaces = level.to_string();
    // the export's options made again for it (C++ m_export_level.clear())
    self.export.level.clear();
    let after = level.to_string();
    self.history.push(crate::history::HistoryEntry::new(
      tr("undo.replaces"),
      move |s| s.settings.replaces = before.clone(),
      move |s| s.settings.replaces = after.clone(),
    ));
  }
}
