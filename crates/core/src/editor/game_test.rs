//! Level > Try in the game (F5): the level, built, played in the game the user installed, without
//! installing or compiling the game. Calls (api.rs): game_installs, set_game_install,
//! test_in_game, game_test_state, stop_game_test.
//!
//! Only the game's official programs run: gk and goalc of the installation; the editor gives them
//! their inputs and drives them. A try, on a worker (job "game_test"):
//!   1. the document exported as the level "jle-test" (DGO JLT.DGO) to <prefs>/game-test/<game>/work/
//!   2. goalc, started once and kept, learns the game's types by compiling its engine (about 20 s),
//!      in a project of its own, <prefs>/game-test/<game>/goalc/: links to the installation's
//!      sources, disc and art, its own out/, a copy of the decompiler's configuration whose
//!      inputs.jsonc lists only the DGOs holding the level's art groups (the builder reads no other)
//!   3. goalc builds the level as the game's project builds a custom level (game.gp's
//!      build-custom-level and custom-level-cgo, the level linked into the project's custom_assets),
//!      its code compiled first (the behaviors set in the editor)
//!   4. the DGO and the .fr3 put next to the game's files: the only files written into the installed
//!      game, taken out again when the try stops
//!   5. gk started (again: a level after another in one gk can show nothing), goalc connected to it,
//!      the level declared to the running game (its level-load-info, as the export writes it for
//!      level-info.gc) and the player started at its start point
//!
//! gk listens on port 8114 for Jak 3 whatever its --port says: another game holding it would get
//! the commands, so a try refuses to start then.

use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::export::ExportResult;
use super::{Editor, Job, Notify};
use crate::i18n::tr;
use crate::io::game_install::{
  GameInstall, art_group_dgos, copy_folder, find_extraction, find_install, game_version,
  link_folder, read_jsonc,
};
use crate::prefs::Prefs;
use crate::{log_error, log_info, strf};

/// the level a try builds: a name of the game's (10 letters at most), its DGO's nickname
const NAME: &str = "jle-test";
const NICKNAME: &str = "jlt";
/// the games whose levels can be tried (the editor edits Jak 3)
const GAMES: [&str; 3] = ["jak1", "jak2", "jak3"];
const TRIED: &str = "jak3";
/// gk's listener for Jak 3 (game/runtime.cpp: its --port is not read)
const GAME_PORT: u16 = 8114;
const STEPS: usize = 6;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
  m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The programs the tries started and did not stop yet, by process id and name: stopped even while
/// a try holds them (the window closed during a try)
type Started = Arc<Mutex<Vec<(u32, &'static str)>>>;

/// A program the try started, its output (stdout and stderr) gathered by a thread
struct Program {
  child: Child,
  /// where it is listed until it stops
  started: Option<Started>,
  /// kept open: goalc quits at the end of its input
  _stdin: Option<ChildStdin>,
  out: Arc<Mutex<String>>,
}

impl Program {
  fn start(mut cmd: Command) -> Result<Self, String> {
    #[cfg(windows)]
    {
      use std::os::windows::process::CommandExt;
      cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console of its own (gk opens its window)
    }
    let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    cmd
      .stdin(Stdio::piped())
      .stdout(writer.try_clone().map_err(|e| e.to_string())?)
      .stderr(writer);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    drop(cmd);
    let out = Arc::new(Mutex::new(String::new()));
    let o = out.clone();
    std::thread::spawn(move || {
      let mut r = BufReader::new(reader);
      let mut buf = [0u8; 8192];
      while let Ok(n) = r.read(&mut buf) {
        if n == 0 {
          break;
        }
        let mut o = lock(&o);
        o.push_str(&strip_ansi(&String::from_utf8_lossy(&buf[..n])));
        // the last megabytes are enough
        if o.len() > 4 << 20 {
          let cut = o.len() - (2 << 20);
          let cut = (cut..o.len()).find(|&i| o.is_char_boundary(i)).unwrap_or(0);
          o.drain(..cut);
        }
      }
    });
    Ok(Self {
      started: None,
      _stdin: child.stdin.take(),
      child,
      out,
    })
  }

  fn alive(&mut self) -> bool {
    matches!(self.child.try_wait(), Ok(None))
  }

  fn len(&self) -> usize {
    lock(&self.out).len()
  }

  fn since(&self, mark: usize) -> String {
    let o = lock(&self.out);
    o.get(mark.min(o.len())..).unwrap_or_default().to_string()
  }

  /// Waits until its output after `mark` holds one of `texts`: which one, else None (timeout, or
  /// it exited)
  fn wait_for(&mut self, mark: usize, texts: &[&str], timeout: Duration) -> Option<usize> {
    let end = Instant::now() + timeout;
    loop {
      let since = self.since(mark);
      if let Some(i) = texts.iter().position(|t| since.contains(t)) {
        return Some(i);
      }
      if Instant::now() > end || !self.alive() {
        return None;
      }
      std::thread::sleep(Duration::from_millis(50));
    }
  }

  fn kill(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
    if let Some(s) = &self.started {
      let pid = self.child.id();
      lock(s).retain(|(p, _)| *p != pid);
    }
  }
}

// the colors of a terminal taken out (goalc writes them whatever its options)
fn strip_ansi(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  let mut chars = s.chars().peekable();
  while let Some(c) = chars.next() {
    if c != '\x1b' {
      out.push(c);
      continue;
    }
    if chars.peek() == Some(&'[') {
      chars.next();
      for c in chars.by_ref() {
        if c.is_ascii_alphabetic() {
          break;
        }
      }
    }
  }
  out
}

/// goalc, kept between tries: it learned the game's types once
struct Goalc {
  program: Program,
  /// its nREPL port (the forms it compiles come there)
  port: u16,
  /// the installation it compiles for
  data: String,
  /// the forms sent: each one's mark of the end
  evals: u64,
}

impl Goalc {
  /// Compiles `form` (run by the game when connected): what goalc printed for it, or the error it
  /// printed. Its prompt comes before its answers: a mark printed by goalc itself after the form
  /// (sent behind it: goos' `fmt`, flushed, unlike its `print`) says it is done.
  fn eval(&mut self, form: &str, timeout: Duration) -> Result<String, String> {
    self.evals += 1;
    let done = format!("jle-done-{}", self.evals);
    let mark = self.program.len();
    let mut s = TcpStream::connect(("127.0.0.1", self.port)).map_err(|e| e.to_string())?;
    for body in [form.to_string(), format!("(seval (fmt #t \"{done}\n\"))")] {
      let mut msg = (body.len() as u32).to_le_bytes().to_vec();
      msg.extend_from_slice(&10u32.to_le_bytes()); // an expression to evaluate
      msg.extend_from_slice(body.as_bytes());
      s.write_all(&msg).map_err(|e| e.to_string())?;
    }
    if self.program.wait_for(mark, &[&done], timeout).is_none() {
      return Err(strf!(
        tr("game_test.error.compiler_silent"),
        form_head(form)
      ));
    }
    drop(s);
    let answer = self.program.since(mark);
    let answer = answer.split(&done).next().unwrap_or_default().to_string();
    for bad in [
      "REPL Error",
      "Compilation Error",
      "-- Compilation Error! --",
    ] {
      if answer.contains(bad) {
        return Err(strf!(
          tr("game_test.error.compile"),
          form_head(form),
          last_lines(&answer, 8)
        ));
      }
    }
    Ok(answer)
  }

  /// The value of `form` in the game, as goalc prints it after its prompt ("gc> 1        #x1 ..."):
  /// its first word
  fn value(&mut self, form: &str) -> Result<String, String> {
    let a = self.eval(form, Duration::from_secs(20))?;
    Ok(
      a.lines()
        .filter_map(|l| l.trim_start().strip_prefix("gc> "))
        .filter_map(|l| l.split_whitespace().next())
        .next_back()
        .unwrap_or_default()
        .to_string(),
    )
  }

  fn connected(&self) -> bool {
    let o = lock(&self.program.out);
    o.rfind("gc> ") > o.rfind("g > ")
  }
}

fn form_head(form: &str) -> String {
  form.chars().take(60).collect()
}

fn last_lines(text: &str, n: usize) -> String {
  let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
  lines[lines.len().saturating_sub(n)..].join("\n")
}

/// a port of this computer no one listens on
fn free_port() -> u16 {
  TcpListener::bind(("127.0.0.1", 0))
    .and_then(|l| l.local_addr())
    .map(|a| a.port())
    .unwrap_or(0)
}

fn port_taken(port: u16) -> bool {
  TcpListener::bind(("0.0.0.0", port)).is_err() || TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// What a try keeps between tries: goalc, the game started, the files put into the installed game
#[derive(Default)]
struct Session {
  goalc: Option<Goalc>,
  gk: Option<Program>,
  files: Vec<PathBuf>,
}

/// Kills the process `pid` if it is still `name` (a program a try started: its id may have gone to
/// another program since)
fn kill_started(pid: u32, name: &str) {
  let mut cmd = if cfg!(windows) {
    let mut c = Command::new("taskkill");
    c.args(["/F", "/FI"])
      .arg(format!("PID eq {pid}"))
      .arg("/FI")
      .arg(format!(
        "IMAGENAME eq {name}{}",
        std::env::consts::EXE_SUFFIX
      ));
    c
  } else {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    if comm.trim() != name {
      return;
    }
    let mut c = Command::new("kill");
    c.args(["-9", &pid.to_string()]);
    c
  };
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000);
  }
  let _ = cmd.output();
}

impl Session {
  fn stop_game(&mut self) {
    if let Some(mut gk) = self.gk.take() {
      gk.kill();
    }
  }

  fn stop(&mut self) {
    self.stop_game();
    if let Some(mut g) = self.goalc.take() {
      // killed, not quit: goalc quitting resets the game it is connected to
      g.program.kill();
    }
    for f in self.files.drain(..) {
      let _ = std::fs::remove_file(f);
    }
  }
}

/// The tries of the editor (Editor::game_test)
#[derive(Default)]
pub struct GameTestState {
  session: Arc<Mutex<Session>>,
  /// the last try: the step it is at, why it failed, how long it took
  last: Arc<Mutex<Value>>,
  /// gk's version by its path, asked once
  versions: Arc<Mutex<std::collections::BTreeMap<String, String>>>,
  started: Started,
}

impl GameTestState {
  /// The game and goalc stopped, the files taken out, those a try is using too
  fn stop(&self) {
    lock(&self.session).stop();
    for (pid, name) in lock(&self.started).drain(..) {
      kill_started(pid, name);
    }
  }
}

impl Drop for GameTestState {
  fn drop(&mut self) {
    self.stop();
  }
}

/// Where a try works, by game: <prefs>/game-test/<game>/
fn test_dir(game: &str) -> PathBuf {
  PathBuf::from(Prefs::dir()).join("game-test").join(game)
}

impl Editor {
  /// The installation of `game` levels are tried in: the host's, else the one of the settings;
  /// `(folder, from the host, the installation or why not)`
  fn game_install(&self, game: &str) -> (String, bool, Result<GameInstall, &'static str>) {
    let (folder, from_host) = self.game_folder(game);
    if folder.is_empty() {
      return (folder, from_host, Err("game_install.error.none"));
    }
    let found = find_install(game, Path::new(&folder)).map(|mut i| {
      i.version = self.gk_version(&i.gk);
      i
    });
    (folder, from_host, found)
  }

  /// The folder of `game`'s installation (its levels extracted from it, tried in it): the host's,
  /// else the settings'; and whether the host gave it
  pub(crate) fn game_folder(&self, game: &str) -> (String, bool) {
    match self.host.game_installs.get(game) {
      Some(f) => (f.to_string_lossy().into_owned(), true),
      None => (
        Prefs::get()
          .game_installs
          .get(game)
          .cloned()
          .unwrap_or_default(),
        false,
      ),
    }
  }

  /// gk's version ("v0.3.9"), asked once
  pub(crate) fn gk_version(&self, gk: &str) -> String {
    lock(&self.game_test.versions)
      .entry(gk.to_string())
      .or_insert_with(|| game_version(gk))
      .clone()
  }

  /// The settings' list (game_installs): `[{game, folder, from_host, install, error,
  /// extract_error}]`: the installation to try levels in or why not, why its levels cannot be
  /// extracted (keys of lang/*.json, "" when they can)
  pub fn game_installs(&self) -> Value {
    let list: Vec<Value> = GAMES
      .iter()
      .map(|g| {
        let (folder, from_host, found) = self.game_install(g);
        let extract_error = if folder.is_empty() {
          "game_install.error.none"
        } else {
          find_extraction(g, Path::new(&folder))
            .err()
            .unwrap_or_default()
        };
        json!({
          "game": g,
          "folder": folder,
          "from_host": from_host,
          "install": found.as_ref().ok(),
          "error": found.err().unwrap_or_default(),
          "extract_error": extract_error,
        })
      })
      .collect();
    json!(list)
  }

  /// The settings' folder of `game` (set_game_install; "" forgets it); the host's comes first
  pub fn set_game_install(&mut self, game: &str, folder: &str) -> Result<Value, String> {
    if !GAMES.contains(&game) {
      return Err(format!("unknown game {game}"));
    }
    if self.host.game_installs.contains_key(game) {
      return Err(tr("game_install.from_host").into());
    }
    self.save_prefs(|p| {
      if folder.is_empty() {
        p.game_installs.remove(game);
      } else {
        p.game_installs.insert(game.into(), folder.into());
      }
    });
    Ok(self.game_installs())
  }

  /// What the toolbar and the menu show (game_test_state): `available` (a try can start), `reason`
  /// (why not: a key), `busy`, `running` (the game of the last try is open), `last` {step, error,
  /// seconds, done}
  pub fn game_test_state(&self) -> Value {
    let (_, _, found) = self.game_install(TRIED);
    let reason = if !self.document_open() {
      "game_test.reason.no_document"
    } else if let Err(e) = &found {
      e
    } else {
      ""
    };
    let running = lock(&self.game_test.session)
      .gk
      .as_mut()
      .is_some_and(Program::alive);
    json!({
      "available": reason.is_empty(),
      "reason": reason,
      "busy": self.busy("game_test"),
      "running": running,
      "last": *lock(&self.game_test.last),
    })
  }

  /// Stops the game of the tries and goalc, takes their files out of the installed game
  /// (stop_game_test; the interface calls it when the window closes)
  pub fn stop_game_test(&mut self) {
    self.game_test.stop();
  }

  /// Level > Try in the game (test_in_game): a try on a worker, its steps in jle:progress and
  /// game_test_state
  pub fn test_in_game(&mut self) -> Result<(), String> {
    if self.busy("game_test") {
      return Err(tr("game_test.busy").into());
    }
    let (_, _, found) = self.game_install(TRIED);
    let install = found.map_err(|e| tr(e).to_string())?;
    // another game listening where goalc connects: told now, nothing written
    let ours = lock(&self.game_test.session)
      .gk
      .as_mut()
      .is_some_and(Program::alive);
    if !ours && port_taken(GAME_PORT) {
      return Err(strf!(tr("game_test.error.port_taken"), GAME_PORT));
    }
    // the document exported as the level of the tries, the Export window's options kept
    let dir = test_dir(TRIED);
    let work = dir.join("work");
    let mut o = self.default_export_options();
    o.name = NAME.into();
    o.nickname = NICKNAME.into();
    o.iso_name = "JLETEST".into();
    o.replaces.clear();
    o.region_trees = Value::Null;
    o.folder = work.to_string_lossy().into_owned();
    o.start = self.export_start_point();
    let kept = (self.export.options.clone(), self.export.level.clone());
    let export = self.export_work(o);
    (self.export.options, self.export.level) = kept;
    let export = export?;
    let _ = std::fs::remove_dir_all(work.join("custom_assets"));
    let _ = std::fs::remove_dir_all(work.join("goal_src"));
    *lock(&self.game_test.last) = json!({"step": "game_test.step.export", "done": false});
    let try_ = Try {
      install,
      dir,
      session: self.game_test.session.clone(),
      programs: self.game_test.started.clone(),
      last: self.game_test.last.clone(),
      notify: self.notify.clone(),
      started: Instant::now(),
    };
    self.run_job(
      "game_test",
      tr("game_test.title").into(),
      move || try_.run(export),
      |ed, (result, read): TryResult| {
        ed.export.originals.extend(read);
        let mut last = lock(&ed.game_test.last);
        match result {
          Ok(seconds) => {
            log_info!(tr("log.game_test_done"), format!("{seconds:.0}"));
            *last = json!({"step": "game_test.step.done", "seconds": seconds, "done": true});
          }
          Err(e) => {
            log_error!(tr("log.game_test_failed"), &e);
            last["error"] = json!(e);
            last["done"] = json!(true);
          }
        }
      },
    );
    Ok(())
  }
}

/// What a try gives back: its time or why it failed, the levels the export read
type TryResult = (Result<f64, String>, Vec<(String, Arc<crate::scene::Scene>)>);

/// A try, on its worker
struct Try {
  install: GameInstall,
  dir: PathBuf,
  session: Arc<Mutex<Session>>,
  programs: Started,
  last: Arc<Mutex<Value>>,
  notify: Option<Notify>,
  started: Instant,
}

impl Try {
  fn started(&self, p: &mut Program, name: &'static str) {
    lock(&self.programs).push((p.child.id(), name));
    p.started = Some(self.programs.clone());
  }

  fn step(&self, n: usize, key: &str) {
    *lock(&self.last) = json!({"step": key, "done": false});
    if let Some(notify) = &self.notify {
      let job = Job {
        id: "game_test".into(),
        label: format!("{}: {}", tr("game_test.title"), tr(key)),
        done: n,
        total: STEPS,
        finished: false,
      };
      notify("jle:progress", json!(job));
    }
  }

  fn run(self, export: impl FnOnce() -> ExportResult) -> TryResult {
    // goalc and the game of the last try, out of the session while the try works (the interface
    // reads the session meanwhile)
    let (goalc, mut gk) = {
      let mut s = lock(&self.session);
      (s.goalc.take(), s.gk.take())
    };
    let (r, goalc, read) = std::thread::scope(|scope| {
      // goalc learns the game's types while the level is exported and built
      let types = scope.spawn(|| self.goalc(goalc));
      self.step(0, "game_test.step.export");
      let (r, _, read) = export();
      let goalc = types.join().unwrap_or_else(|_| Err("goalc".into()));
      match (r, goalc) {
        (Ok(rep), Ok(mut g)) => {
          let r = self.play(&mut g, &mut gk, &rep.code_file);
          (r, Some(g), read)
        }
        (Ok(_), Err(e)) => (Err(e), None, read),
        (Err(e), g) => (Err(e), g.ok(), read),
      }
    });
    // what goalc and the game said, for when a try fails: <prefs>/game-test/<game>/*.log
    for (name, p) in [
      ("goalc.log", goalc.as_ref().map(|g| &g.program)),
      ("gk.log", gk.as_ref()),
    ] {
      if let Some(p) = p {
        let _ = std::fs::write(self.dir.join(name), p.since(0));
      }
    }
    let mut s = lock(&self.session);
    s.goalc = goalc;
    s.gk = gk;
    if r.is_err() {
      s.stop_game();
    }
    (r.map(|()| self.started.elapsed().as_secs_f64()), read)
  }

  /// goalc for the installation, the game's types learned: the one kept, else a new one
  fn goalc(&self, kept: Option<Goalc>) -> Result<Goalc, String> {
    if let Some(mut g) = kept {
      if g.data == self.install.data && g.program.alive() {
        return Ok(g);
      }
      g.program.kill();
    }
    let proj = self.project()?;
    let port = free_port();
    let mut cmd = Command::new(&self.install.goalc);
    cmd
      .args(["-g", &self.install.game, "--proj-path"])
      .arg(&proj)
      .args([
        "-p",
        &port.to_string(),
        "--debug-port",
        &free_port().to_string(),
      ]);
    let mut program = Program::start(cmd)?;
    self.started(&mut program, "goalc");
    if program
      .wait_for(0, &["Listening on"], Duration::from_secs(30))
      .is_none()
    {
      program.kill();
      return Err(strf!(
        tr("game_test.error.compiler_start"),
        last_lines(&program.since(0), 6)
      ));
    }
    let mut g = Goalc {
      program,
      port,
      data: self.install.data.clone(),
      evals: 0,
    };
    // the engine compiled: goalc knows the game's types (a goalc of its own compiles everything
    // once, into its project's out/)
    g.eval(
      "(make \"$OUT/obj/entity-debug.o\")",
      Duration::from_secs(600),
    )?;
    Ok(g)
  }

  /// goalc's project, made again for another installation: links to the installed game's sources
  /// (goal_src), disc (iso_data) and art (decompiler_out/<game>/raw_obj, which the game's project
  /// copies the art groups from), a copy of the decompiler's configuration (its inputs.jsonc written
  /// for each try), its own out/ and custom_assets/
  fn project(&self) -> Result<PathBuf, String> {
    let proj = self.dir.join("goalc");
    let data = self.install.data();
    let game = &self.install.game;
    let marker = proj.join("installation.txt");
    let made_for = format!("{}\n2", self.install.data);
    if std::fs::read_to_string(&marker).ok().as_deref() != Some(made_for.as_str()) {
      let _ = std::fs::remove_file(&marker);
      let _ = std::fs::remove_dir_all(proj.join("out"));
      std::fs::create_dir_all(&proj).map_err(|e| e.to_string())?;
      for f in ["goal_src", "iso_data"] {
        link_folder(&proj.join(f), &data.join(f))?;
      }
      // a link to the installation's configuration before: taken out (not what it shows)
      let decompiler = proj.join("decompiler");
      if std::fs::read_link(&decompiler).is_ok()
        || std::fs::symlink_metadata(&decompiler).is_ok_and(|m| !m.is_dir())
      {
        link_folder(&decompiler, &data.join("decompiler"))?;
        std::fs::remove_dir(&decompiler).map_err(|e| e.to_string())?;
      }
      // the configuration copied (5 MB): the builder reads it from the project, and its list of DGOs
      // changes with each level
      let config = decompiler.join("config");
      copy_folder(
        &data.join("decompiler/config").join(game),
        &config.join(game),
      )?;
      // the other games' types, which the configuration names
      for other in ["jak1", "jak2"] {
        let from = data.join("decompiler/config").join(other);
        if other != game && from.is_dir() {
          link_folder(&config.join(other), &from)?;
        }
      }
      let art = proj.join("decompiler_out").join(game);
      std::fs::create_dir_all(art.join("textures")).map_err(|e| e.to_string())?;
      link_folder(
        &art.join("raw_obj"),
        &data.join("decompiler_out").join(game).join("raw_obj"),
      )?;
      std::fs::write(&marker, &made_for).map_err(|e| e.to_string())?;
    }
    // the folders the game's build writes into (its builder does not make them)
    for f in ["obj", "iso", "fr3"] {
      let _ = std::fs::create_dir_all(proj.join("out").join(game).join(f));
    }
    let _ = std::fs::create_dir_all(proj.join("custom_assets"));
    Ok(proj)
  }

  /// The level built by goalc as the game's project builds a custom level, its DGO and .fr3 put into
  /// the game, the game started on it (`gk`: the game of the last try, closed, then this one's)
  fn play(
    &self,
    goalc: &mut Goalc,
    gk: &mut Option<Program>,
    code_file: &str,
  ) -> Result<(), String> {
    let level = self.dir.join("work/custom_assets/jak3/levels").join(NAME);
    let gd =
      std::fs::read_to_string(level.join(format!("{NAME}.gd"))).map_err(|e| e.to_string())?;
    self.step(1, "game_test.step.build");
    let proj = self.dir.join("goalc");
    let game = &self.install.game;
    // the level in the project, where the game's project looks for custom levels
    let levels = proj.join("custom_assets").join(game).join("levels");
    std::fs::create_dir_all(&levels).map_err(|e| e.to_string())?;
    link_folder(&levels.join(NAME), &level)?;
    // the builder reads the DGOs of inputs.jsonc: GAME.CGO (the common textures) and those holding
    // the level's art groups, as the game's own .gd files list them (all of them when one is not)
    let jsonc = read_jsonc(&level.join(format!("{NAME}.jsonc")))?;
    let wanted: Vec<String> = jsonc["art_groups"]
      .as_array()
      .map(|a| {
        a.iter()
          .filter_map(|v| v.as_str().map(String::from))
          .collect()
      })
      .unwrap_or_default();
    let inputs_file = |root: &Path| {
      root
        .join("decompiler/config")
        .join(game)
        .join("ntsc_v1/inputs.jsonc")
    };
    let mut inputs = read_jsonc(&inputs_file(&self.install.data()))?;
    if let Some(dgos) = art_group_dgos(&self.install, &wanted) {
      let mut names = vec!["CGO/GAME.CGO".to_string()];
      names.extend(dgos.into_iter().filter(|d| d != "CGO/GAME.CGO"));
      inputs["dgo_names"] = json!(names);
    }
    std::fs::write(
      inputs_file(&proj),
      serde_json::to_string_pretty(&inputs).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    // the game's project, then the level's steps, as the export's README adds them to game.gp: the
    // level, its code (the behaviors set in the editor, after the code of the game they derive
    // from: the .o files its DGO lists), its DGO
    let nick = NICKNAME.to_uppercase();
    let mut steps = format!(
      ";; the game's project and the level tried in Jak Level Editor\n\
       (load-file \"goal_src/{game}/game.gp\")\n\
       (build-custom-level \"{NAME}\")\n"
    );
    if !code_file.is_empty() {
      let deps: Vec<String> = std::iter::once("entity-debug.o")
        .chain(
          gd.split('"')
            .skip(1)
            .step_by(2)
            .filter(|f| f.ends_with(".o") && !f.ends_with("-obs.o")),
        )
        .map(|f| format!("\"$OUT/obj/{f}\""))
        .collect();
      steps += &format!(
        "(defstep :in \"{}\" :tool 'goalc :out '(\"$OUT/obj/{NAME}-obs.o\") :dep '({}))\n",
        code_file.replace('\\', "/"),
        deps.join(" ")
      );
    }
    steps += &format!("(custom-level-cgo \"{nick}.DGO\" \"{NAME}/{NAME}.gd\")\n");
    let project = proj.join(format!("{NAME}.gp"));
    std::fs::write(&project, steps).map_err(|e| e.to_string())?;
    goalc.eval(
      &format!(
        "(load-project \"{}\")",
        project.to_string_lossy().replace('\\', "/")
      ),
      Duration::from_secs(60),
    )?;
    make(goalc, &format!("$OUT/obj/{NAME}.go"))?;
    // its DGO: the level's code compiled, the game's code and art groups it lists (copied from the
    // game's art), packed
    self.step(2, "game_test.step.code");
    make(goalc, &format!("$OUT/iso/{nick}.DGO"))?;
    // the game of the last try closed first: it reads the DGO
    if let Some(mut old) = gk.take() {
      old.kill();
    }
    self.step(3, "game_test.step.pack");
    let out = self.install.out();
    let built = proj.join("out").join(game);
    let files = [
      ("iso", format!("{nick}.DGO")),
      ("fr3", format!("{NAME}.fr3")),
    ];
    {
      let mut s = lock(&self.session);
      for (dir, file) in &files {
        let put = out.join(dir).join(file);
        s.files.retain(|f| *f != put);
        s.files.push(put);
      }
    }
    for (dir, file) in &files {
      std::fs::copy(built.join(dir).join(file), out.join(dir).join(file))
        .map_err(|e| format!("{file}: {e}"))?;
    }

    self.step(4, "game_test.step.start");
    if port_taken(GAME_PORT) {
      return Err(strf!(tr("game_test.error.port_taken"), GAME_PORT));
    }
    let mut cmd = Command::new(&self.install.gk);
    cmd
      .args(["--game", &self.install.game, "--proj-path"])
      .arg(self.install.data())
      .args(["--", "-boot", "-fakeiso", "-debug"]);
    let game = gk.insert(Program::start(cmd)?);
    self.started(game, "gk");
    // its listener could not take the port (it says so before booting): another game has it
    let booted = game.wait_for(
      0,
      &["calling play-boot", "failed to bind"],
      Duration::from_secs(90),
    );
    let gk_out = game.since(0);
    match booted {
      _ if gk_out.contains("failed to bind") => {
        return Err(strf!(tr("game_test.error.port_taken"), GAME_PORT));
      }
      Some(_) => {}
      None => return Err(strf!(tr("game_test.error.game"), last_lines(&gk_out, 6))),
    }
    for _ in 0..20 {
      goalc.eval("(lt)", Duration::from_secs(15))?;
      if goalc.connected() {
        break;
      }
      std::thread::sleep(Duration::from_millis(500));
    }
    if !goalc.connected() {
      return Err(tr("game_test.error.connect").into());
    }
    // the game booted (Jak there) before it is told anything
    wait_value(goalc, "(if *target* 1 0)", Duration::from_secs(90))?;

    self.step(5, "game_test.step.load");
    let info = std::fs::read_to_string(level.join("level-info.gc")).map_err(|e| e.to_string())?;
    let define = info
      .find("(define ")
      .zip(info.find("(#when PC_PORT"))
      .map(|(a, b)| info[a..b].trim().to_string())
      .ok_or("level-info.gc")?;
    goalc.eval(&define, Duration::from_secs(60))?;
    goalc.eval(
      &format!("(cons! *level-load-list* '{NAME})"),
      Duration::from_secs(20),
    )?;
    goalc.eval(
      &format!("(start 'play (get-continue-by-name *game-info* \"{NAME}-start\"))"),
      Duration::from_secs(30),
    )?;
    wait_value(
      goalc,
      &format!("(if (= (level-status? *level* '{NAME} #f) 'active) 1 0)"),
      Duration::from_secs(120),
    )
  }
}

/// goalc makes `target` (a file of its project's outputs); an error when it does not say it built it
fn make(goalc: &mut Goalc, target: &str) -> Result<(), String> {
  let a = goalc.eval(&format!("(make \"{target}\")"), Duration::from_secs(600))?;
  if !a.contains("Successfully built") {
    return Err(strf!(tr("game_test.error.build"), last_lines(&a, 8)));
  }
  Ok(())
}

/// Asks `form` until the game answers 1
fn wait_value(goalc: &mut Goalc, form: &str, timeout: Duration) -> Result<(), String> {
  let end = Instant::now() + timeout;
  while Instant::now() < end {
    if goalc.value(form)? == "1" {
      return Ok(());
    }
    if !goalc.connected() {
      return Err(tr("game_test.error.connect").into());
    }
    std::thread::sleep(Duration::from_millis(500));
  }
  Err(strf!(tr("game_test.error.timeout"), form_head(form)))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn compiler_output_read() {
    assert_eq!(
      strip_ansi("\x1b[0m\x1b[1m\x1b[38;2;000;255;255mg > \x1b[0m[?2004h"),
      "g > [?2004h"
    );
    assert_eq!(
      last_lines("a\n\nb\nc\n", 2),
      "b\nc",
      "the last lines that say something"
    );
  }

  // a try needs a document and the game installed; it says why not (a key of lang/*.json)
  #[test]
  fn a_try_says_why_it_cannot_start() {
    let mut ed = Editor::new();
    let s = ed.game_test_state();
    assert_eq!(s["available"], false);
    assert_eq!(s["reason"], "game_test.reason.no_document");
    let installs = ed.game_installs();
    assert_eq!(installs.as_array().unwrap().len(), 3);
    let tmp = std::env::temp_dir().join(format!("jle-game-test-{}", std::process::id()));
    ed.host
      .game_installs
      .insert("jak3".into(), tmp.join("nowhere"));
    let jak3 = &ed.game_installs()[2];
    assert!(jak3["from_host"] == true && jak3["error"] == "game_install.error.no_folder");
    assert!(
      ed.set_game_install("jak3", "D:/elsewhere").is_err(),
      "the host's installation comes first"
    );
    assert!(ed.test_in_game().is_err());
  }
}
