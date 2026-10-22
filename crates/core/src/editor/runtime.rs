//! The game runtime the host gives the editor (host::GameRuntime): the document played in the game
//! (the Play button: exported with the Export window's options into the project's export folder,
//! installed, built, the game started on it), the levels installed in it. Without a runtime the
//! interface has no Play button: the export and its README.
//!
//! Calls (api.rs): play_level {options?, wait?} -> play_state, play_state, runtime_levels,
//! runtime_remove {name, wait?} -> play_state, runtime_launch {name, wait?} -> play_state,
//! runtime_settings. OpenGOAL's tooling, when the runtime installs it itself (the standalone app's,
//! crate::tooling): tooling_state {check?}, tooling_install {version, wait?}, tooling_use {version},
//! tooling_remove {version}, each answering tooling_state.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use super::export::ExportResult;
use super::{Editor, Notify};
use crate::host::{GameRuntime, HostEvent, HostEvents, LevelPackage, PlayOutcome};
use crate::i18n::{Arg, strf, tr};
use crate::io::game_export::LevelExportOptions;
use crate::prefs::Prefs;
use crate::tooling;
use crate::{log_error, log_info};

fn guard<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  m.lock().unwrap_or_else(|e| e.into_inner())
}

/// the lines of the runtime's tools kept for the Play window
const K_PLAY_LOG_LINES: usize = 400;

/// What the Play window shows: the steps of the operation going on or the last one, its progress,
/// its tools' last lines, how it ended.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PlayState {
  pub busy: bool,
  /// "play", "remove", "launch"
  pub action: String,
  /// the level
  pub name: String,
  /// {key, args}: translation keys
  pub steps: Vec<Value>,
  pub done: u64,
  pub total: u64,
  pub log: Vec<String>,
  pub outcome: Option<PlayOutcome>,
  pub error: String,
  pub seconds: f64,
  /// counts the operations ended (a caller waits for it to move)
  pub generation: u64,
}

/// the events of the runtime into the Play window's state, the steps and progress as jle:progress
fn events_into(state: Arc<Mutex<PlayState>>, notify: Option<Notify>) -> HostEvents {
  Arc::new(move |e: HostEvent| {
    let mut s = guard(&state);
    let progress = match &e {
      HostEvent::Step { key, args } => {
        s.steps.push(json!({"key": key, "args": args}));
        (s.done, s.total) = (0, 0);
        true
      }
      HostEvent::Progress { done, total } => {
        let moved = *done != s.done;
        (s.done, s.total) = (*done, *total);
        moved
      }
      HostEvent::Log { line } => {
        s.log.push(line.clone());
        if s.log.len() > K_PLAY_LOG_LINES {
          s.log.remove(0);
        }
        false
      }
    };
    let label = s.steps.last().map_or_else(String::new, |st| {
      let args: Vec<Arg> = st["args"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| Arg::from(a.as_str().unwrap_or_default()))
        .collect();
      strf(tr(st["key"].as_str().unwrap_or_default()), &args)
    });
    let job = json!({"id": "play", "label": label, "done": s.done, "total": s.total.max(1),
      "finished": false});
    drop(s);
    if progress && let Some(n) = &notify {
      n("jle:progress", job);
    }
  })
}

impl Editor {
  fn runtime(&self) -> Result<Arc<dyn GameRuntime>, String> {
    self
      .host
      .runtime
      .clone()
      .ok_or_else(|| tr("runtime.none").to_string())
  }

  /// get_state's `runtime`: the host's runtime (RuntimeInfo), with `tooling`
  /// (tooling::ToolingStatus: the title bar's version and update icon) when it installs OpenGOAL
  /// itself; null without one
  pub fn runtime_state(&self) -> Value {
    let Some(r) = &self.host.runtime else {
      return Value::Null;
    };
    let mut v = json!(r.info());
    if let Some(dir) = r.tooling_dir() {
      let (installed, active) = installed_and_active(&dir);
      v["tooling"] = json!(tooling::status(
        &installed,
        &active,
        &tooling::known(false).releases
      ));
    }
    v
  }

  pub fn play_state(&self) -> Value {
    json!(*guard(&self.play))
  }

  /// Runs `work` (the runtime's operation `action` on the level `name`) on the "play" job, then
  /// `then` under the lock; `wait`: in this call (tests, the remote control)
  fn run_play(
    &mut self,
    action: &str,
    name: &str,
    wait: bool,
    work: impl FnOnce(HostEvents) -> Result<PlayOutcome, String> + Send + 'static,
    then: impl FnOnce(&mut Editor) + Send + 'static,
  ) -> Result<Value, String> {
    if self.busy("play") || guard(&self.play).busy {
      return Err(tr("runtime.busy").into());
    }
    let generation = guard(&self.play).generation;
    *guard(&self.play) = PlayState {
      busy: true,
      action: action.into(),
      name: name.into(),
      generation,
      ..Default::default()
    };
    let events = events_into(self.play.clone(), self.notify.clone());
    let state = self.play.clone();
    let started = Instant::now();
    let job = move || {
      let r = work(events);
      let mut s = guard(&state);
      s.busy = false;
      s.generation += 1;
      s.seconds = started.elapsed().as_secs_f64();
      match &r {
        Ok(outcome) => s.outcome = Some(*outcome),
        Err(e) => s.error = e.clone(),
      }
      r
    };
    let (action, name) = (action.to_string(), name.to_string());
    let done = move |ed: &mut Editor, r: Result<PlayOutcome, String>| {
      then(ed);
      match r {
        Ok(_) => log_info!(tr("log.runtime_done"), &action, &name),
        Err(e) => log_error!(tr("log.runtime_failed"), &action, &name, &e),
      }
    };
    if wait {
      let r = job();
      done(self, r);
    } else {
      self.run_job("play", tr("play.title").into(), job, done);
    }
    Ok(self.play_state())
  }

  /// play_level {options?, wait?}: the document exported (the Export window's options, else the
  /// defaults, into the project's export folder), then played by the runtime
  pub fn play_level(
    &mut self,
    options: Option<LevelExportOptions>,
    wait: bool,
  ) -> Result<Value, String> {
    let runtime = self.runtime()?;
    if self.busy("export") {
      return Err("an export is going on".into());
    }
    let defaults = self.export_defaults();
    let mut o = options
      .unwrap_or_else(|| serde_json::from_value(defaults["options"].clone()).unwrap_or_default());
    // the package in the project's export folder; the window keeps the folder it has
    let kept = self.export.options.folder.clone();
    o.folder = self.default_export_options().folder;
    if let Some(bad) = self.export_check(&o) {
      return Err(tr(bad).into());
    }
    let export = self.export_work(o.clone())?;
    self.export.options.folder = kept;
    // the export's result, for the Export window's report once done
    let slot: Arc<Mutex<Option<ExportResult>>> = Arc::default();
    let filled = slot.clone();
    let name = o.name.clone();
    let work = move |events: HostEvents| {
      events(HostEvent::Step {
        key: "runtime.step.export".into(),
        args: vec![o.name],
      });
      let r = export();
      let report = r.0.clone();
      *guard(&filled) = Some(r);
      let package = LevelPackage::read(Path::new(&report?.dir))?;
      runtime.play(&package, events)
    };
    self.run_play("play", &name, wait, work, move |ed| {
      if let Some(r) = guard(&slot).take() {
        ed.export_done(r);
      }
    })
  }

  /// runtime_remove {name, wait?}: the level taken out of the runtime's game
  pub fn runtime_remove(&mut self, name: &str, wait: bool) -> Result<Value, String> {
    let runtime = self.runtime()?;
    let n = name.to_string();
    let work = move |events| {
      runtime.remove_level("jak3", &n, events)?;
      Ok(PlayOutcome::Started)
    };
    self.run_play("remove", name, wait, work, |_| {})
  }

  /// runtime_launch {name, wait?}: the game started on a level installed
  pub fn runtime_launch(&mut self, name: &str, wait: bool) -> Result<Value, String> {
    let runtime = self.runtime()?;
    let n = name.to_string();
    let work = move |events| {
      runtime.launch("jak3", &n, events)?;
      Ok(PlayOutcome::Started)
    };
    self.run_play("launch", name, wait, work, |_| {})
  }

  /// the levels installed in the runtime's game
  pub fn runtime_levels(&self) -> Result<Value, String> {
    Ok(json!(self.runtime()?.info().installed))
  }

  fn tooling_dir(&self) -> Result<PathBuf, String> {
    self
      .host
      .runtime
      .as_ref()
      .and_then(|r| r.tooling_dir())
      .ok_or_else(|| tr("tooling.none").to_string())
  }

  /// the runtime's settings read again (the standalone runtime's info, kept a moment, too)
  fn runtime_reread(&self) {
    if let Some(r) = &self.host.runtime {
      r.settings();
    }
  }

  /// tooling_state {check?}: the releases (newest first; the versions installed GitHub does not
  /// list after them), which are installed, the one used, the title bar's status, GitHub's last
  /// answer (`check`: asked again now), the version being installed. Null when the runtime does not
  /// install OpenGOAL itself (the launcher's: its header does).
  pub fn tooling_state(&self, check: bool) -> Value {
    let Ok(dir) = self.tooling_dir() else {
      return Value::Null;
    };
    let known = tooling::known(check);
    let (installed, active) = installed_and_active(&dir);
    let mut releases: Vec<Value> = known
      .releases
      .iter()
      .map(|r| {
        json!({"tag": r.tag, "date": r.date, "size": r.size, "asset": r.asset,
          "installed": installed.contains(&r.tag)})
      })
      .collect();
    for v in installed
      .iter()
      .filter(|v| !known.releases.iter().any(|r| &r.tag == *v))
    {
      releases.push(json!({"tag": v, "date": "", "size": 0, "asset": "", "installed": true}));
    }
    json!({
      "dir": crate::io::asset_library::from_path(&dir),
      "status": tooling::status(&installed, &active, &known.releases),
      "active": active,
      "releases": releases,
      "checked": known.checked,
      "checking": known.checking,
      "error": known.error,
      "installing": tooling::installing(),
    })
  }

  /// tooling_install {version, wait?}: the release downloaded and unpacked (the job "tooling":
  /// jle:progress), then used
  pub fn tooling_install(&mut self, version: &str, wait: bool) -> Result<Value, String> {
    let dir = self.tooling_dir()?;
    if self.busy("tooling") {
      return Err(tr("tooling.busy").into());
    }
    let release = (tooling::known(false).releases.into_iter())
      .find(|r| r.tag == version)
      .ok_or_else(|| strf(tr("tooling.unknown"), &[Arg::from(version)]))?;
    let label = strf(tr("tooling.downloading"), &[Arg::from(version)]);
    let (notify, job) = (self.notify.clone(), Mutex::new(label.clone()));
    let events: HostEvents = Arc::new(move |e| {
      let (done, total) = match e {
        HostEvent::Progress { done, total } => (done, total),
        HostEvent::Step { key, args } => {
          let args: Vec<Arg> = args.into_iter().map(Arg::from).collect();
          *guard(&job) = strf(tr(&key), &args);
          (0, 0)
        }
        HostEvent::Log { .. } => return,
      };
      if let Some(n) = &notify {
        let label = guard(&job).clone();
        n(
          "jle:progress",
          json!({"id": "tooling", "label": label, "done": done, "total": total.max(1),
            "finished": false}),
        );
      }
    });
    let work = move || tooling::install(&dir, &release, &events);
    let tag = version.to_string();
    let done = move |ed: &mut Editor, r: Result<PathBuf, String>| match r {
      Ok(_) => {
        ed.save_prefs(|p| p.tooling_version = tag.clone());
        ed.runtime_reread();
        log_info!(tr("log.tooling_installed"), tag);
      }
      Err(e) => log_error!(tr("log.tooling_failed"), tag, e),
    };
    if wait {
      let r = work();
      done(self, r);
    } else {
      self.run_job("tooling", label, work, done);
    }
    Ok(self.tooling_state(false))
  }

  /// tooling_use {version}: an installed version used from now on (its game made again from it at
  /// the next Play)
  pub fn tooling_use(&mut self, version: &str) -> Result<Value, String> {
    let dir = self.tooling_dir()?;
    if !tooling::installed(&dir).iter().any(|v| v == version) {
      return Err(strf(tr("tooling.unknown"), &[Arg::from(version)]));
    }
    self.save_prefs(|p| p.tooling_version = version.to_string());
    self.runtime_reread();
    Ok(self.tooling_state(false))
  }

  /// tooling_remove {version}: an installed version deleted (the game it runs stopped first)
  pub fn tooling_remove(&mut self, version: &str) -> Result<Value, String> {
    let dir = self.tooling_dir()?;
    if self.busy("play") || guard(&self.play).busy || tooling::installing() == version {
      return Err(tr("runtime.busy").into());
    }
    if version.is_empty() || !tooling::installed(&dir).iter().any(|v| v == version) {
      return Err(strf(tr("tooling.unknown"), &[Arg::from(version)]));
    }
    crate::runtime::stop_game();
    let path = dir.join(version);
    std::fs::remove_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    log_info!(tr("log.tooling_removed"), version);
    self.runtime_reread();
    Ok(self.tooling_state(false))
  }

  /// GitHub answered about the releases since the last tick: the interface shows it (the title
  /// bar's update icon)
  pub(crate) fn poll_tooling(&mut self) {
    static SEEN: AtomicU64 = AtomicU64::new(0);
    let n = tooling::answers();
    if SEEN.swap(n, Ordering::Relaxed) != n
      && (self.host.runtime.as_ref()).is_some_and(|r| r.tooling_dir().is_some())
    {
      self.emit("jle:changed", json!({"state": true}));
    }
  }

  /// what the settings show of the runtime: {info, settings} (null without one)
  pub fn runtime_settings(&self) -> Value {
    match &self.host.runtime {
      Some(r) => {
        // first: the standalone runtime reads its settings again there
        let settings = r.settings();
        json!({"info": r.info(), "settings": settings})
      }
      None => Value::Null,
    }
  }
}

/// the versions installed in `dir`, and the one used (the preferences' tooling_version)
fn installed_and_active(dir: &Path) -> (Vec<String>, String) {
  let installed = tooling::installed(dir);
  let chosen = Prefs::get().tooling_version.clone();
  let active = tooling::active(&installed, &chosen);
  (installed, active)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::host::{Host, RuntimeInfo};
  use crate::io::level_package;
  use crate::io::test_library::{make_floor, temp_dir};
  use crate::scene::{Material, Node, NodeKind};
  use std::path::PathBuf;

  /// a runtime installing into a fake data folder, no build nor game
  struct FakeRuntime {
    data: PathBuf,
    played: Mutex<Vec<String>>,
  }

  impl GameRuntime for FakeRuntime {
    fn info(&self) -> RuntimeInfo {
      RuntimeInfo {
        kind: "test".into(),
        available: true,
        installed: level_package::installed(&self.data)
          .into_iter()
          .map(|l| l.name)
          .collect(),
        ..Default::default()
      }
    }
    fn install_level(&self, p: &LevelPackage, events: HostEvents) -> Result<(), String> {
      events(HostEvent::Step {
        key: "runtime.step.install".into(),
        args: vec![p.manifest.name.clone()],
      });
      events(HostEvent::Log {
        line: "copied".into(),
      });
      level_package::apply(&self.data, p).map(|_| ())
    }
    fn remove_level(&self, game: &str, name: &str, _: HostEvents) -> Result<(), String> {
      level_package::remove(&self.data, game, name)
    }
    fn launch(&self, _: &str, name: &str, _: HostEvents) -> Result<(), String> {
      self.played.lock().unwrap().push(name.into());
      Ok(())
    }
  }

  #[test]
  fn played_through_the_runtime() {
    let tmp = temp_dir("editor-play");
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.library_root = tmp.join("library").to_string_lossy().into_owned();
    assert!(ed.runtime_state().is_null());
    assert!(ed.call("play_level", &json!({})).is_err(), "no runtime");

    let data = tmp.join("data");
    let info = data.join("goal_src/jak3/engine/level/level-info.gc");
    std::fs::create_dir_all(info.parent().unwrap()).unwrap();
    std::fs::write(
      data.join("goal_src/jak3/game.gp"),
      "(custom-level-cgo \"TSZ.DGO\" \"test-zone/testzone.gd\")\n(group-list \"iso\")\n",
    )
    .unwrap();
    std::fs::write(&info, "(define test-zone\n  :index #x128\n  )\n").unwrap();
    let runtime = Arc::new(FakeRuntime {
      data: data.clone(),
      played: Mutex::default(),
    });
    ed.host = Host {
      runtime: Some(runtime.clone()),
      ..Default::default()
    };
    // a small level of its own
    ed.scene.settings.level = "testlvl".into();
    let mat = ed.scene.add_material(Material::default());
    ed.scene.add(Node {
      name: "floor".into(),
      kind: NodeKind::Render,
      mesh: Some(make_floor(mat)),
      ..Default::default()
    });
    ed.scene.begin_frame();
    let s = ed.call("play_level", &json!({"wait": true})).unwrap();
    assert!(
      s["outcome"] == "started" && s["error"] == "" && s["steps"].as_array().unwrap().len() == 2,
      "{s}"
    );
    let name = s["name"].as_str().unwrap().to_string();
    assert_eq!(*runtime.played.lock().unwrap(), [name.clone()]);
    assert!(
      ed.export
        .report
        .as_ref()
        .is_some_and(|r| r.dir.ends_with(&format!("/{name}")))
        && Path::new(&ed.library_root)
          .join("exports")
          .join(&name)
          .join("jle-level.json")
          .is_file(),
      "exported into the project's export folder"
    );
    assert_eq!(
      ed.call("runtime_levels", &json!({})).unwrap(),
      json!([name])
    );
    assert_eq!(ed.get_state()["runtime"]["installed"], json!([name]));
    let s = ed
      .call("runtime_remove", &json!({"name": name, "wait": true}))
      .unwrap();
    assert!(
      s["error"] == "" && level_package::installed(&data).is_empty(),
      "{s}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
  }

  /// a runtime installing OpenGOAL itself (the standalone app's) into a folder of its own
  struct ToolingRuntime(PathBuf);

  impl GameRuntime for ToolingRuntime {
    fn info(&self) -> RuntimeInfo {
      RuntimeInfo::default()
    }
    fn install_level(&self, _: &LevelPackage, _: HostEvents) -> Result<(), String> {
      Ok(())
    }
    fn remove_level(&self, _: &str, _: &str, _: HostEvents) -> Result<(), String> {
      Ok(())
    }
    fn launch(&self, _: &str, _: &str, _: HostEvents) -> Result<(), String> {
      Ok(())
    }
    fn tooling_dir(&self) -> Option<PathBuf> {
      Some(self.0.clone())
    }
  }

  #[test]
  fn tooling_offered_by_the_runtime() {
    let tmp = temp_dir("editor-tooling");
    let mut ed = Editor::new();
    ed.write_prefs = false;
    // the launcher's runtime installs none: nothing offered
    ed.host = Host {
      runtime: Some(Arc::new(FakeRuntime {
        data: tmp.join("data"),
        played: Mutex::default(),
      })),
      ..Default::default()
    };
    assert!(ed.call("tooling_state", &json!({})).unwrap().is_null());
    assert!(ed.get_state()["runtime"].get("tooling").is_none());
    assert!(ed.call("tooling_use", &json!({"version": "v1"})).is_err());

    let versions = tmp.join("versions");
    for v in ["v0.3.9-whozghiar.1", "v0.3.9-whozghiar.2"] {
      std::fs::create_dir_all(versions.join(v)).unwrap();
      let gk = format!("gk{}", std::env::consts::EXE_SUFFIX);
      std::fs::write(versions.join(v).join(gk), "").unwrap();
    }
    ed.host = Host {
      runtime: Some(Arc::new(ToolingRuntime(versions.clone()))),
      ..Default::default()
    };
    let was = Prefs::get().tooling_version.clone();
    let s = ed.call("tooling_state", &json!({})).unwrap();
    assert!(
      s["active"] == "v0.3.9-whozghiar.2"
        && s["releases"]
          .as_array()
          .unwrap()
          .iter()
          .any(|r| r["tag"] == "v0.3.9-whozghiar.1" && r["installed"] == true),
      "{s}"
    );
    assert_eq!(
      ed.get_state()["runtime"]["tooling"]["active"],
      "v0.3.9-whozghiar.2"
    );
    let s = ed
      .call("tooling_use", &json!({"version": "v0.3.9-whozghiar.1"}))
      .unwrap();
    assert_eq!(s["active"], "v0.3.9-whozghiar.1");
    assert!(
      ed.call("tooling_use", &json!({"version": "v9"})).is_err(),
      "not installed"
    );
    let s = ed
      .call("tooling_remove", &json!({"version": "v0.3.9-whozghiar.1"}))
      .unwrap();
    assert!(
      s["active"] == "v0.3.9-whozghiar.2" && !versions.join("v0.3.9-whozghiar.1").exists(),
      "{s}"
    );
    assert!(
      ed.call("tooling_install", &json!({"version": "v0.0.0-none"}))
        .is_err(),
      "not a release known"
    );
    Prefs::get().tooling_version = was;
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
