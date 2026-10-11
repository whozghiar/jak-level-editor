// The guide: the guided tour (F1), every tool with its animation, the keys; the tooltips of the
// guide over the tools; and the Build panel (the shapes, the palette of Jak 3's materials).

#include <algorithm>
#include <cstring>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "imgui.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::IsItemHovered(ImGuiHoveredFlags_DelayShort) && text && *text) ImGui::SetTooltip("%s", text);
}

// a key of the translations that has a text
bool has_text(const std::string& key) {
  const char* t = tr(key.c_str());
  return *t && key != t;
}

// What the guide explains: the keys guide.<id>.title, .text, .example and .keys (optional); `demo`
// its animation (render/tool_demo.h), or none
struct GuideEntry {
  const char* id;
  const char* demo;
};
const GuideEntry kGuide[] = {
    {"camera", "camera"},   {"select", "select"},       {"box_select", "box_select"}, {"move", "move"},
    {"rotate", "rotate"},   {"scale", "scale"},         {"snap", "snap"},             {"local", nullptr},
    {"build", "build"},     {"palette", "build"},       {"catalog", "place"},         {"interactive", nullptr},
    {"models", "place"},    {"decor", "place"},         {"jak2", nullptr},            {"prefab", "prefab"},
    {"game_prefabs", "prefab"}, {"bounce", "bounce"},   {"launch", "launch"},         {"collision", nullptr},
    {"levels", "story"},    {"story", "story"},         {"replace", "replace"},       {"export", nullptr}};

const GuideEntry* guide_entry(const char* id) {
  for (const auto& e : kGuide)
    if (!std::strcmp(e.id, id)) return &e;
  return nullptr;
}

// the sections of Every tool
struct GuideSection {
  const char* key;
  std::vector<const char*> ids;
};
const std::vector<GuideSection>& guide_sections() {
  static const std::vector<GuideSection> s = {
      {"guide.section.view", {"camera", "select", "box_select"}},
      {"guide.section.transform", {"move", "rotate", "scale", "snap", "local"}},
      {"guide.section.build", {"build", "palette", "catalog", "interactive", "models", "decor", "jak2", "prefab", "game_prefabs"}},
      {"guide.section.behaviors", {"bounce", "launch", "collision"}},
      {"guide.section.level", {"levels", "story", "replace", "export"}}};
  return s;
}

// The steps of the guided tour: tour.<id>.title, .text and .action (the button that shows it)
enum class TourAction { None, Extract, LevelsTab, BuildTab, CatalogTab, PrefabsTab, Replace, Export };
struct TourStep {
  const char* id;
  std::vector<const char*> demos;
  TourAction action;
};
const std::vector<TourStep>& tour_steps() {
  static const std::vector<TourStep> s = {
      {"extract", {}, TourAction::Extract},
      {"levels", {"story"}, TourAction::LevelsTab},
      {"camera", {"camera"}, TourAction::None},
      {"select", {"select", "box_select"}, TourAction::None},
      {"gizmo", {"move", "rotate", "scale", "snap"}, TourAction::None},
      {"build", {"build"}, TourAction::BuildTab},
      {"catalog", {"place"}, TourAction::CatalogTab},
      {"prefabs", {"prefab"}, TourAction::PrefabsTab},
      {"behaviors", {"bounce", "launch"}, TourAction::None},
      {"replace", {"replace"}, TourAction::Replace},
      {"export", {}, TourAction::Export}};
  return s;
}

const ImVec4 kTitle(0.45f, 0.82f, 0.9f, 1.f);
const ImVec4 kExample(0.95f, 0.8f, 0.5f, 1.f);

}  // namespace

void App::ui_demo(const std::string& name, float w, float h, double t) {
  static std::vector<DemoOverlay> overlay;
  const unsigned tex = demos.draw(name, t, (int)w, (int)h, &overlay);
  const ImVec2 at = ImGui::GetCursorScreenPos();
  if (!tex) {
    ImGui::Dummy(ImVec2(w, h));
    return;
  }
  ImGui::Image((ImTextureID)(uint64_t)tex, ImVec2(w, h), ImVec2(0, 1), ImVec2(1, 0));
  ImDrawList* dl = ImGui::GetWindowDrawList();
  dl->PushClipRect(at, ImVec2(at.x + w, at.y + h), true);
  auto px = [&](const Vec2& p) { return ImVec2(at.x + p.x, at.y + p.y); };
  for (const auto& o : overlay) {
    const ImVec2 p = px(o.at), q = px(o.to);
    switch (o.kind) {
      case DemoOverlay::Kind::Rect: {
        const ImVec2 a(std::min(p.x, q.x), std::min(p.y, q.y)), b(std::max(p.x, q.x), std::max(p.y, q.y));
        if (o.text == "bar") {
          dl->AddRectFilled(a, b, IM_COL32(90, 96, 108, 255), 3.f);
        } else if (o.text == "knob") {
          dl->AddRectFilled(a, b, IM_COL32(230, 236, 245, 255), 3.f);
        } else {
          dl->AddRectFilled(a, b, IM_COL32(80, 160, 255, 40));
          dl->AddRect(a, b, IM_COL32(80, 160, 255, 220));
        }
        break;
      }
      case DemoOverlay::Kind::Click: {
        const float k = o.to.x;
        dl->AddCircle(p, 6.f + 16.f * k, IM_COL32(255, 255, 255, (int)(255 * (1.f - k))), 0, 2.f);
        break;
      }
      case DemoOverlay::Kind::Cursor: {
        // an arrow: white, outlined
        const ImVec2 a = p, b(p.x, p.y + 17.f), c(p.x + 12.f, p.y + 12.f);
        dl->AddLine(ImVec2(p.x + 4.5f, p.y + 12.f), ImVec2(p.x + 8.f, p.y + 19.f), IM_COL32(0, 0, 0, 255), 4.f);
        dl->AddLine(ImVec2(p.x + 4.5f, p.y + 12.f), ImVec2(p.x + 8.f, p.y + 19.f), IM_COL32(255, 255, 255, 255), 2.f);
        dl->AddTriangleFilled(a, b, c, IM_COL32(255, 255, 255, 255));
        dl->AddTriangle(a, b, c, IM_COL32(0, 0, 0, 255), 1.5f);
        break;
      }
      case DemoOverlay::Kind::Label: {
        const char* text = o.text.rfind("guide.", 0) == 0 ? tr(o.text.c_str()) : o.text.c_str();
        const float wrap = std::max(60.f, w - o.at.x - 12.f);
        const ImVec2 size = ImGui::CalcTextSize(text, nullptr, false, wrap);
        ImVec2 tp = p;
        if (tp.y + size.y + 4.f > at.y + h) tp.y = at.y + h - size.y - 6.f;  // kept in the picture
        dl->AddRectFilled(ImVec2(tp.x - 4, tp.y - 2), ImVec2(tp.x + size.x + 4, tp.y + size.y + 2), IM_COL32(0, 0, 0, 160), 4.f);
        dl->AddText(nullptr, 0.f, tp, IM_COL32(255, 255, 255, 255), text, nullptr, wrap);
        break;
      }
    }
  }
  dl->PopClipRect();
}

void App::guide_tip(const char* id) {
  if (!ImGui::BeginItemTooltip()) return;
  // its animation starts over each time the tooltip opens
  static std::string shown;
  static int last_frame = -10;
  static double since = 0;
  const int frame = ImGui::GetFrameCount();
  if (shown != id || frame > last_frame + 1) {
    shown = id;
    since = ImGui::GetTime();
  }
  last_frame = frame;
  const std::string k = std::string("guide.") + id;
  const float width = ImGui::GetFontSize() * 22.f;
  ImGui::PushTextWrapPos(width);
  ImGui::TextColored(kTitle, "%s", tr((k + ".title").c_str()));
  if (has_text(k + ".keys")) {
    ImGui::SameLine();
    ImGui::TextDisabled("%s", tr((k + ".keys").c_str()));
  }
  ImGui::TextWrapped("%s", tr((k + ".text").c_str()));
  if (has_text(k + ".example")) {
    ImGui::PushStyleColor(ImGuiCol_Text, kExample);
    ImGui::TextWrapped("%s", tr((k + ".example").c_str()));
    ImGui::PopStyleColor();
  }
  ImGui::PopTextWrapPos();
  if (const GuideEntry* e = guide_entry(id); e && e->demo) ui_demo(e->demo, width, width * 9.f / 16.f, ImGui::GetTime() - since);
  ImGui::EndTooltip();
}

void App::ui_guide_window() {
  if (!show_help) return;
  ImGui::SetNextWindowSize(ImVec2(780, 640), ImGuiCond_FirstUseEver);
  if (!ImGui::Begin(tr_id("guide.title").c_str(), &show_help)) {
    ImGui::End();
    return;
  }
  if (ImGui::BeginTabBar("guide")) {
    const char* tabs[] = {"guide.tab.tour", "guide.tab.tools", "guide.tab.keys"};
    for (int tab = 0; tab < 3; tab++) {
      if (!ImGui::BeginTabItem(tr_id(tabs[tab]).c_str(), nullptr, guide_tab_request == tab ? ImGuiTabItemFlags_SetSelected : 0))
        continue;
      if (tab == 0) {
        // the guided tour: a step at a time, its animations, a button that shows where it is
        const auto& steps = tour_steps();
        guide_step = std::clamp(guide_step, 0, (int)steps.size() - 1);
        static int started_step = -1;
        static double since = 0;
        if (started_step != guide_step) {
          started_step = guide_step;
          since = ImGui::GetTime();
        }
        const TourStep& s = steps[guide_step];
        const std::string k = std::string("tour.") + s.id;
        // where we are: a dot per step
        for (int i = 0; i < (int)steps.size(); i++) {
          ImGui::PushID(i);
          ImGui::PushStyleColor(ImGuiCol_Button, i == guide_step ? ImVec4(0.22f, 0.6f, 0.66f, 1.f)
                                                                 : ImVec4(0.3f, 0.32f, 0.36f, 1.f));
          if (ImGui::Button(strf("%d", i + 1).c_str(), ImVec2(28, 0))) guide_step = i;
          ImGui::PopStyleColor();
          tooltip(tr(("tour." + std::string(steps[i].id) + ".title").c_str()));
          ImGui::PopID();
          ImGui::SameLine();
        }
        ImGui::NewLine();
        ImGui::Spacing();
        ImGui::TextColored(kTitle, "%s", tr((k + ".title").c_str()));
        ImGui::TextWrapped("%s", tr((k + ".text").c_str()));
        ImGui::Spacing();
        // its animations: one wide, two side by side, four in a square
        const float room = ImGui::GetContentRegionAvail().x;
        const float bottom = ImGui::GetFrameHeightWithSpacing() * 2.f + 8.f;
        const int n = (int)s.demos.size();
        if (n) {
          const int per_row = n == 1 ? 1 : 2;
          const int rows = (n + per_row - 1) / per_row;
          const float gap = ImGui::GetStyle().ItemSpacing.x;
          float w = (room - gap * (per_row - 1)) / per_row, h = w * 9.f / 16.f;
          const float max_h = (ImGui::GetContentRegionAvail().y - bottom) / rows - ImGui::GetStyle().ItemSpacing.y;
          if (h > max_h && max_h > 60.f) {
            h = max_h;
            w = h * 16.f / 9.f;
          }
          for (int i = 0; i < n; i++) {
            if (i % per_row) ImGui::SameLine();
            ui_demo(s.demos[i], w, h, ImGui::GetTime() - since);
          }
        }
        // at the bottom: the button that shows it, previous, next
        ImGui::SetCursorPosY(std::max(ImGui::GetCursorPosY(), ImGui::GetWindowHeight() - bottom));
        ImGui::Separator();
        if (s.action != TourAction::None) {
          const bool needs_document = s.action == TourAction::Replace || s.action == TourAction::Export;
          ImGui::BeginDisabled(needs_document && !document_open());
          if (ImGui::Button(tr_id((k + ".action").c_str()).c_str())) {
            switch (s.action) {
              case TourAction::Extract: open_extract_window(); break;
              case TourAction::LevelsTab: left_tab_request = 0; break;
              case TourAction::CatalogTab: left_tab_request = 1; break;
              case TourAction::PrefabsTab: left_tab_request = 2; break;
              case TourAction::BuildTab: left_tab_request = 3; break;
              case TourAction::Replace:
                select_only(0);
                m_scroll_to_replace = true;
                break;
              case TourAction::Export: open_export_window(); break;
              case TourAction::None: break;
            }
          }
          ImGui::EndDisabled();
          if (needs_document && !document_open()) tooltip(tr("tour.needs_level"));
          ImGui::SameLine();
        }
        const float nav_w = 110.f;
        ImGui::SetCursorPosX(std::max(ImGui::GetCursorPosX(), ImGui::GetWindowWidth() - nav_w * 2 - 24.f));
        ImGui::BeginDisabled(guide_step == 0);
        if (ImGui::Button(tr_id("tour.previous").c_str(), ImVec2(nav_w, 0))) guide_step--;
        ImGui::EndDisabled();
        ImGui::SameLine();
        if (guide_step + 1 < (int)steps.size()) {
          if (ImGui::Button(tr_id("tour.next").c_str(), ImVec2(nav_w, 0))) guide_step++;
        } else if (ImGui::Button(tr_id("tour.finish").c_str(), ImVec2(nav_w, 0))) {
          show_help = false;
        }
      } else if (tab == 1) {
        // every tool: its animation, what it does, a concrete example
        ImGui::BeginChild("tools");
        const float w = 224.f, h = 126.f;
        for (const auto& section : guide_sections()) {
          ImGui::SeparatorText(tr(section.key));
          for (const char* id : section.ids) {
            const GuideEntry* e = guide_entry(id);
            const std::string k = std::string("guide.") + id;
            ImGui::PushID(id);
            const ImVec2 top = ImGui::GetCursorScreenPos();
            // drawn only on screen: each animation is a picture made every frame
            if (e && e->demo && ImGui::IsRectVisible(ImVec2(w, h))) ui_demo(e->demo, w, h, ImGui::GetTime());
            else ImGui::Dummy(ImVec2(w, h));
            ImGui::SameLine();
            ImGui::BeginGroup();
            ImGui::TextColored(kTitle, "%s", tr((k + ".title").c_str()));
            if (has_text(k + ".keys")) {
              ImGui::SameLine();
              ImGui::TextDisabled("%s", tr((k + ".keys").c_str()));
            }
            ImGui::TextWrapped("%s", tr((k + ".text").c_str()));
            if (has_text(k + ".example")) {
              ImGui::PushStyleColor(ImGuiCol_Text, kExample);
              ImGui::TextWrapped("%s", tr((k + ".example").c_str()));
              ImGui::PopStyleColor();
            }
            ImGui::EndGroup();
            ImGui::SetCursorScreenPos(ImVec2(top.x, std::max(ImGui::GetCursorScreenPos().y, top.y + h + 6.f)));
            ImGui::Dummy(ImVec2(0, 0));  // the room taken, for the child's scrolling
            ImGui::PopID();
          }
        }
        ImGui::EndChild();
      } else {
        // the keys, and where things are
        auto bullet = [](const char* text) {
          ImGui::Bullet();
          ImGui::TextWrapped("%s", text);
        };
        ImGui::SeparatorText(tr("help.camera"));
        for (const char* key : {"help.camera.1", "help.camera.2", "help.camera.3", "help.camera.4"}) bullet(tr(key));
        ImGui::SeparatorText(tr("help.story"));
        for (const char* key : {"help.story.1", "help.story.2", "help.story.3"}) bullet(tr(key));
        ImGui::SeparatorText(tr("help.editing"));
        for (const char* key : {"help.editing.1", "help.editing.2", "help.editing.3", "help.editing.4", "help.editing.5",
                                "help.editing.6", "help.editing.7", "help.editing.8", "help.editing.9"})
          bullet(tr(key));
      }
      ImGui::EndTabItem();
    }
    guide_tab_request = -1;
    ImGui::EndTabBar();
  }
  ImGui::End();
}

void App::ui_build_panel() {
  if (!document_open()) {
    ImGui::Spacing();
    ImGui::TextWrapped("%s", tr("build.no_document"));
    return;
  }
  // the shapes: a picture each, a click places it (click again in the view for another, Esc stops)
  ImGui::SeparatorText(tr("build.shapes"));
  guide_tip("build");
  const float size = 52.f, cell = size + ImGui::GetStyle().FramePadding.x * 2.f;
  const float room = ImGui::GetContentRegionAvail().x, gap = ImGui::GetStyle().ItemSpacing.x;
  const int per_row = std::max(1, (int)((room + gap) / (cell + gap)));
  const auto& presets = shape_presets();
  for (size_t i = 0; i < presets.size(); i++) {
    const ShapePreset& p = presets[i];
    if (i % per_row) ImGui::SameLine();
    ImGui::PushID(p.name);
    ImGui::BeginGroup();
    const bool active =
        tool == ToolMode::Place && placement.kind == Placement::Kind::Shape && placement.name == p.name;
    if (active) ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(0.22f, 0.6f, 0.66f, 1.f));
    const unsigned tex = demos.icon(p.params, p.name, (int)size);
    const bool clicked = tex ? ImGui::ImageButton("##shape", (ImTextureID)(uint64_t)tex, ImVec2(size, size), ImVec2(0, 1), ImVec2(1, 0))
                             : ImGui::Button("##shape", ImVec2(cell, cell));
    if (active) ImGui::PopStyleColor();
    const char* name = tr(("menu.create." + std::string(p.name)).c_str());
    tooltip(strf(tr("build.shape.tip"), name, p.params.size.x, p.params.size.y, p.params.size.z).c_str());
    if (clicked) start_shape_placement(p, false);
    // its name under it, cut to the cell
    ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + cell);
    ImGui::TextDisabled("%s", name);
    ImGui::PopTextWrapPos();
    ImGui::EndGroup();
    ImGui::PopID();
  }
  ImGui::Checkbox((strf(tr("build.snap"), gizmo.snap.translate) + "###build-snap").c_str(), &gizmo.snap.enabled);
  tooltip(tr("build.snap.tip"));

  // the palette: the textures Jak 3's levels use again and again, on what is selected
  ImGui::SeparatorText(tr("build.materials"));
  guide_tip("palette");
  static uint64_t counted_rev = ~0ull;
  static std::set<uint32_t> counted_sel;
  static std::vector<uint32_t> ids;
  if (counted_rev != history.revision() || counted_sel != selection) {
    counted_rev = history.revision();
    counted_sel = selection;
    ids.clear();
    const auto roots = selection_roots();
    const auto below = scene.descendants_of(roots);
    auto add = [&](uint32_t id) {
      const Node* n = scene.find(id);
      if (n && n->kind == NodeKind::Render && n->mesh) ids.push_back(id);
    };
    for (size_t r = 0; r < roots.size(); r++) {
      add(roots[r]);
      for (auto id : below[r]) add(id);
    }
  }
  ImGui::PushStyleColor(ImGuiCol_Text, ImGui::GetStyleColorVec4(ImGuiCol_TextDisabled));
  if (ids.empty()) ImGui::TextWrapped("%s", tr("build.materials.none"));
  else ImGui::TextWrapped("%s", strf(tr("build.materials.count"), ids.size()).c_str());
  ImGui::PopStyleColor();
  ui_palette(ids, -1, ImGui::GetContentRegionAvail().x);
}

}  // namespace ogle
