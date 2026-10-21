#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

// Jak Level Editor on its own: the same editor the OpenGOAL Launcher opens with its Level Editor
// button (crates/tauri-plugin, ui/), with its command line:
//   jak-level-editor [project.glb] [--library <folder>] [--lang <code>] [--remote-port <port>]
//                    [--hidden] [--open-level <name>]

fn main() {
  tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_dialog::init())
    .plugin(
      tauri_plugin_jak_level_editor::Builder::new()
        .args(std::env::args())
        .build(),
    )
    .run(tauri::generate_context!())
    .expect("Jak Level Editor could not start");
}
