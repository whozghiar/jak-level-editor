//! The editor's preferences, kept between sessions in one folder whoever starts the editor (on its
//! own or the OpenGOAL Launcher): %APPDATA%/OpenGOAL/level-editor on Windows,
//! ~/Library/Application Support/OpenGOAL/level-editor on macOS, ~/.local/share/OpenGOAL/
//! level-editor on Linux; $JLE_PREFS_DIR instead when set (tests and captures, with fresh ones):
//! where the extracted game is, the language, recent projects, the games installed where levels
//! are tried. Thumbnails and the catalog's index are cached next to them.
//!
//! Earlier editors kept them elsewhere (`old_dirs`): the first time, the newest of those folders
//! becomes the new one and the others are deleted.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};

use serde::{Deserialize, Serialize};

/// the command line modes (captures, tests) read the preferences, never write them
pub static READ_ONLY: AtomicBool = AtomicBool::new(false);

const K_FILE: &str = "prefs.json";

static DIR: OnceLock<String> = OnceLock::new();

fn slashed(dir: &str) -> String {
  format!("{}/", dir.replace('\\', "/").trim_end_matches('/'))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
  /// the folder the extraction writes to: the game in its subfolder jak3/
  pub library_root: String,
  /// "" = the system's
  pub language: String,
  pub recent_projects: Vec<String>,
  /// the guided tour was shown (at the first start)
  pub guide_seen: bool,
  /// the user chose where the library goes (the first-run assistant); preferences of before it
  /// whose library holds an extracted game count as chosen
  pub library_chosen: bool,
  /// an assistant (the MCP server) may control the editor: its remote control listens on this
  /// computer (editor/remote.rs); off unless the user turns it on, or the MCP server starts the
  /// editor itself (--remote-port)
  pub remote_control: bool,
  /// the games installed whose levels are extracted and tried, by game ("jak3": its folder),
  /// chosen in the settings (io/game_install.rs); a host's (the OpenGOAL Launcher's) comes first
  pub game_installs: std::collections::BTreeMap<String, String>,
}

fn env(k: &str) -> Option<String> {
  std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// The folder where applications keep their settings, as SDL_GetPrefPath has it ('/' separated),
/// if the system has one.
fn settings_base() -> Option<String> {
  let base = if cfg!(windows) {
    env("APPDATA")
  } else if cfg!(target_os = "macos") {
    env("HOME").map(|h| format!("{h}/Library/Application Support"))
  } else {
    env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| format!("{h}/.local/share")))
  };
  base.map(|b| b.replace('\\', "/").trim_end_matches('/').to_owned())
}

/// The folders where earlier editors kept the preferences: the standalone app's, the OpenGOAL
/// Launcher's (in its config folder, Tauri's app_config_dir: ~/.config on Linux), the C++ editor's.
fn old_dirs() -> Vec<PathBuf> {
  let launcher = if cfg!(windows) || cfg!(target_os = "macos") {
    settings_base()
  } else {
    env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| format!("{h}/.config")))
  };
  let mut dirs: Vec<PathBuf> = settings_base()
    .into_iter()
    .flat_map(|b| {
      ["jak-level-editor", "open-goal-level-editor"]
        .map(|app| Path::new(&b).join("OpenGOAL").join(app))
    })
    .collect();
  dirs.extend(launcher.map(|b| Path::new(&b).join("OpenGOAL-Launcher/jak-level-editor")));
  dirs
}

/// The newest preferences of the `old` folders become `dir` (the folder itself, its caches with
/// it, else its preferences alone), when `dir` has none yet; the old folders are then deleted, as
/// they are at every start (an earlier editor, or an earlier launcher's, makes its own again).
fn migrate(dir: &Path, old: &[PathBuf]) {
  let newest = old
    .iter()
    .filter_map(|d| Some((std::fs::metadata(d.join(K_FILE)).ok()?.modified().ok()?, d)))
    .max_by_key(|(time, _)| *time)
    .map(|(_, d)| d);
  if let Some(from) = newest.filter(|_| !dir.join(K_FILE).is_file()) {
    if let Some(parent) = dir.parent() {
      let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(from, dir).is_err() {
      let _ = std::fs::create_dir_all(dir);
      let _ = std::fs::copy(from.join(K_FILE), dir.join(K_FILE));
    }
    if !dir.join(K_FILE).is_file() {
      return;
    }
  }
  for d in old {
    let _ = std::fs::remove_dir_all(d);
  }
}

impl Prefs {
  /// folder of the preferences and of the thumbnail cache (created on demand), '/' separated:
  /// $JLE_PREFS_DIR when set (tests of the application with fresh preferences), else
  /// OpenGOAL/level-editor in the system's settings folder ("./" without one)
  pub fn dir() -> String {
    DIR
      .get_or_init(|| match env("JLE_PREFS_DIR") {
        Some(d) => slashed(&d),
        None => settings_base().map_or("./".into(), |b| format!("{b}/OpenGOAL/level-editor/")),
      })
      .clone()
  }

  pub fn get() -> MutexGuard<'static, Prefs> {
    static PREFS: LazyLock<Mutex<Prefs>> = LazyLock::new(Mutex::default);
    PREFS.lock().unwrap_or_else(|e| e.into_inner())
  }

  pub fn load(&mut self) {
    let dir = Self::dir();
    if env("JLE_PREFS_DIR").is_none() && dir != "./" && !READ_ONLY.load(Ordering::Relaxed) {
      migrate(Path::new(&dir), &old_dirs());
    }
    self.load_from(Path::new(&dir));
  }

  pub fn load_from(&mut self, dir: &Path) {
    let Ok(text) = std::fs::read(dir.join(K_FILE)) else {
      return;
    };
    match serde_json::from_slice::<Prefs>(&text) {
      Ok(p) => *self = p,
      Err(e) => crate::log_warn!("unreadable preferences (%s): defaults used", e.to_string()),
    }
    // before the first-run assistant: a library holding the extracted game was the user's choice
    if !crate::io::asset_library::game_folder(&self.library_root).is_empty() {
      self.library_chosen = true;
    }
  }

  pub fn save(&self) {
    self.save_to(Path::new(&Self::dir()));
  }

  pub fn save_to(&self, dir: &Path) {
    if READ_ONLY.load(Ordering::Relaxed) {
      return;
    }
    let _ = std::fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string_pretty(self) {
      let _ = std::fs::write(dir.join(K_FILE), text + "\n");
    }
  }

  pub fn add_recent_project(&mut self, path: &str) {
    push_recent(&mut self.recent_projects, path, 10);
  }
}

fn push_recent(list: &mut Vec<String>, v: &str, max: usize) {
  list.retain(|x| x != v);
  list.insert(0, v.to_string());
  list.truncate(max);
}

#[cfg(test)]
mod tests {
  use super::*;

  // the standalone app's, the launcher's and the C++ editor's folders: one folder left, holding
  // the newest preferences and their caches
  #[test]
  fn the_old_folders_become_one() {
    let tmp = std::env::temp_dir().join(format!("jle-prefs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let new = tmp.join("OpenGOAL/level-editor");
    let old = [
      "OpenGOAL/jak-level-editor",
      "OpenGOAL-Launcher/jak-level-editor",
      "OpenGOAL/open-goal-level-editor",
    ]
    .map(|d| tmp.join(d));
    let now = std::time::SystemTime::now();
    for (i, (d, language)) in old.iter().zip(["fr", "en", "de"]).enumerate() {
      std::fs::create_dir_all(d.join("thumbs")).unwrap();
      let p = Prefs {
        language: language.into(),
        ..Prefs::default()
      };
      p.save_to(d);
      let age = std::time::Duration::from_secs(3600 * (i as u64 + 1));
      std::fs::File::options()
        .write(true)
        .open(d.join(K_FILE))
        .unwrap()
        .set_modified(now - age)
        .unwrap();
    }
    migrate(&new, &old);
    let mut p = Prefs::default();
    p.load_from(&new);
    assert_eq!(p.language, "fr", "the newest preferences kept");
    assert!(new.join("thumbs").is_dir(), "with their caches");
    assert!(old.iter().all(|d| !d.exists()), "the old folders deleted");

    // an old folder made again (by an earlier launcher's editor) is deleted, the preferences kept
    Prefs {
      language: "en".into(),
      ..Prefs::default()
    }
    .save_to(&old[1]);
    migrate(&new, &old);
    p.load_from(&new);
    assert_eq!(p.language, "fr");
    assert!(!old[1].exists());

    p.add_recent_project("b");
    p.add_recent_project("a");
    p.add_recent_project("b");
    assert_eq!(p.recent_projects, ["b", "a"]);
    assert!(
      Prefs::dir().ends_with("OpenGOAL/level-editor/")
        || Prefs::dir() == "./"
        || std::env::var_os("JLE_PREFS_DIR").is_some()
    );
    let _ = std::fs::remove_dir_all(&tmp);
  }

  // the first-run assistant: asked until a library is chosen; preferences of before it whose
  // library holds the extracted game count as chosen (the user's library kept)
  #[test]
  fn library_chosen_migrated() {
    let tmp = std::env::temp_dir().join(format!("jle-prefs-chosen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (dir, lib) = (tmp.join("prefs"), tmp.join("lib"));
    std::fs::create_dir_all(lib.join("jak3")).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let write = |root: &std::path::Path| {
      let j = serde_json::json!({ "library_root": root.to_string_lossy() });
      std::fs::write(dir.join(K_FILE), j.to_string()).unwrap();
    };
    let load = || {
      let mut p = Prefs::default();
      p.load_from(&dir);
      p
    };
    write(&lib);
    assert!(!load().library_chosen, "no game there yet: asked");
    std::fs::write(lib.join("jak3/library.json"), r#"{"game": "jak3"}"#).unwrap();
    assert!(
      load().library_chosen,
      "an extracted game: the user's choice"
    );
    write(&tmp.join("elsewhere"));
    assert!(!load().library_chosen);
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
