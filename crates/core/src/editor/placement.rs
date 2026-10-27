//! The Place tool and the prefabs (actions.cpp, app.cpp place_tool): what is placed (a prefab, an
//! object of the catalog, a model, a decor part of a level, a shape), the mesh of its preview merged
//! into the document once, where it goes (placement_matrix: the surface under the cursor, the
//! snap, its yaw, the tilt the right button gives it), placing it lit like the level around it;
//! the prefabs made of the selection (without their small pieces: building_tools.cpp), read once;
//! the floors and walls in the style of a level (guide_panel.cpp,
//! terrain_tool.cpp). The viewport draws the preview see-through at `placement_preview`'s matrix
//! and calls `place_at` when the button is let go (ui/render/place_tool.ts).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};

use super::groups::{new_group_id, set_group_tag};
use super::levels::{style_water_mesh, unique_name};
use super::structures::{BuildingKit, StructureParams};
use super::{Editor, ToolMode};
use crate::history::NodeChange;
use crate::i18n::tr;
use crate::io::asset_library::merge_mesh;
use crate::io::catalog::{CatalogModel, CatalogObject, CatalogPart, make_catalog_actor, part_mesh};
use crate::io::jak2_import::import_jak2_mesh;
use crate::io::prefabs::{load_prefab, new_prefab_path, place_prefab, save_prefab};
use crate::math::{Mat4, Quat, Ray, Transform, Vec3, normalize, radians, ray_plane};
use crate::scene::level_style::{LevelStyle, Surface, measure_style};
use crate::scene::pick::{PickFilter, PickHit, pick};
use crate::scene::relight::{K_LIGHT_MAX_VERTICES, light_like_surroundings};
use crate::scene::shapes::{
  ShapeParams, ShapePreset, make_shape, shape_default_material, shape_preset,
};
use crate::scene::{Mesh, Node, NodeKind, Primitive, Scene};
use crate::{log_error, log_info, log_warn, strf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlacementKind {
  #[default]
  Prefab,
  Object,
  Model,
  Part,
  Shape,
  /// a house in the style of a level (editor/structures): its box under the cursor
  House,
}

/// What the Place tool puts in the level: a prefab, an object of the catalog, a model, a decor
/// part of a level or a shape (app.h Placement).
#[derive(Clone, Debug)]
pub struct Placement {
  pub kind: PlacementKind,
  /// the prefab file, the model file (object, model), the part's level background
  pub path: String,
  pub name: String,
  /// Object
  pub object: CatalogObject,
  /// Part
  pub part: CatalogPart,
  /// Part, Model: "jak2" for Jak II's (io/jak2_import), else Jak 3's
  pub game: String,
  /// Model: the level it comes from
  pub level: String,
  /// Shape
  pub shape: ShapeParams,
  /// Shape: its material in the document (a level's style), -1 the grey grid
  pub material: i32,
  /// House: the structure it is (`shape` is its box)
  pub house: StructureParams,
  /// radians (R, Ctrl + wheel)
  pub yaw: f32,
  /// the turn the right button gives it, in the world (applied after the yaw)
  pub tilt: Quat,
  /// dragged from a panel: one placement, then back to selecting
  pub once: bool,
}

impl Default for Placement {
  fn default() -> Self {
    Self {
      kind: PlacementKind::Prefab,
      path: String::new(),
      name: String::new(),
      object: CatalogObject::default(),
      part: CatalogPart::default(),
      game: String::new(),
      level: String::new(),
      shape: ShapeParams::default(),
      material: -1,
      house: StructureParams::default(),
      yaw: 0.0,
      tilt: Quat::default(),
      once: false,
    }
  }
}

impl Placement {
  /// of its preview and of the meshes merged into the document
  pub fn key(&self) -> String {
    match self.kind {
      PlacementKind::Object | PlacementKind::Model => format!("model:{}", self.path),
      PlacementKind::Part => format!("part:{}#{}", self.path, self.part.mesh_node),
      PlacementKind::Shape => format!("shape:{}#{}", self.shape.to_json(), self.material),
      PlacementKind::House => format!("house:{}", self.house.to_json()),
      PlacementKind::Prefab => self.path.clone(),
    }
  }
}

/// The Place tool's state and what it keeps (app.h: placement, m_prefab_scenes, m_prefab_meshes,
/// m_prefab_previews, m_batch_change, m_styles...)
#[derive(Default)]
pub(crate) struct PlacingState {
  pub placement: Placement,
  /// the prefabs read, by path
  prefab_scenes: HashMap<String, Arc<Scene>>,
  /// the prefab meshes merged into the document, per prefab file and mesh (shared by placements)
  prefab_meshes: HashMap<String, HashMap<usize, Arc<Mesh>>>,
  /// the preview meshes, merged into the document once (by Placement::key)
  previews: HashMap<String, Arc<Mesh>>,
  /// the document they were merged into (Editor::document_epoch)
  document: u64,
  /// a batch of placements (place_many): one undo entry for all, lit at its end
  batch: Option<(NodeChange, Vec<u32>)>,
  /// captures and tests write their prefabs elsewhere
  pub prefab_dir_override: String,
  /// the level whose floors and walls the Build panel (and the terrain) take the style of, "" none
  pub style_level: String,
  /// measured once its decor is read, by level
  pub styles: BTreeMap<String, LevelStyle>,
  /// what its buildings are made of, read once its decor is, by level (editor/structures)
  pub kits: BTreeMap<String, BuildingKit>,
}

fn surface_json(s: &Surface) -> Value {
  json!({"material": s.material, "texture": s.texture, "tile": s.tile})
}

impl Editor {
  /// the meshes merged into another document are not this one's
  fn placement_caches(&mut self) {
    if self.placing.document != self.document_epoch {
      self.placing.document = self.document_epoch;
      self.placing.prefab_meshes.clear();
      self.placing.previews.clear();
    }
  }

  pub fn placement(&self) -> &Placement {
    &self.placing.placement
  }

  pub fn placement_mut(&mut self) -> &mut Placement {
    &mut self.placing.placement
  }

  fn begin_placement(&mut self, p: Placement) {
    self.placing.placement = p;
    self.tool = ToolMode::Place;
  }

  pub fn start_placement(&mut self, prefab_path: &str, name: &str, once: bool) {
    self.begin_placement(Placement {
      path: prefab_path.into(),
      name: name.into(),
      once,
      ..Default::default()
    });
    // the preview appears as soon as the file is read
    self.catalog.store.model(prefab_path, true);
  }

  pub fn start_object_placement(&mut self, object: &CatalogObject, once: bool) {
    self.begin_placement(Placement {
      kind: PlacementKind::Object,
      path: object.model_path.clone(),
      name: object.name.clone(),
      object: object.clone(),
      once,
      ..Default::default()
    });
    if !object.model_path.is_empty() {
      self.catalog.store.model(&object.model_path, true);
    }
  }

  pub fn start_model_placement(&mut self, model: &CatalogModel, once: bool) {
    self.begin_placement(Placement {
      kind: PlacementKind::Model,
      path: model.path.clone(),
      name: model.name.clone(),
      game: model.game.clone(),
      level: model.group.clone(),
      once,
      ..Default::default()
    });
    self.catalog.store.model(&model.path, true);
  }

  pub fn start_part_placement(&mut self, level_background: &str, part: &CatalogPart, once: bool) {
    let jak2 = self.library_jak2.level_of(level_background).is_some();
    self.begin_placement(Placement {
      kind: PlacementKind::Part,
      path: level_background.into(),
      name: part.name.clone(),
      part: part.clone(),
      game: if jak2 { "jak2".into() } else { String::new() },
      once,
      ..Default::default()
    });
    self.catalog.store.level(level_background, true);
  }

  pub fn start_shape_placement(&mut self, preset: &ShapePreset, once: bool) {
    self.begin_placement(Placement {
      kind: PlacementKind::Shape,
      name: preset.name.into(),
      shape: preset.params.clone(),
      once,
      ..Default::default()
    });
  }

  /// back to selecting
  pub fn stop_placement(&mut self) {
    if self.tool == ToolMode::Place {
      self.tool = ToolMode::Select;
    }
    self.placing.placement = Placement::default();
  }

  /// Where what is placed goes for a pick of the surface under the cursor and its ray: on the
  /// surface, else on the ground plane (y 0) not too far, else 15 m along the ray; snapped (x and
  /// z) with the gizmo's snap; turned by its yaw, then its tilt.
  pub fn placement_matrix(&self, hit: &PickHit, ray: &Ray) -> Mat4 {
    let mut p = if hit.hit() {
      hit.point
    } else {
      match ray_plane(ray, Vec3::default(), Vec3::new(0.0, 1.0, 0.0)) {
        Some(t) if t > 0.0 && t < 2000.0 => ray.at(t),
        _ => ray.at(15.0),
      }
    };
    let snap = &self.gizmo.snap;
    if snap.enabled && snap.translate > 0.0 {
      let s = snap.translate;
      p.x = (p.x / s).round() * s;
      p.z = (p.z / s).round() * s;
    }
    let pl = &self.placing.placement;
    Mat4::translate(p)
      * Mat4::rotate(pl.tilt)
      * Mat4::rotate(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), pl.yaw))
  }

  /// what is under a ray, for placing: the ground too, not the actors (pick_surface)
  pub fn pick_surface(&self, ray: &Ray) -> PickHit {
    let f = PickFilter {
      actors: false,
      include_locked: true,
      ..Default::default()
    };
    pick(&self.scene, &Ray::new(ray.o, normalize(ray.d)), &f)
  }

  /// The mesh of what is placed, merged into the document once; None while it loads (or an object
  /// drawn without a model: a marker).
  pub fn placement_mesh(&mut self) -> Option<Arc<Mesh>> {
    self.placement_caches();
    let key = self.placing.placement.key();
    if let Some(m) = self.placing.previews.get(&key) {
      return Some(m.clone());
    }
    let pl = self.placing.placement.clone();
    let mesh = match pl.kind {
      PlacementKind::Prefab | PlacementKind::Object | PlacementKind::Model => {
        if pl.path.is_empty() {
          return None; // an object drawn without a model: a marker
        }
        let lm = self.catalog.store.model(&pl.path, true)?;
        if pl.game == "jak2" {
          // a model of Jak II: its textures renamed, what could break checked (kept with the mesh)
          let mut checks = Value::Null;
          let mut m = import_jak2_mesh(
            &mut self.scene,
            &lm.mesh,
            &lm.textures,
            &lm.materials,
            Some(&mut checks),
          );
          Arc::make_mut(&mut m).extras["og_import_checks"] = checks;
          m
        } else {
          let m = merge_mesh(&mut self.scene, &lm.mesh, &lm.textures, &lm.materials);
          if pl.kind == PlacementKind::Object
            && self
              .game_data()
              .is_some_and(|d| d.is_water(&pl.object.etype))
          {
            style_water_mesh(&mut self.scene, &m, &pl.object.model);
          }
          m
        }
      }
      PlacementKind::Part => {
        let level = self.decor_scene_of(&pl.path)?;
        let part = part_mesh(&level, &pl.part)?;
        if pl.game == "jak2" {
          // a structure of Jak II: its textures renamed, what could break checked (kept with the
          // mesh)
          let mut checks = Value::Null;
          let mut m = import_jak2_mesh(
            &mut self.scene,
            &part,
            &level.textures,
            &level.materials,
            Some(&mut checks),
          );
          Arc::make_mut(&mut m).extras["og_import_checks"] = checks;
          m
        } else {
          merge_mesh(&mut self.scene, &part, &level.textures, &level.materials)
        }
      }
      PlacementKind::Shape | PlacementKind::House => {
        let material = if pl.material >= 0 {
          pl.material
        } else {
          shape_default_material(&mut self.scene)
        };
        make_shape(&pl.shape, material)
      }
    };
    self.placing.previews.insert(key, mesh.clone());
    Some(mesh)
  }

  /// the preview's file could not be read: nothing will come
  pub fn placement_failed(&self) -> bool {
    let pl = &self.placing.placement;
    match pl.kind {
      PlacementKind::Part => self.catalog.store.failed_level(&pl.path),
      PlacementKind::Shape | PlacementKind::House => false,
      _ => !pl.path.is_empty() && self.catalog.store.failed_model(&pl.path),
    }
  }

  /// the Place tool waits for its preview's file
  pub(crate) fn placement_waiting(&self) -> bool {
    let pl = &self.placing.placement;
    self.tool == ToolMode::Place
      && !(pl.kind == PlacementKind::Object && pl.path.is_empty())
      && !self.placing.previews.contains_key(&pl.key())
      && !self.placement_failed()
  }

  /// a prefab read once, None when unreadable
  pub fn prefab_scene(&mut self, path: &str) -> Option<Arc<Scene>> {
    if let Some(s) = self.placing.prefab_scenes.get(path) {
      return Some(s.clone());
    }
    let mut loaded = Scene::new();
    if let Err(e) = load_prefab(path, &mut loaded) {
      log_error!("%s: %s", path, e);
      return None;
    }
    let s = Arc::new(loaded);
    self.placing.prefab_scenes.insert(path.into(), s.clone());
    Some(s)
  }

  /// a prefab's file changed or went: read again next time
  pub(crate) fn forget_prefab(&mut self, path: &str) {
    self.placing.prefab_scenes.remove(path);
    self.placing.prefab_meshes.remove(path);
    self.placing.previews.remove(path);
    self.catalog.store.forget_model(path);
  }

  /// Puts what is placed at `m`, lit like the level around it (scene/relight), selected; one undo
  /// step (or the batch's). Returns the new roots (none while its file loads).
  pub fn place_at(&mut self, m: &Mat4) -> Vec<u32> {
    self.placement_caches();
    let pl = self.placing.placement.clone();
    if pl.kind == PlacementKind::House {
      // the house built there (its pieces placed one by one); the next one a little different
      return match self.build_structure(&pl.house, m) {
        Ok((roots, _)) => {
          self.placing.placement.house.seed = pl.house.seed.wrapping_add(1);
          roots
        }
        Err(e) => {
          log_warn!("{}", e);
          Vec::new()
        }
      };
    }
    let mut roots = Vec::new();
    let change = NodeChange::new(&self.scene);
    if pl.kind == PlacementKind::Prefab {
      let Some(prefab) = self.prefab_scene(&pl.path) else {
        return roots;
      };
      let merged = self
        .placing
        .prefab_meshes
        .entry(pl.path.clone())
        .or_default();
      roots = place_prefab(&mut self.scene, &prefab, m, merged);
      if roots.is_empty() {
        return roots;
      }
      // its elements stay together: a group named after the prefab (a double click selects it all)
      let gid = new_group_id(&self.scene);
      for &id in &roots {
        if let Some(n) = self.scene.find_mut(id) {
          set_group_tag(n, gid, &pl.name, true);
        }
      }
    } else {
      let mesh = self.placement_mesh();
      let markerless = pl.kind == PlacementKind::Object && pl.path.is_empty();
      if mesh.is_none() && !markerless {
        return roots; // still loading
      }
      let mut names: HashSet<String> = self.scene.nodes.iter().map(|n| n.name.clone()).collect();
      let local = Transform::from_matrix(m);
      let jak2 = pl.game == "jak2";
      let checks = |mesh: &Option<Arc<Mesh>>| {
        mesh
          .as_ref()
          .and_then(|m| m.extras.get("og_import_checks"))
          .filter(|c| !c.is_null())
          .cloned()
          .unwrap_or(json!([]))
      };
      match pl.kind {
        PlacementKind::Object => {
          // an actor named like the game names them: <type>-<number>
          let mut i = 1;
          let name = loop {
            let name = format!("{}-{i}", pl.object.etype);
            if !names.contains(&name) {
              break name;
            }
            i += 1;
          };
          let mut actor = make_catalog_actor(&pl.object, &name);
          actor.local = local;
          let id = self.scene.add(actor);
          if let Some(mesh) = mesh {
            self.scene.add(Node {
              parent: id,
              name: pl.object.model.clone(),
              kind: NodeKind::Render,
              mesh: Some(mesh),
              extras: json!({"og_actor_visual": pl.object.model}),
              ..Default::default()
            });
          }
          roots.push(id);
        }
        PlacementKind::Model => {
          let base = if jak2 {
            format!("jak2-{}", pl.name)
          } else {
            pl.name.clone()
          };
          // which model (its animations: the preview)
          let extras = if jak2 {
            json!({"og_import": {"game": "jak2", "level": pl.level, "part": pl.name, "checks": checks(&mesh)}})
          } else {
            json!({"og_model": pl.name})
          };
          roots.push(self.scene.add(Node {
            name: unique_name(&base, &mut names),
            kind: NodeKind::Render,
            mesh,
            extras,
            local,
            ..Default::default()
          }));
        }
        PlacementKind::Shape => {
          // its own mesh: its size changes apart from the others'
          let own = mesh.map(|m| Arc::new(Mesh::clone(&m)));
          roots.push(self.scene.add(Node {
            name: unique_name(&pl.name, &mut names),
            kind: NodeKind::Render,
            mesh: own,
            extras: json!({"og_shape": pl.shape.to_json()}),
            local,
            ..Default::default()
          }));
        }
        PlacementKind::Part | PlacementKind::Prefab | PlacementKind::House => {
          let mut base = pl
            .part
            .name
            .strip_suffix(".mb")
            .unwrap_or(&pl.part.name)
            .to_string();
          let mut extras = json!({});
          if jak2 {
            // named apart from Jak 3's parts of the same name; where it comes from and what was
            // checked
            base = format!("jak2-{base}");
            let from = self
              .library_jak2
              .level_of(&pl.path)
              .map(|l| l.name.clone())
              .unwrap_or_default();
            extras = json!({"og_import": {"game": "jak2", "level": from, "part": pl.part.name, "checks": checks(&mesh)}});
          }
          roots.push(self.scene.add(Node {
            name: unique_name(&base, &mut names),
            kind: NodeKind::Render,
            mesh,
            extras,
            local,
            ..Default::default()
          }));
        }
      }
    }
    // lit like the level around it (not an actor's model: the game lights it); a batch: at its end
    if let Some((_, placed)) = &mut self.placing.batch {
      placed.extend_from_slice(&roots);
    } else {
      light_like_surroundings(&mut self.scene, &roots, K_LIGHT_MAX_VERTICES);
      self.scene.begin_frame();
      let label = strf!(tr("undo.place"), &pl.name);
      self.history.push(change.commit(&self.scene, label, None));
    }
    self.scene.begin_frame();
    self.selection = roots.iter().copied().collect();
    self.primary = roots[0];
    roots
  }

  /// Placements made one after the other (the remote control's place_many) are one undo step,
  /// lit at the end (`end_place_batch`).
  pub fn begin_place_batch(&mut self) {
    self.placing.batch = Some((NodeChange::new(&self.scene), Vec::new()));
  }

  /// a batch whose undo step starts from `change` (what it replaces saved in it)
  pub(crate) fn begin_place_batch_with(&mut self, change: NodeChange) {
    self.placing.batch = Some((change, Vec::new()));
  }

  /// the batch dropped: no undo step, nothing lit (what it placed stays)
  pub(crate) fn discard_place_batch(&mut self) {
    self.placing.batch = None;
  }

  /// the meshes merged into the document forgotten (another document is about to take them)
  pub(crate) fn forget_merged_meshes(&mut self) {
    self.placing.prefab_meshes.clear();
    self.placing.previews.clear();
  }

  /// the batch's end: what it placed lit like the level around it, one undo step when it placed
  /// something; how many roots it placed
  pub fn end_place_batch(&mut self) -> usize {
    self.finish_place_batch(None, None)
  }

  /// the batch's end: `light` (else all it placed) lit like the level around it, one undo step
  /// named `label` (else "Place N")
  pub(crate) fn finish_place_batch(
    &mut self,
    light: Option<&[u32]>,
    label: Option<String>,
  ) -> usize {
    let Some((change, placed)) = self.placing.batch.take() else {
      return 0;
    };
    if placed.is_empty() {
      return 0;
    }
    light_like_surroundings(
      &mut self.scene,
      light.unwrap_or(&placed),
      K_LIGHT_MAX_VERTICES,
    );
    self.scene.begin_frame();
    let label = label.unwrap_or_else(|| strf!(tr("undo.place_many"), placed.len()));
    self.history.push(change.commit(&self.scene, label, None));
    placed.len()
  }

  // ----------------------------------------------------------------------------------------------
  // prefabs
  // ----------------------------------------------------------------------------------------------

  pub fn prefab_folder(&self) -> String {
    if self.placing.prefab_dir_override.is_empty() {
      self.library.prefab_folder()
    } else {
      self.placing.prefab_dir_override.clone()
    }
  }

  /// The elements of `roots` smaller than `min_size` meters left out (their names to `left_out`);
  /// an actor stays (building_tools.cpp)
  pub fn without_small(
    &self,
    roots: &[u32],
    min_size: f32,
    mut left_out: Option<&mut Vec<String>>,
  ) -> Vec<u32> {
    if min_size <= 0.0 {
      return roots.to_vec();
    }
    let mut kept = Vec::new();
    for &id in roots {
      let (b, _) = self.subtree_bounds(id);
      let s = if b.valid() { b.size() } else { Vec3::default() };
      let n = self.scene.find(id);
      // an actor is an object of the game, whatever its size (a button, a lamp)
      if n.is_some_and(|n| n.kind == NodeKind::Actor) || s.x.max(s.y).max(s.z) >= min_size {
        kept.push(id);
      } else if let (Some(out), Some(n)) = (left_out.as_deref_mut(), n) {
        out.push(n.name.clone());
      }
    }
    kept
  }

  /// The selection saved as a prefab: its file ("" when it could not be), without its elements
  /// smaller than `min_size` meters (their names to `left_out`).
  pub fn create_prefab(
    &mut self,
    name: &str,
    min_size: f32,
    left_out: Option<&mut Vec<String>>,
  ) -> String {
    let roots = self.without_small(&self.selection_roots(), min_size, left_out);
    if roots.is_empty() {
      return String::new();
    }
    let folder = self.prefab_folder();
    if folder.is_empty() {
      log_warn!("%s", tr("log.prefab_no_library"));
      return String::new();
    }
    let path = new_prefab_path(&folder, name);
    if let Err(e) = save_prefab(&self.scene, &roots, &path) {
      log_error!(tr("log.prefab_failed"), e);
      return String::new();
    }
    self.refresh_prefabs();
    let stem = std::path::Path::new(&path)
      .file_stem()
      .map(|s| s.to_string_lossy().into_owned())
      .unwrap_or_default();
    log_info!(tr("log.prefab_saved"), stem, roots.len());
    path
  }

  /// A prefab of mine deleted (the Prefabs panel's menu)
  /// Deletes a prefab of the prefab folder (a path elsewhere, or that is not there, is refused)
  pub fn delete_prefab(&mut self, path: &str) -> Result<(), String> {
    let inside = std::fs::canonicalize(path)
      .ok()
      .zip(std::fs::canonicalize(self.prefab_folder()).ok())
      .is_some_and(|(file, folder)| file != folder && file.starts_with(folder));
    if !inside {
      return Err(format!("not a prefab of the prefab folder: {path}"));
    }
    if self.placing.placement.path == path {
      self.stop_placement();
    }
    let _ = std::fs::remove_file(path);
    self.forget_prefab(path);
    self.refresh_prefabs();
    let stem = std::path::Path::new(path)
      .file_stem()
      .map(|s| s.to_string_lossy().into_owned())
      .unwrap_or_default();
    log_info!(tr("log.prefab_deleted"), stem);
    Ok(())
  }

  // ----------------------------------------------------------------------------------------------
  // the style of a level (Build panel, terrain): its floors' and walls' look
  // ----------------------------------------------------------------------------------------------

  /// A level's style, measured once its decor is read (a few seconds: None meanwhile, or when it
  /// cannot be read; the decor is asked for).
  pub fn level_style(&mut self, level_name: &str) -> Option<&LevelStyle> {
    if !self.placing.styles.contains_key(level_name) {
      let bg = self.library.level(level_name)?.background.clone();
      let decor = self.catalog.store.level(&bg, true)?;
      self
        .placing
        .styles
        .insert(level_name.into(), measure_style(&decor));
    }
    self.placing.styles.get(level_name)
  }

  /// the style level's decor cannot be read
  fn level_style_failed(&self, level_name: &str) -> bool {
    self
      .library
      .level(level_name)
      .is_some_and(|l| self.catalog.store.failed_level(&l.background))
  }

  /// the style level waits for its decor
  pub(crate) fn style_waiting(&self) -> bool {
    let l = &self.placing.style_level;
    !l.is_empty()
      && !self.placing.styles.contains_key(l)
      && self.library.level(l).is_some()
      && !self.level_style_failed(l)
  }

  /// A level style's floor (or wall) material, added to the document once; -1: none
  pub fn style_material(&mut self, level_name: &str, floor: bool) -> i32 {
    let Some(level) = self.library.level(level_name) else {
      return -1;
    };
    let Some(style) = self.placing.styles.get(level_name) else {
      return -1;
    };
    let material = if floor {
      style.floor.material
    } else {
      style.wall.material
    };
    let bg = level.background.clone();
    let Some(decor) = self.catalog.store.level(&bg, true) else {
      return -1;
    };
    if material < 0 {
      return -1;
    }
    // its material in the document (its texture once)
    let mut one = Mesh::new();
    one.prims.push(Primitive {
      material,
      ..Default::default()
    });
    merge_mesh(&mut self.scene, &one, &decor.textures, &decor.materials).prims[0].material
  }

  /// A shape (`shape`: a preset's name; "": a ground or a wall) with a level's floors' or walls'
  /// look; false when the style is not measured yet or has no such surface.
  pub fn start_style_placement(&mut self, level_name: &str, floor: bool, shape: &str) -> bool {
    let Some(style) = self.placing.styles.get(level_name).cloned() else {
      return false;
    };
    let surface = if floor { &style.floor } else { &style.wall };
    let material = self.style_material(level_name, floor);
    if material < 0 {
      return false;
    }
    let Some(chosen) =
      shape_preset(shape).or_else(|| shape_preset(if floor { "ground" } else { "wall" }))
    else {
      return false;
    };
    let mut preset = chosen.clone();
    preset.params.tile = surface.tile;
    preset.params.color = surface.color;
    if preset.params.kind == "ground" {
      preset.params.bumps = style.bumps;
      preset.params.bump_size = style.bump_size;
    }
    self.start_shape_placement(&preset, false);
    self.placing.placement.material = material;
    true
  }

  /// `level_style {level}`: the Build panel's level ("" none) chosen, its style: null (no level),
  /// {reading: true}, {failed: true} or {floor: {material, texture, tile}, wall: {...}, bumps,
  /// bump_size, valid} (a material < 0: no such surface)
  fn level_style_call(&mut self, level: &str) -> Value {
    self.placing.style_level = level.into();
    if level.is_empty() || self.library.level(level).is_none() {
      return Value::Null;
    }
    if let Some(s) = self.level_style(level) {
      return json!({
        "floor": surface_json(&s.floor),
        "wall": surface_json(&s.wall),
        "bumps": s.bumps,
        "bump_size": s.bump_size,
        "valid": s.valid(),
      });
    }
    if self.level_style_failed(level) {
      json!({"failed": true})
    } else {
      json!({"reading": true})
    }
  }

  // ----------------------------------------------------------------------------------------------
  // what the interface shows and asks
  // ----------------------------------------------------------------------------------------------

  /// get_state.placement while placing: {kind, name, path, preset (a shape's), once, yaw, tilt,
  /// mesh (the preview's uid, 0 while it loads or a marker), version, failed}
  pub(crate) fn placement_state(&self) -> Value {
    if self.tool != ToolMode::Place {
      return Value::Null;
    }
    let pl = &self.placing.placement;
    let mesh = self.placing.previews.get(&pl.key());
    json!({
      "kind": pl.kind,
      "name": pl.name,
      "path": pl.path,
      "game": pl.game,
      "etype": pl.object.etype,
      "mesh_node": pl.part.mesh_node,
      "once": pl.once,
      "yaw": pl.yaw,
      "tilt": pl.tilt,
      "mesh": mesh.map_or(0, |m| m.uid as u32),
      "version": mesh.map_or(0, |m| m.version as u32),
      "failed": self.placement_failed(),
    })
  }

  /// what get_state.placement is made of (state_key)
  pub(crate) fn placement_key(&self) -> Value {
    json!([
      self.placement_state(),
      self.catalog.epoch,
      self.placing.style_level
    ])
  }

  /// the preview's mesh (the jle:// protocol serves it as mesh/<uid>)
  pub fn placement_preview_mesh(&self) -> Option<&Arc<Mesh>> {
    if self.tool != ToolMode::Place {
      return None;
    }
    self.placing.previews.get(&self.placing.placement.key())
  }

  /// The Place tool's calls (api.rs): None when `name` is not one of them.
  pub(crate) fn placement_call(&mut self, name: &str, a: &Value) -> Option<Result<Value, String>> {
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or_default();
    let once = a.get("once").and_then(Value::as_bool).unwrap_or(false);
    let ray = || -> Option<Ray> {
      let v = |k: &str| serde_json::from_value::<Vec3>(a.get(k)?.clone()).ok();
      Some(Ray::new(v("origin")?, v("dir")?))
    };
    let done = Ok(Value::Null);
    let r = match name {
      "start_shape_placement" => {
        let Some(preset) = shape_preset(s("preset")) else {
          return Some(Err(format!("no shape '{}'", s("preset"))));
        };
        let mut preset = preset.clone();
        // its size and the rest changed (the remote control's)
        if let Some(over) = a.get("params").and_then(Value::as_object) {
          let mut j = preset.params.to_json();
          for (k, v) in over {
            j[k] = v.clone();
          }
          preset.params = ShapeParams::from_json(&j);
        }
        self.start_shape_placement(&preset, once);
        done
      }
      "start_style_placement" => {
        let floor = a.get("floor").and_then(Value::as_bool).unwrap_or(true);
        if self.start_style_placement(s("level"), floor, s("shape")) {
          done
        } else {
          Err(tr("build.style.reading").to_string())
        }
      }
      "start_model_placement" | "import_model" => {
        let path = s("path");
        let stem = std::path::Path::new(path)
          .file_stem()
          .map(|s| s.to_string_lossy().into_owned())
          .unwrap_or_default();
        let model = CatalogModel {
          name: if s("name").is_empty() {
            stem
          } else {
            s("name").into()
          },
          path: path.into(),
          group: s("group").into(),
          game: s("game").into(),
        };
        self.start_model_placement(&model, once);
        done
      }
      "start_object_placement" => {
        let found = self
          .catalog
          .objects
          .iter()
          .find(|o| o.name == s("name") && (s("etype").is_empty() || o.etype == s("etype")))
          .cloned();
        match found {
          Some(o) => {
            self.start_object_placement(&o, once);
            done
          }
          None => Err(format!("no object '{}' in the catalog", s("name"))),
        }
      }
      "start_part_placement" => {
        let part = CatalogPart {
          name: s("name").into(),
          mesh_node: s("mesh_node").into(),
          ..Default::default()
        };
        self.start_part_placement(s("background"), &part, once);
        done
      }
      "start_prefab_placement" => Ok(self.start_prefab_placement(s("path"), s("name"), once)),
      "stop_placement" => {
        self.stop_placement();
        done
      }
      "turn_placement" => {
        let d = a.get("degrees").and_then(Value::as_f64).unwrap_or(45.0) as f32;
        self.placing.placement.yaw += radians(d);
        done
      }
      "set_placement" => {
        let p = &mut self.placing.placement;
        if let Some(y) = a.get("yaw").and_then(Value::as_f64) {
          p.yaw = y as f32;
        }
        if let Some(t) = a
          .get("tilt")
          .and_then(|t| serde_json::from_value::<Quat>(t.clone()).ok())
        {
          p.tilt = t.normalized();
        }
        done
      }
      // {origin, dir} -> {matrix, point, hit} where it would go (null when not placing)
      "placement_preview" => {
        if self.tool != ToolMode::Place {
          return Some(Ok(Value::Null));
        }
        let Some(ray) = ray() else {
          return Some(Err("origin, dir: the cursor's ray".into()));
        };
        let hit = self.pick_surface(&ray);
        let m = self.placement_matrix(&hit, &ray);
        Ok(json!({"matrix": m, "point": m.col3(3), "hit": hit.hit()}))
      }
      // {matrix, keep?} or {origin, dir, keep?}: placed there, selected; then back to selecting
      // unless `keep` (Shift held: copies again) - a `once` placement always stops
      "place_at" | "place_here" => {
        if self.tool != ToolMode::Place {
          return Some(Ok(json!({"roots": []})));
        }
        let m = match a
          .get("matrix")
          .and_then(|m| serde_json::from_value::<Mat4>(m.clone()).ok())
        {
          Some(m) => m,
          None => match ray() {
            Some(ray) => self.placement_matrix(&self.pick_surface(&ray), &ray),
            None => return Some(Err("matrix: where it goes (16 floats)".into())),
          },
        };
        let keep = a.get("keep").and_then(Value::as_bool).unwrap_or(false);
        let once = self.placing.placement.once;
        let roots = self.place_at(&m);
        if once || (!roots.is_empty() && !keep) {
          // placed: selected (a prefab all of it) with the gizmo on it, to move it again at once
          self.stop_placement();
        }
        Ok(json!({"roots": roots}))
      }
      "create_prefab" => {
        let min = a.get("min_size").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let mut left_out = Vec::new();
        let path = self.create_prefab(s("name"), min.max(0.0), Some(&mut left_out));
        if !left_out.is_empty() {
          log_info!(tr("log.prefab_left_out"), left_out.len());
        }
        Ok(json!({"path": path, "left_out": left_out}))
      }
      // the New prefab window: its name at first, the elements selected, those kept
      "prefab_dialog" => {
        let roots = self.selection_roots();
        let min = a.get("min_size").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let kept = self.without_small(&roots, min, None).len();
        let primary = self.scene.find(self.primary);
        let name = primary
          .map(|n| n.mesh.as_ref().map_or(n.name.as_str(), |m| m.name.as_str()))
          .unwrap_or_default();
        let name = name.strip_suffix(".mb").unwrap_or(name);
        Ok(json!({
          "name": name,
          "roots": roots.len(),
          "kept": kept,
          "folder": !self.prefab_folder().is_empty(),
        }))
      }
      "delete_prefab" => self.delete_prefab(s("path")).map(|_| Value::Null),
      "level_style" => Ok(self.level_style_call(s("level"))),
      _ => return None,
    };
    // the preview merged at once when its file is there already
    if name.starts_with("start_") || name == "import_model" {
      self.placement_mesh();
    }
    Some(r)
  }

  /// A prefab placed from its panel: a game's prefab not made yet is made with its level's, then
  /// placed (answers {waiting: true}).
  fn start_prefab_placement(&mut self, path: &str, name: &str, once: bool) -> Value {
    let game = self
      .catalog
      .game_prefabs
      .iter()
      .find(|p| p.path == path)
      .cloned();
    if let Some(p) = game.filter(|p| !p.made) {
      self.catalog.place_when_made = p.name.clone();
      self.want_game_prefabs(&p.game, &p.level);
      return json!({"waiting": true});
    }
    let name = if name.is_empty() {
      std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
    } else {
      name.to_string()
    };
    self.start_placement(path, &name, once);
    Value::Null
  }
}

#[cfg(test)]
#[path = "placement_tests.rs"]
mod tests;
