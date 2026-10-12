// The topology tool: the points or the faces of one element's mesh, picked with the mouse (a click,
// Shift to add, a box), moved, turned or scaled with the gizmo, deleted, pulled out (extruded),
// cut in four, turned over (scene/topology.h). Every change is undone with Ctrl+Z.

#include <algorithm>
#include <cmath>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "imgui.h"
#include "scene/topology.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::BeginItemTooltip()) {
    ImGui::PushTextWrapPos(ImGui::GetFontSize() * 28.f);
    ImGui::TextUnformatted(text);
    ImGui::PopTextWrapPos();
    ImGui::EndTooltip();
  }
}

bool ray_triangle(const Ray& r, const Vec3& a, const Vec3& b, const Vec3& c, float* t) {
  const Vec3 e1 = b - a, e2 = c - a, p = cross(r.d, e2);
  const float det = dot(e1, p);
  if (std::fabs(det) < 1e-9f) return false;
  const float inv = 1.f / det;
  const Vec3 s = r.o - a;
  const float u = dot(s, p) * inv;
  if (u < 0 || u > 1) return false;
  const Vec3 q = cross(s, e1);
  const float v = dot(r.d, q) * inv;
  if (v < 0 || u + v > 1) return false;
  *t = dot(e2, q) * inv;
  return *t > 0;
}

}  // namespace

bool App::topology_editable(uint32_t id) const {
  const Node* n = scene.find(id);
  return n && n->kind == NodeKind::Render && n->mesh && !scene.actor_of(*n) && !n->mesh->prims.empty();
}

bool App::start_topology() {
  if (!topology_editable(primary)) return false;
  stop_placement();
  stop_preview();
  m_topo_node = primary;
  m_topo_points.clear();
  m_topo_faces_sel.clear();
  m_topo_revision = history.revision();
  tool = ToolMode::Topology;
  return true;
}

void App::stop_topology() {
  if (tool == ToolMode::Topology) tool = ToolMode::Select;
  m_topo_node = 0;
  m_topo_points.clear();
  m_topo_faces_sel.clear();
  m_topo_before.reset();
}

const MeshPoints& App::topology_points() {
  const Node* n = scene.find(m_topo_node);
  if (n && n->mesh && (n->mesh.get() != m_topo_cache_mesh || n->mesh->version != m_topo_cache_version)) {
    m_topo_cache = mesh_points(*n->mesh);
    m_topo_cache_mesh = n->mesh.get();
    m_topo_cache_version = n->mesh->version;
  }
  return m_topo_cache;
}

std::set<MeshRef> App::topology_vertices() {
  const Node* n = scene.find(m_topo_node);
  if (!n || !n->mesh) return {};
  if (m_topo_faces) return face_vertices(*n->mesh, m_topo_faces_sel);
  std::set<MeshRef> out;
  const MeshPoints& pts = topology_points();
  for (uint32_t p : m_topo_points)
    if (p < pts.verts.size()) out.insert(pts.verts[p].begin(), pts.verts[p].end());
  return out;
}

void App::topology_edit(const char* undo_key, const std::function<void(Mesh&)>& edit) {
  Node* n = scene.find(m_topo_node);
  if (!n || !n->mesh) return;
  const Node before = *n;
  n->mesh = std::make_shared<Mesh>(*n->mesh);  // its own, the one before kept for undo
  edit(*n->mesh);
  n->mesh->touch();
  n->extras.erase("og_shape");  // no longer made from its size
  commit_node_edit(before, tr(undo_key));
  m_topo_revision = history.revision();
}

void App::topology_tool(bool clicked, bool released) {
  ImGuiIO& io = ImGui::GetIO();
  ImDrawList* dl = ImGui::GetBackgroundDrawList();
  Node* node = scene.find(m_topo_node);
  if (!node || !node->mesh) {
    stop_topology();
    return;
  }
  // undone or redone: what was picked may be gone
  if (history.revision() != m_topo_revision && !m_topo_before) {
    m_topo_revision = history.revision();
    m_topo_points.clear();
    m_topo_faces_sel.clear();
  }
  hovered = 0;
  const Mat4 world = scene.world(*node);
  const Mat4 inv = world.inverse();
  Mesh& mesh = *node->mesh;
  const MeshPoints& pts = topology_points();

  // the gizmo on what is picked
  const std::set<MeshRef> verts = topology_vertices();
  if (!verts.empty()) {
    Vec3 c{0, 0, 0};
    for (MeshRef r : verts) c = c + world.point(mesh.prims[ref_prim(r)].pos[ref_index(r)]);
    c = c * (1.f / verts.size());
    Mat4 frame = world;
    frame.m[12] = c.x;
    frame.m[13] = c.y;
    frame.m[14] = c.z;
    if (gizmo.space == GizmoSpace::World) frame = Mat4::translate(c);
    GizmoInput gi;
    gi.mouse = m_mouse_vp;
    gi.viewport_pos = {m_vp_x, m_vp_y};
    gi.viewport_size = {m_vp_w, m_vp_h};
    gi.mouse_in_viewport = m_mouse_in_vp && !m_box_selecting;
    gi.pressed = clicked;
    gi.down = ImGui::IsMouseDown(ImGuiMouseButton_Left);
    gi.released = released;
    gi.fine = io.KeyShift;
    gi.snap_toggle = io.KeyCtrl;
    GizmoResult res = gizmo.update(camera, renderer, gi, frame, dl);
    if (res.started) {
      m_topo_before = std::make_unique<Node>(*node);
      node->mesh = std::make_shared<Mesh>(*node->mesh);  // its own, the one before kept for undo
      m_topo_start.clear();
      for (MeshRef r : verts) m_topo_start.push_back({r, world.point(node->mesh->prims[ref_prim(r)].pos[ref_index(r)])});
      m_topo_pivot = c;
      m_topo_axes = frame;
    }
    if (res.dragging && m_topo_before) {
      Mesh& m = *node->mesh;
      for (const auto& [r, start] : m_topo_start) {
        Vec3 w;
        if (gizmo.op == GizmoOp::Scale) {
          // along the gizmo's axes, around the middle of what is picked
          const Vec3 d = start - m_topo_pivot;
          Vec3 out = m_topo_pivot;
          for (int k = 0; k < 3; k++) {
            const Vec3 axis = normalize(m_topo_axes.col3(k));
            out = out + axis * (dot(d, axis) * res.scale[k]);
          }
          w = out;
        } else {
          w = res.delta.point(start);
        }
        m.prims[ref_prim(r)].pos[ref_index(r)] = inv.point(w);
      }
      m.touch();
    }
    if (res.finished && m_topo_before) {
      std::set<size_t> prims;
      for (const auto& [r, start] : m_topo_start) prims.insert(ref_prim(r));
      for (size_t p : prims) node->mesh->prims[p].compute_normals();
      node->mesh->touch();
      node->extras.erase("og_shape");
      const Node before = *m_topo_before;
      m_topo_before.reset();
      commit_node_edit(before, tr(gizmo.op == GizmoOp::Translate ? "undo.topology.move"
                                  : gizmo.op == GizmoOp::Rotate ? "undo.topology.rotate"
                                                                : "undo.topology.scale"));
      m_topo_revision = history.revision();
    }
    if (gizmo.active() || (gizmo.hovering() && clicked)) return;
  }

  // the points (or the faces' edges) drawn over the view
  const bool many = pts.pos.size() > 40000;
  if (!m_topo_faces && !many)
    for (uint32_t i = 0; i < pts.pos.size(); i++) {
      Vec2 s;
      if (!renderer.project(world.point(pts.pos[i]), &s)) continue;
      const bool on = m_topo_points.count(i) != 0;
      dl->AddCircleFilled(ImVec2(m_vp_x + s.x, m_vp_y + s.y), on ? 4.5f : 2.5f,
                          on ? IM_COL32(255, 160, 40, 255) : IM_COL32(220, 230, 240, 200));
    }

  // picking: a click, Shift adds (or takes out), a box
  if (clicked) {
    m_box_start = m_mouse_vp;
    m_box_selecting = false;
  }
  if (m_lmb_in_vp && ImGui::IsMouseDown(ImGuiMouseButton_Left) && !gizmo.active()) {
    if (!m_box_selecting && (m_mouse_vp - m_box_start).length() > 6.f) m_box_selecting = true;
    if (m_box_selecting) {
      ImVec2 a(m_vp_x + std::min(m_box_start.x, m_mouse_vp.x), m_vp_y + std::min(m_box_start.y, m_mouse_vp.y));
      ImVec2 b(m_vp_x + std::max(m_box_start.x, m_mouse_vp.x), m_vp_y + std::max(m_box_start.y, m_mouse_vp.y));
      dl->AddRectFilled(a, b, IM_COL32(255, 160, 40, 30));
      dl->AddRect(a, b, IM_COL32(255, 160, 40, 200));
    }
  }
  if (!(released && m_lmb_in_vp)) return;
  const bool add = io.KeyShift || io.KeyCtrl;
  if (m_box_selecting) {
    m_box_selecting = false;
    const float x0 = std::min(m_box_start.x, m_mouse_vp.x), x1 = std::max(m_box_start.x, m_mouse_vp.x);
    const float y0 = std::min(m_box_start.y, m_mouse_vp.y), y1 = std::max(m_box_start.y, m_mouse_vp.y);
    auto inside = [&](const Vec3& local) {
      Vec2 s;
      return renderer.project(world.point(local), &s) && s.x >= x0 && s.x <= x1 && s.y >= y0 && s.y <= y1;
    };
    if (!add) {
      m_topo_points.clear();
      m_topo_faces_sel.clear();
    }
    if (m_topo_faces) {
      for (size_t pi = 0; pi < mesh.prims.size(); pi++) {
        const Primitive& p = mesh.prims[pi];
        for (size_t t = 0; t + 2 < p.idx.size(); t += 3)
          if (inside((p.pos[p.idx[t]] + p.pos[p.idx[t + 1]] + p.pos[p.idx[t + 2]]) * (1.f / 3.f)))
            m_topo_faces_sel.insert(mesh_ref(pi, t / 3));
      }
    } else {
      for (uint32_t i = 0; i < pts.pos.size(); i++)
        if (inside(pts.pos[i])) m_topo_points.insert(i);
    }
    return;
  }
  if (!m_mouse_in_vp) return;
  if (m_topo_faces) {
    // the face under the cursor
    const Ray r = camera.ray(m_mouse_vp.x, m_mouse_vp.y, m_vp_w, m_vp_h);
    float best = 1e30f;
    MeshRef hit = ~0ull;
    for (size_t pi = 0; pi < mesh.prims.size(); pi++) {
      const Primitive& p = mesh.prims[pi];
      for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
        float d;
        if (ray_triangle(r, world.point(p.pos[p.idx[t]]), world.point(p.pos[p.idx[t + 1]]), world.point(p.pos[p.idx[t + 2]]), &d) &&
            d < best) {
          best = d;
          hit = mesh_ref(pi, t / 3);
        }
      }
    }
    if (!add) m_topo_faces_sel.clear();
    if (hit != ~0ull) {
      if (add && m_topo_faces_sel.count(hit)) m_topo_faces_sel.erase(hit);
      else m_topo_faces_sel.insert(hit);
    }
  } else {
    // the point nearest the cursor on screen (within 12 pixels), the nearest the camera among equals
    int best = -1;
    float best_d = 12.f, best_depth = 1e30f;
    for (uint32_t i = 0; i < pts.pos.size(); i++) {
      Vec2 s;
      const Vec3 w = world.point(pts.pos[i]);
      if (!renderer.project(w, &s)) continue;
      const float d = (s - m_mouse_vp).length(), depth = length(w - camera.pos);
      if (d < best_d - 0.5f || (d <= best_d + 0.5f && depth < best_depth)) {
        best = (int)i;
        best_d = std::min(d, best_d);
        best_depth = depth;
      }
    }
    if (!add) m_topo_points.clear();
    if (best >= 0) {
      if (add && m_topo_points.count((uint32_t)best)) m_topo_points.erase((uint32_t)best);
      else m_topo_points.insert((uint32_t)best);
    }
  }
}

void App::draw_topology_overlay() {
  const Node* node = scene.find(m_topo_node);
  if (!node || !node->mesh) return;
  const Mat4 world = scene.world(*node);
  const Mesh& mesh = *node->mesh;
  size_t tris = 0;
  for (const auto& p : mesh.prims) tris += p.idx.size() / 3;
  // its edges (a mesh of many triangles: its picked faces only)
  if (tris <= 60000)
    for (const auto& p : mesh.prims)
      for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
        const Vec3 a = world.point(p.pos[p.idx[t]]), b = world.point(p.pos[p.idx[t + 1]]), c = world.point(p.pos[p.idx[t + 2]]);
        renderer.line(a, b, 0x90f0e0c0);
        renderer.line(b, c, 0x90f0e0c0);
        renderer.line(c, a, 0x90f0e0c0);
      }
  for (MeshRef f : m_topo_faces_sel) {
    const size_t pi = ref_prim(f), t = ref_index(f);
    if (pi >= mesh.prims.size() || t * 3 + 2 >= mesh.prims[pi].idx.size()) continue;
    const Primitive& p = mesh.prims[pi];
    renderer.tri(world.point(p.pos[p.idx[t * 3]]), world.point(p.pos[p.idx[t * 3 + 1]]),
                 world.point(p.pos[p.idx[t * 3 + 2]]), 0x8028a0ff);
  }
  renderer.flush(true, true);
}

void App::ui_topology_inspector() {
  Node* node = scene.find(m_topo_node);
  if (!node || !node->mesh) return;
  ImGui::SeparatorText(tr("topology.title"));
  guide_tip("topology");
  ImGui::TextWrapped("%s", node->name.c_str());
  const MeshPoints& pts = topology_points();
  size_t tris = 0;
  for (const auto& p : node->mesh->prims) tris += p.idx.size() / 3;
  ImGui::TextDisabled(tr("topology.counts"), pts.pos.size(), tris);
  if (ImGui::RadioButton(tr_id("topology.points").c_str(), !m_topo_faces)) {
    // the faces picked give their points
    if (m_topo_faces) {
      const std::set<MeshRef> v = face_vertices(*node->mesh, m_topo_faces_sel);
      m_topo_points.clear();
      for (uint32_t i = 0; i < pts.verts.size(); i++)
        for (MeshRef r : pts.verts[i])
          if (v.count(r)) {
            m_topo_points.insert(i);
            break;
          }
    }
    m_topo_faces = false;
  }
  tooltip(tr("topology.points.tip"));
  ImGui::SameLine();
  if (ImGui::RadioButton(tr_id("topology.faces").c_str(), m_topo_faces)) {
    if (!m_topo_faces) m_topo_faces_sel = faces_of_vertices(*node->mesh, topology_vertices());
    m_topo_faces = true;
  }
  tooltip(tr("topology.faces.tip"));
  const std::set<MeshRef> faces = m_topo_faces ? m_topo_faces_sel : faces_of_vertices(*node->mesh, topology_vertices());
  ImGui::TextDisabled(tr("topology.picked"), m_topo_faces ? m_topo_faces_sel.size() : m_topo_points.size(),
                      tr(m_topo_faces ? "topology.faces_word" : "topology.points_word"));
  if (ImGui::Button(tr_id("topology.all").c_str())) {
    if (m_topo_faces) {
      for (size_t pi = 0; pi < node->mesh->prims.size(); pi++)
        for (size_t t = 0; t * 3 + 2 < node->mesh->prims[pi].idx.size(); t++) m_topo_faces_sel.insert(mesh_ref(pi, t));
    } else {
      for (uint32_t i = 0; i < pts.pos.size(); i++) m_topo_points.insert(i);
    }
  }
  ImGui::SameLine();
  if (ImGui::Button(tr_id("topology.none").c_str())) {
    m_topo_points.clear();
    m_topo_faces_sel.clear();
  }
  ImGui::BeginDisabled(faces.empty());
  ImGui::PushItemWidth(80);
  if (ImGui::Button(tr_id("topology.extrude").c_str())) {
    std::set<MeshRef> made;
    topology_edit("undo.topology.extrude", [&](Mesh& m) { made = extrude_faces(m, faces, m_topo_extrude); });
    m_topo_faces = true;
    m_topo_faces_sel = made;
  }
  tooltip(tr("topology.extrude.tip"));
  ImGui::SameLine();
  ImGui::DragFloat("##extrude", &m_topo_extrude, 0.05f, -100.f, 100.f, "%.2f m");
  ImGui::PopItemWidth();
  if (ImGui::Button(tr_id("topology.subdivide").c_str())) {
    std::set<MeshRef> made;
    topology_edit("undo.topology.subdivide", [&](Mesh& m) { made = subdivide_faces(m, faces); });
    m_topo_faces = true;
    m_topo_faces_sel = made;
  }
  tooltip(tr("topology.subdivide.tip"));
  ImGui::SameLine();
  if (ImGui::Button(tr_id("topology.flip").c_str())) topology_edit("undo.topology.flip", [&](Mesh& m) { flip_faces(m, faces); });
  tooltip(tr("topology.flip.tip"));
  ImGui::SameLine();
  if (ImGui::Button(tr_id("topology.delete").c_str())) topology_delete();
  tooltip(tr("topology.delete.tip"));
  ImGui::EndDisabled();
  if (ImGui::Button(tr_id("topology.done").c_str(), ImVec2(-1, 0))) stop_topology();
  ImGui::TextDisabled("%s", tr("topology.keys"));
}

void App::topology_delete() {
  Node* node = scene.find(m_topo_node);
  if (!node || !node->mesh) return;
  const std::set<MeshRef> faces = m_topo_faces ? m_topo_faces_sel : faces_of_vertices(*node->mesh, topology_vertices());
  if (faces.empty()) return;
  topology_edit("undo.topology.delete", [&](Mesh& m) { delete_faces(m, faces); });
  m_topo_points.clear();
  m_topo_faces_sel.clear();
}

}  // namespace ogle
