//! The game runtimes: an OpenGOAL that installs a level exported for the game, builds it and starts
//! the game on it (host::GameRuntime), and the standalone app's runtime, which uses the one the user
//! chose in the settings.
//!
//! `OpenGoal` is an OpenGOAL the editor can drive: a jak-project checkout (its goal_src and
//! custom_assets, its build's goalc and gk) or an OpenGOAL distribution (`extractor`, `gk` and
//! `data/`, as the OpenGOAL Launcher installs them). The OpenGOAL Launcher uses the same pieces
//! (`OpenGoal::distribution`, `compile_command`, `launch_command`, `run_logged`) for its own
//! runtime. The level builder must have the fixes of the editor's exports
//! (`custom_assets/<game>/jle-builder.json`: `{"builder": "jle-1"}`).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::host::{GameRuntime, HostEvent, HostEvents, LevelPackage, RuntimeInfo};
use crate::io::level_package::{self, BUILDER_MARKER};
use crate::prefs::Prefs;

/// the builder this editor's exports need (LevelManifest::builder)
pub const BUILDER: &str = "jle-1";

fn exe(dir: &Path, name: &str) -> PathBuf {
  dir.join(if cfg!(windows) {
    format!("{name}.exe")
  } else {
    name.to_string()
  })
}

fn step(events: &HostEvents, key: &str, args: &[&str]) {
  events(HostEvent::Step {
    key: key.into(),
    args: args.iter().map(|a| a.to_string()).collect(),
  });
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
  use std::os::windows::process::CommandExt;
  cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console window of its own
}

#[cfg(not(windows))]
fn no_window(_: &mut Command) {}

/// a line without its terminal colors (`ESC [ ... m`)
fn strip_ansi(line: &str) -> String {
  let mut out = String::with_capacity(line.len());
  let mut chars = line.chars();
  while let Some(c) = chars.next() {
    if c == '\x1b' {
      // up to the letter ending the sequence
      for c in chars.by_ref() {
        if c.is_ascii_alphabetic() {
          break;
        }
      }
    } else {
      out.push(c);
    }
  }
  out
}

/// Runs `cmd` to its end, each line of its output an event (the make system's `[ 42%]` a
/// progress); an error with its last lines when it fails.
pub fn run_logged(mut cmd: Command, events: &HostEvents) -> Result<(), String> {
  no_window(&mut cmd);
  let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
  cmd
    .stdin(Stdio::null())
    .stdout(writer.try_clone().map_err(|e| e.to_string())?)
    .stderr(writer);
  let program = cmd.get_program().to_string_lossy().into_owned();
  let mut child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
  drop(cmd); // its ends of the pipe: the reader sees the end when the program exits
  let mut last: Vec<String> = Vec::new();
  for line in BufReader::new(reader).lines().map_while(Result::ok) {
    let line = strip_ansi(&line);
    if let Some(p) = line
      .strip_prefix('[')
      .and_then(|l| l.split_once("%]"))
      .and_then(|(n, _)| n.trim().parse::<u64>().ok())
    {
      events(HostEvent::Progress {
        done: p.min(100),
        total: 100,
      });
    }
    last.push(line.clone());
    if last.len() > 20 {
      last.remove(0);
    }
    events(HostEvent::Log { line });
  }
  let status = child.wait().map_err(|e| e.to_string())?;
  if status.success() {
    Ok(())
  } else {
    Err(format!("{program}: {status}\n{}", last.join("\n")))
  }
}

/// An OpenGOAL the editor drives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenGoal {
  /// "jak-project" (a checkout) or "launcher" (the OpenGOAL Launcher's isolated game)
  pub kind: String,
  /// the folder the user knows it by
  pub folder: PathBuf,
  /// the folder holding goal_src/ and custom_assets/: the levels go there (`--proj-path`, always
  /// given: else the programs look for it by the name "jak-project" in their own path)
  pub data_dir: PathBuf,
  pub gk: PathBuf,
  pub goalc: PathBuf,
  pub extractor: PathBuf,
  /// the game's files (iso_data/<game>) it builds from (`extractor --folder --compile`)
  pub iso_data: PathBuf,
  /// gk's settings and saves, apart from the user's game
  pub config_dir: PathBuf,
}

/// a folder holding the game's files (a checkout's iso_data/jak3 is there, empty, until the game
/// is extracted into it)
fn has_game_files(iso_data: &Path) -> bool {
  iso_data.join("CGO").is_dir()
}

/// the game's files the OpenGOAL Launcher extracted (read only: building from them writes nothing
/// there)
pub fn launcher_iso_data(install: &Path, game: &str) -> PathBuf {
  install
    .join("active")
    .join(game)
    .join("data")
    .join("iso_data")
    .join(game)
}

impl OpenGoal {
  /// A jak-project checkout (`root`: goal_src/, custom_assets/), its programs in `bin` (else its
  /// build's out/build/Release/bin), building from its iso_data/<game>, else from the game the
  /// OpenGOAL Launcher extracted. Its settings in out/jle-settings.
  pub fn checkout(root: &Path, bin: Option<&Path>, game: &str) -> OpenGoal {
    let bin = bin.map_or_else(|| root.join("out/build/Release/bin"), Path::to_path_buf);
    let own = root.join("iso_data").join(game);
    let iso_data = match launcher_install_dir() {
      Some(install) if !has_game_files(&own) => launcher_iso_data(&install, game),
      _ => own,
    };
    OpenGoal {
      kind: "jak-project".into(),
      folder: root.to_path_buf(),
      data_dir: root.to_path_buf(),
      gk: exe(&bin, "gk"),
      goalc: exe(&bin, "goalc"),
      extractor: exe(&bin, "extractor"),
      iso_data,
      config_dir: root.join("out/jle-settings"),
    }
  }

  /// An OpenGOAL distribution in `folder` (extractor, gk, goalc, data/), building from the game's
  /// files `iso_data` (launcher_iso_data for the launcher's), its settings in `<folder>/_settings`
  /// (the launcher's mods keep theirs in the same way).
  pub fn distribution(kind: &str, folder: &Path, iso_data: &Path) -> OpenGoal {
    OpenGoal {
      kind: kind.into(),
      folder: folder.to_path_buf(),
      data_dir: folder.join("data"),
      gk: exe(folder, "gk"),
      goalc: exe(folder, "goalc"),
      extractor: exe(folder, "extractor"),
      iso_data: iso_data.to_path_buf(),
      config_dir: folder.join("_settings"),
    }
  }

  /// The builder it has: its jle-builder.json, else (a checkout of before it) the fixes' code
  pub fn builder(&self, game: &str) -> String {
    let b = level_package::builder_of(&self.data_dir, game);
    if !b.is_empty() {
      return b;
    }
    let gltf = self.data_dir.join("common/util/gltf_util.cpp");
    match std::fs::read_to_string(gltf) {
      Ok(code) if code.contains("og_blend") => BUILDER.into(),
      _ => String::new(),
    }
  }

  /// Why it cannot play a level of `game` now (a translation key), None when it can
  pub fn problem(&self, game: &str) -> Option<&'static str> {
    if !self
      .data_dir
      .join("goal_src")
      .join(game)
      .join("game.gp")
      .is_file()
    {
      return Some("runtime.no_sources");
    }
    if ![&self.gk, &self.goalc, &self.extractor]
      .iter()
      .all(|p| p.is_file())
    {
      return Some("runtime.no_programs");
    }
    if self.builder(game) != BUILDER {
      return Some("runtime.old_builder");
    }
    if !has_game_files(&self.iso_data) {
      return Some("runtime.no_game_files");
    }
    None
  }

  /// The game built again, the levels installed included: the extractor's `--folder --compile`
  /// from the game's files (goalc's `(mi)`; what the OpenGOAL Launcher runs for mods)
  pub fn compile_command(&self, game: &str) -> Command {
    let mut cmd = Command::new(&self.extractor);
    cmd
      .arg(&self.iso_data)
      .args(["--folder", "--compile", "--game", game, "--proj-path"])
      .arg(&self.data_dir)
      .current_dir(self.extractor.parent().unwrap_or(&self.folder));
    cmd
  }

  /// The game started (a debug game: its larger level heaps), with its own settings
  pub fn launch_command(&self, game: &str) -> Command {
    let mut cmd = Command::new(&self.gk);
    cmd
      .args(["-v", "--game", game, "--proj-path"])
      .arg(&self.data_dir)
      .arg("--config-path")
      .arg(&self.config_dir)
      .args(["--", "-boot", "-fakeiso", "-debug"])
      .current_dir(self.gk.parent().unwrap_or(&self.folder));
    cmd
  }

  /// The startup file of goalc's user "jle": what its REPL runs (startup_code)
  pub fn startup_file(&self, game: &str) -> PathBuf {
    self
      .data_dir
      .join("goal_src/user/jle")
      .join(format!("startup-{game}.gc"))
  }

  /// Jak taken to a continue point of the game started (Jak 3's kernel has no argument for it):
  /// goalc's REPL as the user "jle" runs its startup file (startup_code) once it listens to the
  /// game. Its stdin must stay open (a REPL that ends resets the game); `port` and `debug_port`:
  /// free ones (the user's own goalc may be running).
  pub fn repl_command(&self, game: &str, port: u16, debug_port: u16) -> Command {
    let mut cmd = Command::new(&self.goalc);
    cmd
      .args(["-g", game, "--disable-ansi", "--user", "jle", "-p"])
      .arg(port.to_string())
      .arg("--debug-port")
      .arg(debug_port.to_string())
      .arg("--proj-path")
      .arg(&self.data_dir)
      .current_dir(self.goalc.parent().unwrap_or(&self.folder));
    cmd
  }

  pub fn info(&self, game: &str) -> RuntimeInfo {
    let problem = self.problem(game);
    RuntimeInfo {
      kind: self.kind.clone(),
      label: self.folder.to_string_lossy().into_owned(),
      available: problem.is_none(),
      reason: problem.unwrap_or_default().into(),
      games: vec![game.into()],
      installed: level_package::installed(&self.data_dir)
        .into_iter()
        .filter(|l| l.game == game)
        .map(|l| l.name)
        .collect(),
    }
  }

  fn compile(&self, game: &str, events: &HostEvents) -> Result<(), String> {
    // the game started before keeps open the files the build writes
    stop_game();
    step(events, "runtime.step.compile", &[]);
    run_logged(self.compile_command(game), events)
  }
}

/// goalc's startup file taking Jak to `continue_point`: the game's code known, the game listened
/// to, then (the lines below the marker run once it listens) `start`
pub fn startup_code(continue_point: &str) -> String {
  format!(
    "(make-group \"all-code\")\n(lt)\n;; og:run-below-on-listen\n(start 'play (get-continue-by-name *game-info* \"{continue_point}\"))\n"
  )
}

/// a port nothing listens on
fn free_port() -> u16 {
  std::net::TcpListener::bind("127.0.0.1:0")
    .and_then(|l| l.local_addr())
    .map_or(8193, |a| a.port())
}

/// the game the editor started last: stopped before another one starts, or a build
static GAME: Mutex<Option<std::process::Child>> = Mutex::new(None);

/// Stops the game the editor started, when it still runs.
pub fn stop_game() {
  if let Some(mut child) = GAME.lock().unwrap_or_else(|e| e.into_inner()).take()
    && child.try_wait().ok().flatten().is_none()
  {
    let _ = child.kill();
    let _ = child.wait();
  }
}

impl GameRuntime for OpenGoal {
  fn info(&self) -> RuntimeInfo {
    OpenGoal::info(self, "jak3")
  }

  fn install_level(&self, package: &LevelPackage, events: HostEvents) -> Result<(), String> {
    let m = &package.manifest;
    if let Some(p) = self.problem(&m.game) {
      return Err(crate::i18n::tr(p).into());
    }
    if !m.builder.is_empty() && self.builder(&m.game) != m.builder {
      return Err(crate::i18n::tr("runtime.old_builder").into());
    }
    step(&events, "runtime.step.install", &[&m.name]);
    level_package::apply(&self.data_dir, package)?;
    self.compile(&m.game, &events)
  }

  fn remove_level(&self, game: &str, name: &str, events: HostEvents) -> Result<(), String> {
    step(&events, "runtime.step.remove", &[name]);
    level_package::remove(&self.data_dir, game, name)?;
    self.compile(game, &events)
  }

  fn launch(&self, game: &str, name: &str, events: HostEvents) -> Result<(), String> {
    let level = level_package::installed(&self.data_dir)
      .into_iter()
      .find(|l| l.game == game && l.name == name)
      .ok_or_else(|| format!("{name} is not installed"))?;
    stop_game();
    step(&events, "runtime.step.launch", &[&level.continue_point]);
    let startup = self.startup_file(game);
    let _ = std::fs::create_dir_all(startup.parent().unwrap_or(&self.data_dir));
    std::fs::write(&startup, startup_code(&level.continue_point))
      .map_err(|e| format!("{}: {e}", startup.display()))?;
    let _ = std::fs::create_dir_all(&self.config_dir);
    // the game's output in a file next to its settings: where it is, why it stopped
    let log = self.config_dir.join("jle-game.log");
    let out = std::fs::File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let mut cmd = self.launch_command(game);
    no_window(&mut cmd);
    cmd
      .stdin(Stdio::null())
      .stdout(out.try_clone().map_err(|e| e.to_string())?)
      .stderr(out);
    events(HostEvent::Log {
      line: format!("{cmd:?}"),
    });
    let gk = cmd
      .spawn()
      .map_err(|e| format!("{}: {e}", self.gk.display()))?;
    *GAME.lock().unwrap_or_else(|e| e.into_inner()) = Some(gk);
    let game_ended = || {
      let mut g = GAME.lock().unwrap_or_else(|e| e.into_inner());
      g.as_mut()
        .and_then(|c| c.try_wait().ok().flatten())
        .map(|status| {
          let text = std::fs::read_to_string(&log).unwrap_or_default();
          let tail: Vec<&str> = text.lines().rev().take(15).collect();
          let tail: Vec<&str> = tail.into_iter().rev().collect();
          format!("gk: {status}\n{}", tail.join("\n"))
        })
    };

    // goalc's REPL, its output as events, killed once the level shows (its end would reset the
    // game; killed, it leaves the game as it is)
    let (port, debug_port) = (free_port(), free_port());
    let mut cmd = self.repl_command(game, port, debug_port);
    no_window(&mut cmd);
    let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    cmd
      .stdin(Stdio::piped())
      .stdout(writer.try_clone().map_err(|e| e.to_string())?)
      .stderr(writer);
    let mut goalc = cmd
      .spawn()
      .map_err(|e| format!("{}: {e}", self.goalc.display()))?;
    drop(cmd);
    let lines = events.clone();
    std::thread::spawn(move || {
      for line in BufReader::new(reader).lines().map_while(Result::ok) {
        lines(HostEvent::Log { line });
      }
    });
    // the level the game shows: this one, or the one it replaces (its name kept)
    let shown = if level.replaces.is_empty() {
      &level.name
    } else {
      &level.replaces
    };
    let wanted = format!("Displaying level {shown} ");
    let started = Instant::now();
    let r = loop {
      let text = std::fs::read(&log).unwrap_or_default();
      if String::from_utf8_lossy(&text).contains(&wanted) {
        break Ok(());
      }
      if let Some(e) = game_ended() {
        break Err(e);
      }
      if let Ok(Some(status)) = goalc.try_wait() {
        break Err(format!("goalc: {status}"));
      }
      if started.elapsed() > Duration::from_secs(600) {
        break Err(format!("{shown}: not shown after 10 minutes"));
      }
      std::thread::sleep(Duration::from_millis(500));
    };
    let _ = goalc.kill();
    let _ = goalc.wait();
    r
  }
}

// ------------------------------------------------------------------------------------------------
// the standalone app's runtime
// ------------------------------------------------------------------------------------------------

/// The OpenGOAL Launcher's installation folder (its settings.json's installationDir)
pub fn launcher_install_dir() -> Option<PathBuf> {
  let appdata = std::env::var_os("APPDATA")
    .map(PathBuf::from)
    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
  let text = std::fs::read(appdata.join("OpenGOAL-Launcher/settings.json")).ok()?;
  let v: Value = serde_json::from_slice(&text).ok()?;
  v["installationDir"]
    .as_str()
    .filter(|d| !d.is_empty())
    .map(PathBuf::from)
}

/// The OpenGOAL Launcher's isolated game for the editor: `<install>/features/<game>/level_edited/`,
/// built from the disc the launcher extracted (`<install>/active/<game>/data/iso_data/<game>`)
pub fn launcher_level_edited(install: &Path, game: &str) -> OpenGoal {
  OpenGoal::distribution(
    "launcher",
    &install.join("features").join(game).join("level_edited"),
    &launcher_iso_data(install, game),
  )
}

/// The standalone app's runtime: the jak-project checkout chosen in the settings
/// (Prefs::runtime_folder, its programs in Prefs::runtime_bin or its build's), else the OpenGOAL
/// Launcher's isolated game when it is there. Read again at each call (the settings change).
#[derive(Default)]
pub struct StandaloneRuntime {
  /// info() is asked at each get_state: kept a moment
  info: Mutex<Option<(Instant, RuntimeInfo)>>,
}

impl StandaloneRuntime {
  /// The checkout chosen, the launcher's game detected (each or None)
  pub fn candidates() -> (Option<OpenGoal>, Option<OpenGoal>) {
    let (folder, bin) = {
      let p = Prefs::get();
      (p.runtime_folder.clone(), p.runtime_bin.clone())
    };
    let checkout = (!folder.is_empty()).then(|| {
      OpenGoal::checkout(
        Path::new(&folder),
        (!bin.is_empty()).then(|| Path::new(&bin)),
        "jak3",
      )
    });
    let launcher = launcher_install_dir().map(|d| launcher_level_edited(&d, "jak3"));
    (checkout, launcher)
  }

  /// the one used: the checkout chosen, else the launcher's game when it can play
  pub fn current() -> Option<OpenGoal> {
    match Self::candidates() {
      (Some(c), _) => Some(c),
      (None, Some(l)) if l.problem("jak3").is_none() => Some(l),
      _ => None,
    }
  }

  fn used(&self) -> Result<OpenGoal, String> {
    *self.info.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Self::current().ok_or_else(|| crate::i18n::tr("runtime.none").into())
  }
}

impl GameRuntime for StandaloneRuntime {
  fn info(&self) -> RuntimeInfo {
    let mut kept = self.info.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, info)) = kept.as_ref()
      && at.elapsed() < Duration::from_secs(2)
    {
      return info.clone();
    }
    let info = Self::current().map_or_else(
      || RuntimeInfo {
        reason: "runtime.none".into(),
        games: vec!["jak3".into()],
        ..Default::default()
      },
      |r| OpenGoal::info(&r, "jak3"),
    );
    *kept = Some((Instant::now(), info.clone()));
    info
  }

  fn install_level(&self, package: &LevelPackage, events: HostEvents) -> Result<(), String> {
    self.used()?.install_level(package, events)
  }

  fn remove_level(&self, game: &str, name: &str, events: HostEvents) -> Result<(), String> {
    self.used()?.remove_level(game, name, events)
  }

  fn launch(&self, game: &str, name: &str, events: HostEvents) -> Result<(), String> {
    self.used()?.launch(game, name, events)
  }

  fn settings(&self) -> Value {
    // read again: the settings window changed them (runtime_settings tells the interface)
    *self.info.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let (checkout, launcher) = Self::candidates();
    let describe = |o: &OpenGoal| {
      json!({
        "kind": o.kind,
        "folder": o.folder,
        "available": o.problem("jak3").is_none(),
        "reason": o.problem("jak3").unwrap_or_default(),
        "builder": o.builder("jak3"),
      })
    };
    // (current() reads the preferences: not while they are held)
    let used = Self::current().map(|o| o.kind);
    let (folder, bin) = {
      let p = Prefs::get();
      (p.runtime_folder.clone(), p.runtime_bin.clone())
    };
    json!({
      "folder": folder,
      "bin": bin,
      "checkout": checkout.as_ref().map(describe),
      "launcher": launcher.as_ref().map(describe),
      "used": used,
      "marker": BUILDER_MARKER,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::test_library::temp_dir;

  #[test]
  fn command_lines() {
    assert_eq!(
      strip_ansi("[\x1b[38;2;144;238;144minfo\x1b[0m] x"),
      "[info] x"
    );
    let tmp = temp_dir("runtime");
    let mut c = OpenGoal::checkout(&tmp, None, "jak3");
    c.iso_data = tmp.join("iso_data/jak3");
    assert_eq!(c.problem("jak3"), Some("runtime.no_sources"));
    let gp = tmp.join("goal_src/jak3/game.gp");
    std::fs::create_dir_all(gp.parent().unwrap()).unwrap();
    std::fs::write(&gp, "").unwrap();
    assert_eq!(c.problem("jak3"), Some("runtime.no_programs"));
    let bin = tmp.join("out/build/Release/bin");
    std::fs::create_dir_all(&bin).unwrap();
    for p in ["gk", "goalc", "extractor"] {
      std::fs::write(exe(&bin, p), "").unwrap();
    }
    assert_eq!(c.problem("jak3"), Some("runtime.old_builder"));
    std::fs::create_dir_all(tmp.join("custom_assets/jak3")).unwrap();
    std::fs::write(
      tmp.join("custom_assets/jak3").join(BUILDER_MARKER),
      r#"{"builder": "jle-1"}"#,
    )
    .unwrap();
    assert_eq!(c.problem("jak3"), Some("runtime.no_game_files"));
    // an empty iso_data/jak3 (a checkout's before the extraction) is not the game
    std::fs::create_dir_all(tmp.join("iso_data/jak3")).unwrap();
    assert_eq!(c.problem("jak3"), Some("runtime.no_game_files"));
    std::fs::create_dir_all(tmp.join("iso_data/jak3/CGO")).unwrap();
    assert_eq!(c.problem("jak3"), None);
    let args = |cmd: &Command| -> Vec<String> {
      cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
    };
    // always with the data folder: the programs would look for "jak-project" in their path
    let root = tmp.to_string_lossy().into_owned();
    let compile = args(&c.compile_command("jak3"));
    assert!(
      compile[1..6] == ["--folder", "--compile", "--game", "jak3", "--proj-path"]
        && compile[6] == root,
      "{compile:?}"
    );
    let launch = args(&c.launch_command("jak3"));
    assert!(
      launch.contains(&"--config-path".into())
        && launch.contains(&root)
        && launch.ends_with(&["-debug".into()]),
      "{launch:?}"
    );
    let repl = args(&c.repl_command("jak3", 8193, 8194));
    assert!(
      repl.ends_with(&["--proj-path".into(), root.clone()])
        && repl.windows(2).any(|w| w == ["--user", "jle"])
        && repl.windows(2).any(|w| w == ["-p", "8193"]),
      "{repl:?}"
    );
    assert_eq!(
      c.startup_file("jak3"),
      tmp.join("goal_src/user/jle/startup-jak3.gc")
    );
    let code = startup_code("my-test-start");
    assert!(
      code.starts_with("(make-group \"all-code\")\n(lt)\n;; og:run-below-on-listen\n")
        && code.ends_with("(start 'play (get-continue-by-name *game-info* \"my-test-start\"))\n"),
      "{code}"
    );
    // a distribution: its data/, its settings
    let d = OpenGoal::distribution("launcher", &tmp.join("le"), &tmp.join("iso/jak3"));
    assert!(
      d.data_dir == tmp.join("le/data")
        && d.config_dir == tmp.join("le/_settings")
        && d.extractor == exe(&tmp.join("le"), "extractor"),
      "{d:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
  }

  /// the standalone runtime answers (it reads the preferences more than once)
  #[test]
  fn standalone_settings() {
    let r = StandaloneRuntime::default();
    let s = r.settings();
    assert!(s.is_object() && s["marker"] == BUILDER_MARKER, "{s}");
    assert_eq!(r.info().games, ["jak3"]);
  }
}
