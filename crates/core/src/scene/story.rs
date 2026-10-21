//! The story view: a level as the game shows it at a point of the story. The actors, decor
//! prototypes and layers the game does not show at that point are hidden (Node::story_hidden);
//! nothing is removed, the document keeps every state.
//!
//! The levels loaded with the one opened hang under a group telling what they are:
//!   extras.og_layer      a layer: the actors (and decor) a mission adds, shown while the story
//!                        lends it to the level
//!   extras.og_companion  a level the game shows with it (the hut of Dead Town)
//!   extras.og_level      a level added to the document (a neighbor): shown as when the player is
//!                        there, with its own companions and layers under its group

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::{Node, NodeKind, Scene, instance_tree};
use crate::io::game_data::{GameData, StoryCond, StoryMoment};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryView {
  /// off: every state at once
  pub enabled: bool,
  /// the level opened's: the story nodes before it are closed (GameData)
  pub step: i32,
  /// the levels added to the document, each at its own point of the story (name -> step); the
  /// others (and the companions and layers of the level opened) at `step`
  pub level_steps: BTreeMap<String, i32>,
}

impl Default for StoryView {
  fn default() -> Self {
    Self {
      enabled: true,
      step: 1,
      level_steps: BTreeMap::new(),
    }
  }
}

impl StoryView {
  pub fn step_of(&self, level: &str) -> i32 {
    self.level_steps.get(level).copied().unwrap_or(self.step)
  }
}

// What the story decides in a document, collected once per evaluation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ItemKind {
  Actor,
  Prototype,
  Layer,
  LayerDecor,
}

struct Item<'a> {
  /// in Scene::nodes
  index: usize,
  kind: ItemKind,
  /// in Items::levels
  level: usize,
  kill_mask: u32,
  /// all must hold
  rules: Vec<&'a StoryCond>,
}

#[derive(Default)]
struct Items<'a> {
  /// [0]: the level opened
  levels: Vec<String>,
  is_layer: Vec<bool>,
  /// per level: the level the player is in (levels[0], or an added level)
  host: Vec<usize>,
  items: Vec<Item<'a>>,
}

fn extra_string<'a>(n: &'a Node, key: &str) -> &'a str {
  n.extras
    .get(key)
    .and_then(|v| v.as_str())
    .unwrap_or_default()
}

fn kill_mask_of(n: &Node) -> u32 {
  n.extras
    .get("lump")
    .and_then(|l| l.get("kill-mask"))
    .and_then(|k| k.as_f64())
    .map_or(0, |k| k as i64 as u32)
}

// a level per group (the same level twice counts once); host: an added level is its own (None),
// the companions and layers under its group are in it, the others in the level opened
fn level_id(
  out: &mut Items,
  ids: &mut HashMap<String, usize>,
  name: &str,
  layer: bool,
  host: Option<usize>,
) -> usize {
  if let Some(&id) = ids.get(name) {
    return id;
  }
  let id = out.levels.len();
  out.levels.push(name.to_string());
  out.is_layer.push(layer);
  out.host.push(host.unwrap_or(id));
  ids.insert(name.to_string(), id);
  id
}

fn collect<'a>(scene: &Scene, data: &'a GameData) -> Items<'a> {
  let mut out = Items {
    levels: vec![scene.settings.level.clone()],
    is_layer: vec![false],
    host: vec![0],
    items: Vec::new(),
  };
  let n = scene.nodes.len();
  let index: HashMap<u32, usize> = scene
    .nodes
    .iter()
    .enumerate()
    .map(|(i, x)| (x.id, i))
    .collect();
  let parent_of = |node: &Node| {
    if node.parent != 0 {
      index.get(&node.parent).copied()
    } else {
      None
    }
  };
  let mut ids: HashMap<String, usize> = HashMap::new();
  let host_of = |out: &mut Items, ids: &mut HashMap<String, usize>, group: &Node| {
    let mut p = parent_of(group);
    while let Some(i) = p {
      let up = &scene.nodes[i];
      let added = extra_string(up, "og_level");
      if !added.is_empty() {
        return level_id(out, ids, added, false, None);
      }
      p = parent_of(up);
    }
    0
  };
  // the level of each node: a group of an extra level, else the level opened
  let mut level_of: Vec<Option<usize>> = vec![None; n];
  let mut chain: Vec<usize> = Vec::new();
  for i in 0..n {
    chain.clear();
    let mut cur = i;
    let found = loop {
      if let Some(l) = level_of[cur] {
        break l;
      }
      chain.push(cur);
      let node = &scene.nodes[cur];
      let layer = extra_string(node, "og_layer");
      let companion = extra_string(node, "og_companion");
      let added = extra_string(node, "og_level");
      if !added.is_empty() {
        break level_id(&mut out, &mut ids, added, false, None);
      }
      if !layer.is_empty() || !companion.is_empty() {
        let host = host_of(&mut out, &mut ids, node);
        break if layer.is_empty() {
          level_id(&mut out, &mut ids, companion, false, Some(host))
        } else {
          level_id(&mut out, &mut ids, layer, true, Some(host))
        };
      }
      match parent_of(node) {
        Some(p) if chain.len() <= 1000 => cur = p,
        _ => break 0,
      }
    };
    for &c in &chain {
      level_of[c] = Some(found);
    }
  }

  for (i, node) in scene.nodes.iter().enumerate() {
    let level = level_of[i].unwrap_or(0);
    let in_layer = out.is_layer[level];
    let item = |kind, kill_mask, rules| Item {
      index: i,
      kind,
      level,
      kill_mask,
      rules,
    };
    if !extra_string(node, "og_layer").is_empty() {
      out.items.push(item(ItemKind::Layer, 0, Vec::new()));
      continue;
    }
    let parent = parent_of(node).map(|p| &scene.nodes[p]);
    if in_layer
      && parent.is_some_and(|p| !extra_string(p, "og_layer").is_empty())
      && !node.name.ends_with("-actors")
    {
      out.items.push(item(ItemKind::LayerDecor, 0, Vec::new()));
      continue;
    }
    if node.kind == NodeKind::Actor {
      let rules: Vec<&StoryCond> = data
        .type_rule(extra_string(node, "etype"))
        .into_iter()
        .collect();
      let kill_mask = kill_mask_of(node);
      if kill_mask != 0 || !rules.is_empty() || in_layer {
        out.items.push(item(ItemKind::Actor, kill_mask, rules));
      }
      continue;
    }
    // a decor prototype: a node under the level's tie or shrub trees
    if parent.is_some_and(|p| instance_tree(&p.name)) {
      let Some(rules) = data.prototype_rules(&out.levels[level]) else {
        continue;
      };
      let name = node.name.strip_suffix(".mb").unwrap_or(&node.name);
      let rules: Vec<&StoryCond> = rules
        .iter()
        .flat_map(|r| {
          r.names
            .iter()
            .filter(|p| p.as_str() == name)
            .map(move |_| &r.when)
        })
        .collect();
      if !rules.is_empty() {
        out.items.push(item(ItemKind::Prototype, 0, rules));
      }
    }
  }
  out
}

// hidden[k]: whether items.items[k] is hidden, each host (items.host) at its moment (moments[host]);
// only the items of host `only` when it is set
fn evaluate(
  items: &Items,
  data: &GameData,
  moments: &[&StoryMoment],
  hidden: &mut Vec<bool>,
  only: Option<usize>,
) {
  // what each host shows at its moment (the layers it borrows), and each level's task mask there
  let mut borrowed: HashMap<usize, BTreeMap<String, bool>> = HashMap::new();
  let mut masks = vec![0u32; items.levels.len()];
  for (l, &h) in items.host.iter().enumerate() {
    if only.is_some_and(|o| o != h) {
      continue;
    }
    let m = moments[h];
    borrowed
      .entry(h)
      .or_insert_with(|| data.borrowed(&items.levels[h], m));
    masks[l] = data.task_mask(&items.levels[l], &items.levels[h], m);
  }
  hidden.clear();
  hidden.resize(items.items.len(), false);
  for (k, it) in items.items.iter().enumerate() {
    let h = items.host[it.level];
    if only.is_some_and(|o| o != h) {
      continue;
    }
    let m = moments[h];
    let level = &items.levels[it.level];
    let shown = &borrowed[&h];
    let b = if items.is_layer[it.level] {
      shown.get(level)
    } else {
      None
    };
    let special = b.copied().unwrap_or(false);
    let mut hide = match it.kind {
      ItemKind::Layer => b.is_none(),
      ItemKind::LayerDecor => special, // a 'special level draws no decor
      ItemKind::Actor => !data.actor_alive(it.kill_mask, masks[it.level], special),
      ItemKind::Prototype => false,
    };
    for r in &it.rules {
      hide |= !data.eval(r, m);
    }
    hidden[k] = hide;
  }
}

/// Sets Node::story_hidden for that view; clears it when the view is off or the game has no story.
pub fn apply_story(scene: &mut Scene, data: Option<&GameData>, view: &StoryView) {
  for n in &mut scene.nodes {
    n.story_hidden = false;
  }
  scene.begin_frame();
  let Some(data) = data.filter(|d| d.has_story()) else {
    return;
  };
  if !view.enabled || scene.nodes.is_empty() {
    return;
  }
  let hidden = {
    let items = collect(scene, data);
    // the moment of each host: the level opened at view.step, a level added at its own
    let mut by_host: Vec<Option<StoryMoment>> = vec![None; items.levels.len()];
    for l in 0..items.levels.len() {
      let h = items.host[l];
      if by_host[h].is_none() {
        let step = if h == 0 {
          view.step
        } else {
          view.step_of(&items.levels[h])
        };
        by_host[h] = Some(data.moment(step));
      }
    }
    let moments: Vec<&StoryMoment> = (0..items.levels.len())
      .map(|l| by_host[items.host[l]].as_ref().expect("a host's moment"))
      .collect();
    let mut hidden = Vec::new();
    evaluate(&items, data, &moments, &mut hidden, None);
    items
      .items
      .iter()
      .zip(hidden)
      .map(|(it, h)| (it.index, h))
      .collect::<Vec<_>>()
  };
  for (i, h) in hidden {
    scene.nodes[i].story_hidden = h;
  }
}

/// The steps at which what the document shows of a level changes, in order: `level` is a level
/// added (with its companions and layers), "" the level opened (with its own).
pub fn story_changes(scene: &Scene, data: &GameData, level: &str) -> Vec<i32> {
  let mut out = Vec::new();
  if !data.has_story() || scene.nodes.is_empty() {
    return out;
  }
  let items = collect(scene, data);
  if items.items.is_empty() {
    return out;
  }
  let host = if level.is_empty() {
    Some(0)
  } else {
    (1..items.levels.len()).find(|&l| items.levels[l] == level && items.host[l] == l)
  };
  let Some(host) = host else {
    return out;
  };
  let (mut prev, mut cur) = (Vec::new(), Vec::new());
  for s in 1..=data.step_count() {
    let m = data.moment(s);
    let moments = vec![&m; items.levels.len()];
    evaluate(&items, data, &moments, &mut cur, Some(host));
    if s > 1 && cur != prev {
      out.push(s);
    }
    std::mem::swap(&mut prev, &mut cur);
  }
  out
}

#[cfg(test)]
mod tests {
  use serde_json::{Value, json};

  use super::*;
  use crate::io::game_data::tests::story_data;

  fn add(s: &mut Scene, name: &str, kind: NodeKind, parent: u32, extras: Value) -> u32 {
    s.add(Node {
      name: name.into(),
      kind,
      parent,
      extras,
      ..Default::default()
    })
  }

  // a level's decor as a level opened has it: its shell under its tfrag tree, its prototypes
  // (with an instance each) under its tie tree
  fn decor(s: &mut Scene, level: &str, protos: &[&str], parent: u32) {
    let t = add(
      s,
      &format!("{level}-tfrag-0"),
      NodeKind::Group,
      parent,
      json!({}),
    );
    add(s, &format!("{level}-floor"), NodeKind::Render, t, json!({}));
    add(s, &format!("{level}-wall"), NodeKind::Render, t, json!({}));
    let tie = add(
      s,
      &format!("{level}-tie-0"),
      NodeKind::Group,
      parent,
      json!({}),
    );
    for p in protos {
      let id = add(s, &format!("{p}.mb"), NodeKind::Group, tie, json!({}));
      add(s, &format!("{p}.mb-0"), NodeKind::Render, id, json!({}));
    }
  }

  fn actor(s: &mut Scene, name: &str, etype: &str, lump: Value, parent: u32) -> u32 {
    let mut l = lump;
    l["name"] = name.into();
    add(
      s,
      name,
      NodeKind::Actor,
      parent,
      json!({"etype": etype, "lump": l}),
    )
  }

  // town as open_level leaves it (the creatures and the cutscene character left out), with the
  // hut the game shows with it and the layer of its mission, under `parent`
  fn town(s: &mut Scene, parent: u32) {
    decor(s, "town", &["rock", "rubble"], parent);
    actor(
      s,
      "crate-1",
      "crate",
      json!({"kill-mask": 4294905854.0}),
      parent,
    );
    actor(
      s,
      "part-spawner-3",
      "part-spawner",
      json!({"art-name": "group-town-torch"}),
      parent,
    );
    actor(s, "pool-5", "water-anim", json!({"look": 0}), parent);
    actor(s, "statue-broken-6", "statue-broken", json!({}), parent);
    let hut = add(
      s,
      "hut",
      NodeKind::Group,
      parent,
      json!({"og_companion": "hut"}),
    );
    decor(s, "hut", &[], hut);
    let layer = add(
      s,
      "lmission",
      NodeKind::Group,
      parent,
      json!({"og_layer": "lmission"}),
    );
    actor(s, "lcrate-1", "crate", json!({}), layer);
  }

  fn shown(s: &Scene, name: &str) -> bool {
    s.nodes
      .iter()
      .find(|n| n.name == name)
      .is_some_and(|n| s.effectively_visible(n))
  }

  #[test]
  fn the_story_view() {
    let data = story_data();
    let mut s = Scene::new();
    s.settings.level = "town".into();
    town(&mut s, 0);
    // the story: step 1 (the mission under way), step 4 (the mission over)
    let mut view = StoryView::default();
    apply_story(&mut s, Some(&data), &view);
    let start = shown(&s, "crate-1")
      && !shown(&s, "statue-broken-6")
      && shown(&s, "rock.mb")
      && !shown(&s, "rubble.mb")
      && shown(&s, "lcrate-1")
      && shown(&s, "hut");
    view.step = 4;
    apply_story(&mut s, Some(&data), &view);
    let after = !shown(&s, "crate-1")
      && shown(&s, "statue-broken-6")
      && !shown(&s, "rock.mb")
      && shown(&s, "rubble.mb")
      && !shown(&s, "lcrate-1")
      && shown(&s, "hut");
    view.enabled = false;
    apply_story(&mut s, Some(&data), &view);
    let all = [
      "crate-1",
      "statue-broken-6",
      "rock.mb",
      "rubble.mb",
      "lcrate-1",
    ]
    .iter()
    .all(|n| shown(&s, n));
    assert!(
      start && after && all,
      "story: kill masks, type and prototype rules, borrowed layer at two steps; everything when off"
    );
    assert_eq!(
      story_changes(&s, &data, ""),
      [4],
      "story: the steps where the level changes"
    );
    // without game data, everything
    view.enabled = true;
    apply_story(&mut s, None, &view);
    assert!(shown(&s, "crate-1") && shown(&s, "rubble.mb"));
  }

  #[test]
  fn the_story_of_several_levels() {
    // port opened, town added at its place with its companion and its mission's layer under its
    // group, shown as when the player is in town (its layer borrowed at the start)
    let data = story_data();
    let mut doc = Scene::new();
    doc.settings.level = "port".into();
    decor(&mut doc, "port", &["crane"], 0);
    actor(&mut doc, "barrel-1", "crate", json!({}), 0);
    let group = add(
      &mut doc,
      "town",
      NodeKind::Group,
      0,
      json!({"og_level": "town"}),
    );
    town(&mut doc, group);
    let mut v = StoryView::default();
    apply_story(&mut doc, Some(&data), &v);
    let at_start = shown(&doc, "crate-1") && shown(&doc, "lcrate-1") && shown(&doc, "barrel-1");
    v.step = 4;
    apply_story(&mut doc, Some(&data), &v);
    let at_end = !shown(&doc, "crate-1") && !shown(&doc, "lcrate-1") && shown(&doc, "rubble.mb");
    assert!(
      at_start && at_end,
      "levels: an added level shown as when the player is there, its layer too"
    );
    // each level at its own point of the story: town still at the start while port is at the end
    v.level_steps.insert("town".into(), 1);
    apply_story(&mut doc, Some(&data), &v);
    let town_early = shown(&doc, "crate-1") && shown(&doc, "lcrate-1");
    assert!(
      town_early
        && story_changes(&doc, &data, "town") == [4]
        && story_changes(&doc, &data, "no-such-level").is_empty()
        && story_changes(&doc, &data, "").is_empty(),
      "story: each level at its own step, and the steps where an added level changes"
    );
    assert_eq!((v.step_of("town"), v.step_of("port")), (1, 4));
  }
}
