//! The game runtimes: an OpenGOAL that installs a level exported for the game, builds it and starts
//! the game on it (host::GameRuntime), and the standalone app's runtime, which uses the one the user
//! chose in the settings.
//!
//! `OpenGoal` is an OpenGOAL the editor can drive: a jak-project checkout (its goal_src and
//! custom_assets, its build's goalc and gk) or an OpenGOAL distribution (`extractor`, `gk` and
//! `data/`, as the OpenGOAL Launcher installs them), possibly run with the programs of another
//! one: the launcher's isolated game for the editor runs those of the launcher's active tooling
//! version, its data/ made from that version's (`launcher_level_edited`, `prepare`). The OpenGOAL
//! Launcher uses the same pieces (`launcher_level_edited`, `prepare`, `decompile_command`,
//! `compile_command`, `apply_level`, `launch`) for its own runtime. The launcher's game plays with
//! the releases of whozghiar/jak-project only (`is_fork_release`), whose level builder has the
//! fixes of the editor's exports; a checkout the user chose is used as it is.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::host::{GameRuntime, HostEvent, HostEvents, LevelPackage, RuntimeInfo};
use crate::io::level_package;
use crate::prefs::Prefs;

/// the suffix of the tags of whozghiar/jak-project's releases (`v0.3.9-whozghiar.1`: upstream's
/// 0.3.9, the fork's first build of it, with the level builder's fixes for the editor's exports)
pub const FORK_SUFFIX: &str = "-whozghiar.";

/// A tooling version the editor's levels can be played with: a release of whozghiar/jak-project
pub fn is_fork_release(version: &str) -> bool {
  version.contains(FORK_SUFFIX)
}

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
  /// the launcher's isolated game: the tooling version whose programs it runs and whose data/ its
  /// own is made from (`prepare`)
  pub tooling: Option<Tooling>,
}

/// A tooling version of the OpenGOAL Launcher (`<install>/versions/official/<version>/`: its
/// programs and its data/)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tooling {
  pub version: String,
  pub dir: PathBuf,
}

/// What the launcher's isolated game was made from (`jle-runtime.json` in its folder): the tooling
/// version its data/ was copied from, and whether the disc was decompiled into it since
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeState {
  pub version: String,
  pub decompiled: bool,
}

impl RuntimeState {
  pub const FILE: &'static str = "jle-runtime.json";

  pub fn read(folder: &Path) -> RuntimeState {
    std::fs::read(folder.join(Self::FILE))
      .ok()
      .and_then(|b| serde_json::from_slice(&b).ok())
      .unwrap_or_default()
  }

  pub fn write(&self, folder: &Path) -> Result<(), String> {
    let path = folder.join(Self::FILE);
    let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
  }
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
  if let Some(dir) = to.parent() {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  }
  std::fs::copy(from, to)
    .map(|_| ())
    .map_err(|e| format!("{}: {e}", from.display()))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
  let entries = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
  std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
  for entry in entries {
    let entry = entry.map_err(|e| e.to_string())?;
    let (src, dst) = (entry.path(), to.join(entry.file_name()));
    if src.is_dir() {
      copy_dir(&src, &dst)?;
    } else {
      copy_file(&src, &dst)?;
    }
  }
  Ok(())
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
      tooling: None,
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
      tooling: None,
    }
  }

  /// Run with the programs of the tooling version `tooling`, its data/ made from that version's
  /// (`prepare`)
  pub fn with_tooling(mut self, tooling: Tooling) -> OpenGoal {
    self.gk = exe(&tooling.dir, "gk");
    self.goalc = exe(&tooling.dir, "goalc");
    self.extractor = exe(&tooling.dir, "extractor");
    self.tooling = Some(tooling);
    self
  }

  /// Why it cannot play a level of `game` now (a translation key), None when it can
  pub fn problem(&self, game: &str) -> Option<&'static str> {
    if let Some(t) = &self.tooling {
      // its data/ is made from its version's by prepare
      if !is_fork_release(&t.version)
        || ![&self.gk, &self.goalc, &self.extractor]
          .iter()
          .all(|p| p.is_file())
      {
        return Some("runtime.launcher_version");
      }
      if !has_game_files(&self.iso_data) {
        return Some("runtime.no_game_files");
      }
      return None;
    }
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

  /// The game's files decompiled into its data folder (`extractor --folder --decompile`): what the
  /// launcher's game needs once per tooling version, before its first build
  pub fn decompile_command(&self, game: &str) -> Command {
    let mut cmd = Command::new(&self.extractor);
    cmd
      .arg(&self.iso_data)
      .args(["--folder", "--decompile", "--game", game, "--proj-path"])
      .arg(&self.data_dir)
      .current_dir(self.extractor.parent().unwrap_or(&self.folder));
    cmd
  }

  /// where the launcher's game keeps a copy of each level installed in it, to install it again in
  /// the data of another tooling version
  pub fn packages_dir(&self) -> PathBuf {
    self.folder.join("packages")
  }

  /// the copies kept (packages/<name>/), by name
  fn packages(&self) -> Vec<PathBuf> {
    let mut packages: Vec<PathBuf> = std::fs::read_dir(self.packages_dir())
      .into_iter()
      .flatten()
      .flatten()
      .map(|e| e.path())
      .filter(|p| p.join(LevelPackage::MANIFEST).is_file())
      .collect();
    packages.sort();
    packages
  }

  /// The launcher's game must be made again from its tooling version: its data/ was made from
  /// another one, or not yet
  pub fn stale(&self) -> bool {
    self.tooling.as_ref().is_some_and(|t| {
      RuntimeState::read(&self.folder).version != t.version
        || !self.data_dir.join("goal_src").is_dir()
    })
  }

  /// The launcher's game made from its tooling version when it is stale: data/ copied again from
  /// the version's (its _settings/, the saves, kept), to be decompiled again, and the levels
  /// installed before installed again from their copies. Returns what it is made from.
  pub fn prepare(&self) -> Result<RuntimeState, String> {
    let Some(t) = &self.tooling else {
      return Ok(RuntimeState {
        decompiled: true,
        ..Default::default()
      });
    };
    if !self.stale() {
      return Ok(RuntimeState::read(&self.folder));
    }
    // the game started before keeps its data's files open
    stop_game();
    if self.data_dir.exists() {
      std::fs::remove_dir_all(&self.data_dir)
        .map_err(|e| format!("{}: {e}", self.data_dir.display()))?;
    }
    copy_dir(&t.dir.join("data"), &self.data_dir)?;
    // the programs of the game's first versions, when it had its own
    for p in ["gk", "goalc", "extractor"] {
      let _ = std::fs::remove_file(exe(&self.folder, p));
    }
    for dir in self.packages() {
      level_package::apply(&self.data_dir, &LevelPackage::read(&dir)?)?;
    }
    // written last: a preparation stopped halfway starts again
    let state = RuntimeState {
      version: t.version.clone(),
      decompiled: false,
    };
    state.write(&self.folder)?;
    Ok(state)
  }

  /// Installs the level in its data folder (level_package::apply); the launcher's game keeps a
  /// copy of it (packages/<name>/)
  pub fn apply_level(&self, package: &LevelPackage) -> Result<(), String> {
    level_package::apply(&self.data_dir, package)?;
    let copy = self.packages_dir().join(&package.manifest.name);
    if self.tooling.is_none() || copy == package.dir {
      return Ok(());
    }
    let _ = std::fs::remove_dir_all(&copy);
    let files = std::iter::once(LevelPackage::MANIFEST)
      .chain(package.manifest.files.iter().map(String::as_str))
      .chain((!package.manifest.level_info.is_empty()).then_some(&*package.manifest.level_info));
    for f in files {
      if !Path::new(f)
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
      {
        return Err(format!("jle-level.json: a file outside the export ({f})"));
      }
      copy_file(&package.dir.join(f), &copy.join(f))?;
    }
    Ok(())
  }

  /// Takes the level out of its data folder (level_package::remove), and its copy
  pub fn remove_level_files(&self, game: &str, name: &str) -> Result<(), String> {
    let copy = self.packages_dir().join(name);
    let had_copy = copy.is_dir();
    let _ = std::fs::remove_dir_all(&copy);
    let installed = level_package::installed(&self.data_dir)
      .iter()
      .any(|l| l.game == game && l.name == name);
    if had_copy && !installed {
      return Ok(());
    }
    level_package::remove(&self.data_dir, game, name)
  }

  /// The launcher's game ready to build: prepared, the disc decompiled when needed
  fn ready(&self, game: &str, events: &HostEvents) -> Result<(), String> {
    let Some(t) = &self.tooling else {
      return Ok(());
    };
    if self.stale() {
      step(events, "runtime.step.prepare", &[&t.version]);
    }
    let mut state = self.prepare()?;
    if !state.decompiled {
      step(events, "runtime.step.decompile", &[]);
      run_logged(self.decompile_command(game), events)?;
      state.decompiled = true;
      state.write(&self.folder)?;
    }
    Ok(())
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
      // the launcher's game of another version: the levels prepare installs again
      installed: if self.stale() {
        self
          .packages()
          .iter()
          .filter_map(|p| Some(p.file_name()?.to_string_lossy().into_owned()))
          .collect()
      } else {
        level_package::installed(&self.data_dir)
          .into_iter()
          .filter(|l| l.game == game)
          .map(|l| l.name)
          .collect()
      },
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
    self.ready(&m.game, &events)?;
    step(&events, "runtime.step.install", &[&m.name]);
    self.apply_level(package)?;
    self.compile(&m.game, &events)
  }

  fn remove_level(&self, game: &str, name: &str, events: HostEvents) -> Result<(), String> {
    step(&events, "runtime.step.remove", &[name]);
    self.remove_level_files(game, name)?;
    self.ready(game, &events)?;
    self.compile(game, &events)
  }

  fn launch(&self, game: &str, name: &str, events: HostEvents) -> Result<(), String> {
    // the launcher's game of another tooling version: made again, built, then started
    if self.stale() {
      self.ready(game, &events)?;
      self.compile(game, &events)?;
    }
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

/// A text of the OpenGOAL Launcher's settings.json
fn launcher_setting(key: &str) -> Option<String> {
  let appdata = std::env::var_os("APPDATA")
    .map(PathBuf::from)
    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
  let text = std::fs::read(appdata.join("OpenGOAL-Launcher/settings.json")).ok()?;
  let v: Value = serde_json::from_slice(&text).ok()?;
  v[key]
    .as_str()
    .filter(|d| !d.is_empty())
    .map(str::to_string)
}

/// The OpenGOAL Launcher's installation folder (its settings.json's installationDir)
pub fn launcher_install_dir() -> Option<PathBuf> {
  launcher_setting("installationDir").map(PathBuf::from)
}

/// The OpenGOAL Launcher's active tooling version (its settings.json's activeVersion: "v0.3.9")
pub fn launcher_active_version() -> Option<String> {
  launcher_setting("activeVersion")
}

/// The OpenGOAL Launcher's isolated game for the editor: `<install>/features/<game>/level_edited/`
/// (its data/, _settings/, packages/), run with the programs of the launcher's tooling version
/// `version` (`<install>/versions/official/<version>/`, its data/ made from that version's) and
/// built from the disc the launcher extracted (`<install>/active/<game>/data/iso_data/<game>`)
pub fn launcher_level_edited(install: &Path, version: &str, game: &str) -> OpenGoal {
  OpenGoal::distribution(
    "launcher",
    &install.join("features").join(game).join("level_edited"),
    &launcher_iso_data(install, game),
  )
  .with_tooling(Tooling {
    version: version.into(),
    dir: install.join("versions").join("official").join(version),
  })
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
    let launcher = launcher_install_dir()
      .map(|d| launcher_level_edited(&d, &launcher_active_version().unwrap_or_default(), "jak3"));
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
    // a checkout the user chose: used as it is
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

  /// the launcher's game follows its tooling version: made from the version's data, its levels
  /// installed again in another version's, its saves kept
  #[test]
  fn launcher_game_follows_the_tooling_version() {
    use crate::io::level_package::tests::{fake_data_dir, package};
    let tmp = temp_dir("launcher-game");
    let install = tmp.join("install");
    let version = |v: &str| {
      let dir = install.join("versions/official").join(v);
      fake_data_dir(&dir.join("data"));
      for p in ["gk", "goalc", "extractor"] {
        std::fs::write(exe(&dir, p), "").unwrap();
      }
      dir
    };
    // upstream's releases: not for the editor's levels
    assert!(
      is_fork_release("v0.3.9-whozghiar.1")
        && !is_fork_release("v0.3.9")
        && !is_fork_release("v0.3.9-jle.1")
    );
    version("v0.3.9");
    assert_eq!(
      launcher_level_edited(&install, "v0.3.9", "jak3").problem("jak3"),
      Some("runtime.launcher_version")
    );
    let og = launcher_level_edited(&install, "v0.3.9-whozghiar.1", "jak3");
    assert_eq!(og.problem("jak3"), Some("runtime.launcher_version"));
    let v1 = version("v0.3.9-whozghiar.1");
    let folder = install.join("features/jak3/level_edited");
    assert!(
      og.gk == exe(&v1, "gk") && og.data_dir == folder.join("data") && og.folder == folder,
      "{og:?}"
    );
    assert_eq!(og.problem("jak3"), Some("runtime.no_game_files"));
    std::fs::create_dir_all(install.join("active/jak3/data/iso_data/jak3/CGO")).unwrap();
    assert_eq!(og.problem("jak3"), None);
    assert_eq!(
      launcher_level_edited(&install, "", "jak3").problem("jak3"),
      Some("runtime.launcher_version")
    );

    // made from the version's data, to decompile
    assert!(og.stale());
    let s = og.prepare().unwrap();
    assert!(
      s.version == "v0.3.9-whozghiar.1"
        && !s.decompiled
        && !og.stale()
        && folder.join("data/goal_src/jak3/game.gp").is_file()
        && RuntimeState::read(&folder) == s,
      "{s:?}"
    );
    let decompile: Vec<String> = og
      .decompile_command("jak3")
      .get_args()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();
    assert!(
      decompile[1..6] == ["--folder", "--decompile", "--game", "jak3", "--proj-path"]
        && Path::new(&decompile[6]) == folder.join("data"),
      "{decompile:?}"
    );
    RuntimeState {
      decompiled: true,
      ..s
    }
    .write(&folder)
    .unwrap();
    assert!(og.prepare().unwrap().decompiled, "prepared once");

    // a level installed, and its copy kept
    let p = package(&tmp.join("export"), "my-test", "", "v1");
    std::fs::write(
      p.dir.join(LevelPackage::MANIFEST),
      serde_json::to_string(&p.manifest).unwrap(),
    )
    .unwrap();
    og.apply_level(&p).unwrap();
    let copy = folder.join("packages/my-test");
    assert!(
      copy.join(LevelPackage::MANIFEST).is_file()
        && copy.join(&p.manifest.files[0]).is_file()
        && copy.join(&p.manifest.level_info).is_file(),
      "the copy"
    );
    std::fs::create_dir_all(&og.config_dir).unwrap();
    std::fs::write(og.config_dir.join("save.bin"), "save").unwrap();
    std::fs::write(folder.join("data/old.txt"), "").unwrap();

    // another version: data/ made again, the level installed again, to decompile again
    version("v0.3.9-whozghiar.2");
    let og2 = launcher_level_edited(&install, "v0.3.9-whozghiar.2", "jak3");
    assert!(og2.stale());
    // the levels it will have, even with its data/ half gone
    std::fs::remove_file(folder.join("data").join(level_package::RECORD)).unwrap();
    assert_eq!(og2.info("jak3").installed, ["my-test"]);
    let s2 = og2.prepare().unwrap();
    let names = |og: &OpenGoal| -> Vec<String> {
      level_package::installed(&og.data_dir)
        .into_iter()
        .map(|l| l.name)
        .collect()
    };
    assert!(
      s2.version == "v0.3.9-whozghiar.2"
        && !s2.decompiled
        && names(&og2) == ["my-test"]
        && folder
          .join("data/custom_assets/jak3/levels/my-test/my-test.glb")
          .is_file()
        && !folder.join("data/old.txt").exists()
        && og2.config_dir.join("save.bin").is_file(),
      "{s2:?}"
    );

    // removed, its copy too
    og2.remove_level_files("jak3", "my-test").unwrap();
    assert!(!copy.exists() && names(&og2).is_empty());
    assert!(og2.remove_level_files("jak3", "my-test").is_err());
    let _ = std::fs::remove_dir_all(&tmp);
  }

  /// the standalone runtime answers (it reads the preferences more than once)
  #[test]
  fn standalone_settings() {
    let r = StandaloneRuntime::default();
    let s = r.settings();
    assert!(s.is_object() && s.get("used").is_some(), "{s}");
    assert_eq!(r.info().games, ["jak3"]);
  }
}
