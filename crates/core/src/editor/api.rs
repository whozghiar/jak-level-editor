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
      // File > Extract game assets (extract.rs); the editor gives the preferences' extract_source
      // and its library (root, levels) when it has them
      "extract_defaults" => Ok(self.extraction.defaults("", "", 0)),
      "extract_detect" => {
        self.extraction.detect(args["source"].as_str().unwrap_or_default());
        Ok(self.extraction.snapshot())
      }
      "extract_start" => {
        let arg = |k: &str| args[k].as_str().unwrap_or_default();
        self.extraction.start(arg("source"), arg("output"));
        Ok(self.extraction.snapshot())
      }
      "extract_stop" => {
        self.extraction.cancel();
        Ok(self.extraction.snapshot())
      }
      "extract_state" => Ok(self.extraction.snapshot()),
      _ => Err(format!("unknown call: {name}")),
    }
  }
}
