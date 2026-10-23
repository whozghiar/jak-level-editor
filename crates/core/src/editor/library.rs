//! Where the library lives: the host's default (crate::host::Host), the first-run assistant's
//! choice, the settings' change of folder; the extraction's help from the host (its disc already
//! extracted, its ogle-extract, its provider fetching one).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};

use super::extract::{extractor_path, extractor_places};
use super::{Editor, tr_err};
use crate::host::{Host, HostEvent, HostEvents};
use crate::i18n::{Arg, strf, tr};
use crate::io::asset_library::{default_library_root, from_path, game_folder};
use crate::prefs::{Prefs, READ_ONLY};
use crate::{log_error, log_info};

impl Editor {
  /// What the application gives the editor (the plugin's Builder::host and set_host).
  pub fn set_host(&mut self, host: Host) {
    self.extraction.extractor = host.extractor.clone();
    self.host = host;
  }

  /// The library's folder before the user chose one: the host's (the standalone app's
  /// Documents/Jak Level Editor, the launcher's <install>/jak-level-editor), else library/ of the
  /// editor's own tree in a development build; "" when neither.
  pub fn default_library(&self) -> String {
    match &self.host.default_library {
      Some(p) => from_path(p),
      None => default_library_root(),
    }
  }

  /// What the first-run assistant and the settings show (library_setup): `chosen` (no assistant
  /// then; the command line modes never ask), `default` and `default_game` (it holds an extracted
  /// game), `root` and `root_game` (the library now), `host`; ogle-extract: `extractor` (its path,
  /// "" when missing), `extractor_dirs` (where it is looked for), `can_fetch` (the host's download).
  pub fn library_setup(&self) -> Value {
    let default = self.default_library();
    let has_game = |root: &str| !game_folder(root).is_empty();
    let x = &self.extraction;
    let (host, res) = (x.extractor.as_deref(), x.resource_dir.as_deref());
    json!({
      "chosen": Prefs::get().library_chosen || READ_ONLY.load(std::sync::atomic::Ordering::Relaxed),
      "default_game": has_game(&default),
      "default": default,
      "root": self.library_root,
      "root_game": has_game(&self.library_root),
      "host": self.host.name,
      "extractor": extractor_path(host, res),
      "extractor_dirs": extractor_places(host, res),
      "can_fetch": self.host.extractor_provider.is_some(),
    })
  }

  /// The library the user chose (the first-run assistant's Confirm, the settings' change of
  /// folder): the folder created when missing, then used, kept as chosen. Nothing in the folder
  /// used before is moved or deleted.
  pub fn choose_library(&mut self, path: &str) -> Result<Value, String> {
    if path.trim().is_empty() {
      return Err(tr("settings.library.none").into());
    }
    std::fs::create_dir_all(path)
      .map_err(|e| tr_err("settings.library.cannot_create", &e.to_string()))?;
    self.set_library(path);
    self.save_prefs(|p| p.library_chosen = true);
    log_info!(tr("log.library_chosen"), self.library_root.clone());
    Ok(self.library_setup())
  }

  /// File > Extract game assets' first values (Extraction::defaults): the disc chosen last, else
  /// Jak 3's the host already extracted; `host_sources`, the host's discs by game (the launcher's
  /// iso_data/<game>), which the window takes when its game is chosen; the library, else its
  /// default; `can_fetch` (the host can fetch ogle-extract when it is missing), `fetching`.
  pub(crate) fn extract_defaults(&self) -> Value {
    let host_sources: BTreeMap<&str, String> = (self.host.extract_sources.iter())
      .map(|(game, disc)| (game.as_str(), disc.to_string_lossy().into_owned()))
      .collect();
    let mut source = Prefs::get().extract_source.clone();
    if source.is_empty() {
      source = host_sources.get("jak3").cloned().unwrap_or_default();
    }
    let root = if self.library_root.is_empty() {
      self.default_library()
    } else {
      self.library_root.clone()
    };
    let mut d = self
      .extraction
      .defaults(&source, &root, self.library.levels().len());
    d["host_sources"] = json!(host_sources);
    d["can_fetch"] = json!(self.host.extractor_provider.is_some());
    d["fetching"] = json!(self.busy("extract_fetch"));
    d
  }

  /// The Extract window's Download ogle-extract (extract_fetch): the host's provider fetches it on a
  /// job (jle:progress "extract_fetch"), then the extraction uses it; why not in `fetch_error`.
  pub(crate) fn extract_fetch(&mut self) -> Result<(), String> {
    let Some(provider) = self.host.extractor_provider.clone() else {
      return Err(tr("extract.no_provider").into());
    };
    if self.busy("extract_fetch") {
      return Ok(());
    }
    self.extraction.fetch_error.clear();
    let label = tr("extract.fetching").to_string();
    let (notify, job) = (self.notify.clone(), label.clone());
    let events: HostEvents = Arc::new(move |e| match e {
      HostEvent::Progress { done, total } => {
        if let Some(n) = &notify {
          let p = json!({"id": "extract_fetch", "label": job, "done": done, "total": total.max(1), "finished": false});
          n("jle:progress", p);
        }
      }
      HostEvent::Step { key, args } => {
        let args: Vec<Arg> = args.into_iter().map(Arg::from).collect();
        log_info!("%s", strf(tr(&key), &args));
      }
      HostEvent::Log { line } => log_info!("%s", line),
    });
    self.run_job(
      "extract_fetch",
      label,
      move || provider.fetch(events),
      |ed, r| match r {
        Ok(path) => {
          log_info!(
            tr("log.extractor_fetched"),
            path.to_string_lossy().into_owned()
          );
          ed.extraction.extractor = Some(path.clone());
          ed.host.extractor = Some(path);
        }
        Err(e) => {
          log_error!(tr("extract.fetch_failed"), e.clone());
          ed.extraction.fetch_error = e;
        }
      },
    );
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;
  use crate::host::ExtractorProvider;
  use crate::io::test_library::temp_dir;

  // a host's provider: an "ogle-extract" written in a folder of its own, or an error
  struct TestProvider(Result<PathBuf, String>);

  impl ExtractorProvider for TestProvider {
    fn fetch(&self, events: HostEvents) -> Result<PathBuf, String> {
      events(HostEvent::Progress { done: 1, total: 2 });
      events(HostEvent::Log {
        line: "downloading".into(),
      });
      let path = self.0.clone()?;
      std::fs::create_dir_all(path.parent().unwrap()).unwrap();
      std::fs::write(&path, b"not really ogle-extract").unwrap();
      Ok(path)
    }
  }

  fn editor(host: Host) -> Editor {
    let mut ed = Editor::new();
    ed.write_prefs = false;
    ed.set_host(host);
    ed
  }

  #[test]
  fn library_default_per_host_and_chosen() {
    let tmp = temp_dir("library-setup");
    let docs = tmp.join("Documents").join("Jak Level Editor");
    let mut ed = editor(Host {
      name: "standalone".into(),
      default_library: Some(docs.clone()),
      ..Default::default()
    });
    let setup = ed.call("library_setup", &Value::Null).unwrap();
    assert_eq!(setup["default"], from_path(&docs));
    assert_eq!(setup["default_game"], false);
    assert_eq!(setup["host"], "standalone");
    assert!(!docs.exists(), "nothing created before the user confirms");
    // Confirm: created, used, chosen
    let was = Prefs::get().clone();
    let chosen = ed
      .call("choose_library", &json!({"path": docs.to_string_lossy()}))
      .unwrap();
    assert!(docs.is_dir());
    assert_eq!(chosen["root"], from_path(&docs));
    assert_eq!(
      (&chosen["chosen"], &chosen["root_game"]),
      (&json!(true), &json!(false))
    );
    assert!(ed.call("choose_library", &json!({"path": " "})).is_err());
    // a default that holds an extracted game says so
    std::fs::create_dir_all(docs.join("jak3")).unwrap();
    std::fs::write(docs.join("jak3/library.json"), "{}").unwrap();
    assert_eq!(ed.library_setup()["default_game"], true);
    assert_eq!(ed.library_setup()["root_game"], true);
    // the projects of a library chosen
    assert_eq!(PathBuf::from(ed.projects_root()), docs.join("projects"));
    *Prefs::get() = was;
  }

  #[test]
  fn extraction_helped_by_the_host() {
    let tmp = temp_dir("extract-host");
    let disc = tmp.join("active/jak3/data/iso_data/jak3");
    let disc2 = tmp.join("active/jak2/data/iso_data/jak2");
    let fetched = tmp.join("fetched").join(if cfg!(windows) {
      "ogle-extract.exe"
    } else {
      "ogle-extract"
    });
    let was = Prefs::get().clone();
    Prefs::get().extract_source.clear();
    let mut ed = editor(Host {
      name: "launcher".into(),
      default_library: Some(tmp.join("jak-level-editor")),
      extract_sources: BTreeMap::from([
        ("jak3".into(), disc.clone()),
        ("jak2".into(), disc2.clone()),
      ]),
      extractor_provider: Some(Arc::new(TestProvider(Ok(fetched.clone())))),
      ..Default::default()
    });
    let d = ed.call("extract_defaults", &Value::Null).unwrap();
    assert_eq!(
      d["source"],
      disc.to_string_lossy().as_ref(),
      "the host's disc"
    );
    assert_eq!(
      d["host_sources"]["jak2"],
      disc2.to_string_lossy().as_ref(),
      "Jak II's too"
    );
    assert_eq!(d["output"], from_path(&tmp.join("jak-level-editor")));
    assert_eq!(d["can_fetch"], true);
    // the user's own disc wins
    Prefs::get().extract_source = "D:/my.iso".into();
    assert_eq!(ed.extract_defaults()["source"], "D:/my.iso");
    Prefs::get().extract_source.clear();
    // fetched (inline: an editor on its own), then used before any other
    ed.call("extract_fetch", &Value::Null).unwrap();
    let d = ed.extract_defaults();
    assert_eq!(d["extractor"], fetched.to_string_lossy().as_ref(), "{d}");
    assert_eq!(d["extractor_found"], true);
    assert_eq!(d["fetch_error"], "");
    let dir = fetched.parent().unwrap().to_string_lossy().into_owned();
    assert_eq!(d["extractor_dirs"][0], dir.as_str());
    assert_eq!(ed.host.extractor.as_deref(), Some(fetched.as_path()));

    // a provider that fails: why, for the window
    let mut ed = editor(Host {
      extractor_provider: Some(Arc::new(TestProvider(Err("offline".into())))),
      ..Default::default()
    });
    ed.call("extract_fetch", &Value::Null).unwrap();
    assert_eq!(ed.extract_defaults()["fetch_error"], "offline");
    // no provider: an error
    assert!(
      editor(Host::default())
        .call("extract_fetch", &Value::Null)
        .is_err()
    );
    *Prefs::get() = was;
  }
}
