//! The application without its interface (src/app/app.h): the document open, its history, the
//! selection, the tools, the jobs. The interface reaches it through `Editor::call` (api.rs).

mod api;

use serde_json::Value;

/// What the editor tells its interface (the plugin turns it into Tauri events): `jle:changed`,
/// `jle:log`, `jle:progress`, `jle:request`, with their payload.
pub type Notify = Box<dyn Fn(&str, Value) + Send + Sync>;

pub struct Editor {
  notify: Option<Notify>,
}

impl Default for Editor {
  fn default() -> Self {
    Self::new()
  }
}

impl Editor {
  pub fn new() -> Self {
    Self { notify: None }
  }

  pub fn set_notify(&mut self, notify: Notify) {
    self.notify = Some(notify);
  }

  pub(crate) fn emit(&self, event: &str, payload: Value) {
    if let Some(n) = &self.notify {
      n(event, payload);
    }
  }
}
