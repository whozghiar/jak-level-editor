//! The particle effects of the document for the view (App::update_effects): the interface runs
//! them (ui/render/particles.ts), the engine says which actors run one and where.

use serde_json::Value;

use super::Editor;
use crate::scene::particles::{effect_emitters, particle_data};

impl Editor {
  /// `effect_emitters` -> [{id, group, world: 16 floats, column-major}]
  pub fn effect_emitters_call(&self) -> Value {
    serde_json::to_value(effect_emitters(&self.scene, particle_data())).unwrap_or_default()
  }
}
