//! Documents and projects (src/app/actions.cpp, app.cpp): a new document, a new level, a project
//! opened, saved only when the user asks, in its folder of <library>/projects/<name>/ (its .glb,
//! and its nav meshes for the game, <name>-nav.json, when it has some).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::levels::{open_project, save_project};
use super::{CameraState, Editor};
use crate::i18n::tr;
use crate::io::asset_library::default_projects_dir;
use crate::io::nav_export::export_navmeshes;
use crate::scene::{LevelSettings, NodeKind, Scene};
use crate::{log_error, log_info, log_warn};

impl Editor {
  /// An empty document (app.cpp new_document)
  pub fn new_document(&mut self) {
    self.scene.clear();
    self.scene.settings = LevelSettings::default();
    self.history.clear();
    self.selection.clear();
    self.primary = 0;
    self.hovered = 0;
    self.edit_before = None;
    self.stop_placement();
    self.game_chunks.reset();
    self.ocean = Default::default();
    self.story_steps.clear();
    self.level_story_steps.clear();
    self.story_level.clear();
    self.project_path.clear();
    self.saved_revision = self.history.revision();
    self.camera = CameraState::default();
    self.document_epoch += 1;
    self.scene_changed();
  }

  /// an empty document of the library's game
  pub fn new_level(&mut self) {
    self.new_document();
    // a document without a level of the game: no story, no ocean
    self.scene.settings.level = "custom-level".into();
    // nothing of the game's to light it: the editor's own view (the game rendering would show it
    // all blue, under the sky)
    self.view.game_view = false;
    self.story_document_changed();
    log_info!("%s", tr("log.new_level"));
  }

  pub fn open_project_file(&mut self, path: &str) -> Result<(), String> {
    let mut loaded = Scene::new();
    if let Err(e) = open_project(&mut loaded, path) {
      log_error!("%s: %s", path, &e);
      return Err(format!("{path}: {e}"));
    }
    self.new_document();
    self.scene = loaded;
    self.history.clear();
    self.saved_revision = self.history.revision();
    self.project_path = path.to_string();
    self.save_prefs(|p| p.add_recent_project(path));
    self.story_document_changed();
    self.frame_all();
    log_info!(tr("log.project_opened"), path);
    let lit = self.restore_level_lighting();
    if lit > 0 {
      log_info!(tr("log.lighting_restored"), lit);
    }
    Ok(())
  }

  /// <library>/projects
  pub fn projects_root(&self) -> String {
    if self.library_root.is_empty() {
      default_projects_dir()
    } else {
      Path::new(&self.library_root)
        .join("projects")
        .to_string_lossy()
        .into_owned()
    }
  }

  /// its folder's name, or the level's
  pub fn project_name(&self) -> String {
    if !self.project_path.is_empty() {
      return Path::new(&self.project_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    }
    let s = &self.scene.settings;
    let base = if s.replaces.is_empty() {
      &s.level
    } else {
      &s.replaces
    };
    if base.is_empty() || base == "custom-level" {
      "my-level".into()
    } else {
      format!("my-{base}")
    }
  }

  /// Saves the project, only when the user asks: in its folder (the first time and with save_as,
  /// the user names it: `{ask_name}` is answered, then save_project_as), else `{saved: true}`.
  pub fn save(&mut self, save_as: bool) -> Result<Value, String> {
    if save_as || self.project_path.is_empty() {
      return Ok(json!({ "ask_name": self.project_name() }));
    }
    let path = self.project_path.clone();
    self.save_project_files(&path).map_err(|e| {
      log_error!(tr("log.save_failed"), &e);
      e
    })?;
    Ok(json!({ "saved": true }))
  }

  /// In <library>/projects/<name>/<name>.glb
  pub fn save_project_as(&mut self, name: &str) -> Result<String, String> {
    let folder = project_folder_name(name);
    if folder.is_empty() {
      return Err(tr("save.bad_name").to_string());
    }
    let dir = PathBuf::from(self.projects_root()).join(&folder);
    let _ = std::fs::create_dir_all(&dir);
    if !dir.is_dir() {
      return Err(format!("cannot create {}", dir.display()));
    }
    let path = dir
      .join(format!("{folder}.glb"))
      .to_string_lossy()
      .into_owned();
    self.save_project_files(&path).map_err(|e| {
      log_error!(tr("log.save_failed"), &e);
      e
    })?;
    Ok(path)
  }

  /// The project's .glb and its nav meshes for the game next to it (none: the file of an earlier
  /// save goes).
  pub fn save_project_files(&mut self, glb_path: &str) -> Result<(), String> {
    self.stop_preview(); // the model as it is, not as the preview draws it
    save_project(&self.scene, glb_path)?;
    let p = Path::new(glb_path);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let nav = p.with_file_name(format!("{stem}-nav.json"));
    if self.scene.nodes.iter().any(|n| n.kind == NodeKind::NavMesh) {
      if let Err(e) = export_navmeshes(&self.scene, &nav, self.nav.first_aid) {
        log_warn!(tr("log.navmesh_export_failed"), e);
      }
    } else if nav.exists() {
      let _ = std::fs::remove_file(&nav);
    }
    self.project_path = glb_path.to_string();
    self.saved_revision = self.history.revision();
    self.save_prefs(|p| p.add_recent_project(glb_path));
    log_info!(tr("log.project_saved"), glb_path);
    Ok(())
  }

  /// the window's title: the project's file name, else the level's name, else "" (untitled)
  pub fn document_title(&self) -> String {
    if !self.project_path.is_empty() {
      return Path::new(&self.project_path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    }
    self.scene.settings.level.clone()
  }

  pub fn unsaved(&self) -> bool {
    self.history.revision() != self.saved_revision && self.document_open()
  }
}

/// a name as a folder can have it on every system: no separators nor reserved characters, no space
pub fn project_folder_name(name: &str) -> String {
  let out: String = name
    .chars()
    .filter(|&c| c as u32 >= 32 && !"<>:\"/\\|?*".contains(c))
    .map(|c| if c == ' ' { '-' } else { c })
    .collect();
  out.trim_matches(['.', '-']).to_string()
}
