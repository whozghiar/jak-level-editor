//! What the application hosting the editor gives it (the app, completed by the command line of an
//! application starting it, as the OpenGOAL Launcher does): where the library goes by default, the
//! discs already extracted to read the levels from, the extractor. The editor never asks who gave
//! them: it uses what it is given. It runs no game: a level is exported, its README and the guide's
//! tutorial tell how to add it to the game.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

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

/// A host that can fetch ogle-extract when it is missing (the Extract window's **Download
/// ogle-extract**).
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
  /// the discs the host already extracted, by game ("jak3": the launcher's iso_data/jak3): the
  /// extraction's default source for that game, so the disc is not asked again
  pub extract_sources: BTreeMap<String, PathBuf>,
  /// ogle-extract when the host has it at hand
  pub extractor: Option<PathBuf>,
  pub extractor_provider: Option<Arc<dyn ExtractorProvider>>,
}

impl std::fmt::Debug for Host {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Host")
      .field("name", &self.name)
      .field("default_library", &self.default_library)
      .field("extract_sources", &self.extract_sources)
      .field("extractor", &self.extractor)
      .field("extractor_provider", &self.extractor_provider.is_some())
      .finish()
  }
}
