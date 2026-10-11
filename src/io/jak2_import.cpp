#include "io/jak2_import.h"

#include <algorithm>
#include <cmath>
#include <set>

#include "io/asset_library.h"

namespace ogle {

namespace {

bool prefixed(const std::string& s) { return s.rfind("jak2-", 0) == 0; }

bool pow2(int v) { return v > 0 && (v & (v - 1)) == 0; }

// the part of the document that does not come from Jak II
bool from_jak2(const Node& n) {
  auto it = n.extras.find("og_import");
  return it != n.extras.end() && it->is_object() && it->value("game", std::string()) == "jak2";
}

}  // namespace

bool jak2_structure(const CatalogPart& part) { return part.name.find("lowres") == std::string::npos; }

float mesh_lighting(const Mesh& mesh) {
  double sum = 0;
  size_t count = 0;
  for (const auto& p : mesh.prims)
    for (const auto& c : p.col) {
      sum += (c.x + c.y + c.z) / 3.0;
      count++;
    }
  return count ? (float)(sum / count) : 0.f;
}

float document_lighting(const Scene& doc) {
  double sum = 0;
  size_t count = 0;
  std::set<const Mesh*> seen;
  for (const auto& n : doc.nodes) {
    if (n.kind != NodeKind::Render || !n.mesh || from_jak2(n) || !seen.insert(n.mesh.get()).second) continue;
    for (const auto& p : n.mesh->prims)
      for (const auto& c : p.col) {
        sum += (c.x + c.y + c.z) / 3.0;
        count++;
      }
  }
  return count ? (float)(sum / count) : 0.f;
}

float lighting_ratio(const Mesh& part, const Scene& doc) {
  const float a = mesh_lighting(part), b = document_lighting(doc);
  return a > 0 && b > 0 ? a / b : 1.f;
}

std::shared_ptr<Mesh> scaled_colors(const Mesh& mesh, float k) {
  auto out = std::make_shared<Mesh>(mesh);
  for (auto& p : out->prims)
    for (auto& c : p.col) c = {std::clamp(c.x * k, 0.f, 1.f), std::clamp(c.y * k, 0.f, 1.f), std::clamp(c.z * k, 0.f, 1.f), c.w};
  out->touch();
  return out;
}

std::shared_ptr<Mesh> import_jak2_mesh(Scene& doc, const Mesh& mesh, const std::vector<Texture>& textures,
                                       const std::vector<Material>& materials, json* checks) {
  // the textures it draws: renamed unless the document has the very same image under that name
  std::set<int> used_materials, used_textures;
  for (const auto& p : mesh.prims)
    if (p.material >= 0 && p.material < (int)materials.size()) {
      used_materials.insert(p.material);
      if (materials[p.material].texture >= 0 && materials[p.material].texture < (int)textures.size())
        used_textures.insert(materials[p.material].texture);
    }
  std::vector<Texture> tex = textures;
  size_t renamed = 0, odd = 0;
  for (int ti : used_textures) {
    Texture& t = tex[ti];
    bool same = false;
    for (const auto& d : doc.textures)
      same |= d.name == t.name && d.width == t.width && d.height == t.height && d.rgba == t.rgba;
    if (!same && !prefixed(t.name)) {
      t.name = "jak2-" + t.name;
      renamed++;
    }
    if (!pow2(t.width) || !pow2(t.height) || t.width > 256 || t.height > 256) odd++;
  }
  std::vector<Material> mats = materials;
  size_t blended = 0;
  for (int mi : used_materials) {
    if (!prefixed(mats[mi].name)) mats[mi].name = "jak2-" + mats[mi].name;
    blended += mats[mi].alpha == AlphaMode::Blend;
  }
  auto out = merge_mesh(doc, mesh, tex, mats);
  if (!prefixed(out->name)) out->name = "jak2-" + out->name;
  out->extras.erase("og_terrain");

  if (checks) {
    size_t tris = 0, bad = 0, no_uv = 0, no_color = 0;
    for (const auto& p : mesh.prims) {
      tris += p.tri_count();
      for (auto i : p.idx) bad += i >= p.pos.size();
      const bool textured = p.material >= 0 && p.material < (int)materials.size() && materials[p.material].texture >= 0;
      no_uv += textured && p.uv.size() != p.pos.size();
      no_color += p.col.size() != p.pos.size();
    }
    float colors_out = 0;  // vertex colors outside 0..1
    for (const auto& p : mesh.prims)
      for (const auto& c : p.col) colors_out += c.x < 0 || c.y < 0 || c.z < 0 || c.x > 1 || c.y > 1 || c.z > 1;
    const float ratio = lighting_ratio(mesh, doc);
    *checks = json::array({
        {{"check", "mesh"}, {"ok", bad == 0 && no_uv == 0}, {"triangles", tris}, {"bad_indices", bad}, {"no_uv", no_uv}},
        {{"check", "colors"}, {"ok", colors_out == 0 && no_color == 0}, {"without", no_color}, {"outside", (size_t)colors_out}},
        {{"check", "lighting"}, {"ok", ratio >= 0.75f && ratio <= 1.f / 0.75f}, {"ratio", std::round(ratio * 100) / 100}},
        {{"check", "materials"}, {"ok", true}, {"count", used_materials.size()}, {"blended", blended}},
        {{"check", "textures"}, {"ok", odd == 0}, {"count", used_textures.size()}, {"renamed", renamed}, {"odd", odd}},
        {{"check", "metadata"}, {"ok", true}},
    });
  }
  return out;
}

}  // namespace ogle
