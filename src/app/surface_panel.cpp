// The inspector's sections of a surface (panels.cpp shows them): the size of a shape made in the
// editor (scene/shapes.h), and the materials of any piece: an image file, or a texture of the games
// (the browser: a game, a level, a name). A change gives the pieces selected a new material and
// copies of their meshes: the other pieces drawing the same material keep it, and undo puts the
// meshes back. Their color is the game's (not edited: the game's materials are drawn twice as
// bright, their vertex colors at half).

#include <algorithm>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "imgui.h"
#include "imgui_stdlib.h"
#include "io/jak2_import.h"
#include "scene/relight.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::IsItemHovered(ImGuiHoveredFlags_DelayShort) && text && *text) ImGui::SetTooltip("%s", text);
}

std::string lower(std::string s) {
  for (auto& c : s) c = (char)std::tolower((unsigned char)c);
  return s;
}

// the materials a mesh draws, in the order of its pieces
std::vector<int> mesh_materials(const Mesh& mesh) {
  std::vector<int> out;
  for (const auto& p : mesh.prims)
    if (std::find(out.begin(), out.end(), p.material) == out.end()) out.push_back(p.material);
  return out;
}

}  // namespace

void App::ui_shape_inspector(Node& n) {
  ShapeParams p;
  if (n.kind != NodeKind::Render || !node_shape(n, &p)) return;
  ImGui::SeparatorText(tr("inspector.shape"));
  const Node snap = n;
  bool changed = false;
  ImGui::PushItemWidth(-90);
  changed |= ImGui::DragFloat3(tr_id("inspector.shape.size").c_str(), &p.size.x, 0.05f, 0.05f, 2000.f, "%.2f m");
  edit_begin(snap);
  edit_end(tr("undo.shape"));
  tooltip(tr("inspector.shape.size.tip"));
  changed |= ImGui::DragFloat(tr_id("inspector.shape.tile").c_str(), &p.tile, 0.05f, 0.1f, 100.f, "%.2f m");
  edit_begin(snap);
  edit_end(tr("undo.shape"));
  tooltip(tr("inspector.shape.tile.tip"));
  if (p.kind == "stairs") {
    changed |= ImGui::SliderInt(tr_id("inspector.shape.steps").c_str(), &p.steps, 1, 64);
    edit_begin(snap);
    edit_end(tr("undo.shape"));
  }
  if (p.kind == "cylinder") {
    changed |= ImGui::SliderInt(tr_id("inspector.shape.sides").c_str(), &p.sides, 3, 64);
    edit_begin(snap);
    edit_end(tr("undo.shape"));
  }
  if (p.kind == "doorway" || p.kind == "window") {
    changed |= ImGui::DragFloat2(tr_id("inspector.shape.opening").c_str(), &p.opening.x, 0.05f, 0.1f, 2000.f, "%.2f m");
    edit_begin(snap);
    edit_end(tr("undo.shape"));
    tooltip(tr("inspector.shape.opening.tip"));
  }
  ImGui::PopItemWidth();
  if (changed) reshape(scene, n, ShapeParams::from_json(p.to_json()));
  // stretched with the scale tool: its texture too; the scale made its size keeps the texture's
  if (n.local.s.x != 1.f || n.local.s.y != 1.f || n.local.s.z != 1.f) {
    if (ImGui::Button(tr_id("inspector.shape.apply_scale").c_str(), ImVec2(-1, 0))) {
      p.size = {p.size.x * n.local.s.x, p.size.y * n.local.s.y, p.size.z * n.local.s.z};
      n.local.s = {1, 1, 1};
      reshape(scene, n, ShapeParams::from_json(p.to_json()));
      commit_node_edit(snap, tr("undo.shape"));
    }
    tooltip(tr("inspector.shape.apply_scale.tip"));
  }
}

void App::ui_texture_browser(const std::vector<uint32_t>& ids, int material, float width, float height,
                             bool choose_game) {
  const float top = ImGui::GetCursorPosY();  // `height` is the whole browser's
  // the game: Jak 3's textures, or Jak II's when it was extracted too (chosen here, or by the caller)
  if (library_jak2.empty() && !m_tex_game.empty()) {
    m_tex_game.clear();
    m_tex_level.clear();
  }
  if (choose_game && !library_jak2.empty()) {
    ImGui::AlignTextToFramePadding();
    ImGui::TextUnformatted(tr("textures.game"));
    ImGui::SameLine();
    if (ImGui::RadioButton("Jak 3##tex-game", m_tex_game.empty()) && !m_tex_game.empty()) {
      m_tex_game.clear();
      m_tex_level.clear();
    }
    ImGui::SameLine();
    if (ImGui::RadioButton("Jak II##tex-game", m_tex_game == "jak2") && m_tex_game != "jak2") {
      m_tex_game = "jak2";
      m_tex_level.clear();
    }
  }
  const bool jak2 = m_tex_game == "jak2";
  const LibraryIndex& index = jak2 ? library_index_jak2 : library_index;
  const std::vector<PaletteTexture>& every = jak2 ? game_textures_jak2 : game_textures;
  // a level of the other game chosen before: every level
  if (!m_tex_level.empty() && m_tex_level != "*" && index.ready()) {
    bool there = false;
    for (const auto& li : index.levels()) there |= li.level == m_tex_level;
    if (!there) m_tex_level.clear();
  }
  // a level (the textures its decor draws), every level, or the textures of the document
  const float combo_w = std::min(230.f, width * 0.5f);
  ImGui::SetNextItemWidth(combo_w);
  const std::string label = m_tex_level.empty()   ? tr("textures.all_levels")
                            : m_tex_level == "*" ? tr("textures.document")
                                                 : m_tex_level;
  if (ImGui::BeginCombo("##tex-level", label.c_str(), ImGuiComboFlags_HeightLarge)) {
    if (ImGui::Selectable(tr("textures.all_levels"), m_tex_level.empty())) m_tex_level.clear();
    if (ImGui::Selectable(tr("textures.document"), m_tex_level == "*")) m_tex_level = "*";
    if (index.ready())
      for (const auto& li : index.levels())
        if (!li.textures.empty() && ImGui::Selectable(strf("%s (%zu)", li.level.c_str(), li.textures.size()).c_str(),
                                                      m_tex_level == li.level))
          m_tex_level = li.level;
    ImGui::EndCombo();
  }
  tooltip(tr("textures.level.tip"));
  ImGui::SameLine();
  ImGui::SetNextItemWidth(std::max(80.f, width - combo_w - ImGui::GetStyle().ItemSpacing.x));
  ImGui::InputTextWithHint("##tex-filter", tr("textures.filter"), &m_tex_filter);
  const std::string f = lower(m_tex_filter);

  // what is shown: the document's textures (by index), or textures of the game
  std::vector<int> mine;
  std::vector<const PaletteTexture*> shown;
  size_t total = 0;
  static std::vector<PaletteTexture> of_level;  // the textures of the level chosen
  static std::string of_level_key;
  if (m_tex_level == "*") {
    total = scene.textures.size();
    for (int i = 0; i < (int)scene.textures.size(); i++)
      if (f.empty() || lower(scene.textures[i].name).find(f) != std::string::npos) mine.push_back(i);
  } else {
    if (!index.ready()) {
      ImGui::TextDisabled(tr("catalog.indexing"), index.done(), index.total());
      return;
    }
    const std::vector<PaletteTexture>* list = &every;
    if (!m_tex_level.empty()) {
      const std::string key = library_root + "/" + m_tex_game + "/" + m_tex_level;
      if (of_level_key != key) {
        of_level_key = key;
        of_level.clear();
        for (const auto& li : index.levels())
          if (li.level == m_tex_level)
            for (const auto& [name, n] : li.textures) {
              if (name.rfind("anim-slot", 0) == 0) continue;
              PaletteTexture t;
              t.name = name;
              t.game = m_tex_game;
              t.background = li.background;
              t.levels = 1;
              t.uses = n;
              of_level.push_back(std::move(t));
            }
        std::sort(of_level.begin(), of_level.end(),
                  [](const PaletteTexture& a, const PaletteTexture& b) { return a.uses != b.uses ? a.uses > b.uses : a.name < b.name; });
      }
      list = &of_level;
    }
    total = list->size();
    for (const auto& t : *list)
      if (f.empty() || t.name.find(f) != std::string::npos) shown.push_back(&t);
  }
  const size_t count = m_tex_level == "*" ? mine.size() : shown.size();
  ImGui::TextDisabled(tr("textures.count"), count, total);

  // the texture of the material chosen, marked
  std::string current;
  if (material >= 0 && material < (int)scene.materials.size()) {
    const int ti = scene.materials[material].texture;
    if (ti >= 0 && ti < (int)scene.textures.size()) current = scene.textures[ti].name;
  }
  auto give = [&](int ti) {
    if (ti < 0) return;
    NodeChange change(scene);
    for (auto id : ids) change.save(id);
    if (edit_material(scene, ids, material, [ti](Material& m) { m.texture = ti; })) history.push(change.commit(tr("undo.texture")));
  };
  const float size = 60.f;
  ImGui::BeginChild("tex-grid", ImVec2(width, std::max(120.f, height - (ImGui::GetCursorPosY() - top))),
                    ImGuiChildFlags_Borders);
  const float cell = size + 6 + ImGui::GetStyle().ItemSpacing.x;
  const int per_row = std::max(1, (int)((ImGui::GetContentRegionAvail().x + ImGui::GetStyle().ItemSpacing.x) / cell));
  ImGuiListClipper clipper;
  clipper.Begin((int)(count + per_row - 1) / per_row, size + 6 + ImGui::GetStyle().ItemSpacing.y);
  while (clipper.Step())
    for (int r = clipper.DisplayStart; r < clipper.DisplayEnd; r++)
      for (int c = 0; c < per_row; c++) {
        const size_t i = (size_t)r * per_row + c;
        if (i >= count) break;
        if (c) ImGui::SameLine();
        ImGui::PushID((int)i);
        const PaletteTexture* t = m_tex_level == "*" ? nullptr : shown[i];
        const std::string name = t ? (t->game.empty() ? t->name : t->game + "-" + t->name) : scene.textures[mine[i]].name;
        const unsigned tex = t ? palette.gl(*t) : renderer.gl_texture(scene, mine[i]);
        const bool on = !current.empty() && current == name;
        if (on) ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(0.22f, 0.6f, 0.66f, 1.f));
        ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(3, 3));
        const bool clicked = tex ? ImGui::ImageButton("##t", (ImTextureID)(uint64_t)tex, ImVec2(size, size))
                                 : ImGui::Button("...", ImVec2(size + 6, size + 6));
        ImGui::PopStyleVar();
        if (on) ImGui::PopStyleColor();
        if (ImGui::BeginItemTooltip()) {
          ImGui::TextUnformatted(name.c_str());
          if (!t) {
            ImGui::TextDisabled("%dx%d", scene.textures[mine[i]].width, scene.textures[mine[i]].height);
          } else if (m_tex_level.empty()) {
            ImGui::TextDisabled(tr("textures.used"), t->levels, t->uses);
          } else {
            ImGui::TextDisabled(tr("textures.used_level"), t->uses);
          }
          ImGui::EndTooltip();
        }
        // the texture into the document (read now if its image is not yet), then on the pieces
        if (clicked) give(t ? palette_texture(*t, true) : mine[i]);
        ImGui::PopID();
      }
  if (!count) ImGui::TextDisabled("%s", tr("textures.none"));
  ImGui::EndChild();
}

void App::ui_texture_window() {
  TextureWindow& w = m_texture_window;
  if (!w.open) return;
  // the pieces it was opened for, while they are there
  w.ids.erase(std::remove_if(w.ids.begin(), w.ids.end(), [&](uint32_t id) { return !scene.find(id); }), w.ids.end());
  if (w.ids.empty()) {
    w.open = false;
    return;
  }
  ImGui::SetNextWindowSize(ImVec2(640, 600), ImGuiCond_FirstUseEver);
  if (!ImGui::Begin(tr_id("textures.title").c_str(), &w.open)) {
    ImGui::End();
    return;
  }
  const Node* n = scene.find(w.ids.front());
  const std::vector<int> materials = n->mesh ? mesh_materials(*n->mesh) : std::vector<int>{};
  const int material = w.slot >= 0 && w.slot < (int)materials.size() ? materials[w.slot] : -1;
  const Material* m = material >= 0 && material < (int)scene.materials.size() ? &scene.materials[material] : nullptr;
  const Texture* t = m && m->texture >= 0 && m->texture < (int)scene.textures.size() ? &scene.textures[m->texture] : nullptr;
  ImGui::TextWrapped(tr("textures.for"), n->name.c_str(), t ? t->name.c_str() : tr("inspector.material.no_texture"));
  if (w.ids.size() > 1) ImGui::TextDisabled(tr("textures.for_more"), w.ids.size() - 1);
  ImGui::Separator();
  ui_texture_browser(w.ids, material, ImGui::GetContentRegionAvail().x, ImGui::GetContentRegionAvail().y);
  ImGui::End();
}

void App::ui_import_inspector(Node& n) {
  auto import = n.extras.find("og_import");
  if (n.kind != NodeKind::Render || !n.mesh || import == n.extras.end() || !import->is_object()) return;
  ImGui::SeparatorText(strf(tr("import.title"), import->value("level", std::string()).c_str()).c_str());
  tooltip(tr("import.tip"));
  const json checks = import->value("checks", json::array());
  for (const auto& c : checks) {
    if (!c.is_object()) continue;
    const std::string what = c.value("check", std::string());
    const bool good = c.value("ok", true);
    std::string text;
    if (what == "mesh")
      text = strf(tr("import.check.mesh"), c.value("triangles", (size_t)0), c.value("bad_indices", (size_t)0),
                  c.value("no_uv", (size_t)0));
    else if (what == "colors")
      text = strf(tr("import.check.colors"), c.value("without", (size_t)0), c.value("outside", (size_t)0));
    else if (what == "lighting" && !n.extras.value(kLitHere, false))  // lit where it stands: moot
      text = strf(tr("import.check.lighting"), c.value("ratio", 1.0));
    else if (what == "materials")
      text = strf(tr("import.check.materials"), c.value("count", (size_t)0), c.value("blended", (size_t)0));
    else if (what == "textures")
      text = strf(tr("import.check.textures"), c.value("count", (size_t)0), c.value("renamed", (size_t)0),
                  c.value("odd", (size_t)0));
    else if (what == "metadata")
      text = tr("import.check.metadata");
    else
      continue;
    ImGui::TextColored(good ? ImVec4(0.45f, 0.85f, 0.5f, 1.f) : ImVec4(1.f, 0.65f, 0.3f, 1.f), "%s", good ? "ok" : "!");
    ImGui::SameLine();
    ImGui::TextWrapped("%s", text.c_str());
    if (what == "lighting" && !good) {
      // its vertex colors scaled to the brightness of the document's Jak 3 decor
      if (ImGui::Button(tr_id("import.match_lighting").c_str())) {
        const float ratio = lighting_ratio(*n.mesh, scene);
        if (ratio > 0) {
          const Node before = n;
          n.mesh = scaled_colors(*n.mesh, 1.f / ratio);
          for (auto& k : n.extras["og_import"]["checks"])
            if (k.value("check", std::string()) == "lighting") {
              k["ok"] = true;
              k["ratio"] = 1.0;
            }
          commit_node_edit(before, tr("undo.match_lighting"));
        }
      }
      tooltip(tr("import.match_lighting.tip"));
    }
  }
}

void App::ui_material_inspector(Node& n) {
  if (n.kind != NodeKind::Render || !n.mesh) return;
  // every selected piece drawing the same material changes with it
  std::vector<uint32_t> targets(selection.begin(), selection.end());
  if (std::find(targets.begin(), targets.end(), n.id) == targets.end()) targets.push_back(n.id);
  const std::vector<int> materials = mesh_materials(*n.mesh);
  ImGui::SeparatorText(tr("inspector.material"));
  // its light: the game's palettes of where it stands (done when it is placed or moved, or asked)
  if (ImGui::Button(tr_id("inspector.relight").c_str(), ImVec2(-1, 0))) relight_selection();
  guide_tip("relight");
  constexpr size_t kShown = 12;
  for (size_t slot = 0; slot < materials.size() && slot < kShown; slot++) {
    const int mi = materials[slot];
    const Material* m = mi >= 0 && mi < (int)scene.materials.size() ? &scene.materials[mi] : nullptr;
    ImGui::PushID((int)slot);
    const Texture* t = m && m->texture >= 0 && m->texture < (int)scene.textures.size() ? &scene.textures[m->texture] : nullptr;
    if (t) ImGui::TextWrapped("%s  (%dx%d)", t->name.c_str(), t->width, t->height);
    else ImGui::TextDisabled("%s", tr("inspector.material.no_texture"));
    if (ImGui::Button(tr_id("inspector.material.image").c_str())) {
      m_texture_target = {targets, mi};
      open_dialog(DialogAction::TextureImage);
    }
    tooltip(tr("inspector.material.image.tip"));
    ImGui::SameLine();
    // a texture of the games: the window chooses it (a game, a level, a name)
    const bool open = m_texture_window.open && m_texture_window.slot == (int)slot && m_texture_window.ids == targets;
    if (open) ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(0.22f, 0.6f, 0.66f, 1.f));
    if (ImGui::Button(tr_id("inspector.material.textures").c_str())) m_texture_window = {true, targets, (int)slot};
    if (open) ImGui::PopStyleColor();
    tooltip(tr("inspector.material.textures.tip"));
    ImGui::PopID();
    if (slot + 1 < materials.size()) ImGui::Separator();
  }
  if (materials.size() > kShown) ImGui::TextDisabled(tr("inspector.material.more"), materials.size() - kShown);
}

}  // namespace ogle
