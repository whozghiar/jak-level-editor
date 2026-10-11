#pragma once

// Transform gizmo, written from scratch: move (axes, planes, view plane), rotate (rings) and scale
// (along an axis of the selection, or all of them from the center). Math in 3D, drawing in 2D
// with the ImGui draw list over the viewport.
//
// While dragging: Shift moves ten times finer, Ctrl turns the snap on (or off when it is on). In
// world space the snap puts the gizmo's pivot on the grid (absolute), in local space it moves by
// steps (relative). An axis seen end-on and a plane seen edge-on cannot be grabbed (they would
// jump), and a ring seen edge-on turns with the mouse along it instead of around its center. The
// value of the drag is written next to the cursor.

#include <string>

#include "render/camera.h"
#include "render/renderer.h"

struct ImDrawList;

namespace ogle {

enum class GizmoOp { Translate = 0, Rotate, Scale };
enum class GizmoSpace { World = 0, Local };

struct GizmoSnap {
  bool enabled = false;
  float translate = 0.5f;    // meters
  float rotate_deg = 15.f;
  float scale = 0.1f;        // steps of the scale factor
};

struct GizmoInput {
  Vec2 mouse;          // viewport pixels, origin top-left
  Vec2 viewport_pos;   // screen position of the viewport (for drawing)
  Vec2 viewport_size;
  bool mouse_in_viewport = false;
  bool pressed = false;   // left button went down this frame
  bool down = false;
  bool released = false;
  bool fine = false;         // Shift: ten times finer
  bool snap_toggle = false;  // Ctrl: the snap the other way
};

struct GizmoResult {
  bool started = false;   // drag began this frame
  bool dragging = false;  // drag in progress (delta is valid)
  bool finished = false;  // drag ended this frame
  // world delta: new_world = delta * start_world (move, rotate)
  Mat4 delta;
  // scale: the factors along the axes of the gizmo's frame (the selection's own axes), to multiply
  // the scale the elements had when the drag began
  Vec3 scale{1, 1, 1};
};

class Gizmo {
 public:
  GizmoOp op = GizmoOp::Translate;
  GizmoSpace space = GizmoSpace::World;
  GizmoSnap snap;

  // `frame`: pivot position (column 3) and orientation (columns 0-2, used in Local space)
  GizmoResult update(const Camera& cam, const Renderer& renderer, const GizmoInput& in,
                     const Mat4& frame, ImDrawList* dl);
  bool hovering() const { return m_hot >= 0; }
  bool active() const { return m_active >= 0; }
  // what the drag going on did, in words ("X +1.50 m", "Y +45.0 deg"...), "" when none
  const std::string& readout() const { return m_readout; }

 private:
  // handle ids: 0-2 axes (rings when rotating), 3-5 planes (yz, xz, xy), 6 view plane (all axes
  // when scaling)
  int hit_test(const Renderer& r, const GizmoInput& in, const Mat4& f, float size, const Vec3& view) const;
  void draw(const Renderer& r, const GizmoInput& in, const Mat4& f, float size, const Vec3& view, ImDrawList* dl) const;
  bool ray_on_drag_plane(const Ray& ray, Vec3* p) const;

  int m_hot = -1;
  int m_active = -1;
  Mat4 m_start_frame;
  Vec3 m_plane_point, m_plane_normal;
  Vec3 m_start_hit, m_last_hit;
  Vec2 m_start_mouse, m_last_mouse;
  bool m_edge_on = false;  // a ring seen edge-on: turned along its tangent on screen
  Vec2 m_tangent;          // that tangent (unit, screen), and the ring's radius on screen
  float m_radius_px = 1.f;
  // what the mouse did since the drag began (raw), and what it does (Shift: a tenth of it)
  Vec3 m_raw_move{0, 0, 0}, m_move{0, 0, 0};
  float m_raw_angle = 0, m_angle = 0, m_raw_scale = 1, m_scale = 1;
  std::string m_readout;
};

// The value of a drag snapped: `v` to the nearest multiple of `step` (no snap when step <= 0).
float snap_value(float v, float step);

}  // namespace ogle
