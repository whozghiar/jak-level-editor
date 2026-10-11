#include "tools/gizmo.h"

#include <cmath>
#include <cstdio>

#include "imgui.h"

namespace ogle {

namespace {

float dist_point_segment(const Vec2& p, const Vec2& a, const Vec2& b) {
  Vec2 ab = b - a, ap = p - a;
  float len2 = ab.x * ab.x + ab.y * ab.y;
  float t = len2 > 1e-6f ? std::clamp((ap.x * ab.x + ap.y * ab.y) / len2, 0.f, 1.f) : 0.f;
  Vec2 c = a + ab * t;
  return (p - c).length();
}

bool point_in_quad(const Vec2& p, const Vec2 q[4]) {
  // convex quad, either winding
  int sign = 0;
  for (int i = 0; i < 4; i++) {
    Vec2 a = q[i], b = q[(i + 1) % 4];
    float c = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    int s = c > 0 ? 1 : (c < 0 ? -1 : 0);
    if (s == 0) continue;
    if (sign == 0) sign = s;
    else if (s != sign) return false;
  }
  return true;
}

ImU32 axis_color(int i, bool hot) {
  if (hot) return IM_COL32(255, 220, 70, 255);
  switch (i) {
    case 0: return IM_COL32(232, 72, 72, 255);
    case 1: return IM_COL32(96, 206, 90, 255);
    case 2: return IM_COL32(80, 132, 245, 255);
    default: return IM_COL32(230, 230, 230, 255);
  }
}

Vec3 perpendicular(const Vec3& v) {
  Vec3 o = std::fabs(v.y) < 0.9f ? Vec3{0, 1, 0} : Vec3{1, 0, 0};
  return normalize(cross(v, o));
}

// an axis seen end-on, a plane seen edge-on: a drag along them would jump
bool axis_usable(const Vec3& axis, const Vec3& view) { return std::fabs(dot(axis, view)) < 0.985f; }
bool plane_usable(const Vec3& normal, const Vec3& view) { return std::fabs(dot(normal, view)) > 0.12f; }

float wrap_angle(float a) {
  while (a > kPi) a -= 2 * kPi;
  while (a < -kPi) a += 2 * kPi;
  return a;
}

constexpr float kAxisPick = 10.f;  // pixels from a handle that grab it
constexpr float kCenterPick = 9.f;

}  // namespace

float snap_value(float v, float step) { return step > 0 ? std::round(v / step) * step : v; }

bool Gizmo::ray_on_drag_plane(const Ray& ray, Vec3* p) const {
  float t;
  if (!ray_plane(ray, m_plane_point, m_plane_normal, &t) || t < 0) return false;
  *p = ray.at(t);
  return true;
}

int Gizmo::hit_test(const Renderer& r, const GizmoInput& in, const Mat4& f, float size, const Vec3& view) const {
  Vec3 pivot = f.col3(3);
  Vec2 o;
  if (!r.project(pivot, &o)) return -1;
  const Vec2 m = in.mouse;
  if ((m - o).length() < kCenterPick && op != GizmoOp::Rotate) return 6;
  if (op == GizmoOp::Rotate) {
    int best = -1;
    float best_d = kAxisPick;
    for (int i = 0; i < 3; i++) {
      Vec3 a = f.col3(i);
      Vec3 u = perpendicular(a), v = cross(a, u);
      Vec2 prev;
      bool has_prev = false;
      for (int k = 0; k <= 64; k++) {
        float ang = (float)k / 64 * 2.f * kPi;
        Vec2 s;
        bool ok = r.project(pivot + (u * std::cos(ang) + v * std::sin(ang)) * size, &s);
        if (ok && has_prev) {
          float d = dist_point_segment(m, prev, s);
          if (d < best_d) {
            best_d = d;
            best = i;
          }
        }
        prev = s;
        has_prev = ok;
      }
    }
    return best;
  }
  if (op == GizmoOp::Translate) {
    for (int k = 0; k < 3; k++) {
      if (!plane_usable(f.col3(k), view)) continue;
      int i = (k + 1) % 3, j = (k + 2) % 3;
      Vec3 ai = f.col3(i), aj = f.col3(j);
      Vec2 q[4];
      bool ok = r.project(pivot + (ai * 0.18f + aj * 0.18f) * size, &q[0]) &&
                r.project(pivot + (ai * 0.42f + aj * 0.18f) * size, &q[1]) &&
                r.project(pivot + (ai * 0.42f + aj * 0.42f) * size, &q[2]) &&
                r.project(pivot + (ai * 0.18f + aj * 0.42f) * size, &q[3]);
      if (ok && point_in_quad(m, q)) return 3 + k;
    }
  }
  int best = -1;
  float best_d = kAxisPick;
  for (int i = 0; i < 3; i++) {
    if (!axis_usable(f.col3(i), view)) continue;
    Vec2 e;
    if (!r.project(pivot + f.col3(i) * size, &e)) continue;
    float d = dist_point_segment(m, o, e);
    if (d < best_d) {
      best_d = d;
      best = i;
    }
  }
  return best;
}

void Gizmo::draw(const Renderer& r, const GizmoInput& in, const Mat4& f, float size, const Vec3& view,
                 ImDrawList* dl) const {
  if (!dl) return;
  Vec3 pivot = f.col3(3);
  Vec2 o;
  if (!r.project(pivot, &o)) return;
  auto S = [&](const Vec2& v) { return ImVec2(in.viewport_pos.x + v.x, in.viewport_pos.y + v.y); };
  int hot = m_active >= 0 ? m_active : m_hot;
  // the axis being dragged along: a line through the pivot, to see where it goes
  if (m_active >= 0 && m_active < 3 && op != GizmoOp::Rotate) {
    Vec2 a, b;
    const Vec3 dir = m_start_frame.col3(m_active);
    if (r.project(pivot - dir * size * 40.f, &a) && r.project(pivot + dir * size * 40.f, &b))
      dl->AddLine(S(a), S(b), (axis_color(m_active, false) & 0x00ffffff) | 0x90000000, 1.5f);
  }
  if (op == GizmoOp::Scale) {
    // an axis with a square at its end for each direction, a square in the center for all
    for (int i = 0; i < 3; i++) {
      if (!axis_usable(f.col3(i), view) && hot != i) continue;
      Vec2 e;
      if (!r.project(pivot + f.col3(i) * size, &e)) continue;
      ImU32 c = axis_color(i, hot == i);
      dl->AddLine(S(o), S(e), c, hot == i ? 4.f : 2.8f);
      dl->AddRectFilled(ImVec2(S(e).x - 5, S(e).y - 5), ImVec2(S(e).x + 5, S(e).y + 5), c);
    }
    ImU32 cc = hot == 6 ? IM_COL32(255, 220, 70, 255) : IM_COL32(235, 235, 235, 230);
    dl->AddRectFilled(ImVec2(S(o).x - 6, S(o).y - 6), ImVec2(S(o).x + 6, S(o).y + 6), cc);
    return;
  }
  if (op == GizmoOp::Rotate) {
    for (int i = 0; i < 3; i++) {
      Vec3 a = f.col3(i);
      Vec3 u = perpendicular(a), v = cross(a, u);
      std::vector<ImVec2> pts;
      for (int k = 0; k <= 64; k++) {
        float ang = (float)k / 64 * 2.f * kPi;
        Vec2 s;
        if (r.project(pivot + (u * std::cos(ang) + v * std::sin(ang)) * size, &s)) pts.push_back(S(s));
      }
      if (pts.size() > 1) dl->AddPolyline(pts.data(), (int)pts.size(), axis_color(i, hot == i), 0, hot == i ? 3.5f : 2.2f);
    }
    // the turn so far: from where the ring was grabbed to where it is now
    if (m_active >= 0 && m_active < 3 && std::fabs(m_angle) > 1e-4f) {
      const Vec3 a = m_start_frame.col3(m_active);
      Vec3 from = m_start_hit - m_start_frame.col3(3);
      from = from - a * dot(from, a);
      if (length2(from) > 1e-8f) {
        from = normalize(from);
        const Vec3 side = cross(a, from);
        std::vector<ImVec2> fan{S(o)};
        const int steps = std::max(2, (int)(std::fabs(m_angle) / kPi * 32));
        for (int k = 0; k <= steps; k++) {
          const float t = m_angle * k / steps;
          Vec2 s;
          if (r.project(pivot + (from * std::cos(t) + side * std::sin(t)) * size, &s)) fan.push_back(S(s));
        }
        if (fan.size() > 2) dl->AddConvexPolyFilled(fan.data(), (int)fan.size(), IM_COL32(255, 220, 70, 50));
      }
    }
    dl->AddCircleFilled(S(o), 3.f, IM_COL32(240, 240, 240, 220));
    return;
  }
  // plane handles
  for (int k = 0; k < 3; k++) {
    if (!plane_usable(f.col3(k), view) && hot != 3 + k) continue;
    int i = (k + 1) % 3, j = (k + 2) % 3;
    Vec3 ai = f.col3(i), aj = f.col3(j);
    Vec2 q[4];
    if (r.project(pivot + (ai * 0.18f + aj * 0.18f) * size, &q[0]) &&
        r.project(pivot + (ai * 0.42f + aj * 0.18f) * size, &q[1]) &&
        r.project(pivot + (ai * 0.42f + aj * 0.42f) * size, &q[2]) &&
        r.project(pivot + (ai * 0.18f + aj * 0.42f) * size, &q[3])) {
      ImU32 c = hot == 3 + k ? IM_COL32(255, 220, 70, 150) : (axis_color(k, false) & 0x00ffffff) | 0x70000000;
      dl->AddQuadFilled(S(q[0]), S(q[1]), S(q[2]), S(q[3]), c);
    }
  }
  for (int i = 0; i < 3; i++) {
    if (!axis_usable(f.col3(i), view) && hot != i) continue;
    Vec2 e;
    Vec3 a = f.col3(i);
    if (!r.project(pivot + a * size, &e)) continue;
    ImU32 c = axis_color(i, hot == i);
    dl->AddLine(S(o), S(e), c, hot == i ? 4.f : 2.8f);
    Vec2 d = e - o;
    float len = d.length();
    if (len > 1) {
      Vec2 dir = d * (1.f / len), nrm{-dir.y, dir.x};
      Vec2 base = e - dir * 13.f;
      dl->AddTriangleFilled(S(e + dir * 3.f), S(base + nrm * 6.f), S(base - nrm * 6.f), c);
    }
  }
  ImU32 cc = hot == 6 ? IM_COL32(255, 220, 70, 255) : IM_COL32(235, 235, 235, 230);
  dl->AddRect(ImVec2(S(o).x - 6, S(o).y - 6), ImVec2(S(o).x + 6, S(o).y + 6), cc, 0.f, 0, 2.f);
}

GizmoResult Gizmo::update(const Camera& cam, const Renderer& r, const GizmoInput& in,
                          const Mat4& frame, ImDrawList* dl) {
  GizmoResult res;
  res.delta = Mat4::identity();
  // orthonormal gizmo frame
  // scaling is along the selection's own axes
  const bool local = space == GizmoSpace::Local || op == GizmoOp::Scale;
  Mat4 f = Mat4::identity();
  if (local) {
    Vec3 x = normalize(frame.col3(0)), y = frame.col3(1), z;
    if (length2(x) == 0) x = {1, 0, 0};
    z = normalize(cross(x, y));
    if (length2(z) == 0) z = perpendicular(x);
    y = cross(z, x);
    f.set_col3(0, x);
    f.set_col3(1, y);
    f.set_col3(2, z);
  }
  f.set_col3(3, frame.col3(3));
  const Vec3 pivot = f.col3(3);
  float dist = std::max(length(pivot - cam.pos), 0.5f);
  float size = dist * std::tan(radians(cam.fov_deg) * 0.5f) * 0.24f;
  const Vec3 view = normalize(pivot - cam.pos);

  if (m_active < 0) {
    m_readout.clear();
    m_hot = in.mouse_in_viewport ? hit_test(r, in, f, size, view) : -1;
    if (in.pressed && m_hot >= 0) {
      m_active = m_hot;
      m_start_frame = f;
      m_start_mouse = m_last_mouse = in.mouse;
      m_plane_point = pivot;
      m_raw_move = m_move = {0, 0, 0};
      m_raw_angle = m_angle = 0;
      m_raw_scale = m_scale = 1;
      m_edge_on = false;
      if (op == GizmoOp::Rotate) {
        m_plane_normal = f.col3(m_active);
      } else if (op == GizmoOp::Scale && m_active == 6) {
        m_plane_normal = -cam.forward();
      } else if (m_active < 3) {
        Vec3 a = f.col3(m_active);
        Vec3 n = cross(a, cross(view, a));
        m_plane_normal = length2(n) > 1e-8f ? normalize(n) : perpendicular(a);
      } else if (m_active < 6) {
        m_plane_normal = f.col3(m_active - 3);
      } else {
        m_plane_normal = -cam.forward();
      }
      Ray ray = cam.ray(in.mouse.x, in.mouse.y, in.viewport_size.x, in.viewport_size.y);
      if (op == GizmoOp::Rotate && std::fabs(dot(m_plane_normal, view)) < 0.25f) {
        // a ring seen edge-on: the mouse runs along it on screen, where it was grabbed
        const Vec3 a = m_plane_normal;
        Vec3 radial = cam.pos - pivot;
        radial = radial - a * dot(radial, a);
        radial = length2(radial) > 1e-8f ? normalize(radial) : perpendicular(a);
        Vec2 p0, p1, o;
        const Vec3 at = pivot + radial * size;
        if (r.project(at, &p0) && r.project(at + cross(a, radial) * size * 0.1f, &p1) && r.project(pivot, &o)) {
          const Vec2 t = p1 - p0;
          if (t.length() > 1e-3f) {
            m_edge_on = true;
            m_tangent = t * (1.f / t.length());
            m_radius_px = std::max((p0 - o).length(), 20.f);
            m_start_hit = m_last_hit = at;
            res.started = true;
          }
        }
        if (!m_edge_on) m_active = -1;
      } else if (!ray_on_drag_plane(ray, &m_start_hit)) {
        m_active = -1;
      } else {
        m_last_hit = m_start_hit;
        res.started = true;
      }
    }
  }

  if (m_active >= 0) {
    const bool snapping = snap.enabled != in.snap_toggle;
    const float k_fine = in.fine ? 0.1f : 1.f;
    if (in.down) {
      Ray ray = cam.ray(in.mouse.x, in.mouse.y, in.viewport_size.x, in.viewport_size.y);
      Vec3 p;
      const Mat4& sf = m_start_frame;
      const Vec3 sp = sf.col3(3);
      res.dragging = true;
      const bool on_plane = !m_edge_on && ray_on_drag_plane(ray, &p);
      char text[96] = "";
      const char* names[] = {"X", "Y", "Z"};
      if (op == GizmoOp::Scale && on_plane) {
        // along an axis: how far the handle went, in sizes of the gizmo; all axes: the mouse
        // going up (or right) grows, down shrinks
        const Vec3 d = p - m_start_hit;
        float raw = 1.f;
        if (m_active < 3) {
          raw = 1.f + dot(d, sf.col3(m_active)) / size;
        } else {
          const Vec2 md = in.mouse - m_start_mouse;
          raw = std::exp((md.x - md.y) * 0.01f);
        }
        m_scale += (raw - m_raw_scale) * k_fine;
        m_raw_scale = raw;
        float k = snapping ? snap_value(m_scale, snap.scale) : m_scale;
        k = std::max(k, 0.01f);
        res.scale = m_active < 3 ? Vec3{m_active == 0 ? k : 1.f, m_active == 1 ? k : 1.f, m_active == 2 ? k : 1.f}
                                 : Vec3{k, k, k};
        std::snprintf(text, sizeof(text), "%s x%.2f", m_active < 3 ? names[m_active] : "XYZ", k);
      } else if (op == GizmoOp::Translate && on_plane) {
        const Vec3 d = p - m_start_hit;
        Vec3 raw{0, 0, 0};
        if (m_active < 3) {
          const Vec3 a = sf.col3(m_active);
          raw = a * dot(d, a);
        } else if (m_active < 6) {
          for (int i = 0; i < 3; i++)
            if (i != m_active - 3) raw += sf.col3(i) * dot(d, sf.col3(i));
        } else {
          raw = d;
        }
        m_move += (raw - m_raw_move) * k_fine;
        m_raw_move = raw;
        Vec3 offset = m_move;
        if (snapping && snap.translate > 0) {
          if (space == GizmoSpace::World) {
            // the pivot on the grid, along the axes dragged
            Vec3 to = sp + offset;
            for (int i = 0; i < 3; i++) {
              const bool moved = m_active == 6 || (m_active < 3 ? i == m_active : i != m_active - 3);
              if (moved) to[i] = snap_value(to[i], snap.translate);
              else to[i] = sp[i];
            }
            offset = to - sp;
          } else {
            Vec3 snapped{0, 0, 0};
            for (int i = 0; i < 3; i++) snapped += sf.col3(i) * snap_value(dot(offset, sf.col3(i)), snap.translate);
            offset = snapped;
          }
        }
        res.delta = Mat4::translate(offset);
        if (m_active < 3)
          std::snprintf(text, sizeof(text), "%s %+.2f m", names[m_active], dot(offset, sf.col3(m_active)));
        else
          std::snprintf(text, sizeof(text), "%+.2f  %+.2f  %+.2f m", offset.x, offset.y, offset.z);
      } else if (op == GizmoOp::Rotate) {
        const Vec3 a = sf.col3(m_active);
        float step = 0.f;
        if (m_edge_on) {
          // along the ring's tangent on screen: its radius in pixels makes a radian
          const Vec2 md = in.mouse - m_last_mouse;
          step = (md.x * m_tangent.x + md.y * m_tangent.y) / m_radius_px;
        } else if (on_plane) {
          // the angle swept since the last frame, so that going round more than half a turn goes on
          const Vec3 v0 = m_last_hit - sp, v1 = p - sp;
          if (length2(v0) > 1e-10f && length2(v1) > 1e-10f)
            step = wrap_angle(std::atan2(dot(cross(v0, v1), a), dot(v0, v1)));
          m_last_hit = p;
        }
        m_raw_angle += step;
        m_angle += step * k_fine;
        float ang = m_angle;
        if (snapping) ang = radians(snap_value(degrees(ang), snap.rotate_deg));
        res.delta = Mat4::translate(sp) * Mat4::rotate(Quat::axis_angle(a, ang)) * Mat4::translate(-sp);
        std::snprintf(text, sizeof(text), "%s %+.1f deg", names[m_active], degrees(ang));
      }
      m_last_mouse = in.mouse;
      if (text[0]) {
        m_readout = text;
        if (snapping) m_readout += "  [snap]";
        if (in.fine) m_readout += "  [fine]";
      }
    }
    if (!in.down) {
      res.finished = true;
      res.dragging = false;
      m_active = -1;
    }
  }
  draw(r, in, f, size, view, dl);
  // the value next to the cursor
  if (dl && m_active >= 0 && !m_readout.empty()) {
    const ImVec2 at(in.viewport_pos.x + in.mouse.x + 18, in.viewport_pos.y + in.mouse.y + 14);
    const ImVec2 sz = ImGui::CalcTextSize(m_readout.c_str());
    dl->AddRectFilled(ImVec2(at.x - 4, at.y - 2), ImVec2(at.x + sz.x + 4, at.y + sz.y + 2), IM_COL32(12, 14, 16, 210), 3.f);
    dl->AddText(at, IM_COL32(255, 225, 120, 255), m_readout.c_str());
  }
  return res;
}

}  // namespace ogle
