//! The particle effects of Jak 3 (steam, sparks, smoke, fire, lights) as the game makes them: its
//! definitions (data/jak3/particles.json, read in its code by tools/gen_particles.py) run by its
//! rules (sparticle-launcher.gc, sparticle.gc). A group launches the particles of its parts over
//! time (continuously, in bursts every period, or once); a particle starts from its launcher's
//! fields (a value and a random range each: its position, velocity, size, rotation, color and how
//! they change, its life) and moves 60 times a second: velocities are per tick (300 a second, 5 a
//! frame), friction and acceleration per frame. At a time of its life it can take the fields of
//! another launcher (next-launcher: steam that fades out).
//!
//! The editor shows them where the effects of the levels are (the actors naming a group by their
//! art-name: part-spawner...), and in the catalog. The view runs them in the interface
//! (ui/render/particles.ts, the same rules); this is the reference, proven by the tests.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;

use super::{NodeKind, Scene};
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
  pub additive: bool,
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
          additive: p.flags & K_SP_GLOW != 0,
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
      sprites.len() == 1 && sprites[0].additive && (sprites[0].sx - 4.0).abs() < 0.01,
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
