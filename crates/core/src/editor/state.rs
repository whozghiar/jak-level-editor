//! What the interface shows besides the scene: get_state (the frame's fields, ui/lib/
//! engine.svelte.ts), a node for the inspector, the Levels panel's list, a level's information.

use serde_json::{Value, json};

use super::Editor;
use super::levels::LevelRole;
use crate::i18n::i18n_language;
use crate::io::behaviors::behavior_kind;
use crate::io::decor_behaviors::{decor_behavior_json, decor_behavior_of};
use crate::log::Log;
use crate::prefs::Prefs;
use crate::scene::{Node, NodeKind, kind_name};

/// the log's last lines get_state gives (then jle:log, one line each)
const K_STATE_LOG_LINES: usize = 200;

impl Editor {
  /// What get_state's other fields are made from that the scene, the selection and the history
  /// do not tell (`watch`: a change of it is a change of the state).
  pub(crate) fn state_key(&self) -> String {
    json!([
      self.tool,
      self.gizmo,
      self.view,
      self.story,
      self.story_level,
      self.project_path,
      self.saved_revision,
      self.clipboard.roots.len(),
      self.library_root,
      self.load_actors,
      self.hovered,
      self.levels.opening,
      self.levels.adding,
      self.levels.add_error,
      self.jobs.keys().collect::<Vec<_>>(),
      self.remote_clients,
      self.placement_key(),
      self.brush_state(),
      self.topology_key(),
      self.preview_state(),
      i18n_language(),
    ])
    .to_string()
  }

  /// The interface's state (ui/lib/engine.svelte.ts): the document, the selection, the tool, the
  /// history's labels, the view, the story, the jobs, the log's last lines; and the document's
  /// levels, the ocean, the hovered element's group.
  pub fn get_state(&mut self) -> Value {
    let primary = self.primary;
    let group = self.group_of(primary);
    let prefs = Prefs::get().clone();
    let lines = Log::get().lines();
    let log = &lines[lines.len().saturating_sub(K_STATE_LOG_LINES)..];
    let neighbors = self.document_neighbors();
    // the hovered element's group, outlined with it (update_outlines)
    let hovered_group = match (self.hovered, self.group_of(self.hovered)) {
      (0, _) | (_, 0) => json!(null),
      (h, g) => {
        let roots: Vec<u32> = self
          .group_info(g)
          .map(|g| g.roots)
          .unwrap_or_default()
          .into_iter()
          .filter(|r| *r != h && !self.selection.contains(r))
          .collect();
        json!({"id": g, "roots": roots})
      }
    };
    let story = self.story_state();
    let topology = self.topology_state();
    let s = &self.game_chunks.stats;
    json!({
      "document": {
        "open": self.document_open(),
        "title": self.document_title(),
        "project_path": self.project_path,
        "unsaved": self.unsaved(),
        "level": self.scene.settings.level,
        "replaces": self.scene.settings.replaces,
      },
      "projects_root": self.projects_root(),
      "recent_projects": prefs.recent_projects,
      "history": {
        "can_undo": self.history.can_undo(),
        "can_redo": self.history.can_redo(),
        "undo_label": self.history.undo_label(),
        "redo_label": self.history.redo_label(),
        "revision": self.history.revision(),
      },
      "selection": {
        "count": self.selection.len(),
        "primary": primary,
        // which nodes (the panels that follow the selection read again when it changes)
        "hash": self.selection.iter().fold(super::fnv(0, primary as u64), |h, &id| super::fnv(h, id as u64)),
        "group": group,
        "level": self.level_of_node(primary),
        "topology_editable": self.topology_editable(primary),
      },
      "clipboard_empty": self.clipboard.is_empty(),
      "tool": self.tool,
      "gizmo": self.gizmo,
      "brush": self.brush_state(),
      "topology": topology,
      "collision": self.collision_state(),
      "preview": self.preview_state(),
      "view": self.view,
      "story": story,
      // a level read or added, a model or a level's decor being read (the status bar)
      "loading": self.levels.opening.is_some()
        || self.levels.adding.is_some()
        || self.catalog.store.pending() > 0,
      "remote_clients": self.remote_clients,
      "placement": self.placement_state(),
      "catalog": self.catalog_state(),
      "library_empty": self.library.is_empty(),
      "library_root": self.library_root,
      "jak2_available": !self.library_jak2.is_empty(),
      "load_actors": self.load_actors,
      "language": i18n_language(),
      "log": log,
      "jobs": self.jobs.values().collect::<Vec<_>>(),
      "levels": {
        "document": *self.document_levels(),
        "neighbors": neighbors,
        "opening": self.levels.opening,
        "adding": self.levels.adding.as_ref().map(|a| &a.0),
        "adding_reference": self.levels.adding.as_ref().is_some_and(|a| a.1),
        "add_error": self.levels.add_error,
      },
      "ocean": self.ocean_state(),
      "hovered": self.hovered,
      "hovered_group": hovered_group,
      "camera": self.camera,
      "weather": {
        "mood": self.weather.mood,
        "time_passes": self.weather.time_passes,
        "time_speed": self.weather.time_speed,
        "lightning": self.weather.lightning,
        "preset": self.weather.preset_index().map(|i| crate::game::mood::weather_presets()[i].id),
      },
      "game_view": {
        "chunks": s.chunks,
        "triangles": s.triangles,
        "build_ms": s.ms_build,
      },
    })
  }

  // the story bar (ui_story_bar): the level it shows, its states (1: from the start, then each step
  // that changes it), the one shown
  fn story_state(&mut self) -> Value {
    let data = self.document_game_data();
    let Some(data) = data.filter(|d| d.has_story() && !self.scene.nodes.is_empty()) else {
      return json!({"available": false});
    };
    let states = self.story_states(&self.story_level);
    let step = if self.story_level.is_empty() {
      self.story.step
    } else {
      self.story.step_of(&self.story_level)
    };
    let state = 1 + states[1..].iter().filter(|&&s| s <= step).count();
    let node = |s: i32| {
      if s <= 1 {
        String::new()
      } else {
        data.node(s as usize - 1).name.clone()
      }
    };
    json!({
      "available": true,
      "enabled": self.story.enabled,
      "step": self.story.step,
      "level_steps": self.story.level_steps,
      "step_count": data.step_count(),
      "main": self.scene.settings.level,
      "level": self.story_level,
      "levels": self.level_story_steps.keys().collect::<Vec<_>>(),
      "states": states.iter().map(|&s| json!({"step": s, "node": node(s)})).collect::<Vec<_>>(),
      "state": state,
    })
  }

  /// What the inspector shows of a node
  pub fn node_info(&mut self, id: u32) -> Option<Value> {
    let n = self.scene.find(id)?.clone();
    let world = self.scene.world(&n);
    let (bounds, descendants) = self.subtree_bounds(id);
    let group = self.group_of(id);
    let data = self.game_data();
    let mut j = json!({
      "id": n.id,
      "parent": n.parent,
      "name": n.name,
      "kind": kind_name(n.kind),
      "position": n.local.t,
      "rotation": n.local.r.to_euler_deg(),
      "quat": n.local.r,
      "scale": n.local.s,
      "world": world,
      "visible": n.visible,
      "shown": self.scene.effectively_visible(&n),
      "locked": n.locked,
      "extras": n.extras,
      "children": self.scene.children(id).len(),
      "descendants": descendants,
      "bounds": bounds.valid().then(|| json!({"lo": bounds.lo, "hi": bounds.hi})),
      "group": self.group_info(group),
      "level": self.level_of_node(id),
      "actor": self.scene.actor_of(&n),
      "lit": n.tod.as_ref().zip(n.mesh.as_ref()).is_some_and(|(t, m)| t.fits(m)),
    });
    if let Some(mesh) = &n.mesh {
      let materials: Vec<Value> = mesh
        .prims
        .iter()
        .map(|p| {
          let m = usize::try_from(p.material)
            .ok()
            .and_then(|i| self.scene.materials.get(i));
          json!({"index": p.material, "name": m.map(|m| m.name.clone())})
        })
        .collect();
      j["mesh"] = json!({
        "uid": mesh.uid as u32,
        "name": mesh.name,
        "prims": mesh.prims.len(),
        "vertices": mesh.prims.iter().map(|p| p.pos.len()).sum::<usize>(),
        "triangles": mesh.tri_count(),
        "materials": materials,
        "shared": std::sync::Arc::strong_count(mesh) > 1,
      });
    }
    if n.kind == NodeKind::Actor {
      let etype = n.extras["etype"].as_str().unwrap_or_default().to_string();
      let models: Vec<String> = self
        .scene
        .children(id)
        .into_iter()
        .filter_map(|c| self.scene.find(c))
        .filter(|c| c.extras.get("og_actor_visual").is_some())
        .map(|c: &Node| c.name.clone())
        .collect();
      j["actor"] = json!({
        "etype": etype,
        "models": models,
        "behavior": format!("{:?}", behavior_kind(data.as_deref(), &etype)).to_lowercase(),
        "role": data.as_ref().map(|d| format!("{:?}", d.role(&etype)).to_lowercase()),
        "family": data.as_ref().map(|d| d.object_family(&etype)),
      });
    } else if n.kind == NodeKind::Render {
      j["decor_behavior"] = decor_behavior_json(&decor_behavior_of(&n.extras));
    }
    Some(j)
  }

  /// The Levels panel's list: the levels of the library (Jak II's), whether it lists each and
  /// whether the document has it ("open", "added", "companion", "" none)
  pub fn list_levels(&self, jak2: bool) -> Value {
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    let prefix = if jak2 { "jak2-" } else { "" };
    let doc = self.document_levels();
    let levels: Vec<Value> = lib
      .levels()
      .iter()
      .map(|l| {
        let name = format!("{prefix}{}", l.name);
        let state = doc
          .iter()
          .find(|d| d.name == name && d.role != LevelRole::Layer && !d.reference)
          .map_or("", |d| match d.role {
            LevelRole::Main => "open",
            LevelRole::Added => "added",
            _ => "companion",
          });
        json!({
          "name": l.name,
          "level": l.level,
          "dgo": l.dgo,
          "actors": l.actors,
          "collision_triangles": l.collision_triangles,
          "listed": l.listed,
          "background": l.background,
          "mtime": l.mtime,
          "in_document": state,
        })
      })
      .collect();
    json!({
      "jak2": jak2,
      "jak2_available": !self.library_jak2.is_empty(),
      "listed": lib.levels().iter().filter(|l| l.listed).count(),
      "levels": levels,
    })
  }

  /// What the MCP server's get_level_info says of a level (remote_level_json)
  pub fn level_info(&mut self, name: &str) -> Value {
    let mut j = json!({"name": name});
    let entry = self.library.level(name);
    j["in_library"] = json!(entry.is_some());
    if let Some(e) = entry {
      j["dgo"] = json!(e.dgo);
    }
    j["in_document"] = json!(false);
    for l in self.document_levels().iter() {
      if l.name != name || l.role == LevelRole::Layer {
        continue;
      }
      j["in_document"] = json!(match l.role {
        LevelRole::Main => "opened",
        LevelRole::Added => "added",
        _ => "companion",
      });
      if l.group != 0 {
        j["group"] = json!(l.group);
      }
      let b = self.document_level_bounds(l.group);
      if b.valid() {
        j["bounds"] = json!({"min": b.lo, "max": b.hi});
      }
      // what moves it whole (transform_nodes): its group, or the roots of the level opened
      if matches!(l.role, LevelRole::Main | LevelRole::Added) {
        j["roots"] = json!(self.document_level_roots(l.group));
      }
      break;
    }
    if let Some(info) = self.game_data().as_ref().and_then(|d| d.level(name)) {
      j["neighbors"] = json!(info.neighbors);
      j["companions"] = json!(info.companions);
      j["continue_points"] = info
        .continues
        .iter()
        .map(|c| json!({"name": c.name, "position": c.position, "shows": c.shows}))
        .collect();
    }
    j
  }
}
