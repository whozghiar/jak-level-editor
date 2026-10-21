//! The application without its interface (src/app/app.h): the document open, its history, the
//! selection, the tools, the jobs. The interface reaches it through `Editor::call` (api.rs).
//!
//! Split like the C++ App, one file per area, each an `impl Editor` block (and a sub-struct field
//! of the Editor for its own state):
//!   mod.rs        the struct, the jobs, the change notification, the library, the view settings
//!   levels.rs     the levels of the library opened and added (actions.cpp), several levels
//!   project.rs    projects: open, save (only when the user asks), new documents
//!   selection.rs  the selection, the clipboard, moves, the inspector's edits, picking
//!   groups.rs     the groups of the document (groups.cpp)
//!   inspector.rs  the inspector: its counts, the selection's group, its edits (panels.cpp)
//!   behaviors.rs  an actor's behavior and parameters, a piece of decor's (behavior_panel.cpp)
//!   surfaces.rs   a shape's size, the materials, a model imported (surface_panel.cpp)
//!   ocean.rs      the ocean of the level
//!   state.rs      what the interface shows: get_state, a node, the levels' list
//!   tools.rs      placeholders of the Place, topology and terrain tools (their areas replace them)
//!   effects.rs    the particle effects the view runs
//!   protocol.rs   the jle:// protocol's encoders (the plugin serves them)
//!   extract.rs    the extraction of the game's assets (extract.cpp)
//!   api.rs        `call`: every call by name
//!   placement.rs  the Place tool, prefabs, Light like here, a level's style (actions.cpp)
//!   catalog.rs    the catalog, the library's index, the palette, the game's prefabs: the panels'
//!                 lists and the workers making them
//!   thumbs.rs     the panels' pictures: what the interface draws, the PNGs it drew
//!   remote.rs     the remote control (MCP server): a local port, one JSON line per request
//!   remote_tools.rs  the tools of the MCP server
//!   terrain_tool.rs  the terrain brush (terrain_tool.cpp)
//!   topology_tool.rs the points and faces of one element's mesh (topology_tool.cpp)
//!   preview.rs    the inspector's preview of an animation or an effect (preview.cpp)

mod api;
mod behaviors;
mod catalog;
mod effects;
pub mod export;
pub mod extract;
mod groups;
mod inspector;
pub mod levels;
mod ocean;
mod panels;
mod pick_calls;
pub mod placement;
mod preview;
mod project;
pub mod protocol;
pub mod remote;
mod remote_tools;
mod selection;
mod state;
mod surfaces;
pub mod terrain_tool;
pub mod thumbs;
#[cfg(test)]
mod tests;
mod tools;
mod topology_tool;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::game::mood::GameWeather;
use crate::game::view::GameViewChunks;
use crate::history::History;
use crate::i18n::{i18n_select, tr};
use crate::io::asset_library::{AssetLibrary, LevelEntry, game_folder, library_root_of};
use crate::io::game_data::{GameData, game_data_path, level_listed};
use crate::log::Log;
use crate::math::{AABB, Vec3, normalize};
use crate::prefs::Prefs;
use crate::scene::Scene;
use crate::scene::story::StoryView;
use crate::{log_error, log_info, log_warn, strf};

pub use groups::{GroupInfo, group_tag, new_group_id, set_group_tag};
pub use levels::{
  Clipboard, DocumentLevel, LevelRole, add_level, copy_nodes, free_raised_strips, level_ground,
  open_level, open_project, paste_nodes, save_project,
};
pub use ocean::{ocean_land, ocean_land_around, ocean_water};

/// What the editor tells its interface (the plugin turns it into Tauri events): `jle:changed`,
/// `jle:log`, `jle:progress`, `jle:request`, with their payload. Shared with the worker threads
/// and the extraction's reader.
pub type Notify = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// The editor as the plugin keeps it: its worker threads lock it to apply what they made.
pub type SharedEditor = Arc<Mutex<Editor>>;

pub fn lock(editor: &Mutex<Editor>) -> MutexGuard<'_, Editor> {
  // a panic during a call leaves the document as it was at that moment: still usable
  editor.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolMode {
  /// select, move, rotate
  #[default]
  Select,
  /// put the chosen prefab, object or decor part where the cursor points
  Place,
  /// the points and faces of one element's mesh
  Topology,
  /// the terrain brush: ground painted, raised, dug, walls
  Terrain,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GizmoMode {
  #[default]
  Translate,
  Rotate,
  Scale,
}

/// The gizmo's steps (tools/gizmo.h GizmoSnap)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GizmoSnap {
  pub enabled: bool,
  /// meters
  pub translate: f32,
  pub rotate_deg: f32,
  /// steps of the scale factor
  pub scale: f32,
}

impl Default for GizmoSnap {
  fn default() -> Self {
    Self {
      enabled: false,
      translate: 0.5,
      rotate_deg: 15.0,
      scale: 0.1,
    }
  }
}

/// The gizmo as the toolbar sets it (the viewport draws and drags it)
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GizmoState {
  pub mode: GizmoMode,
  /// along the element's own axes, else the world's
  pub local: bool,
  pub snap: GizmoSnap,
}

/// The View menu: what the viewport draws (RenderSettings of the C++ and the App's flags)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSettings {
  /// the level drawn by the game's own renderer (View > Game rendering, G): on from the start
  pub game_view: bool,
  pub game_view_ready: bool,
  /// why there is none
  pub game_view_error: String,
  /// the particle effects of the levels
  pub effects: bool,
  pub actors: bool,
  pub ocean: bool,
  /// the distance fog of the backdrops
  pub fog: bool,
  pub navmesh: bool,
  pub labels: bool,
  pub grid: bool,
  pub textures: bool,
  /// the ground (locked) can be clicked and box-selected
  pub select_terrain: bool,
}

impl Default for ViewSettings {
  fn default() -> Self {
    Self {
      game_view: true,
      game_view_ready: true,
      game_view_error: String::new(),
      effects: true,
      actors: true,
      ocean: true,
      fog: true,
      navmesh: true,
      labels: true,
      grid: true,
      textures: true,
      select_terrain: false,
    }
  }
}

/// Where the viewport's camera is (render/camera.h; set_camera_state, once it stops moving)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraState {
  pub pos: Vec3,
  /// radians, 0 looks down -Z
  pub yaw: f32,
  pub pitch: f32,
  /// degrees
  pub fov: f32,
}

impl Default for CameraState {
  fn default() -> Self {
    Self {
      pos: Vec3::new(0.0, 10.0, 30.0),
      yaw: 0.0,
      pitch: -0.3,
      fov: 60.0,
    }
  }
}

impl CameraState {
  pub const ZFAR: f32 = 20000.0;

  pub fn forward(&self) -> Vec3 {
    let (y, p) = (self.yaw, self.pitch);
    Vec3::new(-y.sin() * p.cos(), p.sin(), -y.cos() * p.cos())
  }

  /// Frames a box: keeps the orientation, backs off so the box fits (render/camera.h)
  pub fn frame(&mut self, b: AABB) {
    if !b.valid() {
      return;
    }
    let r = b.radius().max(0.5);
    let dist = r / (self.fov.to_radians() * 0.5).tan() * 1.1;
    self.pos = b.center() - self.forward() * dist;
  }

  pub fn look_at(&mut self, target: Vec3) {
    let d = normalize(target - self.pos);
    self.pitch = d.y.clamp(-0.999, 0.999).asin();
    self.yaw = (-d.x).atan2(-d.z);
  }
}

/// A work going on (jle:progress): `{id, label, done, total, finished}`
#[derive(Clone, Debug, Default, Serialize)]
pub struct Job {
  pub id: String,
  pub label: String,
  pub done: usize,
  pub total: usize,
  pub finished: bool,
}

/// What get_state and the protocol compare to tell what changed (`notify_changes`).
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Watch {
  revision: u64,
  scene: u64,
  document: u64,
  selection: u64,
  materials: u64,
  textures: u64,
  state: u64,
  gameview: u64,
}

pub struct Editor {
  // ---- the document
  pub scene: Scene,
  pub history: History,
  pub selection: BTreeSet<u32>,
  pub primary: u32,
  /// what the cursor points at (the viewport's select tool)
  pub hovered: u32,
  pub tool: ToolMode,
  pub gizmo: GizmoState,
  pub view: ViewSettings,
  pub camera: CameraState,
  /// the project's .glb, "" before the first save
  pub project_path: String,
  pub saved_revision: u64,
  pub clipboard: Clipboard,
  /// the inspector's field being dragged: the node before it (edit_begin)
  edit_before: Option<crate::scene::Node>,
  /// the inspector's edit going on (a field dragged): the nodes before it (inspector.rs)
  inspector_change: Option<crate::history::NodeChange>,
  /// levels are opened and added with their actors (the objects: the creatures never come), or
  /// with their decor alone (Levels panel)
  pub load_actors: bool,
  // ---- the library: the extracted game, its levels
  pub library_root: String,
  /// empty when the game was not extracted; the workers read their own Arc of it
  pub library: Arc<AssetLibrary>,
  /// Jak II's levels extracted next to Jak 3's (<library>/jak2), their decor brought into Jak 3's
  pub library_jak2: Arc<AssetLibrary>,
  /// the game's data, read once (None inside: missing)
  game_data: Option<Option<Arc<GameData>>>,
  // ---- levels being read on a worker
  levels: levels::LevelJobs,
  // ---- the story: the point of the game the level is shown at, the steps where it changes
  pub story: StoryView,
  story_steps: Vec<i32>,
  /// where each level added changes, and the level the story bar shows ("" the opened)
  level_story_steps: BTreeMap<String, Vec<i32>>,
  story_level: String,
  /// history revision the story view was applied at
  story_revision: u64,
  // ---- the ocean of the level
  ocean: ocean::OceanState,
  // ---- groups, found again when the document changes
  groups: groups::GroupsCache,
  // ---- the game view: its chunks (the plugin serves them), its hour and weather
  pub game_chunks: GameViewChunks,
  pub weather: GameWeather,
  // ---- jobs, notification
  jobs: BTreeMap<String, Job>,
  notify: Option<Notify>,
  me: Weak<Mutex<Editor>>,
  /// bumped when the scene changes outside the history (a level opened, the story applied)
  scene_epoch: u64,
  /// bumped by each new document
  document_epoch: u64,
  /// requests to the interface (jle:request) and its answers (frontend_reply), by id
  next_request: u64,
  pub replies: BTreeMap<u64, Value>,
  // ---- extraction (extract.rs)
  pub extraction: extract::Extraction,
  // ---- the Place tool, prefabs (placement.rs); the library's lists and workers (catalog.rs)
  pub(crate) placing: placement::PlacingState,
  pub(crate) catalog: catalog::CatalogState,
  // ---- the terrain brush, the topology tool, the animation preview
  pub terrain: terrain_tool::TerrainState,
  pub(crate) topology: topology_tool::TopologyState,
  pub(crate) preview: preview::PreviewState,
  // ---- the export for the game, the nav meshes, the holes found (export.rs)
  pub export: export::ExportState,
  pub nav: export::NavState,
  pub gaps: Option<export::GapsFound>,
  /// the preferences changed are written (not in the tests)
  pub write_prefs: bool,
  /// the assistants connected through the MCP server (remote.rs)
  pub remote_clients: usize,
  /// the interface's viewport listens (call `frontend_ready`): it can make the pictures of the view
  pub interface_ready: bool,
}

impl Default for Editor {
  fn default() -> Self {
    Self::new()
  }
}

impl Editor {
  /// An editor on its own (tests): its jobs run in the call that starts them.
  pub fn new() -> Self {
    Self {
      scene: Scene::new(),
      history: History::default(),
      selection: BTreeSet::new(),
      primary: 0,
      hovered: 0,
      tool: ToolMode::Select,
      gizmo: GizmoState::default(),
      view: ViewSettings::default(),
      camera: CameraState::default(),
      project_path: String::new(),
      saved_revision: 0,
      clipboard: Clipboard::default(),
      edit_before: None,
      inspector_change: None,
      load_actors: false,
      library_root: String::new(),
      library: Arc::default(),
      library_jak2: Arc::default(),
      game_data: None,
      levels: levels::LevelJobs::default(),
      story: StoryView::default(),
      story_steps: Vec::new(),
      level_story_steps: BTreeMap::new(),
      story_level: String::new(),
      story_revision: u64::MAX,
      ocean: ocean::OceanState::default(),
      groups: groups::GroupsCache::default(),
      game_chunks: GameViewChunks::default(),
      weather: GameWeather::default(),
      jobs: BTreeMap::new(),
      notify: None,
      me: Weak::new(),
      scene_epoch: 0,
      document_epoch: 0,
      next_request: 1,
      replies: BTreeMap::new(),
      extraction: extract::Extraction::default(),
      placing: Default::default(),
      catalog: Default::default(),
      terrain: Default::default(),
      topology: Default::default(),
      preview: Default::default(),
      export: Default::default(),
      nav: Default::default(),
      gaps: None,
      write_prefs: true,
      remote_clients: 0,
      interface_ready: false,
    }
  }

  /// The editor of an application: its jobs run on worker threads that lock it to apply their
  /// result, `notify` gets its events (the log's lines too), the extraction's hook is wired.
  pub fn shared(mut self, notify: Option<Notify>) -> SharedEditor {
    if let Some(n) = &notify {
      let n = n.clone();
      Log::get().set_hook(Some(Box::new(move |line| {
        n("jle:log", serde_json::to_value(line).unwrap_or_default())
      })));
    }
    self.notify = notify;
    let shared = Arc::new(Mutex::new(self));
    let me = Arc::downgrade(&shared);
    {
      let mut ed = lock(&shared);
      ed.me = me.clone();
      ed.extraction.set_hook(move |e| extraction_event(&me, e));
    }
    shared
  }

  pub(crate) fn emit(&self, event: &str, payload: Value) {
    if let Some(n) = &self.notify {
      n(event, payload);
    }
  }

  /// Asks the interface something (jle:request): its answer comes as `frontend_reply {id, ...}`
  /// into `replies`. Returns the id.
  pub fn request(&mut self, mut payload: Value) -> u64 {
    let id = self.next_request;
    self.next_request += 1;
    payload["id"] = json!(id);
    self.emit("jle:request", payload);
    id
  }

  // ----------------------------------------------------------------------------------------------
  // jobs: long work on a worker thread, applied under the lock when done
  // ----------------------------------------------------------------------------------------------

  /// Runs `work` on a worker thread (shared editors), then `apply` with its result under the
  /// editor's lock, the interface told what changed. An editor on its own runs both now.
  pub(crate) fn run_job<R: Send + 'static>(
    &mut self,
    id: &str,
    label: String,
    work: impl FnOnce() -> R + Send + 'static,
    apply: impl FnOnce(&mut Editor, R) + Send + 'static,
  ) {
    let Some(me) = self.me.upgrade() else {
      let r = work();
      apply(self, r);
      return;
    };
    let job = Job {
      id: id.to_string(),
      label,
      done: 0,
      total: 1,
      finished: false,
    };
    self.emit("jle:progress", json!(job));
    self.jobs.insert(id.to_string(), job);
    let id = id.to_string();
    std::thread::spawn(move || {
      let r = work();
      let mut ed = lock(&me);
      let before = ed.watch();
      apply(&mut ed, r);
      if let Some(mut job) = ed.jobs.remove(&id) {
        job.done = job.total;
        job.finished = true;
        ed.emit("jle:progress", json!(job));
      }
      ed.after_change(&before);
    });
  }

  /// `dt` seconds later: the hour passing, a storm's lightning (the game window's); the plugin
  /// calls it on a timer. Whether the weather moves (the timer can sleep otherwise).
  pub fn tick(&mut self, dt: f32) -> bool {
    // the library's workers, the files the Place tool waits for (catalog.rs)
    self.poll_library_work();
    if !self.weather.time_passes && !self.weather.lightning {
      return false;
    }
    let before = self.watch();
    let on = self.view.game_view && self.document_open();
    self.weather.update(dt, on);
    self.after_change(&before);
    true
  }

  pub fn jobs(&self) -> impl Iterator<Item = &Job> {
    self.jobs.values()
  }

  pub fn busy(&self, id: &str) -> bool {
    self.jobs.contains_key(id)
  }

  // ----------------------------------------------------------------------------------------------
  // what changed: one jle:changed per call (or job) that changed something
  // ----------------------------------------------------------------------------------------------

  pub(crate) fn watch(&self) -> Watch {
    let mut sel = fnv(0, self.primary as u64);
    for &id in &self.selection {
      sel = fnv(sel, id as u64);
    }
    let mut materials = fnv(0, self.scene.materials.len() as u64);
    for m in &self.scene.materials {
      for b in m.name.bytes() {
        materials = fnv(materials, b as u64);
      }
      for v in [
        m.texture as u64,
        m.alpha as u64,
        m.blend as u64,
        m.cutoff.to_bits() as u64,
        m.double_sided as u64 | (m.translucent as u64) << 1 | (m.water as u64) << 2,
        m.base_color.x.to_bits() as u64 | (m.base_color.y.to_bits() as u64) << 32,
        m.base_color.z.to_bits() as u64 | (m.base_color.w.to_bits() as u64) << 32,
      ] {
        materials = fnv(materials, v);
      }
    }
    let mut textures = fnv(0, self.scene.textures.len() as u64);
    for t in &self.scene.textures {
      textures = fnv(textures, t.version);
    }
    let mut state = 0;
    for b in self.state_key().bytes() {
      state = fnv(state, b as u64);
    }
    let mut gameview = fnv(self.game_chunks.filter_serial(), self.view.game_view as u64);
    for b in serde_json::to_string(&self.weather)
      .unwrap_or_default()
      .bytes()
    {
      gameview = fnv(gameview, b as u64);
    }
    Watch {
      revision: self.history.revision(),
      scene: self.scene_epoch,
      document: self.document_epoch,
      selection: sel,
      materials,
      textures,
      state,
      gameview,
    }
  }

  /// After a change: the story shown again on the nodes the history brought (the C++ frame loop
  /// did it each frame the revision moved), then jle:changed with what changed since `before`.
  pub(crate) fn after_change(&mut self, before: &Watch) {
    if self.history.revision() != self.story_revision && !self.scene.nodes.is_empty() {
      self.apply_story_view();
    }
    let after = self.watch();
    if after == *before {
      return;
    }
    let document = after.document != before.document;
    let scene = document || after.revision != before.revision || after.scene != before.scene;
    let selection = document || after.selection != before.selection;
    let materials = document || after.materials != before.materials;
    let textures = document || after.textures != before.textures;
    let gameview = after.gameview != before.gameview;
    let state = scene || selection || materials || after.state != before.state || gameview;
    self.emit(
      "jle:changed",
      json!({
        "revision": after.revision,
        "scene": scene,
        "selection": selection,
        "state": state,
        "document": document,
        "materials": materials,
        "textures": textures,
        "gameview": gameview,
      }),
    );
  }

  /// The scene changed outside the history (the next scene fetch looks at every node).
  pub(crate) fn scene_changed(&mut self) {
    self.scene_epoch += 1;
  }

  // ----------------------------------------------------------------------------------------------
  // the library and the game's data
  // ----------------------------------------------------------------------------------------------

  /// The game's data, read once; None when missing.
  pub fn game_data(&mut self) -> Option<Arc<GameData>> {
    if let Some(d) = &self.game_data {
      return d.clone();
    }
    let data = match GameData::load(&game_data_path()) {
      Ok(d) => Some(Arc::new(d)),
      Err(e) => {
        log_warn!(tr("log.game_data_missing"), e);
        None
      }
    };
    self.game_data = Some(data.clone());
    data
  }

  /// The game's data when a document is open.
  pub fn document_game_data(&mut self) -> Option<Arc<GameData>> {
    if self.document_open() {
      self.game_data()
    } else {
      None
    }
  }

  /// The library folder (the extracted game in its jak3/, Jak II's in its jak2/), kept in the
  /// preferences.
  pub fn set_library(&mut self, root: &str) {
    self.library_root = if root.is_empty() {
      String::new()
    } else {
      library_root_of(root)
    };
    let data = self.game_data();
    let mut library = AssetLibrary::default();
    self.export.originals.clear();
    let folder = game_folder(&self.library_root);
    if !folder.is_empty() {
      match library.scan(&folder) {
        Ok(()) => library.set_listed(|l| listed(l, data.as_deref())),
        Err(e) => log_error!("%s", e),
      }
    }
    self.library = Arc::new(library);
    // Jak II's structures, extracted to import them (<library>/jak2): the levels the game has, none
    // of its actors (Jak 3's game data says nothing of them)
    let mut jak2 = AssetLibrary::default();
    let folder = format!("{}/jak2", self.library_root);
    if !self.library_root.is_empty()
      && std::path::Path::new(&folder).join("library.json").is_file()
      && jak2.scan(&folder).is_ok()
    {
      jak2.set_listed(|l| listed(l, None));
    }
    self.library_jak2 = Arc::new(jak2);
    // the catalog, its index, the palette of materials, the prefabs (catalog.rs)
    self.library_changed();
    let root = self.library_root.clone();
    self.save_prefs(|p| p.library_root = root);
  }

  /// The interface's language (and the log's, the assistant's answers), kept in the preferences.
  pub fn set_language(&mut self, code: &str) -> bool {
    if !i18n_select(code) {
      return false;
    }
    self.save_prefs(|p| p.language = code.to_string());
    true
  }

  /// A change of the preferences, kept (unless `write_prefs` is off: the tests)
  pub(crate) fn save_prefs(&self, edit: impl FnOnce(&mut Prefs)) {
    let mut prefs = Prefs::get();
    edit(&mut prefs);
    if self.write_prefs {
      prefs.save();
    }
  }

  /// A level or a new one is being edited (a new level has no node yet)
  pub fn document_open(&self) -> bool {
    !self.scene.nodes.is_empty() || !self.scene.settings.level.is_empty()
  }
}

fn listed(l: &LevelEntry, data: Option<&GameData>) -> bool {
  // -1: not known (an older extraction)
  let collision = usize::try_from(l.collision_triangles).unwrap_or(usize::MAX);
  level_listed(&l.name, &l.level, collision, data)
}

/// A text of the extraction in the editor's language: its key's translation with its numbers,
/// then its detail; as ogle-extract said it when there is no such key.
pub(crate) fn text_string(t: &extract::Text) -> String {
  let fmt = tr(&t.key);
  let mut s = if fmt == t.key {
    t.text.clone()
  } else {
    let args: Vec<crate::i18n::Arg> = t.args.iter().map(|&a| a.into()).collect();
    crate::i18n::strf(fmt, &args)
  };
  if !t.detail.is_empty() {
    s = format!("{s}: {}", t.detail);
  }
  s
}

fn fnv(h: u64, v: u64) -> u64 {
  (h ^ v).wrapping_mul(1099511628211)
}

/// The extraction's events (its reader thread, no lock held): its progress, then the library it
/// wrote (extract_poll).
fn extraction_event(me: &Weak<Mutex<Editor>>, e: &extract::ExtractionEvent) {
  let Some(me) = me.upgrade() else { return };
  let mut ed = lock(&me);
  match e {
    extract::ExtractionEvent::Line {
      detecting,
      done,
      total,
      current,
    } => {
      if !detecting {
        let job = Job {
          id: "extract".into(),
          label: current.clone(),
          done: *done,
          total: (*total).max(1),
          finished: false,
        };
        ed.emit("jle:progress", json!(job));
      }
    }
    extract::ExtractionEvent::Ended {
      detecting,
      finished,
      error,
      output,
      seconds,
    } => {
      if *detecting {
        ed.emit("jle:changed", json!({"state": true}));
        return;
      }
      ed.emit(
        "jle:progress",
        json!({"id": "extract", "label": "", "done": 1, "total": 1, "finished": true}),
      );
      if !finished {
        if let Some(t) = error {
          log_error!("%s", text_string(t));
        }
        ed.emit("jle:changed", json!({"state": true}));
        return;
      }
      // the Levels panel shows what was written (<library>/jak3)
      log_info!(tr("log.extracted"), extract::time_text(*seconds));
      let before = ed.watch();
      ed.set_library(output);
      ed.after_change(&before);
      ed.emit("jle:changed", json!({"state": true}));
    }
  }
}

/// `strf!` of a translation key, for the errors the interface shows
pub(crate) fn tr_err(key: &str, arg: &str) -> String {
  strf!(tr(key), arg)
}
