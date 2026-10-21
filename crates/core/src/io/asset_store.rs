//! Loads library files on worker threads, so that the panels never wait for them: models and
//! prefabs (baked into one mesh) and whole levels (for their thumbnails). Results are kept in
//! memory, the least recently used first to go.
//!
//!   let m = store.model(path, false);   // None while loading: ask again later
//!   store.update();                      // regularly: starts queued loads, collects finished ones

use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::io::asset_library::{LoadedMesh, load_model_asset};
use crate::io::gltf::{ImportOptions, import_gltf};
use crate::log_warn;
use crate::scene::Scene;

/// loads at once
const MAX_RUNNING: usize = 3;
/// of which levels (hundreds of MB each)
const MAX_LEVELS: usize = 1;
/// queued loads nobody asked for since (updates) are dropped
const STALE_UPDATES: u64 = 30;

#[derive(Clone)]
enum Loaded {
  Model(Arc<LoadedMesh>),
  Level(Arc<Scene>),
}

type Work = Box<dyn FnOnce() -> Option<Loaded> + Send>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
  Queued,
  Running,
  Ready,
  Failed,
}

struct Entry {
  state: State,
  /// 0 model, 1 level
  kind: i32,
  urgent: bool,
  work: Option<Work>,
  thread: Option<JoinHandle<Option<Loaded>>>,
  result: Option<Loaded>,
  last_use: u64,
}

/// The loads, by key ("m:<path>", "l:<path>"). A load still running when its entry goes (clear(),
/// forget_model()) finishes on its thread and is dropped.
#[derive(Default)]
pub struct AssetStore {
  entries: BTreeMap<String, Entry>,
  clock: u64,
}

impl AssetStore {
  pub fn new() -> Self {
    Self::default()
  }

  fn get(
    &mut self,
    key: String,
    kind: i32,
    urgent: bool,
    work: impl FnOnce() -> Work,
  ) -> Option<Loaded> {
    let clock = self.clock;
    let e = self.entries.entry(key).or_insert_with(|| Entry {
      state: State::Queued,
      kind,
      urgent: false,
      work: Some(work()),
      thread: None,
      result: None,
      last_use: 0,
    });
    e.last_use = clock;
    e.urgent |= urgent;
    if e.state == State::Ready {
      e.result.clone()
    } else {
      None
    }
  }

  fn failed(&self, key: &str) -> bool {
    self
      .entries
      .get(key)
      .is_some_and(|e| e.state == State::Failed)
  }

  /// None while loading or after a failure (logged once, see failed_model())
  pub fn model(&mut self, path: &str, urgent: bool) -> Option<Arc<LoadedMesh>> {
    let owned = path.to_string();
    let r = self.get(format!("m:{path}"), 0, urgent, || {
      Box::new(move || match load_model_asset(&owned, false) {
        Ok(m) => Some(Loaded::Model(Arc::new(m))),
        Err(e) => {
          log_warn!("%s: %s", &owned, e);
          None
        }
      })
    });
    match r {
      Some(Loaded::Model(m)) => Some(m),
      _ => None,
    }
  }

  pub fn level(&mut self, background_path: &str, urgent: bool) -> Option<Arc<Scene>> {
    let owned = background_path.to_string();
    let r = self.get(format!("l:{background_path}"), 1, urgent, || {
      Box::new(move || {
        let mut out = Scene::new();
        match import_gltf(&mut out, &owned, &ImportOptions::default()) {
          Ok(_) => Some(Loaded::Level(Arc::new(out))),
          Err(e) => {
            log_warn!("%s: %s", &owned, e);
            None
          }
        }
      })
    });
    match r {
      Some(Loaded::Level(s)) => Some(s),
      _ => None,
    }
  }

  pub fn failed_model(&self, path: &str) -> bool {
    self.failed(&format!("m:{path}"))
  }

  pub fn failed_level(&self, path: &str) -> bool {
    self.failed(&format!("l:{path}"))
  }

  /// the file changed
  pub fn forget_model(&mut self, path: &str) {
    self.entries.remove(&format!("m:{path}"));
  }

  pub fn update(&mut self) {
    self.clock += 1;
    let clock = self.clock;
    let (mut running, mut running_levels) = (0, 0);
    self.entries.retain(|_, e| {
      if e.state == State::Running {
        if e.thread.as_ref().is_some_and(|t| t.is_finished()) {
          e.result = e.thread.take().unwrap().join().ok().flatten();
          e.state = if e.result.is_some() {
            State::Ready
          } else {
            State::Failed
          };
        } else {
          running += 1;
          if e.kind == 1 {
            running_levels += 1;
          }
        }
      } else if e.state == State::Queued && e.last_use + STALE_UPDATES < clock && !e.urgent {
        return false; // scrolled away before it started
      }
      true
    });
    // urgent loads first (placement), then the most recently asked (visible thumbnails)
    while running < MAX_RUNNING {
      let best = self
        .entries
        .values_mut()
        .filter(|e| e.state == State::Queued && !(e.kind == 1 && running_levels >= MAX_LEVELS))
        .reduce(|best, e| {
          if (e.urgent && !best.urgent) || (e.urgent == best.urgent && e.last_use > best.last_use) {
            e
          } else {
            best
          }
        });
      let Some(best) = best else { break };
      best.state = State::Running;
      best.thread = Some(std::thread::spawn(best.work.take().unwrap()));
      running += 1;
      if best.kind == 1 {
        running_levels += 1;
      }
    }
    self.evict(0, 48);
    self.evict(1, 2);
  }

  fn evict(&mut self, kind: i32, keep: usize) {
    let mut ready: Vec<(u64, String)> = self
      .entries
      .iter()
      .filter(|(_, e)| e.kind == kind && e.state == State::Ready)
      .map(|(k, e)| (e.last_use, k.clone()))
      .collect();
    if ready.len() <= keep {
      return;
    }
    ready.sort();
    let drop = ready.len() - keep;
    for (_, k) in ready.into_iter().take(drop) {
      self.entries.remove(&k);
    }
  }

  /// queued or running loads
  pub fn pending(&self) -> usize {
    self
      .entries
      .values()
      .filter(|e| matches!(e.state, State::Queued | State::Running))
      .count()
  }

  /// forgets finished results (running loads finish and are dropped)
  pub fn clear(&mut self) {
    self.entries.clear();
  }
}
