//! The particle effects of Jak 3 (steam, sparks, smoke, fire, lights) as the game makes them: its
//! definitions (data/jak3/particles.json, read in its code by tools/gen_particles.py) run by its
//! rules (sparticle-launcher.gc, sparticle.gc). A group launches the particles of its parts over
//! time (continuously, in bursts every period, or once); a particle starts from its launcher's
//! fields (a value and a random range each: its position, velocity, size, rotation, color and how
//! they change, its life) and moves 60 times a second: velocities are per tick (300 a second, 5 a
//! frame), friction and acceleration per frame. At a time of its life it can take the fields of
//! another launcher (next-launcher: steam that fades out). Fire, flames and lava take their color,
//! alpha and size from the game's curves over their life (birth-func-curve, live-func-curve).
//!
//! The editor shows them where the effects of the levels are (the actors naming a group by their
//! art-name: part-spawner...), and in the catalog. The view runs them in the interface
//! (ui/render/particles.ts, the same rules); this is the reference, proven by the tests.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use serde_json::Value;

use super::{BlendMode, NodeKind, Scene};
use crate::math::{AABB, K_PI, Mat4, Quat, Vec3, Vec4, length, normalize};

/// sp-field-id
pub mod sp {
  pub const TEXTURE: usize = 1;
  pub const NUM: usize = 6;
  pub const X: usize = 10;
  pub const Y: usize = 11;
  pub const Z: usize = 12;
  pub const SCALE_X: usize = 13;
  pub const ROT_X: usize = 14;
  pub const ROT_Y: usize = 15;
  pub const ROT_Z: usize = 16;
  pub const SCALE_Y: usize = 17;
  pub const R: usize = 18;
  pub const G: usize = 19;
  pub const B: usize = 20;
  pub const A: usize = 21;
  pub const OMEGA: usize = 24;
  pub const VEL_X: usize = 25;
  pub const VEL_Y: usize = 26;
  pub const VEL_Z: usize = 27;
  pub const SCALE_VEL_X: usize = 28;
  pub const ROT_VEL_X: usize = 29;
  pub const ROT_VEL_Y: usize = 30;
  pub const ROT_VEL_Z: usize = 31;
  pub const SCALE_VEL_Y: usize = 32;
  pub const FADE_R: usize = 33;
  pub const FADE_G: usize = 34;
  pub const FADE_B: usize = 35;
  pub const FADE_A: usize = 36;
  pub const ACCEL_X: usize = 37;
  pub const ACCEL_Y: usize = 38;
  pub const ACCEL_Z: usize = 39;
  pub const FRICTION: usize = 45;
  pub const TIMER: usize = 46;
  pub const FLAGS: usize = 47;
  pub const NEXT_TIME: usize = 50;
  pub const NEXT_LAUNCHER: usize = 51;
  pub const LAUNCH_ROT_X: usize = 54;
  pub const LAUNCH_ROT_Y: usize = 55;
  pub const LAUNCH_ROT_Z: usize = 56;
  pub const CONE_ROT_X: usize = 58;
  pub const CONE_ROT_Y: usize = 59;
  pub const CONE_ROT_Z: usize = 60;
  pub const ROTATE_X: usize = 62;
  pub const ROTATE_Y: usize = 63;
  pub const ROTATE_Z: usize = 64;
  pub const CONE_ROT_RADIUS: usize = 65;
  pub const COUNT: usize = 72;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpKind {
  Float,
  FloatIntRand,
  Int,
  Copy,
  Next,
  Flags,
  Texture,
  #[default]
  Other,
}

/// an init spec of a launcher: how a field starts
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpSpec {
  pub kind: SpKind,
  pub field: u8,
  /// the value, its random range, its random multiplier
  pub a: f32,
  pub b: f32,
  pub c: f32,
  /// Copy: the field copied; Next: the launcher; Texture: its index
  pub ref_id: i32,
  /// Flags: sp-cpuinfo-flag bits
  pub flags: u32,
}

// sp-cpuinfo-flag bits the editor uses
/// sp-cpuinfo-flag-3: added to what is behind (the GS's alpha #x48)
pub const K_SP_ADD: u32 = 1 << 3;
/// sp-cpuinfo-flag-4: taken from what is behind (#x42)
pub const K_SP_SUB: u32 = 1 << 4;
/// drawn by the glow renderer: added to what is behind
pub const K_SP_GLOW: u32 = 1 << 15;
/// its acceleration in the world (not turned with the effect)
pub const K_SP_GLOBAL_ACC: u32 = 1 << 16;

/// a part of a group: a launcher and when it launches (sp-group-item)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpItem {
  pub launcher: i32,
  /// is-3d sp1..sp15: bit 3 once, at the start
  pub flags: u16,
  /// ticks: launching length ticks every period (0: always)
  pub period: i32,
  pub length: i32,
  pub offset: i32,
  /// meters: not launched beyond, fewer up to
  pub fade_after: f32,
  pub falloff_to: f32,
  /// not launched at these hours (bit h)
  pub hour_mask: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpGroup {
  pub name: String,
  /// sp-group-flag: bit 4, the effect turned with its actor
  pub flags: u16,
  /// meters: its bounds
  pub radius: f32,
  pub items: Vec<SpItem>,
}

/// How a particle mixes with what is behind it (sp-launch-particles-var: the GS's alpha #x44,
/// mixed, unless flag 4, subtracted, or flag 3, added: most fire, sparks and lights, their textures
/// opaque, their shape in their colors on black).
pub fn blend_of(flags: u32) -> BlendMode {
  if flags & (K_SP_ADD | K_SP_GLOW) != 0 {
    BlendMode::AddAlpha
  } else if flags & K_SP_SUB != 0 {
    BlendMode::Subtract
  } else {
    BlendMode::Alpha
  }
}

/// A curve of the game (curve2d-fast; curve-color-fast: ys 4 colors), its keys at -xs: odx the
/// slope of each segment (2d), or 1 / its length (colors).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
  pub xs: [f32; 4],
  pub ys: Vec<f32>,
  pub odx: [f32; 4],
}

impl Curve {
  /// {xs, ys, odx}, ys `n` numbers (4, or 16 for colors)
  fn from_json(v: &Value, n: usize) -> Option<Curve> {
    let list = |k: &str, n: usize| {
      let a = v.get(k)?.as_array().filter(|a| a.len() == n)?;
      a.iter().map(num).collect::<Option<Vec<f32>>>()
    };
    Some(Curve {
      xs: list("xs", 4)?.try_into().ok()?,
      ys: list("ys", n)?,
      odx: list("odx", 4)?.try_into().ok()?,
    })
  }

  /// curve2d-fast at x (0 to 1)
  pub fn at(&self, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let k = if x + self.xs[2] >= 0.0 {
      2
    } else if x + self.xs[1] >= 0.0 {
      1
    } else {
      0
    };
    self.ys[k] + (x + self.xs[k]) * self.odx[k]
  }

  /// curve-color-fast at x: between the two colors of its segment (the sign as the VU tests it)
  pub fn color(&self, x: f32) -> [f32; 3] {
    let w = |k: usize| (x + self.xs[k]) * self.odx[k];
    let k = if !w(2).is_sign_negative() {
      2
    } else if !w(1).is_sign_negative() {
      1
    } else {
      0
    };
    let f = w(k);
    std::array::from_fn(|i| self.ys[k * 4 + i] * (1.0 - f) + self.ys[(k + 1) * 4 + i] * f)
  }
}

/// in the game's order: picked at random at birth, then scaled over the life (0 to 1)
const CURVE_FIELDS: [&str; 10] = [
  "color-start",
  "alpha-start",
  "scale-x-start",
  "scale-y-start",
  "r-scalar",
  "g-scalar",
  "b-scalar",
  "a-scalar",
  "scale-x-scalar",
  "scale-y-scalar",
];

/// particle-curve-settings: a launcher drawn by curves (birth-func-curve, live-func-curve)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurveSettings {
  /// ticks, and a random offset
  pub life: i32,
  pub offset: i32,
  /// pcf0 1: scale-y is scale-x; pcf1 2: the color over its life; pcf2 4: color and size at birth
  pub flags: u32,
  /// the functions it runs
  pub birth: bool,
  pub live: bool,
  /// CURVE_FIELDS
  pub curves: [Option<Curve>; 10],
}

// sp-cpuinfo-flag
fn cpuinfo_flag(name: &str) -> u32 {
  let bit = match name {
    "ready-to-launch" => 6,
    "distort" => 7,
    "aux-list" => 8,
    "level0" => 10,
    "level1" => 11,
    "glow" => 15,
    "use-global-acc" => 16,
    "launch-along-z" => 17,
    "left-multiply-quat" => 18,
    "right-multiply-quat" => 19,
    "set-conerot" => 20,
    _ => match name.strip_prefix("sp-cpuinfo-flag-") {
      Some(n) => atoi(n),
      None => return 0,
    },
  };
  1u32.checked_shl(bit as u32).unwrap_or(0)
}

// the leading digits of a text (0 without)
fn atoi(s: &str) -> i32 {
  let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
  s[..end].parse().unwrap_or(0)
}

// sp1, sp7, is-3d...: their bit
fn sp_bits(names: Option<&Value>) -> u16 {
  let mut out = 0u16;
  for s in names
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
  {
    if s == "is-3d" {
      out |= 1;
    } else if let Some(n) = s.strip_prefix("sp").filter(|_| s.len() > 2) {
      out |= 1u32.checked_shl(atoi(n) as u32).unwrap_or(0) as u16;
    }
  }
  out
}

// an angle of the game (65536 a turn) as radians
fn angle(units: f32) -> f32 {
  units * (2.0 * K_PI / 65536.0)
}

fn rotation(rx: f32, ry: f32, rz: f32, cone: bool) -> Mat4 {
  let x = Mat4::rotate(Quat::axis_angle(Vec3::new(1.0, 0.0, 0.0), angle(rx)));
  let y = Mat4::rotate(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), angle(ry)));
  let z = Mat4::rotate(Quat::axis_angle(Vec3::new(0.0, 0.0, 1.0), angle(rz)));
  // the cone turns the launch away from its axis (z, then x and y), the rotation turns that around
  if cone { z * x * y } else { y * x * z }
}

fn num(v: &Value) -> Option<f32> {
  v.as_f64().map(|v| v as f32)
}

#[derive(Clone, Debug, Default)]
pub struct ParticleData {
  groups: HashMap<String, SpGroup>,
  launchers: HashMap<i32, Vec<SpSpec>>,
  curves: HashMap<i32, Arc<CurveSettings>>,
  textures: Vec<String>,
}

impl ParticleData {
  pub fn load(path: &Path) -> Result<ParticleData, String> {
    let bytes = std::fs::read(path).map_err(|_| format!("cannot read {}", path.display()))?;
    serde_json::from_slice::<Value>(&bytes)
      .ok()
      .and_then(|j| Self::from_json(&j))
      .ok_or_else(|| format!("not the editor's particles: {}", path.display()))
  }

  /// data/jak3/particles.json's contents; None when it is not that
  pub fn from_json(j: &Value) -> Option<ParticleData> {
    let (groups, launchers) = (j.get("groups")?, j.get("launchers")?);
    let mut out = ParticleData::default();
    let mut texture_index: HashMap<String, i32> = HashMap::new();
    for (id, specs) in launchers.as_object().into_iter().flatten() {
      let list = out.launchers.entry(atoi(id)).or_default();
      for s in specs.as_array().into_iter().flatten() {
        let Some(s) = s.as_array().filter(|s| s.len() == 5) else {
          continue;
        };
        let (Some(field), Some(kind)) = (num(&s[0]), s[1].as_str()) else {
          continue;
        };
        if field < 0.0 || field as usize >= sp::COUNT {
          continue;
        }
        let mut spec = SpSpec {
          field: field as u8,
          ..Default::default()
        };
        match kind {
          "f" | "fi" | "i" => {
            spec.kind = match kind {
              "f" => SpKind::Float,
              "fi" => SpKind::FloatIntRand,
              _ => SpKind::Int,
            };
            spec.a = num(&s[2]).unwrap_or(0.0);
            spec.b = num(&s[3]).unwrap_or(0.0);
            spec.c = num(&s[4]).unwrap_or(0.0);
          }
          "copy" | "next" if s[3].is_number() => {
            spec.kind = if kind == "copy" {
              SpKind::Copy
            } else {
              SpKind::Next
            };
            spec.ref_id = num(&s[3]).unwrap_or(0.0) as i32;
          }
          "flags" if s[3].is_array() => {
            spec.kind = SpKind::Flags;
            for n in s[3]
              .as_array()
              .into_iter()
              .flatten()
              .filter_map(Value::as_str)
            {
              spec.flags |= cpuinfo_flag(n);
            }
          }
          "tex" if s[3].is_string() => {
            spec.kind = SpKind::Texture;
            let t = s[3].as_str().unwrap_or_default();
            let next = out.textures.len() as i32;
            spec.ref_id = *texture_index.entry(t.to_string()).or_insert_with(|| {
              out.textures.push(t.to_string());
              next
            });
          }
          _ => continue, // symbols (the game's functions): not run
        }
        list.push(spec);
      }
    }
    for (id, c) in j
      .get("curves")
      .and_then(Value::as_object)
      .into_iter()
      .flatten()
    {
      let life = |i: usize| c.get("life").and_then(|l| l.get(i)).and_then(num);
      let settings = CurveSettings {
        life: life(0).unwrap_or(0.0) as i32,
        offset: life(1).unwrap_or(0.0) as i32,
        flags: c.get("flags").and_then(num).unwrap_or(0.0) as u32,
        birth: c.get("birth") == Some(&Value::Bool(true)),
        live: c.get("live") == Some(&Value::Bool(true)),
        curves: std::array::from_fn(|i| {
          Curve::from_json(c.get(CURVE_FIELDS[i])?, if i == 0 { 16 } else { 4 })
        }),
      };
      out.curves.insert(atoi(id), Arc::new(settings));
    }
    for (name, g) in groups.as_object().into_iter().flatten() {
      let mut group = SpGroup {
        name: name.clone(),
        flags: sp_bits(g.get("flags")),
        ..Default::default()
      };
      if let Some(b) = g
        .get("bounds")
        .and_then(Value::as_array)
        .filter(|b| b.len() == 4)
      {
        group.radius = num(&b[3]).unwrap_or(0.0);
      }
      for p in g
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
      {
        let Some(p) = p.as_array().filter(|p| p.len() == 9) else {
          continue;
        };
        let Some(launcher) = num(&p[0]) else { continue };
        let int = |v: &Value| num(v).unwrap_or(0.0) as i32;
        group.items.push(SpItem {
          launcher: launcher as i32,
          flags: sp_bits(Some(&p[1])),
          period: int(&p[2]),
          length: int(&p[3]),
          offset: int(&p[4]),
          fade_after: num(&p[5]).unwrap_or(0.0),
          falloff_to: num(&p[6]).unwrap_or(0.0),
          hour_mask: p[8].as_f64().unwrap_or(0.0) as u32,
        });
      }
      out.groups.insert(name.clone(), group);
    }
    Some(out)
  }

  pub fn is_empty(&self) -> bool {
    self.groups.is_empty()
  }

  /// an art-name ("group-...", quoted or not)
  pub fn group(&self, name: &str) -> Option<&SpGroup> {
    self.groups.get(name.strip_prefix('\'').unwrap_or(name))
  }

  pub fn groups(&self) -> impl Iterator<Item = &SpGroup> {
    self.groups.values()
  }

  pub fn launcher(&self, id: i32) -> Option<&Vec<SpSpec>> {
    self.launchers.get(&id)
  }

  /// the curves of a launcher drawn by them
  pub fn curve(&self, id: i32) -> Option<&Arc<CurveSettings>> {
    self.curves.get(&id)
  }

  /// the textures the launchers draw: "<tpage>/<name>", or "<page>:<index>"
  pub fn textures(&self) -> &[String] {
    &self.textures
  }
}

/// The game's effects as the editor has them (App::particle_data): data/jak3/particles.json of
/// the game's data folder, read once (empty, with a warning in the log, when it cannot be).
pub fn particle_data() -> &'static ParticleData {
  static DATA: OnceLock<ParticleData> = OnceLock::new();
  DATA.get_or_init(|| {
    let path = crate::io::game_data::data_dir().join("jak3/particles.json");
    ParticleData::load(&path).unwrap_or_else(|e| {
      crate::log_warn!("%s", e);
      ParticleData::default()
    })
  })
}

/// An effect of the document: an actor running a group of particles where it is.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EffectEmitter {
  pub id: u32,
  pub group: String,
  pub world: Mat4,
}

/// The effects of the document (App::update_effects' m_effect_nodes): the visible actors naming a
/// particle group (part-spawner and its kind: their art-name), and where they are.
pub fn effect_emitters(scene: &Scene, data: &ParticleData) -> Vec<EffectEmitter> {
  if data.is_empty() {
    return Vec::new();
  }
  let cache = scene.cache();
  let count = cache.count.min(scene.nodes.len());
  scene.nodes[..count]
    .iter()
    .enumerate()
    .filter(|(i, n)| n.kind == NodeKind::Actor && cache.visible.get(*i).copied().unwrap_or(false))
    .filter_map(|(i, n)| {
      let group = data.group(n.extras["lump"]["art-name"].as_str()?)?;
      Some(EffectEmitter {
        id: n.id,
        group: group.name.clone(),
        world: cache.worlds[i],
      })
    })
    .collect()
}

// ------------------------------------------------------------------------------------------------

/// a particle as drawn: a sprite facing the camera
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParticleSprite {
  /// meters
  pub pos: Vec3,
  /// meters
  pub sx: f32,
  pub sy: f32,
  /// radians
  pub rot: f32,
  /// 1 is the texture as it is (the game's 128)
  pub color: Vec4,
  /// ParticleData::textures(), -1 none
  pub texture: i32,
  /// Alpha, AddAlpha or Subtract
  pub blend: BlendMode,
  /// the sprite starts at its position (the game's flag 4: streaks), else centered
  pub from_left: bool,
}

#[derive(Clone)]
struct Particle {
  /// the fields (sp-field-id)
  f: [f32; sp::COUNT],
  /// game units, per tick, per frame
  pos: Vec3,
  vel: Vec3,
  acc: Vec3,
  /// ticks of its life the next launcher takes over
  next_time: i32,
  /// ticks
  age: i32,
  texture: i32,
  flags: u32,
  /// drawn by curves: their random picks, its life (ticks)
  curve: Option<Arc<CurveSettings>>,
  rnd: [f32; 4],
  life: f32,
}

impl Default for Particle {
  fn default() -> Self {
    Self {
      f: [0.0; sp::COUNT],
      pos: Vec3::default(),
      vel: Vec3::default(),
      acc: Vec3::default(),
      next_time: -1,
      age: 0,
      texture: -1,
      flags: 0,
      curve: None,
      rnd: [0.0; 4],
      life: 0.0,
    }
  }
}

struct Emitter {
  group: String,
  world: Mat4,
  /// per item: the particles started but for a fraction
  accum: Vec<f32>,
  /// per item: launched once (bit 3)
  launched: Vec<bool>,
  /// ticks
  clock: i32,
  seen: bool,
}

// ponytail: xorshift64* instead of the C++'s mt19937 (the same rules, other draws)
struct Rng(u64);

impl Rng {
  // 0..1
  fn next(&mut self) -> f32 {
    self.0 ^= self.0 >> 12;
    self.0 ^= self.0 << 25;
    self.0 ^= self.0 >> 27;
    (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u32 << 24) as f32
  }
}

/// a frame at 60 a second
const K_TICKS: i32 = 5;

pub struct ParticleSystem {
  pub max_particles: usize,
  /// meters
  pub max_distance: f32,
  emitters: BTreeMap<u64, Emitter>,
  particles: Vec<Particle>,
  /// not stepped yet
  time: f64,
  rng: Rng,
}

impl Default for ParticleSystem {
  fn default() -> Self {
    Self {
      max_particles: 20000,
      max_distance: 150.0,
      emitters: BTreeMap::new(),
      particles: Vec::new(),
      time: 0.0,
      rng: Rng(12345),
    }
  }
}

// sp-init-fields!: each field from its spec, the ones not given at 0 (a new particle) or as they
// are (a launcher taking over)
fn init(p: &mut Particle, specs: &[SpSpec], relaunch: bool, rng: &mut Rng) {
  if !relaunch {
    p.f = [0.0; sp::COUNT];
    p.f[sp::FRICTION] = 1.0; // none given: it keeps its speed
    p.f[sp::TIMER] = -1.0;
  }
  for s in specs {
    let field = s.field as usize;
    match s.kind {
      SpKind::Float => {
        p.f[field] = if s.b != 0.0 {
          s.a + s.c * rng.next() * s.b
        } else {
          s.a
        }
      }
      SpKind::FloatIntRand => {
        p.f[field] = if s.b != 0.0 {
          s.a + s.c * (rng.next() * (s.b + 1.0)).floor()
        } else {
          s.a
        }
      }
      SpKind::Int => {
        p.f[field] = if s.b != 0.0 {
          s.a + s.c * (rng.next() * s.b).floor()
        } else {
          s.a
        }
      }
      SpKind::Copy => {
        if (0..sp::COUNT as i32).contains(&s.ref_id) {
          p.f[field] = p.f[s.ref_id as usize];
        }
      }
      SpKind::Next => p.f[field] = s.ref_id as f32,
      SpKind::Flags => p.flags = s.flags,
      SpKind::Texture => p.texture = s.ref_id,
      SpKind::Other => {}
    }
  }
}

// birth-func-curve: its life, its random picks; its color and size picked now (pcf2, or a color that
// does not change)
fn birth(p: &mut Particle, c: &Arc<CurveSettings>, rng: &mut Rng) {
  p.curve = Some(c.clone());
  p.life = c.life as f32
    + if c.offset > 0 {
      (rng.next() * c.offset as f32).floor()
    } else {
      0.0
    };
  p.rnd = std::array::from_fn(|_| rng.next());
  if !c.birth {
    return;
  }
  let (f, r) = (&mut p.f, p.rnd);
  let [color, _, sx, sy, ..] = &c.curves;
  f[sp::TIMER] = p.life;
  f[sp::A] = 0.0;
  if let Some(color) = color
    .as_ref()
    .filter(|_| c.flags & 4 != 0 || c.flags & 2 == 0)
  {
    f[sp::R..sp::R + 3].copy_from_slice(&color.color(r[0]));
  }
  if c.flags & 4 == 0 {
    return;
  }
  if let Some(sx) = sx {
    f[sp::SCALE_X] = sx.at(r[2]) * 4096.0;
  }
  if c.flags & 1 != 0 {
    f[sp::SCALE_Y] = f[sp::SCALE_X];
  } else if let Some(sy) = sy {
    f[sp::SCALE_Y] = sy.at(r[3]) * 4096.0;
  }
}

// live-func-curve: its color, alpha and size, picked at birth, scaled over its life
fn live(p: &mut Particle) {
  let Some(c) = p.curve.clone().filter(|c| c.live) else {
    return;
  };
  let (f, r) = (&mut p.f, p.rnd);
  let [color, alpha, sx, sy, over @ ..] = &c.curves;
  let [_, _, _, a, sx_over, sy_over] = over;
  let t = if p.life > 0.0 {
    (p.life - f[sp::TIMER]) / p.life
  } else {
    0.0
  };
  if let Some(color) = color.as_ref().filter(|_| c.flags & 2 != 0) {
    let rgb = color.color(r[0]);
    for i in 0..3 {
      if let Some(o) = &over[i] {
        f[sp::R + i] = rgb[i] * o.at(t);
      }
    }
  }
  if let (Some(alpha), Some(a)) = (alpha, a) {
    f[sp::A] = alpha.at(r[1]) * a.at(t);
  }
  if let (Some(sx), Some(o)) = (sx, sx_over) {
    f[sp::SCALE_X] = (sx.at(r[2]) * 4096.0 * o.at(t)).abs();
  }
  if c.flags & 1 != 0 {
    f[sp::SCALE_Y] = f[sp::SCALE_X];
  } else if let (Some(sy), Some(o)) = (sy, sy_over) {
    f[sp::SCALE_Y] = (sy.at(r[3]) * 4096.0 * o.at(t)).abs();
  }
}

#[allow(clippy::too_many_arguments)]
fn launch(
  particles: &mut Vec<Particle>,
  rng: &mut Rng,
  max_particles: usize,
  world: &Mat4,
  turned: bool,
  specs: &[SpSpec],
  rate: f32,
  accum: &mut f32,
  curve: Option<&Arc<CurveSettings>>,
) {
  // how many: num a launch (a fraction carried to the next)
  let mut count = 1.0;
  for s in specs {
    if s.field as usize == sp::NUM
      && matches!(s.kind, SpKind::Float | SpKind::Int | SpKind::FloatIntRand)
    {
      count = if s.b != 0.0 {
        s.a + s.c * rng.next() * s.b
      } else {
        s.a
      };
    }
  }
  *accum += count * rate;
  let mut n = accum.floor() as i32;
  *accum -= n as f32;
  let origin = world.point(Vec3::default()) * 4096.0;
  while n > 0 && particles.len() < max_particles {
    let mut p = Particle::default();
    init(&mut p, specs, false, rng);
    if let Some(c) = curve {
      birth(&mut p, c, rng);
    }
    let f = &p.f;
    // launched along its cone and turned (the launcher's), then turned with the effect
    let r = rotation(f[sp::ROTATE_X], f[sp::ROTATE_Y], f[sp::ROTATE_Z], false)
      * rotation(
        f[sp::CONE_ROT_X],
        f[sp::CONE_ROT_Y],
        f[sp::CONE_ROT_Z],
        true,
      );
    let mut pos = r.dir(Vec3::new(
      f[sp::X],
      f[sp::Y],
      f[sp::Z] + f[sp::CONE_ROT_RADIUS],
    ));
    let mut vel = r.dir(Vec3::new(f[sp::VEL_X], f[sp::VEL_Y], f[sp::VEL_Z]));
    let mut acc = Vec3::new(f[sp::ACCEL_X], f[sp::ACCEL_Y], f[sp::ACCEL_Z]);
    if turned {
      let turn = |v: Vec3| normalize(world.dir(v)) * length(v);
      pos = turn(pos);
      vel = turn(vel);
      if p.flags & K_SP_GLOBAL_ACC == 0 {
        acc = turn(acc);
      }
    }
    p.next_time = if f[sp::NEXT_LAUNCHER] != 0.0 {
      f[sp::NEXT_TIME] as i32
    } else {
      -1
    };
    p.pos = origin + pos;
    p.vel = vel;
    p.acc = acc;
    particles.push(p);
    n -= 1;
  }
}

impl ParticleSystem {
  pub fn new() -> Self {
    Self::default()
  }

  /// The effects to run this frame: each by an id of the caller (an actor), its group and where
  /// it is; the effects not given again stop launching and go once their particles are gone.
  pub fn begin_frame(&mut self) {
    for e in self.emitters.values_mut() {
      e.seen = false;
    }
  }

  pub fn emit(&mut self, id: u64, group: &SpGroup, world: Mat4) {
    let fresh = || Emitter {
      group: group.name.clone(),
      world,
      accum: vec![0.0; group.items.len()],
      launched: vec![false; group.items.len()],
      clock: 0,
      seen: true,
    };
    let e = self.emitters.entry(id).or_insert_with(fresh);
    if e.group != group.name {
      *e = fresh();
    }
    e.world = world;
    e.seen = true;
  }

  /// The time passed (seconds: 60 steps a second, at most 10 a call), the camera (the effects far
  /// from it do not launch), the hour (the hour-mask of some parts; < 0: every part, any hour).
  pub fn update(&mut self, data: &ParticleData, dt: f64, camera: Vec3, hour: f32) {
    self.emitters.retain(|_, e| e.seen);
    self.time = (self.time + dt).min(10.0 / 60.0);
    while self.time >= 1.0 / 60.0 {
      self.time -= 1.0 / 60.0;
      self.step(data, camera, hour);
    }
  }

  pub fn clear(&mut self) {
    self.emitters.clear();
    self.particles.clear();
    self.time = 0.0;
  }

  pub fn count(&self) -> usize {
    self.particles.len()
  }

  fn step(&mut self, data: &ParticleData, camera: Vec3, hour: f32) {
    // the effects launch (sparticle-launch-control spawn)
    let hour_bit = if hour < 0.0 {
      0
    } else {
      1u32 << (hour as i32).clamp(0, 23)
    };
    for e in self.emitters.values_mut() {
      if !e.seen {
        continue;
      }
      let Some(group) = data.group(&e.group) else {
        continue;
      };
      let dist = length(e.world.point(Vec3::default()) - camera);
      if dist > self.max_distance {
        continue;
      }
      let turned = group.flags & (1 << 4) != 0; // sp4: the effect turns with its actor
      for (i, item) in group.items.iter().enumerate() {
        let Some(specs) = data.launcher(item.launcher) else {
          continue;
        };
        let rate = if item.falloff_to > 0.0 {
          1.0 - dist / item.falloff_to
        } else {
          1.0
        };
        if rate <= 0.0 {
          continue;
        }
        let (particles, rng, max) = (&mut self.particles, &mut self.rng, self.max_particles);
        if item.flags & (1 << 3) != 0 {
          // sp3: once
          if !e.launched[i] {
            e.launched[i] = true;
            launch(
              particles,
              rng,
              max,
              &e.world,
              turned,
              specs,
              rate,
              &mut e.accum[i],
              data.curve(item.launcher),
            );
          }
          continue;
        }
        if item.hour_mask & hour_bit != 0 || (item.fade_after > 0.0 && dist > item.fade_after) {
          continue;
        }
        if item.period > 0 {
          let t = ((e.clock - item.offset) % item.period + item.period) % item.period;
          if t >= item.length {
            continue;
          }
        }
        launch(
          particles,
          rng,
          max,
          &e.world,
          turned,
          specs,
          rate,
          &mut e.accum[i],
          data.curve(item.launcher),
        );
      }
      e.clock += K_TICKS;
    }
    // the particles move (sp-process-block): their velocity per tick, their friction and
    // acceleration per frame
    let rng = &mut self.rng;
    let ticks = K_TICKS as f32;
    self.particles.retain_mut(|p| {
      // freed once its timer ran out the frame before (a particle of one frame is drawn once)
      if p.f[sp::TIMER] != -1.0 {
        if p.f[sp::TIMER] <= 0.0 {
          return false;
        }
        p.f[sp::TIMER] = (p.f[sp::TIMER] - ticks).max(0.0);
      }
      p.age += K_TICKS;
      if p.next_time >= 0 && p.age >= p.next_time {
        // another launcher takes over (sp-relaunch-setup-fields): its fields over these
        let next = data.launcher(p.f[sp::NEXT_LAUNCHER] as i32);
        p.next_time = -1;
        if let Some(next) = next {
          let friction = p.f[sp::FRICTION];
          p.f[sp::NEXT_LAUNCHER] = 0.0;
          init(p, next, true, rng);
          let f = &mut p.f;
          if f[sp::FRICTION] == 0.0 {
            f[sp::FRICTION] = friction;
          }
          for s in next {
            match s.field as usize {
              sp::VEL_X | sp::VEL_Y | sp::VEL_Z => {
                p.vel = Vec3::new(f[sp::VEL_X], f[sp::VEL_Y], f[sp::VEL_Z])
              }
              sp::ACCEL_X | sp::ACCEL_Y | sp::ACCEL_Z => {
                p.acc = Vec3::new(f[sp::ACCEL_X], f[sp::ACCEL_Y], f[sp::ACCEL_Z])
              }
              _ => {}
            }
          }
          if f[sp::NEXT_LAUNCHER] != 0.0 {
            p.next_time = p.age + f[sp::NEXT_TIME] as i32;
          }
        }
      }
      let f = &mut p.f;
      p.vel += p.acc;
      p.vel *= f[sp::FRICTION];
      p.pos += p.vel * ticks;
      f[sp::SCALE_X] += f[sp::SCALE_VEL_X] * ticks;
      f[sp::SCALE_Y] += f[sp::SCALE_VEL_Y] * ticks;
      f[sp::ROT_Z] += f[sp::ROT_VEL_Z] * ticks;
      for c in 0..4 {
        f[sp::R + c] = (f[sp::R + c] + f[sp::FADE_R + c] * ticks).max(0.0);
      }
      if p.curve.as_ref().is_some_and(|c| c.live) {
        live(p);
        return true; // at the end of its life
      }
      // faded out
      !(f[sp::A] <= 0.0 && f[sp::FADE_A] <= 0.0 && p.age > K_TICKS)
    });
  }

  pub fn sprites(&self) -> Vec<ParticleSprite> {
    self
      .particles
      .iter()
      .map(|p| {
        let f = &p.f;
        ParticleSprite {
          pos: p.pos / 4096.0,
          sx: f[sp::SCALE_X] / 4096.0,
          sy: f[sp::SCALE_Y] / 4096.0,
          rot: angle(f[sp::ROT_Z]),
          color: Vec4::new(
            f[sp::R] / 128.0,
            f[sp::G] / 128.0,
            f[sp::B] / 128.0,
            f[sp::A].min(128.0) / 128.0,
          ),
          texture: p.texture,
          blend: blend_of(p.flags),
          from_left: (f[sp::ROT_X] as i32 & 15) == 4,
        }
      })
      .filter(|s| s.sx > 0.0 && s.sy > 0.0 && s.color.w > 0.002)
      .collect()
  }

  /// the box of the particles (meters), for a picture of an effect
  pub fn bounds(&self) -> AABB {
    let mut b = AABB::default();
    for p in &self.particles {
      let r = p.f[sp::SCALE_X].max(p.f[sp::SCALE_Y]) / 8192.0;
      b.add(p.pos / 4096.0 - Vec3::new(r, r, r));
      b.add(p.pos / 4096.0 + Vec3::new(r, r, r));
    }
    b
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::Node;
  use serde_json::json;

  fn data() -> Option<ParticleData> {
    let path = crate::io::game_data::data_dir().join("jak3/particles.json");
    if !path.is_file() {
      eprintln!("skipped: no {}", path.display());
      return None;
    }
    Some(ParticleData::load(&path).expect("particles.json"))
  }

  // the particle effects of the game (data/jak3/particles.json): ctyindb's fast steam launches
  // puffs every frame that rise along its vent, slow down and fade out (their launcher hands over
  // to another at a third of a second)
  #[test]
  fn particles_steam_rises_and_fades() {
    let Some(pd) = data() else { return };
    let steam = pd
      .group("'group-ctyindb-fast-steam")
      .expect("the steam's group, quoted");
    let mut ps = ParticleSystem::new();
    let run = |ps: &mut ParticleSystem| {
      for _ in 0..3 {
        // 30 frames (10 at most a call)
        ps.begin_frame();
        ps.emit(1, steam, Mat4::identity());
        ps.update(&pd, 1.0 / 6.0, Vec3::default(), 12.0);
      }
    };
    run(&mut ps);
    let sprites = ps.sprites();
    let far = sprites.iter().map(|s| s.pos.z).fold(0.0, f32::max); // along the vent: its z
    let faint = sprites.iter().map(|s| s.color.w).fold(1.0, f32::min);
    assert!(
      ps.count() > 20 && far > 3.0,
      "{} particles, {far:.2} m along its vent",
      ps.count()
    );
    // later: the first puffs gone (half a second of life)
    run(&mut ps);
    let sprites = ps.sprites();
    assert!(
      !sprites.is_empty() && faint < 0.2 && ps.count() < 200,
      "{} particles, faintest {faint:.3}",
      ps.count()
    );
    // the launcher's hand-over: at 100 ticks (a third of a second) the puff fades faster
    let p = ps
      .particles
      .iter()
      .find(|p| p.age > 100)
      .expect("a puff older than the hand-over");
    assert_eq!(p.next_time, -1);
    assert!(
      p.f[sp::FADE_A] < 0.0,
      "fades out after the hand-over: {}",
      p.f[sp::FADE_A]
    );
    assert!(
      ps.particles
        .iter()
        .any(|p| p.age < 100 && p.f[sp::FADE_A] > 0.0)
    );
    // the effect not given again: no new puffs, gone with its last
    for _ in 0..12 {
      ps.begin_frame();
      ps.update(&pd, 1.0 / 6.0, Vec3::default(), 12.0);
    }
    assert_eq!(ps.count(), 0);
    let b = ps.bounds();
    assert!(!b.valid());
  }

  // a light of one frame is drawn once, a glow
  #[test]
  fn particles_light_of_one_frame_glows() {
    let Some(pd) = data() else { return };
    let light = pd
      .group("group-citylights-newslums")
      .expect("the city's lights");
    let mut ps = ParticleSystem::new();
    ps.begin_frame();
    ps.emit(1, light, Mat4::identity());
    ps.update(&pd, 1.0 / 60.0, Vec3::default(), 12.0);
    let sprites = ps.sprites();
    assert!(
      sprites.len() == 1
        && sprites[0].blend == BlendMode::AddAlpha
        && (sprites[0].sx - 4.0).abs() < 0.01,
      "{sprites:?}"
    );
    assert!(sprites[0].texture >= 0 && pd.textures()[sprites[0].texture as usize].contains('/'));
    // far from the camera: nothing launched
    let mut far = ParticleSystem::new();
    far.begin_frame();
    far.emit(1, light, Mat4::translate(Vec3::new(0.0, 0.0, 200.0)));
    far.update(&pd, 1.0 / 60.0, Vec3::default(), 12.0);
    assert_eq!(far.count(), 0);
  }

  // the desert's fire (group-firepit-fire, demo-oasis): its glow, its sparks and its flames are
  // added to what is behind (glow, sp-cpuinfo-flag-3), their textures opaque, their shape in their
  // colors on black: mixed, they were squares; its flames take their color, alpha and size from
  // the game's curves (else never seen)
  #[test]
  fn particles_fire_is_added() {
    assert_eq!(blend_of(0), BlendMode::Alpha);
    assert_eq!(blend_of(K_SP_SUB), BlendMode::Subtract);
    assert_eq!(blend_of(K_SP_SUB | K_SP_ADD), BlendMode::AddAlpha);
    assert_eq!(blend_of(K_SP_GLOW), BlendMode::AddAlpha);
    let Some(pd) = data() else { return };
    let fire = pd.group("group-firepit-fire").expect("the firepit's fire");
    let mut ps = ParticleSystem::new();
    for _ in 0..30 {
      ps.begin_frame();
      ps.emit(1, fire, Mat4::identity());
      ps.update(&pd, 1.0 / 6.0, Vec3::default(), 12.0);
    }
    let sprites = ps.sprites();
    let names: Vec<&str> = sprites
      .iter()
      .map(|s| pd.textures()[s.texture as usize].as_str())
      .collect();
    for t in [
      "level-default-sprite/glow",
      "level-default-sprite/glow-hotdot",
      "level-default-sprite/explosion-nebula",
    ] {
      assert!(names.contains(&t), "{t} in {names:?}");
    }
    assert!(
      sprites.iter().all(|s| s.blend == BlendMode::AddAlpha),
      "{sprites:?}"
    );
    // the flames: 10 to 15 m times 0.2 to 0.6 over their life
    let flames: Vec<_> = (0..sprites.len())
      .filter(|&i| names[i].ends_with("explosion-nebula"))
      .map(|i| sprites[i])
      .collect();
    assert!(flames.len() > 3, "{flames:?}");
    for s in &flames {
      let c = s.color;
      assert!(
        (1.9..9.1).contains(&s.sx) && (1.9..9.1).contains(&s.sy) && c.w <= 1.0,
        "{s:?}"
      );
      assert!(c.x > 0.5, "{s:?}");
    }
    // white at first, then orange and red (their blue gone)
    assert!(
      flames.iter().any(|s| s.color.z < s.color.x * 0.5),
      "{flames:?}"
    );
  }

  // the game's curves (particle-curves.gc): picked at random in a range, scaled over the life
  #[test]
  fn particles_curves() {
    let range = |ys: Value, odx: Value| json!({"xs": [0, -1, -2, -3], "ys": ys, "odx": odx});
    let j = json!({"groups": {}, "launchers": {}, "curves": {"7": {
      "life": [90, 120], "flags": 2, "birth": true, "live": true,
      "color-start": range(json!([255, 255, 255, 128, 128, 128, 128, 128, 128, 128, 128, 128,
        128, 128, 128, 128]), json!([1, 1, 1, 1])),
      "alpha-start": range(json!([96, 255, 256, 257]), json!([159, 1, 1, 1])),
      "a-scalar": {"xs": [0, -0.4, -0.7, -1], "ys": [0, 0, 1, 0.2],
        "odx": [0, 3.3333335, -2.6666665, 1]},
      "r-scalar": {"xs": [0, -1, -2], "ys": [1, 1, 1, 1], "odx": [0, 0, 0, 0]}}}});
    let pd = ParticleData::from_json(&j).unwrap();
    let c = pd.curve(7).unwrap();
    assert_eq!(
      (c.life, c.offset, c.flags, c.birth, c.live),
      (90, 120, 2, true, true)
    );
    assert!(c.curves[4].is_none(), "r-scalar: 3 keys, not a curve");
    let (alpha, a) = (c.curves[1].as_ref().unwrap(), c.curves[7].as_ref().unwrap());
    assert_eq!(alpha.at(0.5), 175.5);
    assert_eq!(alpha.at(2.0), 255.0); // clamped to 0..1
    assert_eq!(a.at(0.2), 0.0);
    assert!((a.at(0.55) - 0.5).abs() < 1e-5 && (a.at(0.85) - 0.6).abs() < 1e-5);
    assert_eq!(c.curves[0].as_ref().unwrap().color(0.5), [191.5; 3]);
  }

  #[test]
  fn particles_data_rules() {
    let j = json!({
      "groups": {"group-a": {"bounds": [0, 0, 0, 6], "flags": ["sp4"], "parts": [
        [7, ["sp3", "is-3d"], 10, 5, 2, 80.0, 0.0, 0, 4096],
        [8, [], 0, 0, 0, 0.0, 0.0]]}},
      "launchers": {"7": [
        [1, "tex", 0, "page/glow", 0], [13, "f", 2048.0, 0, 1], [17, "copy", 0, 13, 0],
        [47, "flags", 0, ["glow", "use-global-acc", "sp-cpuinfo-flag-2"], 0],
        [4, "sym", 0, "birth-func", 0], [51, "next", 0, 9, 0], [99, "f", 1.0, 0, 1]]}
    });
    let pd = ParticleData::from_json(&j).unwrap();
    let g = pd.group("'group-a").unwrap();
    assert_eq!((g.flags, g.radius, g.items.len()), (1 << 4, 6.0, 1));
    let it = &g.items[0];
    assert_eq!(
      (it.launcher, it.flags, it.period, it.length, it.offset),
      (7, 1 | 1 << 3, 10, 5, 2)
    );
    assert_eq!((it.fade_after, it.hour_mask), (80.0, 4096));
    let l = pd.launcher(7).unwrap();
    assert_eq!(l.len(), 5); // the symbol and the field out of range left out
    assert_eq!(l[2].kind, SpKind::Copy);
    assert_eq!(l[3].flags, K_SP_GLOW | K_SP_GLOBAL_ACC | 4);
    assert_eq!((l[4].kind, l[4].ref_id), (SpKind::Next, 9));
    assert_eq!(pd.textures(), ["page/glow"]);
    assert!(ParticleData::from_json(&json!({"groups": {}})).is_none());
  }

  // m_effect_nodes: the visible actors naming a group by their art-name, where they are
  #[test]
  fn particles_effect_emitters() {
    let pd =
      ParticleData::from_json(&json!({"groups": {"group-a": {"parts": []}}, "launchers": {}}))
        .unwrap();
    let mut scene = Scene::new();
    let actor = |name: &str, art: Value, x: f32| {
      let mut n = Node {
        name: name.into(),
        kind: NodeKind::Actor,
        ..Default::default()
      };
      n.local.t = Vec3::new(x, 0.0, 0.0);
      n.extras = json!({"lump": {"art-name": art}});
      n
    };
    let a = scene.add(actor("a", json!("'group-a"), 3.0));
    scene.add(actor("unknown", json!("group-b"), 0.0));
    scene.add(actor("not a name", json!(5), 0.0));
    let hidden = scene.add(Node {
      visible: false,
      ..actor("hidden", json!("group-a"), 0.0)
    });
    let mut render = actor("model", json!("group-a"), 0.0);
    render.kind = NodeKind::Render;
    scene.add(render);
    let e = effect_emitters(&scene, &pd);
    assert_eq!(e.len(), 1);
    assert_eq!((e[0].id, e[0].group.as_str()), (a, "group-a"));
    assert_eq!(e[0].world.point(Vec3::default()), Vec3::new(3.0, 0.0, 0.0));
    assert_ne!(hidden, a);
    assert_eq!(
      serde_json::to_value(&e[0]).unwrap()["world"]
        .as_array()
        .unwrap()
        .len(),
      16
    );
  }
}
