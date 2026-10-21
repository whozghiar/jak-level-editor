//! Jak Level Editor inside a Tauri application: `.plugin(tauri_plugin_jak_level_editor::init())`.
//! The interface (ui/) calls the engine through the `call` command and reads the viewport's data
//! on the `jle://` protocol (docs/rust-architecture.md).

use std::sync::{Arc, Mutex, MutexGuard};

use jle_core::editor::Editor;
use serde_json::Value;
use tauri::http::{Request, Response, StatusCode};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Emitter, Manager, Runtime};

/// The engine, shared by the commands, the protocol, the jobs and the remote control.
#[derive(Clone)]
pub struct EditorState(pub Arc<Mutex<Editor>>);

impl EditorState {
  pub fn lock(&self) -> MutexGuard<'_, Editor> {
    // a panic during a call leaves the document as it was at that moment: still usable
    self.0.lock().unwrap_or_else(|e| e.into_inner())
  }
}

#[tauri::command]
async fn call(state: tauri::State<'_, EditorState>, name: String, args: Option<Value>) -> Result<Value, String> {
  let state = state.inner().clone();
  let args = args.unwrap_or(Value::Null);
  tauri::async_runtime::spawn_blocking(move || state.lock().call(&name, &args))
    .await
    .map_err(|e| e.to_string())?
}

fn serve(state: &EditorState, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
  let _ = (state, request);
  Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()).unwrap()
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("jak-level-editor")
    .invoke_handler(tauri::generate_handler![call])
    .register_asynchronous_uri_scheme_protocol("jle", |ctx, request, responder| {
      let state = ctx.app_handle().state::<EditorState>().inner().clone();
      std::thread::spawn(move || responder.respond(serve(&state, &request)));
    })
    .setup(|app, _api| {
      let mut editor = Editor::new();
      let handle = app.clone();
      editor.set_notify(Box::new(move |event, payload| {
        let _ = handle.emit(event, payload);
      }));
      app.manage(EditorState(Arc::new(Mutex::new(editor))));
      Ok(())
    })
    .build()
}
