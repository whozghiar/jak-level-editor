//! What the library takes from a level's bsp-header (the last object of its DGO, as OpenGOAL's
//! extractor dumps it): its actors with the type of each of their tags (the level builder writes a
//! tag with its type), and Jak 3's regions (where the game runs a script when Jak or the camera
//! goes in or out: the levels it loads and shows around this one, which a level built in its place
//! needs). The layouts are the game's (goal_src/<game>, all-types.gc); the JSON is written as
//! ogle-extract wrote it (the decompiler's extract_actors.cpp, BspHeader.cpp).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::goal_object::{GoalObject, Word};

/// bsp-header fields
const BSP_NAME: u32 = 72;
const BSP_ACTORS: u32 = 112;
const BSP_REGION_TREES: u32 = 188;

pub struct Bsp {
  /// the level's name
  pub name: String,
  /// as <level>-actors.json, with "lump_types"
  pub actors: Vec<Value>,
  /// by tree (camera, target...): its sphere and regions; {} when none
  pub region_trees: Value,
}

/// The objects of each DGO, as `decompiler_out/<game>/dgo.txt` lists them: the last is its level
pub fn dgo_objects(dgo_txt: &str) -> BTreeMap<String, Vec<String>> {
  let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
  let mut current = None;
  for line in dgo_txt.lines() {
    let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    if line.starts_with("(\"") {
      current = quoted.first().map(|s| s.to_string());
    } else if let (Some(dgo), Some(file)) = (&current, quoted.first()) {
      let name = file
        .strip_suffix(".go")
        .or(file.strip_suffix(".o"))
        .unwrap_or(file);
      out.entry(dgo.clone()).or_default().push(name.to_string());
    }
  }
  out
}

fn floats(o: &GoalObject, at: u32, n: u32, meters: bool) -> Value {
  let scale = if meters { 4096.0 } else { 1.0 };
  json!(
    (0..n)
      .map(|i| (o.f32(at + 4 * i) / scale) as f64)
      .collect::<Vec<_>>()
  )
}

/// one value, or a list of them (strings_json, value_json)
fn one_or_list(mut v: Vec<Value>) -> Value {
  if v.len() == 1 {
    v.pop().unwrap()
  } else if v.is_empty() {
    Value::Null
  } else {
    Value::Array(v)
  }
}

pub fn read_bsp(data: &[u8], game: &str) -> Result<Bsp, String> {
  let o = GoalObject::read(data)?;
  if o.type_name(0) != Some("bsp-header") {
    return Err("not a level (bsp-header)".into());
  }
  let actors = match o.ptr(BSP_ACTORS) {
    Some(p) => read_actors(&o, p - 4)?,
    None => Vec::new(),
  };
  let region_trees = if game == "jak3" {
    read_region_trees(&o).unwrap_or_else(|| json!({}))
  } else {
    json!({})
  };
  Ok(Bsp {
    name: o.symbol(BSP_NAME).unwrap_or("").to_string(),
    actors,
    region_trees,
  })
}

/// drawable-inline-array-actor at `arr`: its length @6, its drawable-actors from @32, 32 bytes each
/// (their sphere @16, their entity-actor @8)
fn read_actors(o: &GoalObject, arr: u32) -> Result<Vec<Value>, String> {
  if o.type_name(arr) != Some("drawable-inline-array-actor") {
    return Err("bad actor array".into());
  }
  let n = o.i16(arr + 6).max(0) as u32;
  let mut out = Vec::new();
  for i in 0..n {
    let d = arr + 32 + 32 * i;
    if o.type_name(d) != Some("drawable-actor") {
      return Err(format!("bad drawable-actor {i}"));
    }
    let e = o
      .ptr(d + 8)
      .ok_or_else(|| format!("actor {i} without entity"))?
      - 4;
    out.push(read_actor(o, e, floats(o, d + 16, 4, true)));
  }
  Ok(out)
}

/// An entity-actor at `e` (its type tag): trans @32, aid @48, etype @56, task @60, quat @64; its
/// res-lump: length @4, data-base @12, tag @28 (16 bytes each: name, key-frame, elt-type, then
/// data-offset 16 bits, elt-count 15, inlined 1)
fn read_actor(o: &GoalObject, e: u32, bsphere: Value) -> Value {
  let mut lump = Map::new();
  let mut types = Map::new();
  let tags = o.ptr(e + 28).unwrap_or(0);
  let base = o.ptr(e + 12).unwrap_or(0);
  for i in 0..o.u32(e + 4).min(10_000) {
    let t = tags + 16 * i;
    let name = o.symbol(t).unwrap_or("").to_string();
    let ty = o.type_name(t + 8).unwrap_or("").to_string();
    let vals = o.u32(t + 12);
    let data = base + (vals & 0xffff);
    let count = (vals >> 16) & 0x7fff;
    if ty != "actor-group" {
      types.insert(name.clone(), json!(ty));
    }
    let values = |size: u32, f: &dyn Fn(u32) -> Value| {
      one_or_list((0..count).map(|j| f(data + size * j)).collect())
    };
    let value = match ty.as_str() {
      "string" => values(4, &|at| {
        json!(o.ptr(at).and_then(|p| o.string(p)).unwrap_or_default())
      }),
      "symbol" => values(4, &|at| json!(format!("'{}", o.symbol(at).unwrap_or("")))),
      "type" => values(4, &|at| {
        json!(format!("'{}", o.type_name(at).unwrap_or("")))
      }),
      "vector" => {
        let v: Vec<Value> = (0..count)
          .map(|j| floats(o, data + 16 * j, 4, false))
          .collect();
        if count == 1 {
          lump.insert(name, v.into_iter().next().unwrap());
        } else {
          // tags of the same name add to the list
          let list = lump.entry(name).or_insert(json!([]));
          if let Some(l) = list.as_array_mut() {
            l.extend(v);
          }
        }
        continue;
      }
      "pair" => match o.ptr(data) {
        Some(p) if p & 7 == 2 => json!(o.list(p, true)),
        _ => continue,
      },
      "float" => values(4, &|at| json!(o.f32(at) as f64)),
      "int32" => values(4, &|at| json!(o.u32(at) as i32)),
      "uint32" => values(4, &|at| json!(o.u32(at))),
      "int16" => values(2, &|at| json!(o.i16(at))),
      "uint16" => values(2, &|at| json!(o.i16(at) as u16)),
      "int8" => values(1, &|at| json!(o.bytes(at, 1)[0] as i8)),
      "uint8" => values(1, &|at| json!(o.bytes(at, 1)[0])),
      _ => continue,
    };
    lump.insert(name, value);
  }
  json!({
    "bsphere": bsphere,
    "trans": floats(o, e + 32, 4, true),
    "aid": o.u32(e + 48),
    "etype": o.type_name(e + 56).unwrap_or(""),
    "game_task": o.bytes(e + 60, 1)[0],
    "quat": floats(o, e + 64, 4, false),
    "lump": lump,
    "lump_types": types,
  })
}

/// A script's parentheses and atoms, each with the spaces before it
fn tokens(script: &str) -> Vec<(&str, &str)> {
  let mut out = Vec::new();
  let mut rest = script;
  while !rest.is_empty() {
    let start = rest.len() - rest.trim_start().len();
    let after = &rest[start..];
    let len = match after.chars().next() {
      None => break,
      Some('(' | ')') => 1,
      Some(_) => after
        .find(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .unwrap_or(after.len()),
    };
    out.push((&rest[..start], &after[..len]));
    rest = &after[len..];
  }
  out
}

/// The actors' scripts (their lists) laid out as OpenGOAL's extractor prints them, on several
/// lines (the `<level>-actors.json` it writes, its actors in the bsp's order): its line breaks
/// and indents around this reader's atoms (the extractor prints a list's integers times 8)
pub fn printed_scripts(actors: &mut [Value], printed: &Value) {
  for (a, p) in actors
    .iter_mut()
    .zip(printed.as_array().into_iter().flatten())
  {
    if a["aid"] != p["aid"] {
      continue;
    }
    for (key, v) in a["lump"].as_object_mut().into_iter().flatten() {
      let (Some(mine), Some(theirs)) = (v.as_str(), p["lump"][key].as_str()) else {
        continue;
      };
      let (m, t) = (tokens(mine), tokens(theirs));
      // the same list: the same parentheses where they are
      let paren = |x: &str| x == "(" || x == ")";
      if !mine.starts_with('(')
        || m.len() != t.len()
        || m
          .iter()
          .zip(&t)
          .any(|(x, y)| paren(x.1) != paren(y.1) || (paren(x.1) && x.1 != y.1))
      {
        continue;
      }
      let laid: String = m
        .iter()
        .zip(&t)
        .map(|(x, y)| format!("{}{}", y.0, x.1))
        .collect();
      *v = json!(laid);
    }
  }
}

/// A face of a region at `d`: its plane (the normal, its offset in meters) and its points
fn region_face(o: &GoalObject, d: u32) -> Value {
  let normal = [
    o.f32(d) as f64,
    o.f32(d + 4) as f64,
    o.f32(d + 8) as f64,
    (o.f32(d + 12) / 4096.0) as f64,
  ];
  let points: Vec<Value> = (0..o.u32(d + 16).min(1000))
    .map(|i| floats(o, d + 32 + 16 * i, 3, true))
    .collect();
  json!({"normal": normal, "points": points})
}

/// region-trees: an array (length @4, a tree per word from @16) of drawable-tree-region-prim
/// (length @6, name @8, sphere @16, its arrays from @32); the game reads the last array, its prims
/// 32 bytes each from @32: sphere @16, region @8 (id, on-enter, on-inside, on-exit), the face or
/// volume @12
fn read_region_trees(o: &GoalObject) -> Option<Value> {
  let arr = o.ptr(BSP_REGION_TREES)? - 4;
  let mut out = Map::new();
  for t in 0..o.u32(arr + 4) {
    let Some(tree) = o.ptr(arr + 16 + 4 * t).map(|p| p - 4) else {
      continue;
    };
    let len = o.i16(tree + 6) as i32;
    let Some(name) = o.symbol(tree + 8).map(str::to_string) else {
      continue;
    };
    if len < 1 {
      continue;
    }
    let leaf = o.ptr(tree + 32 + 4 * (len as u32 - 1))? - 4;
    let mut regions = Vec::new();
    for j in 0..o.i16(leaf + 6).max(0) as u32 {
      let prim = leaf + 32 + 32 * j;
      let Some(region) = o.ptr(prim + 8) else {
        continue;
      };
      let mut r = Map::new();
      r.insert("id".into(), json!(o.u32(region)));
      r.insert("bsphere".into(), floats(o, prim + 16, 4, true));
      match o.type_name(prim) {
        Some("drawable-region-face") => {
          r.insert("shape".into(), json!("face"));
          if let Some(d) = o.ptr(prim + 12) {
            r.insert("face".into(), region_face(o, d));
          }
        }
        Some("drawable-region-volume") => {
          r.insert("shape".into(), json!("volume"));
          let faces = o.ptr(prim + 12).map(|p| p - 4);
          let list: Vec<Value> = faces
            .into_iter()
            .flat_map(|f| (0..o.u32(f + 4).min(1000)).map(move |k| (f, k)))
            .filter_map(|(f, k)| o.ptr(f + 16 + 32 * k + 12).map(|d| region_face(o, d)))
            .collect();
          r.insert("volume".into(), json!({ "faces": list }));
        }
        _ => {
          r.insert("shape".into(), json!("sphere"));
        }
      }
      for (k, key) in ["on-enter", "on-inside", "on-exit"].iter().enumerate() {
        if let Some(Word::Ptr(p)) = o.word(region + 4 + 4 * k as u32)
          && p & 7 == 2
        {
          r.insert(key.to_string(), json!(o.list(*p, false)));
        }
      }
      regions.push(Value::Object(r));
    }
    out.insert(
      name,
      json!({"bsphere": floats(o, tree + 16, 4, true), "regions": regions}),
    );
  }
  Some(Value::Object(out))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn scripts_printed_on_lines() {
    let mut actors = vec![json!({"aid": 7, "lump": {
      "on-cross": "(cond ((a) (seconds 1)))", "name": "x", "on-exit": "(b)"}})];
    // the extractor's: on lines, its integers times 8
    let printed = json!([{"aid": 7, "lump": {
      "on-cross": "(cond\n  ((a)\n   (seconds 8)\n   )\n  )", "name": "y", "on-exit": "(b c)"}}]);
    printed_scripts(&mut actors, &printed);
    assert_eq!(
      actors[0]["lump"]["on-cross"],
      "(cond\n  ((a)\n   (seconds 1)\n   )\n  )"
    );
    // not a list, not the same list: kept
    assert_eq!(actors[0]["lump"]["name"], "x");
    assert_eq!(actors[0]["lump"]["on-exit"], "(b)");
  }

  #[test]
  fn dgo_listing_read() {
    let text = ";; DGO File Listing\n\n(\"CIA.DGO\"\n  (\"tpage-1.o\" \"tpage-1\")\n  (\"crate-ag.o\" \"crate\")\n  (\"ctyinda-vis.go\" \"ctyinda-vis\")\n  )\n";
    let dgos = dgo_objects(text);
    assert_eq!(dgos["CIA.DGO"], ["tpage-1", "crate-ag", "ctyinda-vis"]);
  }
}
