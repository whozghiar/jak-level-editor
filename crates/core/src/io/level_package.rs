//! Installs a level exported for the game (`host::LevelPackage`: an export and its
//! `jle-level.json`) into an OpenGOAL data folder, and takes it out again: what the export's README
//! tells a person to do, for a host's game runtime (the OpenGOAL Launcher's isolated game).
//!
//! The data folder holds `goal_src/<game>` and `custom_assets/<game>`: a jak-project checkout's
//! root, or an OpenGOAL distribution's `data/`. `apply` copies the export's files there, adds its
//! lines to `goal_src/<game>/game.gp` and its definition to `engine/level/level-info.gc` (with an
//! `:index` no other level has), or changes the `:nickname` of the level it replaces, each inside a
//! marked block:
//!
//! ```text
//! ;; jak-level-editor: begin <name>
//! (build-custom-level "<name>")
//! ;; jak-level-editor: end <name>
//! ```
//!
//! A line it replaces stays in the block as `;; jak-level-editor: was <line>`, put back by
//! `remove`. Each file edited is backed up once (`<file>.jle-backup`, deleted when the file is back
//! as it was). The blocks are the record of what is installed (`installed`); the level's files are
//! in its own folders (`custom_assets/<game>/levels/<name>/`, `goal_src/<game>/levels/<name>/`),
//! which `remove` deletes. Applying again replaces the level's blocks and files.

use std::path::{Component, Path, PathBuf};

use crate::host::LevelPackage;

const MARK: &str = ";; jak-level-editor: ";

/// The levels installed in a data folder for `game`: the names of the editor's blocks in its
/// game.gp (none when it has none).
pub fn installed(data_dir: &Path, game: &str) -> Vec<String> {
  let begin = format!("{MARK}begin ");
  let text = std::fs::read_to_string(game_gp(data_dir, game)).unwrap_or_default();
  let mut names: Vec<String> = text
    .lines()
    .filter_map(|l| l.trim_end().strip_prefix(&begin))
    .map(str::to_string)
    .collect();
  names.dedup();
  names
}

fn game_gp(data_dir: &Path, game: &str) -> PathBuf {
  data_dir.join("goal_src").join(game).join("game.gp")
}

fn level_info_gc(data_dir: &Path, game: &str) -> PathBuf {
  data_dir
    .join("goal_src")
    .join(game)
    .join("engine/level/level-info.gc")
}

fn read(p: &Path) -> Result<String, String> {
  std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn write(p: &Path, text: &str) -> Result<(), String> {
  std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()))
}

fn backup_path(p: &Path) -> PathBuf {
  let mut s = p.as_os_str().to_owned();
  s.push(".jle-backup");
  PathBuf::from(s)
}

/// a file edited: backed up the first time
fn write_edited(p: &Path, text: &str) -> Result<(), String> {
  let backup = backup_path(p);
  if !backup.exists() {
    std::fs::copy(p, &backup).map_err(|e| format!("{}: {e}", backup.display()))?;
  }
  write(p, text)
}

/// a file whose blocks were all taken out: its backup goes when it is the same
fn forget_backup(p: &Path) {
  let backup = backup_path(p);
  if std::fs::read(&backup).ok() == std::fs::read(p).ok() {
    let _ = std::fs::remove_file(backup);
  }
}

fn newline(text: &str) -> &'static str {
  if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn block(name: &str, lines: &[String], nl: &str) -> String {
  let mut b = format!("{MARK}begin {name}{nl}");
  for l in lines {
    b += l;
    b += nl;
  }
  b + &format!("{MARK}end {name}{nl}")
}

/// `text` without the blocks of `name` (the lines they replaced put back), and where the first one
/// was: a new block goes there (applying again changes nothing)
fn strip_blocks(text: &str, name: &str) -> (String, Option<usize>) {
  let (begin, end) = (format!("{MARK}begin {name}"), format!("{MARK}end {name}"));
  let was = format!("{MARK}was ");
  let mut out = String::with_capacity(text.len());
  let (mut inside, mut found) = (false, None);
  for line in text.split_inclusive('\n') {
    let bare = line.trim_end_matches(['\r', '\n']);
    if !inside && bare == begin {
      inside = true;
      found = found.or(Some(out.len()));
    } else if inside && bare == end {
      inside = false;
    } else if inside {
      if let Some(original) = line.strip_prefix(&was) {
        out += original;
      }
    } else {
      out += line;
    }
  }
  (out, found)
}

/// the byte range of the line holding `at`
fn line_range(text: &str, at: usize) -> (usize, usize) {
  let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
  let end = text[at..].find('\n').map_or(text.len(), |i| at + i + 1);
  (start, end)
}

/// where `(define <name>` starts (the symbol whole), outside the editor's blocks
fn find_define(text: &str, name: &str) -> Option<usize> {
  let key = format!("(define {name}");
  let mut from = 0;
  while let Some(i) = text[from..].find(&key) {
    let at = from + i;
    let after = text[at + key.len()..].chars().next();
    if after.is_none_or(|c| c.is_whitespace() || c == ')') {
      return Some(at);
    }
    from = at + key.len();
  }
  None
}

/// the `:index` values of the levels defined
fn indexes(text: &str) -> Vec<u32> {
  let mut out = Vec::new();
  for (i, _) in text.match_indices(":index ") {
    let v: String = text[i + 7..]
      .chars()
      .take_while(|c| c.is_ascii_alphanumeric() || *c == '#')
      .collect();
    let n = match v.strip_prefix("#x") {
      Some(h) => u32::from_str_radix(h, 16).ok(),
      None => v.parse().ok(),
    };
    out.extend(n);
  }
  out
}

fn is_name(s: &str) -> bool {
  !s.is_empty()
    && s
      .chars()
      .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// the level's own folders in the data folder (relative, '/')
fn level_dirs(game: &str, name: &str) -> [String; 2] {
  [
    format!("custom_assets/{game}/levels/{name}/"),
    format!("goal_src/{game}/levels/{name}/"),
  ]
}

/// a path of the package: relative, inside the level's own folders
fn safe_path(f: &str, game: &str, name: &str) -> bool {
  let p = Path::new(f);
  p.components().all(|c| matches!(c, Component::Normal(_)))
    && level_dirs(game, name)
      .iter()
      .any(|d| f.starts_with(d.as_str()))
}

/// deletes the level's own folders, then the folders above them left empty
fn delete_level_dirs(data_dir: &Path, game: &str, name: &str) {
  for d in level_dirs(game, name) {
    let p = data_dir.join(d);
    let _ = std::fs::remove_dir_all(&p);
    let mut dir = p.parent();
    while let Some(d) = dir.filter(|d| d.starts_with(data_dir) && *d != data_dir) {
      // stops at the first folder that is not empty
      if std::fs::remove_dir(d).is_err() {
        break;
      }
      dir = d.parent();
    }
  }
}

/// the `:index` of the level's block (the one it was given when installed)
fn block_index(text: &str, name: &str) -> Option<u32> {
  let begin = text.find(&format!("{MARK}begin {name}"))?;
  let end = text[begin..].find(&format!("{MARK}end {name}"))? + begin;
  indexes(&text[begin..end]).first().copied()
}

/// game.gp and level-info.gc without the blocks of `name`, where they were
type Stripped = (PathBuf, String, Option<usize>);

fn texts_without(data_dir: &Path, game: &str, name: &str) -> Result<[Stripped; 2], String> {
  let gp = game_gp(data_dir, game);
  let info = level_info_gc(data_dir, game);
  let (gp_text, gp_had) = strip_blocks(&read(&gp)?, name);
  let (info_text, info_had) = strip_blocks(&read(&info)?, name);
  Ok([(gp, gp_text, gp_had), (info, info_text, info_had)])
}

/// Installs the level of `package` into `data_dir` (or installs its new version in place of the
/// one installed).
pub fn apply(data_dir: &Path, package: &LevelPackage) -> Result<(), String> {
  let m = &package.manifest;
  let (game, name) = (m.game.as_str(), m.name.as_str());
  if !is_name(game) || !is_name(name) || !is_name(&m.nickname) {
    return Err(format!(
      "jle-level.json: bad names ({game}, {name}, {})",
      m.nickname
    ));
  }
  if !m.replaces.is_empty() && !is_name(&m.replaces) {
    return Err(format!("jle-level.json: bad replaces ({})", m.replaces));
  }
  if let Some(f) = m.files.iter().find(|f| !safe_path(f, game, name)) {
    return Err(format!(
      "jle-level.json: a file outside the level's folders ({f})"
    ));
  }
  // only the lines the export writes
  let gp_ok = |l: &String| {
    !l.contains('\n')
      && (l.starts_with("(build-custom-level \"")
        || l.starts_with(&format!("(goal-src \"levels/{name}/"))
        || l.starts_with("(custom-level-cgo \""))
  };
  if m.game_gp.is_empty() || !m.game_gp.iter().all(gp_ok) {
    return Err("jle-level.json: unexpected game.gp lines".into());
  }
  let old_index = block_index(&read(&level_info_gc(data_dir, game))?, name);

  // the texts without this level's blocks (a version installed before)
  let [(gp, mut gp_text, gp_at), (info, mut info_text, info_at)] =
    texts_without(data_dir, game, name)?;
  let was_installed = gp_at.is_some();
  if gp_text.contains(&format!("(build-custom-level \"{name}\"")) {
    return Err(format!(
      "{}: {name} is already there (not added by the editor)",
      gp.display()
    ));
  }
  let gp_nl = newline(&gp_text);
  let gp_block = block(name, &m.game_gp, gp_nl);
  // where it was, else next to the test-zone's lines, else before the list of what (mi) builds
  let at = match (gp_at, gp_text.find("(custom-level-cgo \"TSZ.DGO\"")) {
    (Some(at), _) => at,
    (None, Some(i)) => line_range(&gp_text, i).1,
    (None, None) => gp_text
      .find("(group-list \"iso\"")
      .ok_or_else(|| format!("{}: where to add the level?", gp.display()))?,
  };
  gp_text.insert_str(at, &gp_block);

  let info_nl = newline(&info_text);
  if m.replaces.is_empty() {
    // a level of its own: its definition, with an index no other level has (the one it had)
    if find_define(&info_text, name).is_some() {
      return Err(format!(
        "{}: {name} is already defined (not by the editor)",
        info.display()
      ));
    }
    let def = read(&package.dir.join(&m.level_info))?;
    if find_define(&def, name).is_none() {
      return Err(format!("{}: no (define {name} ...)", m.level_info));
    }
    let taken = indexes(&info_text);
    let kept = old_index.filter(|i| *i != 0 && !taken.contains(i));
    let index = kept.unwrap_or_else(|| taken.iter().max().map_or(0x128, |m| m + 1));
    // from its define on (the export's comment above it left out)
    let lines: Vec<String> = def
      .lines()
      .skip_while(|l| !l.starts_with("(define"))
      .map(|l| match l.find(":index ") {
        Some(i) => format!("{}:index #x{index:x}", &l[..i]),
        None => l.to_string(),
      })
      .collect();
    let b = block(name, &lines, info_nl);
    // where it was, else before the test-zone's definition (and its comment), else at the end
    let at = info_at.or_else(|| {
      let d = find_define(&info_text, "test-zone")?;
      let line = line_range(&info_text, d).0;
      let above = line_range(&info_text, line.saturating_sub(1)).0;
      Some(if line > 0 && info_text[above..line].starts_with(";;") {
        above
      } else {
        line
      })
    });
    match at {
      Some(at) => info_text.insert_str(at, &b),
      None => {
        if !info_text.ends_with('\n') {
          info_text += info_nl;
        }
        info_text += &b;
      }
    }
  } else {
    // in place of a level: the nickname of its definition names this level's DGO
    let r = &m.replaces;
    let at = find_define(&info_text, r)
      .ok_or_else(|| format!("{}: no (define {r} ...)", info.display()))?;
    let nick = info_text[at..]
      .find(":nickname '")
      .map(|i| at + i)
      .ok_or_else(|| format!("{}: {r} has no :nickname", info.display()))?;
    let (start, end) = line_range(&info_text, nick);
    let line = &info_text[start..end];
    if line.starts_with(MARK) {
      return Err(format!(
        "{r} is already replaced by another level: remove it first"
      ));
    }
    let indent = &line[..nick - start];
    let original = line.trim_end_matches(['\r', '\n']).to_string();
    let b = block(
      name,
      &[
        format!("{MARK}was {original}"),
        format!("{indent}:nickname '{}", m.nickname),
      ],
      info_nl,
    );
    info_text.replace_range(start..end, &b);
  }

  // the files: the old version's first out
  if was_installed {
    delete_level_dirs(data_dir, game, name);
  }
  for f in &m.files {
    let (from, to) = (package.dir.join(f), data_dir.join(f));
    if let Some(parent) = to.parent() {
      std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
  }
  write_edited(&gp, &gp_text)?;
  write_edited(&info, &info_text)
}

/// Takes the level `name` out of `data_dir`: its blocks (the lines they replaced put back), its
/// folders.
pub fn remove(data_dir: &Path, game: &str, name: &str) -> Result<(), String> {
  if !installed(data_dir, game).iter().any(|n| n == name) {
    return Err(format!("{name} is not installed"));
  }
  for (path, text, had) in texts_without(data_dir, game, name)? {
    if had.is_some() {
      write(&path, &text)?;
    }
    forget_backup(&path);
  }
  delete_level_dirs(data_dir, game, name);
  Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::host::LevelManifest;
  use crate::io::test_library::temp_dir;

  /// game.gp and level-info.gc of jak-project (read only), else small stand-ins
  pub(crate) fn fake_data_dir(dir: &Path) {
    let jak = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../jak-project/goal_src/jak3");
    let gp = dir.join("goal_src/jak3/game.gp");
    let info = dir.join("goal_src/jak3/engine/level/level-info.gc");
    std::fs::create_dir_all(info.parent().unwrap()).unwrap();
    if std::fs::copy(jak.join("game.gp"), &gp).is_err() {
      std::fs::write(
        &gp,
        "(define *all-cgos* '())\r\n(build-custom-level \"test-zone\")\r\n(custom-level-cgo \"TSZ.DGO\" \"test-zone/testzone.gd\")\r\n(group-list \"iso\" ,@(reverse *all-cgos*))\r\n",
      )
      .unwrap();
    }
    if std::fs::copy(jak.join("engine/level/level-info.gc"), &info).is_err() {
      std::fs::write(
        &info,
        "(define ctyindb\n  (new 'static 'level-load-info\n    :name 'ctyindb\n    :nickname 'cib\n    :index #x127\n    ))\n(define test-zone\n  (new 'static 'level-load-info\n    :nickname 'tsz\n    :index #x128\n    ))\n",
      )
      .unwrap();
    }
  }

  /// an export of a level `name` (its files and manifest)
  pub(crate) fn package(dir: &Path, name: &str, replaces: &str, content: &str) -> LevelPackage {
    let level = format!("custom_assets/jak3/levels/{name}");
    std::fs::create_dir_all(dir.join(&level)).unwrap();
    std::fs::write(dir.join(format!("{level}/{name}.glb")), content).unwrap();
    std::fs::write(
      dir.join(format!("{level}/level-info.gc")),
      format!(";; add this\n\n(define {name}\n  (new 'static 'level-load-info\n    :name '{name}\n    :index #x128 ;; the test-zone's\n    ))\n"),
    )
    .unwrap();
    let m = LevelManifest {
      format: 1,
      game: "jak3".into(),
      name: name.into(),
      nickname: "mte".into(),
      replaces: replaces.into(),
      dgo: "MTE.DGO".into(),
      continue_point: format!("{name}-start"),
      game_gp: vec![
        format!("(build-custom-level \"{name}\")"),
        format!("(custom-level-cgo \"MTE.DGO\" \"{name}/{name}.gd\")"),
      ],
      level_info: if replaces.is_empty() {
        format!("{level}/level-info.gc")
      } else {
        String::new()
      },
      files: vec![format!("{level}/{name}.glb")],
      ..Default::default()
    };
    LevelPackage {
      dir: dir.to_path_buf(),
      manifest: m,
    }
  }

  #[test]
  fn installed_then_removed() {
    let tmp = temp_dir("level-package");
    let data = tmp.join("data");
    fake_data_dir(&data);
    let gp = game_gp(&data, "jak3");
    let info = level_info_gc(&data, "jak3");
    let (gp0, info0) = (std::fs::read(&gp).unwrap(), std::fs::read(&info).unwrap());
    let max0 = indexes(&String::from_utf8_lossy(&info0))
      .into_iter()
      .max()
      .unwrap();

    // a level of its own
    let p = package(&tmp.join("export"), "my-test", "", "v1");
    apply(&data, &p).unwrap();
    let (gp1, info1) = (read(&gp).unwrap(), read(&info).unwrap());
    assert!(
      gp1.contains(";; jak-level-editor: begin my-test")
        && gp1.find("(build-custom-level \"my-test\")") > gp1.find("(custom-level-cgo \"TSZ.DGO\"")
        && gp1.find("(build-custom-level \"my-test\")") < gp1.find("(group-list \"iso\"")
        && block_index(&info1, "my-test") == Some(max0 + 1)
        // (jak-project's level-info.gc has the line endings of its checkout)
        && info1.replace("\r\n", "\n").contains(&format!(":index #x{:x}\n", max0 + 1))
        && find_define(&info1, "my-test") < find_define(&info1, "test-zone")
        && !info1.contains(";; add this")
        && data
          .join("custom_assets/jak3/levels/my-test/my-test.glb")
          .is_file()
        && backup_path(&gp).is_file(),
      "installed"
    );
    // applied again: nothing changes; a new version replaces its block, the same index kept
    apply(&data, &p).unwrap();
    assert!(
      read(&gp).unwrap() == gp1 && read(&info).unwrap() == info1,
      "idempotent"
    );
    let p2 = package(&tmp.join("export"), "my-test", "", "v2");
    apply(&data, &p2).unwrap();
    assert!(
      read(&info).unwrap() == info1
        && read(&data.join("custom_assets/jak3/levels/my-test/my-test.glb")).unwrap() == "v2",
      "a new version, the same index kept"
    );
    assert_eq!(installed(&data, "jak3"), ["my-test"]);

    // in place of ctyindb; a second level in place of it is refused
    let q = package(&tmp.join("export-b"), "my-ctyb", "ctyindb", "b");
    apply(&data, &q).unwrap();
    let info2 = read(&info).unwrap();
    let at = find_define(&info2, "ctyindb").unwrap();
    let def = &info2[at..at + 400];
    assert!(
      def.contains(";; jak-level-editor: begin my-ctyb")
        && def.contains(":nickname 'cib")
        && def.contains(";; jak-level-editor: was ")
        && def.contains(":nickname 'mte"),
      "the nickname of ctyindb ({def})"
    );
    let q2 = package(&tmp.join("export-c"), "my-ctyc", "ctyindb", "c");
    assert!(apply(&data, &q2).is_err(), "one level in place of ctyindb");
    // in game.gp's order: each new block goes next to the test-zone's lines
    assert_eq!(installed(&data, "jak3"), ["my-ctyb", "my-test"]);

    // removed: the files as they were, no backup nor folder left
    remove(&data, "jak3", "my-ctyb").unwrap();
    assert_eq!(read(&info).unwrap(), info1);
    remove(&data, "jak3", "my-test").unwrap();
    assert!(
      std::fs::read(&gp).unwrap() == gp0
        && std::fs::read(&info).unwrap() == info0
        && !backup_path(&gp).exists()
        && !backup_path(&info).exists()
        && !data.join("custom_assets").exists(),
      "removed"
    );
    assert!(remove(&data, "jak3", "my-test").is_err());

    // a manifest reaching outside the level's folders
    let mut bad = package(&tmp.join("export"), "my-test", "", "v1");
    bad.manifest.files = vec!["custom_assets/jak3/levels/../../../x".into()];
    assert!(apply(&data, &bad).is_err());
    bad.manifest.files = vec!["custom_assets/jak3/levels/other/x.glb".into()];
    assert!(apply(&data, &bad).is_err(), "another level's folder");
    let _ = std::fs::remove_dir_all(&tmp);
  }
}
