//! What the application hosting the editor gives it (the app, completed by the command line of an
//! application starting it, as the OpenGOAL Launcher does): where the library goes by default, the
//! games installed. The editor never asks who gave them: it uses what it is given. It installs no
//! game: it extracts the levels of a game installed (editor/extract.rs), tries a level in it
//! (editor/game_test.rs), or exports it, its README and the guide's tutorial telling how to add it
//! to the game.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Everything a host gives the editor (`tauri_plugin_jak_level_editor::Builder::host`).
#[derive(Clone, Debug, Default)]
pub struct Host {
  /// "standalone", "launcher": for the log and the interface's texts only, never to decide
  pub name: String,
  /// where the library goes when the user has not chosen yet (the first-run assistant offers it)
  pub default_library: Option<PathBuf>,
  /// the games the host installed, by game ("jak3": the OpenGOAL Launcher's installation folder):
  /// where the levels are extracted from and tried (io/game_install.rs), instead of the folders of
  /// the settings
  pub game_installs: BTreeMap<String, PathBuf>,
}
