//! Where the library lives: the host's default (crate::host::Host), the first-run assistant's
//! choice, the settings' change of folder; File > Extract game assets: the game installed whose
//! levels it reads (the host's, else the settings').

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::extract::{Detected, Text};
use super::{Editor, text_string, tr_err};
use crate::host::Host;
use crate::i18n::tr;
use crate::io::asset_library::{default_library_root, from_path, game_folder};
use crate::io::extraction::{Options, game_levels};
use crate::io::game_install::{find_extraction, read_jsonc};
use crate::prefs::{Prefs, READ_ONLY};
use crate::{log_error, log_info};

impl Editor {
  /// What the application gives the editor (the plugin's Builder::host and set_host).
  pub fn set_host(&mut self, host: Host) {
    self.host = host;
  }

  /// The library's folder before the user chose one: the host's (the standalone app's
  /// Documents/Jak Level Editor, the launcher's <install>/jak-level-editor), else library/ of the
  /// editor's own tree in a development build; "" when neither.
  pub fn default_library(&self) -> String {
    match &self.host.default_library {
      Some(p) => from_path(p),
      None => default_library_root(),
    }
  }

  /// What the first-run assistant and the settings show (library_setup): `chosen` (no assistant
  /// then; the command line modes never ask), `default` and `default_game` (it holds an extracted
  /// game), `root` and `root_game` (the library now), `host`.
  pub fn library_setup(&self) -> Value {
    let default = self.default_library();
    let has_game = |root: &str| !game_folder(root).is_empty();
    json!({
      "chosen": Prefs::get().library_chosen || READ_ONLY.load(std::sync::atomic::Ordering::Relaxed),
      "default_game": has_game(&default),
      "default": default,
      "root": self.library_root,
      "root_game": has_game(&self.library_root),
      "host": self.host.name,
    })
  }

  /// The library the user chose (the first-run assistant's Confirm, the settings' change of
  /// folder): the folder created when missing, then used, kept as chosen. Nothing in the folder
  /// used before is moved or deleted.
  pub fn choose_library(&mut self, path: &str) -> Result<Value, String> {
    if path.trim().is_empty() {
      return Err(tr("settings.library.none").into());
    }
    std::fs::create_dir_all(path)
      .map_err(|e| tr_err("settings.library.cannot_create", &e.to_string()))?;
    self.set_library(path);
    self.save_prefs(|p| p.library_chosen = true);
    log_info!(tr("log.library_chosen"), self.library_root.clone());
    Ok(self.library_setup())
  }

  /// File > Extract game assets' first values (extract_defaults): `output`, the library (its
  /// default before one is chosen); `library` {root, levels} when one is loaded.
  pub(crate) fn extract_defaults(&self) -> Value {
    let root = if self.library_root.is_empty() {
      self.default_library()
    } else {
      self.library_root.clone()
    };
    let levels = self.library.levels().len();
    json!({
      "output": root,
      "library": (levels > 0).then(|| json!({ "root": self.library_root, "levels": levels })),
    })
  }

  /// The game the Extract window extracts (extract_detect): its installation, or why not
  pub(crate) fn extract_detect(&mut self, game: &str) -> Value {
    let d = self.extraction_source(game);
    self.extraction.set_detected(d);
    self.extraction.snapshot()
  }

  /// The installation of `game` to extract its levels from: its disc's serial, its levels
  fn extraction_source(&self, game: &str) -> Detected {
    let (folder, from_host) = self.game_folder(game);
    let mut d = Detected {
      game: game.into(),
      folder: folder.clone(),
      from_host,
      ..Default::default()
    };
    if folder.is_empty() {
      d.error = Some(Text::key("game_install.error.none"));
      return d;
    }
    match find_extraction(game, Path::new(&folder)) {
      Ok(mut install) => {
        install.version = self.gk_version(&install.gk);
        let disc = install.data().join("iso_data").join(game);
        d.serial = read_jsonc(&disc.join("buildinfo.json"))
          .ok()
          .and_then(|b| b[0]["serial"].as_str().map(str::to_string))
          .unwrap_or_default();
        match game_levels(&install, &[]) {
          Ok(levels) => d.levels = levels.len(),
          Err(e) => {
            d.error = Some(Text {
              detail: e,
              ..Text::key("extract.error.extraction")
            })
          }
        }
        d.install = Some(install);
      }
      Err(key) => d.error = Some(Text::key(key)),
    }
    d
  }

  /// Extracts the levels of `game` to `output`/<game> (extract_start): its installation's
  /// extractor works in the editor's folder (<preferences>/extract/<game>)
  pub(crate) fn extract_start(&mut self, game: &str, output: &str) -> Value {
    let d = self.extraction_source(game);
    let (install, error) = (d.install.clone(), d.error.clone());
    self.extraction.set_detected(d);
    match (install, error) {
      (Some(install), None) => {
        let work = PathBuf::from(Prefs::dir()).join("extract").join(game);
        if self
          .extraction
          .start(install, output, work, Options::default())
        {
          log_info!(tr("log.extracting"), output);
        }
      }
      (_, error) => log_error!(
        "%s",
        text_string(&error.unwrap_or_else(|| Text::key("game_install.error.none")))
      ),
    }
    self.extraction.snapshot()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::test_library::temp_dir;

  fn editor(host: Host) -> Editor {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.set_host(host);
    ed
  }

  #[test]
  fn library_default_per_host_and_chosen() {
    let tmp = temp_dir("library-setup");
    let docs = tmp.join("Documents").join("Jak Level Editor");
    let mut ed = editor(Host {
      name: "standalone".into(),
      default_library: Some(docs.clone()),
      ..Default::default()
    });
    let setup = ed.call("library_setup", &Value::Null).unwrap();
    assert_eq!(setup["default"], from_path(&docs));
    assert_eq!(setup["default_game"], false);
    assert_eq!(setup["host"], "standalone");
    assert!(!docs.exists(), "nothing created before the user confirms");
    // Confirm: created, used, chosen
    let was = Prefs::get().clone();
    let chosen = ed
      .call("choose_library", &json!({"path": docs.to_string_lossy()}))
      .unwrap();
    assert!(docs.is_dir());
    assert_eq!(chosen["root"], from_path(&docs));
    assert_eq!(
      (&chosen["chosen"], &chosen["root_game"]),
      (&json!(true), &json!(false))
    );
    assert!(ed.call("choose_library", &json!({"path": " "})).is_err());
    // a default that holds an extracted game says so
    std::fs::create_dir_all(docs.join("jak3")).unwrap();
    std::fs::write(docs.join("jak3/library.json"), "{}").unwrap();
    assert_eq!(ed.library_setup()["default_game"], true);
    assert_eq!(ed.library_setup()["root_game"], true);
    // the projects of a library chosen
    assert_eq!(PathBuf::from(ed.projects_root()), docs.join("projects"));
    *Prefs::get() = was;
  }

  #[test]
  fn extraction_from_the_game_installed() {
    let tmp = temp_dir("extract-host");
    let was = Prefs::get().clone();
    Prefs::get().game_installs.clear();
    // none: where to choose it
    let mut ed = editor(Host::default());
    let snap = ed.call("extract_detect", &json!({"game": "jak3"})).unwrap();
    assert_eq!(snap["detected"]["error"]["key"], "game_install.error.none");
    // the host's: what it lacks
    let mut ed = editor(Host {
      name: "launcher".into(),
      default_library: Some(tmp.join("jak-level-editor")),
      game_installs: [("jak3".into(), tmp.clone())].into(),
    });
    let snap = ed.call("extract_detect", &json!({"game": "jak3"})).unwrap();
    assert_eq!(snap["detected"]["from_host"], true);
    assert_eq!(
      snap["detected"]["error"]["key"],
      "game_install.error.no_game"
    );
    let d = ed.call("extract_defaults", &Value::Null).unwrap();
    assert_eq!(d["output"], from_path(&tmp.join("jak-level-editor")));
    // started without its game: nothing runs
    let snap = ed
      .call(
        "extract_start",
        &json!({"game": "jak3", "output": tmp.to_string_lossy()}),
      )
      .unwrap();
    assert_eq!(snap["running"], false);
    *Prefs::get() = was;
  }
}
