//! The terrain brush (B, its own small window): a square target over the view, its size changed
//! with Ctrl + wheel or [ and ], what it would do shown see-through under it before it is done; the
//! left button held paints ground (with the relief and the textures of the level style chosen, the
//! open level's at first), raises walls from it or erases ground and walls (shown see-through until
//! it is let go, Esc: nothing done), smooths or flattens it; the right button held and the mouse
//! moved up raises the ground there, down digs it (shown until it is released). Its strokes go into
//! the terrain under the square (else one of its own, scene/terrain.rs), each one undone with
//! Ctrl+Z (src/app/terrain_tool.cpp).
//!
//! The viewport drives it (ui/render/terrain_tool.ts): `brush_hover` at each move of the mouse (the
//! ray under it), `brush_press`, `brush_drag` each frame the left button is held, `brush_release`,
//! `brush_sculpt` while the right button raises or digs. Each answers what the viewport draws over
//! the view (`brush_overlay`): the square on the ground, the magnet's edge, the see-through ghost of
//! what the brush would change.

use std::sync::Arc;

use serde_json::{Value, json};

use super::remote::Reply;
use super::{Editor, ToolMode};
use crate::history::NodeChange;
use crate::i18n::tr;
use crate::io::texture_palette::PaletteTexture;
use crate::math::{Ray, Vec2, Vec3, length, normalize, radians};
use crate::scene::pick::{PickFilter, pick};
use crate::scene::relight::{K_LIGHT_MAX_VERTICES, light_like_surroundings};
use crate::scene::shapes::shape_default_material;
use crate::scene::terrain::{
  BrushMode, BrushStroke, TerrainGrid, TerrainLook, apply_brush, generate_terrain,
  make_terrain_mesh, nearest_decor_edge, node_terrain, terrain_changes, terrain_height,
  terrain_level_near,
};
use crate::scene::{Mesh, Node, NodeKind, Scene};

/// The brush as its window sets it (App::TerrainBrush)
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainBrush {
  pub mode: BrushMode,
  /// meters: the square's side
  pub size: f32,
  /// raise, dig: meters per second at its middle; smooth, flatten: how fast
  pub strength: f32,
  /// meters above the ground
  pub wall_height: f32,
  /// a new terrain's cells (meters)
  pub cell: f32,
  /// paint: the level style's bumps
  pub relief: bool,
  /// the magnet brush: the ground laid at the level of the ground already there beside it
  pub magnet: bool,
  /// generate: the rectangle (meters)
  pub gen_size: Vec2,
  pub hills: f32,
  pub roughness: f32,
  pub seed: i32,
  /// a texture of the level style's level chosen for the ground, the walls ("": the style's)
  pub floor_texture: String,
  pub wall_texture: String,
}

impl Default for TerrainBrush {
  fn default() -> Self {
    Self {
      mode: BrushMode::Paint,
      size: 4.0,
      strength: 3.0,
      wall_height: 4.0,
      cell: 1.0,
      relief: true,
      magnet: false,
      gen_size: Vec2::new(64.0, 64.0),
      hills: 12.0,
      roughness: 0.5,
      seed: 1,
      floor_texture: String::new(),
      wall_texture: String::new(),
    }
  }
}

/// The brush's modes by name (the window's, the MCP tool's)
pub const BRUSH_MODES: [(&str, BrushMode); 8] = [
  ("paint", BrushMode::Paint),
  ("erase", BrushMode::Erase),
  ("raise", BrushMode::Raise),
  ("dig", BrushMode::Dig),
  ("smooth", BrushMode::Smooth),
  ("flatten", BrushMode::Flatten),
  ("wall", BrushMode::Wall),
  ("unwall", BrushMode::Unwall),
];

pub fn brush_mode_name(m: BrushMode) -> &'static str {
  BRUSH_MODES
    .iter()
    .find(|(_, b)| *b == m)
    .map_or("paint", |(n, _)| n)
}

pub fn brush_mode_from_name(name: &str) -> Option<BrushMode> {
  BRUSH_MODES
    .iter()
    .find(|(n, _)| *n == name)
    .map(|(_, m)| *m)
}

/// Shift: the other way (paint or walls and erase)
fn inverted(m: BrushMode) -> BrushMode {
  match m {
    BrushMode::Paint | BrushMode::Wall => BrushMode::Erase,
    BrushMode::Erase => BrushMode::Paint,
    m => m,
  }
}

/// "Terrain", else "Terrain-2"... (a name no element has)
pub fn terrain_name(scene: &Scene) -> String {
  let mut name = "Terrain".to_string();
  for i in 2.. {
    if !scene.nodes.iter().any(|n| n.name == name) {
      break;
    }
    name = format!("Terrain-{i}");
  }
  name
}

/// The brush, the stroke going on, where the brush is (App's m_terrain_*, m_brush_*, m_magnet_*;
/// the level style painted with is the Build panel's, `placing.style_level`)
pub struct TerrainState {
  pub brush: TerrainBrush,
  /// the textures a level's decor draws, the most used first (the brush's choices), and the level
  level_textures: Vec<PaletteTexture>,
  level_textures_of: String,
  /// the terrain painted (0: a new one at the first stroke)
  pub node: u32,
  /// a stroke going on
  change: Option<NodeChange>,
  /// its grid while it goes on
  grid: TerrainGrid,
  new: bool,
  changed: bool,
  /// a stroke laying ground, walls or erasing: shown see-through while the button is held (its
  /// grid before, the cells it reached), done when it is let go
  stroke_preview: bool,
  stroke_before: TerrainGrid,
  /// i0, k0, i1, k1
  stroke_cells: [i32; 4],
  /// the ground's height where the stroke started (its node's space)
  stroke_height: f32,
  /// the look its cells take (in the grid's looks)
  stroke_look: i32,
  /// where the brush is when the ray meets nothing
  brush_plane: f32,
  brush_point: Vec3,
  brush_valid: bool,
  /// what the brush is over (0: the plane)
  brush_hit_node: u32,
  /// where the last ray started (the camera)
  eye: Vec3,
  /// Shift held: the mode the other way
  shift: bool,
  // the magnet brush: where the cursor is, the point of the ground whose level it keeps, its piece,
  // how far below it the ground is laid (the decor's: 2 cm, not to flicker)
  magnet_valid: bool,
  magnet_below: f32,
  /// the stroke stuck to the decor once: flat all along (a platform made longer)
  stroke_stuck: bool,
  brush_raw: Vec3,
  magnet_edge: Vec3,
  magnet_node: u32,
  /// the last search (done again when the cursor moved)
  magnet_asked: Vec3,
  magnet_asked_size: f32,
  last_stamp: Vec3,
  has_stamp: bool,
  /// the element's grid, for the square drawn over it, and its mesh's (uid, version)
  grid_view: TerrainGrid,
  view_mesh: (u64, u64),
  /// the document the state is of
  epoch: u64,
}

impl Default for TerrainState {
  fn default() -> Self {
    Self {
      brush: TerrainBrush::default(),
      level_textures: Vec::new(),
      level_textures_of: String::new(),
      node: 0,
      change: None,
      grid: TerrainGrid::default(),
      new: false,
      changed: false,
      stroke_preview: false,
      stroke_before: TerrainGrid::default(),
      stroke_cells: [0, 0, -1, -1],
      stroke_height: 0.0,
      stroke_look: 0,
      brush_plane: 0.0,
      brush_point: Vec3::default(),
      brush_valid: false,
      brush_hit_node: 0,
      eye: Vec3::default(),
      shift: false,
      magnet_valid: false,
      magnet_below: 0.0,
      stroke_stuck: false,
      brush_raw: Vec3::default(),
      magnet_edge: Vec3::default(),
      magnet_node: 0,
      magnet_asked: Vec3::new(1e30, 0.0, 0.0),
      magnet_asked_size: 0.0,
      last_stamp: Vec3::default(),
      has_stamp: false,
      grid_view: TerrainGrid::default(),
      view_mesh: (0, 0),
      epoch: 0,
    }
  }
}

/// to the millimeter (the overlay's numbers, shorter)
fn mm(v: f32) -> f32 {
  (v * 1000.0).round() / 1000.0
}

/// The cells of [i0, i1] x [k0, k1] changed from `before` to `after`, for the viewport to draw
/// see-through (draw_terrain_ghost): per cell `i, k, flags (1 taken away, 2 a wall), the heights of
/// its corners (i, k), (i + 1, k), (i + 1, k + 1), (i, k + 1), a wall's height above them`.
fn ghost(before: &TerrainGrid, after: &TerrainGrid, r: [i32; 4], mode: BrushMode) -> Value {
  let changes = terrain_changes(before, after, r[0], r[1], r[2], r[3]);
  let mut cells = Vec::with_capacity(changes.len() * 8);
  for ch in &changes {
    let g = if ch.removed { before } else { after };
    let (i, k) = (ch.i, ch.k);
    let wall = g.at(i, k) == TerrainGrid::WALL;
    cells.extend([
      i as f32,
      k as f32,
      (ch.removed as u8 | (wall as u8) << 1) as f32,
      mm(g.corner(i, k)),
      mm(g.corner(i + 1, k)),
      mm(g.corner(i + 1, k + 1)),
      mm(g.corner(i, k + 1)),
      if wall {
        mm(g.wall[g.cell_index(i, k)])
      } else {
        0.0
      },
    ]);
  }
  let color = match mode {
    BrushMode::Wall => "wall",
    BrushMode::Dig => "dig",
    _ => "paint",
  };
  json!({"cell": after.cell, "mode": color, "cells": cells})
}

/// What a stamp would change (draw_terrain_preview); None: it changes nothing
fn preview(before: &TerrainGrid, stamp: &BrushStroke) -> Option<Value> {
  let mut after = before.clone();
  if !apply_brush(&mut after, stamp) {
    return None;
  }
  let (cl, half, c) = (after.cell, stamp.size * 0.5, stamp.center);
  let cell = |v: f32| (v / cl).floor() as i32;
  Some(ghost(
    before,
    &after,
    [
      cell(c.x - half) - 1,
      cell(c.z - half) - 1,
      cell(c.x + half) + 1,
      cell(c.z + half) + 1,
    ],
    stamp.mode,
  ))
}

fn vec3(a: &Value, k: &str) -> Option<Vec3> {
  serde_json::from_value(a.get(k)?.clone()).ok()
}

fn num(a: &Value, k: &str) -> Option<f32> {
  a.get(k).and_then(Value::as_f64).map(|v| v as f32)
}

/// a ray `{origin, dir}` of the call, None when it has none (the mouse out of the view)
fn ray_of(a: &Value) -> Option<Ray> {
  let (o, d) = (vec3(a, "origin")?, vec3(a, "dir")?);
  (length(d) > 0.0).then(|| Ray::new(o, normalize(d)))
}

impl Editor {
  // ----------------------------------------------------------------------------------------------
  // the level style: its textures (its look and materials: placement.rs level_style,
  // style_material)
  // ----------------------------------------------------------------------------------------------

  /// The textures a level's decor draws, the most used first (the brush's choices); empty until the
  /// library's index is ready (asked again then)
  pub fn level_textures(&mut self, level: &str) -> Vec<PaletteTexture> {
    let t = &self.terrain;
    if t.level_textures_of == level && !t.level_textures.is_empty() {
      return t.level_textures.clone();
    }
    let mut list = Vec::new();
    if self.catalog.index.ready() {
      for li in self.catalog.index.levels() {
        if li.level != level {
          continue;
        }
        for (name, &n) in &li.textures {
          if name.starts_with("anim-slot") {
            continue;
          }
          list.push(PaletteTexture {
            name: name.clone(),
            background: li.background.clone(),
            levels: 1,
            uses: n,
            ..Default::default()
          });
        }
      }
    }
    list.sort_by(|a, b| b.uses.cmp(&a.uses).then_with(|| a.name.cmp(&b.name)));
    let t = &mut self.terrain;
    t.level_textures_of = if self.catalog.index.ready() {
      level.to_string()
    } else {
      String::new() // asked again once the index is there
    };
    t.level_textures = list.clone();
    list
  }

  /// A material of the document drawing a texture of a level, like `like` (-1: the editor's)
  pub fn material_with_texture(&mut self, level: &str, texture: &str, like: i32) -> i32 {
    let Some(t) = self
      .level_textures(level)
      .into_iter()
      .find(|t| t.name == texture)
    else {
      return -1;
    };
    let ti = self.palette_texture(&t, true);
    if ti < 0 {
      return -1;
    }
    // the style's material (its blending, its sides) with that texture instead
    let like = if like >= 0 && (like as usize) < self.scene.materials.len() {
      like
    } else {
      shape_default_material(&mut self.scene)
    };
    let mut m = self.scene.materials[like as usize].clone();
    m.texture = ti;
    m.name = texture.to_string();
    let same = self.scene.materials.iter().position(|d| {
      d.name == m.name
        && d.texture == m.texture
        && d.alpha == m.alpha
        && d.blend == m.blend
        && d.double_sided == m.double_sided
        && d.translucent == m.translucent
    });
    same.map_or_else(|| self.scene.add_material(m), |i| i as i32)
  }

  /// A terrain's materials and texture sizes: the level style chosen, else the editor's grid; the
  /// textures chosen among the level's instead of the style's
  pub fn terrain_look(&mut self) -> TerrainLook {
    let mut g = TerrainLook::default();
    let level = self.placing.style_level.clone();
    let style = if level.is_empty() {
      None
    } else {
      self.level_style(&level).cloned()
    };
    if let Some(style) = &style {
      g.floor_material = self.style_material(&level, true);
      g.wall_material = self.style_material(&level, false);
      g.floor_tile = style.floor.tile;
      g.wall_tile = if style.wall.material >= 0 {
        style.wall.tile
      } else {
        style.floor.tile
      };
      g.color = if style.floor.material >= 0 {
        style.floor.color
      } else {
        style.wall.color
      };
    }
    if !level.is_empty() {
      let (floor, wall) = (
        self.terrain.brush.floor_texture.clone(),
        self.terrain.brush.wall_texture.clone(),
      );
      if !floor.is_empty() {
        let m = self.material_with_texture(&level, &floor, g.floor_material);
        if m >= 0 {
          g.floor_material = m;
        }
      }
      if !wall.is_empty() {
        let like = if g.wall_material >= 0 {
          g.wall_material
        } else {
          g.floor_material
        };
        let m = self.material_with_texture(&level, &wall, like);
        if m >= 0 {
          g.wall_material = m;
        }
      }
    }
    if g.floor_material < 0 && g.wall_material < 0 {
      let m = shape_default_material(&mut self.scene);
      g.floor_material = m;
      g.wall_material = m;
      g.floor_tile = 2.0;
      g.wall_tile = 2.0;
      g.color = Default::default();
    }
    g
  }

  // ----------------------------------------------------------------------------------------------
  // the tool
  // ----------------------------------------------------------------------------------------------

  pub fn start_terrain_brush(&mut self) -> bool {
    if !self.document_open() {
      return false;
    }
    self.stop_placement();
    self.stop_preview();
    if self.tool == ToolMode::Topology {
      self.stop_topology();
    }
    self.terrain_sync();
    // its look: the style chosen before, else the open level's
    if self.placing.style_level.is_empty()
      && self.library.level(&self.scene.settings.level).is_some()
    {
      self.placing.style_level = self.scene.settings.level.clone();
    }
    let level = self.placing.style_level.clone();
    if !level.is_empty() {
      self.level_style(&level); // read now: the window shows it, the first stroke takes it
    }
    // the terrain selected is the one painted, else a new one at the first stroke
    self.terrain.node = self
      .scene
      .find(self.primary)
      .filter(|n| node_terrain(n).is_some())
      .map_or(0, |n| n.id);
    self.select_only(0); // its highlight would hide the ground under the square
    self.tool = ToolMode::Terrain;
    true
  }

  pub(crate) fn stop_terrain_brush(&mut self) {
    if self.terrain.change.is_some() {
      if self.terrain.stroke_preview {
        self.terrain_cancel_stroke();
      } else {
        self.terrain_end_stroke();
      }
    }
    if self.tool == ToolMode::Terrain {
      self.tool = ToolMode::Select;
    }
  }

  /// another document, or the element painted gone (undone, deleted)
  fn terrain_sync(&mut self) {
    let t = &mut self.terrain;
    if t.epoch != self.document_epoch {
      t.epoch = self.document_epoch;
      t.node = 0;
      t.change = None;
      t.stroke_preview = false;
      t.view_mesh = (0, 0);
    }
    if t.change.is_none() && self.scene.find(t.node).is_none() {
      t.node = 0;
    }
  }

  /// The stroke going on: written into the element, one undo step
  fn terrain_end_stroke(&mut self) {
    let id = self.terrain.node;
    let exists = self.scene.find(id).is_some();
    if exists && self.terrain.grid.empty() && self.terrain.new {
      // nothing drawn on a new terrain: no element
      self.scene.remove(&[id]);
      self.terrain.node = 0;
      self.terrain.change = None;
      self.terrain.stroke_preview = false;
      self.scene_changed();
      return;
    }
    let t = &mut self.terrain;
    if let Some(n) = self.scene.find_mut(id) {
      n.extras["og_terrain_grid"] = t.grid.to_json();
      // what was shown, made
      if t.stroke_preview && t.changed {
        let prims = Arc::unwrap_or_clone(make_terrain_mesh(&t.grid)).prims;
        let mesh = Arc::make_mut(n.mesh.get_or_insert_with(|| Arc::new(Mesh::new())));
        mesh.prims = prims;
        mesh.touch();
      }
    }
    t.stroke_preview = false;
    let (changed, new) = (t.changed, t.new);
    let change = t.change.take();
    // lit like the level around it, as what is placed (its palettes made again for its new shape)
    if exists && changed {
      light_like_surroundings(&mut self.scene, &[id], K_LIGHT_MAX_VERTICES);
    }
    if let Some(c) = change
      && changed
    {
      let label = tr(if new {
        "undo.terrain.new"
      } else {
        "undo.terrain"
      });
      self.history.push(c.commit(&self.scene, label, None));
    }
    self.scene.begin_frame();
  }

  /// A stroke shown, not let go: nothing done (false: none shown). The element made for it taken
  /// away; the one painted keeps its mesh (the stroke only showed).
  pub(crate) fn terrain_cancel_stroke(&mut self) -> bool {
    if self.terrain.change.is_none() || !self.terrain.stroke_preview {
      return false;
    }
    if self.terrain.new && self.scene.find(self.terrain.node).is_some() {
      self.scene.remove(&[self.terrain.node]);
      self.terrain.node = 0;
      self.scene_changed();
    }
    self.terrain.change = None;
    self.terrain.stroke_preview = false;
    true
  }

  /// A stroke starts where the brush is: the element painted (a new one when there is none), its
  /// state kept for undo
  fn terrain_begin_stroke(&mut self, mode: BrushMode) -> bool {
    let mut change = NodeChange::new(&self.scene);
    self.terrain.new = false;
    self.terrain.changed = false;
    if self.scene.find(self.terrain.node).is_none() {
      if matches!(
        mode,
        BrushMode::Erase | BrushMode::Unwall | BrushMode::Smooth | BrushMode::Flatten
      ) {
        return false;
      }
      let id = self.scene.next_id;
      change.save(&self.scene, id);
      let g = TerrainGrid {
        cell: self.terrain.brush.cell,
        looks: vec![self.terrain_look()],
        ..Default::default()
      };
      let node = Node {
        id,
        name: terrain_name(&self.scene),
        kind: NodeKind::Render,
        extras: json!({"og_terrain_grid": g.to_json()}),
        mesh: Some(make_terrain_mesh(&g)),
        ..Default::default()
      };
      self.terrain.node = self.scene.add(node);
      self.terrain.new = true;
      self.scene_changed();
    } else {
      change.save(&self.scene, self.terrain.node);
    }
    // its mesh changed in place during the stroke is its own (Arc::make_mut: the one before stays
    // with the undo)
    let node = self.scene.find(self.terrain.node).unwrap();
    let grid = node_terrain(node);
    let world = self.scene.world(node);
    let mut grid = match grid {
      Some(g) => g,
      None => TerrainGrid {
        cell: self.terrain.brush.cell,
        looks: vec![self.terrain_look()],
        ..Default::default()
      },
    };
    // the cells painted take the style chosen now
    let look = self.terrain_look();
    self.terrain.stroke_look = grid.add_look(&look);
    let preview = matches!(mode, BrushMode::Paint | BrushMode::Wall | BrushMode::Erase);
    let height = self.terrain_start_height(&grid, self.terrain.node);
    let t = &mut self.terrain;
    t.stroke_preview = preview;
    t.stroke_stuck = t.magnet_valid;
    t.stroke_before = if preview {
      grid.clone()
    } else {
      TerrainGrid::default()
    };
    t.grid = grid;
    t.stroke_cells = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    t.stroke_height = height;
    t.brush_plane = t.brush_point.y;
    t.last_stamp = world.inverse().point(t.brush_point);
    t.has_stamp = false;
    t.change = Some(change);
    true
  }

  /// The ground's height where a stroke starting now starts (the terrain's space): on the terrain
  /// itself, its ground there (not the side of its edge, half a cell nearer the camera)
  fn terrain_start_height(&self, g: &TerrainGrid, node: u32) -> f32 {
    let t = &self.terrain;
    let n = self.scene.find(node);
    let to_node = n.map_or(crate::math::Mat4::identity(), |n| {
      self.scene.world(n).inverse()
    });
    let at = to_node.point(t.brush_point);
    if n.is_none() || t.brush_hit_node != node || t.magnet_valid {
      return at.y;
    }
    let mut back = to_node.dir(t.eye - t.brush_point);
    back.y = 0.0;
    let probe = if length(back) > 1e-4 {
      at + normalize(back) * (g.cell * 0.5)
    } else {
      at
    };
    terrain_height(g, at.x, at.z)
      .or_else(|| terrain_height(g, probe.x, probe.z))
      .unwrap_or(at.y)
  }

  /// A stamp of the brush as it is set, at `center` (the terrain's space)
  fn terrain_stamp(
    &mut self,
    mode: BrushMode,
    center: Vec3,
    amount: f32,
    height: f32,
  ) -> BrushStroke {
    let b = &self.terrain.brush;
    let mut st = BrushStroke {
      mode,
      center,
      size: b.size,
      height,
      seed: b.seed,
      look: self.terrain.stroke_look,
      amount,
      // the right button erasing: it raises or digs what is there only
      lay: b.mode != BrushMode::Erase,
      ..Default::default()
    };
    // new ground with the relief of the style (raise and dig lay some where there is none); flat
    // where it sticks to the decor (it meets it with no step)
    let level = self.placing.style_level.clone();
    let wanted = b.relief
      && mode != BrushMode::Wall
      && !self.terrain.magnet_valid
      && !self.terrain.stroke_stuck;
    if wanted
      && !level.is_empty()
      && let Some(style) = self.level_style(&level)
    {
      st.bumps = style.bumps;
      st.bump_size = style.bump_size.max(0.5);
    }
    st
  }

  /// The brush applied from `from` to `to` (the terrain's space), stamps a quarter of its size
  /// apart; `amount`: raise, dig, smooth, flatten: shared by the stamps; wall: its height
  fn terrain_apply(&mut self, mode: BrushMode, from: Vec3, to: Vec3, amount: f32) {
    if self.scene.find(self.terrain.node).is_none() {
      return;
    }
    let (size, cl) = (self.terrain.brush.size, self.terrain.grid.cell);
    let step = (size * 0.25).max(cl * 0.5);
    let dist = length(Vec3::new(to.x - from.x, 0.0, to.z - from.z));
    let n_stamps = ((dist / step).ceil() as i32).clamp(1, 64);
    let half = (size * 0.5).max(cl * 0.5);
    let mut changed = false;
    for s in 1..=n_stamps {
      let at = from + (to - from) * (s as f32 / n_stamps as f32);
      let a = if mode == BrushMode::Wall {
        amount
      } else {
        amount / n_stamps as f32
      };
      let stamp = self.terrain_stamp(mode, at, a, self.terrain.stroke_height);
      let t = &mut self.terrain;
      changed |= apply_brush(&mut t.grid, &stamp);
      let c = &mut t.stroke_cells;
      c[0] = c[0].min(((at.x - half) / cl).floor() as i32 - 1);
      c[1] = c[1].min(((at.z - half) / cl).floor() as i32 - 1);
      c[2] = c[2].max(((at.x + half) / cl).floor() as i32 + 1);
      c[3] = c[3].max(((at.z + half) / cl).floor() as i32 + 1);
    }
    if !changed {
      return;
    }
    self.terrain.changed = true;
    if !self.terrain.stroke_preview {
      let prims = Arc::unwrap_or_clone(make_terrain_mesh(&self.terrain.grid)).prims;
      if let Some(n) = self.scene.find_mut(self.terrain.node) {
        let mesh = Arc::make_mut(n.mesh.get_or_insert_with(|| Arc::new(Mesh::new())));
        mesh.prims = prims;
        mesh.touch();
      }
      self.scene_changed();
      self.scene.begin_frame();
    }
  }

  fn brush_mode_now(&self) -> BrushMode {
    let m = self.terrain.brush.mode;
    if self.terrain.shift { inverted(m) } else { m }
  }

  /// Where the brush is (terrain_tool): on what the ray meets (the terrain itself, the level), else
  /// on the plane of the last stroke; the terrain under the square, the magnet's level
  fn brush_update(&mut self, ray: Option<Ray>) {
    if !self.document_open() {
      self.stop_terrain_brush();
      return;
    }
    self.terrain_sync();
    self.refresh_grid_view();
    let t = &mut self.terrain;
    t.brush_valid = false;
    if let Some(ray) = ray {
      t.eye = ray.o;
      let pf = PickFilter {
        actors: false,
        include_locked: true,
        ..Default::default()
      };
      let hit = pick(&self.scene, &ray, &pf);
      t.brush_hit_node = hit.node;
      if hit.hit() {
        t.brush_point = hit.point;
        t.brush_valid = true;
        // the magnet brush keeps the height of what it was over: off a quay, over the water, the
        // square stays at its level (and sticks to its edge)
        if t.brush.magnet && t.change.is_none() {
          t.brush_plane = hit.point.y;
        }
      } else if ray.d.y.abs() > 1e-4 {
        let k = (t.brush_plane - ray.o.y) / ray.d.y;
        if k > 0.0 {
          t.brush_point = ray.at(k);
          t.brush_valid = true;
        }
      }
    }
    // the terrain under the square is the one it changes
    if t.change.is_none()
      && t.brush_hit_node != 0
      && t.brush_hit_node != t.node
      && self
        .scene
        .find(t.brush_hit_node)
        .is_some_and(|h| h.extras.get("og_terrain_grid").is_some())
    {
      t.node = t.brush_hit_node;
      self.refresh_grid_view();
    }
    let mode = self.brush_mode_now();
    // the magnet brush (ground, walls, flattening): the square goes where the cursor is, the ground
    // it lays at the level of the ground already laid beside it: the terrain painted's nearest,
    // else the top of the decor beside it (just under it: no flicker where they meet)
    let t = &mut self.terrain;
    t.magnet_valid = false;
    if !(t.brush.magnet
      && t.brush_valid
      && matches!(
        mode,
        BrushMode::Paint | BrushMode::Wall | BrushMode::Flatten
      ))
    {
      return;
    }
    let tn = self.scene.find(t.node).map(|n| (n.id, self.scene.world(n)));
    let stroke = t.change.is_some();
    let g = if stroke {
      Some(&t.grid)
    } else if tn.is_some() {
      Some(&t.grid_view)
    } else {
      None
    };
    let cell = g.filter(|g| g.w > 0).map_or(t.brush.cell, |g| g.cell);
    let reach = (t.brush.size * 0.5).max(cell * 0.5) + 2.0;
    if length(t.brush_point - t.magnet_asked) > 0.02
      || t.brush.size != t.magnet_asked_size
      || stroke
    {
      t.magnet_asked = t.brush_point;
      t.magnet_asked_size = t.brush.size;
      t.magnet_node = 0;
      let near = match (tn, g) {
        (Some((_, w)), Some(g)) if g.w > 0 => {
          terrain_level_near(g, w.inverse().point(t.brush_point), reach).map(|l| w.point(l))
        }
        _ => None,
      };
      if let (Some(level), Some((id, _))) = (near, tn) {
        t.magnet_edge = level;
        t.magnet_node = id;
        t.magnet_below = 0.0;
      } else if let Some((edge, on)) =
        nearest_decor_edge(&self.scene, t.brush_point, reach, 8.0, t.node)
      {
        // another terrain beside: the one painted from now on, made larger at its level
        t.magnet_edge = edge;
        t.magnet_node = on;
        let terrain = self
          .scene
          .find(on)
          .is_some_and(|n| n.extras.get("og_terrain_grid").is_some());
        if terrain && !stroke {
          t.node = on;
        }
        t.magnet_below = if terrain { 0.0 } else { 0.02 };
      }
    }
    if t.magnet_node == 0 {
      return;
    }
    t.magnet_valid = true;
    t.brush_raw = t.brush_point;
    t.brush_point.y = t.magnet_edge.y - t.magnet_below;
    // a stroke going on lays its ground at the height of what it sticks to from there on, flat
    if stroke && let Some(n) = self.scene.find(t.node) {
      t.stroke_height = self.scene.world(n).inverse().point(t.brush_point).y;
      t.stroke_stuck = true;
    }
    if !stroke {
      self.refresh_grid_view();
    }
  }

  /// the grid as the element has it (during a stroke, the one being drawn is `grid`)
  fn refresh_grid_view(&mut self) {
    let t = &mut self.terrain;
    let Some(n) = self.scene.find(t.node) else {
      return;
    };
    let key = n.mesh.as_ref().map_or((0, 0), |m| (m.uid, m.version));
    if key != t.view_mesh {
      t.view_mesh = key;
      t.grid_view = node_terrain(n).unwrap_or_default();
    }
  }

  /// The left button, held: the brush applied along the way (`dt`: seconds since the last time)
  fn brush_held(&mut self, dt: f32) {
    let Some(node) = self.scene.find(self.terrain.node) else {
      return;
    };
    if self.terrain.change.is_none() || !self.terrain.brush_valid {
      return;
    }
    let to = self
      .scene
      .world(node)
      .inverse()
      .point(self.terrain.brush_point);
    let mode = self.brush_mode_now();
    let dt = dt.clamp(0.0, 0.1);
    let b = &self.terrain.brush;
    let amount = match mode {
      BrushMode::Raise | BrushMode::Dig => b.strength * dt,
      BrushMode::Smooth | BrushMode::Flatten => (b.strength * 0.5 * dt).min(1.0),
      BrushMode::Wall => b.wall_height,
      _ => 0.0,
    };
    let from = if self.terrain.has_stamp {
      self.terrain.last_stamp
    } else {
      to
    };
    self.terrain_apply(mode, from, to, amount);
    self.terrain.last_stamp = to;
    self.terrain.has_stamp = true;
  }

  /// Meters the right button held raises (digs: negative): as far as the mouse went up (or down)
  /// on the screen, at the square's distance (`fov` degrees, `view_height` pixels)
  fn terrain_sculpt_height(&self, px: f32, fov: f32, view_height: f32) -> f32 {
    let t = &self.terrain;
    let per_px =
      2.0 * length(t.eye - t.brush_point) * (radians(fov) * 0.5).tan() / view_height.max(1.0);
    (px * per_px).clamp(-100.0, 100.0)
  }

  /// The right button let go: the ground raised or dug as shown, one undo step
  fn terrain_sculpt(&mut self, h: f32) {
    let mode = if h > 0.0 {
      BrushMode::Raise
    } else {
      BrushMode::Dig
    };
    if h.abs() < 0.01 || !self.terrain.brush_valid || !self.terrain_begin_stroke(mode) {
      return;
    }
    let Some(n) = self.scene.find(self.terrain.node) else {
      return;
    };
    let at = self
      .scene
      .world(n)
      .inverse()
      .point(self.terrain.brush_point);
    self.terrain_apply(mode, at, at, h.abs());
    self.terrain_end_stroke();
  }

  /// What the viewport draws over the view (draw_terrain_overlay): the square on the ground under
  /// it (the terrain's where it has some), the magnet's edge, what the brush would do there before
  /// it is done; `sculpt`: the right button raising (digging) that much
  pub(crate) fn brush_overlay(&mut self, sculpt: Option<f32>) -> Value {
    if self.tool != ToolMode::Terrain || !self.terrain.brush_valid {
      return json!({"valid": false, "stroke": self.terrain.change.is_some()});
    }
    self.refresh_grid_view();
    let node = self.scene.find(self.terrain.node).map(|n| n.id);
    let world = node.map_or(crate::math::Mat4::identity(), |id| self.scene.world_of(id));
    let c = world.inverse().point(self.terrain.brush_point);
    let mode = self.brush_mode_now();
    let stroke = self.terrain.change.is_some();
    // the grid as the element has it (during a stroke, the one being drawn)
    let empty = TerrainGrid {
      cell: self.terrain.brush.cell,
      ..Default::default()
    };
    // what it would do there, before it is done: the right button held, the ground raised or dug
    // that much; else what the left button does (a smoothing or flattening done)
    let ghost_of = if stroke {
      None
    } else if let Some(h) = sculpt {
      let m = if h > 0.0 {
        BrushMode::Raise
      } else {
        BrushMode::Dig
      };
      Some(self.terrain_stamp(m, c, h.abs(), c.y))
    } else if node.is_some() || matches!(mode, BrushMode::Paint | BrushMode::Wall) {
      let b = &self.terrain.brush;
      let amount = match mode {
        BrushMode::Wall => b.wall_height,
        BrushMode::Smooth | BrushMode::Flatten => 1.0,
        _ => 0.0,
      };
      let before = if node.is_some() {
        &self.terrain.grid_view
      } else {
        &empty
      };
      let height = self.terrain_start_height(before, node.unwrap_or(0));
      Some(self.terrain_stamp(mode, c, amount, height))
    } else {
      None
    };
    let t = &self.terrain;
    let grid = if stroke { &t.grid } else { &t.grid_view };
    let before = if node.is_some() { grid } else { &empty };
    let ghost = if stroke {
      t.stroke_preview
        .then(|| ghost(&t.stroke_before, &t.grid, t.stroke_cells, mode))
    } else {
      ghost_of.and_then(|s| preview(before, &s))
    };
    // the ground under the square, sampled: (steps + 1)^2 heights, row by row along x
    let size = t.brush.size;
    let half = size * 0.5;
    let steps = ((size / 0.5) as i32).clamp(4, 64);
    let ground = |x: f32, z: f32| {
      node
        .and_then(|_| terrain_height(grid, x, z))
        .map_or(c.y, mm)
    };
    let heights: Option<Vec<f32>> = node.map(|_| {
      let mut h = Vec::with_capacity(((steps + 1) * (steps + 1)) as usize);
      for k in 0..=steps {
        for i in 0..=steps {
          let s = size / steps as f32;
          h.push(ground(c.x - half + s * i as f32, c.z - half + s * k as f32));
        }
      }
      h
    });
    let cross = [
      ground(c.x - 0.3, c.z),
      ground(c.x + 0.3, c.z),
      ground(c.x, c.z - 0.3),
      ground(c.x, c.z + 0.3),
    ];
    let magnet = t.magnet_valid.then(|| {
      let name = self
        .scene
        .find(t.magnet_node)
        .map_or(String::new(), |n| n.name.clone());
      json!({
        "raw": t.brush_raw,
        "edge": t.magnet_edge + Vec3::new(0.0, 0.05, 0.0),
        "name": name,
        "text": crate::strf!(tr("terrain.magnet.on"), name.as_str()),
      })
    });
    // its height next to it (nothing when it changes nothing: erasing over the void)
    let sculpt = sculpt.filter(|_| ghost.is_some());
    json!({
      "valid": true,
      "point": t.brush_point,
      "world": world,
      "center": c,
      "size": size,
      "mode": brush_mode_name(mode),
      "steps": steps,
      "heights": heights,
      "cross": cross,
      "magnet": magnet,
      "ghost": ghost,
      "sculpt": sculpt,
      "stroke": stroke,
    })
  }

  /// Hills made at once (a fractal noise) around `at`, in the terrain painted (a new one when there
  /// is none), one undo step
  pub fn terrain_generate(&mut self, at: Vec3) {
    if !self.document_open() {
      return;
    }
    self.terrain_sync();
    let mut change = NodeChange::new(&self.scene);
    let existing = self
      .scene
      .find(self.terrain.node)
      .and_then(|n| node_terrain(n).map(|g| (n.id, g)));
    let made = existing.is_none();
    let (id, mut g) = match existing {
      Some((id, g)) => {
        change.save(&self.scene, id);
        (id, g)
      }
      None => {
        let id = self.scene.next_id;
        change.save(&self.scene, id);
        let g = TerrainGrid {
          cell: self.terrain.brush.cell,
          looks: vec![self.terrain_look()],
          ..Default::default()
        };
        let node = Node {
          id,
          name: terrain_name(&self.scene),
          kind: NodeKind::Render,
          ..Default::default()
        };
        (self.scene.add(node), g)
      }
    };
    self.terrain.node = id;
    let local = self.scene.world_of(id).inverse().point(at);
    let look = self.terrain_look();
    let look = g.add_look(&look);
    let b = &self.terrain.brush;
    generate_terrain(
      &mut g,
      local,
      b.gen_size,
      b.hills,
      b.roughness,
      b.seed,
      look,
    );
    if let Some(n) = self.scene.find_mut(id) {
      n.extras["og_terrain_grid"] = g.to_json();
      n.mesh = Some(make_terrain_mesh(&g));
    }
    light_like_surroundings(&mut self.scene, &[id], K_LIGHT_MAX_VERTICES);
    let label = tr(if made {
      "undo.terrain.new"
    } else {
      "undo.terrain"
    });
    self.history.push(change.commit(&self.scene, label, None));
    self.scene.begin_frame();
  }

  /// The whole terrain painted in the style chosen (its floor and wall textures), one undo step
  fn terrain_apply_style(&mut self) {
    let Some((before, mut g)) = self
      .scene
      .find(self.terrain.node)
      .and_then(|n| node_terrain(n).map(|g| (n.clone(), g)))
    else {
      return;
    };
    g.looks = vec![self.terrain_look()];
    g.look.fill(0);
    let id = before.id;
    if let Some(n) = self.scene.find_mut(id) {
      n.extras["og_terrain_grid"] = g.to_json();
      n.mesh = Some(make_terrain_mesh(&g));
    }
    light_like_surroundings(&mut self.scene, &[id], K_LIGHT_MAX_VERTICES);
    self.commit_node_edit(before, "undo.terrain");
  }

  /// The brush as get_state shows it (its window's settings, the terrain painted, the style)
  pub(crate) fn brush_state(&self) -> Value {
    let t = &self.terrain;
    let b = &t.brush;
    let painting = self
      .scene
      .find(t.node)
      .map_or(String::new(), |n| n.name.clone());
    // the style: measured (its textures), its decor being read, or that cannot be
    let level = &self.placing.style_level;
    let style = match self.placing.styles.get(level) {
      Some(s) => {
        let floor = (s.floor.material >= 0).then(|| s.floor.texture.clone());
        let wall = if s.wall.material >= 0 {
          Some(s.wall.texture.clone())
        } else {
          floor.clone()
        };
        json!({"status": "ready", "floor": floor, "wall": wall, "valid": s.valid()})
      }
      None => match self.library.level(level) {
        Some(l) if self.catalog.store.failed_level(&l.background) => json!({"status": "failed"}),
        Some(_) => json!({"status": "reading"}),
        None => Value::Null,
      },
    };
    let textures = self.catalog.index.ready();
    json!({
      "mode": brush_mode_name(b.mode),
      "size": b.size,
      "strength": b.strength,
      "wall_height": b.wall_height,
      "cell": b.cell,
      "relief": b.relief,
      "magnet": b.magnet,
      "gen_size": b.gen_size,
      "hills": b.hills,
      "roughness": b.roughness,
      "seed": b.seed,
      "floor_texture": b.floor_texture,
      "wall_texture": b.wall_texture,
      "style_level": level,
      "style": style,
      "textures_ready": textures,
      "node": if painting.is_empty() { 0 } else { t.node },
      "painting": painting,
      "stroke": t.change.is_some(),
    })
  }

  /// set_brush {mode, size, strength, wall_height, cell, relief, magnet, gen_size, hills,
  /// roughness, seed, floor_texture, wall_texture, style_level}: the keys given (the window's
  /// ranges)
  pub(crate) fn set_brush(&mut self, a: &Value) {
    let b = &mut self.terrain.brush;
    if let Some(m) = a
      .get("mode")
      .and_then(Value::as_str)
      .and_then(brush_mode_from_name)
    {
      b.mode = m;
    }
    let f = |k: &str, lo: f32, hi: f32, v: &mut f32| {
      if let Some(x) = num(a, k) {
        *v = x.clamp(lo, hi);
      }
    };
    f("size", 0.5, 64.0, &mut b.size);
    f("strength", 0.1, 20.0, &mut b.strength);
    f("wall_height", 0.5, 30.0, &mut b.wall_height);
    f("cell", 0.5, 4.0, &mut b.cell);
    f("hills", 0.0, 80.0, &mut b.hills);
    f("roughness", 0.0, 1.0, &mut b.roughness);
    if let Some(g) = a
      .get("gen_size")
      .and_then(|v| serde_json::from_value::<[f32; 2]>(v.clone()).ok())
    {
      b.gen_size = Vec2::new(g[0].clamp(8.0, 512.0), g[1].clamp(8.0, 512.0));
    }
    for (k, v) in [("relief", &mut b.relief), ("magnet", &mut b.magnet)] {
      if let Some(x) = a.get(k).and_then(Value::as_bool) {
        *v = x;
      }
    }
    if let Some(s) = a.get("seed").and_then(Value::as_i64) {
      b.seed = s as i32;
    }
    for (k, v) in [
      ("floor_texture", &mut b.floor_texture),
      ("wall_texture", &mut b.wall_texture),
    ] {
      if let Some(x) = a.get(k).and_then(Value::as_str) {
        *v = x.to_string();
      }
    }
    if let Some(level) = a.get("style_level").and_then(Value::as_str)
      && level != self.placing.style_level
    {
      // the textures chosen were that level's
      self.placing.style_level = level.to_string();
      self.terrain.brush.floor_texture.clear();
      self.terrain.brush.wall_texture.clear();
      if !level.is_empty() {
        self.level_style(level);
      }
    }
  }

  /// The brush's calls (api.rs): its settings, the strokes the viewport makes, the window's
  /// buttons, the level's textures to choose from
  pub(crate) fn terrain_call(&mut self, name: &str, a: &Value) -> Result<Value, String> {
    if let Some(shift) = a.get("shift").and_then(Value::as_bool) {
      self.terrain.shift = shift;
    }
    let dt = num(a, "dt").unwrap_or(1.0 / 60.0);
    match name {
      "start_terrain_brush" => return Ok(json!(self.start_terrain_brush())),
      "stop_terrain_brush" => self.stop_terrain_brush(),
      "set_brush" => {
        self.set_brush(a);
        return Ok(self.brush_state());
      }
      // the mouse moved over the view (no ray: out of it)
      "brush_hover" => self.brush_update(ray_of(a)),
      // the left button pressed: a stroke starts where the brush is (and is applied there)
      "brush_press" => {
        self.brush_update(ray_of(a));
        let started = self.terrain.change.is_some()
          || (self.terrain.brush_valid && self.terrain_begin_stroke(self.brush_mode_now()));
        if started {
          self.brush_held(dt);
        }
      }
      // held: the brush applied along the way
      "brush_drag" => {
        self.brush_update(ray_of(a));
        self.brush_held(dt);
      }
      "brush_release" => {
        if self.terrain.change.is_some() {
          self.terrain_end_stroke();
        }
      }
      "brush_cancel" => {
        self.terrain_cancel_stroke();
      }
      // the right button held, the mouse `px` pixels up (down: negative) since it was pressed;
      // `release`: let go, the ground raised or dug
      "brush_sculpt" => {
        let px = num(a, "px").unwrap_or(0.0);
        let h = self.terrain_sculpt_height(
          px,
          num(a, "fov").unwrap_or(self.camera.fov),
          num(a, "view_height").unwrap_or(1.0),
        );
        if a.get("release").and_then(Value::as_bool).unwrap_or(false) {
          self.terrain_sculpt(h);
        } else {
          return Ok(self.brush_overlay(Some(h)));
        }
      }
      // where the brush is (else in front of the camera), or `at`
      "terrain_generate" => {
        let at = vec3(a, "at").unwrap_or_else(|| {
          if self.terrain.brush_valid {
            self.terrain.brush_point
          } else {
            self.camera.pos + self.camera.forward() * 30.0
          }
        });
        self.terrain_generate(at);
        return Ok(Value::Null);
      }
      "terrain_apply_style" => {
        self.terrain_apply_style();
        return Ok(Value::Null);
      }
      // the next stroke makes another terrain
      "terrain_start_new" => {
        self.terrain.node = 0;
        self.select_only(0);
        return Ok(Value::Null);
      }
      // {level} -> {ready (the library's index), textures: [{name, uses, background}]} (their
      // pictures: the protocol's palette?background=&name=)
      "level_textures" => {
        let level = a.get("level").and_then(Value::as_str).unwrap_or_default();
        let textures = self.level_textures(level);
        return Ok(json!({"ready": self.catalog.index.ready(), "textures": textures}));
      }
      _ => return Err(format!("unknown call: {name}")),
    }
    Ok(self.brush_overlay(None))
  }

  /// The MCP tool `terrain` (remote_tools.cpp): one stroke of the brush, along a path when `to` is
  /// given, or hills (mode "generate"), in the terrain `id` (else a new one), one undo step;
  /// answers {id, changed, cells, walls, cell, style}. A `style` waits for its level's decor.
  pub(crate) fn tool_terrain(&mut self, a: &Value) -> Reply {
    if !self.document_open() {
      return Err("no document: open_level or new_level first".to_string()).into();
    }
    let style = a
      .get("style")
      .and_then(Value::as_str)
      .unwrap_or_default()
      .to_string();
    if !style.is_empty() {
      // the level style: measured once its decor is read
      let Some(bg) = self.library.level(&style).map(|l| l.background.clone()) else {
        return Err(format!("style: no level '{style}' (see list_levels)")).into();
      };
      if self.level_style(&style).is_none() {
        if self.catalog.store.failed_level(&bg) {
          return Err(format!("style: the decor of {style} cannot be read")).into();
        }
        let args = a.clone();
        return Reply::wait(move |ed| {
          (ed.level_style(&style).is_some() || ed.catalog.store.failed_level(&bg))
            .then(|| ed.tool_terrain(&args))
        });
      }
      self.placing.style_level = style;
    }
    // textures of that level instead of the style's (the brush window's choice)
    let b = &mut self.terrain.brush;
    for (k, v) in [
      ("floor_texture", &mut b.floor_texture),
      ("wall_texture", &mut b.wall_texture),
    ] {
      if let Some(t) = a.get(k).and_then(Value::as_str) {
        *v = t.to_string();
      }
    }
    let mode = a.get("mode").and_then(Value::as_str).unwrap_or("paint");
    if mode != "generate" && brush_mode_from_name(mode).is_none() {
      return Err(
        "mode: paint, erase, raise, dig, smooth, flatten, wall, unwall or generate".to_string(),
      )
      .into();
    }
    let Some(at) = vec3(a, "position") else {
      return Err("position: [x, y, z] in meters".to_string()).into();
    };
    let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    let existing = self.scene.find(id).and_then(node_terrain);
    if id != 0 && existing.is_none() {
      return Err("id: a terrain (made by this tool or the brush)".to_string()).into();
    }
    let mut change = NodeChange::new(&self.scene);
    let made = existing.is_none();
    let (id, mut g) = match existing {
      Some(g) => {
        change.save(&self.scene, id);
        (id, g)
      }
      None => {
        let nid = self.scene.next_id;
        change.save(&self.scene, nid);
        let g = TerrainGrid {
          cell: num(a, "cell")
            .unwrap_or(self.terrain.brush.cell)
            .clamp(0.25, 8.0),
          looks: vec![self.terrain_look()],
          ..Default::default()
        };
        let node = Node {
          id: nid,
          name: terrain_name(&self.scene),
          kind: NodeKind::Render,
          ..Default::default()
        };
        (self.scene.add(node), g)
      }
    };
    let inv = self.scene.world_of(id).inverse();
    let local = inv.point(at);
    let seed = a.get("seed").and_then(Value::as_i64).unwrap_or(1) as i32;
    let mut changed = false;
    if let Some(m) = brush_mode_from_name(mode) {
      let mut st = BrushStroke {
        mode: m,
        size: num(a, "size").unwrap_or(4.0).clamp(0.25, 256.0),
        height: num(a, "height").map_or(local.y, |h| inv.point(Vec3::new(at.x, h, at.z)).y),
        seed,
        ..Default::default()
      };
      // the cells painted take the style asked
      let look = self.terrain_look();
      st.look = g.add_look(&look);
      let shaping = matches!(m, BrushMode::Smooth | BrushMode::Flatten);
      st.amount = num(a, "amount").unwrap_or(match m {
        BrushMode::Wall => 4.0,
        _ if shaping => 0.5,
        _ => 1.0,
      });
      let relief = a.get("relief").and_then(Value::as_bool).unwrap_or(true);
      if m == BrushMode::Paint
        && relief
        && let Some(s) = self.placing.styles.get(&self.placing.style_level)
      {
        st.bumps = s.bumps;
        st.bump_size = s.bump_size.max(0.5);
      }
      // along a path (a road, a wall): a stamp every quarter of the brush's size
      let to = inv.point(vec3(a, "to").unwrap_or(at));
      let dist = length(Vec3::new(to.x - local.x, 0.0, to.z - local.z));
      let step = (st.size * 0.25).max(g.cell * 0.5);
      let stamps = ((dist / step).ceil() as i32).clamp(0, 4096);
      for k in 0..=stamps {
        st.center = if stamps > 0 {
          local + (to - local) * (k as f32 / stamps as f32)
        } else {
          local
        };
        changed |= apply_brush(&mut g, &st);
      }
    } else {
      let area: [f32; 2] = a
        .get("area")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or([64.0, 64.0]);
      let look = self.terrain_look();
      let look = g.add_look(&look);
      generate_terrain(
        &mut g,
        local,
        Vec2::new(area[0].clamp(4.0, 1024.0), area[1].clamp(4.0, 1024.0)),
        num(a, "hills").unwrap_or(12.0).clamp(0.0, 200.0),
        num(a, "roughness").unwrap_or(0.5).clamp(0.0, 1.0),
        seed,
        look,
      );
      changed = true;
    }
    if let Some(n) = self.scene.find_mut(id) {
      n.extras["og_terrain_grid"] = g.to_json();
      n.mesh = Some(make_terrain_mesh(&g));
    }
    // lit like the level around it
    light_like_surroundings(&mut self.scene, &[id], K_LIGHT_MAX_VERTICES);
    if made || changed {
      let label = tr(if made {
        "undo.terrain.new"
      } else {
        "undo.terrain"
      });
      self.history.push(change.commit(&self.scene, label, None));
    }
    self.scene.begin_frame();
    let cells = g.cells.iter().filter(|&&c| c != TerrainGrid::NONE).count();
    let walls = g.cells.iter().filter(|&&c| c == TerrainGrid::WALL).count();
    Ok(json!({
      "id": id,
      "changed": changed,
      "cells": cells,
      "walls": walls,
      "cell": g.cell,
      "style": self.placing.style_level,
    }))
    .into()
  }
}

#[cfg(test)]
mod tests {
  use serde_json::{Value, json};

  use super::super::Editor;
  use crate::scene::terrain::{TerrainGrid, node_terrain};

  fn editor() -> Editor {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.new_level();
    ed
  }

  /// the brush over (x, z), the camera 10 m above it looking down
  fn ray(x: f32, z: f32) -> Value {
    json!({"origin": [x, 10.0, z], "dir": [0.0, -1.0, 0.0]})
  }

  fn call(ed: &mut Editor, name: &str, mut a: Value) -> Value {
    if a.is_null() {
      a = json!({});
    }
    ed.call(name, &a).unwrap()
  }

  fn terrains(ed: &Editor) -> Vec<(u32, TerrainGrid)> {
    ed.scene
      .nodes
      .iter()
      .filter_map(|n| node_terrain(n).map(|g| (n.id, g)))
      .collect()
  }

  fn cells(g: &TerrainGrid) -> usize {
    g.cells.iter().filter(|&&c| c != TerrainGrid::NONE).count()
  }

  #[test]
  fn a_stroke_shown_until_let_go_then_one_undo_step() {
    let mut ed = editor();
    assert_eq!(
      call(&mut ed, "start_terrain_brush", Value::Null),
      json!(true)
    );
    assert_eq!(ed.state_tool(), "terrain");
    // before the click: the square on the plane, what it would lay see-through
    let o = call(&mut ed, "brush_hover", ray(0.0, 0.0));
    assert!(o["valid"].as_bool().unwrap() && o["ghost"]["cells"].as_array().unwrap().len() >= 8);
    assert!(
      terrains(&ed).is_empty() && !ed.history.can_undo(),
      "the hover lays nothing"
    );
    // pressed and dragged: still only shown (the terrain element made, nothing in it, no undo)
    call(&mut ed, "brush_press", ray(0.0, 0.0));
    let o = call(
      &mut ed,
      "brush_drag",
      json!({"origin": [6.0, 10.0, 0.0], "dir": [0.0, -1.0, 0.0], "dt": 0.016}),
    );
    let shown = o["ghost"]["cells"].as_array().unwrap().len() / 8;
    let t = terrains(&ed);
    assert!(t.len() == 1 && cells(&t[0].1) == 0 && !ed.history.can_undo());
    assert!(
      shown > 16,
      "the ghost shows the whole stroke ({shown} cells)"
    );
    let mesh = ed.scene.find(t[0].0).unwrap().mesh.clone().unwrap();
    assert_eq!(
      mesh.tri_count(),
      0,
      "the preview is not applied before release"
    );
    // let go: laid, one undo step
    call(&mut ed, "brush_release", Value::Null);
    let t = terrains(&ed);
    assert!(cells(&t[0].1) == shown && ed.history.labels().len() == 1);
    assert_eq!(ed.history.undo_label(), crate::i18n::tr("undo.terrain.new"));
    assert!(
      ed.scene
        .find(t[0].0)
        .unwrap()
        .mesh
        .as_ref()
        .unwrap()
        .tri_count()
        > 0
    );
    // a second stroke over it goes into the same terrain
    call(&mut ed, "brush_press", ray(-6.0, 0.0));
    call(&mut ed, "brush_release", Value::Null);
    let t2 = terrains(&ed);
    assert!(t2.len() == 1 && cells(&t2[0].1) > shown && ed.history.labels().len() == 2);
    // undone, redone: one stroke each
    call(&mut ed, "undo", Value::Null);
    assert_eq!(cells(&terrains(&ed)[0].1), shown);
    call(&mut ed, "undo", Value::Null);
    assert!(terrains(&ed).is_empty());
    call(&mut ed, "redo", Value::Null);
    assert_eq!(cells(&terrains(&ed)[0].1), shown);
  }

  #[test]
  fn escape_drops_a_stroke_shown_and_erasing_lays_nothing() {
    let mut ed = editor();
    call(&mut ed, "start_terrain_brush", Value::Null);
    // Esc while the button is held: nothing done, no element left
    call(&mut ed, "brush_press", ray(0.0, 0.0));
    assert_eq!(terrains(&ed).len(), 1);
    call(&mut ed, "escape", Value::Null);
    assert!(terrains(&ed).is_empty() && !ed.history.can_undo());
    assert_eq!(
      ed.state_tool(),
      "terrain",
      "Esc dropped the stroke, the brush stays"
    );
    // erasing over the void: no terrain made, by the left button nor by the right one raising
    call(&mut ed, "set_brush", json!({"mode": "erase"}));
    call(&mut ed, "brush_press", ray(0.0, 0.0));
    call(&mut ed, "brush_release", Value::Null);
    call(&mut ed, "brush_hover", ray(0.0, 0.0));
    let o = call(
      &mut ed,
      "brush_sculpt",
      json!({"px": 100.0, "fov": 60.0, "view_height": 600.0}),
    );
    assert!(
      o["ghost"].is_null() && o["sculpt"].is_null(),
      "nothing to show: it would change nothing"
    );
    call(
      &mut ed,
      "brush_sculpt",
      json!({"px": 100.0, "fov": 60.0, "view_height": 600.0, "release": true}),
    );
    assert!(terrains(&ed).is_empty() && !ed.history.can_undo());
    // painted ground, then the right button erasing raises what is there, never beside it
    call(&mut ed, "set_brush", json!({"mode": "paint"}));
    call(&mut ed, "brush_press", ray(0.0, 0.0));
    call(&mut ed, "brush_release", Value::Null);
    let laid = cells(&terrains(&ed)[0].1);
    call(&mut ed, "set_brush", json!({"mode": "erase"}));
    call(&mut ed, "brush_hover", ray(20.0, 20.0));
    call(
      &mut ed,
      "brush_sculpt",
      json!({"px": 100.0, "fov": 60.0, "view_height": 600.0, "release": true}),
    );
    assert_eq!(cells(&terrains(&ed)[0].1), laid, "erasing never adds");
    assert_eq!(ed.history.labels().len(), 1);
    // over the ground: raised, the height told by the cursor, one undo step
    let o = call(&mut ed, "brush_hover", ray(0.0, 0.0));
    assert!(o["valid"].as_bool().unwrap());
    let o = call(
      &mut ed,
      "brush_sculpt",
      json!({"px": 100.0, "fov": 60.0, "view_height": 600.0}),
    );
    let h = o["sculpt"].as_f64().unwrap();
    assert!(h > 1.5 && h < 2.5 && !o["ghost"].is_null(), "{h}");
    call(
      &mut ed,
      "brush_sculpt",
      json!({"px": 100.0, "fov": 60.0, "view_height": 600.0, "release": true}),
    );
    let (_, g) = &terrains(&ed)[0];
    assert!(g.height.iter().copied().fold(0f32, f32::max) > 1.5 && ed.history.labels().len() == 2);
  }

  #[test]
  fn magnet_keeps_the_level_of_the_ground_beside() {
    // a terrain at 3 m: the magnet brush beside it lays its ground at 3 m, wherever the cursor is
    let mut ed = editor();
    call(&mut ed, "start_terrain_brush", Value::Null);
    call(
      &mut ed,
      "brush_press",
      json!({"origin": [0.0, 13.0, 0.0], "dir": [0.0, -1.0, 0.0]}),
    );
    call(&mut ed, "brush_release", Value::Null);
    call(&mut ed, "set_brush", json!({"magnet": true}));
    // the cursor on the plane below, 4 m away: the square keeps its x, z
    let o = call(
      &mut ed,
      "brush_hover",
      json!({"origin": [5.0, 10.0, 0.0], "dir": [0.0, -1.0, 0.0]}),
    );
    let p = &o["point"];
    assert!(!o["magnet"].is_null() && (p[0].as_f64().unwrap() - 5.0).abs() < 1e-4);
    assert!(
      p[2].as_f64().unwrap().abs() < 1e-4,
      "the square stays under the cursor"
    );
    // generated hills: one undo step more
    call(
      &mut ed,
      "terrain_generate",
      json!({"at": [100.0, 0.0, 0.0]}),
    );
    assert_eq!(ed.history.labels().len(), 2);
    let state = ed.brush_state();
    assert!(state["magnet"].as_bool().unwrap() && state["painting"] == "Terrain");
  }

  impl Editor {
    fn state_tool(&self) -> String {
      serde_json::to_value(self.tool)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
    }
  }
}
