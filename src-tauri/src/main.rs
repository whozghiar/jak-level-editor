#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

// Jak Level Editor on its own: the same editor the OpenGOAL Launcher opens with its Level Editor
// button (crates/tauri-plugin, ui/), with its command line:
//   jak-level-editor [project.glb] [--library <folder>] [--lang <code>] [--remote-port <port>]
//                    [--hidden] [--open-level <name>]

use tauri::Manager;
use tauri_plugin_jak_level_editor::{Host, set_host};

// what this app gives the editor (docs/rust-architecture.md §6): the library in Documents/Jak Level
// Editor (known before the app, so the editor starts on it). No game runtime: the standalone app
// exports the levels; the guide tells how to add one to the game by hand.
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
    // and the ogle-extract installed with it, once the app knows its resources
    .setup(|app| {
      let exe = if cfg!(windows) {
        "ogle-extract.exe"
      } else {
        "ogle-extract"
      };
      let extractor = app.path().resource_dir().ok().map(|d| d.join(exe));
      let extractor = extractor.filter(|e| e.is_file());
      if extractor.is_some() {
        set_host(
          app.handle(),
          Host {
            extractor,
            ..host()
          },
        );
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("Jak Level Editor could not start");
}
