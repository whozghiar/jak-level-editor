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

pub(super) fn level_info(o: &LevelExportOptions, bottom: f32) -> String {
  let (s, c) = (o.start, o.start + Vec3::new(0.0, 3.0, -6.0));
  let (name, nick) = (&o.name, &o.nickname);
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
    :continues '((new 'static 'continue-point
        :name \"{name}-start\"
        :level '{name}
        :trans (static-vectorm {})
        :camera-trans (static-vectorm {})
        :quat (new 'static 'vector4h :data (new 'static 'array int16 4 0 0 0 #x7fff))
        :camera-rot (new 'static 'array int16 9 #x7fff 0 0 0 #x7fff 0 0 0 #x7fff)
        :on-goto #f
        :vis-nick '{nick}
        :vehicle-type #x1b
        :want-count 1
        :want (new 'static 'inline-array level-buffer-state-small 1
          (new 'static 'level-buffer-state-small :name '{name} :display? 'display)
          )
        :want-sound (new 'static 'array symbol 3 #f #f #f)
        )
      )
    :callback-list '()
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
    vectorm(s),
    vectorm(c),
    meters(bottom)
  )
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
