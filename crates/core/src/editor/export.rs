//! What goes to the game (actions.cpp, building_tools.cpp): the export of the level for OpenGOAL's
//! level builder (io::game_export), its nav meshes generated on a worker (navmesh) and written for
//! the game (io::nav_export), the holes through which the player would see the void (scene::gaps).
//!
//! Calls (api.rs): export_defaults, export_check, export_start_point, check_continues,
//! export_for_game, export_state; generate_navmesh, set_navmesh, navmesh_state, navmesh_data,
//! export_navmeshes; find_gaps, gaps_report.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use super::Editor;
use super::levels::make_jak2_level;
use super::project::project_folder_name;
use crate::i18n::tr;
use crate::io::game_export::{
  LevelExportOptions, LevelExportReport, check_continues, check_export_names, collision_levels,
  default_export_options, export_level, export_start_point, read_original,
};
use crate::io::nav_export::export_navmeshes;
use crate::math::{AABB, Vec3};
use crate::navmesh::{
  NavBuildStats, NavMeshData, NavParams, build_navmeshes, navmesh_nodes, navmesh_of_node,
  navmesh_problem, navmesh_surfaces, replace_navmeshes, world_navmeshes,
};
use crate::scene::gaps::{GapOptions, GapReport, find_gaps};
use crate::scene::{NodeKind, Scene};
use crate::{log_error, log_info, log_warn};

/// the ids the exported nav meshes take by default (App::nav_first_aid)
pub const K_NAV_FIRST_AID: u32 = 60000;

/// An export done on a worker: its report or why it failed, the level's name, the levels as
/// extracted it read
pub(super) type ExportResult = (
  Result<LevelExportReport, String>,
  String,
  Vec<(String, Arc<Scene>)>,
);

/// The Export window's (App::export_options, m_export_report, m_export_error)
#[derive(Default)]
pub struct ExportState {
  /// the options of the last export, for the level `level` (open_export_window)
  pub options: LevelExportOptions,
  pub(super) level: String,
  /// the levels as extracted, read once (m_export_originals): the export compares their decor
  pub(super) originals: BTreeMap<String, Arc<Scene>>,
  pub report: Option<LevelExportReport>,
  pub error: String,
  /// counts the exports ended (a caller waits for it to move)
  pub generation: u64,
}

/// The Nav meshes window's (nav_params, nav_keep_selected, nav_first_aid, m_nav_last)
pub struct NavState {
  pub params: NavParams,
  /// keep only the ground the selected elements stand on
  pub keep_selected: bool,
  pub first_aid: u32,
  /// the last generation: its numbers, or why it failed
  pub last: Option<Result<NavBuildStats, String>>,
  /// counts the generations ended (m_nav_generation: the remote waits on it)
  pub generation: u64,
}

impl Default for NavState {
  fn default() -> Self {
    Self {
      params: NavParams::default(),
      keep_selected: false,
      first_aid: K_NAV_FIRST_AID,
      last: None,
      generation: 0,
    }
  }
}

/// The last search for holes (m_gaps), and where it looked
pub struct GapsFound {
  pub report: GapReport,
  pub region: AABB,
}

impl GapsFound {
  pub fn to_json(&self) -> Value {
    let r = &self.report;
    let gaps = r.spots.iter().filter(|s| !s.edge).count();
    let spots: Vec<Value> = r
      .spots
      .iter()
      .map(|s| {
        json!({"at": s.at, "seen_from": s.seen_from, "direction": s.direction, "rays": s.rays,
          "edge": s.edge})
      })
      .collect();
    json!({"spots": spots, "gaps": gaps, "edges": r.spots.len() - gaps,
      "viewpoints": r.viewpoints, "rays": r.rays, "escaped": r.escaped,
      "region": {"lo": self.region.lo, "hi": self.region.hi}})
  }
}

fn vec3_of(v: &Value) -> Option<Vec3> {
  serde_json::from_value(v.clone()).ok()
}

/// a box {lo, hi} or {min, max} (the MCP's)
fn box_of(v: &Value) -> Option<AABB> {
  let lo = vec3_of(v.get("lo").or_else(|| v.get("min"))?)?;
  let hi = vec3_of(v.get("hi").or_else(|| v.get("max"))?)?;
  let mut b = AABB::default();
  b.add(lo);
  b.add(hi);
  Some(b)
}

impl Editor {
  // ----------------------------------------------------------------------------------------------
  // the export for the game
  // ----------------------------------------------------------------------------------------------

  /// The options of a first export of the document (App::default_export_options)
  pub fn default_export_options(&mut self) -> LevelExportOptions {
    let data = self.document_game_data();
    let project = project_folder_name(&self.project_name());
    default_export_options(
      &mut self.scene,
      data.as_deref(),
      &self.library_root,
      &project,
    )
  }

  /// The Export window's options when it opens (open_export_window): those of the last export of
  /// this level, else new ones. With the levels it can replace: the one the document replaces, the
  /// one it opened, when the game has them as levels of their own (a DGO, not a layer: what
  /// check_export_names takes; the C++ combo offered the layers too, then refused them).
  pub fn export_defaults(&mut self) -> Value {
    if self.export.options.name.is_empty() || self.export.level != self.scene.settings.level {
      self.export.options = self.default_export_options();
      self.export.level = self.scene.settings.level.clone();
    }
    let data = self.document_game_data();
    let mut replaceable: Vec<String> = Vec::new();
    for l in [&self.scene.settings.replaces, &self.scene.settings.level] {
      let has_dgo = data
        .as_deref()
        .and_then(|d| d.level(l))
        .is_some_and(|info| !info.dgo.is_empty() && !info.layer);
      if !l.is_empty() && has_dgo && !replaceable.contains(l) {
        replaceable.push(l.clone());
      }
    }
    json!({"options": self.export.options, "replaceable": replaceable})
  }

  /// What is wrong with the export's names (a translation key), None when they can go
  pub fn export_check(&mut self, o: &LevelExportOptions) -> Option<&'static str> {
    let data = self.document_game_data();
    check_export_names(o, data.as_deref())
  }

  pub fn export_start_point(&mut self) -> Vec3 {
    let data = self.document_game_data();
    export_start_point(&mut self.scene, data.as_deref())
  }

  /// Where the game starts the player in the level `level` replaced: the ground under each of its
  /// continue points in the document's own level (null: nothing within a few meters)
  pub fn check_continues(&mut self, level: &str) -> Value {
    let data = self.document_game_data();
    self.scene.begin_frame();
    json!(check_continues(&self.scene, data.as_deref(), level))
  }

  /// Writes the level for the game (App::export_for_game) on a worker; `wait`: in this call (the
  /// remote control's). Its report, or why it failed, in `export` once done (export_state).
  pub fn export_for_game(&mut self, o: LevelExportOptions, wait: bool) -> Result<(), String> {
    if self.busy("export") {
      return Err("an export is going on".into());
    }
    let work = self.export_work(o)?;
    if wait {
      let r = work();
      self.export_done(r);
    } else {
      self.run_job("export", tr("export.title").into(), work, |ed, r| {
        ed.export_done(r)
      });
    }
    Ok(())
  }

  /// The export of the document with `o`, to run on a worker (export_for_game, play_level), then
  /// given to export_done
  pub(super) fn export_work(
    &mut self,
    mut o: LevelExportOptions,
  ) -> Result<impl FnOnce() -> ExportResult + Send + 'static, String> {
    // the models as they are, not as the animation preview draws them
    self.stop_preview();
    if !self.document_open() {
      return Err(tr("export.no_document").into());
    }
    self.export.options = o.clone();
    self.export.level = self.scene.settings.level.clone();
    // the game's lighting of the levels' decor, for a document made before the extraction kept it
    let lit = self.restore_level_lighting();
    if lit > 0 {
      log_info!(tr("log.lighting_restored"), lit);
    }
    (o.navmeshes, o.navmesh_names) = world_navmeshes(&self.scene);
    // the levels as extracted (read once): the pieces of their decor deleted, moved or hidden take
    // the game's collision with them
    let mut to_read: Vec<(String, String, Option<String>)> = Vec::new();
    if o.game_collision {
      for level in collision_levels(&self.scene) {
        if o.originals.contains_key(&level) {
          continue;
        }
        if let Some(s) = self.export.originals.get(&level) {
          o.originals.insert(level, s.clone());
          continue;
        }
        // a level of Jak II (jak2-<name>): read from its library, named as the document has it
        let jak2 = level.strip_prefix("jak2-").map(str::to_string);
        let entry = match &jak2 {
          Some(name) => self.library_jak2.level(name),
          None => self.library.level(&level),
        };
        if let Some(e) = entry {
          to_read.push((level.clone(), e.background.clone(), jak2));
        }
      }
    }
    // the regions of the level replaced, as extracted
    if !o.replaces.is_empty()
      && o.region_trees.is_null()
      && let Some(e) = self.library.level(&o.replaces)
    {
      o.region_trees = e.region_trees.clone();
    }
    self.scene.begin_frame(); // the world matrices and what is shown, after the last changes
    let scene = self.scene.clone();
    let data = self.document_game_data();
    let work = move || {
      let mut read = Vec::new();
      for (level, background, jak2) in to_read {
        match read_original(Path::new(&background)) {
          Ok(mut s) => {
            if let Some(name) = jak2 {
              make_jak2_level(&mut s, 0, &name);
              s.begin_frame();
            }
            let s = Arc::new(s);
            o.originals.insert(level.clone(), s.clone());
            read.push((level, s));
          }
          Err(e) => log_warn!("%s: %s", background, e),
        }
      }
      let r = export_level(&scene, data.as_deref(), &o);
      (r, o.name, read)
    };
    Ok(work)
  }

  pub(super) fn export_done(&mut self, (r, name, read): ExportResult) {
    self.export.originals.extend(read);
    self.export.generation += 1;
    match r {
      Ok(rep) => {
        log_info!(
          tr("log.exported_for_game"),
          &name,
          &rep.dir,
          rep.meshes,
          rep.actors
        );
        self.export.report = Some(rep);
        self.export.error.clear();
      }
      Err(e) => {
        log_error!(tr("log.export_failed"), &e);
        self.export.report = None;
        self.export.error = e;
      }
    }
  }

  pub fn export_state(&self) -> Value {
    json!({
      "busy": self.busy("export"),
      "generation": self.export.generation,
      "report": self.export.report,
      "error": self.export.error,
    })
  }

  // ----------------------------------------------------------------------------------------------
  // nav meshes
  // ----------------------------------------------------------------------------------------------

  /// around the selection, invalid without one
  pub fn navmesh_region_of_selection(&self) -> AABB {
    let b = self.selection_bounds();
    if b.valid() { b.expanded(1.0) } else { b }
  }

  /// where the selected elements stand
  pub fn navmesh_points_of_selection(&self) -> Vec<Vec3> {
    let mut out = Vec::new();
    for &id in &self.selection {
      match self.scene.find(id) {
        Some(n) if n.kind != NodeKind::NavMesh => {}
        _ => continue,
      }
      let b = self.subtree_bounds(id).0;
      if b.valid() {
        out.push(Vec3::new(b.center().x, b.lo.y, b.center().z));
      }
    }
    out
  }

  /// Generates the nav meshes of `region` (invalid: the whole level) on a worker; they replace the
  /// region's when done, one undo step, selected (start_navmesh_generation, then
  /// poll_navmesh_generation). `keep_from`: only the ground next to these points.
  pub fn start_navmesh_generation(
    &mut self,
    region: &AABB,
    keep_from: Vec<Vec3>,
  ) -> Result<(), String> {
    if self.busy("navmesh") {
      return Err(tr("navmesh.busy").into());
    }
    if !self.document_open() {
      return Err(tr("navmesh.no_document").into());
    }
    self.scene.begin_frame();
    let (verts, tris, region) = navmesh_surfaces(&self.scene, region, &self.nav.params);
    if tris.is_empty() {
      return Err(tr("navmesh.nothing").into());
    }
    let params = NavParams {
      keep_from,
      ..self.nav.params.clone()
    };
    log_info!(tr("log.navmesh_started"), tris.len() / 3);
    let document = self.document_epoch;
    self.run_job(
      "navmesh",
      tr("navmesh.working").into(),
      move || build_navmeshes(&verts, &tris, &region, &params),
      move |ed, r| ed.navmesh_generated(document, &region, r),
    );
    Ok(())
  }

  // poll_navmesh_generation: the new meshes replace those of the region
  fn navmesh_generated(
    &mut self,
    document: u64,
    region: &AABB,
    r: Result<(Vec<NavMeshData>, NavBuildStats), String>,
  ) {
    self.nav.generation += 1;
    let (meshes, stats) = match r {
      Ok(r) => r,
      Err(e) => {
        log_warn!(tr("log.navmesh_failed"), &e);
        self.nav.last = Some(Err(e));
        return;
      }
    };
    // another document since: not for this one
    if document != self.document_epoch {
      return;
    }
    let (added, entry) = replace_navmeshes(&mut self.scene, region, &meshes);
    self.history.push(entry);
    self.selection = added.iter().copied().collect();
    self.primary = added.first().copied().unwrap_or(0);
    self.view.navmesh = true;
    let area: f32 = meshes.iter().map(NavMeshData::area).sum();
    log_info!(
      tr("log.navmesh_done"),
      stats.meshes,
      stats.polys,
      area,
      stats.ms / 1000.0
    );
    self.nav.last = Some(Ok(stats));
  }

  /// generate_navmesh {region?: "all" | "selection" | {lo, hi} | {min, max}, node?, keep_selected?,
  /// from?: "selection" | [[x, y, z]...], params?}: `node` regenerates around a nav mesh (its
  /// inspector's), `keep_selected` keeps the ground under the selection (the window's), `from` next
  /// to points (the MCP's); the params given are kept (the window's, as the C++ remote did).
  pub fn generate_navmesh_call(&mut self, a: &Value) -> Result<(), String> {
    if let Some(p) = a.get("params") {
      self.set_navmesh_params(p);
    }
    let mut region = AABB::default();
    if let Some(id) = a.get("node").and_then(Value::as_u64) {
      let n = self.scene.find(id as u32).ok_or("node: no such element")?;
      region = self
        .scene
        .world_bounds(n)
        .expanded(self.nav.params.agent_radius + 1.0);
    } else {
      match a.get("region") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if s == "all" => {}
        Some(Value::String(s)) if s == "selection" => {
          region = self.navmesh_region_of_selection();
          if !region.valid() {
            return Err("region 'selection': nothing is selected".into());
          }
        }
        Some(r) => {
          region = box_of(r)
            .ok_or("region: \"all\", \"selection\" or {\"min\": [x, y, z], \"max\": [x, y, z]}")?
        }
      }
    }
    if let Some(k) = a.get("keep_selected").and_then(Value::as_bool) {
      self.nav.keep_selected = k;
    }
    // the pieces of ground to keep: next to these points, or to the selected elements
    let keep_from = match a.get("from") {
      Some(Value::Array(points)) => points
        .iter()
        .map(vec3_of)
        .collect::<Option<Vec<_>>>()
        .ok_or("from: \"selection\" or a list of points [x, y, z]")?,
      Some(Value::String(s)) if s == "selection" => self.points_or_nothing_selected()?,
      Some(_) => return Err("from: \"selection\" or a list of points [x, y, z]".into()),
      None if a.get("keep_selected").and_then(Value::as_bool) == Some(true) => {
        self.points_or_nothing_selected()?
      }
      None => Vec::new(),
    };
    self.start_navmesh_generation(&region, keep_from)
  }

  fn points_or_nothing_selected(&self) -> Result<Vec<Vec3>, String> {
    let p = self.navmesh_points_of_selection();
    if p.is_empty() {
      return Err("from 'selection': nothing is selected".into());
    }
    Ok(p)
  }

  /// the parameters given (an object of NavParams' fields), the others kept
  fn set_navmesh_params(&mut self, p: &Value) {
    let mut v = json!(self.nav.params);
    if let (Some(base), Some(over)) = (v.as_object_mut(), p.as_object()) {
      for (k, x) in over {
        if base
          .get(k)
          .is_some_and(|old| old.is_number() == x.is_number())
        {
          base.insert(k.clone(), x.clone());
        }
      }
    }
    if let Ok(p) = serde_json::from_value(v) {
      self.nav.params = p;
    }
  }

  /// set_navmesh {params?, defaults?, keep_selected?, first_aid?}: the window's fields
  pub fn set_navmesh(&mut self, a: &Value) {
    if a.get("defaults").and_then(Value::as_bool) == Some(true) {
      self.nav.params = NavParams::default();
    }
    if let Some(p) = a.get("params") {
      self.set_navmesh_params(p);
    }
    if let Some(k) = a.get("keep_selected").and_then(Value::as_bool) {
      self.nav.keep_selected = k;
    }
    if let Some(id) = a.get("first_aid").and_then(Value::as_u64) {
      self.nav.first_aid = id as u32;
    }
  }

  /// The Nav meshes window: its fields, the generation going on or the last one, what the document
  /// has, what the selection allows
  pub fn navmesh_state(&self) -> Value {
    let nodes = navmesh_nodes(&self.scene);
    let (mut polys, mut area, mut bad) = (0, 0.0f32, 0);
    for &id in &nodes {
      let Some(m) = self.scene.find(id).and_then(navmesh_of_node) else {
        continue;
      };
      polys += m.polys.len();
      area += m.area();
      bad += navmesh_problem(&m).is_some() as usize;
    }
    let last = match &self.nav.last {
      None => Value::Null,
      Some(Ok(stats)) => json!({"ok": true, "stats": stats}),
      Some(Err(e)) => json!({"ok": false, "error": e}),
    };
    json!({
      "params": self.nav.params,
      "keep_selected": self.nav.keep_selected,
      "first_aid": self.nav.first_aid,
      "busy": self.busy("navmesh"),
      "generation": self.nav.generation,
      "last": last,
      "nodes": nodes,
      "polys": polys,
      "area": area,
      "bad": bad,
      "selection_region": self.navmesh_region_of_selection().valid(),
      "selection_points": self.navmesh_points_of_selection().len(),
    })
  }

  /// navmesh_data {ids?}: the polygons of nav mesh nodes (all of them without ids), relative to the
  /// node (the viewport applies its world matrix): {id, mesh, mesh_version (the scene's: drawn again
  /// when they change), verts, polys, adj, area, problem}
  pub fn navmesh_data(&self, ids: Option<Vec<u32>>) -> Value {
    let ids = ids.unwrap_or_else(|| navmesh_nodes(&self.scene));
    let mut out = Vec::new();
    for id in ids {
      let Some(n) = self.scene.find(id) else {
        continue;
      };
      let Some(m) = navmesh_of_node(n) else {
        continue;
      };
      let (uid, version) = n
        .mesh
        .as_ref()
        .map_or((0, 0), |m| (m.uid as u32, m.version as u32));
      let polys: Vec<&[usize]> = m.polys.iter().map(|p| &p.v[..p.count]).collect();
      let adj: Vec<&[i32]> = m.polys.iter().map(|p| &p.adj[..p.count]).collect();
      out.push(json!({
        "id": id,
        "mesh": uid,
        "mesh_version": version,
        "verts": m.verts,
        "polys": polys,
        "adj": adj,
        "area": m.area(),
        "problem": navmesh_problem(&m),
      }));
    }
    json!(out)
  }

  /// export_navmeshes {path, first_aid?}: the nav_data file for the level builder (.json added)
  pub fn export_navmeshes_call(
    &mut self,
    path: &str,
    first_aid: Option<u32>,
  ) -> Result<Value, String> {
    if path.is_empty() {
      return Err("path: the nav_data file to write (.json)".into());
    }
    let mut path = path.to_string();
    if !path.to_ascii_lowercase().ends_with(".json") {
      path.push_str(".json");
    }
    let first = first_aid.unwrap_or(self.nav.first_aid);
    match export_navmeshes(&self.scene, Path::new(&path), first) {
      Ok(n) => Ok(json!({"path": path, "meshes": n, "first_aid": first})),
      Err(e) => {
        log_error!(tr("log.navmesh_export_failed"), &e);
        Err(e)
      }
    }
  }

  // ----------------------------------------------------------------------------------------------
  // holes in the decor (building_tools.cpp)
  // ----------------------------------------------------------------------------------------------

  /// the selection's, else the document's own level
  pub fn gaps_region(&self) -> AABB {
    let r = self.selection_bounds();
    if r.valid() {
      r
    } else {
      self.own_level_bounds()
    }
  }

  /// find_gaps {region?: {lo, hi}, spacing?, eye?}: the holes through which the player would see
  /// the void (run_find_gaps), around `region`, else gaps_region(); kept for the Holes window
  /// (gaps_report), which is told by a jle:changed {gaps: true}.
  pub fn find_gaps_call(&mut self, a: &Value) -> Value {
    let mut o = GapOptions {
      region: a.get("region").and_then(box_of).unwrap_or_default(),
      ..Default::default()
    };
    if !o.region.valid() {
      o.region = self.gaps_region();
    }
    if let Some(s) = a.get("spacing").and_then(Value::as_f64) {
      o.spacing = s.max(0.0) as f32;
    }
    if let Some(e) = a.get("eye").and_then(Value::as_f64) {
      o.eye = e as f32;
    }
    if let Some(h) = self.ocean_height().filter(|_| self.view.ocean) {
      o.ocean = h;
    }
    self.scene.begin_frame();
    let report = find_gaps(&self.scene, &o);
    log_info!(tr("log.gaps"), report.spots.len(), report.viewpoints);
    let found = GapsFound {
      report,
      region: o.region,
    };
    let answer = found.to_json();
    self.gaps = Some(found);
    self.emit("jle:changed", json!({"gaps": true}));
    answer
  }

  pub fn gaps_report(&self) -> Value {
    self.gaps.as_ref().map_or(Value::Null, GapsFound::to_json)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::test_library::{make_box, make_floor, temp_dir};
  use crate::scene::{Material, Node};

  fn editor() -> Editor {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed
  }

  /// a level of its own: a floor of 100 m and a box on it
  fn small_level(ed: &mut Editor) -> (u32, u32) {
    ed.scene.settings.level = "testlvl".into();
    let mat = ed.scene.add_material(Material {
      name: "stone".into(),
      ..Default::default()
    });
    let floor = ed.scene.add(Node {
      name: "floor".into(),
      kind: NodeKind::Render,
      mesh: Some(make_floor(mat)),
      ..Default::default()
    });
    let mut bx = Node {
      name: "box-1".into(),
      kind: NodeKind::Render,
      mesh: Some(make_box("box", mat)),
      ..Default::default()
    };
    bx.local.t = Vec3::new(10.0, 0.0, 0.0);
    bx.local.s = Vec3::new(3.0, 3.0, 3.0);
    let bx = ed.scene.add(bx);
    ed.scene.begin_frame();
    (floor, bx)
  }

  #[test]
  fn export_for_the_game_through_the_editor() {
    let tmp = temp_dir("editor-export");
    let mut ed = editor();
    assert!(
      ed.call("export_for_game", &json!({})).is_err(),
      "export: no document"
    );
    small_level(&mut ed);
    let d = ed.call("export_defaults", &json!({})).unwrap();
    let mut o: LevelExportOptions = serde_json::from_value(d["options"].clone()).unwrap();
    assert!(
      o.name == "my-testlvl" && o.folder.ends_with("/exports/my-testlvl") && o.base_id == 90000,
      "export: the default options ({} in {})",
      o.name,
      o.folder
    );
    assert!(
      o.start.y > 0.5 && o.start.y < 1.5,
      "export: the start on the floor"
    );
    o.folder = tmp.join("export").to_string_lossy().into_owned();
    assert!(
      ed.call("export_check", &json!({"options": o}))
        .unwrap()
        .is_null()
    );
    let bad = LevelExportOptions {
      name: "X".into(),
      ..o.clone()
    };
    assert_eq!(
      ed.call("export_check", &json!({"options": bad})).unwrap(),
      json!("export.bad.name")
    );
    let s = ed.call("export_for_game", &json!({"options": o})).unwrap();
    assert!(!s["busy"].as_bool().unwrap() && s["error"] == "", "{s}");
    let r = &s["report"];
    let level_dir = r["level_dir"].as_str().unwrap();
    assert!(
      r["meshes"].as_u64() == Some(2)
        && Path::new(&r["dir"].as_str().unwrap())
          .join("README.md")
          .is_file()
        && Path::new(level_dir).join("my-testlvl.jsonc").is_file(),
      "export: the level written ({r})"
    );
    // the options kept for the next time the window opens
    let again = ed.call("export_defaults", &json!({})).unwrap();
    assert_eq!(again["options"]["folder"], json!(o.folder));
    let _ = std::fs::remove_dir_all(&tmp);
  }

  #[test]
  fn navmesh_generated_applied_and_undone() {
    let mut ed = editor();
    assert!(
      ed.call("generate_navmesh", &json!({})).is_err(),
      "nav mesh: no document"
    );
    let (floor, _) = small_level(&mut ed);
    // around the selection, its ground kept
    ed.call("select_only", &json!({"id": floor})).unwrap();
    let st = ed.call("navmesh_state", &json!({})).unwrap();
    assert!(
      st["selection_region"] == true && st["selection_points"] == 1,
      "{st}"
    );
    let st = ed
      .call(
        "generate_navmesh",
        &json!({"region": "selection", "keep_selected": true, "params": {"agent_radius": 0.5}}),
      )
      .unwrap();
    assert!(st["last"]["ok"] == true && st["generation"] == 1, "{st}");
    assert_eq!(ed.nav.params.agent_radius, 0.5, "nav mesh: the params kept");
    let nodes = navmesh_nodes(&ed.scene);
    assert!(!nodes.is_empty(), "nav mesh: nodes added");
    assert!(
      ed.selection.iter().copied().eq(nodes.iter().copied()) && ed.view.navmesh,
      "nav mesh: the new meshes selected and shown"
    );
    let data = ed.call("navmesh_data", &json!({})).unwrap();
    let m = &data[0];
    assert!(
      m["polys"].as_array().unwrap().len() >= 1
        && m["polys"][0].as_array().unwrap().len() >= 3
        && m["problem"].is_null()
        && m["area"].as_f64().unwrap() > 1000.0,
      "nav mesh: its polygons for the overlay ({m})"
    );
    // for the game
    let tmp = temp_dir("editor-navmesh");
    let path = tmp.join("nav").to_string_lossy().into_owned();
    let e = ed
      .call(
        "export_navmeshes",
        &json!({"path": path, "first_aid": 61000}),
      )
      .unwrap();
    assert!(e["meshes"].as_u64() == Some(nodes.len() as u64) && tmp.join("nav.json").is_file());
    // undone: gone; generated again around the floor's mesh (the box's top too, this time):
    // replaced, not added
    ed.call("undo", &json!({})).unwrap();
    assert!(navmesh_nodes(&ed.scene).is_empty() && ed.selection.is_empty());
    ed.call("redo", &json!({})).unwrap();
    let id = navmesh_nodes(&ed.scene)[0];
    let st = ed.call("generate_navmesh", &json!({"node": id})).unwrap();
    assert_eq!(
      navmesh_nodes(&ed.scene).len() as u64,
      st["last"]["stats"]["meshes"].as_u64().unwrap(),
      "nav mesh: replaced"
    );
    assert!(ed.scene.find(id).is_none());
    let _ = std::fs::remove_dir_all(&tmp);
  }

  #[test]
  fn holes_in_a_small_scene() {
    let mut ed = editor();
    small_level(&mut ed);
    assert!(ed.call("gaps_report", &json!({})).unwrap().is_null());
    // a floor of 100 m alone: its edges are open, seen from the ground near them
    let r = ed.call("find_gaps", &json!({})).unwrap();
    assert!(
      r["viewpoints"].as_u64().unwrap() > 0
        && r["edges"].as_u64().unwrap() > 0
        && r["spots"][0]["seen_from"].is_array(),
      "gaps: the floor's open edges ({r})"
    );
    assert_eq!(
      ed.call("gaps_report", &json!({})).unwrap(),
      r,
      "gaps: kept for the window"
    );
  }

  /// a level of the game exported with its own collision: the level as extracted read once (the
  /// pieces deleted take their collision with them)
  #[test]
  fn real_level_exported_with_the_games_collision() {
    let Some(root) = crate::io::test_library::real_library() else {
      return;
    };
    let tmp = temp_dir("editor-export-real");
    let mut ed = editor();
    ed.set_library(root.to_str().unwrap());
    ed.call("open_level", &json!({"name": "powergd"})).unwrap();
    let d = ed.call("export_defaults", &json!({})).unwrap();
    let mut o: LevelExportOptions = serde_json::from_value(d["options"].clone()).unwrap();
    o.folder = tmp.to_string_lossy().into_owned();
    let s = ed
      .call("export_for_game", &json!({"options": o, "wait": true}))
      .unwrap();
    let r = &s["report"];
    assert!(
      r["collision_triangles"].as_u64().unwrap() > 0 && ed.export.originals.contains_key("powergd"),
      "export: the game's collision, the level as extracted kept ({s})"
    );
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
