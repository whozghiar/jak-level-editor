// The guide: the tutorials (F1: Getting started, then one per tool or task, each named, a few steps
// each), every tool with its animation, the keys; the tooltips of the guide over the tools; and the
// Build panel (the shapes, the materials of the games).

#include <algorithm>
#include <cstring>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "imgui.h"
#include "imgui_stdlib.h"

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
    {"rotate", "rotate"},   {"scale", "scale"},         {"snap", "snap"},             {"local", "local"},
    {"free_rotate", "free_rotate"}, {"game_view", nullptr},
    {"build", "build"},     {"palette", "build"},       {"catalog", "place"},         {"interactive", nullptr},
    {"models", "place"},    {"decor", "place"},         {"jak2", nullptr},            {"prefab", "prefab"},
    {"game_prefabs", "prefab"}, {"bounce", "bounce"},   {"launch", "launch"},         {"collision", nullptr},
    {"ingame", nullptr},    {"surface", nullptr},       {"event", nullptr},           {"relight", nullptr},
    {"levels", "story"},    {"story", "story"},         {"replace", "replace"},       {"export", nullptr},
    {"place_drag", "place"}, {"topology", nullptr},     {"shape_edit", "build"},      {"style", nullptr},
    {"preview", nullptr},   {"jak2_level", nullptr},     {"terrain", nullptr},        {"assembly", nullptr}};

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
      {"guide.section.view", {"camera", "select", "box_select", "assembly", "game_view"}},
      {"guide.section.transform", {"move", "rotate", "free_rotate", "scale", "snap", "local", "place_drag", "topology"}},
      {"guide.section.build", {"build", "shape_edit", "style", "terrain", "palette", "catalog", "interactive", "models", "decor", "jak2",
                               "prefab", "game_prefabs", "relight"}},
      {"guide.section.behaviors", {"ingame", "collision", "surface", "event", "bounce", "launch", "preview"}},
      {"guide.section.level", {"levels", "jak2_level", "story", "replace", "export"}}};
  return s;
}

// The tutorials: tuto.<id>.title names one; a step's keys are <prefix><step>.title, .text and
// .action (the button that shows it), `demos` its animations
enum class TourAction { None, Extract, LevelsTab, BuildTab, CatalogTab, PrefabsTab, Replace, Export, GameView, Save, NavMeshes };
struct TourStep {
  const char* id;
  std::vector<const char*> demos;
  TourAction action;
};
struct Tutorial {
  const char* id;
  const char* prefix;
  std::vector<TourStep> steps;
};
const std::vector<Tutorial>& tutorials() {
  using A = TourAction;
  static const std::vector<Tutorial> t = {
      {"start", "tour.",
       {{"extract", {}, A::Extract},
        {"levels", {"story"}, A::LevelsTab},
        {"camera", {"camera"}, A::None},
        {"select", {"select", "box_select"}, A::None},
        {"gizmo", {"move", "rotate", "scale", "snap"}, A::None},
        {"build", {"build"}, A::BuildTab},
        {"catalog", {"place"}, A::CatalogTab},
        {"prefabs", {"prefab"}, A::PrefabsTab},
        {"behaviors", {"bounce", "launch"}, A::None},
        {"replace", {"replace"}, A::Replace},
        {"export", {}, A::Export}}},
      {"move", "tuto.move.",
       {{"select", {"select", "box_select"}, A::None}, {"drag", {"move"}, A::None}, {"fine", {"snap"}, A::None}, {"exact", {}, A::None}}},
      {"rotate", "tuto.rotate.", {{"rings", {"rotate"}, A::None}, {"free", {"free_rotate"}, A::None}, {"exact", {}, A::None}}},
      {"scale", "tuto.scale.", {{"axes", {"scale"}, A::None}, {"exact", {}, A::None}, {"shapes", {"build"}, A::BuildTab}}},
      {"local", "tuto.local.", {{"what", {"local"}, A::None}, {"when", {"local"}, A::None}}},
      {"snap", "tuto.snap.", {{"on", {"snap"}, A::None}, {"steps", {"snap"}, A::None}, {"place", {"place"}, A::None}}},
      {"game_view", "tuto.game_view.",
       {{"what", {}, A::GameView}, {"hour", {}, A::None}, {"live", {}, A::None}, {"limits", {}, A::None}}},
      {"ingame", "tuto.ingame.",
       {{"section", {}, A::None},
        {"collision", {}, A::None},
        {"surface", {}, A::None},
        {"event", {}, A::None},
        {"more", {}, A::None},
        {"jumps", {"bounce", "launch"}, A::None}}},
      {"materials", "tuto.materials.", {{"window", {}, A::None}, {"catalog", {"build"}, A::CatalogTab}, {"light", {}, A::None}}},
      {"catalog", "tuto.catalog.",
       {{"game", {}, A::CatalogTab}, {"objects", {"place"}, A::None}, {"decor", {}, A::None}, {"jak2", {}, A::None}}},
      {"prefabs", "tuto.prefabs.", {{"game", {"prefab"}, A::PrefabsTab}, {"mine", {"prefab"}, A::None}}},
      {"levels", "tuto.levels.", {{"open", {"story"}, A::LevelsTab}, {"add", {}, A::None}, {"story", {"story"}, A::None}}},
      {"replace", "tuto.replace.", {{"choose", {"replace"}, A::Replace}, {"align", {"replace"}, A::None}, {"export", {}, A::Export}}},
      {"save", "tuto.save.", {{"save", {}, A::Save}, {"export", {}, A::Export}, {"navmesh", {}, A::NavMeshes}}},
      {"place", "tuto.place.", {{"click", {"place"}, A::CatalogTab}, {"hold", {"place"}, A::None}, {"turn", {"free_rotate"}, A::None}}},
      {"topology", "tuto.topology.", {{"enter", {}, A::None}, {"points", {}, A::None}, {"faces", {}, A::None}, {"undo", {}, A::None}}},
      {"shapes", "tuto.shapes.", {{"size", {"build"}, A::BuildTab}, {"bend", {}, A::None}, {"ground", {}, A::None}}},
      {"style", "tuto.style.", {{"choose", {}, A::BuildTab}, {"floor", {}, A::None}, {"walls", {}, A::None}}},
      {"preview", "tuto.preview.", {{"play", {}, A::CatalogTab}, {"motion", {}, A::None}, {"break", {}, A::None}}},
      {"jak2level", "tuto.jak2level.", {{"choose", {}, A::LevelsTab}, {"what", {}, A::None}, {"export", {}, A::Export}}},
      {"terrain", "tuto.terrain.",
       {{"start", {}, A::BuildTab}, {"paint", {}, A::None}, {"shape", {}, A::None}, {"walls", {}, A::None}, {"generate", {}, A::None}}},
      {"assembly", "tuto.assembly.", {{"select", {"select"}, A::None}, {"prefab", {"prefab"}, A::None}, {"game", {"prefab"}, A::PrefabsTab}}}};
  return t;
}

const ImVec4 kTitle(0.45f, 0.82f, 0.9f, 1.f);
const ImVec4 kExample(0.95f, 0.8f, 0.5f, 1.f);

}  // namespace

bool App::select_tutorial(const std::string& id, int step) {
  const auto& all = tutorials();
  for (int i = 0; i < (int)all.size(); i++)
    if (id == all[i].id) {
      guide_tutorial = i;
      guide_step = step;
      return true;
    }
  return false;
}

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
  ImGui::SetNextWindowSize(ImVec2(980, 700), ImGuiCond_FirstUseEver);
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
        // the tutorials, by name, the steps of the one chosen; a step at a time, its animations, a
        // button that shows where it is
        const auto& all = tutorials();
        guide_tutorial = std::clamp(guide_tutorial, 0, (int)all.size() - 1);
        const Tutorial& tut = all[guide_tutorial];
        const auto& steps = tut.steps;
        guide_step = std::clamp(guide_step, 0, (int)steps.size() - 1);
        static int started_tutorial = -1, started_step = -1;
        static double since = 0;
        if (started_step != guide_step || started_tutorial != guide_tutorial) {
          started_step = guide_step;
          started_tutorial = guide_tutorial;
          since = ImGui::GetTime();
        }
        ImGui::BeginChild("tutorials", ImVec2(250, 0), ImGuiChildFlags_Borders);
        for (int i = 0; i < (int)all.size(); i++) {
          ImGui::PushID(i);
          ImGui::PushStyleColor(ImGuiCol_Text, i == guide_tutorial ? kTitle : ImGui::GetStyleColorVec4(ImGuiCol_Text));
          if (ImGui::Selectable(strf("%d. %s", i + 1, tr(("tuto." + std::string(all[i].id) + ".title").c_str())).c_str(),
                                i == guide_tutorial) &&
              i != guide_tutorial) {
            guide_tutorial = i;
            guide_step = 0;
          }
          ImGui::PopStyleColor();
          if (i == guide_tutorial) {
            // its steps, named
            ImGui::Indent();
            for (int j = 0; j < (int)steps.size(); j++) {
              ImGui::PushID(j);
              const std::string title = tr((std::string(tut.prefix) + steps[j].id + ".title").c_str());
              if (ImGui::Selectable(strf("%d.%d %s", i + 1, j + 1, title.c_str()).c_str(), j == guide_step)) guide_step = j;
              ImGui::PopID();
            }
            ImGui::Unindent();
          }
          ImGui::PopID();
        }
        ImGui::EndChild();
        ImGui::SameLine();
        ImGui::BeginChild("tutorial");
        const TourStep& s = steps[guide_step];
        const std::string k = std::string(tut.prefix) + s.id;
        ImGui::TextDisabled("%s - %s", tr(("tuto." + std::string(tut.id) + ".title").c_str()),
                            strf(tr("tuto.step"), guide_step + 1, (int)steps.size()).c_str());
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
          const bool needs_document = s.action == TourAction::Replace || s.action == TourAction::Export ||
                                      s.action == TourAction::Save || s.action == TourAction::NavMeshes;
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
              case TourAction::GameView: game_view_on = game_view.ready(); break;
              case TourAction::Save: save(false); break;
              case TourAction::NavMeshes: show_navmesh_window = true; break;
              case TourAction::None: break;
            }
          }
          ImGui::EndDisabled();
          if (needs_document && !document_open()) tooltip(tr("tour.needs_level"));
          ImGui::SameLine();
        }
        const float nav_w = 130.f;
        ImGui::SetCursorPosX(std::max(ImGui::GetCursorPosX(), ImGui::GetWindowWidth() - nav_w * 2 - 24.f));
        ImGui::BeginDisabled(guide_step == 0);
        if (ImGui::Button(tr_id("tour.previous").c_str(), ImVec2(nav_w, 0))) guide_step--;
        ImGui::EndDisabled();
        ImGui::SameLine();
        if (guide_step + 1 < (int)steps.size()) {
          if (ImGui::Button(tr_id("tour.next").c_str(), ImVec2(nav_w, 0))) guide_step++;
        } else if (guide_tutorial + 1 < (int)all.size()) {
          // the next tutorial
          if (ImGui::Button(tr_id("tuto.next").c_str(), ImVec2(nav_w, 0))) {
            guide_tutorial++;
            guide_step = 0;
          }
          tooltip(tr(("tuto." + std::string(all[guide_tutorial + 1].id) + ".title").c_str()));
        } else if (ImGui::Button(tr_id("tour.finish").c_str(), ImVec2(nav_w, 0))) {
          show_help = false;
        }
        ImGui::EndChild();
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
        for (const char* key : {"help.editing.1", "help.editing.2", "help.editing.10", "help.editing.3", "help.editing.4",
                                "help.editing.5", "help.editing.11", "help.editing.12", "help.editing.6", "help.editing.7", "help.editing.8", "help.editing.9"})
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

  ui_level_style();

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
  ui_texture_browser(ids, -1, ImGui::GetContentRegionAvail().x, std::max(240.f, ImGui::GetContentRegionAvail().y));
}

void App::ui_level_style() {
  // floors and walls looking like a level's: its textures, their size, its light, its ground's bumps
  ImGui::SeparatorText(tr("build.style"));
  guide_tip("style");
  ImGui::SetNextItemWidth(-1);
  if (ImGui::BeginCombo("##style-level", m_style_level.empty() ? tr("build.style.choose") : m_style_level.c_str(),
                        ImGuiComboFlags_HeightLarge)) {
    if (ImGui::IsWindowAppearing()) ImGui::SetKeyboardFocusHere();
    ImGui::SetNextItemWidth(-1);
    ImGui::InputTextWithHint("##style-filter", tr("build.style.filter"), &m_style_filter);
    std::string f = m_style_filter;
    for (auto& c : f) c = (char)std::tolower((unsigned char)c);
    for (const auto& l : library.levels()) {
      if (!l.listed || (!f.empty() && l.name.find(f) == std::string::npos)) continue;
      if (ImGui::Selectable(l.name.c_str(), l.name == m_style_level)) m_style_level = l.name;
    }
    ImGui::EndCombo();
  }
  if (m_style_level.empty()) return;
  const LevelEntry* level = library.level(m_style_level);
  if (!level) return;
  auto it = m_styles.find(m_style_level);
  if (it == m_styles.end()) {
    // measured once its decor is read (a few seconds)
    auto decor = store.level(level->background, true);
    if (!decor) {
      ImGui::TextDisabled("%s", tr(store.failed_level(level->background) ? "build.style.failed" : "build.style.reading"));
      return;
    }
    it = m_styles.emplace(m_style_level, measure_style(*decor)).first;
  }
  const LevelStyle& s = it->second;
  if (s.floor.material >= 0) {
    ImGui::TextWrapped(tr("build.style.floor"), s.floor.texture.c_str(), s.floor.tile, s.bumps, s.bump_size);
    if (ImGui::Button(tr_id("build.style.place_floor").c_str())) start_style_placement(m_style_level, true);
    tooltip(tr("build.style.place_floor.tip"));
  }
  if (s.wall.material >= 0) {
    ImGui::TextWrapped(tr("build.style.wall"), s.wall.texture.c_str(), s.wall.tile);
    if (ImGui::Button(tr_id("build.style.place_wall").c_str())) start_style_placement(m_style_level, false);
    tooltip(tr("build.style.place_wall.tip"));
  }
  if (!s.valid()) ImGui::TextDisabled("%s", tr("build.style.none"));
  if (s.valid() && ImGui::Button(tr_id("build.style.terrain").c_str())) start_terrain_brush();
  tooltip(tr("build.style.terrain.tip"));
}

void App::start_style_placement(const std::string& level_name, bool floor, const std::string& shape) {
  auto it = m_styles.find(level_name);
  if (it == m_styles.end()) return;
  const LevelStyle::Surface& surface = floor ? it->second.floor : it->second.wall;
  const int material = style_material(level_name, floor);
  if (material < 0) return;
  const ShapePreset* chosen = shape.empty() ? nullptr : shape_preset(shape);
  ShapePreset preset = chosen ? *chosen : *shape_preset(floor ? "ground" : "wall");
  preset.params.tile = surface.tile;
  preset.params.color = surface.color;
  if (preset.params.kind == "ground") {
    preset.params.bumps = it->second.bumps;
    preset.params.bump_size = it->second.bump_size;
  }
  start_shape_placement(preset, false);
  placement.material = material;
}

}  // namespace ogle
