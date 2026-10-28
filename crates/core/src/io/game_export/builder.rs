//! What OpenGOAL's level builder (goalc/build_level) reads: its names of the collision's
//! materials and events, the actors' res lumps, the regions' scripts, the level's definition
//! (level-info.gc).

use super::*;

pub(super) const K_MATERIALS: [&str; 34] = [
  "none",
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
  "stone",
  "crmetal",
  "carpet",
  "grmetal",
  "shmetal",
  "hdwood",
  "squish",
  "mhshroom",
  "forest",
  "mhswamp",
  "dmaker",
];

pub(super) const K_EVENTS: [&str; 20] = [
  "none",
  "deadly",
  "endlessfall",
  "burn",
  "deadlyup",
  "burnup",
  "melt",
  "slide",
  "lip",
  "lipramp",
  "shock",
  "shockup",
  "hide",
  "rail",
  "slippery",
  "drag",
  "waterfloor",
  "hang",
  "fry",
  "slime",
];

pub(super) fn material_name(m: usize) -> String {
  K_MATERIALS
    .get(m)
    .map_or_else(|| format!("material {m}"), |s| s.to_string())
}

pub(super) fn event_name(e: usize) -> String {
  K_EVENTS
    .get(e)
    .map_or_else(|| format!("event {e}"), |s| s.to_string())
}

pub(super) fn strip_quote(s: &str) -> &str {
  s.strip_prefix('\'').unwrap_or(s)
}

pub(super) fn is_integer(v: &Value) -> bool {
  v.is_i64() || v.is_u64()
}

/// the numbers of a lump value, flattened
pub(super) fn numbers<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
  match v {
    Value::Number(_) => out.push(v),
    Value::Array(a) => a.iter().for_each(|x| numbers(x, out)),
    _ => {}
  }
}

/// a guess of the type of an untyped lump value (projects of older extractions)
pub(super) fn guess_type(v: &Value) -> &'static str {
  match v {
    Value::String(s) => {
      if strip_quote(s) != s {
        "symbol"
      } else {
        "string"
      }
    }
    Value::Number(_) if is_integer(v) => "int32",
    Value::Number(_) => "float",
    Value::Array(a) if !a.is_empty() => {
      if a[0].is_string() {
        guess_type(&a[0])
      } else if a[0].is_array() {
        "vector"
      } else if a.iter().all(is_integer) {
        "int32"
      } else {
        "float"
      }
    }
    _ => "",
  }
}

/// A script of a region as the builder writes it: its integers as floats (the builder would make
/// them a basic, where the game has a fixnum; the commands read a float as well)
pub(super) fn builder_script(script: &str) -> String {
  let s = script.as_bytes();
  let mut out = Vec::with_capacity(s.len() + 16);
  let mut in_string = false;
  let mut i = 0;
  while i < s.len() {
    let c = s[i];
    if c == b'"' && (i == 0 || s[i - 1] != b'\\') {
      in_string = !in_string;
    }
    let start = !in_string
      && (c.is_ascii_digit() || (c == b'-' && i + 1 < s.len() && s[i + 1].is_ascii_digit()))
      && (i == 0 || s[i - 1] == b' ' || s[i - 1] == b'(');
    if !start {
      out.push(c);
      i += 1;
      continue;
    }
    let mut end = i + 1;
    while end < s.len() && s[end] != b' ' && s[end] != b')' {
      end += 1;
    }
    let token = &s[i..end];
    out.extend_from_slice(token);
    if token.iter().all(|b| b"-0123456789".contains(b)) {
      out.extend_from_slice(b".0");
    }
    i = end;
  }
  String::from_utf8_lossy(&out).into_owned()
}

pub(super) fn region_id(r: &Value) -> u32 {
  r.get("id").and_then(Value::as_u64).unwrap_or(0) as u32
}

/// The regions for the builder: one prim per id (a region of the game with several faces gets a new
/// id for each other face, from `next_id`), the scripts as it writes them
pub(super) fn builder_regions(trees: &Value, mut next_id: u32, count: &mut usize) -> Value {
  let mut out = Map::new();
  let Some(trees) = trees.as_object() else {
    return Value::Object(out);
  };
  let regions_of = |tree: &Value| -> Vec<Value> {
    tree
      .get("regions")
      .and_then(Value::as_array)
      .cloned()
      .unwrap_or_default()
  };
  let mut taken = BTreeSet::new();
  for tree in trees.values() {
    taken.extend(regions_of(tree).iter().map(region_id));
  }
  let mut seen = BTreeSet::new();
  for (name, tree) in trees {
    if !tree.is_object() || tree.get("bsphere").is_none() || tree.get("regions").is_none() {
      continue;
    }
    let mut regions = Vec::new();
    for mut r in regions_of(tree) {
      let mut id = region_id(&r);
      if !seen.insert(id) {
        while taken.contains(&next_id) {
          next_id += 1;
        }
        id = next_id;
        next_id += 1;
        taken.insert(id);
        seen.insert(id);
      }
      if let Some(o) = r.as_object_mut() {
        o.insert("id".into(), id.into());
        for k in ["on-enter", "on-inside", "on-exit"] {
          if let Some(Value::String(s)) = o.get(k) {
            let script = builder_script(s);
            o.insert(k.into(), script.into());
          }
        }
      }
      regions.push(r);
    }
    *count += regions.len();
    out.insert(
      name.clone(),
      json!({"bsphere": tree["bsphere"], "regions": regions}),
    );
  }
  Value::Object(out)
}

pub(super) fn meters(v: f32) -> String {
  format!("(meters {})", num(v as f64))
}

pub(super) fn vectorm(v: Vec3) -> String {
  format!(
    "{} {} {}",
    num(v.x as f64),
    num(v.y as f64),
    num(v.z as f64)
  )
}

/// A continue point of the level's (where Jak starts or continues): `yaw` the way he faces, the
/// camera 6 m behind him and 3 m up
fn continue_point(name: &str, level: &str, nick: &str, at: Vec3, yaw: f32) -> String {
  let q = (
    ((yaw / 2.0).sin() * 32767.0).round() as i32,
    ((yaw / 2.0).cos() * 32767.0).round() as i32,
  );
  let cam = at + Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), yaw).rotate(Vec3::new(0.0, 3.0, -6.0));
  // as the game writes them: 0, #x7fff; a negative one in decimal
  let hex = |v: i32| {
    if v <= 0 {
      format!("{v}")
    } else {
      format!("#x{v:x}")
    }
  };
  format!(
    "(new 'static 'continue-point
        :name \"{name}\"
        :level '{level}
        :trans (static-vectorm {})
        :camera-trans (static-vectorm {})
        :quat (new 'static 'vector4h :data (new 'static 'array int16 4 0 {} 0 {}))
        :camera-rot (new 'static 'array int16 9 #x7fff 0 0 0 #x7fff 0 0 0 #x7fff)
        :on-goto #f
        :vis-nick '{nick}
        :vehicle-type #x1b
        :want-count 1
        :want (new 'static 'inline-array level-buffer-state-small 1
          (new 'static 'level-buffer-state-small :name '{level} :display? 'display)
          )
        :want-sound (new 'static 'array symbol 3 #f #f #f)
        )",
    vectorm(at),
    vectorm(cam),
    hex(q.0),
    hex(q.1),
  )
}

/// The level's definition: `start` where Jak starts and his yaw, `checkpoints` (their name in the
/// editor, where, their yaw) continue points named <level>-checkpoint-<n>
pub(super) fn level_info(
  o: &LevelExportOptions,
  start: (Vec3, f32),
  bottom: f32,
  checkpoints: &[(String, Vec3, f32)],
) -> String {
  let (name, nick) = (&o.name, &o.nickname);
  let mut continues = continue_point(&format!("{name}-start"), name, nick, start.0, start.1);
  for (k, (_, p, yaw)) in checkpoints.iter().enumerate() {
    continues += "\n      ";
    continues += &continue_point(
      &format!("{name}-checkpoint-{}", k + 1),
      name,
      nick,
      *p + Vec3::new(0.0, 0.5, 0.0),
      *yaw,
    );
  }
  format!(
    "(define {name}
  (new 'static 'level-load-info
    :name '{name}
    :visname '{name}-vis
    :nickname '{nick}
    :dbname '{name}
    :taskname 'default
    :index #x128 ;; the test-zone's: give it a number no other level has
    :master-level #f
    :level-flags (level-flags sky)
    :packages '()
    :run-packages '(\"common\")
    :memory-mode (level-memory-mode large)
    :music-bank #f
    :extra-sound-bank #f
    :mood-func 'update-mood-ctyindb ;; the city's: the level's lights lit as the editor shows them
    :special-mood #f
    :ocean #f
    :ocean-alpha 1.0
    :priority 100
    :draw-priority 10.0
    :base-task-mask (task-mask task0)
    :bigmap-id (bigmap-id none)
    :continues '({continues}
      )
    :callback-list '((33 . {name}-login)) ;; what the level builder does not write (the level's code)
    :borrow #f
    :bottom-height {}
    :fog-height (meters 80)
    :max-rain 1.0
    :fog-mult 1.0
    :mood-range (new 'static 'mood-range :max-cloud 1.0 :min-fog 0.5 :max-fog 1.0)
    )
  )

(#when PC_PORT
  (cons! *level-load-list* '{name})
  )
",
    meters(bottom)
  )
}

/// The level's own code for what OpenGOAL's level builder does not write in it, run when the game
/// births the level (`<name>-login`, the slot 33 of its level-load-info: after its bsp is born,
/// before its nav meshes are, its actors born in the frames after), each part only when the level
/// lacks it (the builder of the editor's jak-project branch writes them):
/// - the list of what is visible, everything (no visibility data: the game copies it in the level's
///   vis-bits every frame; without it, with the PC setting "PS2 actor vis", on by default, none of
///   its actors is born)
/// - `translucent`: a drawable-tree-tfrag-trans like its tfrag tree, through which the game draws
///   the translucent surfaces the builder put in the level's .fr3 (glass, water, stains)
/// - `nav`: its nav meshes, the blob of `navmesh_blob` (words, pointers, types, symbols, roots)
///   copied into the level's heap and linked where it lands
pub(super) fn level_login_code(name: &str, translucent: bool, nav: Option<&Value>) -> String {
  let mut code = String::new();
  // where a blob lands: 16-aligned, as it was laid out
  let aligned = |bytes: &str| {
    format!(
      "(the-as pointer (logand -16 (+ (the-as int (malloc 'loading-level (+ {bytes} 16))) 15)))"
    )
  };
  if let Some(nav) = nav {
    let words: Vec<String> = nav["words"]
      .as_array()
      .into_iter()
      .flatten()
      .map(|w| format!("#x{:x}", w.as_u64().unwrap_or(0)))
      .collect();
    let mut links: Vec<String> = Vec::new();
    for p in nav["pointers"].as_array().into_iter().flatten() {
      links.push(p[0].as_u64().unwrap_or(0).to_string());
      links.push(p[1].as_u64().unwrap_or(0).to_string());
    }
    code += &format!(
      ";; its nav meshes, laid out as the game's (entity-nav-mesh...), linked by {name}-login\n\
       (define *{name}-nav-words* (new 'static 'array uint32 {} {}))\n\
       (define *{name}-nav-links* (new 'static 'array int32 {} {}))\n\n",
      words.len(),
      words.join(" "),
      links.len(),
      if links.is_empty() {
        "0".into()
      } else {
        links.join(" ")
      },
    );
  }
  code += &format!(
    ";; What OpenGOAL's level builder does not write in the level, set when the game births it (the
;; slot 33 of its definition), each part only when the level lacks it
(defun {name}-login ((lev level))
  (let ((bsp (-> lev bsp)))
    ;; everything visible: the game copies this list in the level's vis-bits every frame (with the
    ;; PC setting \"PS2 actor vis\", an actor not in it is never born)
    (when (zero? (-> bsp visible-list-length))
      (let ((vis {}))
        (mem-set32! vis 512 -1)
        (set! (-> bsp all-visible-list) (the-as (pointer uint8) vis))
        (set! (-> bsp visible-list-length) 2048)
        )
      )
",
    aligned("2048")
  );
  if translucent {
    code += "    ;; the translucent surfaces: drawn through a tree of their own, like the level's tfrag tree
    (let ((trees (-> bsp drawable-trees))
          (tfrag (the-as drawable-tree-tfrag #f))
          (trans? #f)
          )
      (dotimes (i (-> trees length))
        (let ((tree (-> trees trees i)))
          (cond
            ((= (-> tree type) drawable-tree-tfrag) (set! tfrag (the-as drawable-tree-tfrag tree)))
            ((= (-> tree type) drawable-tree-tfrag-trans) (set! trans? #t))
            )
          )
        )
      (when (and tfrag (not trans?))
        (let ((trans (new 'loading-level 'drawable-tree-tfrag-trans (-> tfrag length)))
              (all (new 'loading-level 'drawable-tree-array (+ (-> trees length) 1)))
              )
          (set! (-> trans id) (-> tfrag id))
          (set! (-> trans bsphere quad) (-> tfrag bsphere quad))
          (set! (-> trans time-of-day-pal) (-> tfrag time-of-day-pal))
          (dotimes (i (-> tfrag length))
            (set! (-> trans arrays i) (-> tfrag arrays i))
            )
          (set! (-> all id) (-> trees id))
          (set! (-> all bsphere quad) (-> trees bsphere quad))
          (dotimes (i (-> trees length))
            (set! (-> all trees i) (-> trees trees i))
            )
          (set! (-> all trees (-> trees length)) trans)
          (set! (-> bsp drawable-trees) all)
          )
        )
      )
";
  }
  if let Some(nav) = nav {
    let at = |v: &Value| v.as_u64().unwrap_or(0) * 4;
    let mut links = String::new();
    for t in nav["types"].as_array().into_iter().flatten() {
      links += &format!(
        "        (set! (-> (the-as (pointer uint32) (&+ base {}))) (the-as uint {}))\n",
        at(&t[0]),
        t[1].as_str().unwrap_or("basic")
      );
    }
    for s in nav["symbols"].as_array().into_iter().flatten() {
      let sym = s[1].as_str().unwrap_or("#f");
      let value = if sym.starts_with('#') {
        sym.to_string()
      } else {
        format!("'{sym}")
      };
      links += &format!(
        "        (set! (-> (the-as (pointer symbol) (&+ base {}))) {value})\n",
        at(&s[0])
      );
    }
    for l in nav["empty_lists"].as_array().into_iter().flatten() {
      let w = if l.is_array() { &l[0] } else { l };
      links += &format!(
        "        (set! (-> (the-as (pointer pair) (&+ base {}))) '())\n",
        at(w)
      );
    }
    let mut roots: Vec<(u64, u64)> = nav["roots"]
      .as_object()
      .into_iter()
      .flatten()
      .filter_map(|(k, v)| Some((k.strip_prefix("nav-mesh-")?.parse().ok()?, v.as_u64()?)))
      .collect();
    roots.sort();
    let mut set_roots = String::new();
    for (i, (_, byte)) in roots.iter().enumerate() {
      set_roots +=
        &format!("          (set! (-> meshes {i}) (the-as entity-nav-mesh (&+ base {byte})))\n");
    }
    let words = nav["words"].as_array().map_or(0, Vec::len);
    let pairs = nav["pointers"].as_array().map_or(0, Vec::len);
    code += &format!(
      "    ;; its nav meshes: the blob copied into the level's heap, linked where it landed; the game
    ;; births them right after this
    (when (or (zero? (-> bsp nav-meshes)) (zero? (-> bsp nav-meshes length)))
      (let* ((size {})
             (base {})
             (links (the-as (pointer int32) *{name}-nav-links*))
             )
        (mem-copy! base (the-as pointer *{name}-nav-words*) size)
        (dotimes (i {pairs})
          (set! (-> (the-as (pointer uint32) (&+ base (* 4 (-> links (* 2 i))))))
                (the-as uint (&+ base (-> links (+ (* 2 i) 1))))
                )
          )
{links}        (let ((meshes (the-as (array entity-nav-mesh) (new 'loading-level 'boxed-array entity-nav-mesh {}))))
{set_roots}          (set! (-> bsp nav-meshes) meshes)
          )
        )
      )
",
      words * 4,
      aligned("size"),
      roots.len()
    );
  }
  code += "    )
  (none)
  )
";
  code
}

/// A collision surface as the level builder takes it, and what it does not keep of it ("hdwood",
/// "rail", "halfpipe", "nopilot"...: "" when nothing).
pub fn builder_surface(pat: u32) -> (BuilderSurface, String) {
  let mut lost = String::new();
  let mut s = BuilderSurface {
    noentity: pat & 1 != 0,
    nocamera: (pat >> 1) & 1 != 0,
    noedge: (pat >> 2) & 1 != 0,
    ..Default::default()
  };
  // goal_src/jak3/engine/collide/pat-h.gc: the builder takes Jak 1's fields and converts them,
  // Jak 1's stone (0) becoming Jak 3's stone (23); what Jak 1 does not have becomes the nearest
  let (mode, material, event) = (
    ((pat >> 7) & 7) as i32,
    ((pat >> 10) & 63) as usize,
    ((pat >> 18) & 63) as usize,
  );
  s.nolineofsight = (pat >> 16) & 1 != 0;
  s.mode = mode;
  if mode > 2 {
    s.mode = 0; // the halfpipes of the boards: ground
    lost = "halfpipe".into();
  }
  const K_NEAREST: [i32; 34] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
    0,  // stone
    16, // crmetal: metal
    17, // carpet: straw
    16, // grmetal: metal
    16, // shmetal: metal
    6,  // hdwood: wood
    19, // squish: swamp
    7,  // mhshroom: grass
    7,  // forest: grass
    19, // mhswamp: swamp
    16, // dmaker: metal
  ];
  s.material = K_NEAREST.get(material).copied().unwrap_or(0);
  // the level builder's collision fields are written in Jak 1's own layout (the builder converts
  // them: goalc/build_level/collide/common); a surface without a material (none) becomes stone
  let kept = material == 23 || (1..=22).contains(&material);
  if !kept {
    lost = material_name(material);
  }
  s.event = if event <= 6 { event as i32 } else { 0 };
  if event > 6 {
    lost = event_name(event);
  }
  // the flags Jak 1's fields do not have (boards, vehicles): the builder sets none of them.
  // nogrind is on nearly every surface (the board grinds rails only), and off on every surface
  // of the levels it builds: not said
  if lost.is_empty() {
    const K_FLAGS: [(u32, &str); 8] = [
      (4, "nojak"),
      (5, "noboard"),
      (6, "nopilot"),
      (24, "probe"),
      (25, "nomech"),
      (26, "noproj"),
      (27, "noendlessfall"),
      (28, "noprobe"),
    ];
    for (bit, name) in K_FLAGS {
      if (pat >> bit) & 1 != 0 {
        if !lost.is_empty() {
          lost.push('+');
        }
        lost += name;
      }
    }
  }
  (s, lost)
}

/// A res lump value of an extracted actor as the builder reads it: typed arrays (["float", ...]),
/// strings and 'symbols. `ty`: the extractor's type of the tag (lump_types), guessed when "".
/// Null for data the builder cannot write.
pub fn builder_lump_value(value: &Value, ty: &str) -> Value {
  let ty = if ty.is_empty() { guess_type(value) } else { ty };
  let strings = |tag: &str, strip: bool| -> Value {
    let one = |s: &str| Value::from(if strip { strip_quote(s) } else { s });
    let mut out = vec![Value::from(tag)];
    match value {
      Value::String(s) => out.push(one(s)),
      Value::Array(a) => out.extend(a.iter().filter_map(Value::as_str).map(one)),
      _ => {}
    }
    if out.len() > 1 {
      Value::Array(out)
    } else {
      Value::Null
    }
  };
  match ty {
    "string" => {
      // a plain string is one; one beginning with a quote would be read as a symbol
      if value.as_str().is_some_and(|s| !s.starts_with('\'')) {
        return value.clone();
      }
      strings("string", false)
    }
    "symbol" => match value.as_str() {
      Some(s) => format!("'{}", strip_quote(s)).into(),
      None => strings("symbol", true),
    },
    "type" => strings("type", true),
    "pair" => {
      if value.is_string() {
        json!(["pair", value])
      } else {
        Value::Null
      }
    }
    "float" | "int32" | "int16" | "int8" | "uint32" | "uint16" | "uint8" => {
      let mut ns = Vec::new();
      numbers(value, &mut ns);
      if ns.is_empty() {
        return Value::Null;
      }
      let (is_float, is_signed) = (ty == "float", ty.starts_with('i'));
      let mut out = vec![Value::from(if is_float {
        "float"
      } else if is_signed {
        "int32"
      } else {
        "uint32"
      })];
      for n in ns {
        let d = n.as_f64().unwrap_or(0.0);
        out.push(if is_float {
          d.into()
        } else if is_signed {
          (d.round() as i64 as i32).into()
        } else {
          (d.round() as i64 as u32).into()
        });
      }
      Value::Array(out)
    }
    "vector" => {
      let Some(a) = value.as_array().filter(|a| !a.is_empty()) else {
        return Value::Null;
      };
      let vec4 = |v: &[Value]| -> Value {
        (0..4)
          .map(|i| match v.get(i).and_then(Value::as_f64) {
            Some(x) => x,
            None if i == 3 => 1.0,
            None => 0.0,
          })
          .collect()
      };
      let mut out = vec![Value::from("vector")];
      if a[0].is_array() {
        out.extend(a.iter().filter_map(Value::as_array).map(|v| vec4(v)));
      } else {
        out.push(vec4(a));
      }
      Value::Array(out)
    }
    _ => Value::Null,
  }
}
