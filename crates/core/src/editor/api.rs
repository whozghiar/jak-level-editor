//! The engine's API: every call of the interface (and of the plugin's protocol) by name.

use serde_json::{Value, json};

use super::Editor;

impl Editor {
  pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
    let _ = args;
    match name {
      "ping" => {
        Ok(json!({ "engine": "jak-level-editor-core", "version": env!("CARGO_PKG_VERSION") }))
      }
      _ => Err(format!("unknown call: {name}")),
    }
  }
}
