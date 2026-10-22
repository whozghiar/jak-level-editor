//! OpenGOAL's tooling (its extractor, compiler and game) from the releases of whozghiar/jak-project,
//! whose level builder has the fixes the editor's exports need (`runtime::is_fork_release`).
//!
//! The standalone app installs it itself (`GameRuntime::tooling_dir`: `<library>/opengoal/versions/`):
//! the releases listed from GitHub, the one chosen downloaded and unpacked into `<tag>/` (its
//! programs and data/, as the OpenGOAL Launcher installs a version), the one used kept in the
//! preferences (`tooling_version`). The OpenGOAL Launcher installs and updates its own (its
//! header, its Versions page): its game only reads `known` to say a newer release is out.

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::host::{HostEvent, HostEvents};
use crate::runtime::FORK_SUFFIX;

/// the repository whose releases are OpenGOAL's tooling, for the editor and the launcher
pub const REPOSITORY: &str = "whozghiar/jak-project";

/// A release of the tooling with an archive for this system
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Release {
  pub tag: String,
  /// published_at (ISO 8601)
  pub date: String,
  /// its archive for this system: name, url, bytes
  pub asset: String,
  pub url: String,
  pub size: u64,
}

/// the system's word in the archives' names (`opengoal-windows-<tag>.zip`)
pub fn platform() -> &'static str {
  if cfg!(windows) {
    "windows"
  } else if cfg!(target_os = "macos") {
    "macos"
  } else {
    "linux"
  }
}

/// The archive of a release for `platform`, chosen as the OpenGOAL Launcher chooses it
/// (getDownloadLinkForCurrentPlatform): its name has the platform, not ".bin" nor "lsp" (the
/// language server); on macOS, the processor's ("arm", "intel") too
pub fn asset_for<'a>(assets: &'a [Value], platform: &str) -> Option<&'a Value> {
  let arch = if cfg!(target_arch = "aarch64") {
    "arm"
  } else {
    "intel"
  };
  assets.iter().find(|a| {
    let n = a["name"].as_str().unwrap_or_default().to_lowercase();
    n.contains(platform)
      && !n.contains(".bin")
      && !n.contains("lsp")
      && (platform != "macos" || n.contains(arch))
  })
}

/// The releases of GitHub's answer this system can install, newest first: an archive for it, not
/// a draft, not marked invalid in its notes (`<!-- invalid: ... -->`, as the launcher reads them)
pub fn parse_releases(answer: &Value, platform: &str) -> Vec<Release> {
  let mut list: Vec<Release> = answer
    .as_array()
    .into_iter()
    .flatten()
    .filter(|r| {
      !r["draft"].as_bool().unwrap_or(false)
        && !r["body"]
          .as_str()
          .unwrap_or_default()
          .contains("<!-- invalid:")
    })
    .filter_map(|r| {
      let a = asset_for(r["assets"].as_array()?, platform)?;
      Some(Release {
        tag: r["tag_name"].as_str()?.to_string(),
        date: r["published_at"].as_str().unwrap_or_default().to_string(),
        asset: a["name"].as_str()?.to_string(),
        url: a["browser_download_url"].as_str()?.to_string(),
        size: a["size"].as_u64().unwrap_or(0),
      })
    })
    .collect();
  list.sort_by(newest_first);
  list
}

/// (X, Y, Z, N) of `vX.Y.Z-whozghiar.N` (N 0 for a tag without the fork's suffix): the higher, the
/// newer
pub fn version_key(tag: &str) -> (u64, u64, u64, u64) {
  let tag = tag.trim_start_matches('v');
  let (base, build) = tag.split_once(FORK_SUFFIX).unwrap_or((tag, ""));
  let base = base.split('-').next().unwrap_or_default();
  let mut n = base.split('.').map(|p| p.parse().unwrap_or(0));
  let mut next = || n.next().unwrap_or(0);
  (next(), next(), next(), build.parse().unwrap_or(0))
}

/// the newer first: its version, then its date (the launcher sorts by date)
fn newest_first(a: &Release, b: &Release) -> Ordering {
  version_key(&b.tag)
    .cmp(&version_key(&a.tag))
    .then_with(|| b.date.cmp(&a.date))
}

fn client() -> Result<reqwest::blocking::Client, String> {
  reqwest::blocking::Client::builder()
    .user_agent(concat!("jak-level-editor/", env!("CARGO_PKG_VERSION")))
    .connect_timeout(Duration::from_secs(10))
    .build()
    .map_err(|e| e.to_string())
}

/// The fork's releases (GitHub's API, its first 100: newest first)
pub fn fetch_releases() -> Result<Vec<Release>, String> {
  let url = format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=100");
  let r = client()?
    .get(url)
    .header("Accept", "application/vnd.github+json")
    .timeout(Duration::from_secs(30))
    .send()
    .map_err(|e| e.to_string())?;
  match r.status().as_u16() {
    // no release yet
    404 => return Ok(Vec::new()),
    403 | 429 => return Err(crate::i18n::tr("tooling.rate_limit").into()),
    s if !(200..300).contains(&s) => return Err(format!("GitHub: {}", r.status())),
    _ => {}
  }
  let answer: Value = serde_json::from_str(&r.text().map_err(|e| e.to_string())?)
    .map_err(|e| format!("GitHub: {e}"))?;
  Ok(parse_releases(&answer, platform()))
}

/// The releases known (newest first), and how the last question to GitHub went
#[derive(Clone, Debug, Default, Serialize)]
pub struct Known {
  pub releases: Vec<Release>,
  /// GitHub answered once at least
  pub checked: bool,
  pub checking: bool,
  /// why the last question failed (no network, GitHub's limit): what was known before is kept
  pub error: String,
}

/// GitHub's answers so far (the editor's tick shows a new one)
static ANSWERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn answers() -> u64 {
  ANSWERS.load(std::sync::atomic::Ordering::Relaxed)
}

static KNOWN: Mutex<(Known, Option<Instant>)> = Mutex::new((
  Known {
    releases: Vec::new(),
    checked: false,
    checking: false,
    error: String::new(),
  },
  None,
));

/// GitHub is asked again an hour later at most (60 questions an hour without an account)
const RECHECK: Duration = Duration::from_secs(3600);

/// The releases known, never waiting: GitHub is asked on a thread of its own the first time, an
/// hour later, or now (`again`); its answer comes at a later call.
pub fn known(again: bool) -> Known {
  let mut k = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
  let due = again || k.1.is_none_or(|at| at.elapsed() > RECHECK);
  if due && !k.0.checking {
    k.0.checking = true;
    k.1 = Some(Instant::now());
    std::thread::spawn(|| {
      let r = fetch_releases();
      let mut k = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
      k.0.checking = false;
      match r {
        Ok(list) => {
          k.0.releases = list;
          k.0.checked = true;
          k.0.error.clear();
        }
        Err(e) => k.0.error = e,
      }
      ANSWERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
  }
  k.0.clone()
}

/// The newest release known when it is newer than `version` (the launcher's game says so)
pub fn newer_than(version: &str) -> Option<String> {
  known(false)
    .releases
    .first()
    .filter(|r| version_key(&r.tag) > version_key(version))
    .map(|r| r.tag.clone())
}

fn exe(dir: &Path, name: &str) -> PathBuf {
  dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// The versions installed in `versions` (a folder with gk), newest first
pub fn installed(versions: &Path) -> Vec<String> {
  let mut list: Vec<String> = std::fs::read_dir(versions)
    .into_iter()
    .flatten()
    .flatten()
    .filter(|e| exe(&e.path(), "gk").is_file())
    .filter_map(|e| e.file_name().to_str().map(str::to_string))
    .filter(|n| !n.ends_with(".part"))
    .collect();
  list.sort_by(|a, b| version_key(b).cmp(&version_key(a)).then_with(|| b.cmp(a)));
  list
}

/// The version used: the one `chosen` (the preferences' tooling_version) when it is installed,
/// else the newest installed, "" when none
pub fn active(installed: &[String], chosen: &str) -> String {
  if installed.iter().any(|v| v == chosen) {
    chosen.to_string()
  } else {
    installed.first().cloned().unwrap_or_default()
  }
}

/// What the title bar shows: the version used, the newest release, whether to update
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ToolingStatus {
  /// "" none installed
  pub active: String,
  /// "" not known yet (never asked, no network)
  pub latest: String,
  /// a release newer than the one used is out and not installed (the title bar's green icon, as
  /// the launcher's header's)
  pub update: bool,
}

pub fn status(installed: &[String], active: &str, releases: &[Release]) -> ToolingStatus {
  let latest = releases.first().map(|r| r.tag.clone()).unwrap_or_default();
  ToolingStatus {
    update: !latest.is_empty()
      && !installed.contains(&latest)
      && (active.is_empty() || version_key(&latest) > version_key(active)),
    active: active.to_string(),
    latest,
  }
}

/// a download's progress, one event per percent
fn download(url: &str, to: &Path, size: u64, events: &HostEvents) -> Result<(), String> {
  let mut r = client()?
    .get(url)
    .send()
    .and_then(|r| r.error_for_status())
    .map_err(|e| e.to_string())?;
  let total = r.content_length().unwrap_or(size).max(1);
  let mut file = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
  let mut buf = vec![0u8; 1 << 16];
  let (mut done, mut last) = (0u64, u64::MAX);
  loop {
    let n = r.read(&mut buf).map_err(|e| e.to_string())?;
    if n == 0 {
      break;
    }
    file
      .write_all(&buf[..n])
      .map_err(|e| format!("{}: {e}", to.display()))?;
    done += n as u64;
    if done * 100 / total != last {
      last = done * 100 / total;
      events(HostEvent::Progress { done, total });
    }
  }
  Ok(())
}

/// A release's archive (.zip, .tar.gz) unpacked into `dest`
pub fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
  let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", archive.display());
  let file = std::fs::File::open(archive).map_err(|e| fail(&e))?;
  if archive.extension().is_some_and(|e| e == "zip") {
    zip::ZipArchive::new(file)
      .and_then(|mut z| z.extract(dest))
      .map_err(|e| fail(&e))
  } else {
    tar::Archive::new(flate2::read::GzDecoder::new(file))
      .unpack(dest)
      .map_err(|e| fail(&e))
  }
}

/// the version being installed (one at a time)
static INSTALLING: Mutex<String> = Mutex::new(String::new());

pub fn installing() -> String {
  INSTALLING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Downloads `release` into `versions/<tag>/` (unpacked in `<tag>.part/`, renamed once complete),
/// its progress as events
pub fn install(versions: &Path, release: &Release, events: &HostEvents) -> Result<PathBuf, String> {
  {
    let mut busy = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
    if !busy.is_empty() {
      return Err(crate::i18n::tr("tooling.busy").into());
    }
    *busy = release.tag.clone();
  }
  let r = (|| {
    let dest = versions.join(&release.tag);
    let part = versions.join(format!("{}.part", release.tag));
    let _ = std::fs::remove_dir_all(&part);
    std::fs::create_dir_all(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let archive = part.join(&release.asset);
    download(&release.url, &archive, release.size, events)?;
    events(HostEvent::Step {
      key: "tooling.step.unpack".into(),
      args: vec![release.tag.clone()],
    });
    unpack(&archive, &part)?;
    let _ = std::fs::remove_file(&archive);
    if !exe(&part, "gk").is_file() {
      return Err(format!("{}: no gk in it", release.asset));
    }
    if dest.exists() {
      std::fs::remove_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    std::fs::rename(&part, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest)
  })();
  INSTALLING.lock().unwrap_or_else(|e| e.into_inner()).clear();
  if r.is_err() {
    let _ = std::fs::remove_dir_all(versions.join(format!("{}.part", release.tag)));
  }
  r
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::test_library::temp_dir;
  use serde_json::json;

  /// GitHub's answer for the fork's first release, as published
  fn answer() -> Value {
    let asset = |name: &str| json!({"name": name, "size": 42, "browser_download_url": format!("https://x/{name}")});
    let release = |tag: &str, date: &str| {
      json!({"tag_name": tag, "published_at": date, "draft": false, "body": "",
        "assets": [asset(&format!("opengoal-linux-{tag}.tar.gz")),
          asset(&format!("opengoal-lsp-linux-{tag}.bin")),
          asset(&format!("opengoal-lsp-windows-{tag}.exe")),
          asset(&format!("opengoal-windows-{tag}.zip"))]})
    };
    let mut invalid = release("v0.3.10-whozghiar.1", "2026-12-01T00:00:00Z");
    invalid["body"] = json!("<!-- invalid: broken | do not use -->");
    let mut draft = release("v0.3.10-whozghiar.2", "2026-12-02T00:00:00Z");
    draft["draft"] = json!(true);
    json!([
      release("v0.3.9-whozghiar.1", "2026-10-22T02:00:27Z"),
      release("v0.3.9-whozghiar.2", "2026-10-21T00:00:00Z"),
      release("v0.3.10-whozghiar.3", "2026-11-01T00:00:00Z"),
      invalid,
      draft,
      {"tag_name": "v0.3.8-whozghiar.1", "published_at": "2026-09-01T00:00:00Z", "assets": []}
    ])
  }

  #[test]
  fn the_archive_for_the_system() {
    let list = parse_releases(&answer(), "windows");
    let tags: Vec<&str> = list.iter().map(|r| r.tag.as_str()).collect();
    // newest version first (N after X.Y.Z), whatever the dates; no invalid, draft nor archive-less
    assert_eq!(
      tags,
      [
        "v0.3.10-whozghiar.3",
        "v0.3.9-whozghiar.2",
        "v0.3.9-whozghiar.1"
      ]
    );
    let r = &list[2];
    assert_eq!(r.asset, "opengoal-windows-v0.3.9-whozghiar.1.zip");
    assert!(r.url.ends_with(&r.asset) && r.size == 42 && r.date.starts_with("2026-10-22"));
    let linux = parse_releases(&answer(), "linux");
    assert_eq!(linux[0].asset, "opengoal-linux-v0.3.10-whozghiar.3.tar.gz");
    assert!(parse_releases(&answer(), "macos").is_empty());
    assert!(parse_releases(&json!({"message": "Not Found"}), "windows").is_empty());
  }

  #[test]
  fn versions_ordered() {
    assert_eq!(version_key("v0.3.9-whozghiar.12"), (0, 3, 9, 12));
    assert_eq!(version_key("v0.3.8"), (0, 3, 8, 0));
    assert_eq!(version_key("v1.0.0-rc1"), (1, 0, 0, 0));
    assert!(version_key("v0.3.10-whozghiar.1") > version_key("v0.3.9-whozghiar.7"));
    assert!(version_key("v0.3.9-whozghiar.2") > version_key("v0.3.9-whozghiar.1"));
    assert!(version_key("v0.3.9-whozghiar.1") > version_key("v0.3.9"));
    // the same version: the later date first
    let r = |date: &str| Release {
      tag: "v0.3.9-whozghiar.1".into(),
      date: date.into(),
      ..Default::default()
    };
    assert_eq!(
      newest_first(&r("2026-10-22"), &r("2026-10-21")),
      Ordering::Less
    );
  }

  #[test]
  fn installed_and_used() {
    let tmp = temp_dir("tooling");
    let versions = tmp.join("versions");
    for v in [
      "v0.3.9-whozghiar.1",
      "v0.3.10-whozghiar.1",
      "v0.3.9-whozghiar.2.part",
    ] {
      std::fs::create_dir_all(versions.join(v)).unwrap();
      std::fs::write(exe(&versions.join(v), "gk"), "").unwrap();
    }
    std::fs::create_dir_all(versions.join("empty")).unwrap();
    let list = installed(&versions);
    assert_eq!(list, ["v0.3.10-whozghiar.1", "v0.3.9-whozghiar.1"]);
    assert_eq!(
      active(&list, "v0.3.9-whozghiar.1"),
      "v0.3.9-whozghiar.1",
      "the one chosen"
    );
    assert_eq!(
      active(&list, "v0.3.8-whozghiar.1"),
      "v0.3.10-whozghiar.1",
      "else the newest"
    );
    assert_eq!(active(&[], ""), "");

    // the title bar's icon: a newer release, not installed
    let releases = parse_releases(&answer(), "windows");
    let s = status(&list, "v0.3.9-whozghiar.1", &releases);
    assert!(s.update && s.latest == "v0.3.10-whozghiar.3", "{s:?}");
    let s = status(
      &["v0.3.10-whozghiar.3".into()],
      "v0.3.9-whozghiar.1",
      &releases,
    );
    assert!(!s.update, "installed already");
    assert!(status(&[], "", &releases).update, "none installed");
    assert!(!status(&list, "v0.3.10-whozghiar.1", &[]).update, "offline");
    let _ = std::fs::remove_dir_all(&tmp);
  }

  #[test]
  fn unpacked() {
    let tmp = temp_dir("tooling-unpack");
    let archive = tmp.join("opengoal-windows-v0.3.9-whozghiar.1.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    let o = zip::write::SimpleFileOptions::default();
    z.add_directory("data/", o).unwrap();
    z.start_file("data/goal_src/jak3/game.gp", o).unwrap();
    z.write_all(b"()").unwrap();
    z.start_file(format!("gk{}", std::env::consts::EXE_SUFFIX), o)
      .unwrap();
    z.finish().unwrap();
    let dest = tmp.join("v0.3.9-whozghiar.1");
    unpack(&archive, &dest).unwrap();
    assert!(dest.join("data/goal_src/jak3/game.gp").is_file());
    assert_eq!(installed(&tmp), ["v0.3.9-whozghiar.1"]);
    // a .tar.gz (Linux's)
    let tgz = tmp.join("a.tar.gz");
    let mut t = tar::Builder::new(flate2::write::GzEncoder::new(
      std::fs::File::create(&tgz).unwrap(),
      flate2::Compression::fast(),
    ));
    let mut h = tar::Header::new_gnu();
    h.set_size(2);
    h.set_mode(0o755);
    t.append_data(&mut h, "gk", &b"gk"[..]).unwrap();
    t.into_inner().unwrap().finish().unwrap();
    unpack(&tgz, &tmp.join("linux")).unwrap();
    assert!(tmp.join("linux/gk").is_file());
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
