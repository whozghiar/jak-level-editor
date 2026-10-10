// The inspector's sections of a surface (panels.cpp shows them): the size of a shape made in the
// editor (scene/shapes.h), and the materials of any piece: an image file or a texture of the
// document, a color. A change gives the pieces selected a new material and copies of their meshes:
// the other pieces drawing the same material keep it, and undo puts the meshes back.

#include <algorithm>

#include "app/app.h"
#include "core/i18n.h"
#include "imgui.h"
#include "imgui_stdlib.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::IsItemHovered(ImGuiHoveredFlags_DelayShort) && text && *text) ImGui::SetTooltip("%s", text);
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

void App::ui_material_inspector(Node& n) {
  if (n.kind != NodeKind::Render || !n.mesh) return;
  // every selected piece drawing the same material changes with it
  std::vector<uint32_t> targets(selection.begin(), selection.end());
  if (std::find(targets.begin(), targets.end(), n.id) == targets.end()) targets.push_back(n.id);
  const std::vector<int> materials = mesh_materials(*n.mesh);
  ImGui::SeparatorText(tr("inspector.material"));
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
    if (ImGui::Button(tr_id("inspector.material.textures").c_str())) ImGui::OpenPopup("textures");
    tooltip(tr("inspector.material.textures.tip"));
    if (ImGui::BeginPopup("textures")) {
      static std::string filter;
      ImGui::SetNextItemWidth(320);
      if (ImGui::IsWindowAppearing()) ImGui::SetKeyboardFocusHere();
      ImGui::InputTextWithHint("##filter", tr("inspector.material.filter"), &filter);
      std::string f = filter;
      for (auto& c : f) c = (char)std::tolower((unsigned char)c);
      std::vector<int> shown;
      for (int i = 0; i < (int)scene.textures.size(); i++) {
        std::string name = scene.textures[i].name;
        for (auto& c : name) c = (char)std::tolower((unsigned char)c);
        if (f.empty() || name.find(f) != std::string::npos) shown.push_back(i);
      }
      ImGui::BeginChild("list", ImVec2(320, 300));
      ImGuiListClipper clip;
      clip.Begin((int)shown.size());
      while (clip.Step())
        for (int k = clip.DisplayStart; k < clip.DisplayEnd; k++) {
          const Texture& tx = scene.textures[shown[k]];
          const std::string label = tx.name + "  (" + std::to_string(tx.width) + "x" + std::to_string(tx.height) + ")##" +
                                    std::to_string(shown[k]);
          if (ImGui::Selectable(label.c_str())) {
            const int ti = shown[k];
            NodeChange c(scene);
            for (auto id : targets) c.save(id);
            if (edit_material(scene, targets, mi, [ti](Material& mat) { mat.texture = ti; }))
              history.push(c.commit(tr("undo.texture")));
            ImGui::CloseCurrentPopup();
          }
        }
      ImGui::EndChild();
      ImGui::EndPopup();
    }
    // its color: multiplies the texture (the game's own materials are at 2, their vertex colors at half)
    Vec4 color = m ? m->base_color : Vec4{1, 1, 1, 1};
    ImGui::SetNextItemWidth(-90);
    const bool edited = ImGui::ColorEdit4(tr_id("inspector.material.color").c_str(), &color.x,
                                          ImGuiColorEditFlags_Float | ImGuiColorEditFlags_HDR);
    if (ImGui::IsItemActivated() || (edited && !m_surface_edit)) {
      m_surface_edit = std::make_unique<NodeChange>(scene);
      for (auto id : targets) m_surface_edit->save(id);
    }
    if (edited) edit_material(scene, targets, mi, [color](Material& mat) { mat.base_color = color; });
    if (ImGui::IsItemDeactivated() && m_surface_edit) {
      if (ImGui::IsItemDeactivatedAfterEdit()) history.push(m_surface_edit->commit(tr("undo.color")));
      m_surface_edit.reset();
    }
    tooltip(tr("inspector.material.color.tip"));
    ImGui::PopID();
    if (slot + 1 < materials.size()) ImGui::Separator();
  }
  if (materials.size() > kShown) ImGui::TextDisabled(tr("inspector.material.more"), materials.size() - kShown);
}

}  // namespace ogle
