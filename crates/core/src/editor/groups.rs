//! Groups of the document (src/app/groups.cpp): elements kept together under a name (extras
//! og_group {id, name, prefab} on each of their root elements), saved with the project. A prefab
//! placed is one (named after the prefab), a selection grouped by the user another (Edit > Group
//! the selection, Groups panel). A click selects one element; a double click, or the left button
//! held still, the whole group (an element in no group: its assembly).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{Value, json};

use super::Editor;
use crate::history::NodeChange;
use crate::i18n::tr;
use crate::scene::{Node, Scene};
use crate::strf;

/// the group an element is the root of (extras og_group), 0 none
pub fn group_tag(n: &Node) -> u32 {
  n.extras
    .get("og_group")
    .and_then(|g| g.get("id"))
    .and_then(Value::as_u64)
    .unwrap_or(0) as u32
}

/// a group given to it (0: none)
pub fn set_group_tag(n: &mut Node, id: u32, name: &str, prefab: bool) {
  if id == 0 {
    if let Some(o) = n.extras.as_object_mut() {
      o.remove("og_group");
    }
  } else {
    n.extras["og_group"] = json!({"id": id, "name": name, "prefab": prefab});
  }
}

/// a group id the document has not given yet
pub fn new_group_id(scene: &Scene) -> u32 {
  scene.nodes.iter().map(group_tag).max().unwrap_or(0) + 1
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct GroupInfo {
  pub id: u32,
  pub name: String,
  /// a prefab placed (named after it)
  pub prefab: bool,
  /// its elements (with what is under them)
  pub roots: Vec<u32>,
}

/// the groups, by id, found again when the document changes
#[derive(Default)]
pub(crate) struct GroupsCache(Mutex<Option<((u64, usize), Arc<Vec<GroupInfo>>)>>);

impl Editor {
  pub fn document_groups(&self) -> Arc<Vec<GroupInfo>> {
    let key = (self.history.revision(), self.scene.nodes.len());
    let mut cache = self.groups.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, g)) = cache.as_ref()
      && *k == key
    {
      return g.clone();
    }
    let mut by_id: BTreeMap<u32, GroupInfo> = BTreeMap::new();
    for n in &self.scene.nodes {
      let id = group_tag(n);
      if id == 0 {
        continue;
      }
      let g = by_id.entry(id).or_default();
      if g.roots.is_empty() {
        let tag = &n.extras["og_group"];
        g.id = id;
        g.name = tag["name"].as_str().unwrap_or_default().to_string();
        g.prefab = tag["prefab"].as_bool().unwrap_or(false);
      }
      g.roots.push(n.id);
    }
    let groups = Arc::new(by_id.into_values().collect::<Vec<_>>());
    *cache = Some((key, groups.clone()));
    groups
  }

  pub fn group_info(&self, gid: u32) -> Option<GroupInfo> {
    self.document_groups().iter().find(|g| g.id == gid).cloned()
  }

  /// the group of an element or of one above it, 0 none
  pub fn group_of(&self, id: u32) -> u32 {
    let mut n = self.scene.find(id);
    let mut guard = 0;
    while let Some(node) = n {
      let g = group_tag(node);
      if g != 0 {
        return g;
      }
      guard += 1;
      if node.parent == 0 || guard > 1000 {
        break;
      }
      n = self.scene.find(node.parent);
    }
    0
  }

  pub fn select_group(&mut self, gid: u32) {
    let Some(g) = self.group_info(gid).filter(|g| !g.roots.is_empty()) else {
      return;
    };
    self.selection = g.roots.iter().copied().collect();
    if !self.selection.contains(&self.primary) {
      self.primary = g.roots[0];
    }
  }

  /// the whole group of an element (double click, the button held still), else its assembly
  pub fn select_whole(&mut self, id: u32) -> bool {
    if id == 0 {
      return false;
    }
    let g = self.group_of(id);
    if g != 0 {
      self.select_group(g);
      return true;
    }
    self.select_assembly(id);
    self.selection.len() > 1
  }

  /// the selection grouped and named (undoable); the new group
  pub fn group_selection(&mut self, name: &str) -> u32 {
    let roots = self.selection_roots();
    if roots.is_empty() {
      return 0;
    }
    let gid = new_group_id(&self.scene);
    self.tag_roots(&roots, gid, name, false, strf!(tr("undo.group"), name));
    gid
  }

  pub fn ungroup(&mut self, gid: u32) {
    let Some(g) = self.group_info(gid) else {
      return;
    };
    self.tag_roots(&g.roots, 0, "", false, strf!(tr("undo.ungroup"), &g.name));
  }

  pub fn rename_group(&mut self, gid: u32, name: &str) {
    let Some(g) = self.group_info(gid) else {
      return;
    };
    if name.is_empty() || name == g.name {
      return;
    }
    let label = strf!(tr("undo.rename_group"), name);
    self.tag_roots(&g.roots, gid, name, g.prefab, label);
  }

  fn tag_roots(&mut self, roots: &[u32], gid: u32, name: &str, prefab: bool, label: String) {
    let mut c = NodeChange::new(&self.scene);
    for &id in roots {
      c.save(&self.scene, id);
      if let Some(n) = self.scene.find_mut(id) {
        set_group_tag(n, gid, name, prefab);
      }
    }
    self.history.push(c.commit(&self.scene, label, None));
  }
}
