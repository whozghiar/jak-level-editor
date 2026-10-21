//! Minimal logger: prints to stdout and keeps the last lines for the in-editor log window.
//!
//! `log_info!`, `log_warn!` and `log_error!` take a printf format and its arguments, like the
//! C++ `LOG_INFO` (`log_info!(tr("log.saved"), path)`): see `i18n::strf`. A hook set by the editor
//! gets every line (the plugin forwards them to the interface as `jle:log` events).

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, RwLock};

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
  Info,
  Warn,
  Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct LogLine {
  pub level: LogLevel,
  pub text: String,
}

/// Called with every line added, outside the log's lock.
pub type LogHook = Box<dyn Fn(&LogLine) + Send + Sync>;

/// The MCP server keeps stdout for its protocol.
pub static TO_STDERR: AtomicBool = AtomicBool::new(false);

const K_MAX_LINES: usize = 2000;

#[derive(Default)]
pub struct Log {
  lines: Mutex<VecDeque<LogLine>>,
  hook: RwLock<Option<LogHook>>,
}

impl Log {
  pub fn get() -> &'static Log {
    static LOG: LazyLock<Log> = LazyLock::new(Log::default);
    &LOG
  }

  pub fn add(&self, level: LogLevel, text: impl Into<String>) {
    let line = LogLine {
      level,
      text: text.into(),
    };
    let prefix = match level {
      LogLevel::Error => "[error] ",
      LogLevel::Warn => "[warn] ",
      LogLevel::Info => "",
    };
    if TO_STDERR.load(Ordering::Relaxed) {
      let _ = writeln!(std::io::stderr(), "{prefix}{}", line.text);
    } else {
      let mut out = std::io::stdout().lock();
      let _ = writeln!(out, "{prefix}{}", line.text);
      let _ = out.flush();
    }
    {
      let mut lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
      lines.push_back(line.clone());
      while lines.len() > K_MAX_LINES {
        lines.pop_front();
      }
    }
    if let Some(hook) = self.hook.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
      hook(&line);
    }
  }

  pub fn lines(&self) -> Vec<LogLine> {
    self
      .lines
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .iter()
      .cloned()
      .collect()
  }

  /// the last message, shown in the status bar
  pub fn last(&self) -> String {
    self
      .lines
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .back()
      .map(|l| l.text.clone())
      .unwrap_or_default()
  }

  pub fn set_hook(&self, hook: Option<LogHook>) {
    *self.hook.write().unwrap_or_else(|e| e.into_inner()) = hook;
  }
}

#[macro_export]
macro_rules! log_info {
  ($($arg:tt)*) => { $crate::log::Log::get().add($crate::log::LogLevel::Info, $crate::strf!($($arg)*)) };
}

#[macro_export]
macro_rules! log_warn {
  ($($arg:tt)*) => { $crate::log::Log::get().add($crate::log::LogLevel::Warn, $crate::strf!($($arg)*)) };
}

#[macro_export]
macro_rules! log_error {
  ($($arg:tt)*) => { $crate::log::Log::get().add($crate::log::LogLevel::Error, $crate::strf!($($arg)*)) };
}
