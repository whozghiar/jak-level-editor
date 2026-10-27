//! The inspector's preview of an object's animation or effect: its model drawn moving where it
//! stands, the document left as it is. Three kinds: an animation of its model (the game's, from its
//! art group), broken (the pieces of its -explode model, or of its own cut up, thrown apart), and
//! for the objects the game moves by its code, that movement imitated (an elevator going up and
//! down, a platform sliding, a fan turning...) (src/app/preview.cpp).
//!
//! How it keeps 60 frames a second: while it plays, the node draws a mesh of its own in its bind
//! pose (the viewport fetches it as any mesh: `start_preview` answers its uid and each vertex's
//! joints and weights, once); each frame the viewport asks `preview_frame {dt}` (the joints'
//! matrices only: 64 bytes each) and moves the vertices itself (ui/render/preview.ts, on the CPU).
//! An actor without a model of its own: its icon (object_icon, a mesh served like the others),
//! drawn by the viewport where `preview_frame` puts it.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::{Value, json};

use super::Editor;
use crate::i18n::tr;
use crate::io::asset_library::{load_model_asset, merge_mesh};
use crate::io::game_data::GameData;
use crate::io::gltf::load_skeleton;
use crate::io::jak2_import::import_jak2_mesh;
use crate::math::{AABB, K_PI, Mat4, Quat, Vec3, Vec4, length, normalize, radians};
use crate::scene::shapes::object_icon;
use crate::scene::skeleton::{Skeleton, joint_matrices, skeleton_pose, skin_mesh};
use crate::scene::{Mesh, NodeKind, Primitive};
use crate::{log_warn, strf};

// The preview's kinds besides an animation of the model (>= 0)
/// its -explode model's pieces thrown apart
const K_BROKEN: i32 = -1;
/// the movement its code gives it, imitated
const K_MOTION: i32 = -2;
/// its own model cut in pieces, thrown apart
const K_FRACTURE: i32 = -3;

// The movement the game's code gives an object, as far as its name and family tell
const RISE: i32 = 1;
const FALL: i32 = 2;
const SLIDE: i32 = 3;
const SPIN: i32 = 4;
const ROCK: i32 = 5;
const PRESS: i32 = 6;
const LIFT: i32 = 7;
const SINK: i32 = 8;
const BOB: i32 = 9;
const FLICKER: i32 = 10;
const BLOW: i32 = 11;
const SWING: i32 = 12;
const CLIMB: i32 = 13;
const MOTION_KEYS: [&str; 14] = [
  "", "rise", "fall", "slide", "spin", "rock", "press", "lift", "sink", "bob", "flicker", "blow",
  "swing", "climb",
];

fn object_motion(etype: &str, family: &str) -> i32 {
  let has = |s: &str| etype.contains(s);
  // what their code does, read in the game's (temple-obs, foresta-obs, factoryc-obs,
  // mine-platforms...)
  if etype == "ladder" {
    return CLIMB;
  }
  if has("elec") || has("eyes") {
    return FLICKER;
  }
  if has("for-pillar") || has("for-tower") {
    return RISE;
  }
  if has("torch") {
    return SLIDE;
  }
  if has("ramp") {
    return ROCK;
  }
  if has("gun") {
    return SPIN;
  }
  if has("elevator") || has("elev") || has("rising") || has("pop") {
    return RISE;
  }
  if has("drop-plat") || has("collapsing") || has("falling") {
    return FALL;
  }
  if ["spin", "fan", "turbine", "saw", "rings", "rot"]
    .iter()
    .any(|s| has(s))
  {
    return SPIN;
  }
  if has("balance") || has("floating") || has("lava-plat") {
    return ROCK;
  }
  if [
    "move",
    "rove",
    "slide",
    "arena-plat",
    "square-plat",
    "air-train",
    "moving",
  ]
  .iter()
  .any(|s| has(s))
  {
    return SLIDE;
  }
  if has("switch") || has("button") || has("jump-pad") {
    return PRESS;
  }
  if has("security-wall") || has("eggwall") || has("krimson-wall") || has("field") {
    return SINK;
  }
  if has("door") || has("gate") || has("lid") {
    return LIFT;
  }
  if has("artifact") || has("idol") || has("interceptor") || has("spawner") {
    return BOB;
  }
  match family {
    "platform" => RISE,
    "button" => PRESS,
    "door-like" => LIFT,
    _ => 0,
  }
}

/// What the picture of an object without a model does (a vent blowing, lightning, a swing pole)
fn icon_motion(etype: &str, family: &str) -> i32 {
  let has = |s: &str| etype.contains(s);
  if has("vent") {
    return BLOW;
  }
  if has("elec-gate") || has("vingate") || has("laser") || has("turbo") {
    return FLICKER;
  }
  if has("swingpole") || has("swing-pole") {
    return SWING;
  }
  if has("whirlpool") || has("spinning") {
    return SPIN;
  }
  if has("conveyor") {
    return SLIDE;
  }
  if family == "hazard" {
    return FLICKER;
  }
  BOB
}

/// Whether the game breaks it (a crate, a breakable, a bridge that falls apart...)
fn breaks(etype: &str, family: &str) -> bool {
  family == "crate"
    || family == "breakable"
    || [
      "break",
      "cactus",
      "fruit-stand",
      "eco-tank",
      "urn",
      "market",
      "power-box",
    ]
    .iter()
    .any(|s| etype.contains(s))
}

/// The family the catalog gives a type (io/catalog.rs build_catalog): its props and doors apart,
/// else the family of the objects that do something; "" for a type the catalog leaves out
fn catalog_family(data: Option<&GameData>, etype: &str) -> String {
  let category = data.map(|d| d.catalog_category(etype)).unwrap_or_default();
  if category == "collectible" || category == "technical" {
    return String::new();
  }
  let interactive = category != "static" && category != "door";
  let family = match category.as_str() {
    "static" => "prop".to_string(),
    "door" => "door".to_string(),
    _ => data.map_or("other".to_string(), |d| d.object_family(etype)),
  };
  if interactive && family == "door" {
    "door-like".into() // gates and switches that are not doors
  } else {
    family
  }
}

/// "fac-elevator-a-gate_up" of model "fac-elevator-a": "gate_up"
fn short_name<'a>(animation: &'a str, model: &str) -> &'a str {
  animation
    .strip_prefix(model)
    .and_then(|r| r.strip_prefix('-'))
    .filter(|r| !r.is_empty())
    .unwrap_or(animation)
}

/// Where the motion puts the model at t, around the middle of its base `base`, model height `h`
fn motion_at(m: i32, t: f32, base: Vec3, h: f32) -> Mat4 {
  let two_pi = 2.0 * K_PI;
  let mut off = Vec3::default();
  let mut q = Quat::default();
  match m {
    RISE => off.y = 8.0 * (0.5 - 0.5 * (two_pi * t / 6.0).cos()),
    FALL => {
      let c = t % 3.0;
      if c < 0.6 {
        off.x = 0.06 * (c * 60.0).sin();
      } else {
        off.y = -(0.5 * 9.8 * (c - 0.6) * (c - 0.6)).min(20.0);
      }
    }
    SLIDE => off.x = 4.0 * (two_pi * t / 5.0).sin(),
    SPIN => q = Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), two_pi * t / 3.0),
    ROCK => {
      q = Quat::axis_angle(
        Vec3::new(1.0, 0.0, 0.0),
        radians(8.0) * (two_pi * t / 4.0).sin(),
      );
      off.y = 0.2 * (two_pi * t / 2.0).sin();
    }
    PRESS => {
      let c = t % 2.0;
      off.y = if c > 0.3 && c < 1.0 {
        -(0.3f32.min(h * 0.3))
      } else {
        0.0
      };
    }
    LIFT | SINK => {
      // open over 1.5 s, held, closed again: a period of 6 s
      let c = t % 6.0;
      let k = if c < 1.5 {
        c / 1.5
      } else if c < 3.0 {
        1.0
      } else if c < 4.5 {
        1.0 - (c - 3.0) / 1.5
      } else {
        0.0
      };
      let e = k * k * (3.0 - 2.0 * k);
      off.y = if m == LIFT { 1.0 } else { -1.0 } * h * 0.95 * e;
    }
    BOB => {
      off.y = 0.4 * (two_pi * t / 2.0).sin();
      q = Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), two_pi * t / 6.0);
    }
    FLICKER => {
      // lightning: it shivers, flares and dims
      let k = (t * 14.0).floor();
      off = Vec3::new(0.06 * (k * 12.9898).sin(), 0.0, 0.06 * (k * 78.233).sin());
      return Mat4::translate(base + off)
        * Mat4::scale(Vec3::new(1.0, 1.0 + 0.12 * (k * 37.719).sin(), 1.0));
    }
    BLOW => {
      // a gust: it rises a meter and a half, again and again
      let c = (t % 1.2) / 1.2;
      off.y = 1.5 * c * c;
    }
    SWING => {
      // a pole Jak swings around: it turns about its bar
      q = Quat::axis_angle(Vec3::new(1.0, 0.0, 0.0), two_pi * t / 1.5);
      return Mat4::translate(base + Vec3::new(0.0, h * 0.75, 0.0))
        * Mat4::rotate(q)
        * Mat4::translate(Vec3::new(0.0, -h * 0.75, 0.0));
    }
    CLIMB => {
      // someone going up the ladder and down again, in front of it (h: the ladder's height)
      off = Vec3::new(
        0.0,
        (h - 1.9).max(0.0) * (0.5 - 0.5 * (two_pi * t / 6.0).cos()),
        0.7,
      );
    }
    _ => {}
  }
  Mat4::translate(base + off) * Mat4::rotate(q)
}

/// the middle of a box's base
fn base_of(b: &AABB) -> Vec3 {
  if b.valid() {
    Vec3::new(b.center().x, b.lo.y, b.center().z)
  } else {
    Vec3::default()
  }
}

/// `src` as pieces that each move with a joint of a made skeleton: one piece (`cell` 0) or its
/// triangles grouped in cells of `cell` meters (their vertices made apart)
fn cut_in_pieces(src: &Mesh, cell: f32) -> (Mesh, Skeleton) {
  let mut bind = Mesh::new();
  let mut s = Skeleton::default();
  let base = base_of(&src.bounds());
  let mut piece_of: BTreeMap<(i32, i32, i32), usize> = BTreeMap::new();
  let mut sum: Vec<Vec3> = Vec::new();
  let mut count: Vec<f32> = Vec::new();
  for p in &src.prims {
    let mut q = Primitive {
      material: p.material,
      ..Default::default()
    };
    let (mut joints, mut weights) = (Vec::new(), Vec::new());
    for t in p.idx.chunks_exact(3) {
      let mut piece = 0;
      if cell > 0.0 {
        let c = (p.pos[t[0] as usize] + p.pos[t[1] as usize] + p.pos[t[2] as usize]) * (1.0 / 3.0);
        let key = (
          (c.x / cell).floor() as i32,
          (c.y / cell).floor() as i32,
          (c.z / cell).floor() as i32,
        );
        let next = piece_of.len();
        piece = *piece_of.entry(key).or_insert(next);
      }
      if sum.len() <= piece {
        sum.resize(piece + 1, Vec3::default());
        count.resize(piece + 1, 0.0);
      }
      for &v in t {
        let v = v as usize;
        q.idx.push(q.pos.len() as u32);
        q.pos.push(p.pos[v]);
        q.nrm
          .push(p.nrm.get(v).copied().unwrap_or(Vec3::new(0.0, 1.0, 0.0)));
        if p.uv.len() == p.pos.len() {
          q.uv.push(p.uv[v]);
        }
        if p.col.len() == p.pos.len() {
          q.col.push(p.col[v]);
        }
        joints.push(Vec4::new(piece as f32, 0.0, 0.0, 0.0));
        weights.push(Vec4::new(1.0, 0.0, 0.0, 0.0));
        sum[piece] += p.pos[v];
        count[piece] += 1.0;
      }
    }
    q.extra_attrs.insert("JOINTS_0".into(), joints);
    q.extra_attrs.insert("WEIGHTS_0".into(), weights);
    bind.prims.push(q);
  }
  // a joint per piece, at its middle (one piece: at the middle of the model's base)
  for i in 0..sum.len() {
    let at = if cell > 0.0 && count[i] > 0.0 {
      sum[i] * (1.0 / count[i])
    } else {
      base
    };
    s.parent.push(-1);
    s.rest.push(Mat4::translate(at));
    s.joints.push(i as i32);
    s.inverse_bind.push(Mat4::translate(at * -1.0));
  }
  (bind, s)
}

/// How long the game's pieces fly (joint-exploder-tuning's duration)
const EXPLODE_SECONDS: f32 = 2.0;

/// Where the pieces are `t` seconds after they were thrown, as the game's joint-exploder moves them
/// with its default tuning (engine/anim/joint-exploder.gc: integrate-and-kill,
/// do-collision-response, joint-exploder-shatter), frame by frame at 60 a second: falling at
/// 70 m/s² (-286720 units), turning by their angle each frame; off the ground they bounce back
/// with 0.75 of their speed along it and 0.7 of it up, turning half as fast; they shrink to nothing
/// in 2 s, their height in 1.5 s
fn exploded(rest: &[Mat4], velocity: &[Vec3], spin: &[Vec3], ground: f32, t: f32) -> Vec<Mat4> {
  const DT: f32 = 1.0 / 60.0;
  let frames = (t / DT) as usize;
  let shrink = |d: f32| (1.0 - t / d).max(0.0001);
  let scale = Mat4::scale(Vec3::new(
    shrink(EXPLODE_SECONDS),
    shrink(0.75 * EXPLODE_SECONDS),
    shrink(EXPLODE_SECONDS),
  ));
  let mut worlds = rest.to_vec();
  for (i, w) in worlds.iter_mut().enumerate() {
    let (Some(&v), Some(&s)) = (velocity.get(i), spin.get(i)) else {
      continue;
    };
    let at = rest[i].col3(3);
    let (mut pos, mut v, mut angle, mut turned) = (at, v, length(s), 0.0);
    for _ in 0..frames {
      v.y -= 70.0 * DT;
      pos += v * DT;
      turned += angle;
      if pos.y < ground {
        pos.y = ground + 0.01;
        v = Vec3::new(v.x * 0.75, -v.y * 0.7, v.z * 0.75);
        angle *= 0.5;
      }
    }
    let q = if turned > 0.0 {
      Quat::axis_angle(s, turned)
    } else {
      Quat::default()
    };
    *w = Mat4::translate(pos) * Mat4::rotate(q) * scale * Mat4::translate(at * -1.0) * rest[i];
  }
  worlds
}

/// A small generator of the same numbers each time (the pieces of a preview thrown the same way):
/// -1..1
struct Rng(u64);

impl Rng {
  fn next(&mut self) -> f32 {
    self.0 = self
      .0
      .wrapping_mul(6364136223846793005)
      .wrapping_add(1442695040888963407);
    ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
  }
}

/// Bytes as base64 (the preview's arrays, the brush's pictures in a call's answer)
pub(crate) fn base64(data: &[u8]) -> String {
  const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
  for c in data.chunks(3) {
    let n =
      (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
    for (k, shift) in [18, 12, 6, 0].into_iter().enumerate() {
      out.push(if k <= c.len() {
        T[(n >> shift & 63) as usize] as char
      } else {
        '='
      });
    }
  }
  out
}

fn f32_bytes(v: impl Iterator<Item = f32>) -> Vec<u8> {
  v.flat_map(f32::to_le_bytes).collect()
}

/// What moves while it plays (AnimPreview)
#[derive(Default)]
struct AnimPreview {
  /// the model node drawn moving (an icon: the actor)
  node: u32,
  original: Option<Arc<Mesh>>,
  moving: Option<Arc<Mesh>>,
  bind: Option<Arc<Mesh>>,
  skeleton: Arc<Skeleton>,
  /// an animation of its model (>= 0), else K_BROKEN, K_MOTION, K_FRACTURE
  animation: i32,
  motion: i32,
  /// the movement: around the middle of its base, its height
  base: Vec3,
  height: f32,
  time: f32,
  /// broken: where its pieces were, how they fly (m/s; the axis they turn about, by the angle of a
  /// frame), the height they bounce off
  rest: Vec<Mat4>,
  velocity: Vec<Vec3>,
  spin: Vec<Vec3>,
  ground: f32,
  /// a change of the document ends it
  revision: u64,
  /// an actor without a model: `moving` is its picture, drawn over the view
  icon: bool,
  /// its lightning's zigzags, drawn again and again (they crackle)
  seed: i32,
  etype: String,
  family: String,
}

/// The preview going on, its settings, the skeletons read (model file -> its skeleton, None: none)
pub struct PreviewState {
  playing: Option<AnimPreview>,
  pub looping: bool,
  skeletons: HashMap<String, Option<Arc<Skeleton>>>,
}

impl Default for PreviewState {
  fn default() -> Self {
    Self {
      playing: None,
      looping: true,
      skeletons: HashMap::new(),
    }
  }
}

/// What an element (an actor, or a model placed from the catalog: Jak 3's Models, Jak II's
/// Objects) can show in the preview
#[derive(Default)]
struct PreviewOptions {
  /// the node drawn moving (an actor's model, the model itself)
  model: u32,
  /// its model's
  name: String,
  /// its file
  asset: Option<String>,
  /// the model of its pieces (<model>-explode)
  broken: Option<String>,
  skeleton: Option<Arc<Skeleton>>,
  /// the ones that move
  animations: Vec<usize>,
  /// broken without pieces of its own: its model cut up
  fracture: bool,
  motion: i32,
  /// an actor without a model: its picture doing what it does
  icon: bool,
  jak2: bool,
}

impl PreviewOptions {
  fn any(&self) -> bool {
    !self.animations.is_empty()
      || self.broken.is_some()
      || self.fracture
      || self.motion != 0
      || self.icon
  }
}

impl Editor {
  fn model_skeleton(&mut self, path: &str) -> Option<Arc<Skeleton>> {
    self
      .preview
      .skeletons
      .entry(path.to_string())
      .or_insert_with(|| load_skeleton(path).ok().map(Arc::new))
      .clone()
  }

  fn preview_options(&mut self, id: u32) -> PreviewOptions {
    let mut o = PreviewOptions::default();
    let Some(n) = self.scene.find(id) else {
      return o;
    };
    let (kind, extras) = (n.kind, n.extras.clone());
    let str_of = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut lib = self.library.clone();
    let mut etype = String::new();
    let mut family = String::new();
    if kind == NodeKind::Actor {
      // an actor: its model, its type's family
      etype = str_of(&extras, "etype");
      family = catalog_family(self.game_data().as_deref(), &etype);
      let model = self.scene.children(id).into_iter().rev().find(|&c| {
        self
          .scene
          .find(c)
          .is_some_and(|cn| cn.extras.get("og_actor_visual").is_some())
      });
      let with_mesh = model
        .and_then(|m| self.scene.find(m))
        .filter(|m| m.mesh.is_some());
      let Some(m) = with_mesh else {
        // nothing drawn of its own (a vent, a swing pole, an electric gate): its picture
        o.icon = true;
        o.motion = icon_motion(&etype, &family);
        return o;
      };
      o.model = m.id;
      o.name = str_of(&m.extras, "og_actor_visual");
    } else if kind == NodeKind::Render && n.mesh.is_some() && self.scene.actor_of(n) == 0 {
      // a model placed from the catalog
      o.model = id;
      o.name = str_of(&extras, "og_model");
      if let Some(imp) = extras.get("og_import").filter(|i| i.is_object())
        && str_of(imp, "game") == "jak2"
      {
        o.name = str_of(imp, "part");
        lib = self.library_jak2.clone();
        o.jak2 = true;
      }
      if let Some(imp) = extras.get("og_import").filter(|i| i.is_object())
        && str_of(imp, "game") == "user"
      {
        // a model of the user's: its animations in its file of the library (My models)
        let file = str_of(imp, "file");
        let path = self
          .catalog
          .models
          .iter()
          .find(|m| m.game == "user" && m.name == file)
          .map(|m| m.path.clone());
        if let Some(path) = path {
          o.skeleton = self.model_skeleton(&path);
          o.asset = Some(path);
          if let Some(s) = &o.skeleton {
            o.animations = (0..s.animations.len())
              .filter(|&i| s.animations[i].duration > 0.0)
              .collect();
          }
        }
        return o;
      }
      if o.name.is_empty() {
        return o;
      }
    } else {
      return o;
    }
    // the copy of its model with the most animations (the same model is saved with every level
    // that loads it, not always with them)
    let mut asset = lib.model_named(&o.name, "").cloned();
    o.skeleton = asset
      .as_ref()
      .and_then(|a| self.model_skeleton(&a.path.clone()));
    let mut best = 0;
    for copy in lib.models_named(&o.name) {
      let s = self.model_skeleton(&copy.path);
      let moving = s.as_ref().map_or(0, |s| {
        s.animations.iter().filter(|a| a.duration > 0.0).count()
      });
      if moving > best {
        best = moving;
        asset = Some(copy.clone());
        o.skeleton = s;
      }
    }
    o.broken = asset.as_ref().and_then(|a| {
      lib
        .model_named(&format!("{}-explode", o.name), &a.group)
        .map(|m| m.path.clone())
    });
    o.asset = asset.map(|a| a.path);
    if let Some(s) = &o.skeleton {
      o.animations = (0..s.animations.len())
        .filter(|&i| s.animations[i].duration > 0.0)
        .collect();
    }
    if !etype.is_empty() {
      o.fracture = o.broken.is_none() && breaks(&etype, &family);
      if o.animations.is_empty() {
        o.motion = object_motion(&etype, &family);
      }
    }
    o
  }

  /// What get_node tells an assistant of it (PreviewOptions::to_json): its animations, its
  /// movement, how it breaks, its picture; None when it shows nothing
  pub fn preview_summary(&mut self, id: u32) -> Option<Value> {
    let o = self.preview_options(id);
    if !o.any() {
      return None;
    }
    let anims: Vec<&str> = o.skeleton.as_ref().map_or(Vec::new(), |s| {
      o.animations
        .iter()
        .map(|&i| s.animations[i].name.as_str())
        .collect()
    });
    let mut j = json!({"animations": anims});
    if o.motion != 0 {
      j["movement"] = json!(MOTION_KEYS[o.motion as usize]);
    }
    if o.broken.is_some() || o.fracture {
      j["break"] = json!(if o.broken.is_some() {
        "its pieces"
      } else {
        "its model cut up"
      });
    }
    if o.icon {
      j["picture"] = json!(true);
    }
    Some(j)
  }

  /// The inspector's section (ui_preview_inspector): `section` false for a model of the catalog
  /// that does nothing (a piece of decor neither)
  pub(crate) fn preview_options_json(&mut self, id: u32) -> Value {
    let Some(kind) = self.scene.find(id).map(|n| n.kind) else {
      return Value::Null;
    };
    let o = self.preview_options(id);
    let moving = if o.icon { id } else { o.model };
    let playing = self
      .preview
      .playing
      .as_ref()
      .is_some_and(|p| p.node == moving || p.node == id);
    let animations: Vec<Value> = o.skeleton.as_ref().map_or(Vec::new(), |s| {
      o.animations
        .iter()
        .map(|&i| {
          let a = &s.animations[i];
          let label = strf!(
            tr("preview.animation"),
            short_name(&a.name, &o.name),
            a.duration
          );
          json!({"index": i, "name": a.name, "label": label})
        })
        .collect()
    });
    // the editor's gameplay flags (where Jak starts, a checkpoint, a creature's spawn point):
    // nothing of theirs to preview
    let marker = self.scene.children(id).into_iter().any(|c| {
      self
        .scene
        .find(c)
        .is_some_and(|n| n.extras.get("og_marker").is_some())
    });
    json!({
      "section": !marker && (kind == NodeKind::Actor || o.any()),
      "any": o.any(),
      "icon": o.icon,
      "motion": MOTION_KEYS[o.motion.clamp(0, 13) as usize],
      "break": o.broken.is_some() || o.fracture,
      "animations": animations,
      "playing": playing,
      "loop": self.preview.looping,
    })
  }

  pub fn stop_preview(&mut self) {
    let Some(p) = self.preview.playing.take() else {
      return;
    };
    if p.icon {
      return;
    }
    if let Some(n) = self.scene.find_mut(p.node)
      && let (Some(m), Some(moving), Some(original)) = (&n.mesh, &p.moving, p.original)
      && Arc::ptr_eq(m, moving)
    {
      n.mesh = Some(original);
      self.scene_changed();
      self.scene.begin_frame();
    }
  }

  /// Its model (or its file at `path` again, with each vertex's joints) moving in place of its own
  /// mesh: an animation (>= 0), K_BROKEN, K_MOTION, K_FRACTURE
  fn start_preview(
    &mut self,
    model_node: u32,
    path: &str,
    animation: i32,
    motion: i32,
    jak2: bool,
  ) -> bool {
    self.stop_preview();
    let Some(original) = self.scene.find(model_node).and_then(|n| n.mesh.clone()) else {
      return false;
    };
    let mut p = AnimPreview {
      node: model_node,
      animation,
      motion,
      ..Default::default()
    };
    let (bind, mut moving, skeleton) = if animation == K_MOTION || animation == K_FRACTURE {
      // its own model, as one piece or cut in pieces about a third of its size
      let b = original.bounds();
      let size = if b.valid() {
        b.hi - b.lo
      } else {
        Vec3::new(1.0, 1.0, 1.0)
      };
      let cell = if animation == K_FRACTURE {
        size.x.max(size.y).max(size.z).max(0.3) / 3.0
      } else {
        0.0
      };
      let (bind, s) = cut_in_pieces(&original, cell);
      p.base = base_of(&b);
      p.height = size.y;
      let moving = bind.clone();
      (bind, moving, s)
    } else {
      // the file again with each vertex's joints: the same vertices as the model in the document
      let read = load_model_asset(path, true).and_then(|lm| Ok((lm, load_skeleton(path)?)));
      let (lm, s) = match read {
        Ok(r) => r,
        Err(e) => {
          log_warn!("%s: %s", path, e);
          return false;
        }
      };
      // broken: the model of its pieces in place of its own, its materials the document's (Jak
      // II's textures named apart)
      let moving = if animation >= 0 {
        (*original).clone()
      } else if jak2 {
        (*import_jak2_mesh(&mut self.scene, &lm.mesh, &lm.textures, &lm.materials, None)).clone()
      } else {
        (*merge_mesh(&mut self.scene, &lm.mesh, &lm.textures, &lm.materials)).clone()
      };
      ((*lm.mesh).clone(), moving, s)
    };
    let mut check = moving.clone();
    if !skin_mesh(
      &bind,
      &joint_matrices(&skeleton, &skeleton_pose(&skeleton, -1, 0.0)),
      &mut check,
    ) {
      log_warn!(tr("log.preview_mismatch"), path);
      return false;
    }
    // the node draws the bind pose (the viewport moves its vertices from there)
    for (m, b) in moving.prims.iter_mut().zip(&bind.prims) {
      m.pos = b.pos.clone();
      m.nrm = if b.nrm.len() == b.pos.len() {
        b.nrm.clone()
      } else {
        vec![Vec3::new(0.0, 1.0, 0.0); b.pos.len()]
      };
    }
    moving.touch();
    if animation == K_BROKEN || animation == K_FRACTURE {
      // each piece thrown as the game's joint-exploder throws it by default (its tuning 0,
      // engine/anim/joint-exploder.gc): -20 to 20 m/s along the ground, 5 to 15 m/s up, turning
      // 8.4 degrees a frame about an axis of its own; the ground: the bottom of the model
      p.rest = skeleton_pose(&skeleton, -1, 0.0);
      let b = bind.bounds();
      p.ground = if b.valid() { b.lo.y } else { 0.0 };
      let mut rng = Rng(7);
      for _ in &p.rest {
        p.velocity.push(Vec3::new(
          20.0 * rng.next(),
          10.0 + 5.0 * rng.next(),
          20.0 * rng.next(),
        ));
        let axis = Vec3::new(rng.next(), rng.next(), rng.next());
        let axis = if length(axis) > 0.01 {
          normalize(axis)
        } else {
          Vec3::new(0.0, 1.0, 0.0)
        };
        p.spin.push(axis * radians(8.4));
      }
    }
    let moving = Arc::new(moving);
    p.original = Some(original);
    p.moving = Some(moving.clone());
    p.bind = Some(Arc::new(bind));
    p.skeleton = Arc::new(skeleton);
    p.revision = self.history.revision();
    self.preview.playing = Some(p);
    if let Some(n) = self.scene.find_mut(model_node) {
      n.mesh = Some(moving);
    }
    self.scene_changed();
    self.scene.begin_frame();
    true
  }

  /// An actor without a model: its picture doing what it does, over the view
  fn start_icon_preview(&mut self, actor: u32, motion: i32) -> bool {
    self.stop_preview();
    let Some(n) = self.scene.find(actor) else {
      return false;
    };
    let etype = n
      .extras
      .get("etype")
      .and_then(Value::as_str)
      .unwrap_or("")
      .to_string();
    let family = catalog_family(self.game_data().as_deref(), &etype);
    // climbing: someone on its model (a ladder), as high as it
    let icon = object_icon(if motion == CLIMB { "climber" } else { &etype }, &family, 0);
    let mut b = icon.bounds();
    if motion == CLIMB {
      for c in self.scene.children(actor) {
        if let Some(m) = self.scene.find(c).and_then(|cn| cn.mesh.as_ref()) {
          b = m.bounds();
        }
      }
    }
    self.preview.playing = Some(AnimPreview {
      node: actor,
      icon: true,
      motion,
      moving: Some(icon),
      height: if b.valid() { b.hi.y - b.lo.y } else { 1.0 },
      revision: self.history.revision(),
      etype,
      family,
      ..Default::default()
    });
    true
  }

  /// the icon a preview draws (the protocol serves it as a mesh)
  pub(crate) fn preview_icon_mesh(&self) -> Option<Arc<Mesh>> {
    self
      .preview
      .playing
      .as_ref()
      .filter(|p| p.icon)
      .and_then(|p| p.moving.clone())
  }

  /// What the viewport needs to play it: the node drawn moving, its mesh (bind pose), each moving
  /// primitive's joints (u16 x 4 a vertex) and weights (f32 x 4), base64; an icon: its mesh
  fn preview_package(&self) -> Value {
    let Some(p) = &self.preview.playing else {
      return Value::Null;
    };
    let Some(m) = &p.moving else {
      return Value::Null;
    };
    if p.icon {
      return json!({"node": p.node, "icon": true, "mesh": m.uid as u32, "version": m.version as u32});
    }
    let mut skin = Vec::new();
    if let Some(bind) = &p.bind {
      for (i, b) in bind.prims.iter().enumerate() {
        let (Some(j), Some(w)) = (
          b.extra_attrs.get("JOINTS_0"),
          b.extra_attrs.get("WEIGHTS_0"),
        ) else {
          continue; // a piece that does not move
        };
        let joints: Vec<u8> = j
          .iter()
          .flat_map(|v| [v.x, v.y, v.z, v.w])
          .flat_map(|x| (x.max(0.0) as u16).to_le_bytes())
          .collect();
        let weights = f32_bytes(w.iter().flat_map(|v| [v.x, v.y, v.z, v.w]));
        skin.push(json!({"prim": i, "joints": base64(&joints), "weights": base64(&weights)}));
      }
    }
    json!({
      "node": p.node,
      "icon": false,
      "mesh": m.uid as u32,
      "version": m.version as u32,
      "joints": p.skeleton.joints.len(),
      "skin": skin,
    })
  }

  /// `dt` seconds later (update_preview): the joints' matrices (16 floats each, column-major,
  /// base64), or the icon's mesh and where it is; null when it ended (another document, the node
  /// changed, an edit)
  fn preview_frame(&mut self, dt: f32) -> Value {
    let revision = self.history.revision();
    let looping = self.preview.looping;
    let Some(p) = &mut self.preview.playing else {
      return Value::Null;
    };
    let Some(n) = self.scene.find(p.node) else {
      self.preview.playing = None;
      return Value::Null;
    };
    if p.icon {
      // a picture over the view: only its time goes on
      if revision != p.revision {
        self.preview.playing = None;
        return Value::Null;
      }
      p.time += dt;
      if p.motion == FLICKER {
        // lightning: its zigzags made anew 14 times a second
        let seed = 1 + (p.time * 14.0) as i32;
        if seed != p.seed {
          p.moving = Some(object_icon(&p.etype, &p.family, seed));
          p.seed = seed;
        }
      }
      let at = self.scene.world(n) * motion_at(p.motion, p.time, Vec3::default(), p.height);
      let m = p.moving.as_ref().unwrap();
      return json!({"icon": {"mesh": m.uid as u32, "version": m.version as u32, "world": at}});
    }
    if !matches!((&n.mesh, &p.moving), (Some(a), Some(b)) if Arc::ptr_eq(a, b)) {
      // another document, or the node changed: nothing to give back
      self.preview.playing = None;
      return Value::Null;
    }
    if revision != p.revision {
      self.stop_preview();
      return Value::Null;
    }
    let s = p.skeleton.clone();
    p.time += dt;
    let worlds = if p.animation >= 0 {
      let d = s.animations[p.animation as usize].duration;
      let mut t = p.time;
      if d > 0.0 {
        t = if looping {
          p.time % (d + 0.5)
        } else {
          p.time.min(d)
        };
      }
      skeleton_pose(&s, p.animation, t.min(d))
    } else if p.animation == K_MOTION {
      let t = if looping { p.time } else { p.time.min(6.0) };
      vec![motion_at(p.motion, t, p.base, p.height)]
    } else {
      // the game's 2 s, then again (or the pieces gone)
      let t = if looping {
        p.time % EXPLODE_SECONDS
      } else {
        p.time.min(EXPLODE_SECONDS)
      };
      exploded(&p.rest, &p.velocity, &p.spin, p.ground, t)
    };
    let joints = joint_matrices(&s, &worlds);
    json!({
      "node": p.node,
      "mesh": p.moving.as_ref().map_or(0, |m| m.uid as u32),
      "joints": base64(&f32_bytes(joints.iter().flat_map(|m| m.m))),
    })
  }

  /// The preview's calls (api.rs)
  pub(crate) fn preview_call(&mut self, name: &str, a: &Value) -> Result<Value, String> {
    let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    if let Some(l) = a.get("loop").and_then(Value::as_bool) {
      self.preview.looping = l;
    }
    match name {
      // {id} -> the inspector's section
      "preview_options" => Ok(self.preview_options_json(id)),
      // {id, what: animation | motion | break | effect, choice?: index into its animations, loop?}
      // -> what the viewport needs to play it, null when it cannot
      "start_preview" => {
        let o = self.preview_options(id);
        let moving = if o.icon { id } else { o.model };
        let what = a.get("what").and_then(Value::as_str).unwrap_or("animation");
        let started = match what {
          "effect" if o.icon => self.start_icon_preview(id, o.motion),
          "animation" if !o.animations.is_empty() && o.asset.is_some() => {
            let choice = a.get("choice").and_then(Value::as_u64).unwrap_or(0) as usize;
            let animation = o.animations[choice.min(o.animations.len() - 1)];
            let path = o.asset.clone().unwrap_or_default();
            self.start_preview(moving, &path, animation as i32, 0, o.jak2)
          }
          // someone on it
          "motion" if o.motion == CLIMB => self.start_icon_preview(id, o.motion),
          "motion" if o.motion != 0 && !o.icon => {
            self.start_preview(moving, "", K_MOTION, o.motion, false)
          }
          "break" if o.broken.is_some() || o.fracture => {
            let path = o.broken.clone().unwrap_or_default();
            let kind = if o.broken.is_some() {
              K_BROKEN
            } else {
              K_FRACTURE
            };
            self.start_preview(moving, &path, kind, 0, o.jak2)
          }
          _ => false,
        };
        Ok(if started {
          self.preview_package()
        } else {
          Value::Null
        })
      }
      "stop_preview" => {
        self.stop_preview();
        Ok(Value::Null)
      }
      // {dt, loop?}
      "preview_frame" => {
        let dt = a.get("dt").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        Ok(self.preview_frame(dt.clamp(0.0, 0.25)))
      }
      _ => Err(format!("unknown call: {name}")),
    }
  }

  /// what get_state shows of it: the node playing (0 none)
  pub(crate) fn preview_state(&self) -> Value {
    json!({
      "node": self.preview.playing.as_ref().map_or(0, |p| p.node),
      "loop": self.preview.looping,
    })
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use serde_json::{Value, json};

  use super::super::Editor;
  use super::{EXPLODE_SECONDS, exploded};
  use crate::io::asset_library::{AssetLibrary, game_folder, load_model_asset};
  use crate::io::test_library::{make_box, real_library};
  use crate::math::{Mat4, Vec3, length, radians};
  use crate::scene::{Node, NodeKind};

  fn editor() -> Editor {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.new_level();
    ed
  }

  fn decode(s: &str) -> Vec<u8> {
    crate::io::texture_palette::base64_decode(s.as_bytes())
  }

  fn floats(s: &str) -> Vec<f32> {
    decode(s)
      .chunks(4)
      .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
      .collect()
  }

  /// an actor of type `etype` with a box as its model
  fn actor(ed: &mut Editor, etype: &str) -> (u32, u32) {
    let a = ed.scene.add(Node {
      name: etype.into(),
      kind: NodeKind::Actor,
      extras: json!({ "etype": etype }),
      ..Default::default()
    });
    let m = ed.scene.add(Node {
      name: format!("{etype}-model"),
      parent: a,
      kind: NodeKind::Render,
      mesh: Some(make_box(etype, -1)),
      extras: json!({ "og_actor_visual": etype }),
      ..Default::default()
    });
    ed.scene.begin_frame();
    (a, m)
  }

  #[test]
  fn base64_round_trip() {
    for data in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0, 255, 7, 128, 9]] {
      assert_eq!(decode(&super::base64(data)), data);
    }
    assert_eq!(super::base64(b"ab"), "YWI=");
  }

  #[test]
  fn an_elevator_rises_a_crate_breaks_a_vent_blows() {
    let mut ed = editor();
    // an elevator without its animations (no library): the movement its code gives it
    let (elevator, model) = actor(&mut ed, "fac-elevator");
    let o = ed
      .call("preview_options", &json!({"id": elevator}))
      .unwrap();
    assert!(o["section"].as_bool().unwrap() && o["motion"] == "rise");
    assert!(!o["playing"].as_bool().unwrap());
    let original = ed.scene.find(model).unwrap().mesh.clone().unwrap();
    let p = ed
      .call("start_preview", &json!({"id": elevator, "what": "motion"}))
      .unwrap();
    assert_eq!(p["node"], model);
    let moving = ed.scene.find(model).unwrap().mesh.clone().unwrap();
    assert!(!Arc::ptr_eq(&moving, &original) && p["mesh"] == moving.uid as u32);
    assert_eq!(p["joints"], 1);
    let n = moving.prims[0].pos.len();
    assert_eq!(
      decode(p["skin"][0]["joints"].as_str().unwrap()).len(),
      n * 8
    );
    assert_eq!(
      floats(p["skin"][0]["weights"].as_str().unwrap()).len(),
      n * 4
    );
    let o = ed
      .call("preview_options", &json!({"id": elevator}))
      .unwrap();
    assert!(o["playing"].as_bool().unwrap() && ed.get_state()["preview"]["node"] == model);
    // 3 s in: 8 m up (the top of its rise)
    for _ in 0..12 {
      ed.call("preview_frame", &json!({"dt": 0.25})).unwrap();
    }
    let f = ed.call("preview_frame", &json!({"dt": 0.0})).unwrap();
    let m = floats(f["joints"].as_str().unwrap());
    assert!(
      m.len() == 16 && (m[13] - 8.0).abs() < 0.01,
      "{:?}",
      &m[12..15]
    );
    // stopped: its own mesh back, the document as it was
    ed.call("stop_preview", &Value::Null).unwrap();
    assert!(Arc::ptr_eq(
      ed.scene.find(model).unwrap().mesh.as_ref().unwrap(),
      &original
    ));
    assert!(
      ed.call("preview_frame", &json!({"dt": 0.1}))
        .unwrap()
        .is_null()
    );
    assert!(!ed.history.can_undo());
    // a crate cut up (the game has no pieces for it here)
    let (crate_, _) = actor(&mut ed, "crate");
    let o = ed.call("preview_options", &json!({"id": crate_})).unwrap();
    assert!(o["break"].as_bool().unwrap());
    let p = ed
      .call("start_preview", &json!({"id": crate_, "what": "break"}))
      .unwrap();
    assert!(p["joints"].as_u64().unwrap() >= 1);
    let f = ed.call("preview_frame", &json!({"dt": 0.5})).unwrap();
    assert_eq!(
      floats(f["joints"].as_str().unwrap()).len(),
      16 * p["joints"].as_u64().unwrap() as usize
    );
    // a vent: no model, its picture blowing (a mesh the protocol serves)
    let vent = ed.scene.add(Node {
      name: "vent".into(),
      kind: NodeKind::Actor,
      extras: json!({"etype": "vent-blue"}),
      ..Default::default()
    });
    ed.scene.begin_frame();
    let o = ed.call("preview_options", &json!({"id": vent})).unwrap();
    assert!(o["icon"].as_bool().unwrap() && o["motion"] == "blow");
    let p = ed
      .call("start_preview", &json!({"id": vent, "what": "effect"}))
      .unwrap();
    assert!(p["icon"].as_bool().unwrap());
    let uid = p["mesh"].as_u64().unwrap() as u32;
    assert!(ed.scene_snapshot().meshes.contains_key(&uid));
    let f = ed.call("preview_frame", &json!({"dt": 0.5})).unwrap();
    assert_eq!(f["icon"]["world"].as_array().unwrap().len(), 16);
    // an edit of the document: it ends
    ed.history
      .push(crate::history::HistoryEntry::new("x", |_| {}, |_| {}));
    assert!(
      ed.call("preview_frame", &json!({"dt": 0.1}))
        .unwrap()
        .is_null()
    );
  }

  #[test]
  fn pieces_fly_as_the_game_throws_them() {
    // a piece 1 m up, thrown at 10 m/s along x and up: it never goes through the ground, slows
    // down by bouncing, and is gone (shrunk) after 2 s
    let rest = [Mat4::translate(Vec3::new(0.0, 1.0, 0.0))];
    let (v, s) = (
      [Vec3::new(10.0, 10.0, 0.0)],
      [Vec3::new(0.0, radians(8.4), 0.0)],
    );
    let mut last_x = 0.0;
    for k in 1..=20 {
      let w = exploded(&rest, &v, &s, 0.0, k as f32 * 0.1)[0];
      let at = w.col3(3);
      assert!(at.y >= 0.0 && at.x > last_x, "{k}: {at:?}");
      last_x = at.x;
    }
    assert!(last_x < 20.0, "bounces slow it down: {last_x}");
    let end = exploded(&rest, &v, &s, 0.0, EXPLODE_SECONDS)[0];
    assert!(length(end.col3(0)) < 0.01 && length(end.col3(1)) < 0.01);
  }

  #[test]
  fn a_real_model_s_animations() {
    // the options of a model of the user's extracted game (skipped without it)
    let Some(root) = real_library() else {
      return;
    };
    let mut lib = AssetLibrary::default();
    lib.scan(&game_folder(root.to_str().unwrap())).unwrap();
    // a model that moves: the first with animations among the doors and elevators
    let found = lib
      .models()
      .iter()
      .filter(|m| m.name.contains("door") || m.name.contains("elevator"))
      .take(80)
      .find(|m| {
        crate::io::gltf::load_skeleton(&m.path)
          .is_ok_and(|s| s.animations.iter().any(|a| a.duration > 0.0))
      })
      .cloned();
    let Some(m) = found else {
      return;
    };
    let mut ed = editor();
    ed.library = Arc::new(lib);
    let lm = load_model_asset(&m.path, false).unwrap();
    let id = ed.scene.add(Node {
      name: m.name.clone(),
      kind: NodeKind::Render,
      mesh: Some(lm.mesh.clone()),
      extras: json!({"og_model": m.name}),
      ..Default::default()
    });
    ed.scene.begin_frame();
    let o = ed.call("preview_options", &json!({"id": id})).unwrap();
    let anims = o["animations"].as_array().unwrap();
    assert!(!anims.is_empty(), "{}: its animations", m.name);
    let p = ed
      .call(
        "start_preview",
        &json!({"id": id, "what": "animation", "choice": 0}),
      )
      .unwrap();
    assert!(!p.is_null(), "{}: played", m.name);
    let f = ed.call("preview_frame", &json!({"dt": 0.3})).unwrap();
    let joints = decode(f["joints"].as_str().unwrap()).len() / 64;
    assert_eq!(joints as u64, p["joints"].as_u64().unwrap());
    eprintln!(
      "{}: {} animations ({}), {joints} joints",
      m.name,
      anims.len(),
      anims[0]["label"]
    );
  }
}
