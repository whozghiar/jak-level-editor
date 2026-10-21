//! Extraction of the game's assets (File > Extract game assets, src/app/extract.cpp).
//!
//! The editor runs ogle-extract, built with it and installed next to its executable, and reads the
//! lines it prints (see extractor/main.cpp):
//!   ogle-extract --detect <iso or folder>      the game, version and serial of the disc
//!   ogle-extract <iso or folder> <library>     every level of Jak 3, to <library>/jak3/
//! When it is done, the Levels panel shows its levels.
//!
//! ogle-extract runs as a child process; a thread reads its output (stdout and stderr in one pipe,
//! as the C++ editor read them) line by line into what the window shows (`snapshot`), and tells the
//! editor through its hook.

use std::io::{BufRead, BufReader, PipeReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use crate::io::asset_library::default_iso_data_dir;

const MAX_LOG_LINES: usize = 400;
const EXTRACTOR: &str = if cfg!(windows) {
  "ogle-extract.exe"
} else {
  "ogle-extract"
};
const EXTRACTOR_DATA: &str = "ogle-extract-data";

/// A text of the window: the translation of `key` with `args`, then ": `detail`" when there is
/// one; `text` when there is no such key (a code ogle-extract added since: shown as it said it).
/// The interface translates it (ui/lib/i18n.ts), the editor's log with `jle_core::i18n`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Text {
  pub key: String,
  pub args: Vec<usize>,
  pub detail: String,
  pub text: String,
}

impl Text {
  fn key(key: &str) -> Self {
    Self {
      key: key.into(),
      text: key.into(),
      ..Default::default()
    }
  }
}

// "levels 147" -> the translation of extract.step.levels with its number
fn step_text(step: &str) -> Text {
  let mut words = step.split_whitespace();
  let code = words.next().unwrap_or_default();
  let mut number = || words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
  let args = vec![number(), number()];
  Text {
    key: format!("extract.step.{code}"),
    args,
    detail: String::new(),
    text: step.into(),
  }
}

// "unknown_disc SCES-12345" -> the translation of extract.error.unknown_disc, then the detail
fn error_text(error: &str) -> Text {
  let (code, detail) = error.split_once(' ').unwrap_or((error, ""));
  Text {
    key: format!("extract.error.{code}"),
    args: Vec::new(),
    detail: detail.into(),
    text: error.into(),
  }
}

/// 75 seconds -> "1:15"
pub fn time_text(seconds: f64) -> String {
  let s = seconds.max(0.0) as u64;
  format!("{}:{:02}", s / 60, s % 60)
}

fn exe_dir() -> Option<PathBuf> {
  std::env::current_exe()
    .ok()
    .and_then(|e| e.parent().map(Path::to_path_buf))
}

// The repository this editor was built from, when it is still there (a build in development).
fn repo() -> Option<PathBuf> {
  let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
  (repo.join("Cargo.toml").is_file() && repo.join("crates").is_dir()).then(|| repo.to_path_buf())
}

// Where ogle-extract may be: the host's (Host::extractor, or the one its provider fetched), next to
// the running executable, in the application's resources, in the repository's build/bin/ (the C++
// editor's build, in development).
fn extractor_dirs(host: Option<&Path>, resource_dir: Option<&Path>) -> Vec<PathBuf> {
  [
    host.and_then(Path::parent).map(Path::to_path_buf),
    exe_dir(),
    resource_dir.map(Path::to_path_buf),
    repo().map(|r| r.join("build").join("bin")),
  ]
  .into_iter()
  .flatten()
  .collect()
}

fn found(paths: impl Iterator<Item = PathBuf>, is: fn(&Path) -> bool) -> String {
  paths
    .into_iter()
    .find(|p| is(p))
    .map(|p| p.to_string_lossy().into_owned())
    .unwrap_or_default()
}

/// ogle-extract: the host's, next to the editor, in its resources, in the repository's build/bin/,
/// else $JLE_EXTRACTOR; "" when missing.
pub fn extractor_path(host: Option<&Path>, resource_dir: Option<&Path>) -> String {
  let env = std::env::var_os("JLE_EXTRACTOR").map(PathBuf::from);
  let dirs = extractor_dirs(None, resource_dir);
  let paths = dirs.into_iter().map(|d| d.join(EXTRACTOR));
  found(
    host
      .map(Path::to_path_buf)
      .into_iter()
      .chain(paths)
      .chain(env),
    Path::is_file,
  )
}

/// Where ogle-extract is looked for (the Extract window's help when it is missing): the folders of
/// `extractor_path`, then JLE_EXTRACTOR's path when it is set
pub fn extractor_places(host: Option<&Path>, resource_dir: Option<&Path>) -> Vec<String> {
  let env =
    std::env::var_os("JLE_EXTRACTOR").map(|e| format!("JLE_EXTRACTOR={}", e.to_string_lossy()));
  let mut places: Vec<String> = Vec::new();
  for d in extractor_dirs(host, resource_dir) {
    // as the user writes it (Windows' resources come as \\?\D:\...), once
    let d = d.to_string_lossy();
    let d = d.strip_prefix(r"\\?\").unwrap_or(&d).to_string();
    if !places.contains(&d) {
      places.push(d);
    }
  }
  places.extend(env);
  places
}

/// The game's tables of ogle-extract (ogle-extract-data/), looked for as `extractor_path`; "" when
/// missing.
pub fn extractor_data_path(host: Option<&Path>, resource_dir: Option<&Path>) -> String {
  let env =
    std::env::var_os("JLE_EXTRACTOR").and_then(|e| Path::new(&e).parent().map(Path::to_path_buf));
  let dirs = extractor_dirs(host, resource_dir).into_iter().chain(env);
  found(dirs.map(|d| d.join(EXTRACTOR_DATA)), Path::is_dir)
}

/// What ogle-extract --detect said about `source`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Detected {
  pub source: String,
  pub game: String,
  pub name: String,
  pub version: String,
  pub serial: String,
  pub error: Option<Text>,
}

/// What the extraction tells the editor (its hook): it emits jle:progress, and once an extraction
/// finished, sets the library to `output` (the Levels panel shows its levels) and logs
/// log.extracted with `time_text(seconds)`; an extraction that failed logs its `error`.
#[derive(Clone, Debug)]
pub enum ExtractionEvent {
  /// a line of ogle-extract was read
  Line {
    detecting: bool,
    done: usize,
    total: usize,
    current: String,
  },
  /// ogle-extract ended by itself (not stopped): `finished` (OGLE_DONE), else `error`
  Ended {
    detecting: bool,
    finished: bool,
    error: Option<Text>,
    output: String,
    seconds: f64,
  },
}

/// Called on ogle-extract's reader thread, without any lock of the extraction held.
pub type ExtractionHook = Arc<dyn Fn(&ExtractionEvent) + Send + Sync>;

#[derive(Default)]
struct State {
  // which ogle-extract the lines are of: the reader of one stopped ignores the rest of its output
  run: u64,
  running: bool,
  detecting: bool, // --detect, else an extraction
  detected: Detected,
  levels: Value, // OGLE_LEVELS: the answer to --list
  log: Vec<String>,
  step: Option<Text>, // last OGLE_STEP
  done: usize,
  total: usize,
  current: String,     // last level written
  error: Option<Text>, // OGLE_ERROR, or why it stopped
  finished: bool,
  started: Option<Instant>,
  ended: Option<Instant>,
}

impl State {
  // extract_line
  fn line(&mut self, line: &str) {
    if let Some(r) = line.strip_prefix("OGLE_GAME ") {
      if let Ok(Value::Object(j)) = serde_json::from_str(r) {
        let field = |k: &str| {
          j.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
        };
        let d = &mut self.detected;
        d.game = field("game");
        d.name = field("name");
        d.version = field("version");
        d.serial = field("serial");
      }
    } else if let Some(r) = line.strip_prefix("OGLE_LEVELS ") {
      self.levels = serde_json::from_str(r).unwrap_or_default();
    } else if let Some(r) = line.strip_prefix("OGLE_STEP ") {
      self.step = Some(step_text(r));
    } else if let Some(r) = line.strip_prefix("OGLE_PROGRESS ") {
      let mut words = r.trim_start().splitn(3, char::is_whitespace);
      let mut number = || words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
      self.done = number();
      self.total = number();
      self.current = words.next().unwrap_or_default().trim_start().into();
    } else if line.starts_with("OGLE_DONE ") {
      self.finished = true;
    } else if let Some(r) = line.strip_prefix("OGLE_ERROR ") {
      self.error = Some(error_text(r));
    }
    self.log.push(line.into());
    if self.log.len() > MAX_LOG_LINES {
      self.log.drain(..self.log.len() - MAX_LOG_LINES);
    }
  }

  // ogle-extract exited by itself (the end of extract_poll)
  fn end(&mut self) {
    self.running = false;
    self.ended = Some(Instant::now());
    if self.detecting {
      self.detected.error = self.error.clone().or_else(|| {
        self
          .detected
          .game
          .is_empty()
          .then(|| Text::key("extract.not_recognized"))
      });
    } else if !self.finished && self.error.is_none() {
      self.error = Some(Text::key("extract.stopped"));
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

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
  state.lock().unwrap_or_else(|e| e.into_inner())
}

/// The extraction window's ogle-extract (ExtractState): the disc's detection, the extraction, its
/// log and progress.
#[derive(Default)]
pub struct Extraction {
  /// the application's resources (the plugin sets it): where ogle-extract may be installed
  pub resource_dir: Option<PathBuf>,
  /// the host's ogle-extract (Host::extractor, or the one its provider fetched): looked at first
  pub extractor: Option<PathBuf>,
  /// why the host's provider could not fetch ogle-extract (extract_fetch), "" when it did
  pub fetch_error: String,
  hook: Option<ExtractionHook>,
  state: Arc<Mutex<State>>,
  child: Option<Child>,
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

  /// ogle-extract is running (a detection or an extraction).
  pub fn running(&self) -> bool {
    lock(&self.state).running
  }

  /// ogle-extract --detect on `source` (extract_detect): the game of the disc, or why not. Nothing
  /// while an extraction is running.
  pub fn detect(&mut self, source: &str) {
    {
      let mut s = lock(&self.state);
      if s.running && !s.detecting {
        return;
      }
      s.detected = Detected {
        source: source.into(),
        ..Default::default()
      };
    }
    if source.is_empty() {
      self.stop();
      lock(&self.state).log.clear();
      return;
    }
    self.run_extractor(&["--detect", source], true, "");
  }

  /// Extracts every level of the disc `source` to `output`/jak3 (start_extraction; the editor
  /// keeps `source` in its preferences and logs log.extracting). False when ogle-extract cannot
  /// start: the snapshot's error says why.
  pub fn start(&mut self, source: &str, output: &str) -> bool {
    self.run_extractor(&[source, output], false, output)
  }

  fn run_extractor(&mut self, args: &[&str], detecting: bool, output: &str) -> bool {
    self.stop();
    let mut s = lock(&self.state);
    let now = Some(Instant::now());
    let detected = std::mem::take(&mut s.detected);
    *s = State {
      run: s.run + 1,
      detecting,
      detected,
      started: now,
      ended: now,
      ..Default::default()
    };
    let exe = extractor_path(self.extractor.as_deref(), self.resource_dir.as_deref());
    if exe.is_empty() {
      s.error = Some(Text::key("extract.missing"));
      return false;
    }
    let mut cmd = Command::new(&exe);
    cmd.args(args);
    // the game's tables: next to it (ogle-extract's default), else where the application has them
    let data = extractor_data_path(self.extractor.as_deref(), self.resource_dir.as_deref());
    if !Path::new(&exe).with_file_name(EXTRACTOR_DATA).is_dir() && !data.is_empty() {
      cmd.arg("--data").arg(data);
    }
    #[cfg(windows)]
    {
      use std::os::windows::process::CommandExt;
      cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console window of its own
    }
    // stdout and stderr in one pipe, in the order it wrote them
    let spawned = std::io::pipe().and_then(|(reader, writer)| {
      cmd
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer);
      Ok((cmd.spawn()?, reader))
    });
    drop(cmd); // its ends of the pipe: the reader sees the end when ogle-extract exits
    let (child, reader) = match spawned {
      Ok(r) => r,
      Err(e) => {
        s.error = Some(Text {
          detail: e.to_string(),
          ..Text::key("extract.cannot_start")
        });
        return false;
      }
    };
    s.running = true;
    let run = s.run;
    drop(s);
    self.child = Some(child);
    let (state, hook, output) = (self.state.clone(), self.hook.clone(), output.to_string());
    std::thread::spawn(move || read_output(reader, run, &state, hook, output));
    true
  }

  /// Kills a running ogle-extract (extract_stop): what it printed stays in the log.
  pub fn stop(&mut self) {
    let Some(mut child) = self.child.take() else {
      return;
    };
    {
      let mut s = lock(&self.state);
      if s.running {
        s.run += 1;
        s.running = false;
        s.ended = Some(Instant::now());
      }
    }
    let _ = child.kill();
    let _ = child.wait();
  }

  /// The window's Cancel: stops ogle-extract; an extraction stopped says it was cancelled.
  pub fn cancel(&mut self) {
    let extracting = {
      let s = lock(&self.state);
      s.running && !s.detecting
    };
    self.stop();
    if extracting {
      lock(&self.state).error = Some(Text::key("extract.cancelled"));
    }
  }

  /// What the window shows (extract_state): `{running, detecting, detected: {source, game, name,
  /// version, serial, error}, levels, log, step, done, total, current, error, finished, seconds}`;
  /// step and the errors are `Text`s (or null).
  pub fn snapshot(&self) -> Value {
    let s = lock(&self.state);
    json!({
      "running": s.running,
      "detecting": s.detecting,
      "detected": s.detected,
      "levels": s.levels,
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

  /// The window's first values (open_extract_window, open_dialog): `source`, the disc chosen last
  /// (the preferences' `extract_source`, else the one the host already extracted); `output`, the
  /// library folder (`library_root`, its default before one is chosen); `library` {root, levels}
  /// when one is loaded (`library_levels`); `iso_dir` where the disc dialogs start;
  /// `extractor` (its path, "" when missing), `extractor_found`, `extractor_dirs` (where it is
  /// looked for), `fetch_error`. The folders the dialogs start in are created, as the C++ editor
  /// did.
  pub fn defaults(&self, extract_source: &str, library_root: &str, library_levels: usize) -> Value {
    let output = library_root.to_string();
    let detected = lock(&self.state).detected.source.clone();
    let chosen = if detected.is_empty() {
      extract_source
    } else {
      &detected
    };
    let iso_dir = match Path::new(chosen).parent() {
      Some(p) if !chosen.is_empty() => p.to_string_lossy().into_owned(),
      _ => default_iso_data_dir(),
    };
    for dir in [&iso_dir, &output].into_iter().filter(|d| !d.is_empty()) {
      let _ = std::fs::create_dir_all(dir);
    }
    let (host, res) = (self.extractor.as_deref(), self.resource_dir.as_deref());
    let extractor = extractor_path(host, res);
    json!({
      "source": extract_source,
      "output": output,
      "library": (library_levels > 0).then(|| json!({ "root": library_root, "levels": library_levels })),
      "iso_dir": iso_dir,
      "extractor_found": !extractor.is_empty(),
      "extractor": extractor,
      "extractor_dirs": extractor_places(host, res),
      "fetch_error": self.fetch_error,
    })
  }
}

// ogle-extract's output, line by line (extract_poll), until it exits
fn read_output(
  reader: PipeReader,
  run: u64,
  state: &Mutex<State>,
  hook: Option<ExtractionHook>,
  output: String,
) {
  let emit = |e: ExtractionEvent| {
    if let Some(h) = &hook {
      h(&e);
    }
  };
  for line in BufReader::new(reader).split(b'\n') {
    let Ok(line) = line else { break };
    let line = String::from_utf8_lossy(&line);
    let event = {
      let mut s = lock(state);
      if s.run != run {
        return; // stopped
      }
      s.line(line.strip_suffix('\r').unwrap_or(&line));
      ExtractionEvent::Line {
        detecting: s.detecting,
        done: s.done,
        total: s.total,
        current: s.current.clone(),
      }
    };
    emit(event);
  }
  let event = {
    let mut s = lock(state);
    if s.run != run {
      return;
    }
    s.end();
    let output = if s.detecting { String::new() } else { output };
    ExtractionEvent::Ended {
      detecting: s.detecting,
      finished: s.finished,
      error: s.error.clone(),
      output,
      seconds: s.seconds(),
    }
  };
  emit(event);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn extractor_places_as_written() {
    let dir = r"C:\Program Files\host\jak-level-editor";
    let twice = extractor_places(None, Some(Path::new(&format!(r"\\?\{dir}"))));
    assert!(twice.iter().any(|p| p == dir), "{twice:?}");
    assert!(twice.iter().all(|p| !p.starts_with(r"\\?\")));
    let mut unique = twice.clone();
    unique.dedup();
    assert_eq!(unique.len(), twice.len());
  }

  #[test]
  fn extract_lines() {
    let mut s = State::default();
    s.line(r#"OGLE_GAME {"game":"jak3","version":"ntsc_v1","name":"Jak 3","serial":"SCUS-97330","iso":true}"#);
    let d = &s.detected;
    assert_eq!(
      (&*d.game, &*d.name, &*d.version, &*d.serial),
      ("jak3", "Jak 3", "ntsc_v1", "SCUS-97330")
    );
    s.line(r#"OGLE_LEVELS [{"dgo": "CIA.DGO", "names": ["ctyinda"]}]"#);
    assert_eq!(s.levels[0]["names"][0], "ctyinda");
    s.line("OGLE_STEP iso 3 10");
    let step = s.step.clone().unwrap();
    assert_eq!(
      (&*step.key, &step.args[..], &*step.text),
      ("extract.step.iso", &[3, 10][..], "iso 3 10")
    );
    s.line("OGLE_STEP levels 147");
    assert_eq!(s.step.as_ref().unwrap().args, [147, 0]);
    s.line("OGLE_PROGRESS 4 148 ctyinda");
    assert_eq!((s.done, s.total, &*s.current), (4, 148, "ctyinda"));
    s.line("[warn] a line of the decompiler");
    assert!(!s.finished && s.error.is_none());
    s.line("OGLE_ERROR unknown_disc SCES-12345");
    let e = s.error.clone().unwrap();
    assert_eq!(
      (&*e.key, &*e.detail, &*e.text),
      (
        "extract.error.unknown_disc",
        "SCES-12345",
        "unknown_disc SCES-12345"
      )
    );
    s.line("OGLE_ERROR no_levels");
    assert_eq!(s.error.as_ref().unwrap().detail, "");
    s.line("OGLE_DONE D:/library/jak3");
    assert!(s.finished);
    assert_eq!(s.log.len(), 9);
    for i in 0..500 {
      s.line(&format!("line {i}"));
    }
    assert_eq!((s.log.len(), &*s.log[0]), (MAX_LOG_LINES, "line 100"));

    // the ends: a disc not recognized, an extraction without its OGLE_DONE
    let mut d = State {
      detecting: true,
      ..Default::default()
    };
    d.end();
    assert_eq!(d.detected.error, Some(Text::key("extract.not_recognized")));
    let mut x = State::default();
    x.end();
    assert_eq!(x.error, Some(Text::key("extract.stopped")));
    assert_eq!(time_text(75.9), "1:15");
  }

  // every code of extractor/README.md has its text
  #[test]
  fn extract_keys_translated() {
    let en: Value = serde_json::from_str(include_str!("../../../../lang/en.json")).unwrap();
    let steps = ["iso", "read", "textures", "levels"].map(|c| format!("extract.step.{c}"));
    let errors = [
      "not_found",
      "unreadable",
      "not_iso",
      "not_jak",
      "no_executable",
      "unknown_disc",
      "not_jak3",
      "tables_missing",
      "tables_unreadable",
      "no_levels",
      "cannot_create",
      "iso_read",
      "extraction",
    ]
    .map(|c| format!("extract.error.{c}"));
    let others = [
      "missing",
      "cannot_start",
      "not_recognized",
      "stopped",
      "cancelled",
    ]
    .map(|c| format!("extract.{c}"));
    for key in steps.iter().chain(&errors).chain(&others) {
      assert!(en[key].is_string(), "{key} missing from lang/en.json");
    }
  }

  #[test]
  fn extract_detect_missing() {
    if extractor_path(None, None).is_empty() {
      return; // ogle-extract not built
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut x = Extraction::default();
    let seen = events.clone();
    x.set_hook(move |e| seen.lock().unwrap().push(e.clone()));
    let missing = std::env::temp_dir()
      .join("jle-no-such-disc.iso")
      .to_string_lossy()
      .into_owned();
    x.detect(&missing);
    // the hook runs once the state is updated: its Ended comes last
    let ended = || {
      matches!(
        events.lock().unwrap().last(),
        Some(ExtractionEvent::Ended { .. })
      )
    };
    let start = Instant::now();
    while !ended() && start.elapsed().as_secs() < 30 {
      std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!x.running());
    let snap = x.snapshot();
    assert_eq!(snap["detected"]["source"], missing);
    assert_eq!(
      snap["detected"]["error"]["key"], "extract.error.not_found",
      "{snap}"
    );
    assert_eq!(snap["detected"]["error"]["detail"], missing);
    let events = events.lock().unwrap();
    assert!(matches!(
      events.last(),
      Some(ExtractionEvent::Ended {
        detecting: true,
        finished: false,
        ..
      })
    ));
  }
}
