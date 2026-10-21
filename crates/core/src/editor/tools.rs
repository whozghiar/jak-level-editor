//! Placeholders of the tools other areas own: the Place tool (placement.rs), the topology and the
//! terrain brush. Each area replaces its functions here with its own file (and its calls in the
//! "not yet" block of api.rs).

use super::{Editor, ToolMode};

impl Editor {
  /// back to selecting (actions.cpp stop_placement)
  pub(crate) fn stop_placement(&mut self) {
    if self.tool == ToolMode::Place {
      self.tool = ToolMode::Select;
    }
  }

  pub(crate) fn stop_topology(&mut self) {
    if self.tool == ToolMode::Topology {
      self.tool = ToolMode::Select;
    }
  }

  pub(crate) fn stop_terrain_brush(&mut self) {
    if self.tool == ToolMode::Terrain {
      self.tool = ToolMode::Select;
    }
  }

  /// a terrain stroke shown, not let go: nothing done (false: none going on)
  pub(crate) fn terrain_cancel_stroke(&mut self) -> bool {
    false
  }

  /// whether the topology tool can edit the element's mesh
  pub(crate) fn topology_editable(&self, _id: u32) -> bool {
    false
  }

  /// the terrain brush as get_state shows it
  pub(crate) fn brush_state(&self) -> serde_json::Value {
    serde_json::json!({"magnet": false, "size": 4.0})
  }

  /// Escape: stop placing, else stop the topology, else cancel a terrain stroke shown, else stop
  /// the brush, else select nothing (handle_shortcuts)
  pub fn escape(&mut self) {
    match self.tool {
      ToolMode::Place => self.stop_placement(),
      ToolMode::Topology => self.stop_topology(),
      ToolMode::Terrain => {
        if !self.terrain_cancel_stroke() {
          self.stop_terrain_brush();
        }
      }
      ToolMode::Select => self.select_only(0),
    }
  }
}
