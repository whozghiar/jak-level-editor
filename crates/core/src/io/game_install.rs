//! The game the user installed, where a level is tried (editor/game_test.rs): its programs (gk, the
//! game; goalc, the compiler that talks to it), its data folder (the game compiled, the disc's
//! files, its sources), its version. Nothing of the game is installed or compiled for a try: its
//! goalc builds the level in a project of the editor's, the level's DGO and .fr3 are put next to the
//! game's, and the level is declared to the running game.
//!
//! An installation is found in a folder chosen by the user (the standalone app's settings), or given
//! by the host (the OpenGOAL Launcher's installation folder), laid out one of three ways:
//!
//!   OpenGOAL Launcher   <folder>/active/bin/gk(.exe), goalc; <folder>/active/<game>/data/
//!   an OpenGOAL release <folder>/gk(.exe), goalc; <folder>/data/ (after its extractor ran)
//!   a jak-project       <folder>/out/build/<preset>/bin/gk(.exe), goalc; <folder>/ itself

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

const EXE: &str = std::env::consts::EXE_SUFFIX;

/// An installation of a game where levels can be tried
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GameInstall {
  pub game: String,
  /// the folder chosen (or given by the host)
  pub root: String,
  pub gk: String,
  pub goalc: String,
  /// OpenGOAL's extractor, "" when missing (io/extraction: File > Extract game assets)
  pub extractor: String,
  /// the data folder: out/<game>/ (the game compiled), iso_data/<game>/ (the disc), goal_src/
  pub data: String,
  /// gk's version ("v0.3.9"), "" when it did not say
  pub version: String,
}

impl GameInstall {
  pub fn data(&self) -> PathBuf {
    PathBuf::from(&self.data)
  }

  /// the game as compiled: obj/ (code and art groups), iso/ (the DGOs it loads), fr3/
  pub fn out(&self) -> PathBuf {
    self.data().join("out").join(&self.game)
  }
}

fn exe(dir: &Path, name: &str) -> PathBuf {
  dir.join(format!("{name}{EXE}"))
}

// (programs' folder, data folder) of the installation in `root`: the first layout with gk
fn layout(game: &str, root: &Path) -> Result<(PathBuf, PathBuf), &'static str> {
  if !root.is_dir() {
    return Err("game_install.error.no_folder");
  }
  let mut layouts = vec![
    (
      root.join("active").join("bin"),
      root.join("active").join(game).join("data"),
    ),
    (root.to_path_buf(), root.join("data")),
  ];
  if let Ok(presets) = std::fs::read_dir(root.join("out").join("build")) {
    for p in presets.flatten() {
      layouts.push((p.path().join("bin"), root.to_path_buf()));
    }
  }
  layouts
    .into_iter()
    .find(|(bin, _)| exe(bin, "gk").is_file())
    .ok_or("game_install.error.no_game")
}

fn install(game: &str, root: &Path, bin: &Path, data: PathBuf) -> GameInstall {
  let s = |p: PathBuf| p.to_string_lossy().into_owned();
  let extractor = exe(bin, "extractor");
  GameInstall {
    game: game.into(),
    root: s(root.to_path_buf()),
    gk: s(exe(bin, "gk")),
    goalc: s(exe(bin, "goalc")),
    extractor: if extractor.is_file() {
      s(extractor)
    } else {
      String::new()
    },
    data: s(data),
    version: String::new(),
  }
}

/// The installation of `game` in `root` to try levels in, or why there is none: a key of
/// lang/*.json (game_install.error.*)
pub fn find_install(game: &str, root: &Path) -> Result<GameInstall, &'static str> {
  let (bin, data) = layout(game, root)?;
  if !exe(&bin, "goalc").is_file() {
    return Err("game_install.error.no_compiler");
  }
  if !data.join("goal_src").join(game).join("game.gp").is_file() {
    return Err("game_install.error.no_sources");
  }
  if !data.join("iso_data").join(game).join("DGO").is_dir() {
    return Err("game_install.error.no_disc");
  }
  if !data
    .join("out")
    .join(game)
    .join("iso")
    .join("GAME.CGO")
    .is_file()
  {
    return Err("game_install.error.not_compiled");
  }
  Ok(install(game, root, &bin, data))
}

/// The installation of `game` in `root` to extract its levels from (io/extraction): OpenGOAL's
/// extractor, the disc's files it extracted, the decompiler's configuration; or why not
pub fn find_extraction(game: &str, root: &Path) -> Result<GameInstall, &'static str> {
  let (bin, data) = layout(game, root)?;
  if !exe(&bin, "extractor").is_file() {
    return Err("game_install.error.no_extractor");
  }
  if !data.join("iso_data").join(game).join("DGO").is_dir() {
    return Err("game_install.error.no_disc");
  }
  if !config_file(&data, game).is_file() {
    return Err("game_install.error.no_config");
  }
  Ok(install(game, root, &bin, data))
}

/// The decompiler's configuration of `game` in a data folder
pub fn config_file(data: &Path, game: &str) -> PathBuf {
  data
    .join("decompiler")
    .join("config")
    .join(game)
    .join(format!("{game}_config.jsonc"))
}

/// gk's version, as `gk --version` prints it last ("v0.3.9"); "" when it did not say
pub fn game_version(gk: &str) -> String {
  let mut cmd = std::process::Command::new(gk);
  cmd.arg("--version");
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
  }
  let Ok(out) = cmd.output() else {
    return String::new();
  };
  let text = String::from_utf8_lossy(&out.stdout);
  text
    .lines()
    .map(str::trim)
    .rfind(|l| l.starts_with('v') && l[1..].starts_with(|c: char| c.is_ascii_digit()))
    .unwrap_or_default()
    .to_string()
}

/// DGOs (`DGO/CWI.DGO`, `CGO/GAME.CGO`) holding the art groups `wanted` (`ctycrate-ag`), as few as
/// it takes, as the game's sources list the files of its DGOs (`goal_src/<game>/dgos/*.gd`); None
/// when one of them is in none
pub fn art_group_dgos(install: &GameInstall, wanted: &[String]) -> Option<Vec<String>> {
  let dir = install
    .data()
    .join("goal_src")
    .join(&install.game)
    .join("dgos");
  let mut lists: Vec<(String, Vec<String>)> = Vec::new();
  for e in std::fs::read_dir(dir).ok()?.flatten() {
    let text = std::fs::read_to_string(e.path()).unwrap_or_default();
    let quoted: Vec<String> = text
      .split('"')
      .skip(1)
      .step_by(2)
      .map(str::to_string)
      .collect();
    if let Some(name) = quoted
      .first()
      .filter(|n| n.ends_with(".DGO") || n.ends_with(".CGO"))
    {
      let folder = if name.ends_with(".CGO") { "CGO" } else { "DGO" };
      lists.push((format!("{folder}/{name}"), quoted[1..].to_vec()));
    }
  }
  lists.sort();
  // few DGOs to read: each time the one holding the most of those still wanted
  let mut left: Vec<String> = wanted.iter().map(|ag| format!("{ag}.go")).collect();
  let mut dgos = Vec::new();
  while !left.is_empty() {
    let held = |files: &Vec<String>| left.iter().filter(|f| files.contains(f)).count();
    let (dgo, files) = lists
      .iter()
      .max_by_key(|(name, files)| (held(files), std::cmp::Reverse(name.clone())))?;
    if held(files) == 0 {
      return None;
    }
    left.retain(|f| !files.contains(f));
    dgos.push(dgo.clone());
  }
  Some(dgos)
}

/// A JSON file with comments (`//`, `/* */`), as OpenGOAL's configuration and levels are written
pub fn read_jsonc(path: &Path) -> Result<Value, String> {
  let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
  serde_json::from_str(&without_comments(&text)).map_err(|e| format!("{}: {e}", path.display()))
}

// the comments of a JSON text taken out, its strings kept
fn without_comments(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  let mut chars = text.chars().peekable();
  let mut in_string = false;
  while let Some(c) = chars.next() {
    if in_string {
      out.push(c);
      match c {
        '\\' => out.extend(chars.next()),
        '"' => in_string = false,
        _ => {}
      }
      continue;
    }
    match (c, chars.peek()) {
      ('"', _) => {
        in_string = true;
        out.push(c);
      }
      ('/', Some('/')) => {
        while chars.peek().is_some_and(|&c| c != '\n') {
          chars.next();
        }
      }
      ('/', Some('*')) => {
        chars.next();
        let mut last = ' ';
        for c in chars.by_ref() {
          if last == '*' && c == '/' {
            break;
          }
          last = c;
        }
      }
      _ => out.push(c),
    }
  }
  out
}

/// `from`'s files copied into `to`, its folders too
pub fn copy_folder(from: &Path, to: &Path) -> Result<(), String> {
  std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
  for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
    let e = e.map_err(|e| e.to_string())?;
    let target = to.join(e.file_name());
    if e.path().is_dir() {
      copy_folder(&e.path(), &target)?;
    } else {
      std::fs::copy(e.path(), &target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
  }
  Ok(())
}

/// `link` shows the folder `target` (a junction on Windows, which needs no rights; a symbolic link
/// elsewhere); an older link there is replaced
pub fn link_folder(link: &Path, target: &Path) -> Result<(), String> {
  if std::fs::symlink_metadata(link).is_ok() {
    // a junction is removed as a folder, without what it shows
    std::fs::remove_dir(link)
      .or_else(|_| std::fs::remove_file(link))
      .map_err(|e| format!("{}: {e}", link.display()))?;
  }
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    let ok = std::process::Command::new("cmd")
      .raw_arg(format!(
        "/C mklink /J \"{}\" \"{}\"",
        link.display(),
        target.display()
      ))
      .creation_flags(0x0800_0000)
      .output()
      .is_ok_and(|o| o.status.success());
    if !ok {
      return Err(format!(
        "{}: cannot link to {}",
        link.display(),
        target.display()
      ));
    }
    Ok(())
  }
  #[cfg(not(windows))]
  std::os::unix::fs::symlink(target, link).map_err(|e| format!("{}: {e}", link.display()))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
  }

  #[test]
  fn installs_found_as_laid_out() {
    let tmp = temp("game-install");
    let touch = |p: PathBuf| {
      std::fs::create_dir_all(p.parent().unwrap()).unwrap();
      std::fs::write(p, b"").unwrap();
    };
    let launcher = tmp.join("launcher");
    assert_eq!(
      find_install("jak3", &launcher),
      Err("game_install.error.no_folder")
    );
    std::fs::create_dir_all(&launcher).unwrap();
    assert_eq!(
      find_install("jak3", &launcher),
      Err("game_install.error.no_game")
    );
    let bin = launcher.join("active/bin");
    touch(bin.join(format!("gk{EXE}")));
    assert_eq!(
      find_install("jak3", &launcher),
      Err("game_install.error.no_compiler")
    );
    touch(bin.join(format!("goalc{EXE}")));
    let data = launcher.join("active/jak3/data");
    touch(data.join("goal_src/jak3/game.gp"));
    std::fs::create_dir_all(data.join("iso_data/jak3/DGO")).unwrap();
    assert_eq!(
      find_install("jak3", &launcher),
      Err("game_install.error.not_compiled")
    );
    touch(data.join("out/jak3/iso/GAME.CGO"));
    let i = find_install("jak3", &launcher).expect("the launcher's");
    assert_eq!(Path::new(&i.data), data);
    assert_eq!(i.out(), data.join("out/jak3"));
    // its levels extracted: by its extractor, from the disc's files, with its configuration
    assert_eq!(
      find_extraction("jak3", &launcher),
      Err("game_install.error.no_extractor")
    );
    touch(bin.join(format!("extractor{EXE}")));
    assert_eq!(
      find_extraction("jak3", &launcher),
      Err("game_install.error.no_config")
    );
    touch(config_file(&data, "jak3"));
    let e = find_extraction("jak3", &launcher).expect("the launcher's extractor");
    assert!(e.extractor.ends_with(&format!("extractor{EXE}")));
    // a jak-project checkout: its programs in a build preset, its data at the top
    let checkout = tmp.join("jak-project");
    touch(checkout.join(format!("out/build/Release/bin/gk{EXE}")));
    touch(checkout.join(format!("out/build/Release/bin/goalc{EXE}")));
    touch(checkout.join("goal_src/jak3/game.gp"));
    std::fs::create_dir_all(checkout.join("iso_data/jak3/DGO")).unwrap();
    touch(checkout.join("out/jak3/iso/GAME.CGO"));
    let i = find_install("jak3", &checkout).expect("the checkout's");
    assert_eq!(Path::new(&i.data), checkout);
    assert!(i.goalc.ends_with(&format!("goalc{EXE}")));
    let _ = std::fs::remove_dir_all(&tmp);
  }

  #[test]
  fn art_groups_found_in_the_games_dgo_lists() {
    let tmp = temp("dgo-lists");
    let data = tmp.join("data");
    let dgos = data.join("goal_src/jak3/dgos");
    std::fs::create_dir_all(&dgos).unwrap();
    std::fs::write(
      dgos.join("cwi.gd"),
      "(\"CWI.DGO\"\n (\"vehicle.o\"\n  \"ctycrate-ag.go\"\n  ))\n",
    )
    .unwrap();
    std::fs::write(dgos.join("game.gd"), "(\"GAME.CGO\" (\"jakb-ag.go\"))").unwrap();
    std::fs::write(
      dgos.join("waspala.gd"),
      "(\"WASPALA.DGO\" (\"urn-a-ag.go\" \"ctycrate-ag.go\"))",
    )
    .unwrap();
    let install = GameInstall {
      game: "jak3".into(),
      data: data.to_string_lossy().into_owned(),
      ..Default::default()
    };
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
      art_group_dgos(&install, &names(&["urn-a-ag", "ctycrate-ag", "jakb-ag"])),
      Some(names(&["DGO/WASPALA.DGO", "CGO/GAME.CGO"])),
      "one DGO for all that it holds, the CGOs in their folder"
    );
    assert_eq!(art_group_dgos(&install, &names(&["nowhere-ag"])), None);
    let _ = std::fs::remove_dir_all(&tmp);
  }

  #[test]
  fn jsonc_read_without_its_comments() {
    let tmp = temp("jsonc");
    let p = tmp.join("inputs.jsonc");
    std::fs::write(
      &p,
      "{\n  // the DGOs\n  \"dgo_names\": [\"CGO/GAME.CGO\", // the engine\n \"DGO/A.DGO\" /* a level */],\n  \"url\": \"http://x // y\"\n}\n",
    )
    .unwrap();
    let v = read_jsonc(&p).unwrap();
    assert_eq!(v["dgo_names"][1], "DGO/A.DGO");
    assert_eq!(v["url"], "http://x // y", "a string keeps its slashes");
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
