#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

// Jak Level Editor (crates/tauri-plugin, ui/), with its command line:
//   jak-level-editor [project.glb] [--library <folder>] [--lang <code>] [--remote-port <port>]
//                    [--hidden] [--open-level <name>]
//                    [--default-library=<folder>] [--game-install=<game>=<folder>]...
// The OpenGOAL Launcher's Level Editor button starts this program (its portable build) with the
// last two, its library and the games it installed.

use tauri_plugin_jak_level_editor::Host;

// what this app gives the editor (docs/architecture.md §16): the library in Documents/Jak Level
// Editor (known before the app, so the editor starts on it), unless the command line gives
// another. The games installed with OpenGOAL, whose levels are extracted and tried, are chosen in
// its settings.
fn host() -> Host {
  Host {
    name: "standalone".into(),
    default_library: dirs::document_dir().map(|d| d.join("Jak Level Editor")),
    ..Default::default()
  }
}

fn main() {
  tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_dialog::init())
    .plugin(
      tauri_plugin_jak_level_editor::Builder::new()
        .args(std::env::args())
        .host(host())
        .build(),
    )
    .run(tauri::generate_context!())
    .expect("Jak Level Editor could not start");
}
