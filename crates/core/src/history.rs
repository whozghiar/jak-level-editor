//! Undo/redo. Every change of the document goes through a HistoryEntry.
//! NodeChange records the state of some nodes before a change and builds the entry afterwards.
//!
//! The entries act on the document they are given (`History::undo(&mut scene)`): the nodes, the
//! settings (`settings.replaces`), the materials and textures are all in the Scene.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::scene::{Node, Scene};

/// What an entry does to the document.
pub type HistoryAction = Box<dyn Fn(&mut Scene) + Send + Sync>;
/// Run after a NodeChange's entry is undone or redone (both share it).
pub type AfterApply = Arc<dyn Fn(&mut Scene) + Send + Sync>;

pub struct HistoryEntry {
  pub label: String,
  pub undo: HistoryAction,
  pub redo: HistoryAction,
}

impl HistoryEntry {
  pub fn new(
    label: impl Into<String>,
    undo: impl Fn(&mut Scene) + Send + Sync + 'static,
    redo: impl Fn(&mut Scene) + Send + Sync + 'static,
  ) -> Self {
    Self {
      label: label.into(),
      undo: Box::new(undo),
      redo: Box::new(redo),
    }
  }
}

const K_MAX: usize = 200;

#[derive(Default)]
pub struct History {
  entries: Vec<HistoryEntry>,
  cursor: usize,
  revision: u64,
}

impl History {
  pub fn push(&mut self, e: HistoryEntry) {
    self.entries.truncate(self.cursor);
    self.entries.push(e);
    if self.entries.len() > K_MAX {
      self.entries.remove(0);
    }
    self.cursor = self.entries.len();
    self.revision += 1;
  }

  pub fn can_undo(&self) -> bool {
    self.cursor > 0
  }

  pub fn can_redo(&self) -> bool {
    self.cursor < self.entries.len()
  }

  pub fn undo(&mut self, scene: &mut Scene) {
    if !self.can_undo() {
      return;
    }
    self.cursor -= 1;
    (self.entries[self.cursor].undo)(scene);
    self.revision += 1;
  }

  pub fn redo(&mut self, scene: &mut Scene) {
    if !self.can_redo() {
      return;
    }
    (self.entries[self.cursor].redo)(scene);
    self.cursor += 1;
    self.revision += 1;
  }

  pub fn clear(&mut self) {
    self.entries.clear();
    self.cursor = 0;
    self.revision += 1;
  }

  pub fn undo_label(&self) -> String {
    if self.can_undo() {
      self.entries[self.cursor - 1].label.clone()
    } else {
      String::new()
    }
  }

  pub fn redo_label(&self) -> String {
    if self.can_redo() {
      self.entries[self.cursor].label.clone()
    } else {
      String::new()
    }
  }

  /// every entry's label, the oldest first: those before `cursor()` are undone by undo()
  pub fn labels(&self) -> Vec<&str> {
    self.entries.iter().map(|e| e.label.as_str()).collect()
  }

  pub fn cursor(&self) -> usize {
    self.cursor
  }

  /// changes every time the document changes (dirty tracking, autosave)
  pub fn revision(&self) -> u64 {
    self.revision
  }
}

/// Replace the given nodes by their recorded state (or remove them), then restore the order.
fn apply_states(scene: &mut Scene, states: &BTreeMap<u32, Option<Node>>, order: &[u32]) {
  // the order of the nodes the order does not name: as they were, then those the states add
  let mut rest: Vec<u32> = scene.nodes.iter().map(|n| n.id).collect();
  rest.extend(states.keys().copied());
  let mut by_id: HashMap<u32, Node> = scene.nodes.drain(..).map(|n| (n.id, n)).collect();
  for (id, st) in states {
    match st {
      Some(n) => by_id.insert(*id, n.clone()),
      None => by_id.remove(id),
    };
  }
  for id in order.iter().chain(&rest) {
    if let Some(n) = by_id.remove(id) {
      scene.nodes.push(n);
    }
  }
  for n in &scene.nodes {
    scene.next_id = scene.next_id.max(n.id + 1);
  }
}

/// Snapshot-based node change:
///
/// ```ignore
/// let mut c = NodeChange::new(&scene);
/// c.save(&scene, id);            // before modifying or deleting node `id`
/// // ... modify the scene (nodes created after new() are detected) ...
/// history.push(c.commit(&scene, "Move", None));
/// ```
#[derive(Clone, Debug, Default)]
pub struct NodeChange {
  order_before: Vec<u32>,
  before: BTreeMap<u32, Option<Node>>,
}

impl NodeChange {
  pub fn new(scene: &Scene) -> Self {
    Self {
      order_before: scene.nodes.iter().map(|n| n.id).collect(),
      before: BTreeMap::new(),
    }
  }

  pub fn save(&mut self, scene: &Scene, id: u32) {
    self
      .before
      .entry(id)
      .or_insert_with(|| scene.find(id).cloned());
  }

  pub fn save_subtree(&mut self, scene: &Scene, id: u32) {
    self.save_subtrees(scene, &[id]);
  }

  /// in one pass over the scene
  pub fn save_subtrees(&mut self, scene: &Scene, ids: &[u32]) {
    for &id in ids {
      self.save(scene, id);
    }
    for below in scene.descendants_of(ids) {
      for d in below {
        self.save(scene, d);
      }
    }
  }

  pub fn commit(
    &self,
    scene: &Scene,
    label: impl Into<String>,
    after_apply: Option<AfterApply>,
  ) -> HistoryEntry {
    let existed: BTreeSet<u32> = self.order_before.iter().copied().collect();
    let mut before = self.before.clone();
    // new nodes
    for n in &scene.nodes {
      if !existed.contains(&n.id) {
        before.insert(n.id, None);
      }
    }
    let after: BTreeMap<u32, Option<Node>> = before
      .keys()
      .map(|&id| (id, scene.find(id).cloned()))
      .collect();
    let order_after: Vec<u32> = scene.nodes.iter().map(|n| n.id).collect();
    let order_before = self.order_before.clone();
    let after_undo = after_apply.clone();
    HistoryEntry::new(
      label,
      move |s: &mut Scene| {
        apply_states(s, &before, &order_before);
        if let Some(f) = &after_undo {
          f(s);
        }
      },
      move |s: &mut Scene| {
        apply_states(s, &after, &order_after);
        if let Some(f) = &after_apply {
          f(s);
        }
      },
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::NodeKind;

  fn ids(s: &Scene) -> Vec<u32> {
    s.nodes.iter().map(|n| n.id).collect()
  }

  #[test]
  fn node_change_undo_redo() {
    let mut s = Scene::new();
    let mut history = History::default();
    let named = |name: &str| Node {
      name: name.into(),
      ..Default::default()
    };
    let a = s.add(named("a"));
    let b = s.add(Node {
      parent: a,
      kind: NodeKind::Render,
      ..named("b")
    });
    let c = s.add(named("c"));
    let start = ids(&s);

    // a move, a deletion (with its descendants) and a new node in one change
    let mut change = NodeChange::new(&s);
    change.save(&s, c);
    change.save_subtree(&s, a);
    s.find_mut(c).unwrap().local.t.x = 5.0;
    s.remove(&[a]);
    let d = s.add(named("d"));
    s.nodes.reverse(); // the order changed too
    history.push(change.commit(&s, "edit", None));
    let edited = ids(&s);
    assert_eq!(edited, [d, c]);
    let rev = history.revision();

    history.undo(&mut s);
    assert_eq!(ids(&s), start, "the nodes back in their order");
    assert!(
      s.find(c).unwrap().local.t.x == 0.0 && s.find(b).unwrap().parent == a && s.find(d).is_none()
    );
    assert_eq!(history.redo_label(), "edit");
    history.redo(&mut s);
    assert_eq!(ids(&s), edited);
    assert!(s.find(c).unwrap().local.t.x == 5.0 && s.find(a).is_none());
    assert!(history.revision() == rev + 2 && history.undo_label() == "edit" && !history.can_redo());
    assert!(s.next_id > d);

    // the hook runs after both, a new entry drops what was undone
    let hook: AfterApply = Arc::new(|s: &mut Scene| s.settings.level = "hooked".into());
    let change = NodeChange::new(&s);
    s.find_mut(d).unwrap().name = "e".into();
    let mut change2 = change.clone();
    change2.save(&s, d); // saved after the change: records the changed state
    history.push(change2.commit(&s, "rename", Some(hook)));
    history.undo(&mut s);
    assert_eq!(s.settings.level, "hooked");
    history.undo(&mut s);
    history.push(HistoryEntry::new("other", |_| {}, |_| {}));
    assert!(!history.can_redo() && history.labels() == ["other"]);
  }

  #[test]
  fn at_most_200_entries() {
    let mut h = History::default();
    for i in 0..250 {
      h.push(HistoryEntry::new(format!("{i}"), |_| {}, |_| {}));
    }
    assert_eq!(h.labels().len(), 200);
    assert_eq!(h.labels()[0], "50");
  }
}
