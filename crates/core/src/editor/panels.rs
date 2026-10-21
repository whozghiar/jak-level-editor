//! What the Levels panel's Replace section and the Save window ask the engine (ui_replace_section
//! in panels.cpp, the save popup of ui_windows).

use std::path::Path;

use serde_json::{Value, json};

use super::Editor;
use super::project::project_folder_name;
use crate::io::game_export::check_continues;

impl Editor {
  /// The Replace section: null without the game's data; else the levels its list offers (the one
  /// the document comes from, `source`, when the game can have it replaced; then the game's
  /// levels the Levels panel lists) and where the game starts the player in the level replaced:
  /// the ground under each continue point (null: nothing within a few meters).
  pub fn replace_info(&mut self) -> Value {
    let Some(data) = self.document_game_data() else {
      return Value::Null;
    };
    let source = &self.scene.settings.level;
    let own = data
      .level(source)
      .is_some_and(|l| !l.dgo.is_empty() && !l.layer);
    let levels: Vec<&str> = self
      .library
      .levels()
      .iter()
      .filter(|l| l.listed && &l.name != source)
      .filter(|l| data.level(&l.name).is_some_and(|i| !i.dgo.is_empty()))
      .map(|l| l.name.as_str())
      .collect();
    let current = &self.scene.settings.replaces;
    let continues = if current.is_empty() {
      Vec::new()
    } else {
      check_continues(&self.scene, Some(&data), current)
    };
    json!({
      "source": own.then_some(source),
      "levels": levels,
      "continues": continues,
    })
  }

  /// The Save window: the project's folder for that name (in <library>/projects), its .glb, and
  /// whether another project is there already (it would be replaced).
  pub fn project_save_path(&self, name: &str) -> Value {
    let folder = project_folder_name(name);
    if folder.is_empty() {
      return json!({"folder": "", "path": "", "exists": false});
    }
    let glb = Path::new(&self.projects_root())
      .join(&folder)
      .join(format!("{folder}.glb"));
    let path = glb.to_string_lossy().into_owned();
    json!({
      "folder": folder,
      "exists": glb.exists() && path != self.project_path,
      "path": path,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn save_path_and_replace_info() {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    let p = ed.project_save_path(" my level: one ");
    assert_eq!(p["folder"], "my-level-one");
    assert!(p["path"].as_str().unwrap().ends_with("my-level-one.glb"));
    assert_eq!(ed.project_save_path("..")["folder"], "");
    // no document: no section
    assert!(ed.replace_info().is_null());
  }
}
