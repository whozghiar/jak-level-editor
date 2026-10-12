// Tools to build levels without visual bugs (the rules of docs/mcp-agent-guide.md, the same for a
// person): closing a building open at the back (scene/closure.h), leaving the small pieces out of
// a prefab, finding the holes through which the player would see the void (scene/gaps.h).

#include <imgui.h>

#include <algorithm>
#include <cmath>
#include <unordered_set>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "scene/relight.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::IsItemHovered(ImGuiHoveredFlags_DelayShort) && text && *text) ImGui::SetTooltip("%s", text);
}

}  // namespace

std::vector<uint32_t> App::without_small(const std::vector<uint32_t>& roots, float min_size,
                                         std::vector<std::string>* left_out) const {
  if (min_size <= 0.f) return roots;
  std::vector<uint32_t> kept;
  for (auto id : roots) {
    const AABB b = subtree_bounds(id);
    const Vec3 s = b.valid() ? b.size() : Vec3{0, 0, 0};
    const Node* n = scene.find(id);
    // an actor is an object of the game, whatever its size (a button, a lamp)
    if ((n && n->kind == NodeKind::Actor) || std::max({s.x, s.y, s.z}) >= min_size) {
      kept.push_back(id);
    } else if (left_out && n) {
      left_out->push_back(n->name);
    }
  }
  return kept;
}

uint32_t App::close_building(const std::vector<uint32_t>& ids, const ClosureOptions& options, ClosureResult* out,
                             std::string* error) {
  std::vector<uint32_t> roots;
  for (auto id : ids)
    if (scene.find(id)) roots.push_back(id);
  if (roots.empty()) {
    if (error) *error = "no element";
    return 0;
  }
  ClosureResult r = close_structure(scene, roots, options);
  if (out) *out = r;
  if (!r.mesh) {
    if (error) *error = r.error;
    return 0;
  }
  NodeChange c(scene);
  const int fallback = shape_default_material(scene);
  for (auto& p : r.mesh->prims)
    if (p.material < 0) p.material = fallback;
  Node n;
  const Node* first = scene.find(roots.front());
  std::unordered_set<std::string> names;
  for (const auto& node : scene.nodes) names.insert(node.name);
  std::string name = (first ? first->name : std::string("building")) + "-closure";
  for (int i = 2; names.count(name); i++) name = (first ? first->name : std::string("building")) + "-closure-" + std::to_string(i);
  n.name = name;
  n.kind = NodeKind::Render;
  n.mesh = r.mesh;
  // its frame: the structure's axes (r.origin is in the frame turned around the structure's middle)
  n.local = Transform::from_matrix(Mat4::translate(r.origin) * Mat4::rotate(Quat::axis_angle({0, 1, 0}, r.yaw)));
  n.extras = json{{"og_closure", {{"cell", r.cell}, {"facade", r.facade}, {"relief", options.relief}, {"seed", options.seed}}}};
  // part of the group of what it closes (a prefab placed: selected and moved with it)
  const uint32_t gid = group_of(roots.front());
  bool one_group = gid != 0;
  for (auto id : roots) one_group &= group_of(id) == gid;
  if (one_group)
    if (const GroupInfo* g = group_info(gid)) set_group_tag(n, gid, g->name, g->prefab);
  const uint32_t id = scene.add(std::move(n));
  light_like_surroundings(scene, {id});
  history.push(c.commit(strf(tr("undo.close_building"), name.c_str())));
  return id;
}

void App::close_selection() {
  ClosureResult r;
  std::string err;
  ClosureOptions o;
  o.relief = closure_relief;
  // its front is the side the camera sees: filled behind and inside, as seen from here
  o.has_view = true;
  o.seen_from = camera.pos;
  const uint32_t id = close_building(selection_roots(), o, &r, &err);
  if (!id) {
    LOG_WARN(tr("log.close_failed"), err.c_str());
    return;
  }
  LOG_INFO(tr("log.closed"), r.added, r.cell);
  if (const uint32_t g = group_of(id)) select_group(g);
  else selection.insert(id);
}

AABB App::gaps_region() const {
  AABB r = selection_bounds();
  if (!r.valid()) r = own_level_bounds();
  return r;
}

void App::run_find_gaps(const AABB& region) {
  GapOptions o;
  o.region = region;
  if (m_ocean && rs.show_ocean) o.ocean = m_ocean_height;
  m_gaps = find_gaps(scene, o);
  m_gaps_done = true;
  show_gaps_window = true;
  LOG_INFO(tr("log.gaps"), m_gaps.spots.size(), m_gaps.viewpoints);
}

void App::draw_gaps_overlay() {
  if (!show_gaps_window || m_gaps.spots.empty()) return;
  size_t shown = 0;
  for (size_t i = 0; i < m_gaps.spots.size() && shown < 200; i++, shown++) {
    const GapSpot& s = m_gaps.spots[i];
    const uint32_t col = s.edge ? pack_rgba(1.f, 0.75f, 0.2f, 0.9f) : pack_rgba(1.f, 0.25f, 0.2f, 0.95f);
    const float r = 0.4f + std::min(2.f, s.rays * 0.1f);
    renderer.circle(s.at, {1, 0, 0}, {0, 0, 1}, r, col, 20);
    renderer.circle(s.at, {1, 0, 0}, {0, 1, 0}, r, col, 20);
    if ((int)i == m_gap_selected) renderer.line(s.seen_from, s.at, col);
  }
  renderer.flush(false);
}

void App::ui_gaps_window() {
  if (!show_gaps_window) return;
  ImGui::SetNextWindowSize(ImVec2(400, 380), ImGuiCond_FirstUseEver);
  if (!ImGui::Begin(tr_id("gaps.title").c_str(), &show_gaps_window)) {
    ImGui::End();
    return;
  }
  ImGui::PushStyleColor(ImGuiCol_Text, ImGui::GetStyleColorVec4(ImGuiCol_TextDisabled));
  ImGui::TextWrapped("%s", tr("gaps.help"));
  ImGui::PopStyleColor();
  ImGui::BeginDisabled(!document_open());
  if (ImGui::Button(tr_id(selection.empty() ? "gaps.run_level" : "gaps.run_selection").c_str(), ImVec2(-1, 0)))
    run_find_gaps(gaps_region());
  ImGui::EndDisabled();
  if (m_gaps_done) {
    size_t gaps = 0;
    for (const auto& s : m_gaps.spots) gaps += !s.edge;
    ImGui::TextWrapped(tr("gaps.result"), gaps, m_gaps.spots.size() - gaps, m_gaps.viewpoints, m_gaps.rays);
    ImGui::BeginChild("spots");
    for (size_t i = 0; i < m_gaps.spots.size(); i++) {
      const GapSpot& s = m_gaps.spots[i];
      ImGui::PushID((int)i);
      const std::string label = strf(tr(s.edge ? "gaps.edge" : "gaps.gap"), s.at.x, s.at.y, s.at.z, s.rays);
      if (ImGui::Selectable(label.c_str(), m_gap_selected == (int)i)) {
        // looked at from where the player would see it
        m_gap_selected = (int)i;
        camera.pos = s.seen_from;
        camera.look_at(s.at);
      }
      tooltip(tr("gaps.look_tip"));
      ImGui::PopID();
    }
    ImGui::EndChild();
  }
  ImGui::End();
}

}  // namespace ogle
