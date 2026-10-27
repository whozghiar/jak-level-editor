//! What the game does with an element of the decor (a wall, a slab, a vent grate, a shape), kept
//! in its node's extras "og_decor" and written by the export for the game (the appendix of docs/user-guide.md):
//!
//!   collision  how it collides, through the level builder's own fields (nothing generated):
//!              "shape" (the default: by its shape, made by the builder), "none" (Jak goes through
//!              it: cables, light shafts) or "invisible" (not drawn, it collides: an invisible
//!              wall); its surface (stone, ice, sand, wood, metal, grass...: the sound of the
//!              steps, ice slides), what it does to Jak (deadly, burn, endless fall, melt), its
//!              mode (ground, wall, obstacle; walls are found from the slope anyway) and its flags
//!              (Jak cannot hang from its edges, the camera and the creatures go through it, it
//!              does not hide Jak)
//!   bounce     Jak touching its top is thrown up `height` meters, on foot and on the board: a
//!              bouncer of the game without a model of its own (as the board vents of the
//!              industrial zone are), a sphere over the element, generated as the level's code
//!   launch     Jak touching its top is thrown along an arc to a point `distance` meters away in
//!              the direction `yaw` (degrees, around the element's own facing), `height` meters
//!              above it, in `time` seconds: the game's own launch (the Metal Head puffers'),
//!              generated
//!
//! Breaking or deforming an element needs a model of its own that the game draws as an actor (the
//! decor the builder makes is drawn as one): not offered for the decor (the user guide's appendix says
//! how).

use std::fmt::Write as _;

use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct DecorCollision {
  /// shape, none, invisible
  pub kind: String,
  /// the builder's surfaces: stone, ice, quicksand, waterbottom, tar, sand, wood, grass, pcmetal,
  /// snow, deepsnow, hotcoals, lava, crwood, gravel, dirt, metal, straw, tube, swamp, stopproj,
  /// rotate, neutral
  pub material: String,
  /// none, deadly, endlessfall, burn, deadlyup, burnup, melt
  pub event: String,
  /// auto (ground, walls by their slope), ground, wall, obstacle
  pub mode: String,
  pub noedge: bool,
  pub nocamera: bool,
  pub noentity: bool,
  pub nolineofsight: bool,
  /// the node has one (else the export leaves it as it was)
  pub set: bool,
}

impl Default for DecorCollision {
  fn default() -> Self {
    Self {
      kind: "shape".into(),
      material: "stone".into(),
      event: "none".into(),
      mode: "auto".into(),
      noedge: false,
      nocamera: false,
      noentity: false,
      nolineofsight: false,
      set: false,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecorBounce {
  pub set: bool,
  /// meters (the game's default 45056 units)
  pub height: f32,
}

impl Default for DecorBounce {
  fn default() -> Self {
    Self {
      set: false,
      height: 11.0,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecorLaunch {
  pub set: bool,
  /// meters, along the ground
  pub distance: f32,
  /// meters above where it starts
  pub height: f32,
  /// degrees, around the element's facing (its local +z)
  pub yaw: f32,
  /// seconds in the air
  pub time: f32,
}

impl Default for DecorLaunch {
  fn default() -> Self {
    Self {
      set: false,
      distance: 20.0,
      height: 8.0,
      yaw: 0.0,
      time: 1.0,
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecorBehavior {
  pub collision: DecorCollision,
  pub bounce: DecorBounce,
  pub launch: DecorLaunch,
}

impl DecorBehavior {
  pub fn any(&self) -> bool {
    self.collision.set || self.bounce.set || self.launch.set
  }
}

fn meters_of(m: f32) -> String {
  format!("{:.1}", m * 4096.0)
}

/// Jak 1's surfaces, in their order: the level builder takes them and converts them to Jak 3's
pub const COLLISION_MATERIALS: [&str; 23] = [
  "stone",
  "ice",
  "quicksand",
  "waterbottom",
  "tar",
  "sand",
  "wood",
  "grass",
  "pcmetal",
  "snow",
  "deepsnow",
  "hotcoals",
  "lava",
  "crwood",
  "gravel",
  "dirt",
  "metal",
  "straw",
  "tube",
  "swamp",
  "stopproj",
  "rotate",
  "neutral",
];

pub const COLLISION_EVENTS: [&str; 7] = [
  "none",
  "deadly",
  "endlessfall",
  "burn",
  "deadlyup",
  "burnup",
  "melt",
];

/// The names the editor offers, the builder's numbers (Jak 1's layout, which it converts: stone 0)
pub fn collision_materials() -> &'static [&'static str] {
  &COLLISION_MATERIALS
}

pub fn collision_events() -> &'static [&'static str] {
  &COLLISION_EVENTS
}

/// -1 when unknown
pub fn builder_material(name: &str) -> i32 {
  COLLISION_MATERIALS
    .iter()
    .position(|&m| m == name)
    .map_or(-1, |i| i as i32)
}

pub fn builder_event(name: &str) -> i32 {
  COLLISION_EVENTS
    .iter()
    .position(|&e| e == name)
    .map_or(-1, |i| i as i32)
}

/// -1 for auto
pub fn builder_mode(name: &str) -> i32 {
  match name {
    "ground" => 0,
    "wall" => 1,
    "obstacle" => 2,
    _ => -1,
  }
}

/// From a node's extras.
pub fn decor_behavior_of(extras: &Value) -> DecorBehavior {
  let mut b = DecorBehavior::default();
  let Some(d) = extras.get("og_decor").filter(|d| d.is_object()) else {
    return b;
  };
  let text = |c: &Value, key: &str, def: &str| {
    c.get(key)
      .and_then(Value::as_str)
      .unwrap_or(def)
      .to_string()
  };
  let flag = |c: &Value, key: &str| c.get(key).and_then(Value::as_bool).unwrap_or(false);
  let num =
    |c: &Value, key: &str, def: f32| c.get(key).and_then(Value::as_f64).map_or(def, |v| v as f32);
  if let Some(c) = d.get("collision").filter(|c| c.is_object()) {
    let k = &mut b.collision;
    k.set = true;
    k.kind = text(c, "kind", &k.kind);
    if k.kind != "none" && k.kind != "invisible" {
      k.kind = "shape".into();
    }
    k.material = text(c, "material", &k.material);
    if builder_material(&k.material) < 0 {
      k.material = "stone".into();
    }
    k.event = text(c, "event", &k.event);
    if builder_event(&k.event) < 0 {
      k.event = "none".into();
    }
    k.mode = text(c, "mode", &k.mode);
    if builder_mode(&k.mode) < 0 {
      k.mode = "auto".into();
    }
    k.noedge = flag(c, "noedge");
    k.nocamera = flag(c, "nocamera");
    k.noentity = flag(c, "noentity");
    k.nolineofsight = flag(c, "nolineofsight");
  }
  if let Some(c) = d.get("bounce").filter(|c| c.is_object()) {
    b.bounce.set = true;
    b.bounce.height = num(c, "height", b.bounce.height).clamp(0.5, 200.0);
  }
  if let Some(c) = d.get("launch").filter(|c| c.is_object()) {
    let l = &mut b.launch;
    l.set = true;
    l.distance = num(c, "distance", l.distance).clamp(0.0, 500.0);
    l.height = num(c, "height", l.height).clamp(-200.0, 200.0);
    l.yaw = num(c, "yaw", l.yaw);
    l.time = num(c, "time", l.time).clamp(0.2, 10.0);
  }
  b
}

/// What og_decor holds, null when nothing.
pub fn decor_behavior_json(b: &DecorBehavior) -> Value {
  if !b.any() {
    return Value::Null;
  }
  let mut j = json!({});
  if b.collision.set {
    let c = &b.collision;
    j["collision"] =
      json!({ "kind": c.kind, "material": c.material, "event": c.event, "mode": c.mode });
    for (key, on) in [
      ("noedge", c.noedge),
      ("nocamera", c.nocamera),
      ("noentity", c.noentity),
      ("nolineofsight", c.nolineofsight),
    ] {
      if on {
        j["collision"][key] = true.into();
      }
    }
  }
  if b.bounce.set {
    j["bounce"] = json!({ "height": b.bounce.height });
  }
  if b.launch.set {
    let l = &b.launch;
    j["launch"] =
      json!({ "distance": l.distance, "height": l.height, "yaw": l.yaw, "time": l.time });
  }
  j
}

/// The glTF extras the level builder reads for an element with that collision
pub fn collision_extras(c: &DecorCollision) -> Value {
  if c.kind == "none" {
    return json!({ "set_collision": 1, "ignore": 1 });
  }
  let mut j = json!({
    "set_collision": 1,
    "collide_material": builder_material(&c.material).max(0),
    "collide_event": builder_event(&c.event).max(0),
  });
  if builder_mode(&c.mode) >= 0 {
    j["collide_mode"] = builder_mode(&c.mode).into();
  }
  for (key, on) in [
    ("set_invisible", c.kind == "invisible"),
    ("noedge", c.noedge),
    ("nocamera", c.nocamera),
    ("noentity", c.noentity),
    ("nolineofsight", c.nolineofsight),
  ] {
    if on {
      j[key] = 1.into();
    }
  }
  j
}

/// The GOAL code of a bouncer without a model, over an element, `radius` meters around: `ty`
/// derives the game's bouncer
pub fn bounce_code(ty: &str, radius: f32, comment: &str) -> String {
  // the game's bouncer without a model (as the industrial zone's board vents, ctyinda-obs.gc): a
  // sphere over the element that Jak overlaps. On the board and in a dive the bouncer's own events
  // throw him; on foot, touching it from above while not going up does (standing, Jak 3's transv
  // still goes up a hair, 2 units a second: up to 0.5 m/s counts as not going up)
  let mut o = String::new();
  if !comment.is_empty() {
    let _ = writeln!(o, ";; {comment}");
  }
  let r = meters_of(radius);
  let _ = write!(
    o,
    "(deftype {ty} (bouncer) ())

(defmethod init-skel! ((this {ty}))
  0
  (none)
  )

(defmethod init-collision! ((this {ty}))
  (let ((s5-0 (new 'process 'collide-shape this (collide-list-enum hit-by-player))))
    (let ((v1-2 (new 'process 'collide-shape-prim-sphere s5-0 (the-as uint 9))))
      (set! (-> v1-2 prim-core collide-as) (collide-spec crate))
      (set! (-> v1-2 prim-core collide-with) (collide-spec jak player-list))
      (set! (-> v1-2 prim-core action) (collide-action))
      (set-vector! (-> v1-2 local-sphere) 0.0 2048.0 0.0 {r})
      (set! (-> s5-0 total-prims) (the-as uint 1))
      (set! (-> s5-0 root-prim) v1-2)
      )
    (set! (-> s5-0 nav-radius) (* 0.75 (-> s5-0 root-prim local-sphere w)))
    (let ((v1-5 (-> s5-0 root-prim)))
      (set! (-> s5-0 backup-collide-as) (-> v1-5 prim-core collide-as))
      (set! (-> s5-0 backup-collide-with) (-> v1-5 prim-core collide-with))
      )
    (set! (-> this root) s5-0)
    )
  0
  (none)
  )

(defstate idle ({ty})
  :virtual #t
  :event (behavior ((proc process) (argc int) (message symbol) (block event-message-block))
    (case message
      (('touch)
       (when (and (= proc *target*)
                  (< (- (-> self root trans y) 1228.8) (-> *target* control trans y))
                  (>= 2048.0 (vector-dot (-> *target* control transv) (-> *target* control dynam gravity-normal)))
                  (send-event proc 'jump (-> self spring-height) (-> self spring-height) (-> self mods))
                  )
         (play-sound self)
         (go-virtual fire)
         )
       )
      (else
        (call-parent-state-handler event :type (function process int symbol event-message-block object)
                                   proc argc message block)
        )
      )
    )
  )

"
  );
  o
}

/// The GOAL code of a launch from an element: Jak within `radius` meters of it is thrown to `to`
/// (game units, the world) in `time` seconds
pub fn launch_code(ty: &str, radius: f32, to: [f32; 3], time: f32, comment: &str) -> String {
  // the game's launch (target-launch-dir, sent the way mhcity-obs2.gc's puffers send it: the way to
  // the landing point, the time, the gravity -327680.0 as its bits), when Jak comes on the element
  let landing = format!("{:.1} {:.1} {:.1}", to[0], to[1], to[2]);
  let seconds = (time * 300.0 + 0.5) as i32;
  let r = meters_of(radius);
  let mut o = String::new();
  if !comment.is_empty() {
    let _ = writeln!(o, ";; {comment}");
  }
  let _ = write!(
    o,
    "(deftype {ty} (process-drawable)
  ((landing  vector  :inline))
  (:state-methods
    idle
    )
  )

(defmethod init-from-entity! ((this {ty}) (arg0 entity-actor))
  (set! (-> this root) (new 'process 'trsqv))
  (process-drawable-from-entity! this arg0)
  (set-vector! (-> this landing) {landing} 1.0)
  (go (method-of-object this idle))
  )

(defstate idle ({ty})
  :virtual #t
  :trans (behavior ()
    (when (and *target*
               (< (vector-vector-xz-distance (-> self root trans) (target-pos 0)) {r})
               (< (fabs (- (-> (target-pos 0) y) (-> self root trans y))) 6144.0)
               )
      (send-event *target* 'launch-dir
                  (vector-! (new 'stack-no-clear 'vector) (-> self landing) (target-pos 0))
                  {seconds}
                  -929038336
                  )
      )
    )
  :code (behavior ()
    (until #f
      (suspend)
      )
    #f
    )
  )

"
  );
  o
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_decors_behaviors() {
    // a slab of deadly ice, a cable Jak goes through, a pad that bounces, a ramp that launches
    let of = |d: Value| decor_behavior_of(&json!({ "og_decor": d }));
    let ice = of(json!({"collision": {"material": "ice", "event": "deadly", "noedge": true}}));
    let cable = of(json!({"collision": {"kind": "none"}}));
    let pad = of(json!({"bounce": {"height": 15}}));
    let ramp = of(json!({"launch": {"distance": 30, "height": 5, "yaw": 90, "time": 1.5}}));
    let ice_extras = collision_extras(&ice.collision);
    assert!(
      ice_extras["collide_material"] == 1
        && ice_extras["collide_event"] == 1
        && ice_extras["noedge"] == 1
        && ice_extras.get("collide_mode").is_none()
    );
    assert_eq!(collision_extras(&cable.collision)["ignore"], 1);
    assert!(pad.bounce.set && pad.bounce.height == 15.0 && !pad.collision.set);
    assert!(ramp.launch.set && ramp.launch.distance == 30.0 && ramp.launch.time == 1.5);
    // what og_decor holds reads back the same; nothing set: null
    assert_eq!(of(decor_behavior_json(&ice)), ice);
    assert_eq!(of(decor_behavior_json(&ramp)), ramp);
    assert!(decor_behavior_json(&DecorBehavior::default()).is_null());
    // the values out of range or unknown: the game's
    let odd = of(json!({
      "collision": {"kind": "glass", "material": "jelly", "mode": "roof"},
      "bounce": {"height": 1000}
    }));
    assert!(
      odd.collision.kind == "shape"
        && odd.collision.material == "stone"
        && odd.collision.mode == "auto"
    );
    assert_eq!(odd.bounce.height, 200.0);
    // the code generated: a bouncer, a launch to its landing point in 450 frames (1.5 s)
    let bounce = bounce_code("my-decor-pad-bounce", 1.5, "the pad");
    assert!(bounce.starts_with(";; the pad\n(deftype my-decor-pad-bounce (bouncer) ())"));
    assert!(bounce.contains("(set-vector! (-> v1-2 local-sphere) 0.0 2048.0 0.0 6144.0)"));
    let launch = launch_code(
      "my-decor-ramp-launch",
      2.0,
      [184320.0, 28672.0, 0.0],
      1.5,
      "",
    );
    assert!(
      launch.contains("(deftype my-decor-ramp-launch (process-drawable)")
        && launch.contains("(set-vector! (-> this landing) 184320.0 28672.0 0.0 1.0)")
        && launch.contains("'launch-dir")
        && launch.contains("                  450\n")
    );
  }
}
