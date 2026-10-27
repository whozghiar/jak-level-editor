//! Structures made in the style of a level (Build panel "A house like in...", the MCP's
//! build_house and building_kit): what its buildings are made of, read from its decor (its
//! windows, doors, pipes, stains, roof plates, chimneys, awnings, signs, lamps: the game's parts, by
//! their names; the textures of its buildings' walls, roofs and frames), then a house put together
//! with them: walls of the level's texture on a plinth, bands between its floors and pillars at its
//! corners (relief), the game's windows and door in their openings, a pipe down a wall, wear
//! (stains, holes, broken plaster) on the walls, a roof of the level's roof texture, a chimney, an
//! awning, a lamp and a sign by the door. One undo step, one group.

use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::PI;
use std::sync::Arc;

use serde_json::{Value, json};

use super::Editor;
use super::groups::{group_tag, new_group_id, set_group_tag};
use crate::history::NodeChange;

use super::placement::PlacementKind;
use crate::i18n::tr;
use crate::io::asset_library::merge_mesh;
use crate::io::catalog::{CatalogPart, PartKind, list_parts, part_mesh};
use crate::io::prefabs::{new_prefab_path, save_prefab};
use crate::math::{Mat4, Quat, Vec2, Vec3, Vec4, cross, length, vmax, vmin};
use crate::math::{Ray, normalize};
use crate::scene::level_style::{Surface, measure_style};
use crate::scene::pick::{PickFilter, pick};
use crate::scene::relight::K_KEEP_LIGHT;
use crate::scene::shapes::{ShapeParams, ShapePreset};
use crate::scene::{AlphaMode, BlendMode, Mesh, NodeKind, Primitive, Scene};
use crate::strf;

/// What a part of a level's decor is for, on a building
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
  Window,
  /// a window's glass, set in its frame
  Glass,
  /// a shade over a window
  Shade,
  Door,
  Doorframe,
  Pipe,
  /// a pipe's ring or bracket
  PipeRing,
  /// stains, scorch marks, bullet holes, broken plaster: wear on a wall
  Wear,
  /// plates, slats, panels laid on a roof
  Roof,
  Chimney,
  Awning,
  Sign,
  Lamp,
  Vine,
  /// what stands out of a wall: the industrial zone's bulges, Spargus' wall panels and armor
  /// plates, the slums' fortress bumps
  Relief,
  /// what stands up a wall's joints: the industrial facades' joints, Spargus' wall joints, the
  /// slums' fortress columns, Haven's corner joints and seam covers
  Column,
  /// metal, wood and stone beams
  Beam,
  /// what lies on a floor and stands out of it: vents, grates, small rocks
  GroundDetail,
  /// what is painted on a floor: dirt patches, pebbles
  GroundDecal,
  /// a level's building blocks (facades): their textures are its buildings'
  Facade,
}

impl Role {
  pub fn name(self) -> &'static str {
    match self {
      Role::Window => "window",
      Role::Glass => "glass",
      Role::Shade => "shade",
      Role::Door => "door",
      Role::Doorframe => "doorframe",
      Role::Pipe => "pipe",
      Role::PipeRing => "pipe_ring",
      Role::Wear => "wear",
      Role::Roof => "roof",
      Role::Chimney => "chimney",
      Role::Awning => "awning",
      Role::Sign => "sign",
      Role::Lamp => "lamp",
      Role::Vine => "vine",
      Role::Relief => "relief",
      Role::Column => "column",
      Role::Beam => "beam",
      Role::GroundDetail => "ground_detail",
      Role::GroundDecal => "ground_decal",
      Role::Facade => "facade",
    }
  }
}

/// the words of a part's name that give its role (the first matching), and those that rule it out
const K_ROLES: &[(Role, &[&str], &[&str])] = &[
  (
    Role::Wear,
    &[
      "stain",
      "blotch",
      "scorch",
      "bullet-hole",
      "plaster-broken",
      "brick-showing",
      "overlay-",
      "wall-broken",
      "rebar",
      "decal",
      "tribal-marking",
    ],
    &["ground", "dirt"],
  ),
  (Role::Glass, &["window-glass"], &["door"]),
  (Role::Shade, &["window-shade"], &["door"]),
  (
    Role::Window,
    &["window-frame", "slum-window"],
    &["door", "stain"],
  ),
  (Role::Doorframe, &["doorframe"], &["windowframe"]),
  (Role::Door, &["door"], &["window", "frame", "large"]),
  (
    Role::PipeRing,
    &[
      "pipe-ring",
      "pipe-bracket",
      "medpipe-ring",
      "smallpipe-ring",
      "tinypipe-ring",
    ],
    &["big"],
  ),
  (
    Role::Pipe,
    &[
      "smallpipe-straight",
      "joint-medpipe-01",
      "wascity-pipe-01",
      "tinypipe-straight",
    ],
    &[],
  ),
  (
    Role::Roof,
    &["roof-"],
    &[
      "blocker",
      "structure",
      "spine",
      "support",
      "squashed",
      "corner",
    ],
  ),
  (Role::Chimney, &["chimney"], &["hires"]),
  (Role::Awning, &["awning"], &["pole"]),
  (Role::Sign, &["hangsign"], &[]),
  (Role::Lamp, &["wall-lamp", "slum-lamp", "buldge-lamp"], &[]),
  (Role::Vine, &["wallvine"], &[]),
  (
    Role::Relief,
    &[
      "deco-bigbuldge",
      "deco-lilbuldge",
      "wallbuldge",
      "wall-bump",
      "wall-outer-panel",
      "armor-plate",
    ],
    &["broken", "light", "lamp", "top"],
  ),
  (
    Role::Column,
    &[
      "joint-bot",
      "joint-top",
      "wall-column",
      "outer-joint",
      "corner-joint",
      "seam-cover",
    ],
    &["ground", "cap", "bigpipe", "medpipe"],
  ),
  (
    Role::Beam,
    &["beam"],
    &["twisted", "bridgebeam", "tapered-large", "poke"],
  ),
  (
    Role::GroundDetail,
    &[
      "ground-vent",
      "bigvent",
      "base-vent",
      "vent-frame",
      "vent-bars",
      "ground-rocks-small",
      "groundlamp",
    ],
    &[],
  ),
  (
    Role::GroundDecal,
    &["dirt-patch", "dirt-straight", "dirt-pebbles"],
    &[],
  ),
  (
    Role::Facade,
    &["facade"],
    &["deco", "inside", "minaret", "round", "steps"],
  ),
];

/// the extras of a structure's anchor: how it is made (StructureParams)
pub const K_STRUCTURE: &str = "og_structure";

/// the levels whose windows and doors a level without its own borrows
const K_DONORS: &[&str] = &["ctygenb", "ctysluma"];

/// what a level's flat pieces may be that is no relief of its walls: plants, props, statues
const K_NOT_RELIEF: &[&str] = &[
  "tree",
  "limb",
  "leaf",
  "plant",
  "bush",
  "grass",
  "crate",
  "shelf",
  "jar",
  "platter",
  "barrel",
  "idol",
  "statue",
  "teardrop",
  "cage",
  "tombstone",
  "skull",
  "bone",
  "debris",
  "scythe",
  "sign",
  "window",
  "lite",
  "light",
  "lamp",
  "glow",
];

/// never on a house: broken, cut or other versions of a part, the ground's
const K_NEVER: &[&str] = &[
  "blasted",
  "hires",
  "collision",
  "fallen",
  "palace",
  "onintent",
  "stadium",
  "canal",
  "-parts",
  "-piece",
];

/// a roof's covering by its name (plates, panels, tiles), not what is under it (slats, spans)
fn covering(name: &str) -> bool {
  let n = name.to_lowercase();
  ["plate", "panel", "square", "tile", "-tri-"]
    .iter()
    .any(|w| n.contains(w))
}

pub fn role_of(name: &str) -> Option<Role> {
  let n = name.to_lowercase();
  let n = n.strip_suffix(".mb").unwrap_or(&n);
  if K_NEVER.iter().any(|w| n.contains(w)) {
    return None;
  }
  K_ROLES
    .iter()
    .find(|(_, any, none)| any.iter().any(|w| n.contains(w)) && !none.iter().any(|w| n.contains(w)))
    .map(|r| r.0)
}

/// A part of the kit, measured
#[derive(Clone, Debug)]
pub struct KitPart {
  pub role: Role,
  pub part: CatalogPart,
  /// its box in its own mesh (meters; x and z around 0, y from 0)
  pub raw: Vec3,
  /// the turn that sets its front towards +z (out of a wall facing +z), a pipe upright
  pub rot: Quat,
  /// its box once turned: x across, y up, z its depth
  pub size: Vec3,
  /// a roof's plates: the way they rise once turned (horizontal), and how steeply (radians)
  pub rise: Vec3,
  pub slope: f32,
  /// the level it comes from (a level without windows or doors borrows another's)
  pub level: String,
}

/// What a level's buildings are made of
#[derive(Clone, Debug, Default)]
pub struct BuildingKit {
  pub parts: Vec<KitPart>,
  /// their walls' (the level's facades'), their roofs' and their trims' (the frames') surfaces,
  /// the level's floors' (its ground), in the level's scene (material -1: none)
  pub wall: Surface,
  pub roof: Surface,
  pub trim: Surface,
  pub floor: Surface,
  /// the level's roofs are flat (Spargus') rather than sloped
  pub flat_roofs: bool,
  /// meters between floors, for a kit without windows nor doors to give it (0: from them)
  pub module: f32,
}

impl BuildingKit {
  /// the parts of a role, the most used in the level first (a roof's plates and panels before
  /// its slats and beams)
  pub fn of(&self, role: Role) -> Vec<usize> {
    let mut v: Vec<usize> = (0..self.parts.len())
      .filter(|&i| self.parts[i].role == role)
      .collect();
    v.sort_by_key(|&i| {
      (
        !covering(&self.parts[i].part.name),
        -self.parts[i].part.copies,
      )
    });
    v
  }

  /// {wall, roof, trim: {texture, tile}, flat_roofs, parts: {role: [{name, size, rotation_deg,
  /// copies}]}}: what the MCP's building_kit answers
  pub fn to_json(&self) -> Value {
    let mut parts = serde_json::Map::new();
    for p in &self.parts {
      if p.role == Role::Facade {
        continue;
      }
      let e = parts
        .entry(p.role.name())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .unwrap();
      let q = p.rot;
      let r = if q.x.abs() < 1e-4 && q.z.abs() < 1e-4 {
        Vec3::new(0.0, (2.0 * q.y.atan2(q.w)).to_degrees(), 0.0)
      } else {
        q.to_euler_deg()
      };
      let round = |v: f32| (v * 100.0).round() / 100.0;
      e.push(json!({
        "name": p.part.name.strip_suffix(".mb").unwrap_or(&p.part.name),
        "size": [round(p.size.x), round(p.size.y), round(p.size.z)],
        "rotation_deg": [r.x.round(), r.y.round(), r.z.round()],
        "copies_in_level": p.part.copies,
      }));
      if p.role == Role::Roof {
        e.last_mut().unwrap()["slope_deg"] = json!(p.slope.to_degrees().round());
      }
    }
    let surface = |s: &Surface| {
      (s.material >= 0)
        .then(|| json!({"texture": s.texture, "tile": (s.tile * 100.0).round() / 100.0}))
    };
    json!({
      "wall": surface(&self.wall),
      "roof": surface(&self.roof),
      "trim": surface(&self.trim),
      "floor": surface(&self.floor),
      "flat_roofs": self.flat_roofs,
      "parts": parts,
    })
  }
}

/// the faces of some meshes, by material: their area, tile and color summed
#[derive(Default, Clone, Copy)]
struct Area {
  area: f64,
  log_tile: f64,
  tile_area: f64,
  color: Vec4,
}

/// The largest surface of these parts (steep faces only when `steep`), its light not darker than
/// 0.3 (as the level's style has it)
fn main_surface(level: &Scene, parts: &[&CatalogPart], steep: bool) -> Surface {
  let mut bins: BTreeMap<i32, Area> = BTreeMap::new();
  for part in parts {
    let Some(mesh) = part_mesh(level, part) else {
      continue;
    };
    for p in &mesh.prims {
      let Some(m) = usize::try_from(p.material)
        .ok()
        .and_then(|m| level.materials.get(m))
      else {
        continue;
      };
      if m.texture < 0 || (m.alpha == AlphaMode::Blend && m.blend != BlendMode::Alpha) {
        continue;
      }
      let (uv, col) = (p.uv.len() == p.pos.len(), p.col.len() == p.pos.len());
      for t in p.idx.chunks_exact(3) {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let cr = cross(p.pos[b] - p.pos[a], p.pos[c] - p.pos[a]);
        let area = length(cr) * 0.5;
        if area < 1e-4 || (steep && cr.y.abs() > 0.7 * length(cr)) {
          continue;
        }
        let bin = bins.entry(p.material).or_default();
        bin.area += area as f64;
        if uv {
          let (ua, ub, uc) = (p.uv[a], p.uv[b], p.uv[c]);
          let uv_area = ((ub.x - ua.x) * (uc.y - ua.y) - (uc.x - ua.x) * (ub.y - ua.y)).abs() * 0.5;
          let tile = (area / uv_area.max(1e-9)).sqrt();
          if uv_area > 1e-6 && tile > 0.05 && tile < 200.0 {
            bin.log_tile += (area * tile.ln()) as f64;
            bin.tile_area += area as f64;
          }
        }
        if col {
          bin.color = bin.color + (p.col[a] + p.col[b] + p.col[c]) * (area / 3.0);
        }
      }
    }
  }
  let Some((&m, a)) = bins.iter().max_by(|x, y| x.1.area.total_cmp(&y.1.area)) else {
    return Surface::default();
  };
  let mut color = if a.color.w > 0.0 {
    a.color * (1.0 / a.area) as f32
  } else {
    Vec4::new(0.5, 0.5, 0.5, 1.0)
  };
  let bright = color.x.max(color.y).max(color.z);
  if bright < 0.3 {
    let k = 0.3 / bright.max(1e-3);
    color = Vec4::new(color.x * k, color.y * k, color.z * k, 1.0);
  }
  color.w = 1.0;
  let material = &level.materials[m as usize];
  Surface {
    material: m,
    texture: usize::try_from(material.texture)
      .ok()
      .and_then(|t| level.textures.get(t))
      .map_or(material.name.clone(), |t| t.name.clone()),
    tile: if a.tile_area > 0.0 {
      (a.log_tile / a.tile_area).exp() as f32
    } else {
      2.0
    },
    color,
    area: a.area,
  }
}

const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);

const fn splat(v: f32) -> Vec3 {
  Vec3::new(v, v, v)
}

fn yaw(rad: f32) -> Quat {
  Quat::axis_angle(UP, rad)
}

/// the box of a part turned by `r`, scaled by `s` in its own axes: its size
fn turned_box(raw: Vec3, r: Quat, s: Vec3) -> (Vec3, Vec3) {
  let (mut lo, mut hi) = (splat(f32::MAX), splat(f32::MIN));
  for i in 0..8 {
    let c = Vec3::new(
      if i & 1 != 0 { 0.5 } else { -0.5 } * raw.x * s.x,
      if i & 2 != 0 { raw.y * s.y } else { 0.0 },
      if i & 4 != 0 { 0.5 } else { -0.5 } * raw.z * s.z,
    );
    let p = r.rotate(c);
    lo = vmin(lo, p);
    hi = vmax(hi, p);
  }
  (lo, hi)
}

/// A part's size and the turn that sets its front towards +z: the thinner of its horizontal sides
/// is its depth, its front the side its faces look to (a decal has one face, a frame more on its
/// front); a pipe lying is turned upright
fn measure(level: &Scene, part: &CatalogPart, role: Role) -> Option<KitPart> {
  let mesh = part_mesh(level, part)?;
  let b = mesh.bounds();
  if !b.valid() {
    return None;
  }
  let s = b.hi - b.lo;
  let mut facing = Vec3::default();
  for p in &mesh.prims {
    for t in p.idx.chunks_exact(3) {
      let (a, bb, c) = (
        p.pos[t[0] as usize],
        p.pos[t[1] as usize],
        p.pos[t[2] as usize],
      );
      facing = facing + cross(bb - a, c - a);
    }
  }
  // its back where the level's walls are behind its copies (the extracted triangles' winding is
  // no guide, about half of them face in): what is hung on a wall (a lamp's arm, a sign, an
  // awning) on any side, what lies on one (a window, a door, a stain) along its thinner side
  let (x, z) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
  let back = match role {
    Role::Lamp | Role::Sign | Role::Awning | Role::Shade => {
      wall_behind(level, part, &mesh, &[x, -x, z, -z], 1.0)
    }
    Role::Window
    | Role::Glass
    | Role::Door
    | Role::Doorframe
    | Role::Wear
    | Role::Vine
    | Role::Relief
    | Role::Column => {
      let thin = if s.x < s.z { x } else { z };
      wall_behind(level, part, &mesh, &[thin, -thin], 3.0)
    }
    _ => None,
  };
  let rot = match (role, back) {
    (_, Some(b)) => {
      // the front (-back) towards +z
      let f = -b;
      yaw(f.x.atan2(f.z) * -1.0)
    }
    (role, None) => facing_turn(role, s, facing),
  };
  let (lo, hi) = turned_box(s, rot, splat(1.0));
  let (rise, slope) = if role == Role::Roof {
    roof_slope(&mesh, rot)
  } else {
    (Vec3::default(), 0.0)
  };
  Some(KitPart {
    role,
    part: part.clone(),
    raw: s,
    rot,
    size: hi - lo,
    rise,
    slope,
    level: String::new(),
  })
}

/// the turn of a part from the faces it shows: a pipe upright, a decal or a frame to +z
fn facing_turn(role: Role, s: Vec3, facing: Vec3) -> Quat {
  match role {
    Role::Pipe if s.y >= s.x.max(s.z) => Quat::default(),
    // lying along x: x up; along z: z up
    Role::Pipe if s.x > s.z => Quat::axis_angle(Vec3::new(0.0, 0.0, 1.0), PI / 2.0),
    Role::Pipe => Quat::axis_angle(Vec3::new(1.0, 0.0, 0.0), -PI / 2.0),
    Role::Roof
    | Role::Chimney
    | Role::Facade
    | Role::PipeRing
    | Role::Beam
    | Role::GroundDetail
    | Role::GroundDecal => Quat::default(),
    // the front towards -x, +x, -z or +z: turned to +z
    _ if s.x < s.z => yaw(if facing.x >= 0.0 { -PI / 2.0 } else { PI / 2.0 }),
    _ if facing.z < 0.0 => yaw(PI),
    _ => Quat::default(),
  }
}

/// Where the level's walls are behind a part's copies: among `dirs` (its own axes), the side a wall
/// is nearest (within its half size and `extra` meters) for clearly most of them; None when it is
/// not clear
fn wall_behind(
  level: &Scene,
  part: &CatalogPart,
  mesh: &Mesh,
  dirs: &[Vec3],
  extra: f32,
) -> Option<Vec3> {
  let first = level.nodes.iter().position(|n| n.name == part.mesh_node)?;
  let parent = level.nodes[first].parent;
  let cache = level.cache();
  let b = mesh.bounds();
  let center = b.center();
  let half = (b.hi - b.lo) * 0.5;
  let mut votes = vec![0; dirs.len()];
  let copies = level
    .nodes
    .iter()
    .enumerate()
    .filter(|(i, n)| {
      n.kind == NodeKind::Render
        && n.mesh.is_some()
        && (*i == first || (part.copies > 1 && n.parent == parent))
    })
    .take(8);
  for (i, n) in copies {
    let w = &cache.worlds[i];
    let f = PickFilter {
      actors: false,
      include_locked: true,
      exclude_list: vec![n.id],
      ..Default::default()
    };
    let o = w.point(center);
    let mut best = (f32::MAX, dirs.len());
    for (k, d) in dirs.iter().enumerate() {
      let reach = (half.x * d.x.abs() + half.z * d.z.abs()) + extra;
      let wd = normalize(w.dir(*d));
      let hit = pick(level, &Ray::new(o, wd), &f);
      // its distance in the part's own size (a scaled copy)
      let scale = length(w.dir(*d)).max(1e-3);
      if hit.hit() && hit.t / scale < reach && hit.t / scale < best.0 {
        best = (hit.t / scale, k);
      }
    }
    if best.1 < dirs.len() {
      votes[best.1] += 1;
    }
  }
  let (k, &n) = votes.iter().enumerate().max_by_key(|(_, v)| **v)?;
  let others = votes.iter().sum::<i32>() - n;
  (n >= 2 && n >= 2 * others).then(|| dirs[k])
}

/// How a roof's plate rises once turned: the horizontal way its top goes up, and the angle
fn roof_slope(mesh: &Mesh, rot: Quat) -> (Vec3, f32) {
  // the heights against x and z (least squares over its vertices)
  let (mut n, mut sx, mut sy, mut sz) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
  let (mut sxx, mut szz, mut sxy, mut szy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
  for p in &mesh.prims {
    for v in &p.pos {
      let v = rot.rotate(*v);
      let (x, y, z) = (v.x as f64, v.y as f64, v.z as f64);
      n += 1.0;
      sx += x;
      sy += y;
      sz += z;
      sxx += x * x;
      szz += z * z;
      sxy += x * y;
      szy += z * y;
    }
  }
  if n < 3.0 {
    return (Vec3::default(), 0.0);
  }
  let gx = (sxy - sx * sy / n) / (sxx - sx * sx / n).max(1e-6);
  let gz = (szy - sz * sy / n) / (szz - sz * sz / n).max(1e-6);
  let g = (gx * gx + gz * gz).sqrt();
  if g < 1e-3 {
    return (Vec3::default(), 0.0);
  }
  (
    Vec3::new((gx / g) as f32, 0.0, (gz / g) as f32),
    (g as f32).atan(),
  )
}

/// The kit of a level, from its decor (its scene as the library has it)
pub fn building_kit(level: &Scene) -> BuildingKit {
  let mut kit = BuildingKit::default();
  let parts = list_parts(level);
  let mut by_role: BTreeMap<Role, Vec<&CatalogPart>> = BTreeMap::new();
  for p in &parts {
    if p.kind != PartKind::Prototype || p.triangles == 0 {
      continue;
    }
    if let Some(role) = role_of(&p.name) {
      by_role.entry(role).or_default().push(p);
    }
  }
  for (&role, list) in &by_role {
    for p in list {
      let Some(k) = measure(level, p, role) else {
        continue;
      };
      // what a house can take (meters: the game's buildings are large, a floor about 11 m)
      let s = k.size;
      let fits = match role {
        Role::Window | Role::Glass => s.y > 0.5 && s.y < 12.0 && s.x < 12.0,
        Role::Door | Role::Doorframe => s.y > 1.5 && s.y < 14.0 && s.x < 12.0,
        Role::Pipe => s.x.min(s.z) < 4.0 && s.y > 1.0,
        Role::Wear => s.x < 20.0 && s.y < 20.0 && s.z < 4.0,
        Role::Relief => s.x < 30.0 && s.y < 30.0 && s.z < 8.0,
        Role::Column => s.y > 2.0 && s.x < 8.0 && s.z < 8.0,
        Role::Beam => s.x.max(s.z) > 1.0 && s.x.max(s.z) < 40.0,
        Role::GroundDecal => s.y < 1.5 && s.x.max(s.z) < 20.0,
        Role::GroundDetail => s.y < 4.0 && s.x.max(s.z) < 20.0,
        Role::Facade => true,
        _ => s.x < 30.0 && s.y < 30.0 && s.z < 30.0,
      };
      if fits {
        kit.parts.push(k);
      }
    }
  }
  // no relief named so (the Precursor core's plates, the throne room's banners, Dead Town's
  // stones): the panels its walls are dressed with, known by their shape: flat against a wall,
  // 1 to 12 m, used more than once in the level; the most used first
  if !kit.parts.iter().any(|k| k.role == Role::Relief) {
    let mut unnamed: Vec<&CatalogPart> = parts
      .iter()
      .filter(|p| p.kind == PartKind::Prototype && p.triangles >= 8 && p.copies >= 2)
      .filter(|p| {
        let n = p.name.to_lowercase();
        role_of(&p.name).is_none()
          && !K_NEVER.iter().any(|w| n.contains(w))
          && !K_NOT_RELIEF.iter().any(|w| n.contains(w))
      })
      .collect();
    unnamed.sort_by_key(|p| -p.copies);
    let flat = |k: &KitPart| {
      let s = k.size;
      (1.0..=12.0).contains(&s.x)
        && (1.0..=12.0).contains(&s.y)
        && s.z >= 0.05
        && s.z <= 0.35 * s.x.min(s.y)
    };
    // hung on a wall of the level first (a crate on the floor is none), else flat ones
    let hung = |p: &CatalogPart| {
      part_mesh(level, p).is_some_and(|mesh| {
        let s = mesh.bounds().hi - mesh.bounds().lo;
        let thin = if s.x < s.z {
          Vec3::new(1.0, 0.0, 0.0)
        } else {
          Vec3::new(0.0, 0.0, 1.0)
        };
        wall_behind(level, p, &mesh, &[thin, -thin], 3.0).is_some()
      })
    };
    // solid: three fourths of its triangles neither glowing, nor a decal, nor see-through (the
    // nav meshes' rule): a panel of the wall, not a lit window nor a stain
    let solid = |p: &CatalogPart| {
      part_mesh(level, p).is_some_and(|mesh| {
        let (mut ok, mut all) = (0, 0);
        for prim in &mesh.prims {
          let n = prim.idx.len() / 3;
          all += n;
          if usize::try_from(prim.material)
            .ok()
            .and_then(|i| level.materials.get(i))
            .is_some_and(|m| !crate::navmesh::navmesh_ignores(m))
          {
            ok += n;
          }
        }
        all > 0 && ok * 4 >= all * 3
      })
    };
    let (on_walls, others): (Vec<&CatalogPart>, Vec<&CatalogPart>) = unnamed
      .into_iter()
      .filter(|p| solid(p))
      .take(40)
      .partition(|p| hung(p));
    let measured = |list: Vec<&CatalogPart>| -> Vec<KitPart> {
      list
        .into_iter()
        .filter_map(|p| measure(level, p, Role::Relief))
        .filter(flat)
        .collect()
    };
    let mut panels = measured(on_walls);
    if panels.is_empty() {
      panels = measured(others);
    }
    panels.truncate(4);
    kit.parts.extend(panels);
  }
  let list = |r: Role| by_role.get(&r).cloned().unwrap_or_default();
  let style = measure_style(level);
  kit.floor = style.floor.clone();
  kit.wall = main_surface(level, &list(Role::Facade), true);
  if kit.wall.material < 0 {
    kit.wall = style.wall;
  }
  let roofs = list(Role::Roof);
  let covers: Vec<&CatalogPart> = roofs
    .iter()
    .copied()
    .filter(|p| covering(&p.name))
    .collect();
  kit.roof = main_surface(
    level,
    if covers.is_empty() { &roofs } else { &covers },
    false,
  );
  let frames: Vec<&CatalogPart> = [Role::Doorframe, Role::Window, Role::Door]
    .into_iter()
    .flat_map(list)
    .collect();
  kit.trim = main_surface(level, &frames, false);
  // flat roofs: their plates thin and wide
  let roofs: Vec<f32> = kit
    .parts
    .iter()
    .filter(|p| p.role == Role::Roof)
    .map(|p| p.size.y / p.size.x.max(p.size.z))
    .collect();
  kit.flat_roofs = !roofs.is_empty() && roofs.iter().sum::<f32>() / (roofs.len() as f32) < 0.08;
  kit
}

// ------------------------------------------------------------------------------------------------
// a house
// ------------------------------------------------------------------------------------------------

/// What the house is like: everything has a default from its level's parts
#[derive(Clone, Debug, PartialEq)]
pub struct HouseOptions {
  /// the level it is like
  pub level: String,
  /// 1 to 4
  pub floors: i32,
  /// meters (x, its front's), 0: from the level's windows
  pub width: f32,
  /// meters (z), 0: from the level's windows
  pub depth: f32,
  /// "auto" (the level's), "gable", "hip" or "flat"
  pub roof: String,
  /// 0 (new) to 1 (worn out): stains, holes, broken plaster on its walls
  pub wear: f32,
  /// other windows, wear, chimney: another house alike
  pub seed: u32,
}

impl Default for HouseOptions {
  fn default() -> Self {
    Self {
      level: String::new(),
      floors: 2,
      width: 0.0,
      depth: 0.0,
      roof: "auto".into(),
      wear: 0.5,
      seed: 1,
    }
  }
}

impl HouseOptions {
  pub fn from_json(j: &Value) -> HouseOptions {
    let mut o = HouseOptions::default();
    let num = |k: &str| j.get(k).and_then(Value::as_f64).map(|v| v as f32);
    if let Some(l) = j.get("level").and_then(Value::as_str) {
      o.level = l.into();
    }
    if let Some(v) = num("floors") {
      o.floors = (v as i32).clamp(1, 4);
    }
    if let Some(v) = num("width") {
      o.width = v.clamp(0.0, 200.0);
    }
    if let Some(v) = num("depth") {
      o.depth = v.clamp(0.0, 200.0);
    }
    if let Some(r) = j.get("roof").and_then(Value::as_str)
      && ["auto", "gable", "hip", "flat"].contains(&r)
    {
      o.roof = r.into();
    }
    if let Some(v) = num("wear") {
      o.wear = v.clamp(0.0, 1.0);
    }
    if let Some(v) = j.get("seed").and_then(Value::as_u64) {
      o.seed = v as u32;
    }
    o
  }

  pub fn to_json(&self) -> Value {
    json!({"level": self.level, "floors": self.floors, "width": self.width, "depth": self.depth,
      "roof": self.roof, "wear": self.wear, "seed": self.seed})
  }
}

/// the surface a shape of the house takes: the level's walls' (darker: the plinth, the bands, the
/// pillars), its roofs', its frames', or a dark one (a window without glass)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Look {
  Wall,
  WallDark,
  Roof,
  Trim,
  Dark,
  /// the level's ground
  Floor,
}

#[derive(Clone, Debug)]
pub enum What {
  Shape {
    name: &'static str,
    params: ShapeParams,
    look: Look,
  },
  /// a part of the kit, scaled along its turned axes
  Part { index: usize, scale: Vec3 },
}

/// A piece of the house: the middle of its base at `at` (the house's base in the middle of its
/// footprint, its front towards +z), turned by `turn`
#[derive(Clone, Debug)]
pub struct Piece {
  pub what: What,
  pub at: Vec3,
  pub turn: Quat,
}

/// The house planned: its pieces, its size (meters: width, height with its roof, depth)
#[derive(Clone, Debug, Default)]
pub struct HousePlan {
  pub pieces: Vec<Piece>,
  /// the piece at the structure's origin, not turned: its anchor (it keeps the parameters)
  pub anchor: usize,
  pub size: Vec3,
  /// the kit's parts used, by role
  pub used: BTreeMap<&'static str, String>,
}

/// the same numbers for the same seed: 0..1
struct Rng(u64);

impl Rng {
  fn next(&mut self) -> f32 {
    self.0 = self
      .0
      .wrapping_mul(6364136223846793005)
      .wrapping_add(1442695040888963407);
    (self.0 >> 40) as f32 / (1u64 << 24) as f32
  }
  fn pick(&mut self, n: usize) -> usize {
    ((self.next() * n as f32) as usize).min(n.saturating_sub(1))
  }
}

/// a wall of the house: the middle of its thickness, turned so that its outside faces +z
struct Side {
  center: Vec3,
  turn: Quat,
  length: f32,
  front: bool,
}

impl Side {
  /// a point `a` along it, `y` up, `d` out of its middle
  fn at(&self, a: f32, y: f32, d: f32) -> Vec3 {
    self.center + self.turn.rotate(Vec3::new(a, 0.0, d)) + Vec3::new(0.0, y, 0.0)
  }
}

/// a rectangle on a wall: along it from a0 to a1, from y0 up to y1
#[derive(Clone, Copy)]
struct Rect {
  a0: f32,
  a1: f32,
  y0: f32,
  y1: f32,
}

impl Rect {
  fn overlaps(&self, o: &Rect) -> bool {
    self.a0 < o.a1 && o.a0 < self.a1 && self.y0 < o.y1 && o.y0 < self.y1
  }
}

/// The house in the style of a kit: its pieces, in the house's own space
pub fn plan_house(kit: &BuildingKit, o: &HouseOptions) -> HousePlan {
  let mut rng = Rng(o.seed as u64 * 7919 + 17);
  let mut plan = HousePlan::default();
  let first = |r: Role| kit.of(r).first().copied();
  // one window and one door for the whole house: among the two most used of the level
  let pick_of = |r: Role, rng: &mut Rng| {
    let l = kit.of(r);
    (!l.is_empty()).then(|| l[rng.pick(l.len().min(2))])
  };
  let window = pick_of(Role::Window, &mut rng);
  let door = pick_of(Role::Door, &mut rng);
  let size_of = |i: Option<usize>, d: Vec3| i.map_or(d, |i| kit.parts[i].size);
  let ws = size_of(window, Vec3::new(1.6, 1.8, 0.2));
  let ds = size_of(door, Vec3::new(1.4, 2.6, 0.1));
  // a floor as high as the level's doors and windows ask, its walls cut in bays a window wide
  let h = (ds.y * 1.45).max(ws.y * 2.0).clamp(3.5, 16.0);
  let bay = (ws.x.max(ds.x) * 1.9).max(h * 0.5);
  let t = (h * 0.05).max(0.3); // the walls' thickness
  let n = o.floors.clamp(1, 4);
  let w = if o.width > 0.0 {
    o.width.max(bay)
  } else {
    bay * 3.0
  };
  let d = if o.depth > 0.0 {
    o.depth.max(bay)
  } else {
    bay * 2.0
  };
  let top = n as f32 * h;
  let shape = |name: &'static str, kind: &str, size: Vec3, look: Look| What::Shape {
    name,
    params: ShapeParams {
      kind: kind.into(),
      size,
      ..Default::default()
    },
    look,
  };
  let mut add = |what: What, at: Vec3, turn: Quat| plan.pieces.push(Piece { what, at, turn });
  let part = |i: usize, k: f32| What::Part {
    index: i,
    scale: splat(k),
  };
  for (role, i) in [(Role::Window, window), (Role::Door, door)] {
    if let Some(i) = i {
      plan
        .used
        .insert(role.name(), kit.parts[i].part.name.clone());
    }
  }

  // the walls: front (+z), back, then the sides between them
  let sides = [
    Side {
      center: Vec3::new(0.0, 0.0, d / 2.0 - t / 2.0),
      turn: Quat::default(),
      length: w,
      front: true,
    },
    Side {
      center: Vec3::new(0.0, 0.0, -(d / 2.0 - t / 2.0)),
      turn: yaw(PI),
      length: w,
      front: false,
    },
    Side {
      center: Vec3::new(w / 2.0 - t / 2.0, 0.0, 0.0),
      turn: yaw(PI / 2.0),
      length: d - 2.0 * t,
      front: false,
    },
    Side {
      center: Vec3::new(-(w / 2.0 - t / 2.0), 0.0, 0.0),
      turn: yaw(-PI / 2.0),
      length: d - 2.0 * t,
      front: false,
    },
  ];
  // the window (and door) as large as a bay and a floor take
  let wk = (bay * 0.8 / ws.x).min(h * 0.75 / ws.y).min(1.0);
  let dk = (bay * 0.85 / ds.x).min((h - 0.5) / ds.y).min(1.0);
  let (ow, oh) = (ws.x * wk * 0.92, ws.y * wk * 0.92);
  let (dw, dh) = (ds.x * dk * 0.98, ds.y * dk * 0.99);
  let glass = kit
    .of(Role::Glass)
    .into_iter()
    .find(|&g| kit.parts[g].size.x <= ws.x * 1.1);
  let shade = first(Role::Shade);
  // what is on each wall, where wear must not go
  let mut openings: Vec<Vec<Rect>> = vec![Vec::new(); 4];
  let mut door_at = 0.0;
  for (si, side) in sides.iter().enumerate() {
    let bays = ((side.length / bay).round() as i32).max(1);
    let b = side.length / bays as f32;
    for f in 0..n {
      let y0 = f as f32 * h;
      for i in 0..bays {
        let a = -side.length / 2.0 + b * (i as f32 + 0.5);
        let is_door = side.front && f == 0 && i == bays / 2;
        // windows all along the front, most of the other walls
        let is_window = !is_door && (side.front || rng.next() < 0.7);
        let at = side.at(a, y0, 0.0);
        if is_door {
          door_at = a;
          let mut wall = shape("house-wall", "doorway", Vec3::new(b, h, t), Look::Wall);
          if let What::Shape { params, .. } = &mut wall {
            params.opening = crate::math::Vec2::new(dw, dh);
          }
          add(wall, at, side.turn);
          openings[si].push(Rect {
            a0: a - dw / 2.0 - 0.3,
            a1: a + dw / 2.0 + 0.3,
            y0,
            y1: y0 + dh + 0.3,
          });
          match door {
            Some(di) => add(part(di, dk), side.at(a, y0, 0.0), side.turn),
            None => add(
              shape("house-door", "box", Vec3::new(dw, dh, t * 0.3), Look::Trim),
              side.at(a, y0, 0.0),
              side.turn,
            ),
          }
          if let Some(fi) = first(Role::Doorframe) {
            let fs = kit.parts[fi].size;
            let k = (dw * 1.25 / fs.x).min(1.0).max(dw / fs.x);
            add(part(fi, k), side.at(a, y0, t / 2.0), side.turn);
          }
          // a step before it
          add(
            shape(
              "house-step",
              "box",
              Vec3::new(dw + 1.0, h * 0.04, 1.2),
              Look::WallDark,
            ),
            side.at(a, y0, t / 2.0 + 0.6),
            side.turn,
          );
        } else if is_window {
          let mut wall = shape("house-wall", "window", Vec3::new(b, h, t), Look::Wall);
          if let What::Shape { params, .. } = &mut wall {
            params.opening = crate::math::Vec2::new(ow, oh);
          }
          add(wall, at, side.turn);
          let wy = y0 + (h - ws.y * wk) / 2.0;
          openings[si].push(Rect {
            a0: a - ws.x * wk / 2.0,
            a1: a + ws.x * wk / 2.0,
            y0: wy,
            y1: wy + ws.y * wk,
          });
          match window {
            Some(wi) => {
              add(part(wi, wk), side.at(a, wy, t * 0.25), side.turn);
              if let Some(g) = glass {
                let gs = kit.parts[g].size;
                let k = (ow / gs.x).min(oh / gs.y).min(wk);
                add(
                  part(g, k),
                  side.at(a, y0 + (h - gs.y * k) / 2.0, 0.0),
                  side.turn,
                );
              }
            }
            None => {
              // a frame of shapes: a sill, a lintel, the dark inside
              let y = y0 + (h - oh) / 2.0;
              add(
                shape(
                  "house-sill",
                  "box",
                  Vec3::new(ow + 0.4, 0.2, t + 0.3),
                  Look::Trim,
                ),
                side.at(a, y - 0.2, 0.0),
                side.turn,
              );
              add(
                shape(
                  "house-lintel",
                  "box",
                  Vec3::new(ow + 0.4, 0.25, t + 0.2),
                  Look::Trim,
                ),
                side.at(a, y + oh, 0.0),
                side.turn,
              );
              add(
                shape("house-glass", "box", Vec3::new(ow, oh, 0.05), Look::Dark),
                side.at(a, y, 0.0),
                side.turn,
              );
            }
          }
          // a shade over some windows upstairs
          if let Some(si) = shade
            && f > 0
            && rng.next() < 0.4
          {
            let ss = kit.parts[si].size;
            let k = (b * 0.9 / ss.x).min(1.0);
            add(
              part(si, k),
              side.at(a, wy + ws.y * wk, t / 2.0 + ss.z * k / 2.0),
              side.turn,
            );
          }
        } else {
          add(
            shape("house-wall", "box", Vec3::new(b, h, t), Look::Wall),
            at,
            side.turn,
          );
        }
      }
    }
  }

  // relief: a plinth (around the door), bands between the floors and under the roof, pillars at
  // the corners; the floors inside
  let p = t * 0.6;
  let hp = h * 0.07;
  for (si, side) in sides.iter().enumerate() {
    let ext = if si < 2 { p * 2.0 } else { 0.0 };
    let (a0, a1) = (
      -side.length / 2.0 - ext / 2.0,
      side.length / 2.0 + ext / 2.0,
    );
    let spans = if side.front {
      vec![(a0, door_at - dw / 2.0), (door_at + dw / 2.0, a1)]
    } else {
      vec![(a0, a1)]
    };
    for (s0, s1) in spans {
      // down into the ground too: a foundation where the ground is lower than the door
      if s1 - s0 > 0.05 {
        add(
          shape(
            "house-plinth",
            "box",
            Vec3::new(s1 - s0, hp + h * 0.4, t + 2.0 * p),
            Look::WallDark,
          ),
          side.at((s0 + s1) / 2.0, -h * 0.4, 0.0),
          side.turn,
        );
      }
    }
  }
  let hb = h * 0.05;
  for f in 1..=n {
    let out = if f == n { p * 2.5 } else { p * 1.5 };
    add(
      shape(
        "house-band",
        "box",
        Vec3::new(w + 2.0 * out, hb, d + 2.0 * out),
        Look::WallDark,
      ),
      Vec3::new(0.0, f as f32 * h - hb, 0.0),
      Quat::default(),
    );
  }
  add(
    shape(
      "house-floor",
      "box",
      Vec3::new(w - t, 0.1, d - t),
      Look::WallDark,
    ),
    Vec3::default(),
    Quat::default(),
  );
  let pw = (t * 2.2).max(bay * 0.07);
  for (sx, sz) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
    add(
      shape(
        "house-pillar",
        "box",
        Vec3::new(pw, top, pw),
        Look::WallDark,
      ),
      Vec3::new(
        sx * (w / 2.0 - pw / 2.0 + 0.15),
        0.0,
        sz * (d / 2.0 - pw / 2.0 + 0.15),
      ),
      Quat::default(),
    );
  }

  // the roof: the level's (flat in Spargus, sloped elsewhere) unless chosen
  let roof = match o.roof.as_str() {
    "gable" | "hip" | "flat" => o.roof.clone(),
    _ if kit.flat_roofs => "flat".into(),
    _ => "gable".into(),
  };
  let over = t * 2.5;
  let roof_look = if kit.roof.material >= 0 {
    Look::Roof
  } else {
    Look::WallDark
  };
  let roof_top;
  let roofs = kit.of(Role::Roof);
  if roof == "flat" {
    let slab = t * 1.2;
    add(
      shape(
        "house-roof",
        "box",
        Vec3::new(w + 2.0 * over, slab, d + 2.0 * over),
        roof_look,
      ),
      Vec3::new(0.0, top, 0.0),
      Quat::default(),
    );
    // a low wall around it, the level's roof plates on it
    let ph = h * 0.12;
    let y = top + slab;
    for (at, size) in [
      (
        Vec3::new(0.0, y, d / 2.0 + over - t / 2.0),
        Vec3::new(w + 2.0 * over, ph, t),
      ),
      (
        Vec3::new(0.0, y, -(d / 2.0 + over - t / 2.0)),
        Vec3::new(w + 2.0 * over, ph, t),
      ),
      (
        Vec3::new(w / 2.0 + over - t / 2.0, y, 0.0),
        Vec3::new(t, ph, d + 2.0 * over - 2.0 * t),
      ),
      (
        Vec3::new(-(w / 2.0 + over - t / 2.0), y, 0.0),
        Vec3::new(t, ph, d + 2.0 * over - 2.0 * t),
      ),
    ] {
      add(
        shape("house-parapet", "box", size, Look::Wall),
        at,
        Quat::default(),
      );
    }
    if let Some(&ri) = roofs.first() {
      let rs = kit.parts[ri].size;
      let k = ((w - t) / rs.x).min((d - t) / rs.z).min(1.0);
      add(part(ri, k), Vec3::new(0.0, y, 0.0), Quat::default());
      plan.used.insert("roof", kit.parts[ri].part.name.clone());
    }
    roof_top = y + ph;
  } else {
    // two slopes (or four) over the longer side, as steep as the level's roof plates (its two most
    // used plates and slats, not its beams)
    let along_x = w > d;
    let (rw, rd) = if along_x { (d, w) } else { (w, d) };
    let run = rw / 2.0 + over;
    let plates: Vec<usize> = roofs
      .iter()
      .copied()
      .filter(|&i| {
        let s = kit.parts[i].size;
        s.y < 0.35 * s.x.max(s.z)
      })
      .take(2)
      .collect();
    let angle = plates
      .first()
      .map(|&i| kit.parts[i].slope)
      .filter(|a| *a > 0.14 && *a < 0.7)
      .unwrap_or(0.4);
    let rh = run * angle.tan();
    let kind = if roof == "hip" { "roof-hip" } else { "roof" };
    let turn = if along_x {
      yaw(PI / 2.0)
    } else {
      Quat::default()
    };
    add(
      shape(
        "house-roof",
        kind,
        Vec3::new(rw + 2.0 * over, rh, rd + 2.0 * over),
        roof_look,
      ),
      Vec3::new(0.0, top, 0.0),
      turn,
    );
    // its ridge capped
    let ridge = if roof == "hip" {
      (rd - rw).max(0.5)
    } else {
      rd + 2.0 * over
    };
    add(
      shape(
        "house-ridge",
        "box",
        Vec3::new(t * 1.2, t * 0.8, ridge),
        Look::Trim,
      ),
      Vec3::new(0.0, top + rh * (1.0 - 0.6 * t / run) - t * 0.4, 0.0),
      turn,
    );
    // the plates in rows from the eaves up, overlapping like tiles, laid on the slope (a gable's
    // two slopes); turned so that they rise towards the ridge (-side along x), tilted to its angle
    if roof == "gable" && !plates.is_empty() {
      let ridge_len = rd + 2.0 * over;
      // a plate turned for a slope: its horizontal run, its width along the ridge
      let laid = |i: usize, side: f32| {
        let kp = &kit.parts[i];
        let b = -side * PI / 2.0 - kp.rise.x.atan2(kp.rise.z);
        let (lo, hi) = turned_box(kp.raw, yaw(b) * kp.rot, splat(1.0));
        (b, hi.x - lo.x, hi.z - lo.z)
      };
      let (_, l0, w0) = laid(plates[0], 1.0);
      let slope_len = run / angle.cos();
      let rows = (slope_len / (l0 / kit.parts[plates[0]].slope.cos() * 0.85))
        .ceil()
        .max(1.0);
      let cols = (ridge_len / (w0 * 0.95)).ceil().max(1.0);
      let step = run / rows;
      for side in [1.0f32, -1.0] {
        for i in 0..rows as i32 {
          let u = run - step * (i as f32 + 0.5); // from the ridge
          let low = u + step / 0.85 / 2.0;
          let y = top + rh * (1.0 - low / run) + 0.05;
          for j in 0..cols as i32 {
            let pi = plates[(i + j) as usize % plates.len()];
            let kp = &kit.parts[pi];
            let (b, l, wd) = laid(pi, side);
            let kr = step / 0.85 / l;
            let kc = ridge_len / cols / wd;
            // along its run: its own x or z once turned by b
            let v = yaw(b).rotate(Vec3::new(1.0, 0.0, 0.0));
            let scale = if v.x.abs() > v.z.abs() {
              Vec3::new(kr, kr.min(1.0), kc)
            } else {
              Vec3::new(kc, kr.min(1.0), kr)
            };
            let tilt = Quat::axis_angle(
              Vec3::new(0.0, 0.0, 1.0),
              -side * (angle - kp.slope).max(0.0),
            );
            let z = -ridge_len / 2.0 + (j as f32 + 0.5) * ridge_len / cols;
            add(
              What::Part { index: pi, scale },
              turn.rotate(Vec3::new(side * u, 0.0, z)) + Vec3::new(0.0, y, 0.0),
              turn * tilt * yaw(b),
            );
          }
        }
      }
      plan
        .used
        .insert("roof", kit.parts[plates[0]].part.name.clone());
    }
    roof_top = top + rh;
  }
  // a chimney through the roof, at the back
  let cx = w * 0.25 * if rng.next() < 0.5 { 1.0 } else { -1.0 };
  let cz = -d * 0.2;
  let chimneys = kit.of(Role::Chimney);
  match chimneys.get(rng.pick(chimneys.len())) {
    Some(&ci) => {
      add(part(ci, 1.0), Vec3::new(cx, top, cz), Quat::default());
      plan.used.insert("chimney", kit.parts[ci].part.name.clone());
    }
    None => add(
      shape(
        "house-chimney",
        "box",
        Vec3::new(t * 3.0, roof_top - top + h * 0.3, t * 3.0),
        Look::WallDark,
      ),
      Vec3::new(cx, top, cz),
      Quat::default(),
    ),
  }

  // a pipe down the right wall, at the back; rings on it
  let right = &sides[2];
  let pipe_a = right.length / 2.0 - pw - 0.6; // towards the back
  match first(Role::Pipe) {
    Some(pi) => {
      let ps = kit.parts[pi].size;
      let count = (top / ps.y).ceil().max(1.0);
      let k = top / (count * ps.y);
      for c in 0..count as i32 {
        add(
          What::Part {
            index: pi,
            scale: Vec3::new(1.0, k, 1.0),
          },
          right.at(pipe_a, c as f32 * ps.y * k, t / 2.0 + ps.z / 2.0),
          right.turn,
        );
      }
      if let Some(ri) = kit.of(Role::PipeRing).into_iter().min_by(|&a, &b| {
        let fit = |i: usize| (kit.parts[i].size.x - ps.x * 1.2).abs();
        fit(a).total_cmp(&fit(b))
      }) {
        for c in 1..count as i32 {
          add(
            part(ri, 1.0),
            right.at(
              pipe_a,
              c as f32 * ps.y * k - kit.parts[ri].size.y / 2.0,
              t / 2.0 + ps.z / 2.0,
            ),
            right.turn,
          );
        }
      }
      plan.used.insert("pipe", kit.parts[pi].part.name.clone());
    }
    None => {
      let r = (h * 0.03).max(0.15);
      let mut pipe = shape(
        "house-pipe",
        "cylinder",
        Vec3::new(2.0 * r, top, 2.0 * r),
        Look::Trim,
      );
      if let What::Shape { params, .. } = &mut pipe {
        params.sides = 10;
      }
      add(pipe, right.at(pipe_a, 0.0, t / 2.0 + r), right.turn);
    }
  }
  openings[2].push(Rect {
    a0: pipe_a - 1.5,
    a1: pipe_a + 1.5,
    y0: 0.0,
    y1: top,
  });

  // by the door: an awning over it, a lamp beside it, a sign at the corner
  let front = &sides[0];
  let door_top = dh;
  if let Some(ai) = first(Role::Awning) {
    let s = kit.parts[ai].size;
    let k = (dw * 1.8 / s.x).min(1.0);
    add(
      part(ai, k),
      front.at(door_at, door_top + h * 0.06, t / 2.0 + s.z * k / 2.0),
      front.turn,
    );
    openings[0].push(Rect {
      a0: door_at - s.x * k / 2.0,
      a1: door_at + s.x * k / 2.0,
      y0: door_top,
      y1: door_top + h * 0.06 + s.y * k,
    });
  }
  if let Some(li) = first(Role::Lamp) {
    let s = kit.parts[li].size;
    let k = (h * 0.35 / s.y).min(1.0);
    let a = door_at + dw / 2.0 + s.x * k / 2.0 + 0.4;
    add(
      part(li, k),
      front.at(a, door_top - s.y * k * 0.3, t / 2.0 + s.z * k / 2.0),
      front.turn,
    );
    plan.used.insert("lamp", kit.parts[li].part.name.clone());
  }
  if let Some(gi) = first(Role::Sign) {
    // sticking out of the front near its corner (turned as the level hangs it)
    let s = kit.parts[gi].size;
    let k = (bay * 0.8 / s.z.max(s.x)).min(1.0);
    let a = w / 2.0 - pw - s.x * k / 2.0 - 0.5;
    add(
      part(gi, k),
      front.at(a, h * 0.8 - s.y * k, t / 2.0 + s.z * k / 2.0),
      front.turn,
    );
    plan.used.insert("sign", kit.parts[gi].part.name.clone());
  }

  // wear: streaks under the windows and the bands, holes and broken plaster between the openings
  let wear = kit.of(Role::Wear);
  if !wear.is_empty() && o.wear > 0.0 {
    let count = (o.wear * (w + d) * n as f32 / bay * 2.5).round() as i32;
    let mut used_wear = 0;
    for _ in 0..count {
      let wi = wear[rng.pick(wear.len())];
      let s = kit.parts[wi].size;
      let name = kit.parts[wi].part.name.to_lowercase();
      let si = rng.pick(4);
      let side = &sides[si];
      let k = (bay * 0.6 / s.x).min(h * 0.6 / s.y).min(1.0);
      let (sw, sh) = (s.x * k, s.y * k);
      let streak = name.contains("stain") || name.contains("streak") || name.contains("blotch");
      for _try in 0..8 {
        let f = rng.pick(n as usize) as f32;
        let a = (rng.next() - 0.5) * (side.length - sw - 2.0 * pw).max(0.0);
        // streaks hang from the band above, the rest anywhere on the floor
        let y = if streak {
          (f + 1.0) * h - hb - sh
        } else {
          f * h + hp + rng.next() * (h - hp - hb - sh).max(0.0)
        };
        let r = Rect {
          a0: a - sw / 2.0,
          a1: a + sw / 2.0,
          y0: y,
          y1: y + sh,
        };
        if !streak && openings[si].iter().any(|o| o.overlaps(&r)) {
          continue;
        }
        add(
          part(wi, k),
          side.at(a, y, t / 2.0 + s.z * k / 2.0 + 0.02),
          side.turn,
        );
        if !streak {
          openings[si].push(r);
        }
        used_wear += 1;
        break;
      }
    }
    if used_wear > 0 {
      plan.used.insert("wear", format!("{used_wear}"));
    }
  }
  // a vine down a side
  if let Some(vi) = first(Role::Vine)
    && rng.next() < 0.7
  {
    let s = kit.parts[vi].size;
    let side = &sides[3];
    add(
      part(vi, (top * 0.5 / s.y).min(1.0)),
      side.at(
        side.length * 0.2,
        top - hb - s.y.min(top * 0.5),
        t / 2.0 + s.z / 2.0 + 0.03,
      ),
      side.turn,
    );
  }
  plan.size = Vec3::new(w + 2.0 * over, roof_top.max(top), d + 2.0 * over);
  // its anchor: the floor inside, at its origin
  plan.anchor = plan
    .pieces
    .iter()
    .position(|p| matches!(&p.what, What::Shape { name, .. } if *name == "house-floor"))
    .unwrap_or(0);
  plan
}

/// Where a part goes for a piece: its matrix in the house (its turned box's base at `at`)
pub fn part_matrix(kp: &KitPart, piece: &Piece, scale: Vec3) -> Mat4 {
  // the scale along its turned axes, in its own
  let mut raw_scale = splat(1.0);
  for i in 0..3 {
    let mut e = Vec3::default();
    e[i] = 1.0;
    let r = kp.rot.rotate(e);
    let j = (0..3)
      .max_by(|&a, &b| r[a].abs().total_cmp(&r[b].abs()))
      .unwrap_or(i);
    raw_scale[i] = scale[j];
  }
  let rot = piece.turn * kp.rot;
  let (lo, hi) = turned_box(kp.raw, rot, raw_scale);
  let base = Vec3::new((lo.x + hi.x) / 2.0, lo.y, (lo.z + hi.z) / 2.0);
  Mat4::translate(piece.at - base) * Mat4::rotate(rot) * Mat4::scale(raw_scale)
}

// ------------------------------------------------------------------------------------------------
// structures that grow: walls, floors, buildings
// ------------------------------------------------------------------------------------------------

/// A structure of the level's pieces that is made again when its size changes (the game makers'
/// modular kits, put together by rules): its kind, the level it is like, its size, its look.
/// Kept on its anchor's extras (og_structure), it survives a prefab.
#[derive(Clone, Debug, PartialEq)]
pub struct StructureParams {
  /// "wall", "floor" or "house"
  pub kind: String,
  /// the level it is like
  pub level: String,
  /// meters: a wall's length, height, thickness; a floor's width, thickness, depth; a house's
  /// width, (its floors' height), depth. 0: from the level's pieces
  pub size: Vec3,
  /// a house's floors, a wall's height in floors when it has none (1 to 6)
  pub floors: i32,
  /// a house's roof: "auto", "gable", "hip" or "flat"
  pub roof: String,
  /// 0 (new) to 1 (worn out)
  pub wear: f32,
  /// another look of the same: other details, the same rules
  pub seed: u32,
  /// a wall with the level's windows in some of its bays
  pub windows: bool,
  /// a wall with a doorway in its middle bay, at the ground (an interior closed stays open to Jak)
  pub door: bool,
}

impl Default for StructureParams {
  fn default() -> Self {
    Self {
      kind: "house".into(),
      level: String::new(),
      size: Vec3::default(),
      floors: 2,
      roof: "auto".into(),
      wear: 0.5,
      seed: 1,
      windows: false,
      door: false,
    }
  }
}

pub const K_STRUCTURE_KINDS: [&str; 3] = ["wall", "floor", "house"];

impl StructureParams {
  /// {kind, level, size: [x, y, z], floors, roof, wear, seed, windows}, or a house's width and
  /// depth (build_house), a size's length / height / thickness by name
  pub fn from_json(j: &Value) -> StructureParams {
    let mut o = StructureParams::default();
    let num = |k: &str| j.get(k).and_then(Value::as_f64).map(|v| v as f32);
    if let Some(k) = j.get("kind").and_then(Value::as_str)
      && K_STRUCTURE_KINDS.contains(&k)
    {
      o.kind = k.into();
    }
    if let Some(l) = j.get("level").and_then(Value::as_str) {
      o.level = l.into();
    }
    if let Some(a) = j.get("size").and_then(Value::as_array)
      && a.len() == 3
    {
      for (i, v) in a.iter().enumerate() {
        o.size[i] = v.as_f64().unwrap_or(0.0) as f32;
      }
    }
    for (k, i) in [
      ("width", 0),
      ("length", 0),
      ("height", 1),
      ("thickness", if o.kind == "floor" { 1 } else { 2 }),
      ("depth", 2),
    ] {
      if let Some(v) = num(k) {
        o.size[i] = v;
      }
    }
    for i in 0..3 {
      o.size[i] = o.size[i].clamp(0.0, 400.0);
    }
    match num("floors") {
      Some(v) => o.floors = (v as i32).clamp(1, 6),
      // a wall is a floor high unless asked
      None if o.kind != "house" => o.floors = 1,
      None => {}
    }
    if let Some(r) = j.get("roof").and_then(Value::as_str)
      && ["auto", "gable", "hip", "flat"].contains(&r)
    {
      o.roof = r.into();
    }
    if let Some(v) = num("wear") {
      o.wear = v.clamp(0.0, 1.0);
    }
    if let Some(v) = j.get("seed").and_then(Value::as_u64) {
      o.seed = v as u32;
    }
    if let Some(v) = j.get("door").and_then(Value::as_bool) {
      o.door = v;
    }
    if let Some(v) = j.get("windows").and_then(Value::as_bool) {
      o.windows = v;
    }
    o
  }

  pub fn to_json(&self) -> Value {
    let r = |v: f32| (v * 100.0).round() / 100.0;
    json!({"kind": self.kind, "level": self.level,
      "size": [r(self.size.x), r(self.size.y), r(self.size.z)], "floors": self.floors,
      "roof": self.roof, "wear": self.wear, "seed": self.seed, "windows": self.windows,
      "door": self.door})
  }

  fn house(&self) -> HouseOptions {
    HouseOptions {
      level: self.level.clone(),
      floors: self.floors,
      width: self.size.x,
      depth: self.size.z,
      roof: self.roof.clone(),
      wear: self.wear,
      seed: self.seed,
    }
  }
}

/// A number of 0..1 drawn from the seed and a place (a bay, a cell, a face): the same place keeps
/// its draw when the structure grows around it
fn draw(seed: u32, a: i32, b: i32, c: u32) -> f32 {
  let mut x = seed
    .wrapping_mul(0x9E37_79B9)
    .wrapping_add((a as u32).wrapping_mul(0x85EB_CA6B))
    .wrapping_add((b as u32).wrapping_mul(0xC2B2_AE35))
    .wrapping_add(c.wrapping_mul(0x27D4_EB2F));
  x ^= x >> 15;
  x = x.wrapping_mul(0x2C1B_3C6D);
  x ^= x >> 12;
  x = x.wrapping_mul(0x297A_2D39);
  x ^= x >> 15;
  (x >> 8) as f32 / (1u32 << 24) as f32
}

/// A floor's height and a bay's width, from the level's windows and doors (as a house's)
fn module_sizes(kit: &BuildingKit) -> (f32, f32) {
  if kit.module > 0.0 {
    return (kit.module, kit.module);
  }
  let size_of = |r: Role, d: Vec3| kit.of(r).first().map_or(d, |&i| kit.parts[i].size);
  let ws = size_of(Role::Window, Vec3::new(1.6, 1.8, 0.2));
  let ds = size_of(Role::Door, Vec3::new(1.4, 2.6, 0.1));
  let h = (ds.y * 1.45).max(ws.y * 2.0).clamp(3.5, 16.0);
  let bay = (ws.x.max(ds.x) * 1.9).max(h * 0.5);
  (h, bay)
}

/// a level's beams lying (longer than high) and standing (columns): the ones it uses most
fn beams(kit: &BuildingKit) -> (Vec<usize>, Vec<usize>) {
  let beams = kit.of(Role::Beam);
  let lying = beams
    .iter()
    .copied()
    .filter(|&i| {
      let s = kit.parts[i].size;
      s.x.max(s.z) >= 3.0 && s.x.max(s.z) > 3.0 * s.y
    })
    .collect();
  let standing = kit
    .of(Role::Column)
    .into_iter()
    .chain(beams.iter().copied().filter(|&i| {
      let s = kit.parts[i].size;
      s.y > 3.0 * s.x.max(s.z)
    }))
    .collect();
  (lying, standing)
}

/// A wall of the level, solid (its texture on both faces and its top): bays between pillars that
/// stand out of both faces, a plinth, bands between its floors, a coping; on each face, in each bay,
/// the level's own relief (its bulges, panels), wear, a pipe, a lamp, a vine, drawn from the bay's
/// number: a longer wall keeps its bays as they were and gets new ones. Its length along x, its
/// front towards +z, the middle of its base at the origin.
pub fn plan_wall(kit: &BuildingKit, p: &StructureParams) -> HousePlan {
  let mut plan = HousePlan::default();
  let (fh, _) = module_sizes(kit);
  let bay = (fh * 0.8).clamp(4.0, 12.0);
  let l = if p.size.x > 0.0 {
    p.size.x.max(1.0)
  } else {
    bay * 3.0
  };
  let h = if p.size.y > 0.0 {
    p.size.y.max(0.5)
  } else {
    fh * p.floors.max(1) as f32
  };
  let t = if p.size.z > 0.0 {
    p.size.z.clamp(0.1, 20.0)
  } else {
    (fh * 0.06).max(0.4)
  };
  // a huge wall (closing a landmark of the game): larger floors and bays, at most about 12 floors
  // and 16 bays, rather than thousands of pieces
  let fh = fh.max(h / 12.0);
  let bay = bay.max(l / 16.0);
  let n = ((l / bay).round() as i32).max(1);
  let b = l / n as f32;
  let pw = (b * 0.1).clamp(0.4, 1.4); // a pillar's width
  let q = (t * 0.4).clamp(0.1, 0.6); // how far the pillars, the plinth and the bands stand out
  // a doorway: in the bay nearest the middle, Jak's size and more, over a low sill
  let door = Vec2::new((b - pw - 0.6).min(3.2), (h * 0.8).min(fh * 0.85).min(4.5));
  let door_bay = (p.door && door.x >= 1.6 && door.y >= 2.6).then_some(n / 2);
  let hp = if door_bay.is_some() {
    0.25
  } else {
    (h * 0.08).clamp(0.2, 1.2)
  };
  let hc = (h * 0.04).clamp(0.15, 0.6);
  let hb = (fh * 0.05).clamp(0.2, 0.6);
  let shape = |name: &'static str, size: Vec3, look: Look| What::Shape {
    name,
    params: ShapeParams {
      kind: "box".into(),
      size,
      ..Default::default()
    },
    look,
  };
  let part = |i: usize, k: f32| What::Part {
    index: i,
    scale: splat(k),
  };
  let mut add = |what: What, at: Vec3, turn: Quat| plan.pieces.push(Piece { what, at, turn });
  // the anchor: the plinth, at the origin
  add(
    shape(
      "wall-plinth",
      Vec3::new(l + 2.0 * q, hp, t + 2.0 * q),
      Look::WallDark,
    ),
    Vec3::default(),
    Quat::default(),
  );
  // the faces: the front (+z) and the back, each a Side (along it from -l/2 to l/2)
  let faces = [
    Side {
      center: Vec3::default(),
      turn: Quat::default(),
      length: l,
      front: true,
    },
    Side {
      center: Vec3::default(),
      turn: yaw(PI),
      length: l,
      front: false,
    },
  ];
  // along a face: x of the wall (the back's runs the other way)
  let along = |face: &Side, x: f32| if face.front { x } else { -x };
  let window = kit.of(Role::Window).first().copied();
  let ws = window.map(|i| kit.parts[i].size);
  let windowed = |k: i32| {
    p.windows && ws.is_some_and(|s| h >= s.y * 1.3 && b >= s.x * 1.3) && draw(p.seed, k, 0, 1) < 0.7
  };
  // the bays
  for k in 0..n {
    let x = -l / 2.0 + b * (k as f32 + 0.5);
    if door_bay == Some(k) {
      // the doorway up to a lintel over it, the bay's box above (cut like the others)
      let hd = h.min(door.y + 2.0);
      let mut wall = shape("wall-bay", Vec3::new(b, hd, t), Look::Wall);
      if let What::Shape { params, .. } = &mut wall {
        params.kind = "doorway".into();
        params.opening = door;
      }
      add(wall, Vec3::new(x, 0.0, 0.0), Quat::default());
      if h > hd + 0.01 {
        add(
          shape("wall-bay", Vec3::new(b, h - hd, t), Look::Wall),
          Vec3::new(x, hd, 0.0),
          Quat::default(),
        );
      }
      continue;
    }
    match ws.filter(|_| windowed(k)) {
      Some(s) => {
        let mut wall = shape("wall-bay", Vec3::new(b, h, t), Look::Wall);
        if let What::Shape { params, .. } = &mut wall {
          params.kind = "window".into();
          params.opening = crate::math::Vec2::new(s.x * 0.92, s.y * 0.92);
        }
        add(wall, Vec3::new(x, 0.0, 0.0), Quat::default());
        // the window seen from both sides
        for face in &faces {
          add(
            part(window.unwrap(), 1.0),
            face.at(along(face, x), (h - s.y) / 2.0, t * 0.25),
            face.turn,
          );
        }
      }
      None => add(
        shape("wall-bay", Vec3::new(b, h, t), Look::Wall),
        Vec3::new(x, 0.0, 0.0),
        Quat::default(),
      ),
    }
  }
  // the bands between its floors, the coping on top
  let mut f = 1;
  // the frame (bands, pillars, coping) in the level's trims (its frames' material) when it has
  // some: a wall of its own material and the level's frame around its bays
  let coping = if kit.trim.material >= 0 {
    Look::Trim
  } else {
    Look::WallDark
  };
  while (f as f32) * fh < h - hc - fh * 0.4 {
    add(
      shape("wall-band", Vec3::new(l + 2.0 * q, hb, t + 2.0 * q), coping),
      Vec3::new(0.0, f as f32 * fh - hb / 2.0, 0.0),
      Quat::default(),
    );
    f += 1;
  }
  add(
    shape(
      "wall-coping",
      Vec3::new(l + 3.0 * q, hc, t + 3.0 * q),
      coping,
    ),
    Vec3::new(0.0, h - hc, 0.0),
    Quat::default(),
  );
  // the pillars between the bays, through the wall; the level's columns over them on each face
  let (lying, standing) = beams(kit);
  for k in 0..=n {
    let x = (-l / 2.0 + b * k as f32).clamp(-l / 2.0 + pw / 2.0, l / 2.0 - pw / 2.0);
    add(
      shape("wall-pillar", Vec3::new(pw, h, t + 2.0 * q), coping),
      Vec3::new(x, 0.0, 0.0),
      Quat::default(),
    );
    if let Some(&ci) = standing.get((draw(p.seed, k, 7, 2) * standing.len() as f32) as usize) {
      let cs = kit.parts[ci].size;
      // as wide as the pillar at most, its height in a few lengths of its own
      let kw = (pw * 1.5 / cs.x).min(1.0);
      let count = ((h - hc) / (cs.y * kw)).round().max(1.0);
      let ky = (h - hc) / (count * cs.y);
      for face in &faces {
        for c in 0..count as i32 {
          add(
            What::Part {
              index: ci,
              scale: Vec3::new(kw, ky, kw),
            },
            face.at(
              along(face, x),
              c as f32 * cs.y * ky,
              t / 2.0 + q + cs.z * kw / 2.0 - 0.05,
            ),
            face.turn,
          );
        }
      }
      plan.used.insert("column", kit.parts[ci].part.name.clone());
    }
  }
  // in each bay, on each face: the level's relief, wear, a pipe, a lamp, a vine; a beam along
  let relief = kit.of(Role::Relief);
  let wear = kit.of(Role::Wear);
  let pipe = kit.of(Role::Pipe).first().copied();
  let lamp = kit.of(Role::Lamp).first().copied();
  let vine = kit.of(Role::Vine).first().copied();
  let room = (b - pw - 0.4).max(0.5); // across a bay, between its pillars
  let tall = (h - hp - hc - 0.4).max(0.5);
  let mut used_wear = 0;
  for k in (0..n).filter(|&k| door_bay != Some(k)) {
    let x = -l / 2.0 + b * (k as f32 + 0.5);
    for (fi, face) in faces.iter().enumerate() {
      let r = |i: u32| draw(p.seed, k, fi as i32, i);
      let a = along(face, x);
      let out = t / 2.0;
      // a level with no relief of its own (a desert, a prefab's own material): a raised panel on
      // each floor of the bay, the coffers of a wall of stone or of plates
      if relief.is_empty() && !windowed(k) {
        let floors = ((h - hp - hc) / fh).round().max(1.0);
        let fl = (h - hp - hc) / floors;
        // a huge wall: one in two, as a checkerboard
        let every = if n as f32 * floors > 48.0 { 2 } else { 1 };
        for f in (0..floors as i32).filter(|f| (k + f) % every == 0) {
          let size = Vec3::new(room * 0.72, (fl - hb) * 0.66, q * 1.2);
          add(
            shape("wall-panel", size, Look::WallDark),
            face.at(a, hp + fl * f as f32 + (fl - size.y) / 2.0, out),
            face.turn,
          );
        }
      }
      // the level's relief on each floor of the bay (the modules of a studio's kit: a taller
      // wall gets more of them, not larger ones), alike on both faces
      if !windowed(k) && !relief.is_empty() {
        let floors = ((h - hp - hc) / fh).round().max(1.0) as i32;
        let fl = tall / floors as f32;
        for f in 0..floors {
          let rf = |i: u32| draw(p.seed, k, 0, i + 40 * f as u32);
          let Some(&ri) = relief.get((rf(1) * relief.len() as f32) as usize) else {
            continue;
          };
          if rf(2) >= 0.85 {
            continue;
          }
          let s = kit.parts[ri].size;
          let kr = (room * 0.8 / s.x).min(fl * 0.8 / s.y).min(1.0);
          // a panel (2.5 m or more) repeated as many times as it fits the floor, 3 x 3 at most
          // (a tall floor of a landmark's wall: its panels in rows, not one lost in the middle);
          // a smaller piece once (a sign, a vent: not a grid of them)
          let (cols, rows) = if s.x.max(s.y) * kr >= 2.5 {
            (
              ((room * 0.85 / (s.x * kr)).floor() as i32).clamp(1, 3),
              ((fl * 0.85 / (s.y * kr)).floor() as i32).clamp(1, 3),
            )
          } else {
            (1, 1)
          };
          let (cw, ch) = (room / cols as f32, fl / rows as f32);
          for c in 0..cols {
            for rr in 0..rows {
              add(
                part(ri, kr),
                face.at(
                  a - room / 2.0 + cw * (c as f32 + 0.5),
                  hp + fl * f as f32 + ch * rr as f32 + (ch - s.y * kr) / 2.0 + 0.2,
                  out + s.z * kr / 2.0,
                ),
                face.turn,
              );
            }
          }
          plan.used.insert("relief", kit.parts[ri].part.name.clone());
        }
      }
      // wear: as much as asked, where the relief leaves room (beside it, under the coping)
      let count = (p.wear * 2.0 * (0.5 + r(3))).round() as i32;
      for w in 0..count {
        let Some(&wi) = wear.get((r(10 + w as u32) * wear.len() as f32) as usize) else {
          break;
        };
        let s = kit.parts[wi].size;
        let kw = (room * 0.5 / s.x).min(tall * 0.5 / s.y).min(1.0);
        let dx = (r(20 + w as u32) - 0.5) * (room - s.x * kw).max(0.0);
        let y = hp + r(30 + w as u32) * (tall - s.y * kw).max(0.0);
        add(
          part(wi, kw),
          face.at(a + dx, y, out + s.z * kw / 2.0 + 0.03),
          face.turn,
        );
        used_wear += 1;
      }
      // a pipe down the front, beside a pillar
      if face.front
        && let Some(pi) = pipe
        && r(4) < 0.25
      {
        let ps = kit.parts[pi].size;
        let count = ((h - hc) / ps.y).ceil().max(1.0);
        let ky = (h - hc) / (count * ps.y);
        let px = a + b / 2.0 - pw / 2.0 - ps.x / 2.0 - 0.1;
        for c in 0..count as i32 {
          add(
            What::Part {
              index: pi,
              scale: Vec3::new(1.0, ky, 1.0),
            },
            face.at(px, c as f32 * ps.y * ky, out + ps.z / 2.0),
            face.turn,
          );
        }
        plan.used.insert("pipe", kit.parts[pi].part.name.clone());
      }
      if face.front
        && let Some(li) = lamp
        && r(5) < 0.15
      {
        let s = kit.parts[li].size;
        let kl = (h * 0.3 / s.y).min(1.0);
        add(
          part(li, kl),
          face.at(a, h * 0.6, out + s.z * kl / 2.0),
          face.turn,
        );
      }
      if let Some(vi) = vine
        && r(6) < 0.15
      {
        let s = kit.parts[vi].size;
        let kv = (tall * 0.6 / s.y).min(1.0);
        add(
          part(vi, kv),
          face.at(
            a - room / 3.0,
            h - hc - s.y * kv,
            out + s.z * kv / 2.0 + 0.03,
          ),
          face.turn,
        );
      }
      // a beam along the face under the coping, in the bays the draw gives it
      if let Some(&bi) = lying.first()
        && r(8) < 0.5
      {
        let s = kit.parts[bi].size;
        let along_x = s.x >= s.z;
        let len = s.x.max(s.z);
        let kb = (room / len).min(1.0);
        let turn = if along_x {
          face.turn
        } else {
          face.turn * yaw(PI / 2.0)
        };
        let depth = if along_x { s.z } else { s.x };
        add(
          part(bi, kb),
          face.at(a, h - hc - s.y * kb - 0.1, out + depth * kb / 2.0),
          turn,
        );
        plan.used.insert("beam", kit.parts[bi].part.name.clone());
      }
    }
  }
  if used_wear > 0 {
    plan.used.insert("wear", format!("{used_wear}"));
  }
  plan.size = Vec3::new(l + 3.0 * q, h, t + 3.0 * q);
  plan.anchor = 0;
  plan
}

/// A floor of the level, solid (a slab, its underside and sides closed): its ground's texture on
/// top, a curb around it standing out of it, the seams of its paving between cells, and in each
/// cell, drawn from the cell's place, the level's dirt patches and its vents, grates or rocks. Its
/// width along x, its depth along z, its thickness y, the middle of its base at the origin.
pub fn plan_floor(kit: &BuildingKit, p: &StructureParams) -> HousePlan {
  let mut plan = HousePlan::default();
  let (fh, _) = module_sizes(kit);
  let w = if p.size.x > 0.0 {
    p.size.x.max(1.0)
  } else {
    16.0
  };
  let d = if p.size.z > 0.0 {
    p.size.z.max(1.0)
  } else {
    16.0
  };
  let t = if p.size.y > 0.0 {
    p.size.y.clamp(0.1, 20.0)
  } else {
    0.6
  };
  let cell = (fh * 0.75).clamp(4.0, 12.0);
  let cw = (w.min(d) * 0.04).clamp(0.2, 0.8); // the curb's width
  let rise = 0.15;
  let shape = |name: &'static str, size: Vec3, look: Look| What::Shape {
    name,
    params: ShapeParams {
      kind: "box".into(),
      size,
      ..Default::default()
    },
    look,
  };
  let trim = if kit.trim.material >= 0 {
    Look::Trim
  } else {
    Look::WallDark
  };
  let mut add = |what: What, at: Vec3, turn: Quat| plan.pieces.push(Piece { what, at, turn });
  add(
    shape("floor-slab", Vec3::new(w, t, d), Look::Floor),
    Vec3::default(),
    Quat::default(),
  );
  // the curb, around it
  for (at, size) in [
    (
      Vec3::new(0.0, 0.0, d / 2.0 - cw / 2.0),
      Vec3::new(w, t + rise, cw),
    ),
    (
      Vec3::new(0.0, 0.0, -(d / 2.0 - cw / 2.0)),
      Vec3::new(w, t + rise, cw),
    ),
    (
      Vec3::new(w / 2.0 - cw / 2.0, 0.0, 0.0),
      Vec3::new(cw, t + rise, d - 2.0 * cw),
    ),
    (
      Vec3::new(-(w / 2.0 - cw / 2.0), 0.0, 0.0),
      Vec3::new(cw, t + rise, d - 2.0 * cw),
    ),
  ] {
    add(shape("floor-curb", size, trim), at, Quat::default());
  }
  // the paving's seams, between its cells
  let nx = ((w / cell).round() as i32).max(1);
  let nz = ((d / cell).round() as i32).max(1);
  let (cx, cz) = (w / nx as f32, d / nz as f32);
  for i in 1..nx {
    add(
      shape("floor-seam", Vec3::new(0.25, 0.05, d - 2.0 * cw), trim),
      Vec3::new(-w / 2.0 + i as f32 * cx, t - 0.01, 0.0),
      Quat::default(),
    );
  }
  for j in 1..nz {
    add(
      shape("floor-seam", Vec3::new(w - 2.0 * cw, 0.05, 0.25), trim),
      Vec3::new(0.0, t - 0.01, -d / 2.0 + j as f32 * cz),
      Quat::default(),
    );
  }
  // in each cell, from where it is (cells counted from the middle: a larger floor keeps them)
  let decals = kit.of(Role::GroundDecal);
  let details = kit.of(Role::GroundDetail);
  for i in 0..nx {
    for j in 0..nz {
      let (gi, gj) = (i - nx / 2, j - nz / 2);
      let r = |k: u32| draw(p.seed, gi, gj, k);
      let c = Vec3::new(
        -w / 2.0 + cx * (i as f32 + 0.5),
        t,
        -d / 2.0 + cz * (j as f32 + 0.5),
      );
      let room = (cx.min(cz) - 2.0 * cw).max(0.5);
      if let Some(&di) = decals.get((r(1) * decals.len() as f32) as usize)
        && r(2) < 0.2 + 0.6 * p.wear
      {
        let s = kit.parts[di].size;
        let k = (room * 0.7 / s.x.max(s.z)).min(1.0);
        let off = Vec3::new((r(3) - 0.5) * room * 0.3, 0.01, (r(4) - 0.5) * room * 0.3);
        add(
          What::Part {
            index: di,
            scale: splat(k),
          },
          c + off,
          yaw(r(5) * 2.0 * PI),
        );
        plan
          .used
          .insert("ground_decal", kit.parts[di].part.name.clone());
      }
      if let Some(&gi2) = details.get((r(6) * details.len() as f32) as usize)
        && r(7) < 0.2
      {
        let s = kit.parts[gi2].size;
        let k = (room * 0.5 / s.x.max(s.z)).min(1.0);
        let quarter = (r(8) * 4.0).floor() * PI / 2.0;
        add(
          What::Part {
            index: gi2,
            scale: splat(k),
          },
          c,
          yaw(quarter),
        );
        plan
          .used
          .insert("ground_detail", kit.parts[gi2].part.name.clone());
      }
    }
  }
  plan.size = Vec3::new(w, t + rise, d);
  plan.anchor = 0;
  plan
}

/// The plan of a structure of the kit
pub fn plan_structure(kit: &BuildingKit, p: &StructureParams) -> HousePlan {
  match p.kind.as_str() {
    "wall" => plan_wall(kit, p),
    "floor" => plan_floor(kit, p),
    _ => plan_house(kit, &p.house()),
  }
}

// ------------------------------------------------------------------------------------------------
// in the editor
// ------------------------------------------------------------------------------------------------

impl Editor {
  /// A level's building kit, read once its decor is (None meanwhile, or when it cannot be read:
  /// `kit_failed`)
  pub fn building_kit(&mut self, level: &str) -> Option<&BuildingKit> {
    if !self.placing.kits.contains_key(level) {
      let bg = self.kit_background(level)?;
      let decor = self.decor_scene_of(&bg)?;
      let mut kit = building_kit(&decor);
      for p in &mut kit.parts {
        p.level = level.into();
      }
      self.placing.kits.insert(level.into(), kit);
    }
    self.placing.kits.get(level)
  }

  /// A level's kit for a house: a level without windows or doors of its own (the industrial
  /// zone's buildings have none) borrows Haven City's, else the slums'; None while one is read
  fn house_kit(&mut self, level: &str) -> Option<BuildingKit> {
    let mut kit = self.building_kit(level)?.clone();
    let lacks = |k: &BuildingKit| k.of(Role::Window).is_empty() || k.of(Role::Door).is_empty();
    // (a level of Jak II keeps to its own pieces)
    if lacks(&kit) && !level.starts_with("jak2/") {
      for donor in K_DONORS.iter().filter(|&&d| d != level) {
        if self.library.level(donor).is_none() || self.kit_failed(donor) {
          continue;
        }
        let d = self.building_kit(donor)?;
        if lacks(d) {
          continue;
        }
        let borrowed: Vec<KitPart> = d
          .parts
          .iter()
          .filter(|p| {
            matches!(
              p.role,
              Role::Window | Role::Glass | Role::Door | Role::Doorframe
            )
          })
          .filter(|p| kit.of(p.role).is_empty())
          .cloned()
          .collect();
        kit.parts.extend(borrowed);
        break;
      }
    }
    Some(kit)
  }

  pub fn kit_failed(&self, level: &str) -> bool {
    self
      .kit_background(level)
      .is_none_or(|bg| self.catalog.store.failed_level(&bg))
  }

  /// The decor file of a kit's level: one of Jak 3, or of Jak II named "jak2/<level>" (a prefab of
  /// Jak II closed with its own level's pieces)
  pub(crate) fn kit_background(&self, level: &str) -> Option<String> {
    match level.strip_prefix("jak2/") {
      Some(l) => self.library_jak2.level(l),
      None => self.library.level(level),
    }
    .map(|l| l.background.clone())
  }

  /// A material of a level's decor in the document (its texture once); -1 none
  pub(crate) fn decor_material(&mut self, level: &str, material: i32) -> i32 {
    if material < 0 {
      return -1;
    }
    let Some(bg) = self.kit_background(level) else {
      return -1;
    };
    let Some(decor) = self.decor_scene_of(&bg) else {
      return -1;
    };
    let mut one = Mesh::new();
    one.prims.push(Primitive {
      material,
      ..Default::default()
    });
    merge_mesh(&mut self.scene, &one, &decor.textures, &decor.materials).prims[0].material
  }

  /// A level's kit for a structure, and the decor of every level its parts come from read (again
  /// when the library let it go: it keeps two levels; else their meshes and materials would be
  /// missing). Err("reading") meanwhile, or the reason it cannot be.
  fn structure_kit(&mut self, level: &str) -> Result<(BuildingKit, Vec<Arc<Scene>>), String> {
    if self.kit_background(level).is_none() {
      return Err(format!(
        "no level '{level}' in the library (see list_levels)"
      ));
    }
    let Some(kit) = self.house_kit(level) else {
      return Err(if self.kit_failed(level) {
        format!("the decor of {level} cannot be read")
      } else {
        "reading".into()
      });
    };
    let levels: BTreeSet<String> = kit
      .parts
      .iter()
      .map(|p| p.level.clone())
      .chain([level.to_string()])
      .collect();
    let mut decors = Vec::new();
    for l in levels {
      let Some(bg) = self.kit_background(&l) else {
        continue;
      };
      match self.decor_scene_of(&bg) {
        Some(d) => decors.push(d),
        None if self.catalog.store.failed_level(&bg) => {
          return Err(format!("the decor of {l} cannot be read"));
        }
        None => return Err("reading".into()),
      }
    }
    Ok((kit, decors))
  }

  /// The pieces of a plan placed at `m` in the batch going on: one group `gid`, the anchor keeping
  /// the structure's parameters, the game's parts keeping their level's light. Its roots.
  fn place_plan(
    &mut self,
    kit: &BuildingKit,
    p: &StructureParams,
    plan: &HousePlan,
    m: &Mat4,
    gid: u32,
  ) -> Vec<u32> {
    // the looks' materials in the document, their tiles and colors
    let mut looks = BTreeMap::new();
    for look in [
      Look::Wall,
      Look::WallDark,
      Look::Roof,
      Look::Trim,
      Look::Dark,
      Look::Floor,
    ] {
      let s = match look {
        Look::Roof if kit.roof.material >= 0 => &kit.roof,
        Look::Trim if kit.trim.material >= 0 => &kit.trim,
        Look::Floor if kit.floor.material >= 0 => &kit.floor,
        _ => &kit.wall,
      };
      let mut color = s.color;
      match look {
        Look::WallDark => color = Vec4::new(color.x * 0.7, color.y * 0.7, color.z * 0.7, 1.0),
        Look::Dark => color = Vec4::new(0.04, 0.04, 0.05, 1.0),
        _ => {}
      }
      // a kit of the document's own materials (no level: closing a prefab of Jak II) as they are
      let material = if p.level.is_empty() {
        s.material
      } else {
        self.decor_material(&p.level, s.material)
      };
      looks.insert(look, (material, s.tile, color));
    }
    let saved = (self.tool, self.placing.placement.clone());
    let mut roots = Vec::new();
    let mut anchor = 0;
    for (k, piece) in plan.pieces.iter().enumerate() {
      let local = match &piece.what {
        // a box: in segments the game's level builder can split (the anchor's middle one keeps
        // its place)
        What::Shape { name, params, look } if params.kind == "box" => {
          let (material, tile, color) = looks[look];
          let segments = box_segments(params.size, k == plan.anchor);
          let middle = segments.len() / 2;
          for (i, (offset, size)) in segments.into_iter().enumerate() {
            let mut params = params.clone();
            params.tile = tile;
            params.color = color;
            params.size = size;
            self.start_shape_placement(&ShapePreset { name, params }, false);
            self.placement_mut().material = material;
            let at = piece.at + piece.turn.rotate(offset);
            let placed = self.place_at(&(*m * Mat4::translate(at) * Mat4::rotate(piece.turn)));
            if k == plan.anchor && i == middle {
              anchor = placed.first().copied().unwrap_or(0);
            }
            roots.extend(placed);
          }
          continue;
        }
        What::Shape { name, params, look } => {
          let (material, tile, color) = looks[look];
          let mut params = params.clone();
          params.tile = tile;
          params.color = color;
          self.start_shape_placement(&ShapePreset { name, params }, false);
          self.placement_mut().material = material;
          Mat4::translate(piece.at) * Mat4::rotate(piece.turn)
        }
        What::Part { index, scale } => {
          let kp = &kit.parts[*index];
          let Some(bg) = self.kit_background(&kp.level) else {
            continue;
          };
          self.start_part_placement(&bg, &kp.part, false);
          part_matrix(kp, piece, *scale)
        }
      };
      let placed = self.place_at(&(*m * local));
      if matches!(piece.what, What::Part { .. }) {
        for &id in &placed {
          if let Some(n) = self.scene.find_mut(id) {
            n.extras[K_KEEP_LIGHT] = Value::Bool(true);
          }
        }
      }
      if k == plan.anchor {
        anchor = placed.first().copied().unwrap_or(0);
      }
      roots.extend(placed);
    }
    // one group named after the kind and the level; the anchor keeps how it is made (to make it
    // again: a level's, not the document's own materials)
    let name = if p.level.is_empty() {
      format!("{}-closing", p.kind)
    } else {
      format!("{}-{}", p.kind, p.level)
    };
    for &id in &roots {
      if let Some(n) = self.scene.find_mut(id) {
        set_group_tag(n, gid, &name, false);
      }
    }
    if !p.level.is_empty()
      && let Some(n) = self.scene.find_mut(anchor)
    {
      n.extras[K_STRUCTURE] = p.to_json();
    }
    (self.tool, self.placing.placement) = saved;
    self.selection = roots.iter().copied().collect();
    if let Some(&r) = roots.first() {
      self.primary = r;
    }
    roots
  }

  /// A structure like the level's at `m` (its base in the middle of its footprint, its front
  /// towards the matrix's +z): one undo step, one group, selected. Err while the level's decor is
  /// read ("reading") or when it cannot be.
  pub fn build_structure(
    &mut self,
    p: &StructureParams,
    m: &Mat4,
  ) -> Result<(Vec<u32>, HousePlan), String> {
    let (kit, _decors) = self.structure_kit(&p.level)?;
    let plan = plan_structure(&kit, p);
    let gid = new_group_id(&self.scene);
    self.begin_place_batch();
    let roots = self.place_plan(&kit, p, &plan, m, gid);
    // the shapes lit like the place around them; the game's parts keep their level's light
    self.finish_place_batch(None, Some(structure_label(p)));
    Ok((roots, plan))
  }

  /// A house (build_house)
  pub fn build_house(
    &mut self,
    p: &StructureParams,
    m: &Mat4,
  ) -> Result<(Vec<u32>, HousePlan), String> {
    let mut p = p.clone();
    p.kind = "house".into();
    self.build_structure(&p, m)
  }

  /// The structure `id` is part of: its group, how it is made, its frame in the world (where its
  /// anchor is, turned as it is; never scaled)
  pub fn structure_of(&self, id: u32) -> Option<(u32, StructureParams, Mat4)> {
    let gid = self.group_of(id);
    if gid == 0 {
      return None;
    }
    let anchor = self
      .scene
      .nodes
      .iter()
      .find(|n| group_tag(n) == gid && n.extras.get(K_STRUCTURE).is_some())?;
    let p = StructureParams::from_json(&anchor.extras[K_STRUCTURE]);
    let t = crate::math::Transform::from_matrix(&self.scene.world_of(anchor.id));
    Some((gid, p, Mat4::translate(t.t) * Mat4::rotate(t.r)))
  }

  /// The structure `id` is part of made again with `p` where it stands (grown with more of the
  /// level's pieces, not stretched): one undo step, the same group. Its new roots.
  pub fn set_structure(&mut self, id: u32, p: &StructureParams) -> Result<Vec<u32>, String> {
    let Some((gid, old, frame)) = self.structure_of(id) else {
      return Err("not a structure".into());
    };
    let mut p = p.clone();
    p.kind = old.kind.clone();
    if p.level.is_empty() {
      p.level = old.level.clone();
    }
    self.rebuild_structure(gid, &p, &frame)
  }

  /// Structure `gid` made again with `p` at `frame`: one undo step
  fn rebuild_structure(
    &mut self,
    gid: u32,
    p: &StructureParams,
    frame: &Mat4,
  ) -> Result<Vec<u32>, String> {
    let p = p.clone();
    let (kit, _decors) = self.structure_kit(&p.level)?;
    let plan = plan_structure(&kit, &p);
    let ids: Vec<u32> = self
      .scene
      .nodes
      .iter()
      .filter(|n| group_tag(n) == gid)
      .map(|n| n.id)
      .collect();
    let mut change = NodeChange::new(&self.scene);
    change.save_subtrees(&self.scene, &ids);
    self.scene.remove(&ids);
    self.begin_place_batch_with(change);
    let roots = self.place_plan(&kit, &p, &plan, frame, gid);
    self.finish_place_batch(None, Some(structure_label(&p)));
    Ok(roots)
  }

  /// The end of a scale of the gizmo on a whole structure: made again at its new size instead of
  /// stretched (its base where it was). False when `ids` are not one structure, all of it.
  pub(crate) fn scale_structure(&mut self, ids: &[u32], worlds: &[Mat4]) -> bool {
    let Some(&first) = ids.first() else {
      return false;
    };
    let Some((gid, p, frame)) = self.structure_of(first) else {
      return false;
    };
    let group: BTreeSet<u32> = self
      .scene
      .nodes
      .iter()
      .filter(|n| {
        group_tag(n) == gid
          && self
            .scene
            .find(n.parent)
            .is_none_or(|q| group_tag(q) != gid)
      })
      .map(|n| n.id)
      .collect();
    if ids.iter().copied().collect::<BTreeSet<u32>>() != group {
      return false;
    }
    // the anchor's scale gives the structure's
    let Some(k) = ids.iter().position(|&id| {
      self
        .scene
        .find(id)
        .is_some_and(|n| n.extras.get(K_STRUCTURE).is_some())
    }) else {
      return false;
    };
    let before = crate::math::Transform::from_matrix(&self.scene.world_of(ids[k]));
    let after = crate::math::Transform::from_matrix(&worlds[k]);
    let s = Vec3::new(
      after.s.x / before.s.x.max(1e-6),
      after.s.y / before.s.y.max(1e-6),
      after.s.z / before.s.z.max(1e-6),
    );
    // its size as built (0: the level's default, measured from its plan)
    let Ok((kit, _)) = self.structure_kit(&p.level) else {
      return false;
    };
    let built = plan_structure(&kit, &p).size;
    let mut q = p.clone();
    match p.kind.as_str() {
      "house" => {
        let width = if p.size.x > 0.0 { p.size.x } else { built.x };
        let depth = if p.size.z > 0.0 { p.size.z } else { built.z };
        q.size = Vec3::new(width * s.x, 0.0, depth * s.z);
        q.floors = ((p.floors as f32 * s.y).round() as i32).clamp(1, 6);
      }
      _ => {
        for i in 0..3 {
          let was = if p.size[i] > 0.0 { p.size[i] } else { built[i] };
          q.size[i] = was * s[i];
        }
      }
    }
    // where it stands: its anchor moved, at the height it had
    let mut t = crate::math::Transform::from_matrix(&frame);
    t.t = Vec3::new(after.t.x, t.t.y, after.t.z);
    let m = Mat4::translate(t.t) * Mat4::rotate(t.r);
    self.rebuild_structure(gid, &q, &m).is_ok()
  }

  /// The Place tool with a structure: its box see-through under the cursor until it is placed;
  /// false while the level's decor is read
  pub fn start_structure_placement(&mut self, p: &StructureParams, once: bool) -> bool {
    let Ok((kit, _)) = self.structure_kit(&p.level) else {
      return false;
    };
    let plan = plan_structure(&kit, p);
    let mut preset = ShapePreset {
      name: "structure",
      params: ShapeParams {
        kind: "box".into(),
        size: plan.size,
        ..Default::default()
      },
    };
    preset.params.tile = plan.size.y.max(1.0);
    self.start_shape_placement(&preset, once);
    let pl = self.placement_mut();
    pl.kind = PlacementKind::House;
    pl.house = p.clone();
    pl.path = p.level.clone();
    true
  }

  /// A set of prefabs of a level's structures, saved with the user's prefabs: walls of 2 and 4
  /// bays, a high wall, a wall with windows, floors of 16 m and 32 m, houses of 1, 2 and 3 floors.
  /// Each grows when it is resized once placed. Their paths.
  pub fn create_prefab_set(&mut self, level: &str) -> Result<Vec<String>, String> {
    let folder = self.prefab_folder();
    if folder.is_empty() {
      return Err(tr("log.prefab_no_library").to_string());
    }
    let (kit, _decors) = self.structure_kit(level)?;
    let (fh, _) = module_sizes(&kit);
    let bay = (fh * 0.8).clamp(4.0, 12.0);
    let one = |kind: &str, size: Vec3| StructureParams {
      kind: kind.into(),
      level: level.into(),
      size,
      ..Default::default()
    };
    let mut set = vec![
      (
        format!("{level}-wall-{:.0}m", bay * 2.0),
        one("wall", Vec3::new(bay * 2.0, 0.0, 0.0)),
      ),
      (
        format!("{level}-wall-{:.0}m", bay * 4.0),
        one("wall", Vec3::new(bay * 4.0, 0.0, 0.0)),
      ),
      (
        format!("{level}-high-wall"),
        one("wall", Vec3::new(bay * 3.0, fh * 2.0, 0.0)),
      ),
      (format!("{level}-wall-with-windows"), {
        let mut p = one("wall", Vec3::new(bay * 3.0, fh * 2.0, 0.0));
        p.windows = true;
        p
      }),
      (
        format!("{level}-floor-16m"),
        one("floor", Vec3::new(16.0, 0.0, 16.0)),
      ),
      (
        format!("{level}-floor-32m"),
        one("floor", Vec3::new(32.0, 0.0, 32.0)),
      ),
    ];
    for floors in 1..=3 {
      let mut p = one("house", Vec3::default());
      p.floors = floors;
      set.push((format!("{level}-house-{floors}-floors"), p));
    }
    // each built in a document of its own, then saved
    let doc = std::mem::replace(&mut self.scene, Scene::new());
    let saved = (self.selection.clone(), self.primary);
    let mut paths = Vec::new();
    let mut failed = None;
    for (name, p) in &set {
      self.scene = Scene::new();
      self.forget_merged_meshes();
      let plan = plan_structure(&kit, p);
      self.begin_place_batch();
      let roots = self.place_plan(&kit, p, &plan, &Mat4::identity(), 1);
      self.discard_place_batch();
      self.scene.begin_frame();
      let path = new_prefab_path(&folder, name);
      match save_prefab(&self.scene, &roots, &path) {
        Ok(()) => paths.push(path),
        Err(e) => {
          failed = Some(e);
          break;
        }
      }
    }
    self.scene = doc;
    self.forget_merged_meshes();
    (self.selection, self.primary) = saved;
    self.scene.begin_frame();
    self.refresh_prefabs();
    match failed {
      Some(e) => Err(e),
      None => Ok(paths),
    }
  }

  /// get_state / the inspector: the structure `id` is part of, {kind, level, size (as built),
  /// floors, roof, wear, seed, windows, group}
  pub(crate) fn structure_info(&mut self, id: u32) -> Value {
    let Some((gid, p, _)) = self.structure_of(id) else {
      return Value::Null;
    };
    let mut j = p.to_json();
    if let Ok((kit, _)) = self.structure_kit(&p.level) {
      let built = plan_structure(&kit, &p).size;
      let r = |v: f32| (v * 100.0).round() / 100.0;
      let size = |i: usize| if p.size[i] > 0.0 { p.size[i] } else { built[i] };
      j["size"] = json!([r(size(0)), r(size(1)), r(size(2))]);
    }
    j["group"] = json!(gid);
    j
  }

  /// The calls of the structures (api.rs): None when `name` is not one of them
  pub(crate) fn structures_call(&mut self, name: &str, a: &Value) -> Option<Result<Value, String>> {
    let level = a.get("level").and_then(Value::as_str).unwrap_or_default();
    let reading = |ed: &Editor, e: String| {
      if e == "reading" {
        Ok(json!({"reading": true}))
      } else if ed.kit_failed(level) {
        Err(format!("the decor of {level} cannot be read"))
      } else {
        Err(e)
      }
    };
    Some(match name {
      // {level} -> the kit ({reading: true} while its decor is read)
      "building_kit" => match self.building_kit(level).map(BuildingKit::to_json) {
        Some(k) => Ok(k),
        None if self.kit_failed(level) => Err(format!("the decor of {level} cannot be read")),
        None => Ok(json!({"reading": true})),
      },
      // {kind, level, size, floors, roof, wear, seed, windows, once} -> the Place tool with it
      "start_structure_placement" | "start_house_placement" => {
        let once = a.get("once").and_then(Value::as_bool).unwrap_or(false);
        let mut p = StructureParams::from_json(a);
        if name == "start_house_placement" {
          p.kind = "house".into();
        }
        if self.start_structure_placement(&p, once) {
          Ok(Value::Null)
        } else {
          let e = self.structure_kit(level).err().unwrap_or_default();
          reading(self, e)
        }
      }
      // {id} -> the structure it is part of (null: none)
      "structure_info" => {
        let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
        Ok(self.structure_info(id))
      }
      // {id, size, floors, roof, wear, seed, windows} -> made again ({reading: true} meanwhile)
      "set_structure" => {
        let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
        let Some((_, old, _)) = self.structure_of(id) else {
          return Some(Err("not a structure".into()));
        };
        let mut j = old.to_json();
        if let Some(o) = a.as_object() {
          for (k, v) in o {
            if k != "id" && k != "kind" {
              j[k] = v.clone();
            }
          }
        }
        match self.set_structure(id, &StructureParams::from_json(&j)) {
          Ok(roots) => Ok(json!({"roots": roots})),
          Err(e) => reading(self, e),
        }
      }
      // {level} -> the set's paths ({reading: true} meanwhile)
      "create_prefab_set" => match self.create_prefab_set(level) {
        Ok(paths) => Ok(json!({"paths": paths})),
        Err(e) => reading(self, e),
      },
      _ => return None,
    })
  }
}

/// A box (its base's middle at the origin) cut into segments no longer than 24 m across and 12 m
/// high, or than six times its middle size (a panel as wide as high stays whole): the game's level
/// builder cannot split a collision fragment much longer than it is thick (a wall bay 60 m high
/// and 2 m wide fails its build). Each segment's base middle and size; an odd count along each
/// axis when `centered`, its middle segment at the origin.
fn box_segments(size: Vec3, centered: bool) -> Vec<(Vec3, Vec3)> {
  let mut dims = [size.x, size.y, size.z];
  dims.sort_by(f32::total_cmp);
  let slender = dims[1] * 6.0;
  let count = |d: f32, max: f32| {
    let n = (d / max.max(slender)).ceil().max(1.0) as usize;
    if centered && n % 2 == 0 { n + 1 } else { n }
  };
  let (nx, ny, nz) = (
    count(size.x, 24.0),
    count(size.y, 12.0),
    count(size.z, 24.0),
  );
  let part = Vec3::new(size.x / nx as f32, size.y / ny as f32, size.z / nz as f32);
  let mut out = Vec::with_capacity(nx * ny * nz);
  for j in 0..ny {
    for i in 0..nx {
      for k in 0..nz {
        let at = Vec3::new(
          -size.x / 2.0 + part.x * (i as f32 + 0.5),
          part.y * j as f32,
          -size.z / 2.0 + part.z * (k as f32 + 0.5),
        );
        out.push((at, part));
      }
    }
  }
  // centered: the segment at the origin (the lowest row's middle) at the middle of the list
  if centered {
    let at_origin = (nx / 2) * nz + nz / 2;
    let middle = out.len() / 2;
    out.swap(at_origin, middle);
  }
  out
}

/// the undo step of a structure: "Wall like ctyinda"
fn structure_label(p: &StructureParams) -> String {
  strf!(tr(&format!("undo.structure.{}", p.kind)), &p.level)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn roles_of_the_games_parts() {
    let cases = [
      ("city-slum-window-01.mb", Some(Role::Window)),
      ("wascity-window-frame-02.mb", Some(Role::Window)),
      ("city-window-glass-litwindow-01.mb", Some(Role::Glass)),
      ("city-slum-window-shade-01.mb", Some(Role::Shade)),
      ("city-slum-window-stain-01.mb", Some(Role::Wear)),
      ("city-slum-door-02.mb", Some(Role::Door)),
      ("wascity-door-fingerplates-01.mb", Some(Role::Door)),
      ("city-doorframe-metal-01.mb", Some(Role::Doorframe)),
      ("city-door-window-frame.mb", None),
      ("city-ind-scorch-mark-01.mb", Some(Role::Wear)),
      ("city-slum-dirt-patch-01.mb", Some(Role::GroundDecal)),
      ("city-ind-ground-vent-small-01.mb", Some(Role::GroundDetail)),
      (
        "city-ind-facade-deco-bigbuldge-1x1-01.mb",
        Some(Role::Relief),
      ),
      ("wascity-wall-outer-panel-03.mb", Some(Role::Relief)),
      ("wascity-wallbuldge-broken-01.mb", None),
      ("city-ind-joint-BOT-01.mb", Some(Role::Column)),
      ("city-build-seam-cover-cap-01.mb", None),
      ("city-woodbeam-small-01.mb", Some(Role::Beam)),
      ("city-woodbeam-poke-support.mb", None),
      ("city-slum-roof-metalplates-damaged-01.mb", Some(Role::Roof)),
      ("city-roof-structure.mb", None),
      ("city-slum-chimney-01.mb", Some(Role::Chimney)),
      ("wascity-chimney-hires.mb", None),
      ("city-slum-joint-medpipe-01.mb", Some(Role::Pipe)),
      ("city-slum-pipe-bracket-ring-01.mb", Some(Role::PipeRing)),
      ("city-ind-bigpipe-straight-01.mb", None),
      ("city-slum-facade-mid-24w-12h-01.mb", Some(Role::Facade)),
      ("city-slum-facade-mid-12w-12h-01-BLASTED.mb", None),
      ("city-slum-awning-rustymetal-01.mb", Some(Role::Awning)),
      ("wascity-awning-pole-01.mb", None),
    ];
    for (name, role) in cases {
      assert_eq!(role_of(name), role, "{name}");
    }
  }

  /// a kit of boxes: a window 2 x 3, a door 2 x 4, a pipe lying along x, a stain
  fn kit() -> BuildingKit {
    let part = |role, name: &str, raw: Vec3, rot: Quat| {
      let (lo, hi) = turned_box(raw, rot, splat(1.0));
      KitPart {
        role,
        part: CatalogPart {
          name: name.into(),
          copies: 1,
          ..Default::default()
        },
        raw,
        rot,
        size: hi - lo,
        rise: Vec3::default(),
        slope: 0.0,
        level: String::new(),
      }
    };
    let up = Quat::axis_angle(Vec3::new(0.0, 0.0, 1.0), PI / 2.0);
    BuildingKit {
      parts: vec![
        part(Role::Window, "w", Vec3::new(2.0, 3.0, 0.4), Quat::default()),
        part(Role::Door, "d", Vec3::new(2.0, 4.0, 0.2), Quat::default()),
        part(Role::Pipe, "p", Vec3::new(5.0, 0.5, 0.5), up),
        part(
          Role::Wear,
          "stain",
          Vec3::new(1.0, 2.0, 0.0),
          Quat::default(),
        ),
      ],
      wall: Surface {
        material: 0,
        ..Default::default()
      },
      ..Default::default()
    }
  }

  #[test]
  fn a_house_planned_from_a_kit() {
    let kit = kit();
    let o = HouseOptions {
      floors: 2,
      ..Default::default()
    };
    let plan = plan_house(&kit, &o);
    let count = |f: &dyn Fn(&What) -> bool| plan.pieces.iter().filter(|p| f(&p.what)).count();
    let part = |n: usize| move |w: &What| matches!(w, What::Part { index, .. } if *index == n);
    let shape =
      |k: &'static str| move |w: &What| matches!(w, What::Shape { params, .. } if params.kind == k);
    // one door, in a doorway of the front's ground floor; windows in openings their size
    assert_eq!(count(&part(1)), 1);
    assert_eq!(count(&shape("doorway")), 1);
    assert!(count(&part(0)) >= 6 && count(&part(0)) == count(&shape("window")));
    // the pipe from the ground to the roof: upright, its pieces stacked
    let pipes: Vec<&Piece> = plan.pieces.iter().filter(|p| part(2)(&p.what)).collect();
    let h = (4.0f32 * 1.45).max(3.0 * 2.0); // the floor's height
    assert!(!pipes.is_empty() && pipes.iter().all(|p| p.at.y < 2.0 * h));
    let m = part_matrix(&kit.parts[2], pipes[0], splat(1.0));
    let (lo, hi) = (
      m.point(Vec3::new(-2.5, 0.0, 0.0)),
      m.point(Vec3::new(2.5, 0.5, 0.0)),
    );
    assert!((lo.y.min(hi.y) - pipes[0].at.y).abs() < 1e-3 && (hi.y - lo.y).abs() > 4.9);
    // a roof over all of it, wear on the walls, the size of it all
    assert_eq!(count(&shape("roof")), 1);
    assert!(count(&part(3)) > 0);
    assert!(plan.size.y > 2.0 * h && plan.size.x > 3.0 * 2.0 * 1.9);
    // the same seed, the same house; flat roofs asked: a flat roof
    assert_eq!(plan_house(&kit, &o).pieces.len(), plan.pieces.len());
    let flat = plan_house(
      &kit,
      &HouseOptions {
        roof: "flat".into(),
        ..o.clone()
      },
    );
    assert!(!flat.pieces.iter().any(|p| shape("roof")(&p.what)));
  }

  #[test]
  fn a_house_of_the_slums_built() {
    // on the user's extracted game: the slums' kit, a house of it, one group, one undo step
    let Some(root) = crate::io::test_library::real_library() else {
      eprintln!("skipped: no library/jak3/library.json");
      return;
    };
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.set_library(root.to_str().unwrap());
    ed.new_document();
    let o = StructureParams {
      level: "ctysluma".into(),
      ..Default::default()
    };
    let mut built = None;
    for _ in 0..6000 {
      ed.poll_library_work();
      match ed.build_house(&o, &Mat4::identity()) {
        Err(e) if e == "reading" => std::thread::sleep(std::time::Duration::from_millis(10)),
        r => {
          built = Some(r.unwrap());
          break;
        }
      }
    }
    let (roots, plan) = built.expect("built");
    let kit = ed.building_kit("ctysluma").unwrap();
    assert!(!kit.of(Role::Window).is_empty() && !kit.of(Role::Door).is_empty());
    assert!(kit.wall.material >= 0 && kit.roof.material >= 0 && !kit.flat_roofs);
    assert!(roots.len() > 50 && plan.used.contains_key("window") && plan.used.contains_key("roof"));
    let groups: std::collections::BTreeSet<u64> = roots
      .iter()
      .filter_map(|&id| ed.scene.find(id)?.extras["og_group"]["id"].as_u64())
      .collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(ed.history.labels().len(), 1);
    ed.undo_redo(true);
    assert!(roots.iter().all(|&id| ed.scene.find(id).is_none()));
  }

  #[test]
  fn an_industrial_wall_grown_in_the_editor() {
    // on the user's extracted game: a wall of the industrial zone, made twice as long, undone
    let Some(root) = crate::io::test_library::real_library() else {
      eprintln!("skipped: no library/jak3/library.json");
      return;
    };
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.set_library(root.to_str().unwrap());
    ed.new_document();
    let p = StructureParams {
      kind: "wall".into(),
      level: "ctyinda".into(),
      size: Vec3::new(24.0, 12.0, 0.0),
      ..Default::default()
    };
    let roots = loop {
      ed.poll_library_work();
      match ed.build_structure(&p, &Mat4::translate(Vec3::new(10.0, 0.0, 5.0))) {
        Err(e) if e == "reading" => std::thread::sleep(std::time::Duration::from_millis(10)),
        r => break r.unwrap().0,
      }
    };
    let (gid, kept, frame) = ed.structure_of(roots[3]).expect("a structure");
    assert!(kept.kind == "wall" && frame.col3(3) == Vec3::new(10.0, 0.0, 5.0));
    let in_group = |ed: &Editor| {
      ed.scene
        .nodes
        .iter()
        .filter(|n| group_tag(n) == gid)
        .count()
    };
    let before = in_group(&ed);
    let longer = StructureParams {
      size: Vec3::new(48.0, 12.0, 0.0),
      ..kept.clone()
    };
    ed.set_structure(roots[0], &longer).unwrap();
    assert!(in_group(&ed) > before, "{} {before}", in_group(&ed));
    // the game's parts keep their level's light; one step to undo
    assert!(
      ed.scene
        .nodes
        .iter()
        .any(|n| group_tag(n) == gid && n.extras.get(K_KEEP_LIGHT).is_some())
    );
    ed.undo_redo(true);
    assert_eq!(in_group(&ed), before);
    // the scale gizmo let go on all of it: made again 1.5 times longer, nothing stretched
    let roots: Vec<u32> = ed
      .scene
      .nodes
      .iter()
      .filter(|n| group_tag(n) == gid)
      .map(|n| n.id)
      .collect();
    let pivot = Vec3::new(10.0, 0.0, 5.0);
    let s =
      Mat4::translate(pivot) * Mat4::scale(Vec3::new(1.5, 1.0, 1.0)) * Mat4::translate(-pivot);
    let worlds: Vec<Mat4> = roots.iter().map(|&r| s * ed.scene.world_of(r)).collect();
    ed.transform_nodes(&roots, &worlds, "undo.scale");
    let (_, p2, _) = ed
      .structure_of(*ed.selection.iter().next().unwrap())
      .unwrap();
    assert!((p2.size.x - 36.0).abs() < 0.1, "{:?}", p2.size);
    let anchor = ed
      .scene
      .nodes
      .iter()
      .find(|n| group_tag(n) == gid && n.extras.get(K_STRUCTURE).is_some())
      .unwrap();
    assert!((anchor.local.s.x - 1.0).abs() < 1e-4);
  }

  #[test]
  fn a_set_of_prefabs_that_grow_once_placed() {
    // on the user's extracted game: a set of Spargus' structures saved (in a folder of the test's),
    // one placed, then made longer: it is still a structure
    let Some(root) = crate::io::test_library::real_library() else {
      eprintln!("skipped: no library/jak3/library.json");
      return;
    };
    let tmp = crate::io::test_library::temp_dir("structure-set");
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.set_library(root.to_str().unwrap());
    ed.placing.prefab_dir_override = tmp.to_string_lossy().into();
    ed.new_document();
    let paths = loop {
      ed.poll_library_work();
      match ed.create_prefab_set("wascitya") {
        Err(e) if e == "reading" => std::thread::sleep(std::time::Duration::from_millis(10)),
        r => break r.unwrap(),
      }
    };
    assert_eq!(paths.len(), 9);
    assert!(paths.iter().all(|p| std::path::Path::new(p).is_file()));
    // the document left as it was
    assert!(ed.scene.nodes.is_empty() && ed.history.labels().is_empty());
    let wall = paths
      .iter()
      .find(|p| p.contains("wall-with-windows"))
      .unwrap();
    ed.start_placement(wall, "wall", true);
    let roots = loop {
      ed.poll_library_work();
      let r = ed.place_at(&Mat4::identity());
      if !r.is_empty() {
        break r;
      }
      std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let (_, p, _) = ed.structure_of(roots[0]).expect("still a structure");
    assert!(p.kind == "wall" && p.windows && p.level == "wascitya");
    let longer = StructureParams {
      size: Vec3::new(p.size.x.max(30.0) * 2.0, p.size.y, p.size.z),
      ..p
    };
    assert!(ed.set_structure(roots[0], &longer).is_ok());
    let _ = std::fs::remove_dir_all(&tmp);
  }

  #[test]
  fn a_wall_and_a_floor_that_grow() {
    // a kit with a relief panel: on both faces of each bay
    let mut kit = kit();
    let relief = kit.parts[0].clone();
    kit.parts.push(KitPart {
      role: Role::Relief,
      part: CatalogPart {
        name: "bulge".into(),
        copies: 1,
        ..Default::default()
      },
      ..relief
    });
    let wall = |len: f32| {
      plan_wall(
        &kit,
        &StructureParams {
          kind: "wall".into(),
          size: Vec3::new(len, 0.0, 0.0),
          wear: 0.0,
          ..Default::default()
        },
      )
    };
    let (fh, _) = module_sizes(&kit);
    let bay = (fh * 0.8).clamp(4.0, 12.0);
    let (short, long) = (wall(bay * 3.0), wall(bay * 6.0));
    let count = |p: &HousePlan, n: &str| {
      p.pieces
        .iter()
        .filter(|x| matches!(&x.what, What::Shape { name, .. } if *name == n))
        .count()
    };
    // solid bays, a pillar between each, more of them when it is longer: not stretched
    assert_eq!(
      (count(&short, "wall-bay"), count(&short, "wall-pillar")),
      (3, 4)
    );
    assert_eq!(
      (count(&long, "wall-bay"), count(&long, "wall-pillar")),
      (6, 7)
    );
    // relief on both faces: as many on the back as on the front
    let reliefs = |p: &HousePlan| -> (usize, usize) {
      let on = |front: bool| {
        p.pieces
          .iter()
          .filter(|x| matches!(x.what, What::Part { index: 4, .. }) && (x.at.z > 0.0) == front)
          .count()
      };
      (on(true), on(false))
    };
    let (f, b) = reliefs(&short);
    assert!(f > 0 && f == b, "{f} {b}");
    // the first bays keep what they had: the same draws from their number
    let first = |p: &HousePlan, l: f32| -> Vec<(i32, i32)> {
      p.pieces
        .iter()
        .filter(|x| matches!(x.what, What::Part { .. }) && x.at.x < -l / 2.0 + bay * 3.0)
        .map(|x| {
          (
            ((x.at.x + l / 2.0) * 10.0).round() as i32,
            (x.at.y * 10.0).round() as i32,
          )
        })
        .collect()
    };
    let (mut a, mut c) = (first(&short, bay * 3.0), first(&long, bay * 6.0));
    a.sort();
    c.sort();
    assert_eq!(a, c);
    // the anchor at the origin, not turned
    let anchor = &short.pieces[short.anchor];
    assert!(anchor.at == Vec3::default() && anchor.turn == Quat::default());
    // a floor: a slab, its curb, the seams between its cells; more cells when it is larger
    let floor = |w: f32| {
      plan_floor(
        &kit,
        &StructureParams {
          kind: "floor".into(),
          size: Vec3::new(w, 0.0, w),
          ..Default::default()
        },
      )
    };
    let (small, large) = (floor(16.0), floor(48.0));
    assert!(count(&small, "floor-slab") == 1 && count(&small, "floor-curb") == 4);
    assert!(count(&large, "floor-seam") > count(&small, "floor-seam"));
    // the parameters kept by name
    let p = StructureParams::from_json(&json!({"kind": "wall", "length": 30, "height": 9}));
    assert!(p.kind == "wall" && p.size == Vec3::new(30.0, 9.0, 0.0));
    assert_eq!(StructureParams::from_json(&p.to_json()), p);
  }

  #[test]
  fn a_part_turned_to_face_out() {
    // a window facing -x in its mesh: turned to face +z, then put on the right wall (facing +x):
    // its front faces +x, its base where asked
    let k = KitPart {
      role: Role::Window,
      part: CatalogPart::default(),
      raw: Vec3::new(0.2, 3.0, 2.0),
      rot: yaw(PI / 2.0),
      size: Vec3::new(2.0, 3.0, 0.2),
      rise: Vec3::default(),
      slope: 0.0,
      level: String::new(),
    };
    let piece = Piece {
      what: What::Part {
        index: 0,
        scale: splat(1.0),
      },
      at: Vec3::new(5.0, 1.0, 0.0),
      turn: yaw(PI / 2.0),
    };
    let m = part_matrix(&k, &piece, splat(1.0));
    let front = m.point(Vec3::new(-0.1, 0.0, 0.0)) - m.point(Vec3::default());
    assert!(front.x > 0.09, "{front:?}");
    let base = m.point(Vec3::new(0.0, 0.0, 0.0));
    assert!((base - Vec3::new(5.0, 1.0, 0.0)).x.abs() < 1e-3 && (base.y - 1.0).abs() < 1e-3);
  }

  #[test]
  fn long_boxes_in_segments() {
    // a wall bay 60 m high, 2 m wide: five segments of 12 m (no longer than six times 2 m); a
    // plinth 100 m long, centered: five of 20 m, the middle one at the origin and in the middle
    let s = box_segments(Vec3::new(2.0, 60.0, 1.0), false);
    assert!(s.len() == 5 && s.iter().all(|(_, z)| (z.y - 12.0).abs() < 1e-4));
    assert!((s[4].0.y - 48.0).abs() < 1e-4);
    let s = box_segments(Vec3::new(100.0, 1.0, 2.0), true);
    assert_eq!(s.len(), 5);
    assert!(
      length(s[2].0) < 1e-4 && (s[2].1.x - 20.0).abs() < 1e-4,
      "{s:?}"
    );
    // a panel 36 m wide and 40 m high stays whole
    assert_eq!(box_segments(Vec3::new(36.0, 40.0, 1.5), false).len(), 1);
    // a wall closing a landmark 300 m wide and 570 m high: a few hundred pieces, not thousands
    let plan = plan_wall(
      &BuildingKit::default(),
      &StructureParams {
        kind: "wall".into(),
        size: Vec3::new(300.0, 570.0, 1.5),
        ..Default::default()
      },
    );
    assert!(plan.pieces.len() < 400, "{}", plan.pieces.len());
    // even counts become odd when centered: 30 m is two of 24 m at most, three centered
    assert_eq!(box_segments(Vec3::new(30.0, 1.0, 1.0), false).len(), 2);
    assert_eq!(box_segments(Vec3::new(30.0, 1.0, 1.0), true).len(), 3);
  }

  #[test]
  fn a_wall_without_relief_gets_panels() {
    // a level with no relief parts: a raised panel in each bay, on each floor, on both faces,
    // standing out of them
    let kit = BuildingKit {
      module: 4.0,
      ..Default::default()
    };
    let p = StructureParams {
      kind: "wall".into(),
      size: Vec3::new(24.0, 8.0, 1.0),
      ..Default::default()
    };
    let plan = plan_wall(&kit, &p);
    let panels: Vec<&Piece> = plan
      .pieces
      .iter()
      .filter(|p| {
        matches!(
          p.what,
          What::Shape {
            name: "wall-panel",
            ..
          }
        )
      })
      .collect();
    let bays = plan
      .pieces
      .iter()
      .filter(|p| {
        matches!(
          p.what,
          What::Shape {
            name: "wall-bay",
            ..
          }
        )
      })
      .count();
    assert_eq!(panels.len(), bays * 2 * 2, "{bays} bays, 2 floors, 2 faces");
    assert!(panels.iter().all(|p| p.at.z.abs() > 0.4), "on the faces");
  }
}
