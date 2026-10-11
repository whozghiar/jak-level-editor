// The editor's interface: menus, toolbar, the Levels and Prefabs panels on the left, the inspector
// on the right, the status bar and the floating windows. Every text comes from the translations
// (core/i18n.h, lang/*.json).

#include <SDL3/SDL.h>

#include <algorithm>
#include <cstdio>
#include <filesystem>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "core/prefs.h"
#include "imgui.h"
#include "imgui_stdlib.h"
#include "io/jak2_import.h"

namespace ogle {

namespace fs = std::filesystem;

namespace {

fs::path to_path(const std::string& s) { return fs::path(std::u8string((const char8_t*)s.c_str())); }

std::string lower(std::string s) {
  for (auto& c : s) c = (char)std::tolower((unsigned char)c);
  return s;
}

// the longest prefix of `s` that fits in `width` pixels, with "..." when cut
std::string fit_text(const std::string& s, float width) {
  if (ImGui::CalcTextSize(s.c_str()).x <= width) return s;
  std::string t = s;
  while (!t.empty() && ImGui::CalcTextSize((t + "...").c_str()).x > width) t.pop_back();
  return t + "...";
}

void tooltip(const char* text) {
  if (ImGui::BeginItemTooltip()) {
    ImGui::PushTextWrapPos(ImGui::GetFontSize() * 28.f);
    ImGui::TextUnformatted(text);
    ImGui::PopTextWrapPos();
    ImGui::EndTooltip();
  }
}

const char* kind_label(NodeKind k) {
  switch (k) {
    case NodeKind::Render: return tr("kind.render");
    case NodeKind::Collision: return tr("kind.collision");
    case NodeKind::NavMesh: return tr("kind.navmesh");
    case NodeKind::Actor: return tr("kind.actor");
    default: return tr("kind.group");
  }
}

}  // namespace

// ------------------------------------------------------------------------------------------------
// undoable property edits
// ------------------------------------------------------------------------------------------------

void App::commit_node_edit(const Node& before, const std::string& label) {
  const Node* now = scene.find(before.id);
  if (!now) return;
  Node after = *now;
  Scene* s = &scene;
  uint32_t id = before.id;
  history.push({label,
                [s, id, before]() {
                  if (Node* n = s->find(id)) *n = before;
                },
                [s, id, after]() {
                  if (Node* n = s->find(id)) *n = after;
                }});
}

void App::edit_begin(const Node& n) {
  if (ImGui::IsItemActivated()) m_edit_before = n;
}

void App::edit_end(const std::string& label) {
  if (ImGui::IsItemDeactivatedAfterEdit() && m_edit_before) {
    commit_node_edit(*m_edit_before, label);
    m_edit_before.reset();
  } else if (ImGui::IsItemDeactivated()) {
    m_edit_before.reset();
  }
}

// ------------------------------------------------------------------------------------------------
// layout
// ------------------------------------------------------------------------------------------------

void App::draw_ui() {
  ImGuiIO& io = ImGui::GetIO();
  const float W = io.DisplaySize.x, H = io.DisplaySize.y;
  ui_menu();
  const float menu_h = ImGui::GetFrameHeight();
  const float tb_h = ImGui::GetFrameHeight() + 8.f + ImGui::GetStyle().WindowPadding.y * 2.f;
  ui_toolbar(menu_h, tb_h);
  const float top = menu_h + tb_h;
  const float status_h = ImGui::GetFrameHeight() + 8.f;
  const float panel_h = std::max(H - top - status_h, 100.f);
  m_left_w = std::clamp(m_left_w, 260.f, W * 0.45f);
  m_right_w = std::clamp(m_right_w, 260.f, W * 0.4f);
  ui_left_panel(0, top, m_left_w, panel_h);
  ui_inspector(W - m_right_w, top, m_right_w, panel_h);
  ui_statusbar(H - status_h, status_h);
  m_vp_x = m_left_w;
  m_vp_y = top;
  m_vp_w = std::max(W - m_left_w - m_right_w, 50.f);
  m_vp_h = panel_h;
  ui_story_bar();
  ui_windows();
  ui_extract_window();
}

void App::ui_menu() {
  if (!ImGui::BeginMainMenuBar()) return;
  const bool sel = !selection.empty();
  if (ImGui::BeginMenu(tr_id("menu.file").c_str())) {
    if (ImGui::MenuItem(tr("menu.file.extract"))) open_extract_window();
    ImGui::Separator();
    if (ImGui::MenuItem(tr("menu.file.new"), "Ctrl+N")) confirm_discard([this]() { new_level(); });
    if (ImGui::MenuItem(tr("menu.file.open"), "Ctrl+O"))
      confirm_discard([this]() { open_dialog(DialogAction::OpenProject); });
    const auto& recent = Prefs::get().recent_projects;
    if (ImGui::BeginMenu(tr_id("menu.file.recent").c_str(), !recent.empty())) {
      for (const auto& p : recent)
        if (ImGui::MenuItem(p.c_str())) confirm_discard([this, p]() { open_project_file(p); });
      ImGui::EndMenu();
    }
    if (ImGui::MenuItem(tr("menu.file.save"), "Ctrl+S", false, document_open())) save(false);
    if (ImGui::MenuItem(tr("menu.file.save_as"), "Ctrl+Shift+S", false, document_open())) save(true);
    ImGui::Separator();
    if (ImGui::MenuItem(tr("menu.file.import_model"), nullptr, false, document_open())) open_dialog(DialogAction::ImportModel);
    ImGui::Separator();
    if (ImGui::MenuItem(tr("menu.file.quit"))) request_quit();
    ImGui::EndMenu();
  }
  if (ImGui::BeginMenu(tr_id("menu.edit").c_str())) {
    if (ImGui::MenuItem(tr("menu.edit.undo"), "Ctrl+Z", false, history.can_undo())) history.undo();
    if (ImGui::MenuItem(tr("menu.edit.redo"), "Ctrl+Y", false, history.can_redo())) history.redo();
    ImGui::Separator();
    if (ImGui::MenuItem(tr("menu.edit.copy"), "Ctrl+C", false, sel)) copy_selection();
    if (ImGui::MenuItem(tr("menu.edit.paste"), "Ctrl+V", false, !clipboard.empty())) paste();
    if (ImGui::MenuItem(tr("menu.edit.duplicate"), "Ctrl+D", false, sel)) duplicate_selection();
    if (ImGui::MenuItem(tr("menu.edit.delete"), tr("key.delete"), false, sel)) delete_selection();
    ImGui::Separator();
    if (ImGui::MenuItem(tr("menu.edit.create_prefab"), nullptr, false, sel)) m_show_new_prefab = true;
    if (ImGui::MenuItem(tr("menu.edit.select_none"), tr("key.escape"), false, sel)) select_only(0);
    if (ImGui::MenuItem(tr("menu.edit.select_level"), "Ctrl+L", false, sel)) select_level(level_of_node(primary));
    ImGui::EndMenu();
  }
  if (ImGui::BeginMenu(tr_id("menu.view").c_str())) {
    if (ImGui::MenuItem(tr("menu.view.frame_selection"), "F", false, sel)) frame_selection();
    if (ImGui::MenuItem(tr("menu.view.frame_all"), tr("key.home"))) frame_all();
    ImGui::Separator();
    // the level drawn by the game's own renderer, lit at an hour of the day
    ImGui::BeginDisabled(!game_view.ready());
    ImGui::MenuItem(tr("menu.view.game_view"), "G", &game_view_on);
    ImGui::EndDisabled();
    if (ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled))
      ImGui::SetTooltip("%s", game_view.ready() ? tr("menu.view.game_view.tip")
                                                : strf(tr("menu.view.game_view.unavailable"), game_view_error.c_str()).c_str());
    if (game_view_on && game_view.ready()) {
      ImGui::Indent();
      ImGui::SetNextItemWidth(160);
      ImGui::SliderFloat(tr_id("menu.view.game_hour").c_str(), &game_view.mood.hour, 0.f, 24.f, "%.1f h");
      ImGui::MenuItem(tr("menu.view.game_fog"), nullptr, &game_view.fog);
      ImGui::MenuItem(tr("menu.view.game_lights"), nullptr, &game_view.mood.level_lights);
      if (ImGui::IsItemHovered()) ImGui::SetTooltip("%s", tr("menu.view.game_lights.tip"));
      ImGui::Unindent();
    }
    ImGui::Separator();
    ImGui::MenuItem(tr("menu.view.actors"), nullptr, &rs.show_actors);
    ImGui::MenuItem(tr("menu.view.ocean"), nullptr, &rs.show_ocean);
    ImGui::MenuItem(tr("menu.view.fog"), nullptr, &rs.fog);
    ImGui::MenuItem(tr("menu.view.navmesh"), nullptr, &show_navmesh);
    ImGui::MenuItem(tr("menu.view.labels"), nullptr, &show_labels);
    ImGui::MenuItem(tr("menu.view.grid"), nullptr, &rs.show_grid);
    ImGui::MenuItem(tr("menu.view.textures"), nullptr, &rs.textures);
    ImGui::MenuItem(tr("menu.view.select_terrain"), nullptr, &select_terrain);
    if (ImGui::IsItemHovered()) ImGui::SetTooltip("%s", tr("menu.view.select_terrain.tip"));
    ImGui::MenuItem(tr("menu.view.log"), nullptr, &show_log);
    ImGui::Separator();
    if (ImGui::BeginMenu(tr_id("menu.view.language").c_str())) {
      for (const auto& l : i18n_languages())
        if (ImGui::MenuItem(l.name.c_str(), nullptr, l.code == i18n_language())) set_language(l.code);
      ImGui::EndMenu();
    }
    ImGui::EndMenu();
  }
  // Build: the shapes, the palette of materials, a model of one's own
  if (ImGui::BeginMenu(tr_id("menu.create").c_str(), document_open())) {
    for (const auto& p : shape_presets())
      if (ImGui::MenuItem(tr(("menu.create." + std::string(p.name)).c_str()))) start_shape_placement(p, false);
    ImGui::Separator();
    if (ImGui::MenuItem(tr("menu.create.materials"))) left_tab_request = 3;
    if (ImGui::MenuItem(tr("menu.file.import_model"))) open_dialog(DialogAction::ImportModel);
    ImGui::EndMenu();
  }
  // Level: the levels of the document, the one it replaces in the game, nav meshes, the export
  if (ImGui::BeginMenu(tr_id("menu.level").c_str(), document_open())) {
    if (ImGui::MenuItem(tr("menu.level.add"))) left_tab_request = 0;
    if (ImGui::MenuItem(tr("menu.level.select"), "Ctrl+L")) select_level(0);
    if (ImGui::MenuItem(tr("menu.level.replace"))) {
      select_only(0);
      m_scroll_to_replace = true;
    }
    guide_tip("replace");
    if (ImGui::MenuItem(tr("menu.level.restore_effects"))) {
      size_t left_out = 0;
      const size_t added = restore_level_effects(&left_out);
      LOG_INFO(tr("log.effects_restored"), added, left_out);
    }
    if (ImGui::IsItemHovered()) ImGui::SetTooltip("%s", tr("menu.level.restore_effects.tip"));
    if (ImGui::MenuItem(tr("menu.level.navmesh"), nullptr, show_navmesh_window)) show_navmesh_window = !show_navmesh_window;
    ImGui::Separator();
    if (ImGui::MenuItem(tr("menu.file.export_game"))) open_export_window();
    guide_tip("export");
    ImGui::EndMenu();
  }
  if (ImGui::BeginMenu(tr_id("menu.help").c_str())) {
    const char* items[] = {"menu.help.tour", "menu.help.tools", "menu.help.keys"};
    for (int i = 0; i < 3; i++)
      if (ImGui::MenuItem(tr(items[i]), i == 0 ? "F1" : nullptr)) {
        show_help = true;
        guide_tab_request = i;
        if (i == 0) guide_step = 0;
      }
    ImGui::EndMenu();
  }
  ImGui::EndMainMenuBar();
}

void App::ui_toolbar(float y, float h) {
  ImGuiIO& io = ImGui::GetIO();
  ImGui::SetNextWindowPos(ImVec2(0, y));
  ImGui::SetNextWindowSize(ImVec2(io.DisplaySize.x, h));
  ImGui::Begin("##toolbar", nullptr,
               ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoMove |
                   ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoSavedSettings |
                   ImGuiWindowFlags_NoBringToFrontOnFocus);
  const float bh = ImGui::GetFrameHeight() + 8.f;
  const bool sel = !selection.empty();
  // a tip of the guide (its animation) for the tools, a plain one for the others
  auto button = [&](const char* key, const char* tip_key, bool active, bool enabled, const char* guide = nullptr) {
    if (active) ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(0.22f, 0.6f, 0.66f, 1.f));
    ImGui::BeginDisabled(!enabled);
    bool pressed = ImGui::Button(tr_id(key).c_str(), ImVec2(0, bh));
    ImGui::EndDisabled();
    if (active) ImGui::PopStyleColor();
    if (guide) guide_tip(guide);
    else if (tip_key) tooltip(tr(tip_key));
    ImGui::SameLine();
    return pressed;
  };
  if (button("toolbar.move", nullptr, gizmo.op == GizmoOp::Translate, true, "move")) {
    stop_placement();
    gizmo.op = GizmoOp::Translate;
  }
  if (button("toolbar.rotate", nullptr, gizmo.op == GizmoOp::Rotate, true, "rotate")) {
    stop_placement();
    gizmo.op = GizmoOp::Rotate;
  }
  if (button("toolbar.scale", nullptr, gizmo.op == GizmoOp::Scale, true, "scale")) {
    stop_placement();
    gizmo.op = GizmoOp::Scale;
  }
  ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 4);
  bool local = gizmo.space == GizmoSpace::Local;
  if (ImGui::Checkbox(tr_id("toolbar.local").c_str(), &local)) gizmo.space = local ? GizmoSpace::Local : GizmoSpace::World;
  guide_tip("local");
  ImGui::SameLine();
  ImGui::Checkbox(tr_id("toolbar.snap").c_str(), &gizmo.snap.enabled);
  guide_tip("snap");
  ImGui::SameLine(0, 2);
  if (ImGui::ArrowButton("##snap-steps", ImGuiDir_Down)) ImGui::OpenPopup("snap-steps");
  tooltip(tr("toolbar.snap.steps.tip"));
  if (ImGui::BeginPopup("snap-steps")) {
    // the steps of the snap: the grid of modular building (walls of 4 m on a 0.5 m grid...)
    ImGui::TextDisabled("%s", tr("toolbar.snap.steps"));
    ImGui::SetNextItemWidth(140);
    ImGui::DragFloat(tr_id("toolbar.snap.move").c_str(), &gizmo.snap.translate, 0.05f, 0.05f, 64.f, "%.2f m");
    for (float s : {0.1f, 0.25f, 0.5f, 1.f, 2.f, 4.f}) {
      ImGui::SameLine();
      if (ImGui::SmallButton(strf("%g##m%g", s, s).c_str())) gizmo.snap.translate = s;
    }
    ImGui::SetNextItemWidth(140);
    ImGui::DragFloat(tr_id("toolbar.snap.turn").c_str(), &gizmo.snap.rotate_deg, 0.5f, 1.f, 180.f, "%.0f deg");
    for (float s : {5.f, 15.f, 45.f, 90.f}) {
      ImGui::SameLine();
      if (ImGui::SmallButton(strf("%g##r%g", s, s).c_str())) gizmo.snap.rotate_deg = s;
    }
    ImGui::SetNextItemWidth(140);
    ImGui::DragFloat(tr_id("toolbar.snap.scale").c_str(), &gizmo.snap.scale, 0.01f, 0.01f, 10.f, "%.2f");
    ImGui::TextDisabled("%s", tr("toolbar.snap.keys"));
    ImGui::EndPopup();
  }
  ImGui::SameLine();
  ImGui::SetCursorPosY(ImGui::GetCursorPosY() - 4);
  ImGui::TextDisabled("|");
  ImGui::SameLine();
  if (button("toolbar.copy", "toolbar.copy.tip", false, sel)) copy_selection();
  if (button("toolbar.paste", "toolbar.paste.tip", false, !clipboard.empty())) paste();
  if (button("toolbar.delete", "toolbar.delete.tip", false, sel)) delete_selection();
  ImGui::TextDisabled("|");
  ImGui::SameLine();
  if (button("toolbar.prefab", nullptr, false, sel, "prefab")) m_show_new_prefab = true;
  ImGui::TextDisabled("|");
  ImGui::SameLine();
  if (button("toolbar.undo", nullptr, false, history.can_undo())) history.undo();
  if (button("toolbar.redo", nullptr, false, history.can_redo())) history.redo();
  ImGui::End();
}

void App::ui_left_panel(float x, float y, float w, float h) {
  ImGui::SetNextWindowPos(ImVec2(x, y));
  ImGui::SetNextWindowSize(ImVec2(w, h), ImGuiCond_FirstUseEver);
  ImGui::SetNextWindowSizeConstraints(ImVec2(260, h), ImVec2(ImGui::GetIO().DisplaySize.x * 0.45f, h));
  ImGui::Begin("##left", nullptr, ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoCollapse |
                                      ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoBringToFrontOnFocus);
  m_left_w = ImGui::GetWindowWidth();
  if (ImGui::BeginTabBar("panels")) {
    // shown in this order: Levels, Build, Catalog, Prefabs (left_tab_request: 0, 3, 1, 2)
    const char* tabs[] = {"tab.levels", "tab.catalog", "tab.prefabs", "tab.build"};
    const char* guides[] = {"levels", "catalog", "game_prefabs", "build"};
    for (int i : {0, 3, 1, 2}) {
      ImGuiTabItemFlags flags = left_tab_request == i ? ImGuiTabItemFlags_SetSelected : 0;
      const bool open = ImGui::BeginTabItem(tr_id(tabs[i]).c_str(), nullptr, flags);
      guide_tip(guides[i]);
      if (open) {
        if (i == 0) ui_levels_panel();
        else if (i == 1) ui_catalog_panel();
        else if (i == 2) ui_prefabs_panel();
        else ui_build_panel();
        ImGui::EndTabItem();
      }
    }
    left_tab_request = -1;
    ImGui::EndTabBar();
  }
  ImGui::End();
}

// ------------------------------------------------------------------------------------------------
// levels
// ------------------------------------------------------------------------------------------------

void App::ui_levels_panel() {
  if (library.empty()) {
    ImGui::Spacing();
    ImGui::TextWrapped("%s", tr("levels.no_library"));
    ImGui::Spacing();
    if (ImGui::Button(tr_id("levels.extract").c_str(), ImVec2(-1, 40))) open_extract_window();
    if (!library_root.empty()) {
      ImGui::Spacing();
      ImGui::TextDisabled("%s", tr("levels.folder_empty"));
      ImGui::TextWrapped("%s", library_root.c_str());
    }
    return;
  }
  ImGui::SetNextItemWidth(-1);
  ImGui::InputTextWithHint("##filter", tr("levels.filter"), &m_levels_filter);
  const std::string f = lower(m_levels_filter);
  std::vector<const LevelEntry*> shown;
  size_t listed = 0;
  for (const auto& l : library.levels()) {
    if (!l.listed) continue;
    listed++;
    if (f.empty() || l.name.find(f) != std::string::npos || lower(l.dgo).find(f) != std::string::npos)
      shown.push_back(&l);
  }
  ImGui::TextDisabled(tr("levels.count"), shown.size(), listed);

  const bool loading = m_level_future.valid() || level_adding();
  const LevelEntry* selected = library.level(m_selected_level);
  const float preview = std::min(ImGui::GetContentRegionAvail().x, 180.f);
  const float footer = selected ? preview + ImGui::GetFrameHeightWithSpacing() * 5 : ImGui::GetFrameHeightWithSpacing() * 4;
  // the levels of the document besides the one opened (counted again when it changes)
  static uint64_t added_rev = ~0ull;
  static size_t added_nodes = ~(size_t)0;
  static std::set<std::string> added;
  if (added_rev != history.revision() || added_nodes != scene.nodes.size()) {
    added_rev = history.revision();
    added_nodes = scene.nodes.size();
    added.clear();
    for (const auto& l : document_levels())
      if (l.role == DocumentLevel::Added || l.role == DocumentLevel::Companion) added.insert(l.name);
  }
  ImGui::BeginChild("levels", ImVec2(0, -footer), ImGuiChildFlags_Borders);
  ImGuiListClipper clipper;
  clipper.Begin((int)shown.size());
  while (clipper.Step()) {
    for (int i = clipper.DisplayStart; i < clipper.DisplayEnd; i++) {
      const LevelEntry* l = shown[i];
      const bool is_open = l->name == scene.settings.level;
      if (ImGui::Selectable((l->name + "##" + l->name).c_str(), l->name == m_selected_level,
                            ImGuiSelectableFlags_AllowDoubleClick)) {
        m_selected_level = l->name;
        if (ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left) && !loading) {
          LevelEntry entry = *l;
          confirm_discard([this, entry]() { load_level(entry); });
        }
      }
      ImGui::SameLine();
      if (is_open) ImGui::TextColored(ImVec4(0.45f, 0.85f, 0.5f, 1.f), "%s", tr("levels.open_mark"));
      else if (added.count(l->name)) ImGui::TextColored(ImVec4(0.45f, 0.7f, 0.95f, 1.f), "%s", tr("levels.added_mark"));
      else ImGui::TextDisabled("%s", l->dgo.c_str());
    }
  }
  if (shown.empty()) ImGui::TextDisabled("%s", tr("levels.none_found"));
  ImGui::EndChild();

  // the chosen level: a picture and the button that loads it
  if (selected) {
    ThumbRequest tr_req;
    tr_req.kind = ThumbRequest::Kind::Level;
    tr_req.path = selected->background;
    tr_req.stamp = selected->mtime;
    unsigned tex = thumbs.get(tr_req);
    if (tex) ImGui::Image((ImTextureID)(uint64_t)tex, ImVec2(preview, preview));
    else ImGui::Button(thumbs.failed(tr_req) ? "?" : "...", ImVec2(preview, preview));
    ImGui::SameLine();
    ImGui::BeginGroup();
    ImGui::TextUnformatted(selected->name.c_str());
    ImGui::TextDisabled("%s", selected->dgo.c_str());
    ImGui::EndGroup();
  }
  ImGui::Checkbox(tr_id("levels.with_actors").c_str(), &load_actors);
  tooltip(tr("levels.with_actors.tip"));
  ImGui::BeginDisabled(!selected || loading);
  if (ImGui::Button(m_level_future.valid() ? tr_id("levels.loading").c_str() : tr_id("levels.load").c_str(),
                    ImVec2(-1, 34)) &&
      selected) {
    LevelEntry entry = *selected;
    confirm_discard([this, entry]() { load_level(entry); });
  }
  ImGui::EndDisabled();
  // or add it to the document, at its place in the game's world
  const bool in_document = selected && (selected->name == scene.settings.level || added.count(selected->name));
  ImGui::BeginDisabled(!selected || loading || !document_open() || in_document);
  if (ImGui::Button(level_adding() ? tr_id("levels.adding").c_str() : tr_id("levels.add").c_str(), ImVec2(-1, 0)) &&
      selected) {
    std::string err;
    if (!add_level_to_document(*selected, &err)) LOG_WARN("%s", err.c_str());
  }
  ImGui::EndDisabled();
  tooltip(in_document ? tr("levels.add.in_document") : tr("levels.add.tip"));
  if (ImGui::SmallButton(tr_id("levels.extract_more").c_str())) open_extract_window();
}

// ------------------------------------------------------------------------------------------------
// catalog
// ------------------------------------------------------------------------------------------------

void App::ui_catalog_panel() {
  if (library.empty()) {
    ImGui::Spacing();
    ImGui::TextWrapped("%s", tr("catalog.no_library"));
    return;
  }
  if (ImGui::RadioButton(tr_id("catalog.objects").c_str(), m_catalog_tab == 0)) m_catalog_tab = 0;
  guide_tip("catalog");
  ImGui::SameLine();
  if (ImGui::RadioButton(tr_id("catalog.interactive").c_str(), m_catalog_tab == 3)) m_catalog_tab = 3;
  guide_tip("interactive");
  ImGui::SameLine();
  if (ImGui::RadioButton(tr_id("catalog.models").c_str(), m_catalog_tab == 1)) m_catalog_tab = 1;
  guide_tip("models");
  ImGui::SameLine();
  if (ImGui::RadioButton(tr_id("catalog.decor").c_str(), m_catalog_tab == 2)) m_catalog_tab = 2;
  guide_tip("decor");
  if (m_catalog_tab == 0 || m_catalog_tab == 3) ui_catalog_objects(m_catalog_tab == 3);
  else if (m_catalog_tab == 1) ui_catalog_models();
  else ui_catalog_decor();
}

namespace {

// A grid of thumbnails with their names, rows outside the view skipped (the catalog has
// hundreds of cells). `cell(i)` draws cell i: its button and its name.
void thumbnail_grid(int count, float size, const std::function<void(int)>& cell) {
  const ImGuiStyle& style = ImGui::GetStyle();
  const float cell_w = size + 6 + style.ItemSpacing.x;
  const int per_row = std::max(1, (int)((ImGui::GetContentRegionAvail().x + style.ItemSpacing.x) / cell_w));
  const int rows = (count + per_row - 1) / per_row;
  const float row_h = size + 6 + ImGui::GetTextLineHeightWithSpacing() + style.ItemSpacing.y;
  ImGuiListClipper clipper;
  clipper.Begin(rows, row_h);
  while (clipper.Step()) {
    for (int r = clipper.DisplayStart; r < clipper.DisplayEnd; r++) {
      for (int c = 0; c < per_row; c++) {
        const int i = r * per_row + c;
        if (i >= count) break;
        if (c) ImGui::SameLine();
        ImGui::PushID(i);
        ImGui::BeginGroup();
        cell(i);
        ImGui::EndGroup();
        ImGui::PopID();
      }
    }
  }
}

}  // namespace

void App::ui_catalog_objects(bool interactive) {
  // the family, the filter
  std::vector<std::string> families;
  for (const auto& o : catalog)
    if (o.interactive == interactive && (families.empty() || families.back() != o.family)) families.push_back(o.family);
  if (std::find(families.begin(), families.end(), m_catalog_family) == families.end()) m_catalog_family.clear();
  ImGui::SetNextItemWidth(-1);
  const std::string current = m_catalog_family.empty() ? tr("catalog.all_families") : tr(("family." + m_catalog_family).c_str());
  if (ImGui::BeginCombo("##family", current.c_str())) {
    if (ImGui::Selectable(tr("catalog.all_families"), m_catalog_family.empty())) m_catalog_family.clear();
    for (const auto& f : families)
      if (ImGui::Selectable(tr(("family." + f).c_str()), m_catalog_family == f)) m_catalog_family = f;
    ImGui::EndCombo();
  }
  ImGui::SetNextItemWidth(-1);
  ImGui::InputTextWithHint("##catalog-filter", tr("catalog.filter"), &m_catalog_filter);
  const std::string f = lower(m_catalog_filter);
  std::vector<const CatalogObject*> shown;
  size_t total = 0;
  for (const auto& o : catalog) {
    if (o.interactive != interactive) continue;
    total++;
    if (!m_catalog_family.empty() && o.family != m_catalog_family) continue;
    if (!f.empty() && o.name.find(f) == std::string::npos && o.etype.find(f) == std::string::npos &&
        o.model.find(f) == std::string::npos)
      continue;
    shown.push_back(&o);
  }
  ImGui::TextDisabled(tr("catalog.count"), shown.size(), total);
  if (catalog.empty()) {
    ImGui::TextWrapped("%s", tr("catalog.empty"));
    return;
  }
  ImGui::BeginChild("objects");
  const float size = 80.f;
  thumbnail_grid((int)shown.size(), size, [&](int i) {
    const CatalogObject& o = *shown[i];
    ThumbRequest req;
    req.kind = ThumbRequest::Kind::Model;
    req.path = o.model_path;
    unsigned tex = o.model_path.empty() ? 0 : thumbs.get(req);
    const bool placing = tool == ToolMode::Place && placement.kind == Placement::Kind::Object &&
                         placement.object.name == o.name && placement.object.etype == o.etype;
    if (placing) ImGui::PushStyleColor(ImGuiCol_Button, ImGui::GetStyleColorVec4(ImGuiCol_ButtonActive));
    ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(3, 3));
    const char* label = !o.model_path.empty()   ? (thumbs.failed(req) ? "?" : "...")
                        : o.family == "effect" ? tr("catalog.effect")
                                               : tr("catalog.marker");
    const bool clicked = tex ? ImGui::ImageButton("##img", (ImTextureID)(uint64_t)tex, ImVec2(size, size))
                             : ImGui::Button((std::string(label) + "##img").c_str(), ImVec2(size + 6, size + 6));
    ImGui::PopStyleVar();
    if (placing) ImGui::PopStyleColor();
    if (ImGui::BeginItemTooltip()) {
      ImGui::TextUnformatted(o.name.c_str());
      ImGui::TextDisabled("%s", tr(("family." + o.family).c_str()));
      ImGui::Text(tr("inspector.type"), o.etype.c_str());
      if (o.family == "effect") {
        ImGui::Text(tr("catalog.particles"), o.lump.value("art-name", std::string()).c_str());
        ImGui::TextDisabled("%s", tr("catalog.effect_tip"));
      } else if (o.model.empty()) {
        ImGui::TextDisabled("%s", tr("catalog.no_model"));
      } else {
        ImGui::Text(tr("inspector.model"), o.model.c_str());
      }
      if (o.count) {
        std::string levels;
        for (const auto& l : o.levels) levels += (levels.empty() ? "" : ", ") + l;
        ImGui::TextWrapped(tr("catalog.found"), o.count, levels.c_str());
      } else {
        ImGui::TextDisabled("%s", tr("catalog.not_found"));
      }
      ImGui::TextDisabled("%s", tr("catalog.place_tip"));
      ImGui::EndTooltip();
    }
    if (clicked) start_object_placement(o, false);
    if (ImGui::BeginDragDropSource()) {
      if (!m_drag_from_panel) {
        m_drag_from_panel = true;
        start_object_placement(o, true);
      }
      int dummy = 0;
      ImGui::SetDragDropPayload("OGLE_PREFAB", &dummy, sizeof(dummy));
      ImGui::TextUnformatted(o.name.c_str());
      ImGui::EndDragDropSource();
    }
    ImGui::TextUnformatted(fit_text(o.name, size + 6).c_str());
  });
  ImGui::EndChild();
}

void App::ui_catalog_models() {
  ImGui::SetNextItemWidth(-1);
  ImGui::InputTextWithHint("##models-filter", tr("catalog.filter"), &m_catalog_filter);
  const std::string f = lower(m_catalog_filter);
  std::vector<const CatalogModel*> shown;
  for (const auto& m : catalog_models)
    if (f.empty() || m.name.find(f) != std::string::npos || m.group.find(f) != std::string::npos) shown.push_back(&m);
  ImGui::TextDisabled(tr("catalog.count"), shown.size(), catalog_models.size());
  ImGui::BeginChild("models");
  const float size = 80.f;
  thumbnail_grid((int)shown.size(), size, [&](int i) {
    const CatalogModel& m = *shown[i];
    ThumbRequest req;
    req.kind = ThumbRequest::Kind::Model;
    req.path = m.path;
    unsigned tex = thumbs.get(req);
    const bool placing = tool == ToolMode::Place && placement.kind == Placement::Kind::Model && placement.path == m.path;
    if (placing) ImGui::PushStyleColor(ImGuiCol_Button, ImGui::GetStyleColorVec4(ImGuiCol_ButtonActive));
    ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(3, 3));
    const bool clicked = tex ? ImGui::ImageButton("##img", (ImTextureID)(uint64_t)tex, ImVec2(size, size))
                             : ImGui::Button(thumbs.failed(req) ? "?" : "...", ImVec2(size + 6, size + 6));
    ImGui::PopStyleVar();
    if (placing) ImGui::PopStyleColor();
    if (ImGui::BeginItemTooltip()) {
      ImGui::TextUnformatted(m.name.c_str());
      ImGui::TextDisabled(tr("catalog.from"), m.group.c_str());
      ImGui::TextDisabled("%s", tr("catalog.model_tip"));
      ImGui::TextDisabled("%s", tr("catalog.place_tip"));
      ImGui::EndTooltip();
    }
    if (clicked) start_model_placement(m, false);
    if (ImGui::BeginDragDropSource()) {
      if (!m_drag_from_panel) {
        m_drag_from_panel = true;
        start_model_placement(m, true);
      }
      int dummy = 0;
      ImGui::SetDragDropPayload("OGLE_PREFAB", &dummy, sizeof(dummy));
      ImGui::TextUnformatted(m.name.c_str());
      ImGui::EndDragDropSource();
    }
    ImGui::TextUnformatted(fit_text(m.name, size + 6).c_str());
  });
  ImGui::EndChild();
}

void App::ui_decor_cell(const std::string& background, int64_t stamp, const CatalogPart& p, const std::string& from,
                        int levels, float size) {
  ThumbRequest req;
  req.kind = ThumbRequest::Kind::Part;
  req.path = background;
  req.node = p.mesh_node;
  req.stamp = stamp;
  unsigned tex = thumbs.get(req);
  const bool placing = tool == ToolMode::Place && placement.kind == Placement::Kind::Part &&
                       placement.path == background && placement.part.mesh_node == p.mesh_node;
  if (placing) ImGui::PushStyleColor(ImGuiCol_Button, ImGui::GetStyleColorVec4(ImGuiCol_ButtonActive));
  ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(3, 3));
  const bool clicked = tex ? ImGui::ImageButton("##img", (ImTextureID)(uint64_t)tex, ImVec2(size, size))
                           : ImGui::Button(thumbs.failed(req) ? "?" : "...", ImVec2(size + 6, size + 6));
  ImGui::PopStyleVar();
  if (placing) ImGui::PopStyleColor();
  if (ImGui::BeginItemTooltip()) {
    ImGui::TextUnformatted(p.name.c_str());
    if (p.kind == CatalogPart::Kind::Prototype) ImGui::TextDisabled(tr("catalog.prototype"), p.copies);
    else ImGui::TextDisabled("%s", tr("catalog.piece"));
    if (!from.empty()) ImGui::TextDisabled(tr("catalog.from"), from.c_str());
    if (levels > 1) ImGui::TextDisabled(tr("catalog.in_levels"), levels);
    ImGui::TextDisabled(tr("inspector.triangles"), p.triangles);
    ImGui::TextDisabled("%s", tr("catalog.place_tip"));
    ImGui::EndTooltip();
  }
  if (clicked) start_part_placement(background, p, false);
  if (ImGui::BeginDragDropSource()) {
    if (!m_drag_from_panel) {
      m_drag_from_panel = true;
      start_part_placement(background, p, true);
    }
    int dummy = 0;
    ImGui::SetDragDropPayload("OGLE_PREFAB", &dummy, sizeof(dummy));
    ImGui::TextUnformatted(p.name.c_str());
    ImGui::EndDragDropSource();
  }
  ImGui::TextUnformatted(fit_text(p.name, size + 6).c_str());
}

void App::ui_catalog_decor() {
  // the decor of Jak 3's levels, or Jak II's structures when Jak II was extracted too
  if (!library_jak2.empty()) {
    const bool before = m_decor_jak2;
    if (ImGui::RadioButton(tr_id("catalog.decor.jak3").c_str(), !m_decor_jak2)) m_decor_jak2 = false;
    ImGui::SameLine();
    if (ImGui::RadioButton(tr_id("catalog.decor.jak2").c_str(), m_decor_jak2)) m_decor_jak2 = true;
    guide_tip("jak2");
    if (m_decor_jak2 != before) {
      m_decor_level.clear();
      m_decor_scene.reset();
      m_decor_parts.clear();
      m_all_parts_filter = "\x01";  // the search made again
    }
  } else {
    m_decor_jak2 = false;
  }
  const AssetLibrary& lib = m_decor_jak2 ? library_jak2 : library;
  // the level whose decor is shown, or every level
  ImGui::SetNextItemWidth(-1);
  if (ImGui::BeginCombo("##decor-level", m_decor_level.empty() ? tr("catalog.all_levels") : m_decor_level.c_str(),
                        ImGuiComboFlags_HeightLarge)) {
    if (ImGui::Selectable(tr("catalog.all_levels"), m_decor_level.empty()) && !m_decor_level.empty()) {
      m_decor_level.clear();
      m_decor_scene.reset();
      m_decor_parts.clear();
    }
    for (const auto& l : lib.levels()) {
      if (!l.listed) continue;
      if (ImGui::Selectable(l.name.c_str(), l.name == m_decor_level) && l.name != m_decor_level) {
        m_decor_level = l.name;
        m_decor_scene.reset();
        m_decor_parts.clear();
      }
    }
    ImGui::EndCombo();
  }
  const LevelEntry* level = lib.level(m_decor_level);
  if (!level) {
    m_decor_level.clear();
    ui_catalog_decor_all();
    return;
  }
  if (!m_decor_scene) {
    m_decor_scene = store.level(level->background, true);
    if (m_decor_scene) m_decor_parts = list_parts(*m_decor_scene);
    if (m_decor_jak2)
      m_decor_parts.erase(std::remove_if(m_decor_parts.begin(), m_decor_parts.end(),
                                         [](const CatalogPart& p) { return !jak2_structure(p); }),
                          m_decor_parts.end());
  }
  if (!m_decor_scene) {
    if (store.failed_level(level->background)) ImGui::TextWrapped("%s", tr("catalog.decor.failed"));
    else ImGui::TextDisabled(tr("catalog.decor.loading"), level->name.c_str());
    return;
  }
  ImGui::SetNextItemWidth(-1);
  ImGui::InputTextWithHint("##decor-filter", tr("catalog.filter"), &m_catalog_filter);
  const std::string f = lower(m_catalog_filter);
  std::vector<const CatalogPart*> shown;
  for (const auto& p : m_decor_parts)
    if (f.empty() || lower(p.name).find(f) != std::string::npos) shown.push_back(&p);
  ImGui::TextDisabled(tr("catalog.parts_count"), shown.size(), m_decor_parts.size());
  if (m_decor_parts.empty()) {
    ImGui::TextWrapped("%s", tr("catalog.decor.none"));
    return;
  }
  ImGui::BeginChild("parts");
  const float size = 80.f;
  thumbnail_grid((int)shown.size(), size,
                 [&](int i) { ui_decor_cell(level->background, level->mtime, *shown[i], std::string(), 1, size); });
  ImGui::EndChild();
}

void App::ui_catalog_decor_all() {
  const LibraryIndex& index = m_decor_jak2 ? library_index_jak2 : library_index;
  const std::vector<DecorEntry>& parts = m_decor_jak2 ? all_parts_jak2 : all_parts;
  // the levels are read once (then from the cache)
  if (!index.ready()) {
    const int done = index.done(), total = index.total();
    ImGui::TextDisabled(tr("catalog.indexing"), done, total);
    ImGui::ProgressBar(total > 0 ? (float)done / total : 0.f, ImVec2(-1, 0), "");
    return;
  }
  ImGui::SetNextItemWidth(-1);
  ImGui::InputTextWithHint("##decor-all-filter", tr("catalog.decor.filter"), &m_catalog_filter);
  const std::string f = lower(m_catalog_filter);
  if (f.empty()) {
    if (m_decor_jak2) {
      ImGui::TextWrapped(tr("catalog.decor.jak2.search"), parts.size(), library_jak2.levels().size());
    } else {
      ImGui::TextWrapped(tr("catalog.decor.search"), m_all_prototypes, m_all_pieces);
    }
    return;
  }
  if (f != m_all_parts_filter) {
    m_all_parts_filter = f;
    m_all_parts_shown.clear();
    for (size_t i = 0; i < parts.size(); i++)
      if (parts[i].key.find(f) != std::string::npos) m_all_parts_shown.push_back(i);
  }
  ImGui::TextDisabled(tr("catalog.parts_count"), m_all_parts_shown.size(), parts.size());
  ImGui::BeginChild("all-parts");
  const float size = 80.f;
  thumbnail_grid((int)m_all_parts_shown.size(), size, [&](int i) {
    const DecorEntry& e = parts[m_all_parts_shown[i]];
    ui_decor_cell(e.background, e.stamp, e.part, e.level, e.levels, size);
  });
  ImGui::EndChild();
}

// ------------------------------------------------------------------------------------------------
// prefabs
// ------------------------------------------------------------------------------------------------

void App::ui_prefabs_panel() {
  if (prefab_folder().empty()) {
    ImGui::Spacing();
    ImGui::TextWrapped("%s", tr("prefabs.no_library"));
    return;
  }
  const bool sel = !selection.empty();
  ImGui::BeginChild("prefabs");
  const float size = 96.f;
  const float cell_w = size + 6 + ImGui::GetStyle().ItemSpacing.x;
  const int per_row = std::max(1, (int)((ImGui::GetContentRegionAvail().x + ImGui::GetStyle().ItemSpacing.x) / cell_w));
  std::string to_delete;

  // the game's: ready-made assemblies of its decor, by category, made from the extracted levels
  if (ImGui::CollapsingHeader(tr_id("prefabs.game").c_str(), ImGuiTreeNodeFlags_DefaultOpen)) {
    guide_tip("game_prefabs");
    // made the first time they are shown (a few seconds: each level read once)
    static bool asked = false;
    if (!asked && !library.empty() && game_prefabs.size() < prefab_recipes.size()) {
      asked = true;
      start_game_prefabs();
    }
    if (game_prefabs_busy()) {
      ImGui::ProgressBar(m_game_prefab_total ? (float)*m_game_prefab_done / m_game_prefab_total : 0.f, ImVec2(-1, 0),
                         strf(tr("prefabs.game.making"), (int)*m_game_prefab_done, m_game_prefab_total).c_str());
    } else if (game_prefabs.size() < prefab_recipes.size()) {
      if (ImGui::Button(tr_id("prefabs.game.make").c_str(), ImVec2(-1, 0))) start_game_prefabs();
      tooltip(tr("prefabs.game.make.tip"));
    }
    static std::string category;
    std::vector<std::string> categories;
    for (const auto& p : game_prefabs)
      if (std::find(categories.begin(), categories.end(), p.category) == categories.end()) categories.push_back(p.category);
    if (!categories.empty()) {
      ImGui::SetNextItemWidth(-1);
      const std::string all = tr("prefabs.game.all");
      if (ImGui::BeginCombo("##prefab-category",
                            category.empty() ? all.c_str() : tr(("prefabs.category." + category).c_str()))) {
        if (ImGui::Selectable(all.c_str(), category.empty())) category.clear();
        for (const auto& c : categories)
          if (ImGui::Selectable(tr(("prefabs.category." + c).c_str()), category == c)) category = c;
        ImGui::EndCombo();
      }
    }
    size_t shown = 0;
    for (const PrefabInfo& p : game_prefabs) {
      if (!category.empty() && p.category != category) continue;
      ImGui::PushID(p.path.c_str());
      if (shown++ % per_row) ImGui::SameLine();
      ImGui::BeginGroup();
      ThumbRequest req;
      req.kind = ThumbRequest::Kind::Model;
      req.path = p.path;
      req.stamp = p.mtime;
      unsigned tex = thumbs.get(req);
      const bool placing = tool == ToolMode::Place && placement.path == p.path;
      if (placing) ImGui::PushStyleColor(ImGuiCol_Button, ImGui::GetStyleColorVec4(ImGuiCol_ButtonActive));
      ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(3, 3));
      const bool clicked = tex ? ImGui::ImageButton("##img", (ImTextureID)(uint64_t)tex, ImVec2(size, size))
                               : ImGui::Button(thumbs.failed(req) ? "?" : "...", ImVec2(size + 6, size + 6));
      ImGui::PopStyleVar();
      if (placing) ImGui::PopStyleColor();
      if (ImGui::BeginItemTooltip()) {
        ImGui::TextUnformatted(p.name.c_str());
        ImGui::TextDisabled("%s", tr(("prefabs.category." + p.category).c_str()));
        ImGui::PushTextWrapPos(ImGui::GetFontSize() * 24.f);
        if (!p.description.empty()) ImGui::TextUnformatted(p.description.c_str());
        ImGui::TextDisabled("%s", tr("prefabs.cell_tip"));
        ImGui::PopTextWrapPos();
        ImGui::EndTooltip();
      }
      if (clicked) start_placement(p.path, p.name, false);
      if (ImGui::BeginDragDropSource()) {
        if (!m_drag_from_panel) {
          m_drag_from_panel = true;
          start_placement(p.path, p.name, true);
        }
        int dummy = 0;
        ImGui::SetDragDropPayload("OGLE_PREFAB", &dummy, sizeof(dummy));
        ImGui::TextUnformatted(p.name.c_str());
        ImGui::EndDragDropSource();
      }
      ImGui::TextUnformatted(fit_text(p.name, size + 6).c_str());
      ImGui::EndGroup();
      ImGui::PopID();
    }
  }

  // the user's own
  const bool mine_open = ImGui::CollapsingHeader(tr_id("prefabs.mine").c_str(), ImGuiTreeNodeFlags_DefaultOpen);
  guide_tip("prefab");
  if (!mine_open) {
    ImGui::EndChild();
    return;
  }
  ImGui::BeginDisabled(!sel);
  if (ImGui::Button(tr_id("prefabs.create").c_str(), ImVec2(-1, 34))) m_show_new_prefab = true;
  ImGui::EndDisabled();
  tooltip(tr("prefabs.create.tip"));
  if (prefabs.empty()) ImGui::TextWrapped("%s", tr("prefabs.empty"));
  for (size_t i = 0; i < prefabs.size(); i++) {
    const PrefabInfo& p = prefabs[i];
    ImGui::PushID(p.path.c_str());
    if (i % per_row) ImGui::SameLine();
    ImGui::BeginGroup();
    ThumbRequest req;
    req.kind = ThumbRequest::Kind::Model;
    req.path = p.path;
    req.stamp = p.mtime;
    unsigned tex = thumbs.get(req);
    const bool placing = tool == ToolMode::Place && placement.path == p.path;
    if (placing) ImGui::PushStyleColor(ImGuiCol_Button, ImGui::GetStyleColorVec4(ImGuiCol_ButtonActive));
    ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(3, 3));
    bool clicked = tex ? ImGui::ImageButton("##img", (ImTextureID)(uint64_t)tex, ImVec2(size, size))
                       : ImGui::Button(thumbs.failed(req) ? "?" : "...", ImVec2(size + 6, size + 6));
    ImGui::PopStyleVar();
    if (placing) ImGui::PopStyleColor();
    tooltip((p.name + "\n" + tr("prefabs.cell_tip")).c_str());
    if (clicked) start_placement(p.path, p.name, false);
    if (ImGui::BeginDragDropSource()) {
      if (!m_drag_from_panel) {
        m_drag_from_panel = true;
        start_placement(p.path, p.name, true);
      }
      int dummy = 0;
      ImGui::SetDragDropPayload("OGLE_PREFAB", &dummy, sizeof(dummy));
      ImGui::TextUnformatted(p.name.c_str());
      ImGui::EndDragDropSource();
    }
    if (ImGui::BeginPopupContextItem("ctx")) {
      ImGui::TextDisabled("%s", p.name.c_str());
      if (ImGui::MenuItem(tr("prefabs.place"))) start_placement(p.path, p.name, false);
      if (ImGui::MenuItem(tr("prefabs.delete"))) to_delete = p.path;
      ImGui::EndPopup();
    }
    ImGui::TextUnformatted(fit_text(p.name, size + 6).c_str());
    ImGui::EndGroup();
    ImGui::PopID();
  }
  ImGui::EndChild();
  if (!to_delete.empty()) {
    if (placement.path == to_delete) stop_placement();
    std::error_code ec;
    fs::remove(to_path(to_delete), ec);
    store.forget_model(to_delete);
    m_prefab_scenes.erase(to_delete);
    refresh_prefabs();
    LOG_INFO(tr("log.prefab_deleted"), to_path(to_delete).stem().string().c_str());
  }
}

// ------------------------------------------------------------------------------------------------
// inspector
// ------------------------------------------------------------------------------------------------

void App::ui_inspector(float x, float y, float w, float h) {
  ImGui::SetNextWindowPos(ImVec2(x, y));
  ImGui::SetNextWindowSize(ImVec2(w, h), ImGuiCond_FirstUseEver);
  ImGui::SetNextWindowSizeConstraints(ImVec2(260, h), ImVec2(ImGui::GetIO().DisplaySize.x * 0.4f, h));
  ImGui::Begin(tr_id("inspector.title").c_str(), nullptr,
               ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoCollapse | ImGuiWindowFlags_NoBringToFrontOnFocus);
  m_right_w = ImGui::GetWindowWidth();
  if (tool == ToolMode::Place) {
    ImGui::SeparatorText(tr("inspector.placing"));
    ImGui::TextWrapped("%s", placement.name.c_str());
    ImGui::TextWrapped("%s", tr("inspector.placing.help"));
    if (ImGui::Button(tr_id("inspector.placing.stop").c_str())) stop_placement();
    ImGui::Spacing();
  }
  Node* n = scene.find(primary);
  if (!n || selection.empty()) {
    ImGui::SeparatorText(tr("inspector.level"));
    if (!document_open()) {
      ImGui::TextWrapped("%s", tr("inspector.no_level"));
    } else {
      ImGui::TextUnformatted(scene.settings.level.empty() ? tr("title.untitled") : scene.settings.level.c_str());
      // counted again only when the document changes (a city is 150 000 nodes)
      static uint64_t counted_rev = ~0ull;
      static size_t counted_nodes = ~(size_t)0;
      static size_t objects = 0, actors = 0;
      static std::vector<uint32_t> companions, layers;
      static std::vector<DocumentLevel> levels;
      static std::vector<std::string> neighbors;
      if (counted_rev != history.revision() || counted_nodes != scene.nodes.size()) {
        counted_rev = history.revision();
        counted_nodes = scene.nodes.size();
        const SceneCache& cache = scene.cache();
        objects = actors = 0;
        companions.clear();
        layers.clear();
        levels = document_levels();
        neighbors = document_neighbors();
        for (size_t i = 0; i < scene.nodes.size(); i++) {
          const Node& node = scene.nodes[i];
          if (node.kind == NodeKind::Render && !cache.actor[i]) objects++;
          if (node.kind == NodeKind::Actor) actors++;
          if (node.extras.contains("og_companion")) companions.push_back(node.id);
          if (node.extras.contains("og_layer")) layers.push_back(node.id);
        }
      }
      ImGui::Text(tr("inspector.counts"), objects, actors);
      // its levels: the one opened, the ones added (shown or not, framed, selected whole to move
      // them, removed)
      {
        bool any_added = false;
        for (const auto& l : levels) any_added |= l.role == DocumentLevel::Added;
        ImGui::SeparatorText(tr(any_added ? "inspector.added_levels" : "inspector.levels_title"));
        // the next button on the same line when it fits there, else on the next line
        const float edge = ImGui::GetCursorScreenPos().x + ImGui::GetContentRegionAvail().x;
        auto next = [edge](const std::string& label) {
          const ImGuiStyle& st = ImGui::GetStyle();
          const float w = ImGui::CalcTextSize(label.c_str(), nullptr, true).x + st.FramePadding.x * 2;
          if (ImGui::GetItemRectMax().x + st.ItemSpacing.x + w <= edge) ImGui::SameLine();
        };
        if (ImGui::SmallButton((scene.settings.level + "##frame-main").c_str())) {
          const AABB b = document_level_bounds(0);
          if (b.valid()) camera.frame(b);
        }
        tooltip(tr("inspector.added_levels.main.tip"));
        next(tr_id("inspector.added_levels.select"));
        if (ImGui::SmallButton(tr_id("inspector.added_levels.select").c_str())) select_level(0);
        tooltip(tr("inspector.added_levels.select.tip"));
        for (const auto& l : levels) {
          if (l.role != DocumentLevel::Added || l.reference) continue;
          Node* g = scene.find(l.group);
          if (!g) continue;
          ImGui::PushID((int)l.group);
          const Node before = *g;
          bool v = g->visible;
          if (ImGui::Checkbox("##shown", &v)) {
            g->visible = v;
            commit_node_edit(before, tr("undo.visibility"));
          }
          tooltip(tr("inspector.added_levels.shown.tip"));
          ImGui::SameLine();
          if (ImGui::SmallButton(l.name.c_str())) {
            const AABB b = document_level_bounds(l.group);
            if (b.valid()) camera.frame(b);
          }
          tooltip(tr("inspector.added_levels.frame.tip"));
          next(tr_id("inspector.added_levels.junction"));
          if (ImGui::SmallButton(tr_id("inspector.added_levels.junction").c_str())) {
            const AABB j = document_junction(0, l.group);
            if (j.valid()) {
              camera.pitch = -0.6f;
              camera.frame(j);
            } else {
              LOG_WARN(tr("log.no_junction"), scene.settings.level.c_str(), l.name.c_str());
            }
          }
          tooltip(tr("inspector.added_levels.junction.tip"));
          next(tr_id("inspector.added_levels.select"));
          if (ImGui::SmallButton(tr_id("inspector.added_levels.select").c_str())) select_level(l.group);
          tooltip(tr("inspector.added_levels.select.tip"));
          next(tr_id("inspector.added_levels.remove"));
          if (ImGui::SmallButton(tr_id("inspector.added_levels.remove").c_str())) {
            std::string err;
            if (!remove_document_level(l.group, &err)) LOG_WARN("%s", err.c_str());
          }
          ImGui::PopID();
        }
      }
      ui_replace_section(levels);
      // the levels around it in the game, to add
      if (!neighbors.empty()) {
        ImGui::SeparatorText(tr("inspector.neighbors"));
        ImGui::BeginDisabled(level_adding() || m_level_future.valid());
        float x = 0;
        const float avail = ImGui::GetContentRegionAvail().x;
        for (const auto& name : neighbors) {
          const float w = ImGui::CalcTextSize(name.c_str()).x + ImGui::GetStyle().FramePadding.x * 2;
          if (x > 0 && x + w < avail) ImGui::SameLine();
          else x = 0;
          x += w + ImGui::GetStyle().ItemSpacing.x;
          if (ImGui::SmallButton((name + "##neighbor").c_str()))
            if (const LevelEntry* e = library.level(name)) {
              std::string err;
              if (!add_level_to_document(*e, &err)) LOG_WARN("%s", err.c_str());
            }
          tooltip(tr("inspector.neighbors.tip"));
        }
        ImGui::EndDisabled();
        if (level_adding()) ImGui::TextDisabled(tr("inspector.neighbors.adding"), m_adding_level.c_str());
      }
      // the levels shown with it, the layers of the missions
      if (!companions.empty()) {
        ImGui::SeparatorText(tr("inspector.companions"));
        for (auto id : companions) {
          Node* c = scene.find(id);
          if (!c) continue;
          const Node before = *c;
          bool v = c->visible;
          if (ImGui::Checkbox((c->name + "##companion").c_str(), &v)) {
            c->visible = v;
            commit_node_edit(before, tr("undo.visibility"));
          }
          tooltip(tr("inspector.companions.tip"));
        }
      }
      if (!layers.empty()) {
        size_t shown = 0;
        for (auto id : layers)
          if (const Node* l = scene.find(id)) shown += l->visible && !l->story_hidden;
        ImGui::Spacing();
        ImGui::PushStyleColor(ImGuiCol_Text, ImGui::GetStyleColorVec4(ImGuiCol_TextDisabled));
        ImGui::TextWrapped(tr("inspector.layers"), layers.size(), shown);
        ImGui::PopStyleColor();
        tooltip(tr("inspector.layers.tip"));
      }
      ImGui::Spacing();
      ImGui::TextWrapped("%s", tr("inspector.hint"));
    }
    ImGui::End();
    return;
  }
  if (selection.size() > 1) {
    ImGui::TextColored(ImVec4(1, 0.7f, 0.3f, 1), tr("inspector.selected_count"), selection.size());
  }
  ui_node_inspector(*n);
  ImGui::End();
}

// Replacing a level of the game with the document's: which one, the original shown to compare
// with, the document's level placed and turned where it goes, where the game starts the player.
void App::ui_replace_section(const std::vector<DocumentLevel>& levels) {
  const GameData* data = document_game_data();
  if (!data) return;
  ImGui::SeparatorText(tr("replace.title"));
  if (m_scroll_to_replace) {  // Level > Replace a level of the game
    ImGui::SetScrollHereY(0.f);
    m_scroll_to_replace = false;
  }
  const std::string& current = scene.settings.replaces;
  const std::string source = scene.settings.level;
  auto set_replaced = [this](const std::string& level) {
    const std::string before = scene.settings.replaces;
    if (before == level) return;
    scene.settings.replaces = level;
    LevelSettings* s = &scene.settings;
    history.push({tr("undo.replaces"), [s, before]() { s->replaces = before; }, [s, level]() { s->replaces = level; }});
    m_export_level.clear();  // the export's options are made again for it
  };
  ImGui::SetNextItemWidth(-1);
  if (ImGui::BeginCombo("##replaces", current.empty() ? tr("replace.none_chosen") : current.c_str(),
                        ImGuiComboFlags_HeightLarge)) {
    if (ImGui::Selectable(tr("replace.none_chosen"), current.empty())) set_replaced("");
    // the level the document comes from first, then the game's levels the Levels panel lists
    const LevelInfo* own = data->level(source);
    if (own && !own->dgo.empty() && !own->layer &&
        ImGui::Selectable(strf(tr("replace.source"), source.c_str()).c_str(), current == source))
      set_replaced(source);
    for (const auto& l : library.levels()) {
      const LevelInfo* info = data->level(l.name);
      if (!l.listed || !info || info->dgo.empty() || l.name == source) continue;
      if (ImGui::Selectable(l.name.c_str(), current == l.name)) set_replaced(l.name);
    }
    ImGui::EndCombo();
  }
  guide_tip("replace");
  if (current.empty()) return;
  // the original, as the game has it, shown to compare with
  const DocumentLevel* original = nullptr;
  for (const auto& l : levels)
    if (l.reference && l.name == current) original = &l;
  ImGui::BeginDisabled(level_adding() || m_level_future.valid());
  if (!original) {
    if (ImGui::Button(tr_id("replace.show").c_str(), ImVec2(-1, 0)))
      if (const LevelEntry* e = library.level(current)) {
        std::string err;
        if (!add_level_to_document(*e, &err, true)) LOG_WARN("%s", err.c_str());
      }
    tooltip(tr("replace.show.tip"));
  } else if (Node* g = scene.find(original->group)) {
    const Node before = *g;
    bool shown = g->visible;
    if (ImGui::Checkbox(tr_id("replace.shown").c_str(), &shown)) {
      g->visible = shown;
      commit_node_edit(before, tr("undo.visibility"));
    }
    tooltip(tr("replace.shown.tip"));
    ImGui::SameLine();
    if (ImGui::SmallButton(tr_id("replace.remove").c_str())) {
      std::string err;
      if (!remove_document_level(original->group, &err)) LOG_WARN("%s", err.c_str());
    }
  }
  ImGui::EndDisabled();
  if (level_adding() && m_adding_reference) ImGui::TextDisabled(tr("inspector.neighbors.adding"), m_adding_level.c_str());
  // the document's own level, where the original is
  ImGui::BeginDisabled(!original);
  if (ImGui::Button(tr_id("replace.align").c_str(), ImVec2(-1, 0))) {
    std::string err;
    if (!align_to_replaced(&err)) LOG_WARN("%s", err.c_str());
  }
  ImGui::EndDisabled();
  tooltip(strf(tr(original ? "replace.align.tip" : "replace.align.disabled"), current.c_str()).c_str());
  if (ImGui::SmallButton(tr_id("replace.turn_left").c_str())) turn_own_level(90.f);
  tooltip(tr("replace.turn.tip"));
  ImGui::SameLine();
  if (ImGui::SmallButton(tr_id("replace.turn_right").c_str())) turn_own_level(-90.f);
  tooltip(tr("replace.turn.tip"));
  ImGui::SameLine();
  // the same text as the levels' Select above, another id (one id per button of the window)
  if (ImGui::SmallButton((std::string(tr("inspector.added_levels.select")) + "###replace-select").c_str()))
    select_level(0);
  tooltip(tr("replace.select.tip"));
  // where the game starts the player there: on the ground of the new level? (checked again when the
  // document changes)
  static uint64_t checked_rev = ~0ull;
  static std::string checked_level;
  static std::vector<ContinueCheck> checks;
  if (checked_rev != history.revision() || checked_level != current) {
    checked_rev = history.revision();
    checked_level = current;
    checks = check_continues(scene, data, current);
  }
  if (!checks.empty()) {
    ImGui::TextDisabled(tr("replace.continues"), current.c_str());
    tooltip(tr("replace.continues.tip"));
    for (const auto& c : checks) {
      ImGui::PushID(c.name.c_str());
      if (ImGui::SmallButton(tr_id("replace.go").c_str())) {
        camera.pos = c.position + Vec3{0, 6.f, 9.f};
        camera.look_at(c.position);
      }
      ImGui::SameLine();
      if (c.ok()) ImGui::TextColored(ImVec4(0.45f, 0.85f, 0.5f, 1.f), "%s", c.name.c_str());
      else ImGui::TextColored(ImVec4(1.f, 0.6f, 0.3f, 1.f), "%s", c.name.c_str());
      tooltip(c.ok() ? strf(tr("replace.continue.ok"), c.position.y - c.ground).c_str() : tr("replace.continue.none"));
      ImGui::PopID();
    }
  }
  if (ImGui::Button(strf(tr("replace.export"), current.c_str()).c_str(), ImVec2(-1, 0))) open_export_window();
  tooltip(tr("replace.export.tip"));
}

void App::ui_node_inspector(Node& n) {
  ImGui::SeparatorText(kind_label(n.kind));
  ImGui::TextWrapped("%s", n.name.c_str());
  if (n.kind == NodeKind::Actor) {
    ImGui::Text(tr("inspector.type"), n.extras.value("etype", std::string("?")).c_str());
    for (auto c : scene.children(n.id))
      if (const Node* cn = scene.find(c); cn && cn->extras.contains("og_actor_visual"))
        ImGui::TextDisabled(tr("inspector.model"), cn->name.c_str());
  }
  // where it is
  {
    const Node snap = n;  // before this frame's edits, for undo
    Vec3 t = n.local.t;
    Vec3 e = n.local.r.to_euler_deg();
    ImGui::PushItemWidth(-90);
    if (ImGui::DragFloat3(tr_id("inspector.position").c_str(), &t.x, 0.05f, 0, 0, "%.2f")) n.local.t = t;
    edit_begin(snap);
    edit_end(tr("undo.move"));
    if (ImGui::DragFloat3(tr_id("inspector.rotation").c_str(), &e.x, 0.5f, 0, 0, "%.0f deg"))
      n.local.r = Quat::from_euler_deg(e);
    edit_begin(snap);
    edit_end(tr("undo.rotate"));
    Vec3 sc = n.local.s;
    if (ImGui::DragFloat3(tr_id("inspector.scale").c_str(), &sc.x, 0.01f, 0.01f, 100.f, "%.2f")) {
      if (m_scale_uniform) {
        // the value changed gives the ratio of the three
        const float before[3] = {n.local.s.x, n.local.s.y, n.local.s.z}, after[3] = {sc.x, sc.y, sc.z};
        for (int i = 0; i < 3; i++)
          if (after[i] != before[i] && before[i] > 0) {
            const float k = after[i] / before[i];
            sc = {before[0] * k, before[1] * k, before[2] * k};
            break;
          }
      }
      n.local.s = {std::max(sc.x, 0.01f), std::max(sc.y, 0.01f), std::max(sc.z, 0.01f)};
    }
    edit_begin(snap);
    edit_end(tr("undo.scale"));
    ImGui::PopItemWidth();
    ImGui::Checkbox(tr_id("inspector.scale.uniform").c_str(), &m_scale_uniform);
    tooltip(tr("inspector.scale.uniform.tip"));
    if (n.kind == NodeKind::Render) {
      // locked: clicks go through it to what is on it (the ground is, from the start)
      ImGui::SameLine();
      bool locked = n.locked;
      if (ImGui::Checkbox(tr_id("inspector.locked").c_str(), &locked)) {
        n.locked = locked;
        commit_node_edit(snap, tr("undo.lock"));
      }
      tooltip(tr("inspector.locked.tip"));
    }
    if (n.kind == NodeKind::Actor && (n.local.s.x != 1.f || n.local.s.y != 1.f || n.local.s.z != 1.f)) {
      ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(1, 0.75f, 0.35f, 1));
      ImGui::TextWrapped("%s", tr("inspector.scale.actor"));
      ImGui::PopStyleColor();
    }
  }
  if (n.kind == NodeKind::NavMesh) ui_navmesh_inspector(n);
  if (n.kind == NodeKind::Actor) {
    ui_actor_behavior(n);
    ui_actor_params(n);
  }
  ui_shape_inspector(n);
  ui_import_inspector(n);
  ui_material_inspector(n);
  ui_decor_behavior(n);
  ImGui::Spacing();
  if (ImGui::Button(tr_id("inspector.copy").c_str())) copy_selection();
  ImGui::SameLine();
  if (ImGui::Button(tr_id("inspector.duplicate").c_str())) duplicate_selection();
  ImGui::SameLine();
  if (ImGui::Button(tr_id("inspector.delete").c_str())) {
    delete_selection();
    return;
  }
  if (ImGui::Button(tr_id("inspector.create_prefab").c_str(), ImVec2(-1, 0))) m_show_new_prefab = true;
  if (n.kind == NodeKind::Render && n.mesh) {
    ImGui::SeparatorText(tr("inspector.mesh"));
    ImGui::TextWrapped("%s", n.mesh->name.c_str());
    ImGui::TextDisabled(tr("inspector.triangles"), n.mesh->tri_count());
    size_t users = 0;
    for (const auto& o : scene.nodes)
      if (o.mesh == n.mesh) users++;
    if (users > 1) ImGui::TextDisabled(tr("inspector.copies"), users);
  }
}

// ------------------------------------------------------------------------------------------------
// status bar, viewport texts
// ------------------------------------------------------------------------------------------------

void App::ui_statusbar(float y, float h) {
  ImGuiIO& io = ImGui::GetIO();
  ImGui::SetNextWindowPos(ImVec2(0, y));
  ImGui::SetNextWindowSize(ImVec2(io.DisplaySize.x, h));
  ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(8, 4));
  ImGui::Begin("##status", nullptr,
               ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoMove |
                   ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoSavedSettings |
                   ImGuiWindowFlags_NoBringToFrontOnFocus);
  ImGui::TextUnformatted(tr(tool == ToolMode::Place ? "status.place" : "status.select"));
  ImGui::SameLine();
  if (history.revision() != saved_revision && document_open()) {
    ImGui::TextColored(ImVec4(1, 0.75f, 0.3f, 1), "|  %s", tr("status.unsaved"));
    ImGui::SameLine();
  }
  if (m_level_future.valid() || store.pending()) {
    ImGui::TextColored(ImVec4(0.5f, 0.85f, 1, 1), "|  %s", tr("status.loading"));
    ImGui::SameLine();
  }
  if (m_remote.clients() > 0) {
    ImGui::TextColored(ImVec4(0.75f, 0.6f, 1, 1), "|  %s", tr("status.remote"));
    if (ImGui::IsItemHovered()) ImGui::SetTooltip("%s", tr("status.remote.tip"));
    ImGui::SameLine();
  }
  auto lines = Log::get().lines();
  if (!lines.empty()) {
    const auto& last = lines.back();
    ImVec4 col = last.level == LogLevel::Error  ? ImVec4(1, 0.45f, 0.4f, 1)
                 : last.level == LogLevel::Warn ? ImVec4(1, 0.8f, 0.4f, 1)
                                                : ImVec4(0.6f, 0.65f, 0.7f, 1);
    ImGui::TextColored(col, "|  %s", last.text.c_str());
    if (ImGui::IsItemClicked()) show_log = true;
  }
  ImGui::End();
  ImGui::PopStyleVar();
}

// The story bar, at the bottom of the viewport: the point of the game the level is shown at.
void App::ui_story_bar() {
  const GameData* data = document_game_data();
  if (!data || !data->has_story() || scene.nodes.empty() || m_vp_w < 200.f) return;
  const ImGuiStyle& style = ImGui::GetStyle();
  const float pad = 8.f;
  const float h = ImGui::GetFrameHeight() + style.WindowPadding.y * 2.f;
  ImGui::SetNextWindowPos(ImVec2(m_vp_x + pad, m_vp_y + m_vp_h - h - pad));
  ImGui::SetNextWindowSize(ImVec2(m_vp_w - 2 * pad, h));
  ImGui::SetNextWindowBgAlpha(0.88f);
  ImGui::Begin("##story", nullptr,
               ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoMove |
                   ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoSavedSettings |
                   ImGuiWindowFlags_NoFocusOnAppearing);
  StoryView v = story;
  bool changed = ImGui::Checkbox(tr_id("story.enabled").c_str(), &v.enabled);
  tooltip(tr("story.enabled.tip"));
  // the level shown: its own point of the story, when the document has several
  if (!level_story_steps.empty()) {
    ImGui::SameLine();
    ImGui::SetNextItemWidth(std::min(150.f, ImGui::GetContentRegionAvail().x * 0.2f));
    const std::string& main = scene.settings.level;
    if (ImGui::BeginCombo("##story-level", story_level.empty() ? main.c_str() : story_level.c_str())) {
      if (ImGui::Selectable(main.c_str(), story_level.empty())) story_level.clear();
      for (const auto& [name, steps] : level_story_steps)
        if (ImGui::Selectable(name.c_str(), story_level == name)) story_level = name;
      ImGui::EndCombo();
    }
    tooltip(tr("story.level.tip"));
  }
  // its states: from the start, and from each step that changes it (the steps where nothing
  // changes for it are skipped)
  const std::vector<int> states = story_states(story_level);
  const int step = story_level.empty() ? v.step : v.step_of(story_level);
  int state = 1;
  for (size_t i = 1; i < states.size(); i++)
    if (states[i] <= step) state = (int)i + 1;
  const int count = (int)states.size();
  auto set_state = [&](int s) {
    const int to = states[std::clamp(s, 1, count) - 1];
    if (story_level.empty()) v.step = to;
    else v.level_steps[story_level] = to;
    changed = true;
  };
  const float label_w = std::min(420.f, ImGui::GetContentRegionAvail().x * 0.42f);
  ImGui::SameLine();
  ImGui::BeginDisabled(!v.enabled);
  ImGui::BeginDisabled(state <= 1);
  if (ImGui::ArrowButton("##prev", ImGuiDir_Left)) set_state(state - 1);
  ImGui::EndDisabled();
  tooltip(tr("story.prev.tip"));
  ImGui::SameLine();
  ImGui::SetNextItemWidth(std::max(80.f, ImGui::GetContentRegionAvail().x - label_w - ImGui::GetFrameHeight() -
                                             style.ItemSpacing.x * 2));
  int slider = state;
  ImGui::BeginDisabled(count <= 1);
  const std::string format = "%d / " + std::to_string(count);
  if (ImGui::SliderInt("##state", &slider, 1, std::max(count, 1), format.c_str(), ImGuiSliderFlags_AlwaysClamp) &&
      slider != state)
    set_state(slider);
  ImGui::EndDisabled();
  tooltip(tr(count <= 1 ? "story.slider.none" : "story.slider.tip"));
  ImGui::SameLine();
  ImGui::BeginDisabled(state >= count);
  if (ImGui::ArrowButton("##next", ImGuiDir_Right)) set_state(state + 1);
  ImGui::EndDisabled();
  tooltip(tr("story.next.tip"));
  ImGui::EndDisabled();
  ImGui::SameLine();
  const int shown_step = states[state - 1];
  std::string text;
  if (!v.enabled) {
    text = tr("story.all");
  } else if (count <= 1) {
    text = tr("story.no_change");
  } else {
    text = shown_step <= 1 ? std::string(tr("story.start"))
                           : strf(tr("story.after"), data->node(shown_step - 1).name.c_str());
  }
  ImGui::TextUnformatted(fit_text(text, label_w).c_str());
  if (v.enabled && shown_step > 1) tooltip(data->node(shown_step - 1).name.c_str());
  ImGui::End();
  if (changed) set_story_view(v);
}

// A word in the empty viewport: where to start.
void App::ui_viewport_hint() {
  if (document_open() || tool == ToolMode::Place) return;
  ImDrawList* dl = ImGui::GetBackgroundDrawList();
  const char* lines[] = {tr(library.empty() ? "viewport.hint.extract" : "viewport.hint.load"),
                         tr("viewport.hint.camera")};
  float y = m_vp_y + m_vp_h * 0.44f;
  for (const char* l : lines) {
    ImVec2 s = ImGui::CalcTextSize(l);
    dl->AddText(ImVec2(m_vp_x + (m_vp_w - s.x) * 0.5f, y), IM_COL32(200, 210, 220, 170), l);
    y += s.y + 8;
  }
}

// Names of the actors near the camera, and of the selection.
void App::ui_labels() {
  if (!show_labels) return;
  ImDrawList* dl = ImGui::GetBackgroundDrawList();
  int drawn = 0;
  const bool few_selected = selection.size() <= 12;
  const SceneCache& cache = scene.cache();
  for (size_t i = 0; i < scene.nodes.size() && drawn <= 300; i++) {
    const auto& n = scene.nodes[i];
    const bool sel = few_selected && selection.count(n.id) != 0;
    if (!sel && (n.kind != NodeKind::Actor || !rs.show_actors)) continue;
    if (!cache.visible[i]) continue;
    Vec3 p = cache.worlds[i].col3(3);
    AABB b = n.mesh ? n.mesh->bounds().transformed(cache.worlds[i]) : AABB{};
    if (b.valid() && !b.expanded(1.f).contains(p)) p = b.center();  // meshes in world space: origin at 0
    if (!sel && length(p - camera.pos) > 60.f) continue;
    Vec2 s;
    if (!renderer.project(p + Vec3{0, 0.9f, 0}, &s)) continue;
    if (s.x < 0 || s.y < 0 || s.x > m_vp_w || s.y > m_vp_h) continue;
    std::string text = n.name;
    if (n.kind == NodeKind::Actor) text += "  (" + n.extras.value("etype", std::string()) + ")";
    ImVec2 pos(m_vp_x + s.x, m_vp_y + s.y);
    ImVec2 size = ImGui::CalcTextSize(text.c_str());
    dl->AddRectFilled(ImVec2(pos.x - 3, pos.y - 1), ImVec2(pos.x + size.x + 3, pos.y + size.y + 1),
                      IM_COL32(10, 12, 14, 150), 3.f);
    dl->AddText(pos, sel ? IM_COL32(255, 180, 80, 255) : IM_COL32(230, 232, 236, 230), text.c_str());
    drawn++;
  }
}

// ------------------------------------------------------------------------------------------------
// floating windows and popups
// ------------------------------------------------------------------------------------------------

void App::ui_navmesh_inspector(Node& n) {
  NavMeshData m;
  if (!navmesh_of_node(n, &m)) return;
  ImGui::SeparatorText(tr("navmesh.inspector"));
  ImGui::Text(tr("navmesh.polys"), m.polys.size(), m.verts.size());
  ImGui::Text(tr("navmesh.area"), m.area());
  const std::string problem = navmesh_problem(m);
  if (problem.empty()) {
    ImGui::TextColored(ImVec4(0.45f, 0.85f, 0.5f, 1), "%s", tr("navmesh.ok"));
  } else {
    ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(1, 0.6f, 0.3f, 1));
    ImGui::TextWrapped(tr("navmesh.problem"), problem.c_str());
    ImGui::PopStyleColor();
  }
  ImGui::BeginDisabled(navmesh_busy());
  if (ImGui::Button(tr_id("navmesh.regenerate").c_str())) {
    std::string err;
    if (!start_navmesh_generation(scene.world_bounds(n).expanded(nav_params.agent_radius + 1.f), &err))
      LOG_WARN("%s", err.c_str());
  }
  ImGui::EndDisabled();
  tooltip(tr("navmesh.regenerate.tip"));
}

void App::ui_navmesh_window() {
  if (!show_navmesh_window) return;
  ImGui::SetNextWindowSize(ImVec2(430, 0), ImGuiCond_FirstUseEver);
  if (!ImGui::Begin(tr_id("navmesh.title").c_str(), &show_navmesh_window)) {
    ImGui::End();
    return;
  }
  ImGui::TextWrapped("%s", tr("navmesh.help"));
  NavParams& p = nav_params;
  ImGui::SeparatorText(tr("navmesh.creature"));
  ImGui::PushItemWidth(-190);
  ImGui::DragFloat(tr_id("navmesh.radius").c_str(), &p.agent_radius, 0.05f, 0.1f, 5.f, "%.2f m");
  tooltip(tr("navmesh.radius.tip"));
  ImGui::DragFloat(tr_id("navmesh.height").c_str(), &p.agent_height, 0.05f, 0.5f, 10.f, "%.2f m");
  tooltip(tr("navmesh.height.tip"));
  ImGui::DragFloat(tr_id("navmesh.climb").c_str(), &p.max_climb, 0.05f, 0.f, 5.f, "%.2f m");
  tooltip(tr("navmesh.climb.tip"));
  ImGui::DragFloat(tr_id("navmesh.slope").c_str(), &p.max_slope, 0.5f, 5.f, 85.f, "%.0f deg");
  tooltip(tr("navmesh.slope.tip"));
  if (ImGui::TreeNode(tr_id("navmesh.detail").c_str())) {
    ImGui::DragFloat(tr_id("navmesh.cell").c_str(), &p.cell_size, 0.01f, 0.1f, 4.f, "%.2f m");
    tooltip(tr("navmesh.cell.tip"));
    ImGui::DragFloat(tr_id("navmesh.edge").c_str(), &p.max_edge, 0.5f, 4.f, 200.f, "%.0f m");
    tooltip(tr("navmesh.edge.tip"));
    ImGui::DragFloat(tr_id("navmesh.error").c_str(), &p.max_error, 0.05f, 0.1f, 5.f, "%.2f m");
    tooltip(tr("navmesh.error.tip"));
    ImGui::DragFloat(tr_id("navmesh.min_region").c_str(), &p.min_region, 1.f, 0.f, 1000.f, "%.0f m2");
    tooltip(tr("navmesh.min_region.tip"));
    if (ImGui::Button(tr_id("navmesh.defaults").c_str())) p = NavParams{};
    ImGui::TreePop();
  }
  ImGui::PopItemWidth();

  ImGui::Spacing();
  const bool busy = navmesh_busy();
  const AABB around = navmesh_region_of_selection();
  const std::vector<Vec3> points = navmesh_points_of_selection();
  ImGui::Checkbox(tr_id("navmesh.keep_selected").c_str(), &nav_keep_selected);
  tooltip(tr("navmesh.keep_selected.tip"));
  const bool keep = nav_keep_selected;
  ImGui::BeginDisabled(busy || !document_open() || (keep && points.empty()));
  if (ImGui::Button(tr_id("navmesh.generate_all").c_str(), ImVec2(-1, 0))) {
    std::string err;
    if (!start_navmesh_generation(AABB{}, &err, keep ? points : std::vector<Vec3>{})) LOG_WARN("%s", err.c_str());
  }
  tooltip(tr("navmesh.generate_all.tip"));
  ImGui::BeginDisabled(!around.valid());
  if (ImGui::Button(tr_id("navmesh.generate_selection").c_str(), ImVec2(-1, 0))) {
    std::string err;
    if (!start_navmesh_generation(around, &err, keep ? points : std::vector<Vec3>{})) LOG_WARN("%s", err.c_str());
  }
  ImGui::EndDisabled();
  tooltip(tr("navmesh.generate_selection.tip"));
  ImGui::EndDisabled();
  if (busy) {
    ImGui::TextDisabled("%s", tr("navmesh.working"));
  } else if (m_nav_generation) {
    if (m_nav_last.ok) {
      ImGui::TextDisabled(tr("navmesh.last"), m_nav_last.stats.meshes, m_nav_last.stats.polys, m_nav_last.stats.ms / 1000.0);
      if (m_nav_last.stats.split) ImGui::TextWrapped(tr("navmesh.split"), m_nav_last.stats.split);
      if (m_nav_last.stats.left_out) ImGui::TextWrapped(tr("navmesh.left_out"), m_nav_last.stats.left_out);
    } else {
      ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(1, 0.6f, 0.3f, 1));
      ImGui::TextWrapped("%s", m_nav_last.error.c_str());
      ImGui::PopStyleColor();
    }
  }

  // what the document has
  const auto nodes = navmesh_nodes();
  size_t polys = 0, bad = 0;
  float area = 0;
  for (auto id : nodes) {
    NavMeshData m;
    if (!navmesh_of_node(*scene.find(id), &m)) continue;
    polys += m.polys.size();
    area += m.area();
    bad += !navmesh_problem(m).empty();
  }
  ImGui::SeparatorText(tr("navmesh.document"));
  ImGui::Text(tr("navmesh.count"), nodes.size(), polys, area);
  if (bad) ImGui::TextColored(ImVec4(1, 0.6f, 0.3f, 1), tr("navmesh.bad"), bad);
  ImGui::BeginDisabled(nodes.empty());
  if (ImGui::Button(tr_id("navmesh.select_all").c_str())) {
    selection = std::set<uint32_t>(nodes.begin(), nodes.end());
    primary = nodes.front();
  }
  ImGui::SameLine();
  if (ImGui::Button(tr_id("navmesh.delete_all").c_str())) {
    selection = std::set<uint32_t>(nodes.begin(), nodes.end());
    delete_selection();
  }
  ImGui::EndDisabled();

  ImGui::SeparatorText(tr("navmesh.export"));
  ImGui::TextWrapped("%s", tr("navmesh.export.help"));
  ImGui::SetNextItemWidth(140);
  ImGui::InputScalar(tr_id("navmesh.first_id").c_str(), ImGuiDataType_U32, &nav_first_aid);
  tooltip(tr("navmesh.first_id.tip"));
  ImGui::BeginDisabled(nodes.empty() || bad > 0);
  if (ImGui::Button(tr_id("navmesh.export_button").c_str(), ImVec2(-1, 0))) open_dialog(DialogAction::ExportNavMeshes);
  ImGui::EndDisabled();
  ImGui::End();
}

void App::ui_export_window() {
  if (!show_export_window) return;
  const ImGuiViewport* vp = ImGui::GetMainViewport();
  ImGui::SetNextWindowPos(vp->GetCenter(), ImGuiCond_Appearing, ImVec2(0.5f, 0.5f));
  ImGui::SetNextWindowSizeConstraints(ImVec2(520, 0), ImVec2(900, vp->Size.y * 0.9f));
  if (!ImGui::Begin(tr_id("export.title").c_str(), &show_export_window)) {
    ImGui::End();
    return;
  }
  LevelExportOptions& o = export_options;
  ImGui::TextWrapped("%s", tr("export.help"));
  ImGui::PushItemWidth(-200);

  ImGui::SeparatorText(tr("export.names"));
  auto text_field = [](const char* key, std::string& value, size_t max) {
    char buf[256];
    std::snprintf(buf, sizeof(buf), "%s", value.c_str());
    if (ImGui::InputText(tr_id(key).c_str(), buf, std::min(sizeof(buf), max + 1))) value = buf;
    tooltip(tr((std::string(key) + ".tip").c_str()));
  };
  text_field("export.name", o.name, 10);
  text_field("export.nickname", o.nickname, 3);
  text_field("export.iso_name", o.iso_name, 8);
  ImGui::InputScalar(tr_id("export.base_id").c_str(), ImGuiDataType_U32, &o.base_id);
  tooltip(tr("export.base_id.tip"));
  // in place of a level of the game, or a level of its own
  {
    const std::string none = tr("export.replaces.none");
    if (ImGui::BeginCombo(tr_id("export.replaces").c_str(), o.replaces.empty() ? none.c_str() : o.replaces.c_str())) {
      if (ImGui::Selectable(none.c_str(), o.replaces.empty())) o.replaces.clear();
      for (const std::string& l : {scene.settings.replaces, scene.settings.level}) {
        const GameData* data = document_game_data();
        const LevelInfo* info = data ? data->level(l) : nullptr;
        if (!l.empty() && info && !info->dgo.empty() && ImGui::Selectable(l.c_str(), o.replaces == l)) o.replaces = l;
      }
      ImGui::EndCombo();
    }
    tooltip(tr("export.replaces.tip"));
  }

  ImGui::SeparatorText(tr("export.content"));
  ImGui::Checkbox(tr_id("export.actors").c_str(), &o.actors);
  tooltip(tr("export.actors.tip"));
  int collision = o.game_collision ? 0 : 1;
  ImGui::RadioButton(tr_id("export.collision.game").c_str(), &collision, 0);
  tooltip(tr("export.collision.game.tip"));
  ImGui::SameLine();
  ImGui::RadioButton(tr_id("export.collision.decor").c_str(), &collision, 1);
  tooltip(tr("export.collision.decor.tip"));
  o.game_collision = collision == 0;
  if (o.replaces.empty()) {
    ImGui::DragFloat3(tr_id("export.start").c_str(), &o.start.x, 0.1f, 0.f, 0.f, "%.1f");
    tooltip(tr("export.start.tip"));
    ImGui::SameLine();
    if (ImGui::SmallButton(tr_id("export.start.reset").c_str())) o.start = export_start_point();
    tooltip(tr("export.start.reset.tip"));
  } else {
    // the game starts the player at the continue points of the level replaced
    ImGui::TextWrapped(tr("export.replaces.start"), o.replaces.c_str());
  }

  ImGui::SeparatorText(tr("export.folder"));
  text_field("export.folder_path", o.folder, 250);
  ImGui::SameLine();
  if (ImGui::SmallButton(tr_id("export.browse").c_str())) open_dialog(DialogAction::ExportFolder);
  ImGui::TextDisabled("%s", tr("export.folder.hint"));
  ImGui::PopItemWidth();

  ImGui::Spacing();
  const std::string bad = check_export_names(o, document_game_data());
  if (!bad.empty()) ImGui::TextColored(ImVec4(1, 0.6f, 0.3f, 1), "%s", tr(bad.c_str()));
  ImGui::BeginDisabled(!bad.empty() || !document_open());
  if (ImGui::Button(tr_id("export.button").c_str(), ImVec2(-1, 34))) {
    LevelExportReport rep;
    m_export_error.clear();
    if (export_for_game(o, &rep, &m_export_error)) {
      m_export_report = std::move(rep);
    } else {
      m_export_report.reset();
      LOG_ERROR(tr("log.export_failed"), m_export_error.c_str());
    }
  }
  ImGui::EndDisabled();

  if (!m_export_error.empty()) {
    ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(1, 0.6f, 0.3f, 1));
    ImGui::TextWrapped("%s", m_export_error.c_str());
    ImGui::PopStyleColor();
  }
  if (m_export_report) {
    const LevelExportReport& r = *m_export_report;
    ImGui::SeparatorText(tr("export.done"));
    ImGui::TextWrapped(tr("export.written"), r.dir.c_str());
    ImGui::TextWrapped(tr("export.counts"), r.meshes, r.triangles, r.collision_triangles, r.actors, r.art_groups);
    if (r.navmeshes) ImGui::TextWrapped(tr("export.counts.navmeshes"), r.navmeshes);
    if (r.regions) ImGui::TextWrapped(tr("export.counts.regions"), r.regions);
    for (const auto& note : r.notes) {
      ImGui::Bullet();
      ImGui::SameLine();
      ImGui::TextWrapped("%s", strf(tr(note.key.c_str()), note.count, note.detail.c_str()).c_str());
    }
    ImGui::TextWrapped("%s", tr("export.install"));
    if (ImGui::Button(tr_id("export.open_folder").c_str())) SDL_OpenURL(("file:///" + r.dir).c_str());
  }
  ImGui::End();
}

void App::ui_windows() {
  ui_navmesh_window();
  ui_export_window();
  ui_guide_window();

  if (show_log) {
    ImGui::SetNextWindowSize(ImVec2(760, 360), ImGuiCond_FirstUseEver);
    if (ImGui::Begin(tr_id("log.title").c_str(), &show_log)) {
      auto lines = Log::get().lines();
      if (ImGui::SmallButton(tr_id("log.copy").c_str())) {
        std::string all;
        for (const auto& l : lines) all += l.text + "\n";
        ImGui::SetClipboardText(all.c_str());
      }
      ImGui::BeginChild("lines", ImVec2(0, 0), ImGuiChildFlags_Borders, ImGuiWindowFlags_HorizontalScrollbar);
      for (const auto& l : lines) {
        ImVec4 col = l.level == LogLevel::Error  ? ImVec4(1, 0.45f, 0.4f, 1)
                     : l.level == LogLevel::Warn ? ImVec4(1, 0.8f, 0.4f, 1)
                                                 : ImVec4(0.8f, 0.82f, 0.85f, 1);
        ImGui::TextColored(col, "%s", l.text.c_str());
      }
      if (ImGui::GetScrollY() >= ImGui::GetScrollMaxY() - 20) ImGui::SetScrollHereY(1.f);
      ImGui::EndChild();
    }
    ImGui::End();
  }

  // a name for the prefab made of the selection
  if (m_show_new_prefab) {
    ImGui::OpenPopup(tr_id("prefab_popup.title").c_str());
    m_show_new_prefab = false;
    m_new_prefab_name.clear();
    if (const Node* n = scene.find(primary)) m_new_prefab_name = n->mesh ? n->mesh->name : n->name;
    if (m_new_prefab_name.size() > 3 && m_new_prefab_name.substr(m_new_prefab_name.size() - 3) == ".mb")
      m_new_prefab_name.resize(m_new_prefab_name.size() - 3);
  }
  if (ImGui::BeginPopupModal(tr_id("prefab_popup.title").c_str(), nullptr, ImGuiWindowFlags_AlwaysAutoResize)) {
    ImGui::Text(tr("prefab_popup.text"), selection_roots().size());
    if (prefab_folder().empty()) ImGui::TextColored(ImVec4(1, 0.45f, 0.4f, 1), "%s", tr("log.prefab_no_library"));
    ImGui::SetNextItemWidth(360);
    if (ImGui::IsWindowAppearing()) ImGui::SetKeyboardFocusHere();
    bool enter = ImGui::InputText(tr_id("prefab_popup.name").c_str(), &m_new_prefab_name,
                                  ImGuiInputTextFlags_EnterReturnsTrue);
    ImGui::BeginDisabled(m_new_prefab_name.empty() || prefab_folder().empty() || selection.empty());
    if (ImGui::Button(tr_id("prefab_popup.create").c_str(), ImVec2(140, 0)) || (enter && !m_new_prefab_name.empty())) {
      create_prefab(m_new_prefab_name);
      left_tab_request = 1;
      ImGui::CloseCurrentPopup();
    }
    ImGui::EndDisabled();
    ImGui::SameLine();
    if (ImGui::Button(tr_id("popup.cancel").c_str(), ImVec2(140, 0))) ImGui::CloseCurrentPopup();
    ImGui::EndPopup();
  }

  // unsaved changes before loading or quitting
  if (m_show_discard) {
    ImGui::OpenPopup(tr_id("discard.title").c_str());
    m_show_discard = false;
  }
  if (ImGui::BeginPopupModal(tr_id("discard.title").c_str(), nullptr, ImGuiWindowFlags_AlwaysAutoResize)) {
    ImGui::TextUnformatted(tr("discard.text"));
    ImGui::Spacing();
    if (ImGui::Button(tr_id("discard.save").c_str(), ImVec2(160, 0))) {
      save(false);
      ImGui::CloseCurrentPopup();
      // saved at once when the project has a file; else the Save as dialog opens and the action waits
      // for the user to ask again
      if (history.revision() == saved_revision && m_pending_discard) m_pending_discard();
      m_pending_discard = nullptr;
    }
    ImGui::SameLine();
    if (ImGui::Button(tr_id("discard.discard").c_str(), ImVec2(160, 0))) {
      ImGui::CloseCurrentPopup();
      if (m_pending_discard) m_pending_discard();
      m_pending_discard = nullptr;
    }
    ImGui::SameLine();
    if (ImGui::Button(tr_id("popup.cancel").c_str(), ImVec2(120, 0))) {
      m_pending_discard = nullptr;
      ImGui::CloseCurrentPopup();
    }
    ImGui::EndPopup();
  }
}

}  // namespace ogle
