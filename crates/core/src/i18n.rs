//! Translations of the interface. Each language is a json file in the lang/ folder next to the
//! executable (lang/en.json, lang/fr.json...), mapping keys to texts:
//!
//!   {"_language": "Français", "menu.file": "Fichier", "status.objects": "%zu objets", ...}
//!
//! en.json holds every key. A key missing from another language falls back to English, then to the
//! key itself. A text with printf arguments keeps the same arguments in every language (a test
//! checks it). Adding a language: copy en.json to <code>.json and translate the values.
//!
//! The engine uses them for its log and the answers of the MCP server; the interface has its own
//! reader of the same files (ui/lib/i18n.ts). `strf` formats their printf arguments.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, RwLock};

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Language {
  /// file name without .json: "en", "fr"
  pub code: String,
  /// its "_language" entry: "English", "Français"
  pub name: String,
}

struct Table {
  lang: Language,
  texts: HashMap<String, String>,
}

struct State {
  // Tables are never freed: the texts tr() returns stay valid when the language changes.
  tables: Vec<&'static Table>,
  languages: Vec<Language>,
  current: Option<&'static Table>,
  english: Option<&'static Table>,
  code: String,
}

static STATE: LazyLock<RwLock<State>> = LazyLock::new(|| {
  RwLock::new(State {
    tables: Vec::new(),
    languages: Vec::new(),
    current: None,
    english: None,
    code: "en".into(),
  })
});
static LANG_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

fn read_state() -> std::sync::RwLockReadGuard<'static, State> {
  STATE.read().unwrap_or_else(|e| e.into_inner())
}

fn find_table(st: &State, code: &str) -> Option<&'static Table> {
  st.tables.iter().copied().find(|t| t.lang.code == code)
}

fn select_in(st: &mut State, code: &str) -> bool {
  let Some(t) = find_table(st, code) else {
    return false;
  };
  st.current = Some(t);
  st.code = code.to_string();
  true
}

/// The folder of the translations the application ships (its resources): `lang_dir()` then
/// returns it.
pub fn set_lang_dir(dir: impl Into<PathBuf>) {
  *LANG_DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.into());
}

/// The folder of the translations: the one the application set, else lang/ next to the
/// executable, else the repository's (a build in development).
pub fn lang_dir() -> PathBuf {
  if let Some(dir) = LANG_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
    return dir;
  }
  if let Some(dir) = std::env::current_exe()
    .ok()
    .and_then(|e| e.parent().map(|p| p.join("lang")))
    && dir.is_dir()
  {
    return dir;
  }
  Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lang")
}

/// Reads the languages of `dir` and selects `code` (English when there is no such file).
pub fn i18n_init(dir: &Path, code: &str) {
  // logged once the lock is released (a hook of the log may translate)
  let mut warnings = Vec::new();
  let mut st = STATE.write().unwrap_or_else(|e| e.into_inner());
  for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
    let path = entry.path();
    if !path.is_file() || path.extension().is_none_or(|e| e != "json") {
      continue;
    }
    let lang_code = path
      .file_stem()
      .map(|s| s.to_string_lossy().into_owned())
      .unwrap_or_default();
    if find_table(&st, &lang_code).is_some() {
      continue;
    }
    let json: serde_json::Value = std::fs::read(&path)
      .ok()
      .and_then(|bytes| serde_json::from_slice(&bytes).ok())
      .unwrap_or_default();
    let Some(object) = json.as_object() else {
      warnings.push(crate::strf!(
        "%s: not a translation file",
        path.display().to_string()
      ));
      continue;
    };
    let name = object
      .get("_language")
      .and_then(|v| v.as_str())
      .unwrap_or(&lang_code)
      .to_string();
    let texts = object
      .iter()
      .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
      .collect::<HashMap<_, _>>();
    let table: &'static Table = Box::leak(Box::new(Table {
      lang: Language {
        code: lang_code,
        name,
      },
      texts,
    }));
    st.tables.push(table);
  }
  st.languages = st.tables.iter().map(|t| t.lang.clone()).collect();
  st.languages.sort_by(|a, b| {
    (b.code == "en")
      .cmp(&(a.code == "en"))
      .then_with(|| a.name.cmp(&b.name))
  });
  st.english = find_table(&st, "en");
  if st.english.is_none() {
    warnings.push(crate::strf!(
      "no English translation in %s: the interface shows its keys",
      dir.display().to_string()
    ));
  }
  if !select_in(&mut st, code) {
    select_in(&mut st, "en");
  }
  drop(st);
  for w in warnings {
    crate::log::Log::get().add(crate::log::LogLevel::Warn, w);
  }
}

pub fn i18n_select(code: &str) -> bool {
  select_in(&mut STATE.write().unwrap_or_else(|e| e.into_inner()), code)
}

/// code of the selected language
pub fn i18n_language() -> String {
  read_state().code.clone()
}

/// the files of the folder, English first
pub fn i18n_languages() -> Vec<Language> {
  read_state().languages.clone()
}

/// The language the system prefers among those with a file, else "en".
pub fn i18n_system_language() -> String {
  let st = read_state();
  for locale in system_locales() {
    // "fr-FR", "fr_FR.UTF-8": the language
    let language = locale.split(['-', '_', '.', '@']).next().unwrap_or("");
    if !language.is_empty() && find_table(&st, language).is_some() {
      return language.to_string();
    }
  }
  "en".into()
}

#[cfg(windows)]
fn system_locales() -> Vec<String> {
  #[link(name = "kernel32")]
  unsafe extern "system" {
    fn GetUserPreferredUILanguages(
      flags: u32,
      count: *mut u32,
      buffer: *mut u16,
      size: *mut u32,
    ) -> i32;
  }
  const MUI_LANGUAGE_NAME: u32 = 0x8;
  let (mut count, mut size) = (0u32, 0u32);
  // SAFETY: the first call only asks for the size; the second fills a buffer of that size
  unsafe {
    if GetUserPreferredUILanguages(
      MUI_LANGUAGE_NAME,
      &mut count,
      std::ptr::null_mut(),
      &mut size,
    ) == 0
      || size == 0
    {
      return Vec::new();
    }
    let mut buffer = vec![0u16; size as usize];
    if GetUserPreferredUILanguages(
      MUI_LANGUAGE_NAME,
      &mut count,
      buffer.as_mut_ptr(),
      &mut size,
    ) == 0
    {
      return Vec::new();
    }
    buffer
      .split(|&c| c == 0)
      .filter(|s| !s.is_empty())
      .map(String::from_utf16_lossy)
      .collect()
  }
}

#[cfg(not(windows))]
fn system_locales() -> Vec<String> {
  let mut out = Vec::new();
  // macOS: the languages of System Settings (an application opened from the Finder has no LANG)
  #[cfg(target_os = "macos")]
  if let Ok(o) = std::process::Command::new("defaults")
    .args(["read", "-g", "AppleLanguages"])
    .output()
  {
    out.extend(plist_strings(&String::from_utf8_lossy(&o.stdout)));
  }
  for var in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
    if let Ok(v) = std::env::var(var) {
      out.extend(
        v.split(':')
          .filter(|l| !l.is_empty() && *l != "C" && *l != "POSIX")
          .map(str::to_string),
      );
    }
  }
  out
}

/// The strings of a list as `defaults read` prints it: `(\n    "fr-FR",\n    en\n)`
#[cfg(any(target_os = "macos", test))]
fn plist_strings(text: &str) -> Vec<String> {
  text
    .split([',', '(', ')', '\n'])
    .map(|s| s.trim().trim_matches('"').to_string())
    .filter(|s| !s.is_empty())
    .collect()
}

/// The text of a key in the selected language.
pub fn tr(key: &str) -> &str {
  let st = read_state();
  for t in [st.current, st.english].into_iter().flatten() {
    if let Some(s) = t.texts.get(key) {
      return s.as_str();
    }
  }
  key
}

/// In English, whatever the language selected (files written for the game's code).
pub fn tr_en(key: &str) -> &str {
  if let Some(t) = read_state().english
    && let Some(s) = t.texts.get(key)
  {
    return s.as_str();
  }
  key
}

// ------------------------------------------------------------------------------------------------
// printf
// ------------------------------------------------------------------------------------------------

/// An argument of `strf`, made with `Arg::from` (the `strf!` macro does it).
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
  Str(String),
  Int(i64),
  Uint(u64),
  Float(f64),
}

impl From<&str> for Arg {
  fn from(v: &str) -> Self {
    Arg::Str(v.to_string())
  }
}
impl From<String> for Arg {
  fn from(v: String) -> Self {
    Arg::Str(v)
  }
}
impl From<&String> for Arg {
  fn from(v: &String) -> Self {
    Arg::Str(v.clone())
  }
}
impl From<char> for Arg {
  fn from(v: char) -> Self {
    Arg::Str(v.to_string())
  }
}
impl From<bool> for Arg {
  fn from(v: bool) -> Self {
    Arg::Int(v as i64)
  }
}
macro_rules! arg_from {
  ($variant:ident, $as:ty: $($t:ty),*) => { $(impl From<$t> for Arg { fn from(v: $t) -> Self { Arg::$variant(v as $as) } })* };
}
arg_from!(Int, i64: i8, i16, i32, i64, isize);
arg_from!(Uint, u64: u8, u16, u32, u64, usize);
arg_from!(Float, f64: f32, f64);

/// `strf!(fmt, a, b...)`: `strf(fmt, &[Arg::from(a), Arg::from(b)...])`.
#[macro_export]
macro_rules! strf {
  ($fmt:expr $(,)?) => { $crate::i18n::strf(&$fmt, &[]) };
  ($fmt:expr, $($arg:expr),+ $(,)?) => { $crate::i18n::strf(&$fmt, &[$($crate::i18n::Arg::from($arg)),+]) };
}

/// A conversion of a printf format: `%[-+ #0]*[0-9]*(\.[0-9]+)?(hh|h|ll|l|z|j|t|L)?[diouxXeEfgGcsp%]`.
struct Spec<'a> {
  minus: bool,
  plus: bool,
  space: bool,
  zero: bool,
  alt: bool,
  width: usize,
  precision: Option<usize>,
  length: &'a str,
  conv: u8,
  /// the byte after it
  end: usize,
}

/// The conversion at `at` (a '%'), None when the text there is not one.
fn parse_spec(s: &str, at: usize) -> Option<Spec<'_>> {
  let b = s.as_bytes();
  let mut spec = Spec {
    minus: false,
    plus: false,
    space: false,
    zero: false,
    alt: false,
    width: 0,
    precision: None,
    length: "",
    conv: 0,
    end: 0,
  };
  let mut i = at + 1;
  while i < b.len() {
    match b[i] {
      b'-' => spec.minus = true,
      b'+' => spec.plus = true,
      b' ' => spec.space = true,
      b'#' => spec.alt = true,
      b'0' => spec.zero = true,
      _ => break,
    }
    i += 1;
  }
  let digits = |i: &mut usize| {
    let start = *i;
    while *i < b.len() && b[*i].is_ascii_digit() {
      *i += 1;
    }
    s[start..*i].parse::<usize>().unwrap_or(0)
  };
  spec.width = digits(&mut i);
  if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
    i += 1;
    spec.precision = Some(digits(&mut i));
  }
  for length in ["hh", "ll", "h", "l", "z", "j", "t", "L"] {
    if s[i..].starts_with(length) {
      spec.length = &s[i..i + length.len()];
      i += length.len();
      break;
    }
  }
  if i < b.len() && b"diouxXeEfgGcsp%".contains(&b[i]) {
    spec.conv = b[i];
    spec.end = i + 1;
    return Some(spec);
  }
  None
}

/// A printf-like formatter for the texts of lang/*.json and the log: `%s %d %i %u %zu %x %X %o %c
/// %f %e %g` with flags, width and precision (`%+.2f`, `%5.1f`, `%-8s`), `%%`. An argument of
/// another kind than the conversion asks for is shown as it is (no undefined behavior).
pub fn strf(fmt: &str, args: &[Arg]) -> String {
  let mut out = String::with_capacity(fmt.len() + 16);
  let mut next = args.iter();
  let mut i = 0;
  let mut plain = 0;
  while let Some(at) = fmt[i..].find('%').map(|k| k + i) {
    let Some(spec) = parse_spec(fmt, at) else {
      i = at + 1;
      continue;
    };
    out.push_str(&fmt[plain..at]);
    if spec.conv == b'%' {
      out.push('%');
    } else {
      format_arg(&mut out, &spec, next.next());
    }
    i = spec.end;
    plain = spec.end;
  }
  out.push_str(&fmt[plain..]);
  out
}

fn format_arg(out: &mut String, spec: &Spec, arg: Option<&Arg>) {
  let conv = spec.conv as char;
  let sign_of = |negative: bool| {
    if negative {
      "-"
    } else if spec.plus {
      "+"
    } else if spec.space {
      " "
    } else {
      ""
    }
  };
  match conv {
    'd' | 'i' => match arg {
      Some(Arg::Str(s)) => pad(out, spec, "", s, false),
      _ => {
        let v = match arg {
          Some(Arg::Int(v)) => *v as i128,
          Some(Arg::Uint(v)) => *v as i128,
          Some(Arg::Float(v)) => *v as i128,
          _ => 0,
        };
        let body = int_digits(v.unsigned_abs().to_string(), spec.precision);
        pad(out, spec, sign_of(v < 0), &body, spec.precision.is_none());
      }
    },
    'u' | 'x' | 'X' | 'o' | 'p' => match arg {
      Some(Arg::Str(s)) => pad(out, spec, "", s, false),
      _ => {
        let wide = matches!(spec.length, "l" | "ll" | "z" | "j" | "t") || conv == 'p';
        let v = match arg {
          Some(Arg::Int(v)) if wide => *v as u64,
          Some(Arg::Int(v)) => *v as u32 as u64, // a negative int read as an unsigned int
          Some(Arg::Uint(v)) => *v,
          Some(Arg::Float(v)) => *v as u64,
          _ => 0,
        };
        let digits = match conv {
          'x' => format!("{v:x}"),
          'X' => format!("{v:X}"),
          'o' => format!("{v:o}"),
          'p' => format!("0x{v:x}"),
          _ => v.to_string(),
        };
        let prefix = match conv {
          'x' if spec.alt && v != 0 => "0x",
          'X' if spec.alt && v != 0 => "0X",
          'o' if spec.alt && !digits.starts_with('0') => "0",
          _ => "",
        };
        pad(
          out,
          spec,
          prefix,
          &int_digits(digits, spec.precision),
          spec.precision.is_none(),
        );
      }
    },
    'f' | 'F' | 'e' | 'E' | 'g' | 'G' => {
      let v = match arg {
        Some(Arg::Float(v)) => *v,
        Some(Arg::Int(v)) => *v as f64,
        Some(Arg::Uint(v)) => *v as f64,
        Some(Arg::Str(s)) => return pad(out, spec, "", s, false),
        None => 0.0,
      };
      let upper = conv.is_ascii_uppercase();
      if !v.is_finite() {
        let body = if v.is_nan() { "nan" } else { "inf" };
        let body = if upper {
          body.to_uppercase()
        } else {
          body.to_string()
        };
        return pad(
          out,
          spec,
          sign_of(v.is_sign_negative() && !v.is_nan()),
          &body,
          false,
        );
      }
      let body = float_body(v.abs(), conv.to_ascii_lowercase(), spec.precision, spec.alt);
      let body = if upper { body.to_uppercase() } else { body };
      pad(out, spec, sign_of(v.is_sign_negative()), &body, true);
    }
    'c' => {
      let c = match arg {
        Some(Arg::Str(s)) => s.chars().next().map(String::from).unwrap_or_default(),
        Some(Arg::Int(v)) => char::from_u32(*v as u32)
          .map(String::from)
          .unwrap_or_default(),
        Some(Arg::Uint(v)) => char::from_u32(*v as u32)
          .map(String::from)
          .unwrap_or_default(),
        _ => String::new(),
      };
      pad(out, spec, "", &c, false);
    }
    _ => {
      // 's': the text, cut at the precision
      let text = match arg {
        Some(Arg::Str(s)) => s.clone(),
        Some(Arg::Int(v)) => v.to_string(),
        Some(Arg::Uint(v)) => v.to_string(),
        Some(Arg::Float(v)) => v.to_string(),
        None => String::new(),
      };
      let text = match spec.precision {
        Some(p) => text.chars().take(p).collect(),
        None => text,
      };
      pad(out, spec, "", &text, false);
    }
  }
}

/// An integer's digits, at least `precision` of them.
fn int_digits(digits: String, precision: Option<usize>) -> String {
  match precision {
    Some(p) if digits.len() < p => "0".repeat(p - digits.len()) + &digits,
    Some(0) if digits == "0" => String::new(),
    _ => digits,
  }
}

/// `sign` then `body`, padded to the width: spaces on the left, on the right with `-`, zeros after
/// the sign with `0` when `zero_ok`.
fn pad(out: &mut String, spec: &Spec, sign: &str, body: &str, zero_ok: bool) {
  let len = sign.chars().count() + body.chars().count();
  let fill = spec.width.saturating_sub(len);
  if spec.minus {
    let _ = write!(out, "{sign}{body}{:fill$}", "");
  } else if spec.zero && zero_ok {
    let _ = write!(out, "{sign}{}{body}", "0".repeat(fill));
  } else {
    let _ = write!(out, "{:fill$}{sign}{body}", "");
  }
}

/// A finite, positive value as %f, %e or %g writes it.
fn float_body(v: f64, conv: char, precision: Option<usize>, alt: bool) -> String {
  let exp_form = |p: usize| {
    let s = format!("{v:.p$e}");
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    format!(
      "{mantissa}e{}{:02}",
      if exp < 0 { '-' } else { '+' },
      exp.abs()
    )
  };
  match conv {
    'f' => format!("{v:.*}", precision.unwrap_or(6)),
    'e' => exp_form(precision.unwrap_or(6)),
    _ => {
      // %g: P significant digits, as %e when the exponent is below -4 or at least P, else as %f;
      // the trailing zeros removed (unless '#')
      let p = precision.unwrap_or(6).max(1);
      let e = exp_form(p - 1);
      let x: i32 = e
        .split_once('e')
        .and_then(|(_, x)| x.parse().ok())
        .unwrap_or(0);
      let s = if (x as i64) < p as i64 && x >= -4 {
        format!("{v:.*}", (p as i64 - 1 - x as i64) as usize)
      } else {
        e
      };
      if alt {
        return s;
      }
      let strip = |m: &str| {
        if m.contains('.') {
          m.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
          m.to_string()
        }
      };
      match s.split_once('e') {
        Some((mantissa, exp)) => format!("{}e{exp}", strip(mantissa)),
        None => strip(&s),
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn macos_languages_read() {
    let text = "(\n    \"fr-FR\",\n    en\n)\n";
    assert_eq!(plist_strings(text), ["fr-FR", "en"]);
  }

  #[test]
  fn printf() {
    assert_eq!(strf!("%zu objets, %s", 3usize, "ok"), "3 objets, ok");
    assert_eq!(
      strf!("%.1f m (%.0f ms) %+.2f %+.2f", 2.25f32, 12.6, 1.5, -0.25),
      "2.2 m (13 ms) +1.50 -0.25"
    );
    assert_eq!(strf!("100%% %d%%", -7), "100% -7%");
    assert_eq!(
      strf!("[%5d|%-5s|%05.1f|%03d]", 42, "ab", -2.5, 7),
      "[   42|ab   |-02.5|007]"
    );
    assert_eq!(
      strf!("%g %g %g %g %g", 0.0001, 1e-5, 123456789.0, 100.0, 0.5),
      "0.0001 1e-05 1.23457e+08 100 0.5"
    );
    assert_eq!(
      strf!("%x %X %#x %u %i %e", 255, 255u32, 255, -1, 3, 1234.5),
      "ff FF 0xff 4294967295 3 1.234500e+03"
    );
    assert_eq!(strf!("%f %s %d", 1.0, 2.5, "x"), "1.000000 2.5 x");
    assert_eq!(strf!("%s and %s", "one"), "one and ", "a missing argument");
    assert_eq!(
      strf!("50%, %q", 1),
      "50%, %q",
      "not conversions: kept as they are"
    );
    assert_eq!(
      strf!("% o", 8),
      "10",
      "a space flag (signed conversions only) and an octal, as printf reads it"
    );
    assert_eq!(strf!("é %s ü", "à"), "é à ü");
  }

  // the printf arguments of a text: "%zu objects, %s" -> "zus"
  fn printf_args(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while let Some(at) = s[i..].find('%').map(|k| k + i) {
      match parse_spec(s, at) {
        Some(spec) => {
          if spec.conv != b'%' {
            out += spec.length;
            out.push(spec.conv as char);
          }
          i = spec.end;
        }
        None => i = at + 1,
      }
    }
    out
  }

  // the selftest's translations block: every language has every key of English, with the same
  // printf arguments
  #[test]
  fn translations() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lang");
    let read =
      |p: &Path| serde_json::from_slice::<serde_json::Value>(&std::fs::read(p).unwrap()).unwrap();
    let en = read(&dir.join("en.json"));
    let en = en.as_object().unwrap();
    assert!(en.len() > 50, "translations: lang/en.json");
    let mut languages = 0;
    let mut problems = String::new();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
      let path = entry.path();
      let code = path.file_stem().unwrap().to_string_lossy().into_owned();
      if path.extension().is_none_or(|e| e != "json") || code == "en" {
        continue;
      }
      languages += 1;
      let other = read(&path);
      let other = other.as_object().unwrap();
      for (k, v) in en {
        match other.get(k) {
          None => problems += &format!("{code}: no {k}\n"),
          Some(o)
            if printf_args(v.as_str().unwrap_or("")) != printf_args(o.as_str().unwrap_or("")) =>
          {
            problems += &format!("{code}: other arguments in {k}\n")
          }
          _ => {}
        }
      }
      for k in other.keys() {
        if !en.contains_key(k) {
          problems += &format!("{code}: unknown key {k}\n");
        }
      }
    }
    assert!(
      languages >= 1 && problems.is_empty(),
      "translations: same keys and arguments as English\n{problems}"
    );
    assert_eq!(printf_args("%zu objects, %s, %+.2f %%"), "zusf");

    i18n_init(&dir, "fr");
    let french = tr("menu.file") == "Fichier";
    let names: Vec<String> = i18n_languages().into_iter().map(|l| l.code).collect();
    i18n_select("en");
    assert!(
      french && tr("menu.file") == "File" && tr("no.such.key") == "no.such.key",
      "translations: switching language, a missing key shows itself"
    );
    assert_eq!(
      names.first().map(String::as_str),
      Some("en"),
      "English first"
    );
    assert_eq!(i18n_language(), "en");
    assert_eq!(tr_en("menu.file"), "File");
  }
}
