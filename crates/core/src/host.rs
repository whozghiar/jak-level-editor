//! What the application hosting the editor gives it (the standalone app, the OpenGOAL Launcher):
//! where the library goes by default, a disc already extracted to read the levels from, the
//! extractor, and a game runtime that installs a level, removes it and starts the game on it.
//! The editor never asks which application hosts it: it uses what it is given, and hides what it
//! is not given (no runtime: no Play button, the export and its README instead).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A step of a long operation, for the interface: a translation key with its arguments, its
/// progress when known, a line of a tool's output.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostEvent {
  Step { key: String, args: Vec<String> },
  Progress { done: u64, total: u64 },
  Log { line: String },
}

pub type HostEvents = Arc<dyn Fn(HostEvent) + Send + Sync>;

/// The manifest of a level exported for the game (`jle-level.json` at the root of the export):
/// what the README tells a person to do, for a runtime to do it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelManifest {
  pub format: u32,
  /// "jak3"
  pub game: String,
  /// the level's name ("demo-oasis"), its folder in custom_assets/<game>/levels/
  pub name: String,
  /// its three-letter nickname
  pub nickname: String,
  /// a level of the game it takes the place of ("ctyindb"), "" for a level of its own
  pub replaces: String,
  /// its DGO ("DMO.DGO")
  pub dgo: String,
  /// the continue point the game can start it at ("demo-oasis-start"), and where it is (meters)
  pub continue_point: String,
  pub start: [f32; 3],
  /// the lines to add to goal_src/<game>/game.gp
  pub game_gp: Vec<String>,
  /// a level of its own: the file (relative to the export) holding its `define` for
  /// goal_src/<game>/engine/level/level-info.gc, whose `:index` the installer makes unique
  pub level_info: String,
  /// the files of the export (relative paths, '/'), copied into the game's data folder as they are
  pub files: Vec<String>,
  /// the editor that wrote it
  pub editor_version: String,
}

/// A level exported for the game, ready to install: the folder of an export and its manifest.
#[derive(Clone, Debug)]
pub struct LevelPackage {
  pub dir: PathBuf,
  pub manifest: LevelManifest,
}

impl LevelPackage {
  pub const MANIFEST: &'static str = "jle-level.json";

  pub fn read(dir: &Path) -> Result<LevelPackage, String> {
    let path = dir.join(Self::MANIFEST);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(LevelPackage {
      dir: dir.to_path_buf(),
      manifest,
    })
  }
}

/// What a runtime is, for the interface (the Play button, the settings).
#[derive(Clone, Debug, Default, Serialize)]
pub struct RuntimeInfo {
  /// "jak-project" (a checkout of OpenGOAL's sources), "opengoal" (an installed OpenGOAL),
  /// "launcher" (the OpenGOAL Launcher's isolated game)
  pub kind: String,
  /// what the interface shows ("OpenGOAL Launcher", the folder of a checkout...)
  pub label: String,
  /// it can install and play now; else `reason`, a translation key, says why not
  pub available: bool,
  pub reason: String,
  /// the games it plays ("jak3")
  pub games: Vec<String>,
  /// the levels installed in it (their names)
  pub installed: Vec<String>,
  /// what the Play window says besides, a translation key and its arguments ("" nothing): the
  /// launcher's game says a newer release of the tooling is out
  pub notice: String,
  pub notice_args: Vec<String>,
}

/// What `play` did: the game is starting, or the host took the operation over in its own
/// interface (the launcher's jobs: their progress is shown there).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayOutcome {
  Started,
  HandedOver,
}

/// The game runtime a host gives the editor: installing a level exported for the game, removing
/// it, starting the game on it. The calls block (the editor runs them on a worker thread) and
/// report their steps through `events`.
pub trait GameRuntime: Send + Sync {
  fn info(&self) -> RuntimeInfo;
  /// copies the level into the game's sources, registers it (game.gp, level-info.gc) and builds
  /// the game, the level included
  fn install_level(&self, package: &LevelPackage, events: HostEvents) -> Result<(), String>;
  /// takes the level out of the game's sources and builds the game again
  fn remove_level(&self, game: &str, name: &str, events: HostEvents) -> Result<(), String>;
  /// starts the game on an installed level (its continue point)
  fn launch(&self, game: &str, name: &str, events: HostEvents) -> Result<(), String>;
  /// installs then launches; a host may hand the whole of it to its own interface
  fn play(&self, package: &LevelPackage, events: HostEvents) -> Result<PlayOutcome, String> {
    self.install_level(package, events.clone())?;
    let m = &package.manifest;
    self.launch(&m.game, &m.name, events)?;
    Ok(PlayOutcome::Started)
  }
  /// what the editor's settings show of it beyond `info` (the standalone app's: the OpenGOALs it
  /// can use, which one it uses), null when nothing
  fn settings(&self) -> serde_json::Value {
    serde_json::Value::Null
  }
  /// where it installs OpenGOAL's tooling itself (the standalone app's
  /// `<library>/opengoal/versions/`): the editor then offers its releases, their installation and
  /// the one used (crate::tooling). None: the host does it (the OpenGOAL Launcher: its header, its
  /// Versions page)
  fn tooling_dir(&self) -> Option<PathBuf> {
    None
  }
}

/// A host that can fetch ogle-extract when it is missing (the launcher downloads the one of the
/// editor's version from its release).
pub trait ExtractorProvider: Send + Sync {
  /// the path of the executable, its tables (ogle-extract-data/) next to it
  fn fetch(&self, events: HostEvents) -> Result<PathBuf, String>;
}

/// Everything a host gives the editor (`tauri_plugin_jak_level_editor::Builder::host`).
#[derive(Clone, Default)]
pub struct Host {
  /// "standalone", "launcher": for the log and the interface's texts only, never to decide
  pub name: String,
  /// where the library goes when the user has not chosen yet (the first-run assistant offers it)
  pub default_library: Option<PathBuf>,
  /// a disc the host already extracted (the launcher's iso_data/jak3): the extraction's default
  /// source, so the disc is not asked again
  pub extract_source: Option<PathBuf>,
  /// ogle-extract when the host has it at hand
  pub extractor: Option<PathBuf>,
  pub extractor_provider: Option<Arc<dyn ExtractorProvider>>,
  pub runtime: Option<Arc<dyn GameRuntime>>,
}

impl std::fmt::Debug for Host {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Host")
      .field("name", &self.name)
      .field("default_library", &self.default_library)
      .field("extract_source", &self.extract_source)
      .field("extractor", &self.extractor)
      .field("extractor_provider", &self.extractor_provider.is_some())
      .field("runtime", &self.runtime.is_some())
      .finish()
  }
}
