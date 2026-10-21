//! The editor's preferences, kept between sessions in the user's settings folder
//! (%APPDATA%/OpenGOAL/jak-level-editor on Windows, ~/.local/share/... on Linux): where the
//! extracted game is, the disc it came from, the language, recent projects. Thumbnails are
//! cached next to them.
//!
//! The C++ editor kept them in OpenGOAL/open-goal-level-editor: while the new folder has no
//! preferences, that one's are read (and written to the new folder at the next save).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};

use serde::{Deserialize, Serialize};

/// the command line modes (captures, tests) read the preferences, never write them
pub static READ_ONLY: AtomicBool = AtomicBool::new(false);

const K_FILE: &str = "prefs.json";

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
  /// folder of the preferences and of the thumbnail cache (created on demand), '/' separated
  pub fn dir() -> String {
    static DIR: OnceLock<String> = OnceLock::new();
    DIR
      .get_or_init(|| pref_path("OpenGOAL", "jak-level-editor"))
      .clone()
  }

  /// the C++ editor's folder
  pub fn old_dir() -> String {
    pref_path("OpenGOAL", "open-goal-level-editor")
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
    assert!(Prefs::dir().ends_with("OpenGOAL/jak-level-editor/") || Prefs::dir() == "./");
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
