//! File > Extract game assets: the levels of a game the user installed with OpenGOAL, read by its
//! own extractor and written to the library (io/extraction). The extraction runs on a thread of its
//! own; the window follows it (`snapshot`) on jle:progress and every 500 ms; the editor learns
//! through its hook that it ended (the Levels panel shows the levels written).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use crate::io::extraction::{self, Job, Options, Report};
use crate::io::game_install::GameInstall;

const MAX_LOG_LINES: usize = 400;

/// A text of the window: the translation of `key` with `args`, then ": `detail`" when there is
/// one; `text` when there is no such key. The interface translates it (ui/lib/i18n.ts), the
/// editor's log with `jle_core::i18n`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Text {
  pub key: String,
  pub args: Vec<usize>,
  pub detail: String,
  pub text: String,
}

impl Text {
  pub(crate) fn key(key: &str) -> Self {
    Self {
      key: key.into(),
      text: key.into(),
      ..Default::default()
    }
  }
}

/// 75 seconds -> "1:15"
pub fn time_text(seconds: f64) -> String {
  let s = seconds.max(0.0) as u64;
  format!("{}:{:02}", s / 60, s % 60)
}

/// The game the window extracts: the folder of its installation (the host's, else the settings'),
/// the installation found there or why not (a key of lang/*.json, game_install.error.*), its
/// disc's serial and the number of its levels.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Detected {
  pub game: String,
  pub folder: String,
  pub from_host: bool,
  pub install: Option<GameInstall>,
  pub error: Option<Text>,
  pub serial: String,
  pub levels: usize,
}

/// What the extraction tells the editor (its hook): it emits jle:progress, and once an extraction
/// finished, sets the library to `output` (the Levels panel shows its levels) and logs
/// log.extracted with `time_text(seconds)`; an extraction that failed logs its `error`.
#[derive(Clone, Debug)]
pub enum ExtractionEvent {
  /// a step, a level written, a line of the extractor
  Line {
    done: usize,
    total: usize,
    current: String,
  },
  /// the extraction ended by itself (not stopped)
  Ended {
    finished: bool,
    error: Option<Text>,
    output: String,
    seconds: f64,
  },
}

/// Called on the extraction's threads, without any lock of the extraction held.
pub type ExtractionHook = Arc<dyn Fn(&ExtractionEvent) + Send + Sync>;

#[derive(Default)]
struct State {
  // which extraction the reports are of: one stopped is not heard any more
  run: u64,
  running: bool,
  detected: Detected,
  log: Vec<String>,
  step: Option<Text>,
  done: usize,
  total: usize,
  current: String, // last level written
  error: Option<Text>,
  finished: bool,
  started: Option<Instant>,
  ended: Option<Instant>,
}

impl State {
  fn report(&mut self, r: Report) {
    match r {
      Report::Line(line) => {
        self.log.push(line);
        if self.log.len() > MAX_LOG_LINES {
          self.log.drain(..self.log.len() - MAX_LOG_LINES);
        }
      }
      Report::Step(key, args) => {
        self.step = Some(Text {
          key: format!("extract.step.{key}"),
          args,
          ..Text::key(key)
        });
      }
      Report::Level { done, total, name } => {
        self.done = done;
        self.total = total;
        self.current = name;
      }
    }
  }

  fn seconds(&self) -> f64 {
    let (Some(started), Some(ended)) = (self.started, self.ended) else {
      return 0.0;
    };
    let end = if self.running { Instant::now() } else { ended };
    end.duration_since(started).as_secs_f64()
  }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
  m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The extraction window's extraction (ExtractState): its game, its log and progress.
#[derive(Default)]
pub struct Extraction {
  hook: Option<ExtractionHook>,
  state: Arc<Mutex<State>>,
  cancel: Arc<AtomicBool>,
  /// the extractors running (io/extraction: several at once)
  children: Arc<Mutex<HashMap<u32, Child>>>,
}

impl Drop for Extraction {
  fn drop(&mut self) {
    self.stop();
  }
}

impl Extraction {
  pub fn set_hook(&mut self, hook: impl Fn(&ExtractionEvent) + Send + Sync + 'static) {
    self.hook = Some(Arc::new(hook));
  }

  pub fn running(&self) -> bool {
    lock(&self.state).running
  }

  /// The game the window shows (extract_detect): the editor finds its installation
  pub fn set_detected(&mut self, d: Detected) {
    lock(&self.state).detected = d;
  }

  /// Extracts the levels of `install` to `output`/<game> (extract_start), the extractor working in
  /// `work`. False when an extraction runs.
  pub fn start(
    &mut self,
    install: GameInstall,
    output: &str,
    work: PathBuf,
    opts: Options,
  ) -> bool {
    let run = {
      let mut s = lock(&self.state);
      if s.running {
        return false;
      }
      let now = Some(Instant::now());
      let detected = std::mem::take(&mut s.detected);
      *s = State {
        run: s.run + 1,
        running: true,
        detected,
        started: now,
        ended: now,
        ..Default::default()
      };
      s.run
    };
    self.cancel = Arc::new(AtomicBool::new(false));
    let (state, hook, cancel, children) = (
      self.state.clone(),
      self.hook.clone(),
      self.cancel.clone(),
      self.children.clone(),
    );
    let output = output.to_string();
    std::thread::spawn(move || {
      let emit = |e: ExtractionEvent| {
        if let Some(h) = &hook {
          h(&e);
        }
      };
      let report = |r: Report| {
        let event = {
          let mut s = lock(&state);
          if s.run != run {
            return;
          }
          s.report(r);
          ExtractionEvent::Line {
            done: s.done,
            total: s.total,
            current: s.current.clone(),
          }
        };
        emit(event);
      };
      let job = Job {
        install: &install,
        library: output.as_ref(),
        work: &work,
        options: &opts,
        cancel: &cancel,
        children: &children,
      };
      let result = extraction::run(&job, &report);
      let event = {
        let mut s = lock(&state);
        if s.run != run {
          return; // stopped
        }
        s.running = false;
        s.ended = Some(Instant::now());
        match result {
          Ok(_) => s.finished = true,
          Err(e) => {
            s.log.push(e.clone());
            s.error = Some(Text {
              detail: e,
              ..Text::key("extract.error.extraction")
            });
          }
        }
        ExtractionEvent::Ended {
          finished: s.finished,
          error: s.error.clone(),
          output: output.clone(),
          seconds: s.seconds(),
        }
      };
      emit(event);
    });
    true
  }

  /// Stops a running extraction (extract_stop): what it wrote and logged stays.
  pub fn stop(&mut self) {
    self.cancel.store(true, Ordering::Relaxed);
    for c in lock(&self.children).values_mut() {
      let _ = c.kill();
    }
    let mut s = lock(&self.state);
    if s.running {
      s.run += 1;
      s.running = false;
      s.ended = Some(Instant::now());
    }
  }

  /// The window's Cancel: an extraction stopped says it was cancelled.
  pub fn cancel(&mut self) {
    let running = self.running();
    self.stop();
    if running {
      lock(&self.state).error = Some(Text::key("extract.cancelled"));
    }
  }

  /// What the window shows (extract_state): `{running, detected: Detected, log, step, done, total,
  /// current, error, finished, seconds}`; step and error are `Text`s (or null).
  pub fn snapshot(&self) -> Value {
    let s = lock(&self.state);
    json!({
      "running": s.running,
      "detected": s.detected,
      "log": s.log,
      "step": s.step,
      "done": s.done,
      "total": s.total,
      "current": s.current,
      "error": s.error,
      "finished": s.finished,
      "seconds": s.seconds(),
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn extraction_reports_shown() {
    let mut s = State::default();
    s.report(Report::Step("levels", vec![272]));
    let step = s.step.clone().unwrap();
    assert_eq!(
      (&*step.key, &step.args[..]),
      ("extract.step.levels", &[272][..])
    );
    s.report(Report::Level {
      done: 4,
      total: 273,
      name: "ctyinda".into(),
    });
    assert_eq!((s.done, s.total, &*s.current), (4, 273, "ctyinda"));
    for i in 0..500 {
      s.report(Report::Line(format!("line {i}")));
    }
    assert_eq!((s.log.len(), &*s.log[0]), (MAX_LOG_LINES, "line 100"));
    assert_eq!(time_text(75.9), "1:15");
  }

  // every step and end of an extraction has its text
  #[test]
  fn extract_keys_translated() {
    let en: Value = serde_json::from_str(include_str!("../../../../lang/en.json")).unwrap();
    let keys = [
      "extract.step.levels",
      "extract.error.extraction",
      "extract.cancelled",
      "game_install.error.no_extractor",
      "game_install.error.no_config",
    ];
    for key in keys {
      assert!(en[key].is_string(), "{key} missing from lang/en.json");
    }
  }

  #[test]
  fn extraction_without_its_game_ends_with_why() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut x = Extraction::default();
    let seen = events.clone();
    x.set_hook(move |e| seen.lock().unwrap().push(e.clone()));
    let tmp = std::env::temp_dir().join(format!("jle-extract-{}", std::process::id()));
    let install = GameInstall {
      game: "jak3".into(),
      data: tmp.join("nothing").to_string_lossy().into_owned(),
      ..Default::default()
    };
    assert!(x.start(
      install,
      &tmp.to_string_lossy(),
      tmp.join("work"),
      Options::default()
    ));
    let start = Instant::now();
    while x.running() && start.elapsed().as_secs() < 30 {
      std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let snap = x.snapshot();
    assert_eq!(snap["error"]["key"], "extract.error.extraction", "{snap}");
    assert!(matches!(
      events.lock().unwrap().last(),
      Some(ExtractionEvent::Ended {
        finished: false,
        ..
      })
    ));
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
