//! What the tools share: Escape (the Place tool: placement.rs, the topology: topology_tool.rs, the
//! terrain brush: terrain_tool.rs).

use super::{Editor, ToolMode};

impl Editor {
  /// Escape: stop placing, else stop the topology, else cancel a terrain stroke shown, else stop
  /// the brush, else select nothing (handle_shortcuts)
  pub fn escape(&mut self) {
    match self.tool {
      ToolMode::Place => self.stop_placement(),
      ToolMode::Topology => self.stop_topology(),
      ToolMode::Collision => self.stop_collision_tool(),
      ToolMode::Terrain => {
        if !self.terrain_cancel_stroke() {
          self.stop_terrain_brush();
        }
      }
      ToolMode::Select => self.select_only(0),
    }
  }
}
