//! What the panels list of the library (actions.cpp set_library, rebuild_catalog,
//! apply_library_index, refresh_prefabs, start_game_prefabs; panels.cpp ui_catalog_*,
//! ui_prefabs_panel; surface_panel.cpp ui_texture_browser): the catalog's objects and models, the
//! index of every level read on a worker thread (the decor of all the levels, the textures of the
//! games), the palette of materials, the user's prefabs and the game's (made from the extracted
//! levels on a worker thread). The asset store loads the models and decor the Place tool and the
//! panels show. The plugin's timer polls the workers (`poll_library_work`).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use serde_json::{Value, json};

use super::levels::open_level;
use super::{Editor, Job, ToolMode};
use crate::i18n::tr;
use crate::io::asset_store::AssetStore;
use crate::io::catalog::{
  CatalogModel, CatalogObject, CatalogPart, build_catalog, effect_kind, jak2_creatures,
  list_catalog_models, list_parts,
};
use crate::io::jak2_import::jak2_structure;
use crate::io::library_index::{DecorEntry, LibraryIndex, all_decor};
use crate::io::prefab_recipes::{
  PrefabRecipe, game_prefab_infos, load_prefab_recipes, prefab_recipes_path, start_game_prefabs,
};
use crate::io::prefabs::{PrefabInfo, list_prefabs};
use crate::io::texture_palette::{
  PaletteTexture, TexturePalette, all_textures, read_level_textures, recurring_textures,
};
use crate::io::user_models::{MY_MODELS, list_user_models};
use crate::prefs::{Prefs, READ_ONLY};
use crate::scene::shapes::add_texture_once;
use crate::scene::{NodeKind, Scene};
use crate::strf;

/// The library's lists and workers (app.h: store, catalog, catalog_models, library_index,
/// all_parts, palette, prefabs, game_prefabs, m_game_prefab_work...)
#[derive(Default)]
pub(crate) struct CatalogState {
  /// models, prefabs and levels' decor read on worker threads (the preview, a level's decor)
  pub store: AssetStore,
  /// the objects of the game
  pub objects: Vec<CatalogObject>,
  /// its other models (props)
  pub models: Vec<CatalogModel>,
  /// Jak II's models, placed as decor
  pub models_jak2: Vec<CatalogModel>,
  /// what the catalog reads of every level on a worker thread: their decor, the art-names of their
  /// actors (all the variants and effects), their textures; merged into the catalog once ready
  pub index: LibraryIndex,
  pub index_jak2: LibraryIndex,
  index_applied: bool,
  index_jak2_applied: bool,
  /// the decor of every level (Jak II's: its structures)
  pub all_parts: Vec<DecorEntry>,
  pub all_parts_jak2: Vec<DecorEntry>,
  /// the palette of materials: the textures Jak 3's levels use again and again, their images
  pub palette: TexturePalette,
  pub palette_textures: Vec<PaletteTexture>,
  /// every texture of the games' levels, once their index is ready
  pub game_textures: Vec<PaletteTexture>,
  pub game_textures_jak2: Vec<PaletteTexture>,
  /// the user's prefabs, the game's (made or not yet)
  pub prefabs: Vec<PrefabInfo>,
  pub recipes: Vec<PrefabRecipe>,
  pub game_prefabs: Vec<PrefabInfo>,
  game_job: Option<JoinHandle<usize>>,
  game_total: usize,
  game_done: Arc<AtomicUsize>,
  /// counts the builds finished (the remote control waits on it)
  pub game_prefab_runs: u64,
  /// the levels whose prefabs were wanted (shown, placed): made as soon as possible
  levels_wanted: BTreeSet<(String, String)>,
  /// clicked before it was made: placed once it is
  pub(crate) place_when_made: String,
  /// the decor of the level the Decor tab shows: its background, the level, its parts
  decor: Option<(String, Arc<Scene>, Vec<CatalogPart>)>,
  /// bumped when what the panels list changes (get_state.catalog.epoch)
  pub epoch: u64,
  /// the store's loads at the last poll
  pending: usize,
  /// the indexes' progress last told (Jak 3's, Jak II's)
  index_done: [usize; 2],
}

fn lower(s: &str) -> String {
  s.to_lowercase()
}

fn prefab_json(p: &PrefabInfo) -> Value {
  json!({
    "name": p.name, "path": p.path, "mtime": p.mtime, "category": p.category,
    "description": p.description, "game": p.game, "level": p.level, "made": p.made,
  })
}

/// offset / limit of a list (`catalog`, `texture_list`)
fn page<T>(a: &Value, list: Vec<T>) -> Vec<T> {
  let offset = a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
  let limit = a
    .get("limit")
    .and_then(Value::as_u64)
    .map_or(usize::MAX, |l| l as usize);
  list.into_iter().skip(offset).take(limit).collect()
}

impl Editor {
  /// The library changed (set_library): the store forgotten, the index read again, the catalog,
  /// the palette and the prefabs made again.
  pub(crate) fn library_changed(&mut self) {
    // the index's cache is the editor's (not in the tests: their libraries are temporary)
    let (cache, write) = if self.write_prefs {
      (Prefs::dir() + "index/", !READ_ONLY.load(Ordering::Relaxed))
    } else {
      (String::new(), false)
    };
    let dir = |game: &str| {
      if cache.is_empty() {
        String::new()
      } else {
        format!("{cache}{game}")
      }
    };
    let c = &mut self.catalog;
    c.store.clear();
    c.index.start(&self.library, &dir("jak3"), write);
    c.index_applied = false;
    c.index_done = [usize::MAX; 2];
    c.palette.clear();
    c.palette_textures.clear();
    c.game_textures.clear();
    c.game_textures_jak2.clear();
    c.index_jak2.start(&self.library_jak2, &dir("jak2"), write);
    c.index_jak2_applied = false;
    c.all_parts_jak2.clear();
    c.decor = None;
    self.placing.styles.clear();
    self.rebuild_catalog();
    self.refresh_prefabs();
  }

  /// catalog, catalog_models and all_parts
  fn rebuild_catalog(&mut self) {
    let data = self.game_data();
    let c = &mut self.catalog;
    c.objects.clear();
    c.models.clear();
    c.all_parts.clear();
    c.epoch += 1;
    if self.library.is_empty() {
      return;
    }
    let idx = c.index.ready().then_some(&c.index);
    c.objects = build_catalog(&self.library, data.as_deref(), idx);
    // the user's own models first (My models), then the game's
    c.models = list_user_models(&self.library.models_folder());
    c.models.extend(list_catalog_models(
      &self.library,
      data.as_deref(),
      &c.objects,
      &BTreeSet::new(),
    ));
    // Jak II's: its models alone (its objects' code is not in Jak 3), placed as decor, but its
    // living things' (data/jak2/creatures.json, from its type table)
    c.models_jak2.clear();
    if !self.library_jak2.is_empty() {
      c.models_jak2 = list_catalog_models(&self.library_jak2, None, &[], &jak2_creatures());
    }
    for m in &mut c.models_jak2 {
      m.game = "jak2".into();
    }
    if let Some(idx) = idx {
      c.all_parts = all_decor(idx, &self.library);
    }
  }

  /// The user's models listed again (one was imported): first in the catalog's Models
  pub(crate) fn refresh_user_models(&mut self) {
    let c = &mut self.catalog;
    c.models.retain(|m| m.game != "user");
    let mut models = list_user_models(&self.library.models_folder());
    models.append(&mut c.models);
    c.models = models;
    c.epoch += 1;
  }

  /// the index, once it just became ready
  fn apply_library_index(&mut self) {
    let c = &mut self.catalog;
    if !c.index_jak2_applied && c.index_jak2.ready() {
      c.index_jak2_applied = true;
      c.all_parts_jak2 = all_decor(&c.index_jak2, &self.library_jak2);
      c.all_parts_jak2.retain(|e| jak2_structure(&e.part));
      c.game_textures_jak2 = all_textures(&c.index_jak2, &self.library_jak2, "jak2");
      c.epoch += 1;
    }
    if c.index_applied || !c.index.ready() {
      return;
    }
    c.index_applied = true;
    self.rebuild_catalog();
    let c = &mut self.catalog;
    c.palette_textures = recurring_textures(&c.index, &self.library, 600);
    c.game_textures = all_textures(&c.index, &self.library, "");
  }

  /// Waits for the index of the library and merges it (the tests, the remote control)
  pub fn wait_library_index(&mut self) {
    self.catalog.index.wait();
    self.catalog.index_jak2.wait();
    self.apply_library_index();
  }

  /// the index of the library (Jak II's) once ready (the terrain's level textures)
  pub fn library_index(&self, jak2: bool) -> &LibraryIndex {
    if jak2 {
      &self.catalog.index_jak2
    } else {
      &self.catalog.index
    }
  }

  /// A texture of the palette made a texture of the document (-1 while its image is read; `wait`:
  /// read now); Jak II's named "jak2-<name>"
  pub fn palette_texture(&mut self, t: &PaletteTexture, wait: bool) -> i32 {
    let mut img = self.catalog.palette.image(t).cloned();
    if img.is_none() && wait {
      img = read_level_textures(&t.background, std::slice::from_ref(&t.name))
        .ok()
        .and_then(|v| v.into_iter().next());
    }
    let Some(mut img) = img else { return -1 };
    if !t.game.is_empty() {
      // named apart from Jak 3's of the same name (as io/jak2_import names them)
      img.name = format!("{}-{}", t.game, img.name);
    }
    add_texture_once(&mut self.scene, img)
  }

  // ----------------------------------------------------------------------------------------------
  // prefabs
  // ----------------------------------------------------------------------------------------------

  pub fn refresh_prefabs(&mut self) {
    let folder = self.prefab_folder();
    let c = &mut self.catalog;
    c.prefabs = if folder.is_empty() {
      Vec::new()
    } else {
      list_prefabs(&folder)
    };
    // the game's prefabs made so far, in the order of their recipes (Jak II's when it was
    // extracted)
    c.recipes = load_prefab_recipes(&prefab_recipes_path(""), "").unwrap_or_default();
    if !self.library_jak2.is_empty() {
      c.recipes
        .extend(load_prefab_recipes(&prefab_recipes_path("jak2"), "jak2").unwrap_or_default());
    }
    c.game_prefabs = if folder.is_empty() {
      Vec::new()
    } else {
      game_prefab_infos(&c.recipes, &folder)
    };
    c.epoch += 1;
  }

  /// the prefabs of that level are wanted (shown in the panel, placed): made as soon as possible
  pub fn want_game_prefabs(&mut self, game: &str, level: &str) {
    self
      .catalog
      .levels_wanted
      .insert((game.into(), level.into()));
  }

  pub fn game_prefabs_busy(&self) -> bool {
    self.catalog.game_job.is_some()
  }

  /// Makes the game's prefabs not made yet (all of them again: `rebuild`), or only those of
  /// `levels` (game, level), on a worker thread: each level opened once, without its actors.
  pub fn start_game_prefabs(&mut self, rebuild: bool, levels: &BTreeSet<(String, String)>) {
    let folder = self.prefab_folder();
    if self.catalog.game_job.is_some() || self.library.is_empty() || folder.is_empty() {
      return;
    }
    self.refresh_prefabs();
    let data = self.game_data();
    let (lib, lib2, d) = (
      self.library.clone(),
      self.library_jak2.clone(),
      data.clone(),
    );
    let open = move |game: &str, name: &str| -> Result<Scene, String> {
      let jak2 = game == "jak2";
      let lib = if jak2 { &lib2 } else { &lib };
      // a level missing: no error text of its own (the worker logs the recipe's level)
      let entry = lib.level(name).ok_or_else(String::new)?;
      let mut level = Scene::new();
      open_level(
        &mut level,
        lib,
        if jak2 { None } else { d.as_deref() },
        entry,
        false,
      )?;
      Ok(level)
    };
    let c = &mut self.catalog;
    let done = c.game_done.clone();
    if let Some((work, total)) =
      start_game_prefabs(&c.recipes, &folder, rebuild, levels, data, done, open)
    {
      c.game_job = Some(work);
      c.game_total = total;
      self.game_prefabs_progress(false);
    }
  }

  fn game_prefabs_progress(&mut self, finished: bool) {
    let c = &self.catalog;
    let done = c.game_done.load(Ordering::Relaxed);
    let job = Job {
      id: "game_prefabs".into(),
      label: strf!(tr("prefabs.game.making"), done, c.game_total),
      done: if finished { c.game_total } else { done },
      total: c.game_total.max(1),
      finished,
    };
    self.emit("jle:progress", json!(job));
    if finished {
      self.jobs.remove(&job.id);
    } else {
      self.jobs.insert(job.id.clone(), job);
    }
  }

  fn poll_game_prefabs(&mut self) {
    let Some(work) = &self.catalog.game_job else {
      // the levels whose prefabs were wanted meanwhile, all at once
      if !self.catalog.levels_wanted.is_empty() {
        let levels = std::mem::take(&mut self.catalog.levels_wanted);
        self.start_game_prefabs(false, &levels);
        if self.catalog.game_job.is_none() {
          self.catalog.game_prefab_runs += 1; // nothing to make: they are there
        }
      }
      return;
    };
    if !work.is_finished() {
      let done = self.catalog.game_done.load(Ordering::Relaxed);
      if self.jobs.get("game_prefabs").is_none_or(|j| j.done != done) {
        self.game_prefabs_progress(false);
      }
      return;
    }
    // (the worker logged how many it made)
    let _ = self.catalog.game_job.take().unwrap().join();
    self.catalog.game_prefab_runs += 1;
    self.game_prefabs_progress(true);
    self.refresh_prefabs();
    // clicked before it was made: placed now
    let wanted = self.catalog.place_when_made.clone();
    if !wanted.is_empty()
      && let Some(p) = self
        .catalog
        .game_prefabs
        .iter()
        .find(|p| p.name == wanted && p.made)
        .cloned()
    {
      self.start_placement(&p.path, &p.name, false);
      self.catalog.place_when_made.clear();
    }
  }

  /// Waits for the game's prefabs being made (the tests, the remote control)
  pub fn wait_game_prefabs(&mut self) {
    self.poll_game_prefabs();
    if let Some(work) = self.catalog.game_job.take() {
      while !work.is_finished() {
        std::thread::sleep(std::time::Duration::from_millis(20));
      }
      self.catalog.game_job = Some(work);
      self.poll_game_prefabs();
    }
  }

  // ----------------------------------------------------------------------------------------------
  // the workers, polled
  // ----------------------------------------------------------------------------------------------

  /// The plugin's timer (Editor::tick): the store's loads, the palette's images, the index, the
  /// game's prefabs; the preview merged once its file is read, the style level measured. Nothing
  /// to do: nothing looked at.
  pub(crate) fn poll_library_work(&mut self) {
    let c = &self.catalog;
    let busy = !c.index_applied
      || !c.index_jak2_applied
      || c.store.pending() > 0
      || c.palette.busy()
      || c.game_job.is_some()
      || !c.levels_wanted.is_empty()
      || self.placement_waiting()
      || self.style_waiting();
    if !busy {
      return;
    }
    let before = self.watch();
    self.catalog.store.update();
    self.catalog.palette.update();
    let ready = self.catalog.index.update() | self.catalog.index_jak2.update();
    if ready || (!self.catalog.index_jak2_applied && self.catalog.index_jak2.ready()) {
      self.apply_library_index();
    }
    self.index_progress();
    self.poll_game_prefabs();
    if self.tool == ToolMode::Place {
      self.placement_mesh();
    }
    let level = self.placing.style_level.clone();
    if self.style_waiting() {
      self.level_style(&level);
    }
    // a level's decor read (the Decor tab, a style): the panels look again
    let pending = self.catalog.store.pending();
    if pending != self.catalog.pending {
      self.catalog.pending = pending;
      self.catalog.epoch += 1;
    }
    self.after_change(&before);
  }

  // the indexes' progress (the status bar, the Decor and Materials tabs): Jak 3's, then Jak II's
  // (job library_index_jak2)
  fn index_progress(&mut self) {
    for jak2 in [false, true] {
      let index = self.library_index(jak2);
      let (ready, total) = (index.ready(), index.total());
      let done = if ready { total } else { index.done() };
      let told = &mut self.catalog.index_done[jak2 as usize];
      if done == *told {
        continue;
      }
      *told = done;
      let job = Job {
        id: ["library_index", "library_index_jak2"][jak2 as usize].into(),
        label: strf!(tr("catalog.indexing"), done, total),
        done,
        total: total.max(1),
        finished: ready,
      };
      self.emit("jle:progress", json!(job));
      if ready {
        self.jobs.remove(&job.id);
      } else {
        self.jobs.insert(job.id.clone(), job);
      }
    }
  }

  /// The decor of a level of the library (its background file): the Decor tab's when it is that
  /// one, else the store's (None while it loads)
  pub(crate) fn decor_scene_of(&mut self, background: &str) -> Option<Arc<Scene>> {
    if let Some((bg, s, _)) = &self.catalog.decor
      && bg == background
    {
      return Some(s.clone());
    }
    self.catalog.store.level(background, true)
  }

  // ----------------------------------------------------------------------------------------------
  // what the panels list
  // ----------------------------------------------------------------------------------------------

  /// get_state.catalog: {epoch (what the panels list changed), ready (the index merged), objects,
  /// jak2 (Jak II extracted)}
  pub(crate) fn catalog_state(&self) -> Value {
    let c = &self.catalog;
    json!({
      "epoch": c.epoch,
      "ready": c.index_applied,
      "ready_jak2": c.index_jak2_applied,
      "objects": c.objects.len(),
      "prefabs_busy": c.game_job.is_some(),
      "style_level": self.placing.style_level,
    })
  }

  /// The catalog's calls (api.rs): None when `name` is not one of them.
  pub(crate) fn catalog_call(&mut self, name: &str, a: &Value) -> Option<Result<Value, String>> {
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or_default();
    Some(match name {
      "catalog" => Ok(self.catalog_list(a)),
      "prefabs" => Ok(self.prefabs_list()),
      "want_game_prefabs" => {
        let levels: Vec<(String, String)> = a
          .get("levels")
          .and_then(|l| serde_json::from_value(l.clone()).ok())
          .unwrap_or_else(|| vec![(s("game").into(), s("level").into())]);
        for (game, level) in levels {
          if !level.is_empty() {
            self.want_game_prefabs(&game, &level);
          }
        }
        Ok(Value::Null)
      }
      "game_prefabs" => {
        let rebuild = a.get("rebuild").and_then(Value::as_bool).unwrap_or(false);
        self.start_game_prefabs(rebuild, &BTreeSet::new());
        Ok(json!({"busy": self.game_prefabs_busy(), "total": self.catalog.game_total}))
      }
      "texture_list" => Ok(self.texture_list(a)),
      "selection_meshes" => {
        let actors = a.get("actors").and_then(Value::as_bool).unwrap_or(false);
        Ok(json!(self.selection_meshes(actors)))
      }
      "palette_texture" => {
        let t = PaletteTexture {
          name: s("name").into(),
          game: s("game").into(),
          background: s("background").into(),
          ..Default::default()
        };
        Ok(json!({"texture": self.palette_texture(&t, true)}))
      }
      _ => return None,
    })
  }

  /// `catalog {tab, jak2?, filter?, family?, level?, offset?, limit?}`: the Catalog panel's lists
  /// (ui_catalog_objects, ui_catalog_models, ui_catalog_decor, ui_catalog_decor_all).
  ///   objects | interactive | effects: {total, count, families: [key], family, items:
  ///     [{etype, name, family, category, model, model_path, art, count, levels}], empty}
  ///   models: {total, count, levels: [group], level, items: [{name, path, group, game}]}
  ///   decor (level ""): {indexing: {done, total}} | {total, count, levels, items: [{level,
  ///     background, stamp, part: {kind, name, mesh_node, triangles, copies}, levels}]}
  ///   decor (a level): {level, loading | failed, total, count, levels, items}
  fn catalog_list(&mut self, a: &Value) -> Value {
    let s = |k: &str| {
      a.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
    };
    let jak2 =
      a.get("jak2").and_then(Value::as_bool).unwrap_or(false) && !self.library_jak2.is_empty();
    let f = lower(&s("filter"));
    let tab = s("tab");
    match tab.as_str() {
      "objects" | "interactive" | "effects" => {
        // the objects that make up a place, the interactive ones, the particle effects (smoke,
        // fire, lights...), each kind in its own category
        let (interactive, effects) = (tab == "interactive", tab == "effects");
        let in_tab = |o: &CatalogObject| {
          if effects {
            o.family == "effect"
          } else {
            o.interactive == interactive && o.family != "effect"
          }
        };
        let category = |o: &CatalogObject| {
          if effects {
            format!("effect.{}", effect_kind(o))
          } else {
            format!("family.{}", o.family)
          }
        };
        let objects = &self.catalog.objects;
        let mut families: Vec<String> = Vec::new();
        for o in objects.iter().filter(|o| in_tab(o)) {
          let c = category(o);
          if !families.contains(&c) {
            families.push(c);
          }
        }
        if effects {
          families.sort();
        }
        let mut chosen = s("family");
        if !families.contains(&chosen) {
          chosen.clear();
        }
        let mut total = 0;
        let mut shown = Vec::new();
        for o in objects.iter().filter(|o| in_tab(o)) {
          total += 1;
          if (!chosen.is_empty() && category(o) != chosen)
            || (!f.is_empty()
              && !o.name.contains(&f)
              && !o.etype.contains(&f)
              && !o.model.contains(&f))
          {
            continue;
          }
          let art = o
            .lump
            .get("art-name")
            .and_then(Value::as_str)
            .unwrap_or_default();
          shown.push(json!({
            "etype": o.etype, "name": o.name, "family": o.family, "category": category(o),
            "model": o.model, "model_path": o.model_path, "art": art.trim_start_matches('\''),
            "count": o.count, "levels": o.levels,
          }));
        }
        json!({
          "total": total, "count": shown.len(), "families": families, "family": chosen,
          "empty": objects.is_empty(), "items": page(a, shown),
        })
      }
      "models" => {
        let models = if jak2 {
          &self.catalog.models_jak2
        } else {
          &self.catalog.models
        };
        // the level they come from, or every level
        let mut levels: Vec<&str> = models.iter().map(|m| m.group.as_str()).collect();
        levels.sort();
        levels.dedup();
        // the user's own first
        if let Some(at) = levels.iter().position(|&l| l == MY_MODELS) {
          levels.remove(at);
          levels.insert(0, MY_MODELS);
        }
        let mut level = s("level");
        if !levels.contains(&level.as_str()) {
          level.clear();
        }
        let shown: Vec<&CatalogModel> = models
          .iter()
          .filter(|m| {
            (level.is_empty() || m.group == level)
              && (f.is_empty() || m.name.contains(&f) || m.group.contains(&f))
          })
          .collect();
        json!({
          "total": models.len(), "count": shown.len(), "levels": levels, "level": level,
          "items": page(a, shown),
        })
      }
      "decor" => self.decor_list(a, jak2, &s("level"), &f),
      _ => json!({"error": "tab: objects, interactive, effects, models or decor"}),
    }
  }

  fn decor_list(&mut self, a: &Value, jak2: bool, level: &str, f: &str) -> Value {
    let lib = if jak2 {
      self.library_jak2.clone()
    } else {
      self.library.clone()
    };
    let levels: Vec<&str> = lib
      .levels()
      .iter()
      .filter(|l| l.listed)
      .map(|l| l.name.as_str())
      .collect();
    let part_json = |level: &str, background: &str, stamp: i64, p: &CatalogPart, n: i32| {
      json!({
        "level": level, "background": background, "stamp": stamp, "levels": n,
        "part": {"kind": p.kind, "name": p.name, "mesh_node": p.mesh_node, "triangles": p.triangles, "copies": p.copies},
      })
    };
    let Some(entry) = lib.level(level) else {
      // every part of every level, searched by name (the levels are read once, then from the
      // cache)
      let (index, parts) = if jak2 {
        (&self.catalog.index_jak2, &self.catalog.all_parts_jak2)
      } else {
        (&self.catalog.index, &self.catalog.all_parts)
      };
      if !index.ready()
        || !(if jak2 {
          self.catalog.index_jak2_applied
        } else {
          self.catalog.index_applied
        })
      {
        return json!({
          "levels": levels, "level": "", "indexing": {"done": index.done(), "total": index.total()},
        });
      }
      let shown: Vec<Value> = parts
        .iter()
        .filter(|e| e.key.contains(f))
        .map(|e| part_json(&e.level, &e.background, e.stamp, &e.part, e.levels))
        .collect();
      return json!({
        "levels": levels, "level": "", "total": parts.len(), "count": shown.len(),
        "items": page(a, shown),
      });
    };
    let (bg, stamp, name) = (entry.background.clone(), entry.mtime, entry.name.clone());
    if !self.catalog.decor.as_ref().is_some_and(|d| d.0 == bg) {
      let Some(scene) = self.catalog.store.level(&bg, true) else {
        let failed = self.catalog.store.failed_level(&bg);
        return json!({"levels": levels, "level": name, "loading": !failed, "failed": failed});
      };
      let mut parts = list_parts(&scene);
      if jak2 {
        parts.retain(jak2_structure);
      }
      self.catalog.decor = Some((bg.clone(), scene, parts));
    }
    let parts = &self.catalog.decor.as_ref().unwrap().2;
    let shown: Vec<Value> = parts
      .iter()
      .filter(|p| lower(&p.name).contains(f))
      .map(|p| part_json(&name, &bg, stamp, p, 1))
      .collect();
    json!({
      "levels": levels, "level": name, "total": parts.len(), "count": shown.len(),
      "items": page(a, shown),
    })
  }

  /// `prefabs`: the Prefabs panel's (ui_prefabs_panel): {folder, mine: [prefab], game: [prefab],
  /// busy, done, total, jak2, waiting}; a prefab {name, path, mtime, category, description, game,
  /// level, made}
  fn prefabs_list(&self) -> Value {
    let c = &self.catalog;
    json!({
      "folder": self.prefab_folder(),
      "mine": c.prefabs.iter().map(prefab_json).collect::<Vec<_>>(),
      "game": c.game_prefabs.iter().map(prefab_json).collect::<Vec<_>>(),
      "busy": c.game_job.is_some(),
      "done": c.game_done.load(Ordering::Relaxed),
      "total": c.game_total,
      "jak2": !self.library_jak2.is_empty(),
      "library": !self.library.is_empty(),
      "waiting": c.place_when_made,
    })
  }

  /// `texture_list {game?, level?, filter?, offset?, limit?}` (ui_texture_browser): the textures of
  /// a game ("" Jak 3, "jak2"), of a level of it ("" every level, "*" the document's), whose name
  /// has `filter`: {game, level, ready, indexing?: {done, total}, levels: [{level, count}],
  /// total, count, items: [{name, game, background, levels, uses}] | [{index, name, width,
  /// height}] (the document's)}
  fn texture_list(&self, a: &Value) -> Value {
    let s = |k: &str| {
      a.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
    };
    let jak2 = s("game") == "jak2" && !self.library_jak2.is_empty();
    let game = if jak2 { "jak2" } else { "" };
    let c = &self.catalog;
    let index = if jak2 { &c.index_jak2 } else { &c.index };
    let f = lower(&s("filter"));
    let mut level = s("level");
    // a level of the other game chosen before: every level
    if !level.is_empty()
      && level != "*"
      && index.ready()
      && !index.levels().iter().any(|l| l.level == level)
    {
      level.clear();
    }
    let levels: Vec<Value> = index
      .levels()
      .iter()
      .filter(|l| !l.textures.is_empty())
      .map(|l| json!({"level": l.level, "count": l.textures.len()}))
      .collect();
    let mut j = json!({"game": game, "level": level, "ready": index.ready(), "levels": levels});
    if level == "*" {
      let shown: Vec<Value> = self
        .scene
        .textures
        .iter()
        .enumerate()
        .filter(|(_, t)| f.is_empty() || lower(&t.name).contains(&f))
        .map(|(i, t)| json!({"index": i, "name": t.name, "width": t.width, "height": t.height}))
        .collect();
      j["total"] = json!(self.scene.textures.len());
      j["count"] = json!(shown.len());
      j["items"] = json!(page(a, shown));
      return j;
    }
    let ready = index.ready()
      && if jak2 {
        c.index_jak2_applied
      } else {
        c.index_applied
      };
    if !ready {
      j["indexing"] = json!({"done": index.done(), "total": index.total()});
      return j;
    }
    let of_level: Vec<PaletteTexture>;
    let list = if level.is_empty() {
      if jak2 {
        &c.game_textures_jak2
      } else {
        &c.game_textures
      }
    } else {
      // the textures of the level chosen, the most drawn first
      let mut v: Vec<PaletteTexture> = index
        .levels()
        .iter()
        .filter(|li| li.level == level)
        .flat_map(|li| {
          li.textures
            .iter()
            .filter(|(name, _)| !name.starts_with("anim-slot"))
            .map(|(name, &n)| PaletteTexture {
              name: name.clone(),
              game: game.into(),
              background: li.background.clone(),
              levels: 1,
              uses: n,
              ..Default::default()
            })
        })
        .collect();
      v.sort_by(|a, b| (b.uses, &a.name).cmp(&(a.uses, &b.name)));
      of_level = v;
      &of_level
    };
    let shown: Vec<&PaletteTexture> = list
      .iter()
      .filter(|t| f.is_empty() || t.name.contains(&f))
      .collect();
    j["total"] = json!(list.len());
    j["count"] = json!(shown.len());
    j["items"] = json!(page(a, shown));
    j
  }

  /// the render nodes of the selection a texture of the browser goes to: the Materials tab's (not
  /// an actor's model, unless `actors`: the Build panel's)
  pub fn selection_meshes(&self, actors: bool) -> Vec<u32> {
    let roots = self.selection_roots();
    let below = self.scene.descendants_of(&roots);
    roots
      .iter()
      .zip(below)
      .flat_map(|(&r, b)| std::iter::once(r).chain(b))
      .filter(|&id| {
        self.scene.find(id).is_some_and(|n| {
          n.kind == NodeKind::Render && n.mesh.is_some() && (actors || self.scene.actor_of(n) == 0)
        })
      })
      .collect()
  }
}
