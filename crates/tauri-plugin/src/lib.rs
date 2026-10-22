//! Jak Level Editor inside a Tauri application: `.plugin(tauri_plugin_jak_level_editor::init())`,
//! or with the editor's command line (its own app):
//! `.plugin(tauri_plugin_jak_level_editor::Builder::new().args(std::env::args()).build())`.
//! The interface (ui/) calls the engine through the `call` command and reads the viewport's data
//! on the `jle://` protocol (docs/rust-architecture.md).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use jle_core::editor::protocol::{ProtocolState, content_type, encode_mesh, materials_json};
use jle_core::editor::remote::K_REMOTE_PORT;
use jle_core::editor::{Editor, Notify, SharedEditor};
pub use jle_core::host::{
  ExtractorProvider, GameRuntime, Host, HostEvent, HostEvents, LevelManifest, LevelPackage,
  PlayOutcome, RuntimeInfo,
};
use jle_core::i18n::{i18n_init, i18n_select, i18n_system_language, lang_dir, set_lang_dir};
use jle_core::io::game_data::set_data_dir;
use jle_core::prefs::{Prefs, READ_ONLY};
use jle_core::{log_error, log_info};
use serde_json::Value;
use tauri::http::{Request, Response, StatusCode};
use tauri::plugin::TauriPlugin;
use tauri::{Emitter, Manager, Runtime};

/// The engine, shared by the commands, the protocol, the jobs and the remote control.
#[derive(Clone)]
pub struct EditorState(pub SharedEditor);

impl EditorState {
  pub fn lock(&self) -> MutexGuard<'_, Editor> {
    jle_core::editor::lock(&self.0)
  }
}

/// The server side of the jle:// protocol (the last scene sent, the meshes by uid).
#[derive(Default)]
struct Protocol(Mutex<ProtocolState>);

/// What the application asked for (its command line): see `Builder::args`.
#[derive(Clone, Debug, Default)]
pub struct Options {
  /// a project (.glb) to open
  pub open_path: String,
  /// a level of the library to open (a developer's shortcut: `--open-level ctyinda`)
  pub open_level: String,
  /// the library folder for this session
  pub library: String,
  /// the interface's language for this session
  pub language: String,
  /// the MCP server's connection for this session (it starts the editor with --remote-port);
  /// None or 0: as the preferences say (Prefs::remote_control)
  pub remote_port: Option<u16>,
  /// no window shown, the preferences left unchanged (tests)
  pub hidden: bool,
}

/// The plugin with the application's options: `Builder::new().args(std::env::args()).build()`,
/// and what the application gives the editor (`host`: jle_core::host).
#[derive(Default)]
pub struct Builder {
  options: Options,
  host: Host,
  prefs_in_app_config: bool,
}

impl Builder {
  pub fn new() -> Self {
    Self::default()
  }

  /// The editor's command line (the first argument is the program):
  /// `[project.glb] --library <folder> --lang <code> --remote-port <port> --hidden
  /// --open-level <name>`
  pub fn args(mut self, args: impl IntoIterator<Item = String>) -> Self {
    let mut args = args.into_iter().skip(1);
    let o = &mut self.options;
    while let Some(a) = args.next() {
      let mut next = || args.next().unwrap_or_default();
      match a.as_str() {
        "--library" => o.library = next(),
        "--lang" => o.language = next(),
        "--remote-port" => o.remote_port = next().parse().ok(),
        "--hidden" => o.hidden = true,
        "--open-level" => o.open_level = next(),
        _ if a.starts_with("--") => {}
        _ => o.open_path = a,
      }
    }
    self
  }

  pub fn options(mut self, options: Options) -> Self {
    self.options = options;
    self
  }

  /// What the application gives the editor: the default library, a disc already extracted, the
  /// extractor, the game runtime (jle_core::host::Host).
  pub fn host(mut self, host: Host) -> Self {
    self.host = host;
    self
  }

  /// The editor's preferences (its library, its caches) in `<the app's config folder>/
  /// jak-level-editor/` rather than the standalone app's folder: a host application's editor
  /// never shares them with the standalone app (jle_core::prefs::Prefs::use_dir).
  pub fn prefs_in_app_config(mut self) -> Self {
    self.prefs_in_app_config = true;
    self
  }

  pub fn build<R: Runtime>(self) -> TauriPlugin<R> {
    let options = self.options;
    let host = self.host;
    let prefs_in_app_config = self.prefs_in_app_config;
    let hidden = options.hidden;
    tauri::plugin::Builder::new("jak-level-editor")
      .invoke_handler(tauri::generate_handler![call])
      .register_asynchronous_uri_scheme_protocol("jle", |ctx, request, responder| {
        let app = ctx.app_handle().clone();
        std::thread::spawn(move || {
          let editor = app.state::<EditorState>().inner().clone();
          let protocol = app.state::<Protocol>();
          responder.respond(serve(&editor, &protocol, &request));
        });
      })
      .on_webview_ready(move |webview| {
        if hidden {
          let _ = webview.window().hide();
        }
      })
      .setup(move |app, _api| {
        if prefs_in_app_config && let Ok(dir) = app.path().app_config_dir() {
          Prefs::use_dir(&dir.join("jak-level-editor"));
        }
        setup(app, options, host);
        Ok(())
      })
      .build()
  }
}

/// The editor's engine, for a host that needs more than the plugin (the launcher installs the
/// levels exported for the game with `jle_core::io`).
pub use jle_core;

/// The version of the editor this plugin is: its release holds the ogle-extract a host downloads.
pub const EDITOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the application gives the editor, changed while it runs (the launcher's installation
/// folder is known once its settings are read, and the user can move it).
pub fn set_host<R: Runtime>(app: &tauri::AppHandle<R>, host: Host) {
  if let Some(state) = app.try_state::<EditorState>() {
    let mut ed = state.lock();
    let before = ed.default_library();
    ed.set_host(host);
    // a library not chosen yet follows the host's default (the first-run assistant offers it)
    let default = ed.default_library();
    if !Prefs::get().library_chosen && default != before {
      let _ = ed.call("set_library", &serde_json::json!({ "path": default }));
    }
  }
}

/// The plugin without options (the OpenGOAL Launcher's): the library and the language of the
/// preferences.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new().build()
}

fn setup<R: Runtime>(app: &tauri::AppHandle<R>, options: Options, host: Host) {
  if options.hidden {
    READ_ONLY.store(true, std::sync::atomic::Ordering::Relaxed);
  }
  // the translations and the game's tables: the application's resources when it is bundled with
  // them (tauri.conf.json bundle.resources), else next to the executable or the repository's. In a
  // host application (the launcher) they are under jak-level-editor/, apart from its own.
  let resources = app.path().resource_dir().ok().map(|dir| {
    let own = dir.join("jak-level-editor");
    if own.is_dir() { own } else { dir }
  });
  if let Some(dir) = &resources {
    if dir.join("lang").is_dir() {
      set_lang_dir(dir.join("lang"));
    }
    if dir.join("data").is_dir() {
      set_data_dir(dir.join("data"));
    }
  }
  Prefs::get().load();
  // the interface's language: the session's, the one chosen before, else the system's
  i18n_init(&lang_dir(), "en");
  let language = if options.language.is_empty() {
    Prefs::get().language.clone()
  } else {
    options.language.clone()
  };
  i18n_select(&if language.is_empty() {
    i18n_system_language()
  } else {
    language
  });

  let handle = app.clone();
  let notify: Notify = Arc::new(move |event, payload| {
    let _ = handle.emit(event, payload);
  });
  let mut editor = Editor::new();
  editor.extraction.resource_dir = resources;
  editor.set_host(host);
  let shared = editor.shared(Some(notify));
  {
    let mut ed = jle_core::editor::lock(&shared);
    // the library: the session's (a choice: no first-run assistant), else the one the user chose
    // (or used last with an extracted game: Prefs::load), else the host's default until the
    // first-run assistant asks (Editor::default_library)
    let mut root = options.library.clone();
    if !root.is_empty() {
      Prefs::get().library_chosen = true;
    } else if Prefs::get().library_chosen {
      root = Prefs::get().library_root.clone();
    } else {
      root = ed.default_library();
    }
    ed.set_library(&root);
    ed.new_document();
    if !options.open_path.is_empty()
      && let Err(e) = ed.open_project_file(&options.open_path)
    {
      log_error!("%s", e);
    }
    if !options.open_level.is_empty()
      && let Err(e) = ed.load_level(&options.open_level, false)
    {
      log_error!("%s", e);
    }
  }
  log_info!("Jak Level Editor %s", env!("CARGO_PKG_VERSION"));
  // the remote control of the MCP server: for the session it started the editor for, else when the
  // user lets an assistant control it
  {
    let mut ed = jle_core::editor::lock(&shared);
    ed.remote_session = options.remote_port.filter(|p| *p != 0);
    let wanted = Prefs::get().remote_control.then_some(K_REMOTE_PORT);
    if let Some(port) = ed.remote_session.or(wanted) {
      ed.start_remote_control(port);
    }
  }
  // the game view's hour passing, a storm's lightning (GameWeather::update)
  let weather = shared.clone();
  std::thread::spawn(move || {
    let mut last = std::time::Instant::now();
    loop {
      std::thread::sleep(Duration::from_millis(50));
      let dt = last.elapsed().as_secs_f32().min(0.1);
      last = std::time::Instant::now();
      jle_core::editor::lock(&weather).tick(dt);
    }
  });
  app.manage(EditorState(shared));
  app.manage(Protocol::default());
  app.manage(options);
}

#[tauri::command]
async fn call(
  state: tauri::State<'_, EditorState>,
  name: String,
  args: Option<Value>,
) -> Result<Value, String> {
  let state = state.inner().clone();
  let args = args.unwrap_or(Value::Null);
  tauri::async_runtime::spawn_blocking(move || state.lock().call(&name, &args))
    .await
    .map_err(|e| e.to_string())?
}

fn answer(status: StatusCode, content_type: &str, body: Vec<u8>) -> Response<Vec<u8>> {
  Response::builder()
    .status(status)
    .header("Content-Type", content_type)
    // the page's origin is the app's, not jle's: its fetches are cross-origin
    .header("Access-Control-Allow-Origin", "*")
    .header("Cache-Control", "no-store")
    .body(body)
    .unwrap()
}

fn bytes(body: Option<Vec<u8>>) -> Response<Vec<u8>> {
  match body {
    Some(b) => answer(StatusCode::OK, "application/octet-stream", b),
    None => answer(StatusCode::NOT_FOUND, "text/plain", Vec::new()),
  }
}

/// The jle:// protocol (docs/rust-architecture.md §4). What it needs of the editor is copied
/// under its lock, then encoded without it.
fn serve(
  editor: &EditorState,
  protocol: &Protocol,
  request: &Request<Vec<u8>>,
) -> Response<Vec<u8>> {
  let path = request.uri().path().trim_start_matches('/');
  let query = request.uri().query().unwrap_or_default();
  let param = |key: &str| {
    tauri::Url::parse(&format!("http://x/?{query}"))
      .ok()
      .and_then(|u| {
        u.query_pairs()
          .find(|(k, _)| k == key)
          .map(|(_, v)| v.into_owned())
      })
  };
  let number = |s: &str| s.parse::<u32>().ok();
  let proto = || protocol.0.lock().unwrap_or_else(|e| e.into_inner());
  let (route, rest) = path.split_once('/').unwrap_or((path, ""));
  match route {
    "scene" => {
      let snap = editor.lock().scene_snapshot();
      let full = param("full").is_some_and(|v| v != "0");
      bytes(Some(proto().encode_scene(snap, full)))
    }
    "mesh" => {
      let Some(uid) = number(rest) else {
        return bytes(None);
      };
      let mut mesh = proto().meshes.get(&uid).cloned();
      if mesh.is_none() {
        // asked before the scene (or a mesh just made): the editor's meshes now
        mesh = editor.lock().scene_snapshot().meshes.remove(&uid);
      }
      bytes(mesh.map(|m| encode_mesh(&m)))
    }
    "texture" => bytes(number(rest).and_then(|i| editor.lock().texture_bytes(i as usize))),
    "materials" => {
      let j = materials_json(&editor.lock().scene);
      answer(
        StatusCode::OK,
        "application/json",
        j.to_string().into_bytes(),
      )
    }
    "gameview" => {
      let mut guard = editor.lock();
      let Editor {
        scene, game_chunks, ..
      } = &mut *guard;
      match rest.split_once('/') {
        None if rest.is_empty() => {
          game_chunks.update(scene);
          bytes(Some(game_chunks.encode_chunk_list()))
        }
        Some(("chunk", id)) => bytes(number(id).and_then(|id| game_chunks.encode_chunk(id))),
        Some(("palette", key)) => bytes(number(key).and_then(|k| game_chunks.encode_palette(k))),
        _ => bytes(None),
      }
    }
    "asset" => {
      let file: Option<PathBuf> = param("path").and_then(|p| editor.lock().asset_path(&p));
      match file.and_then(|f| std::fs::read(&f).ok().map(|b| (f, b))) {
        Some((f, b)) => answer(StatusCode::OK, content_type(&f), b),
        None => bytes(None),
      }
    }
    // the panels' pictures: preview/<kind>, thumb/<name>, palette (editor/thumbs.rs)
    "preview" | "thumb" | "palette" => {
      match jle_core::editor::thumbs::serve(&editor.0, route, rest, &param) {
        Some((ct, b)) => answer(StatusCode::OK, ct, b),
        None => bytes(None),
      }
    }
    // tod/<node>: superseded by gameview
    _ => bytes(None),
  }
}
