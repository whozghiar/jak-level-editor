// Groups of the document: elements kept together under a name (extras og_group {id, name, prefab}
// on each of their root elements), saved with the project. A prefab placed is one (named after the
// prefab), a selection grouped by the user another (Edit > Group the selection, Groups panel). A
// click selects one element; a double click, or the left button held still, the whole group (an
// element in no group: its assembly).

#include <imgui.h>
#include <imgui_stdlib.h>

#include <algorithm>
#include <map>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "scene/assemblies.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::IsItemHovered(ImGuiHoveredFlags_DelayShort) && text && *text) ImGui::SetTooltip("%s", text);
}

std::string lower(std::string s) {
  for (auto& c : s) c = (char)std::tolower((unsigned char)c);
  return s;
}

}  // namespace

uint32_t group_tag(const Node& n) {
  auto it = n.extras.find("og_group");
  if (it == n.extras.end() || !it->is_object()) return 0;
  return it->value("id", 0u);
}

void set_group_tag(Node& n, uint32_t id, const std::string& name, bool prefab) {
  if (!id) n.extras.erase("og_group");
  else n.extras["og_group"] = json{{"id", id}, {"name", name}, {"prefab", prefab}};
}

uint32_t new_group_id(const Scene& scene) {
  uint32_t top = 0;
  for (const auto& n : scene.nodes) top = std::max(top, group_tag(n));
  return top + 1;
}

const std::vector<App::GroupInfo>& App::document_groups() const {
  if (m_groups_revision == history.revision() && m_groups_nodes == scene.nodes.size()) return m_groups;
  m_groups_revision = history.revision();
  m_groups_nodes = scene.nodes.size();
  std::map<uint32_t, GroupInfo> by_id;
  for (const auto& n : scene.nodes) {
    const uint32_t id = group_tag(n);
    if (!id) continue;
    GroupInfo& g = by_id[id];
    if (g.roots.empty()) {
      const json& tag = n.extras["og_group"];
      g.id = id;
      g.name = tag.value("name", std::string());
      g.prefab = tag.value("prefab", false);
    }
    g.roots.push_back(n.id);
  }
  m_groups.clear();
  for (auto& [id, g] : by_id) m_groups.push_back(std::move(g));
  return m_groups;
}

const App::GroupInfo* App::group_info(uint32_t gid) const {
  for (const auto& g : document_groups())
    if (g.id == gid) return &g;
  return nullptr;
}

uint32_t App::group_of(uint32_t id) const {
  for (const Node* n = scene.find(id); n; n = n->parent ? scene.find(n->parent) : nullptr)
    if (const uint32_t g = group_tag(*n)) return g;
  return 0;
}

void App::select_group(uint32_t gid) {
  const GroupInfo* g = group_info(gid);
  if (!g || g->roots.empty()) return;
  selection = std::set<uint32_t>(g->roots.begin(), g->roots.end());
  if (!selection.count(primary)) primary = g->roots.front();
}

bool App::select_whole(uint32_t id) {
  if (!id) return false;
  if (const uint32_t g = group_of(id)) {
    select_group(g);
    return true;
  }
  select_assembly(id);
  return selection.size() > 1;
}

uint32_t App::group_selection(const std::string& name) {
  const auto roots = selection_roots();
  if (roots.empty()) return 0;
  const uint32_t gid = new_group_id(scene);
  NodeChange c(scene);
  for (auto id : roots) {
    c.save(id);
    if (Node* n = scene.find(id)) set_group_tag(*n, gid, name, false);
  }
  history.push(c.commit(strf(tr("undo.group"), name.c_str())));
  return gid;
}

void App::ungroup(uint32_t gid) {
  const GroupInfo* g = group_info(gid);
  if (!g) return;
  const std::vector<uint32_t> roots = g->roots;
  const std::string name = g->name;
  NodeChange c(scene);
  for (auto id : roots) {
    c.save(id);
    if (Node* n = scene.find(id)) set_group_tag(*n, 0, "", false);
  }
  history.push(c.commit(strf(tr("undo.ungroup"), name.c_str())));
}

void App::rename_group(uint32_t gid, const std::string& name) {
  const GroupInfo* g = group_info(gid);
  if (!g || name.empty() || name == g->name) return;
  const std::vector<uint32_t> roots = g->roots;
  const bool prefab = g->prefab;
  NodeChange c(scene);
  for (auto id : roots) {
    c.save(id);
    if (Node* n = scene.find(id)) set_group_tag(*n, gid, name, prefab);
  }
  history.push(c.commit(strf(tr("undo.rename_group"), name.c_str())));
}

void App::ui_group_section() {
  // the group of the selection: all of it in one group, else a group to make of it
  const auto roots = selection_roots();
  if (roots.empty()) return;
  uint32_t gid = group_of(roots.front());
  for (auto id : roots)
    if (group_of(id) != gid) gid = 0;
  const GroupInfo* g = gid ? group_info(gid) : nullptr;
  if (g) {
    const bool whole = std::all_of(g->roots.begin(), g->roots.end(), [&](uint32_t r) { return selection.count(r); });
    ImGui::SeparatorText(tr(g->prefab ? "group.prefab_title" : "group.title"));
    ImGui::TextWrapped(tr("group.name_count"), g->name.c_str(), g->roots.size());
    if (!whole) {
      if (ImGui::Button(tr_id("group.select_all").c_str())) select_group(gid);
      tooltip(tr("group.select_all.tip"));
      ImGui::SameLine();
    }
    if (ImGui::Button(tr_id("group.ungroup").c_str())) ungroup(gid);
    tooltip(tr("group.ungroup.tip"));
    return;
  }
  if (roots.size() < 2) return;
  ImGui::SeparatorText(tr("group.title"));
  if (ImGui::Button(tr_id("group.make").c_str())) {
    m_group_name = strf(tr("group.default_name"), document_groups().size() + 1);
    m_show_new_group = true;
  }
  tooltip(tr("group.make.tip"));
}

void App::ui_new_group_window() {
  if (m_show_new_group) {
    ImGui::OpenPopup(tr_id("group.new_title").c_str());
    m_show_new_group = false;
    m_group_focus = true;
  }
  if (!ImGui::BeginPopupModal(tr_id("group.new_title").c_str(), nullptr, ImGuiWindowFlags_AlwaysAutoResize)) return;
  ImGui::TextWrapped(tr("group.new_text"), selection_roots().size());
  if (m_group_focus) {
    ImGui::SetKeyboardFocusHere();
    m_group_focus = false;
  }
  ImGui::SetNextItemWidth(320);
  const bool enter = ImGui::InputText("##group-name", &m_group_name, ImGuiInputTextFlags_EnterReturnsTrue);
  ImGui::BeginDisabled(m_group_name.empty() || selection.empty());
  if (ImGui::Button(tr_id("group.create").c_str()) || (enter && !m_group_name.empty())) {
    group_selection(m_group_name);
    ImGui::CloseCurrentPopup();
  }
  ImGui::EndDisabled();
  ImGui::SameLine();
  if (ImGui::Button(tr_id("popup.cancel").c_str()) || ImGui::IsKeyPressed(ImGuiKey_Escape)) ImGui::CloseCurrentPopup();
  ImGui::EndPopup();
}

void App::ui_groups_panel() {
  if (!document_open()) {
    ImGui::TextWrapped("%s", tr("groups.no_document"));
    return;
  }
  ImGui::PushStyleColor(ImGuiCol_Text, ImGui::GetStyleColorVec4(ImGuiCol_TextDisabled));
  ImGui::TextWrapped("%s", tr("groups.help"));
  ImGui::PopStyleColor();
  ImGui::BeginDisabled(selection_roots().size() < 1);
  if (ImGui::Button(tr_id("group.make").c_str(), ImVec2(-1, 0))) {
    m_group_name = strf(tr("group.default_name"), document_groups().size() + 1);
    m_show_new_group = true;
  }
  ImGui::EndDisabled();
  tooltip(tr("group.make.tip"));
  ImGui::SetNextItemWidth(-1);
  ImGui::InputTextWithHint("##groups-filter", tr("groups.filter"), &m_groups_filter);
  const std::string f = lower(m_groups_filter);
  const auto& groups = document_groups();
  if (groups.empty()) {
    ImGui::Spacing();
    ImGui::TextWrapped("%s", tr("groups.none"));
    return;
  }
  ImGui::TextDisabled(tr("groups.count"), groups.size());
  ImGui::BeginChild("groups");
  uint32_t to_ungroup = 0, to_rename = 0;
  for (const auto& g : groups) {
    if (!f.empty() && lower(g.name).find(f) == std::string::npos) continue;
    ImGui::PushID((int)g.id);
    const bool whole = !g.roots.empty() &&
                       std::all_of(g.roots.begin(), g.roots.end(), [&](uint32_t r) { return selection.count(r); });
    if (m_renaming_group == g.id) {
      if (m_group_focus) {
        ImGui::SetKeyboardFocusHere();
        m_group_focus = false;
      }
      ImGui::SetNextItemWidth(-1);
      if (ImGui::InputText("##rename", &m_group_name, ImGuiInputTextFlags_EnterReturnsTrue)) to_rename = g.id;
      if (ImGui::IsItemDeactivated() && !to_rename) m_renaming_group = 0;
    } else {
      const std::string label = strf("%s %s  (%zu)", g.prefab ? "[P]" : "[G]", g.name.c_str(), g.roots.size());
      if (ImGui::Selectable(label.c_str(), whole, ImGuiSelectableFlags_AllowDoubleClick)) {
        select_group(g.id);
        if (ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left)) frame_selection();
      }
      tooltip(tr(g.prefab ? "groups.prefab.tip" : "groups.group.tip"));
      if (ImGui::BeginPopupContextItem()) {
        if (ImGui::MenuItem(tr("groups.rename"))) {
          m_renaming_group = g.id;
          m_group_name = g.name;
          m_group_focus = true;
        }
        if (ImGui::MenuItem(tr("groups.frame"))) {
          select_group(g.id);
          frame_selection();
        }
        if (ImGui::MenuItem(tr("group.ungroup"))) to_ungroup = g.id;
        ImGui::EndPopup();
      }
    }
    ImGui::PopID();
  }
  ImGui::EndChild();
  if (to_rename) {
    rename_group(to_rename, m_group_name);
    m_renaming_group = 0;
  }
  if (to_ungroup) ungroup(to_ungroup);
}

}  // namespace ogle
