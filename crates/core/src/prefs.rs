//! The editor's preferences, kept between sessions in the user's settings folder
//! (%APPDATA%/OpenGOAL/jak-level-editor on Windows, ~/.local/share/... on Linux), in the one its
//! host gives (`Prefs::use_dir`), or in $JLE_PREFS_DIR (the OpenGOAL Launcher gives its own
//! settings folder, so that its editor never shares a library with the standalone app's): where
//! the extracted game is, the disc it came from, the language,
//! recent projects. Thumbnails and the catalog's index are cached next to them.
//!
//! The C++ editor kept them in OpenGOAL/open-goal-level-editor: while the standalone app's folder
//! has no preferences, that one's are read (and written to the new folder at the next save).

use std::path::Path;
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
  /// the folder ogle-extract writes to: the game in its subfolder jak3/
  pub library_root: String,
  /// the user's .iso or disc folder
  pub extract_source: String,
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
}

/// The settings folder of an application, as SDL_GetPrefPath makes it ('/' separated, ending
/// with '/'), "./" when the system has none.
fn pref_path(org: &str, app: &str) -> String {
  let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
  let base = if cfg!(windows) {
    env("APPDATA")
  } else if cfg!(target_os = "macos") {
    env("HOME").map(|h| format!("{h}/Library/Application Support"))
  } else {
    env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| format!("{h}/.local/share")))
  };
  match base {
    Some(base) => format!(
      "{}/{org}/{app}/",
      base.replace('\\', "/").trim_end_matches('/')
    ),
    None => "./".into(),
  }
}

impl Prefs {
  /// folder of the preferences and of the thumbnail cache (created on demand), '/' separated:
  /// $JLE_PREFS_DIR when set (tests of the application with fresh preferences), else the host's
  /// (use_dir), else the standalone app's
  pub fn dir() -> String {
    DIR
      .get_or_init(|| match std::env::var("JLE_PREFS_DIR") {
        Ok(d) if !d.is_empty() => slashed(&d),
        _ => pref_path("OpenGOAL", "jak-level-editor"),
      })
      .clone()
  }

  /// The host's folder for the preferences, given before they are first read (false after: the
  /// folder stays); $JLE_PREFS_DIR still comes first.
  pub fn use_dir(dir: &Path) -> bool {
    if std::env::var_os("JLE_PREFS_DIR").is_some_and(|d| !d.is_empty()) {
      return false;
    }
    DIR.set(slashed(&dir.to_string_lossy())).is_ok()
  }

  /// the C++ editor's folder, read for the standalone app's first preferences (none in another
  /// folder: fresh preferences)
  pub fn old_dir() -> String {
    if Self::dir() == pref_path("OpenGOAL", "jak-level-editor") {
      pref_path("OpenGOAL", "open-goal-level-editor")
    } else {
      Self::dir()
    }
  }

  pub fn get() -> MutexGuard<'static, Prefs> {
    static PREFS: LazyLock<Mutex<Prefs>> = LazyLock::new(Mutex::default);
    PREFS.lock().unwrap_or_else(|e| e.into_inner())
  }

  pub fn load(&mut self) {
    self.load_from(Path::new(&Self::dir()), Path::new(&Self::old_dir()));
  }

  /// Reads `dir`'s preferences, else (the folder new) `old_dir`'s.
  pub fn load_from(&mut self, dir: &Path, old_dir: &Path) {
    let file = dir.join(K_FILE);
    let file = if file.is_file() {
      file
    } else {
      old_dir.join(K_FILE)
    };
    let Ok(text) = std::fs::read(&file) else {
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

  #[test]
  fn migrated_from_the_old_folder() {
    let tmp = std::env::temp_dir().join(format!("jle-prefs-{}", std::process::id()));
    let (new, old) = (
      tmp.join("jak-level-editor"),
      tmp.join("open-goal-level-editor"),
    );
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(
      old.join(K_FILE),
      r#"{"library_root": "D:/lib", "language": "fr", "recent_projects": ["a"]}"#,
    )
    .unwrap();
    let mut p = Prefs::default();
    p.load_from(&new, &old);
    assert!(
      p.library_root == "D:/lib" && p.language == "fr" && !p.guide_seen,
      "the old folder's preferences read"
    );
    p.add_recent_project("b");
    p.add_recent_project("a");
    assert_eq!(p.recent_projects, ["a", "b"]);
    p.save_to(&new);
    let mut again = Prefs::default();
    std::fs::remove_file(old.join(K_FILE)).unwrap();
    again.load_from(&new, &old);
    assert_eq!(again, p, "written to the new folder");
    assert!(
      Prefs::dir().ends_with("OpenGOAL/jak-level-editor/")
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
    let (dir, old, lib) = (tmp.join("prefs"), tmp.join("old"), tmp.join("lib"));
    std::fs::create_dir_all(lib.join("jak3")).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let write = |root: &std::path::Path| {
      let j = serde_json::json!({ "library_root": root.to_string_lossy() });
      std::fs::write(dir.join(K_FILE), j.to_string()).unwrap();
    };
    let load = || {
      let mut p = Prefs::default();
      p.load_from(&dir, &old);
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
