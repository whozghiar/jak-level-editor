//! The extraction of a game's levels into the library (File > Extract game assets), from the game
//! the user installed with OpenGOAL. Its own extractor reads the disc's files it extracted (its
//! "decompile" step, by batches of levels, in a folder of the editor's: nothing is written in the
//! installation) and writes what its game reads: each level's .fr3 and its objects. The editor
//! writes the library from them:
//!
//!   <library>/<game>/library.json                           the game, the levels extracted
//!   <library>/<game>/<level>/decor/<level>-background.glb   decor.rs: the .fr3 and the bsp
//!   <library>/<game>/<level>/models/<model>-lodN.glb        models.rs: the .fr3 and the art groups
//!   <library>/<game>/<level>/textures/*.png                 its textures (when asked)
//!   <library>/<game>/<level>/level.json                     what the folder holds
//!   <library>/<game>/common/models/                         the models every level can use
//!   <library>/<game>/common/sprites/<page>/<texture>.png    the particles' textures; index.json:
//!                                                           "<page id>:<index>" -> "<page>/<texture>"

pub mod art;
pub mod bsp;
pub mod decor;
pub mod fr3;
mod goal_object;
mod lzo;
pub mod models;
mod strips;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde_json::{Value, json};

use crate::io::game_data::data_dir;
use crate::io::game_install::{GameInstall, config_file, copy_folder, read_jsonc};

/// Levels one extractor reads (in parallel, about 2 GB): each run reads the common file once
const BATCH: usize = 34;
/// Extractors running at once
const WORKERS: usize = 2;

/// What an extraction says as it goes (editor/extract.rs shows it)
#[derive(Clone, Debug, PartialEq)]
pub enum Report {
  /// a line of the extractor's output, or of the editor's
  Line(String),
  /// a step: a key of lang/*.json (extract.step.*) with its numbers
  Step(&'static str, Vec<usize>),
  /// a level written, of `total` (the common models count as one)
  Level {
    done: usize,
    total: usize,
    name: String,
  },
}

#[derive(Clone, Debug, Default)]
pub struct Options {
  /// the DGOs to extract ("CIA.DGO"); all of the game's when empty
  pub levels: Vec<String>,
  /// also each level's textures as .png
  pub textures: bool,
}

/// An extraction: what it reads, where it writes, how it is stopped
pub struct Job<'a> {
  pub install: &'a GameInstall,
  /// the library's folder: the levels go to <library>/<game>/
  pub library: &'a Path,
  /// the editor's folder the extractor works in
  pub work: &'a Path,
  pub options: &'a Options,
  pub cancel: &'a AtomicBool,
  /// the extractors running, by process id: killed to stop
  pub children: &'a Mutex<HashMap<u32, Child>>,
}

fn io_err(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
  move |e| format!("{}: {e}", path.display())
}

/// A name for a file: what is not a letter, a digit, - _ or . becomes _
fn file_safe(s: &str) -> String {
  s.chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || "-_.".contains(c) {
        c
      } else {
        '_'
      }
    })
    .collect()
}

fn strings(v: &Value) -> impl Iterator<Item = &str> {
  v.as_array().into_iter().flatten().filter_map(Value::as_str)
}

/// The decompiler's configuration of the installation, and its inputs (the DGOs, the levels...)
fn configuration(install: &GameInstall) -> Result<(Value, Value), String> {
  let config = read_jsonc(&config_file(&install.data(), &install.game))?;
  let inputs = config["inputs_file"]
    .as_str()
    .ok_or("no inputs_file in the configuration")?;
  let inputs = read_jsonc(&install.data().join(inputs))?;
  Ok((config, inputs))
}

/// The levels of the game (its inputs' levels_to_extract) whose DGO is on the disc, the ones asked
/// only when some are
pub fn game_levels(install: &GameInstall, asked: &[String]) -> Result<Vec<String>, String> {
  let (_, inputs) = configuration(install)?;
  let disc = install
    .data()
    .join("iso_data")
    .join(&install.game)
    .join("DGO");
  let on_disc: HashSet<String> = std::fs::read_dir(&disc)
    .map_err(io_err(&disc))?
    .flatten()
    .map(|e| e.file_name().to_string_lossy().to_uppercase())
    .collect();
  let asked: HashSet<String> = asked.iter().map(|a| a.to_uppercase()).collect();
  Ok(
    strings(&inputs["levels_to_extract"])
      .filter(|d| on_disc.contains(&d.to_uppercase()))
      .filter(|d| asked.is_empty() || asked.contains(&d.to_uppercase()))
      .map(str::to_string)
      .collect(),
  )
}

// the first folder in `dir` holding `name`: the extractor names its output folders after the
// game's version (jak3, jak3_pal, jak2_us2...)
fn output_folder(dir: &Path, name: &str) -> Option<PathBuf> {
  let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
    .ok()?
    .flatten()
    .map(|e| e.path())
    .filter(|p| p.join(name).exists())
    .collect();
  dirs.sort();
  dirs.into_iter().next()
}

// the extractor's outputs of a level, gone (none is a link: nothing outside them is touched)
fn clean_outputs(data: &Path) -> Result<(), String> {
  for d in ["decompiler_out", "out"] {
    let dir = data.join(d);
    if dir.exists() {
      std::fs::remove_dir_all(&dir).map_err(io_err(&dir))?;
    }
  }
  Ok(())
}

/// Extracts the levels: the number written
pub fn run(job: &Job, report: &(dyn Fn(Report) + Sync)) -> Result<usize, String> {
  let install = job.install;
  let game = install.game.as_str();
  let cancelled = || job.cancel.load(Ordering::Relaxed);
  let (_, inputs) = configuration(install)?;
  let levels = game_levels(install, &job.options.levels)?;
  if levels.is_empty() {
    return Err("no level to extract".into());
  }
  let out = job.library.join(game);
  std::fs::create_dir_all(&out).map_err(io_err(&out))?;
  let ctx = Context {
    game,
    out,
    textures: job.options.textures,
    sprite_pages: sprite_pages(game),
    texture_slots: texture_slots(game),
    owners: Mutex::new(HashMap::new()),
    sprites: Mutex::new(BTreeMap::new()),
    done: AtomicUsize::new(0),
    total: levels.len() + 1,
    written: Mutex::new(BTreeSet::new()),
  };
  let batches: Vec<&[String]> = levels.chunks(BATCH).collect();
  let workers = WORKERS.min(batches.len());
  report(Report::Step("levels", vec![levels.len()]));
  let next = AtomicUsize::new(0);
  let common = AtomicBool::new(false);
  let failure: Mutex<Option<String>> = Mutex::new(None);
  std::thread::scope(|s| {
    for w in 0..workers {
      let (next, common, failure, ctx, inputs, batches) =
        (&next, &common, &failure, &ctx, &inputs, &batches);
      s.spawn(move || {
        // the extractor's project: the installation's configuration, its inputs ours
        let data = job.work.join(w.to_string()).join("data");
        let config = |d: &Path| d.join("decompiler").join("config").join(game);
        if let Err(e) =
          clean_outputs(&data).and_then(|_| copy_folder(&config(&install.data()), &config(&data)))
        {
          *failure.lock().unwrap() = Some(e);
          return;
        }
        while let Some(batch) = batches.get(next.fetch_add(1, Ordering::Relaxed)) {
          if cancelled() || failure.lock().unwrap().is_some() {
            break;
          }
          match extract_batch(job, ctx, &data, inputs, batch, common, report) {
            Ok(()) => {}
            Err(_) if cancelled() => break,
            // a level the extractor cannot read stops its batch: the others are read alone
            Err(e) if batch.len() > 1 => {
              report(Report::Line(format!(
                "{}..{}: {e}",
                batch[0],
                batch[batch.len() - 1]
              )));
              for dgo in batch.iter() {
                if cancelled() {
                  break;
                }
                if let Err(e) = extract_batch(
                  job,
                  ctx,
                  &data,
                  inputs,
                  std::slice::from_ref(dgo),
                  common,
                  report,
                ) {
                  report(Report::Line(format!("{dgo}: {e}")));
                }
              }
            }
            Err(e) => report(Report::Line(format!("{}: {e}", batch[0]))),
          }
        }
        let _ = clean_outputs(&data);
      });
    }
  });
  if cancelled() {
    return Err("cancelled".into());
  }
  if let Some(e) = failure.into_inner().unwrap() {
    return Err(e);
  }
  ctx.finish(install)?;
  let written = ctx.written.lock().unwrap().len();
  Ok(written)
}

/// A batch of levels (`dgos`): the extractor reads them with the common files, then the editor
/// writes them (several at once; and the common models, the first time)
fn extract_batch(
  job: &Job,
  ctx: &Context,
  data: &Path,
  inputs: &Value,
  dgos: &[String],
  common: &AtomicBool,
  report: &(dyn Fn(Report) + Sync),
) -> Result<(), String> {
  clean_outputs(data)?;
  // the common file (the shared models and textures; not the kernel's code), then the levels'
  // DGOs; the streamed art stays (JAEXTERN.STR): the common models' animations
  let entry = |dgo: &String| {
    strings(&inputs["dgo_names"])
      .find(|n| n.rsplit('/').next() == Some(dgo.as_str()))
      .map_or_else(|| format!("DGO/{dgo}"), str::to_string)
  };
  let names: Vec<String> = strings(&inputs["dgo_names"])
    .filter(|n| n.ends_with(".CGO") && !n.ends_with("KERNEL.CGO"))
    .map(str::to_string)
    .chain(dgos.iter().map(entry))
    .collect();
  let mut b = inputs.clone();
  b["dgo_names"] = json!(names);
  b["levels_to_extract"] = json!(dgos);
  b["str_file_names"] = json!([]);
  b["streamed_audio_file_names"] = json!([]);
  let path = data.join("inputs.json");
  std::fs::write(&path, b.to_string()).map_err(io_err(&path))?;
  // what the library takes: the levels' .fr3 and the objects (dumped: the levels' bsp, the art
  // groups); not the models as glTF (the editor writes them: OpenGOAL's lose animations at random,
  // reading freed memory in fr3_to_gltf.cpp, add_merc), the code's functions, the texts, the
  // particles' table
  let overrides = json!({
    "inputs_file": path,
    "rip_levels": false,
    "rip_collision": false,
    "save_texture_pngs": false,
    "find_functions": false,
    "process_game_text": false,
    "process_game_count": false,
    "process_part_group_table": false,
  });
  let label = match dgos {
    [one] => one.clone(),
    _ => format!("{}..{}", dgos[0], dgos[dgos.len() - 1]),
  };
  run_extractor(job, data, &overrides, &label, report)?;

  let decomp = output_folder(&data.join("decompiler_out"), "dgo.txt")
    .ok_or("the extractor wrote no dgo.txt")?;
  let fr3_dir = output_folder(&data.join("out"), "fr3")
    .ok_or("the extractor wrote no .fr3")?
    .join("fr3");
  let dgo_txt = decomp.join("dgo.txt");
  let listing = bsp::dgo_objects(&std::fs::read_to_string(&dgo_txt).map_err(io_err(&dgo_txt))?);
  let written = Batch {
    decomp: &decomp,
    fr3_dir: &fr3_dir,
    dgos: &listing,
  };
  if !common.swap(true, Ordering::Relaxed) {
    ctx.common(&written, report);
  }
  // a level two DGOs hold (Jak II's nest): the extractor names its .fr3 after the level, the
  // second's overwrites the first's; the second is read again alone, after
  let mut names = HashSet::new();
  let (now, later): (Vec<&String>, Vec<&String>) = dgos.iter().partition(|dgo| {
    ctx
      .level_name(&written, dgo)
      .is_none_or(|n| names.insert(n))
  });
  // the levels, as many at once as the processor has threads
  let next = AtomicUsize::new(0);
  let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
  std::thread::scope(|s| {
    for _ in 0..threads.min(now.len()) {
      s.spawn(|| {
        while let Some(dgo) = now.get(next.fetch_add(1, Ordering::Relaxed)) {
          if job.cancel.load(Ordering::Relaxed) {
            break;
          }
          match ctx.level(&written, dgo, report) {
            Ok(name) => report(Report::Level {
              done: ctx.done.fetch_add(1, Ordering::Relaxed) + 1,
              total: ctx.total,
              name,
            }),
            Err(e) => report(Report::Line(format!("{dgo}: {e}"))),
          }
        }
      });
    }
  });
  for dgo in later {
    if let Err(e) = extract_batch(
      job,
      ctx,
      data,
      inputs,
      std::slice::from_ref(dgo),
      common,
      report,
    ) {
      report(Report::Line(format!("{dgo}: {e}")));
    }
  }
  Ok(())
}

/// OpenGOAL's extractor, its decompile step: the disc's files and the configuration read where
/// the installation has them, its outputs in `data`; its errors reported
fn run_extractor(
  job: &Job,
  data: &Path,
  overrides: &Value,
  dgo: &str,
  report: &(dyn Fn(Report) + Sync),
) -> Result<(), String> {
  let install = job.install;
  let disc = install.data().join("iso_data").join(&install.game);
  let mut cmd = Command::new(&install.extractor);
  cmd
    .arg(&disc)
    .args([
      "--folder",
      "--decompile",
      "--disable-ansi",
      "-g",
      &install.game,
    ])
    .arg("--proj-path")
    .arg(data)
    .arg("--extract-path")
    .arg(&disc)
    .arg("--decomp-config-override")
    .arg(overrides.to_string())
    .current_dir(data);
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
  }
  // stdout and stderr in one pipe, in the order it wrote them
  let (child, reader) = std::io::pipe()
    .and_then(|(reader, writer)| {
      cmd
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer);
      Ok((cmd.spawn()?, reader))
    })
    .map_err(|e| format!("{}: {e}", install.extractor))?;
  drop(cmd); // its ends of the pipe: the reading ends when the extractor exits
  let pid = child.id();
  job.children.lock().unwrap().insert(pid, child);
  if job.cancel.load(Ordering::Relaxed) {
    // stopped while it started
    if let Some(c) = job.children.lock().unwrap().get_mut(&pid) {
      let _ = c.kill();
    }
  }
  let mut last = Vec::new();
  for line in BufReader::new(reader).split(b'\n') {
    let Ok(line) = line else { break };
    let line = String::from_utf8_lossy(&line).trim_end().to_string();
    if line.is_empty() {
      continue;
    }
    if line.contains("[error]") {
      report(Report::Line(format!("{dgo}: {line}")));
    }
    last.push(line);
    if last.len() > 6 {
      last.remove(0);
    }
  }
  let status = job
    .children
    .lock()
    .unwrap()
    .remove(&pid)
    .map(|mut c| c.wait());
  if job.cancel.load(Ordering::Relaxed) {
    return Err("cancelled".into());
  }
  match status {
    Some(Ok(s)) if s.success() => Ok(()),
    Some(Ok(s)) => Err(format!("the extractor failed ({s}): {}", last.join(" / "))),
    Some(Err(e)) => Err(e.to_string()),
    None => Err("cancelled".into()),
  }
}

/// The particles' texture pages (data/<game>/sprite-pages.json): their names and ids
fn sprite_pages(game: &str) -> (HashSet<String>, HashSet<u32>) {
  let path = data_dir().join(game).join("sprite-pages.json");
  let j: Value = std::fs::read_to_string(path)
    .ok()
    .and_then(|t| serde_json::from_str(&t).ok())
    .unwrap_or_default();
  (
    strings(&j["pages"]).map(str::to_string).collect(),
    j["page_ids"]
      .as_array()
      .into_iter()
      .flatten()
      .filter_map(Value::as_u64)
      .map(|i| i as u32)
      .collect(),
  )
}

/// The names of the animated texture slots of OpenGOAL's renderer, in order
fn texture_slots(game: &str) -> Vec<String> {
  let path = data_dir().join(game).join("texture-slots.json");
  let j: Value = std::fs::read_to_string(path)
    .ok()
    .and_then(|t| serde_json::from_str(&t).ok())
    .unwrap_or_default();
  strings(&j["slots"]).map(str::to_string).collect()
}

/// What the extractor wrote for a batch of levels
struct Batch<'a> {
  decomp: &'a Path,
  fr3_dir: &'a Path,
  dgos: &'a BTreeMap<String, Vec<String>>,
}

struct Context<'a> {
  game: &'a str,
  /// <library>/<game>
  out: PathBuf,
  textures: bool,
  sprite_pages: (HashSet<String>, HashSet<u32>),
  /// the names of the animated texture slots (data/<game>/texture-slots.json)
  texture_slots: Vec<String>,
  /// level name -> the DGO whose folder it has
  owners: Mutex<HashMap<String, String>>,
  /// the sprites written: "<page id>:<index>" -> "<page>/<texture>"
  sprites: Mutex<BTreeMap<String, String>>,
  done: AtomicUsize,
  total: usize,
  /// the DGOs written
  written: Mutex<BTreeSet<String>>,
}

// a texture with its pixels
fn has_pixels(t: &fr3::Texture) -> bool {
  t.w > 0 && t.h > 0 && t.rgba.len() >= t.w as usize * t.h as usize * 4
}

impl Context<'_> {
  /// The particles' textures among a level's (or the common file's), once each
  fn sprites(&self, level: &fr3::Level) {
    let (pages, ids) = &self.sprite_pages;
    for t in &level.textures {
      let page = t.combo_id >> 16;
      if !(pages.contains(&t.tpage) || ids.contains(&page)) || !has_pixels(t) {
        continue;
      }
      let key = format!("{page}:{}", t.combo_id & 0xffff);
      let rel = format!("{}/{}", file_safe(&t.tpage), file_safe(&t.name));
      if self
        .sprites
        .lock()
        .unwrap()
        .insert(key, rel.clone())
        .is_some()
      {
        continue;
      }
      let file = self.out.join("common/sprites").join(format!("{rel}.png"));
      let _ = std::fs::create_dir_all(file.parent().unwrap());
      if let Some(png) = crate::io::gltf::png(t.w as i32, t.h as i32, &t.rgba) {
        let _ = std::fs::write(&file, png);
      }
    }
  }

  /// The skeletons and animations of the art groups of `dgos` (in their order)
  fn art(&self, b: &Batch, dgos: &[&str], report: &(dyn Fn(Report) + Sync)) -> art::Art {
    let mut out = art::Art::new();
    for dgo in dgos {
      for name in b.dgos.get(*dgo).into_iter().flatten() {
        if !name.ends_with("-ag") {
          continue;
        }
        let path = b.decomp.join("raw_obj").join(format!("{name}.go"));
        if let Err(e) = std::fs::read(&path)
          .map_err(|e| e.to_string())
          .and_then(|data| art::read_art_group(&data, self.game, &mut out))
        {
          report(Report::Line(format!("{}: {e}", path.display())));
        }
      }
    }
    out
  }

  /// Writes a level's models to `dir`
  fn models(&self, level: &fr3::Level, art: &art::Art, dir: &Path) -> Result<(), String> {
    let slots = models::slot_textures(level, &self.texture_slots);
    for m in &level.models {
      models::write_model(level, m, art, &slots, &dir.join(format!("{}.glb", m.name)))?;
    }
    Ok(())
  }

  /// The common file's models (collectables, Jak...) and the particles' textures it has
  fn common(&self, b: &Batch, report: &(dyn Fn(Report) + Sync)) {
    match fr3::read_file(&b.fr3_dir.join("GAME.fr3")) {
      Ok(level) => {
        let art = self.art(b, &["GAME.CGO", "ARTSPOOL"], report);
        if let Err(e) = self.models(&level, &art, &self.out.join("common").join("models")) {
          report(Report::Line(e));
        }
        self.sprites(&level);
      }
      Err(e) => report(Report::Line(e)),
    }
    report(Report::Level {
      done: self.done.fetch_add(1, Ordering::Relaxed) + 1,
      total: self.total,
      name: "common".into(),
    });
  }

  /// The name of the level a DGO holds (its bsp's)
  fn level_name(&self, b: &Batch, dgo: &str) -> Option<String> {
    let bsp_name = b.dgos.get(dgo)?.last()?;
    let data = std::fs::read(b.decomp.join("raw_obj").join(format!("{bsp_name}.go"))).ok()?;
    bsp::read_bsp(&data, self.game).ok().map(|b| b.name)
  }

  /// A level (`dgo`): its folder's name
  fn level(
    &self,
    b: &Batch,
    dgo: &str,
    report: &(dyn Fn(Report) + Sync),
  ) -> Result<String, String> {
    let bsp_name = b
      .dgos
      .get(dgo)
      .and_then(|o| o.last())
      .ok_or("not in the extractor's dgo.txt")?;
    let raw = b.decomp.join("raw_obj").join(format!("{bsp_name}.go"));
    let mut bsp = bsp::read_bsp(&std::fs::read(&raw).map_err(io_err(&raw))?, self.game)
      .map_err(|e| format!("{}: {e}", raw.display()))?;
    // the actors' scripts on several lines, as the extractor prints them
    let printed = b
      .decomp
      .join("entities")
      .join(format!("{}-actors.json", bsp.name));
    if let Ok(j) = read_jsonc(&printed) {
      bsp::printed_scripts(&mut bsp.actors, &j);
    }
    let level = fr3::read_file(&b.fr3_dir.join(format!("{}.fr3", bsp.name)))?;
    // a second DGO holding the same level goes to <level>-<dgo> (ponytail: the first read, the
    // first of the game's list when both are in one batch; Jak II's two are)
    let folder = {
      let mut owners = self.owners.lock().unwrap();
      let owner = owners
        .entry(level.name.clone())
        .or_insert_with(|| dgo.to_string());
      if owner == dgo {
        level.name.clone()
      } else {
        let stem = dgo.split('.').next().unwrap_or(dgo).to_lowercase();
        format!("{}-{stem}", level.name)
      }
    };
    let dir = self.out.join(&folder);
    let decor = dir.join("decor").join(format!("{folder}-background.glb"));
    decor::write_decor(&level, self.game, &bsp.actors, &decor)?;
    let art = self.art(b, &[dgo], report);
    self.models(&level, &art, &dir.join("models"))?;
    let mut pngs = 0;
    if self.textures {
      let textures = dir.join("textures");
      let _ = std::fs::create_dir_all(&textures);
      for t in level.textures.iter().filter(|t| has_pixels(t)) {
        if let Some(png) = crate::io::gltf::png(t.w as i32, t.h as i32, &t.rgba)
          && std::fs::write(textures.join(format!("{}.png", file_safe(&t.name))), png).is_ok()
        {
          pngs += 1;
        }
      }
    }
    self.sprites(&level);
    let info = level_info(&level, dgo, self.game, &folder, &bsp, pngs);
    let path = dir.join("level.json");
    std::fs::write(&path, serde_json::to_string_pretty(&info).unwrap()).map_err(io_err(&path))?;
    self.written.lock().unwrap().insert(dgo.to_string());
    Ok(folder)
  }

  /// The sprites' index, library.json (the levels of this extraction with the earlier ones')
  fn finish(&self, install: &GameInstall) -> Result<(), String> {
    let sprites = self.sprites.lock().unwrap();
    if !sprites.is_empty() {
      let path = self.out.join("common/sprites/index.json");
      std::fs::write(&path, serde_json::to_string_pretty(&*sprites).unwrap())
        .map_err(io_err(&path))?;
    }
    let path = self.out.join("library.json");
    let before = read_jsonc(&path).unwrap_or_default();
    let mut levels: BTreeSet<String> = self.written.lock().unwrap().clone();
    levels.extend(strings(&before["levels"]).map(str::to_string));
    let disc = install.data().join("iso_data").join(&install.game);
    let buildinfo = read_jsonc(&disc.join("buildinfo.json")).unwrap_or_default();
    let lib = json!({
      "game": self.game,
      "serial": buildinfo[0]["serial"].as_str().unwrap_or(""),
      "tooling": install.version,
      "levels": levels,
      "generator": "jak-level-editor, from OpenGOAL's extractor",
    });
    std::fs::write(&path, serde_json::to_string_pretty(&lib).unwrap()).map_err(io_err(&path))
  }
}

/// level.json: what the folder holds; the actors by type (how many, the data of the first one: a
/// template for new ones, the art-names of all); the regions
fn level_info(
  level: &fr3::Level,
  dgo: &str,
  game: &str,
  folder: &str,
  bsp: &bsp::Bsp,
  pngs: usize,
) -> Value {
  let mut types = serde_json::Map::new();
  for actor in &bsp.actors {
    let etype = actor["etype"].as_str().unwrap_or("");
    if etype.is_empty() {
      continue;
    }
    let t = types.entry(etype).or_insert_with(
      || json!({"count": 0, "lump": actor["lump"], "lump_types": actor["lump_types"]}),
    );
    t["count"] = json!(t["count"].as_i64().unwrap_or(0) + 1);
    if let Some(name) = actor["lump"]["art-name"].as_str() {
      let names = t
        .as_object_mut()
        .unwrap()
        .entry("art_names")
        .or_insert(json!([]));
      if let Some(list) = names.as_array_mut()
        && !list.iter().any(|n| n == name)
      {
        list.push(json!(name));
      }
    }
  }
  let mut info = json!({
    "level": level.name,
    "dgo": dgo,
    "game": game,
    "decor": format!("decor/{folder}-background.glb"),
    "actors": bsp.actors.len(),
    "actor_types": types,
    "collision_triangles": level.collision.len() / 3,
    "models": level.models.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
    "textures": level.textures.len(),
    "texture_pngs": pngs,
  });
  // the regions are read in Jak 3's levels only
  if game == "jak3" {
    info["region_trees"] = bsp.region_trees.clone();
  }
  info
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn actors_listed_by_type() {
    let actor = |etype: &str, art: &str| json!({"etype": etype, "lump": {"name": "x", "art-name": art}, "lump_types": {"name": "string"}});
    let b = bsp::Bsp {
      name: "lvl".into(),
      actors: vec![
        actor("crate", "crate-a"),
        actor("crate", "crate-b"),
        actor("crate", "crate-a"),
        actor("door", "door-1"),
      ],
      region_trees: json!({}),
    };
    let level = fr3::Level {
      name: "lvl".into(),
      ..Default::default()
    };
    let info = level_info(&level, "LVL.DGO", "jak3", "lvl", &b, 0);
    assert_eq!(info["actors"], 4);
    assert_eq!(info["actor_types"]["crate"]["count"], 3);
    assert_eq!(
      info["actor_types"]["crate"]["art_names"],
      json!(["crate-a", "crate-b"])
    );
    assert_eq!(info["decor"], "decor/lvl-background.glb");
    assert_eq!(file_safe("a b/c.png"), "a_b_c.png");
  }

  /// The whole extraction from the user's installation: JLE_GAME_INSTALL=<OpenGOAL Launcher folder>
  /// JLE_EXTRACT_OUT=<library folder> [JLE_EXTRACT_LEVELS=CIA.DGO,CTA.DGO] cargo test ... -- --ignored
  #[test]
  #[ignore]
  fn extracts_from_the_installation() {
    let (Ok(root), Ok(out)) = (
      std::env::var("JLE_GAME_INSTALL"),
      std::env::var("JLE_EXTRACT_OUT"),
    ) else {
      return;
    };
    let game = std::env::var("JLE_GAME").unwrap_or("jak3".into());
    let install = crate::io::game_install::find_extraction(&game, root.as_ref()).unwrap();
    let options = Options {
      levels: std::env::var("JLE_EXTRACT_LEVELS")
        .map(|l| l.split(',').map(str::to_string).collect())
        .unwrap_or_default(),
      textures: false,
    };
    let (cancel, children) = (AtomicBool::new(false), Mutex::new(HashMap::new()));
    let work = Path::new(&out).join("_work");
    let job = Job {
      install: &install,
      library: out.as_ref(),
      work: &work,
      options: &options,
      cancel: &cancel,
      children: &children,
    };
    let t = std::time::Instant::now();
    let n = run(&job, &|r| match r {
      Report::Line(l) if l.contains("merc failed") => {}
      r => eprintln!("{r:?}"),
    })
    .unwrap();
    eprintln!("{n} levels in {:?}", t.elapsed());
  }
}
