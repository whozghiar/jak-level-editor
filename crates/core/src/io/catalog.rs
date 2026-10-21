//! The catalog of what can be put in a level:
//!
//!   objects  the objects of the game that make up a place: its props (a flag, a windmill, a
//!            sprinkler, a parked hellcat), its doors and airlocks, its water, dark eco and lava,
//!            the particle effects of its levels (one per particle group: lights, neon signs,
//!            steam); and apart, the interactive objects (crates, platforms, elevators, bouncers,
//!            buttons, breakables, hazards...). Each with the model its code draws (one entry per
//!            model for the types that choose theirs by name) and the data of an actor of its type
//!            found in the levels. Never the living (enemies, characters, animals: creatures), the
//!            collectibles (orbs, gems, eco, ammo...) nor the technical types (shots, vehicles,
//!            controllers, the logic of the levels): tools/object_categories.json
//!   models   every other model of the game but the characters: parts of objects, debris, props,
//!            placed as decor
//!   parts    the decor of a level, to reuse in another: the prototypes of its decor (a pillar, a
//!            lamp post) and the pieces of its shell (a wall, a bridge)

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};

use crate::io::asset_library::{AssetLibrary, ModelAsset};
use crate::io::game_data::{ActorRole, GameData, game_data_path};
use crate::io::library_index::LibraryIndex;
use crate::math::{Vec3, cross, length};
use crate::scene::{Mesh, Node, NodeKind, Scene, ground_piece, instance_tree};

#[derive(Clone, Debug, Default, Serialize)]
pub struct CatalogObject {
  pub etype: String,
  /// shown: the type, or the model of a variant (look, art-name)
  pub name: String,
  /// prop (an object of the game that only stands there), door, water, effect: the Objects of the
  /// catalog; else a family of the objects that do something (crate, platform, movement, button,
  /// breakable, hazard...: GameData::object_family), the Interactive objects
  pub family: String,
  pub interactive: bool,
  /// the model drawn for it, "" for an object the game draws without one
  pub model: String,
  /// its file
  pub model_path: String,
  /// the data of a new one: an actor of the game's, without what binds it to its level
  pub lump: Value,
  /// the type of each tag of the data of its type's actors (export for the game)
  pub lump_types: Value,
  /// actors of this type in the extracted levels
  pub count: i32,
  /// the first levels they are in
  pub levels: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogModel {
  /// without "-lodN"
  pub name: String,
  /// its lowest lod
  pub path: String,
  /// the level folder it comes from ("common" for the shared ones)
  pub group: String,
  /// "" Jak 3's, "jak2" Jak II's (placed as decor: io/jak2_import)
  pub game: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PartKind {
  #[default]
  Prototype,
  Piece,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogPart {
  pub kind: PartKind,
  /// the prototype ("ruin-top-tower.mb") or the piece ("ruins-canal-top-2")
  pub name: String,
  /// the node whose mesh it is: the first instance of a prototype, the piece
  pub mesh_node: String,
  pub triangles: usize,
  /// instances of a prototype in its level, copies of a piece
  pub copies: i32,
  /// a piece: its name without its number and its triangles (piece_key), its surface and the
  /// length of its triangles' edges: its copies, here and in other levels, alike within 5 %
  /// (same_piece)
  pub key: String,
  pub surface: f64,
  pub edges: f64,
}

/// the order of the families in the catalog: the Objects, then the Interactive ones
const K_FAMILIES: &[&str] = &[
  "prop",
  "door",
  "water",
  "effect",
  "crate",
  "platform",
  "movement",
  "button",
  "hazard",
  "breakable",
  "light",
  "weapon",
  "door-like",
  "other",
];

fn family_rank(f: &str) -> usize {
  K_FAMILIES
    .iter()
    .position(|&k| k == f)
    .unwrap_or(K_FAMILIES.len())
}

/// the pieces an object leaves when it breaks: not a model of its own
fn debris(model: &str) -> bool {
  if ["-explode", "-debris", "-chunk"]
    .iter()
    .any(|w| model.contains(w))
  {
    return true;
  }
  model
    .rfind("-break")
    .is_some_and(|b| b + 6 == model.len() || model.as_bytes()[b + 6] == b'-')
}

/// a variant of a type that chooses its model by name: its model (once), its art-name
fn add_variant<'a>(
  library: &'a AssetLibrary,
  variants: &mut Vec<(&'a ModelAsset, String)>,
  model: &str,
  art: String,
) {
  if model.is_empty() || debris(model) {
    return;
  }
  let Some(m) = library.model_named(model, "") else {
    return;
  };
  if variants.iter().all(|v| v.0.name != m.name) {
    variants.push((m, art));
  }
}

/// The model the game draws for an actor, as far as the editor can tell: the model of its look for
/// a water actor, the skeleton its art-name names, the first model its type loads, else a model
/// named like its type. From the folder `group` first; None when none.
pub fn resolve_actor_model<'a>(
  library: &'a AssetLibrary,
  data: Option<&GameData>,
  etype: &str,
  lump: &Value,
  group: &str,
) -> Option<&'a ModelAsset> {
  let named = |n: &str| {
    if n.is_empty() {
      None
    } else {
      library.model_named(n, group)
    }
  };
  if lump.is_object() {
    if let Some(d) = data
      && d.is_water(etype)
      && let Some(look) = lump.get("look").and_then(Value::as_f64)
      && let Some(m) = named(d.water_model(look as i32))
    {
      return Some(m);
    }
    // "skel-" + its art-name
    if let Some(name) = lump.get("art-name").and_then(Value::as_str) {
      if let Some(m) = data.and_then(|d| named(&d.skeleton_model(name))) {
        return Some(m);
      }
      if let Some(m) = named(name) {
        return Some(m);
      }
    }
  }
  if let Some(models) = data.and_then(|d| d.type_models(etype))
    && let Some(m) = models.iter().find_map(|n| named(n))
  {
    return Some(m);
  }
  library.model_for_etype(etype, group)
}

/// The data of an actor, without what ties it to its level: its name, story (kill mask),
/// visibility volume, paths and the actors it refers to.
pub fn portable_lump(lump: &Value) -> Value {
  let mut out = serde_json::Map::new();
  for (key, value) in lump.as_object().into_iter().flatten() {
    if matches!(key.as_str(), "name" | "kill-mask" | "visvol" | "task")
      || key.contains("actor")
      || key.contains("path")
      || key.contains("group")
    {
      continue;
    }
    out.insert(key.clone(), value.clone());
  }
  Value::Object(out)
}

/// The kind of a particle effect of the catalog (family "effect"), from the names of its
/// particles: "fire" (fire, flames, embers, torches, lava), "smoke" (smoke, steam, dust, mist,
/// heat haze), "sparks" (sparks, electricity, lasers), "water" (drips, splashes, waterfalls,
/// bubbles), "light" (lights, glows, neon signs, lamps), else "other" (explosions, trails...): its
/// category among the catalog's Effects.
pub fn effect_kind(o: &CatalogObject) -> &'static str {
  let art = o.lump.get("art-name").and_then(Value::as_str).unwrap_or("");
  let n = format!("{} {art}", o.name);
  let any = |words: &[&str]| words.iter().any(|w| n.contains(w));
  if any(&["fire", "flame", "ember", "torch", "burn", "lava", "brazier"]) {
    "fire"
  } else if any(&[
    "smoke", "steam", "dust", "mist", "haze", "fog", "puff", "vapor", "cloud", "exhaust",
  ]) {
    "smoke"
  } else if any(&["spark", "elec", "zap", "lightning", "laser"]) {
    "sparks"
  } else if any(&[
    "water", "drip", "splash", "fall", "bubble", "runoff", "rain", "foam", "spray", "goo",
  ]) {
    "water"
  } else if any(&[
    "light", "glow", "neon", "sign", "lamp", "beam", "flash", "shine", "pulsat",
  ]) {
    "light"
  } else {
    "other"
  }
}

/// A new actor of a catalog object (Actor node, its model added by the caller).
pub fn make_catalog_actor(object: &CatalogObject, name: &str) -> Node {
  let mut lump = if object.lump.is_object() {
    object.lump.clone()
  } else {
    json!({})
  };
  lump["name"] = json!(name);
  let mut extras = json!({"etype": object.etype, "game_task": 0, "lump": lump});
  if object.lump_types.as_object().is_some_and(|t| !t.is_empty()) {
    extras["lump_types"] = object.lump_types.clone();
  }
  Node {
    kind: NodeKind::Actor,
    name: name.to_string(),
    extras,
    ..Default::default()
  }
}

// the actors of the extracted levels, by type
#[derive(Default)]
struct Art {
  /// known from the index
  actors: i32,
  /// the first level with one
  level: String,
  /// the data of the first one (from the index)
  lump: Value,
}

#[derive(Default)]
struct Seen {
  count: i32,
  lump: Value,
  lump_types: Value,
  levels: Vec<String>,
  /// the art-names of its actors: the models, the particle groups they choose
  arts: BTreeMap<String, Art>,
  /// some of its actors choose none
  plain: bool,
}

/// Every object of the game, by family then name. With the index of the library (once ready), the
/// art-names of every actor of the levels: all the variants and particle effects, and their
/// counts.
pub fn build_catalog(
  library: &AssetLibrary,
  data: Option<&GameData>,
  index: Option<&LibraryIndex>,
) -> Vec<CatalogObject> {
  let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
  for level in library.levels() {
    let Some(types) = level.actor_types.as_object() else {
      continue;
    };
    for (etype, info) in types {
      let s = seen.entry(etype.clone()).or_default();
      s.count += info.get("count").and_then(Value::as_f64).unwrap_or(0.0) as i32;
      if s.lump.is_null()
        && let Some(l) = info.get("lump")
      {
        s.lump = l.clone();
      }
      if let Some(t) = info.get("lump_types").and_then(Value::as_object) {
        for (tag, ty) in t {
          s.lump_types[tag] = ty.clone();
        }
      }
      if s.levels.len() < 8 {
        s.levels.push(level.name.clone());
      }
      let note = |s: &mut Seen, art: &Value| {
        if let Some(art) = art.as_str() {
          let a = s.arts.entry(art.to_string()).or_default();
          if a.level.is_empty() {
            a.level = level.name.clone();
          }
        }
      };
      match info.get("lump").filter(|l| l.is_object()) {
        Some(lump) if lump.get("art-name").is_some() => note(s, &lump["art-name"]),
        _ => s.plain = true,
      }
      for a in info
        .get("art_names")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
      {
        note(s, a);
      }
    }
  }
  // every actor of every level (the files of the levels hold them all, level.json the first of
  // each type): how many actors have each art-name, and whether some have none
  let indexed = index.is_some_and(LibraryIndex::ready);
  if let Some(index) = index.filter(|_| indexed) {
    let mut with_art: HashMap<&str, i32> = HashMap::new();
    for li in index.levels() {
      for (etype, names) in &li.art_names {
        let s = seen.entry(etype.clone()).or_default();
        for (name, &n) in names {
          let a = s.arts.entry(name.clone()).or_default();
          if a.level.is_empty() {
            a.level = li.level.clone();
          }
          if a.lump.is_null()
            && let Some(l) = li.art_lumps.get(etype).and_then(|l| l.get(name))
          {
            a.lump = l.clone();
          }
          a.actors += n;
          *with_art.entry(etype).or_default() += n;
        }
      }
    }
    for (etype, s) in &mut seen {
      s.plain = with_art.get(etype.as_str()).copied().unwrap_or(0) < s.count;
    }
  }
  let mut types: BTreeSet<&str> = BTreeSet::new();
  if let Some(d) = data {
    types.extend(d.objects().keys().map(String::as_str));
  }
  for ty in seen.keys() {
    if data.is_none_or(|d| d.role(ty) == ActorRole::Object) {
      types.insert(ty);
    }
  }

  let mut out = Vec::new();
  let add_seen = |o: &mut CatalogObject| {
    let Some(s) = seen.get(&o.etype) else { return };
    o.count = s.count;
    o.levels = s.levels.clone();
    o.lump_types = s.lump_types.clone();
    if o.lump.as_object().is_none_or(|l| l.is_empty()) {
      o.lump = portable_lump(&s.lump);
    }
  };
  // what an art-name of a type gives: its actors, its level, its data
  let add_art = |o: &mut CatalogObject, s: &Seen, name: &str| {
    if let Some(a) = s.arts.get(name) {
      if indexed {
        o.count = a.actors;
      }
      if !a.level.is_empty() {
        o.levels = vec![a.level.clone()];
      }
      if a.lump.is_object() {
        o.lump = portable_lump(&a.lump);
      }
    }
    o.lump["art-name"] = json!(name);
  };
  for &ty in &types {
    if data.is_some_and(|d| d.is_water(ty)) {
      continue; // listed by look below
    }
    // the catalog: the props and doors (Objects), the gameplay objects apart (Interactive); not a
    // collectible, nothing technical (shots, vehicles, controllers)
    let category = data.map(|d| d.catalog_category(ty)).unwrap_or_default();
    if category == "collectible" || category == "technical" {
      continue;
    }
    let interactive = category != "static" && category != "door";
    let mut family = match category.as_str() {
      "static" => "prop".to_string(),
      "door" => "door".to_string(),
      _ => data.map_or("other".to_string(), |d| d.object_family(ty)),
    };
    if interactive && family == "door" {
      family = "door-like".into(); // gates and switches that are not doors
    }
    let s = seen.get(ty);
    let mut lump = s.map_or_else(|| json!({}), |s| s.lump.clone());
    let models = data.and_then(|d| d.type_models(ty));
    let known = data.is_some_and(|d| d.objects().contains_key(ty)) || models.is_some();
    // the types that choose their model by name (art-name): one entry per model, and the one of
    // the actors that choose none
    let mut keep_art = false;
    if let Some(s) = s.filter(|s| !s.arts.is_empty()) {
      // model, art-name
      let mut variants: Vec<(&ModelAsset, String)> = Vec::new();
      for art in s.arts.keys() {
        let skeleton = data.map(|d| d.skeleton_model(art)).unwrap_or_default();
        let model = if skeleton.is_empty() { art } else { &skeleton };
        add_variant(library, &mut variants, model, art.clone());
      }
      if let (Some(models), Some(d), false) = (models, data, variants.is_empty()) {
        for name in models {
          let skeleton = d.skeleton_of_model(name);
          let art = if skeleton.is_empty() {
            name.clone()
          } else {
            skeleton
          };
          add_variant(library, &mut variants, name, art);
        }
      }
      for (m, art) in &variants {
        let mut o = CatalogObject {
          etype: ty.to_string(),
          name: m.name.clone(),
          family: family.clone(),
          interactive,
          model: m.name.clone(),
          model_path: m.path.clone(),
          ..Default::default()
        };
        add_seen(&mut o);
        add_art(&mut o, s, art);
        out.push(o);
      }
      if !variants.is_empty() && !s.plain {
        continue;
      }
      // no art-name names a model (it names something else: a particle group, the actor): one
      // entry that keeps it; else the entry of the actors without one
      if variants.is_empty() {
        keep_art = true;
      } else if let Some(l) = lump.as_object_mut() {
        l.remove("art-name");
      }
    }
    let m = resolve_actor_model(library, data, ty, &lump, "");
    // an object the game draws without a model (trick points, fire, a manager of the level's
    // objects...): one the levels have
    if m.is_none() && (!known || s.is_none()) {
      continue;
    }
    let mut o = CatalogObject {
      etype: ty.to_string(),
      name: ty.to_string(),
      family,
      interactive,
      lump: json!({}),
      ..Default::default()
    };
    if let Some(m) = m {
      o.model = m.name.clone();
      o.model_path = m.path.clone();
    }
    add_seen(&mut o);
    if !keep_art && let Some(l) = o.lump.as_object_mut() {
      l.remove("art-name");
    }
    out.push(o);
  }
  if let Some(d) = data {
    // water, dark eco and lava: one per look, each with its model and the type of the actors
    // that have it (the water of a level adds its ripples and color)
    let dark_eco = if d.is_water("dark-eco-pool") {
      "dark-eco-pool"
    } else {
      "water-anim"
    };
    let water_types = d.water_types();
    let water_type = |model: &str| -> String {
      if model.contains("dark-eco") {
        return dark_eco.to_string();
      }
      let mut best = "water-anim";
      for t in &water_types {
        if t.len() > best.len()
          && model.len() > t.len()
          && model.starts_with(t.as_str())
          && model.as_bytes()[t.len()] == b'-'
        {
          best = t;
        }
      }
      best.to_string()
    };
    for look in 0..d.water_look_count() {
      let model = d.water_model(look as i32);
      let Some(m) = (!model.is_empty())
        .then(|| library.model_named(model, ""))
        .flatten()
      else {
        continue;
      };
      let mut o = CatalogObject {
        etype: water_type(model),
        name: model.to_string(),
        family: "water".into(),
        model: m.name.clone(),
        model_path: m.path.clone(),
        lump: json!({"look": look}),
        ..Default::default()
      };
      add_seen(&mut o);
      out.push(o);
    }
    // the particle effects (lights, neon signs, steam, sparks...): one per particle group the
    // actors of the levels name, with the type of the actors that use it most
    let mut groups: BTreeMap<&str, (&str, i32)> = BTreeMap::new(); // etype, actors
    for (ty, s) in &seen {
      if !d.is_effect(ty) || d.is_unlisted(ty) {
        continue;
      }
      for (name, a) in &s.arts {
        let g = groups.entry(name.as_str()).or_insert(("", -1));
        let n = if indexed { a.actors } else { s.count };
        if n > g.1 {
          *g = (ty.as_str(), n);
        }
      }
    }
    for (name, (ty, _)) in groups {
      let mut o = CatalogObject {
        etype: ty.to_string(),
        name: name.strip_prefix("group-").unwrap_or(name).to_string(),
        family: "effect".into(),
        ..Default::default()
      };
      add_seen(&mut o);
      add_art(&mut o, &seen[ty], name);
      out.push(o);
    }
    // the other logic of the levels (the boats of a port, crowds, spawners of creatures) is not
    // offered: it is no part of a place's architecture
  }
  out.sort_by(|a, b| (family_rank(&a.family), &a.name).cmp(&(family_rank(&b.family), &b.name)));
  out
}

/// The models of the library that are not the model of an object of `objects` nor a character's,
/// by name; `creatures`: more types of living things whose models are left out (Jak II's, which
/// has no game data: data/jak2/creatures.json, jak2_creatures()).
pub fn list_catalog_models(
  library: &AssetLibrary,
  data: Option<&GameData>,
  objects: &[CatalogObject],
  creatures: &BTreeSet<String>,
) -> Vec<CatalogModel> {
  let taken: BTreeSet<&str> = objects
    .iter()
    .filter(|o| !o.model.is_empty())
    .map(|o| o.model.as_str())
    .collect();
  // the characters: the models of the creatures, the ones named after a creature (its parts: a
  // boss's pieces) or after an NPC's name, the ones of the cutscenes
  let mut characters: BTreeSet<&str> = BTreeSet::new();
  let mut creature_names = creatures.clone();
  if let Some(d) = data {
    // and the models of what the catalog leaves out: the collectibles (orbs, gems...), the
    // technical types (shots, vehicles, the player's gun and board)
    for (ty, models) in d.all_type_models() {
      let category = d.catalog_category(ty);
      if d.role(ty) == ActorRole::Creature || category == "collectible" || category == "technical" {
        characters.extend(models.iter().map(String::as_str));
      }
    }
    for (ty, &role) in d.roles() {
      if role == ActorRole::Creature {
        creature_names.insert(ty.clone());
      }
    }
  }
  for ty in creature_names.clone() {
    for suffix in ["-npc", "-highres"] {
      if let Some(stem) = ty.strip_suffix(suffix) {
        creature_names.insert(stem.to_string());
      }
    }
  }
  let named_after_creature = |model: &str| {
    let mut end = Some(model.len());
    while let Some(e) = end {
      if creature_names.contains(&model[..e]) {
        return true;
      }
      if e == 0 {
        break;
      }
      end = model[..e].rfind('-');
    }
    false
  };
  let mut out: Vec<CatalogModel> = Vec::new();
  for m in library.models() {
    if out.last().is_some_and(|l| l.name == m.name) {
      continue; // its other lods and copies
    }
    let n = m.name.as_str();
    // the player's own (Jak, Daxter, the board, the gun, the HUD: the models every level shares,
    // and Jak's elsewhere) are no prop either
    let player = m.group == "common" || n.starts_with("jak") || n.starts_with("daxter");
    // the museums show the game's characters and props again, out of their levels
    let museum = m.group.starts_with("museum");
    if taken.contains(n)
      || characters.contains(n)
      || named_after_creature(n)
      || player
      || museum
      || n.starts_with("collectables-")
      || n.contains("-highres")
      || n.ends_with("-lowres")
      || n.ends_with("-fma")
      || n.ends_with("-shadow")
      || n.contains("-collision")
      || n == "scenecamera"
    {
      continue;
    }
    // the first copy is the lowest lod
    out.push(CatalogModel {
      name: m.name.clone(),
      path: m.path.clone(),
      group: m.group.clone(),
      game: String::new(),
    });
  }
  out
}

/// The actor types of Jak II that are living things (data/jak2/creatures.json, from its type
/// table): list_catalog_models leaves their models out of Jak II's.
pub fn jak2_creatures() -> BTreeSet<String> {
  let path = game_data_path()
    .parent()
    .and_then(|p| p.parent())
    .map(|p| p.join("jak2").join("creatures.json"))
    .unwrap_or_default();
  let j: Value = std::fs::read(path)
    .ok()
    .and_then(|b| serde_json::from_slice(&b).ok())
    .unwrap_or(Value::Null);
  j.get("creatures")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|t| t.as_str().map(str::to_string))
    .collect()
}

/// What the copies of a piece of a shell share: its name without its number, its triangles
pub fn piece_key(name: &str, triangles: usize) -> String {
  let stem = match name.rfind('-') {
    Some(dash) if dash + 1 < name.len() && name[dash + 1..].bytes().all(|b| b.is_ascii_digit()) => {
      &name[..dash]
    }
    _ => name,
  };
  format!("{stem}#{triangles}")
}

/// Copies of one piece: the same key, their surface and their edges within 5 % of each other (the
/// same however turned or placed; the variants the artists bent a little, the leaves of a palm,
/// too)
pub fn same_piece(a: &CatalogPart, b: &CatalogPart) -> bool {
  let close = |x: f64, y: f64| (x - y).abs() <= 0.05 * x.max(y).max(1e-6);
  a.key == b.key && close(a.surface, b.surface) && close(a.edges, b.edges)
}

/// `part` added to `pieces` unless a copy of it is there (then counted in its copies); `by_key`:
/// the pieces of each key
pub fn add_piece(
  pieces: &mut Vec<CatalogPart>,
  by_key: &mut BTreeMap<String, Vec<usize>>,
  part: CatalogPart,
) {
  let alike = by_key.entry(part.key.clone()).or_default();
  if let Some(&i) = alike.iter().find(|&&i| same_piece(&pieces[i], &part)) {
    pieces[i].copies += part.copies;
    return;
  }
  alike.push(pieces.len());
  pieces.push(part);
}

/// A mesh's surface and the length of its triangles' edges
pub fn mesh_measure(mesh: &Mesh) -> (f64, f64) {
  let (mut surface, mut edges) = (0.0f64, 0.0f64);
  for p in &mesh.prims {
    for t in p.idx.as_chunks::<3>().0 {
      let (a, b, c) = (
        p.pos[t[0] as usize],
        p.pos[t[1] as usize],
        p.pos[t[2] as usize],
      );
      surface += 0.5 * length(cross(b - a, c - a)) as f64;
      edges += (length(b - a) + length(c - b) + length(a - c)) as f64;
    }
  }
  (surface, edges)
}

/// The decor parts of a level as the library has it (a level's background file, not prepared for
/// editing): its prototypes and the pieces of its shell that are not terrain, the copies of a
/// piece once (the game repeats a window, a bolt, a trim: the same name, triangles and measures;
/// `copies` counts them).
pub fn list_parts(level: &Scene) -> Vec<CatalogPart> {
  let (mut prototypes, mut pieces) = (Vec::new(), Vec::new());
  // its copies once
  let mut piece_of: BTreeMap<String, Vec<usize>> = BTreeMap::new();
  let mut children: HashMap<u32, Vec<&Node>> = HashMap::new();
  for n in &level.nodes {
    if n.parent != 0 {
      children.entry(n.parent).or_default().push(n);
    }
  }
  for n in &level.nodes {
    let Some(p) = (n.parent != 0).then(|| level.find(n.parent)).flatten() else {
      continue;
    };
    if instance_tree(&p.name) {
      // a prototype: its instances have the mesh (or the node itself)
      let mut part = CatalogPart {
        kind: PartKind::Prototype,
        name: n.name.clone(),
        ..Default::default()
      };
      if let Some(mesh) = &n.mesh {
        part.mesh_node = n.name.clone();
        part.triangles = mesh.tri_count();
        part.copies = 1;
      } else {
        for c in children.get(&n.id).into_iter().flatten() {
          let Some(mesh) = &c.mesh else { continue };
          if part.mesh_node.is_empty() {
            part.mesh_node = c.name.clone();
            part.triangles = mesh.tri_count();
          }
          part.copies += 1;
        }
      }
      if !part.mesh_node.is_empty() {
        prototypes.push(part);
      }
    } else if p.name.contains("-tfrag-")
      && let Some(mesh) = &n.mesh
    {
      // a piece of the shell; the ground and the older extractions (a mesh per texture) are not
      match n.extras.get("og_terrain").and_then(Value::as_bool) {
        Some(terrain) if !(terrain && ground_piece(mesh)) => {}
        _ => continue,
      }
      let (surface, edges) = mesh_measure(mesh);
      let triangles = mesh.tri_count();
      let part = CatalogPart {
        kind: PartKind::Piece,
        name: n.name.clone(),
        mesh_node: n.name.clone(),
        triangles,
        copies: 1,
        key: piece_key(&n.name, triangles),
        surface,
        edges,
      };
      add_piece(&mut pieces, &mut piece_of, part);
    }
  }
  prototypes.sort_by(|a, b| a.name.cmp(&b.name));
  pieces.sort_by(|a, b| a.name.cmp(&b.name));
  prototypes.append(&mut pieces);
  prototypes
}

/// The mesh of a part, moved so that the bottom center of its geometry is its origin (a part sits
/// on the point it is placed at). None when the level has no such part.
pub fn part_mesh(level: &Scene, part: &CatalogPart) -> Option<Arc<Mesh>> {
  let n = level
    .nodes
    .iter()
    .find(|n| n.name == part.mesh_node && n.mesh.is_some())?;
  let mut mesh = Mesh::clone(n.mesh.as_ref()?);
  let b = mesh.bounds();
  if b.valid() {
    let origin = Vec3::new(b.center().x, b.lo.y, b.center().z);
    for prim in &mut mesh.prims {
      for p in &mut prim.pos {
        *p -= origin;
      }
    }
    mesh.touch();
  }
  Some(Arc::new(mesh))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::asset_library::game_folder;
  use crate::io::gltf::{ImportOptions, import_gltf};
  use crate::io::library_index::{LibraryIndex, all_decor, index_level};
  use crate::io::test_library::{temp_dir, write_story_library};

  /// the story library (src/app/selftest.cpp): its game data and its levels
  fn story(name: &str) -> (std::path::PathBuf, GameData, AssetLibrary) {
    let root = temp_dir(name);
    assert!(write_story_library(&root));
    let data = GameData::load(&root.join("game-data.json")).unwrap();
    let mut library = AssetLibrary::default();
    library.scan(&game_folder(root.to_str().unwrap())).unwrap();
    (root, data, library)
  }

  #[test]
  fn catalog_objects_parts_and_index() {
    let (root, data, library) = story("catalog-story");
    // the catalog: the objects with a model, the data of an actor of the game without its level's
    let objects = build_catalog(&library, Some(&data), None);
    let crate_ = objects.iter().find(|o| o.etype == "crate").unwrap();
    assert!(
      crate_.model == "crate-test"
        && crate_.family == "crate"
        && crate_.count == 1
        && crate_.lump.get("eco-info").is_some()
        && crate_.lump.get("kill-mask").is_none()
        && crate_.lump.get("next-actor").is_none()
        && crate_.lump.get("name").is_none(),
      "catalog: objects with their model, family and data (without their level's): {crate_:?}"
    );
    assert!(
      objects
        .iter()
        .any(|o| o.family == "water" && o.model == "water-anim-test-pool" && o.lump["look"] == 0),
      "catalog: water by look"
    );
    assert!(!objects.iter().any(|o| o.etype == "no-model-thing"));
    let placed = make_catalog_actor(crate_, "crate-7");
    assert!(
      placed.kind == NodeKind::Actor
        && placed.extras["etype"] == "crate"
        && placed.extras["lump"]["name"] == "crate-7",
      "catalog: a new actor of an object, named in its data"
    );

    // the decor parts of a level file: its prototypes and the pieces of its shell
    let town_entry = library.level("town").unwrap();
    let mut town_file = Scene::new();
    import_gltf(
      &mut town_file,
      &town_entry.background,
      &ImportOptions::default(),
    )
    .unwrap();
    let parts = list_parts(&town_file);
    let names: String = parts
      .iter()
      .map(|p| match p.kind {
        PartKind::Piece => format!("{}(piece) ", p.name),
        PartKind::Prototype => format!("{} ", p.name),
      })
      .collect();
    let wall = parts
      .iter()
      .filter(|p| p.kind == PartKind::Piece)
      .filter_map(|p| part_mesh(&town_file, p))
      .last()
      .unwrap();
    assert_eq!(names, "rock.mb rubble.mb town-wall(piece) ");
    assert!(
      wall.bounds().lo.y.abs() < 1e-4 && wall.bounds().center().z.abs() < 1e-4,
      "decor parts: prototypes and pieces (not the terrain), each around its base"
    );

    // the index of the library: the same parts without loading the level, the art-names of its
    // actors; the catalog with it: a particle effect per group, the decor of every level
    let town_index = index_level(&town_entry.background).unwrap();
    assert_eq!(town_index.parts.len(), parts.len());
    for (a, b) in town_index.parts.iter().zip(&parts) {
      assert!(
        a.name == b.name
          && a.mesh_node == b.mesh_node
          && a.kind == b.kind
          && a.triangles == b.triangles
          && a.copies == b.copies,
        "library index: the decor parts of a level without loading it: {a:?} {b:?}"
      );
    }
    assert_eq!(town_index.art_names["part-spawner"]["group-town-torch"], 1);
    let cache = root.join("index");
    let mut index = LibraryIndex::default();
    index.start(&library, cache.to_str().unwrap(), true);
    index.wait();
    assert!(index.ready() && index.done() == index.total() && index.total() == 4);
    let with_index = build_catalog(&library, Some(&data), Some(&index));
    let torch = with_index.iter().find(|o| o.family == "effect").unwrap();
    assert!(
      torch.name == "town-torch"
        && torch.etype == "part-spawner"
        && torch.lump["art-name"] == "group-town-torch"
        && torch.count == 1
        && effect_kind(torch) == "fire",
      "catalog: a particle effect per particle group of the levels: {torch:?}"
    );
    let every_part = all_decor(&index, &library);
    assert_eq!(
      every_part.iter().filter(|e| e.level == "town").count(),
      parts.len(),
      "the decor of every level"
    );
    // read again from its cache: the same, a file per level
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 4);
    let mut again = LibraryIndex::default();
    again.start(&library, cache.to_str().unwrap(), false);
    while !again.update() {
      std::thread::sleep(std::time::Duration::from_millis(5));
    }
    for (a, b) in again.levels().iter().zip(index.levels()) {
      assert_eq!(
        (&a.parts, &a.art_names, &a.art_lumps, &a.textures),
        (&b.parts, &b.art_names, &b.art_lumps, &b.textures)
      );
    }
  }

  #[test]
  fn models_pieces_and_names() {
    let (_root, data, library) = story("catalog-models");
    let objects = build_catalog(&library, Some(&data), None);
    // the crate's model is the object's, the pool's the water's: no prop left
    assert!(list_catalog_models(&library, Some(&data), &objects, &BTreeSet::new()).is_empty());
    let all = list_catalog_models(&library, None, &[], &BTreeSet::new());
    assert_eq!(all.len(), 2);
    let creatures = BTreeSet::from(["water-anim-npc".to_string()]);
    assert_eq!(
      list_catalog_models(&library, None, &[], &creatures)
        .iter()
        .map(|m| m.name.as_str())
        .collect::<Vec<_>>(),
      ["crate-test"],
      "a model named after a creature (its -npc stripped) left out"
    );
    // the trees of instanced decor by their names: not a prototype whose name says tie or shrub
    assert!(
      instance_tree("ctyfarma-tie-0")
        && instance_tree("atoll-shrub-12")
        && !instance_tree("atoll-bigvents-tie-01.mb")
        && !instance_tree("des-stone-shrub-01.mb")
        && !instance_tree("ctyfarma-tie-0-wind")
        && !instance_tree("tie-0")
    );
    assert_eq!(piece_key("ruins-canal-top-2", 12), "ruins-canal-top#12");
    assert_eq!(piece_key("wall-", 3), "wall-#3");
    assert_eq!(piece_key("wall", 3), "wall#3");
    assert!(debris("barrel-break") && debris("barrel-break-a") && !debris("barrel-breaker"));
    let mut lump = json!({"name": "x", "kill-mask": 1, "next-actor": "y", "path": [], "alt-actor": 1,
                          "trans-group": 1, "eco-info": [1, 2]});
    lump = portable_lump(&lump);
    assert_eq!(lump, json!({"eco-info": [1, 2]}));
    let a = CatalogPart {
      key: "k#2".into(),
      surface: 10.0,
      edges: 20.0,
      copies: 1,
      ..Default::default()
    };
    let (mut pieces, mut by_key) = (Vec::new(), BTreeMap::new());
    add_piece(&mut pieces, &mut by_key, a.clone());
    add_piece(
      &mut pieces,
      &mut by_key,
      CatalogPart {
        surface: 10.4,
        ..a.clone()
      },
    );
    add_piece(
      &mut pieces,
      &mut by_key,
      CatalogPart {
        surface: 11.0,
        ..a.clone()
      },
    );
    assert!(
      pieces.len() == 2 && pieces[0].copies == 2,
      "a piece's copies once, within 5 %"
    );
  }
}
