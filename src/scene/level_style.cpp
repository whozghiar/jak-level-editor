#include "scene/level_style.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <string>
#include <map>
#include <unordered_map>
#include <vector>

namespace ogle {

namespace {

struct Bin {
  double area = 0;
  double log_tile = 0;  // area-weighted
  double tile_area = 0;
  Vec4 color{0, 0, 0, 0};  // area-weighted
};

}  // namespace

LevelStyle measure_style(Scene& level) {
  const SceneCache& cache = level.cache();
  // the surfaces of water in the decor (a canal's, a pool's: a texture of water; the streams and
  // the sea are actors, not here): the ground under them is a bed, not the ground one walks on.
  // Their height on a grid of 4 m cells.
  auto is_water = [&](const Material& m) {
    std::string name = m.texture >= 0 && m.texture < (int)level.textures.size() ? level.textures[m.texture].name : m.name;
    for (char& ch : name) ch = (char)std::tolower((unsigned char)ch);
    return name.find("water") != std::string::npos;
  };
  constexpr float kCell = 4.f;
  std::unordered_map<uint64_t, float> water;
  auto cell_of = [&](const Vec3& v) {
    return ((uint64_t)(uint32_t)(int)std::floor(v.x / kCell) << 32) | (uint32_t)(int)std::floor(v.z / kCell);
  };
  for (size_t i = 0; i < level.nodes.size(); i++) {
    const Node& n = level.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || cache.actor[i]) continue;
    const Mat4& w = cache.worlds[i];
    for (const auto& p : n.mesh->prims) {
      if (p.material < 0 || p.material >= (int)level.materials.size() || !is_water(level.materials[p.material])) continue;
      for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
        const Vec3 pa = w.point(p.pos[p.idx[t]]), pb = w.point(p.pos[p.idx[t + 1]]), pc = w.point(p.pos[p.idx[t + 2]]);
        const Vec3 cr = cross(pb - pa, pc - pa);
        const float len = length(cr);
        if (len < 1e-4f || std::fabs(cr.y) < 0.95f * len) continue;
        // every cell the triangle covers (its box)
        const float x0 = std::min({pa.x, pb.x, pc.x}), x1 = std::max({pa.x, pb.x, pc.x});
        const float z0 = std::min({pa.z, pb.z, pc.z}), z1 = std::max({pa.z, pb.z, pc.z});
        if ((x1 - x0) * (z1 - z0) > 4096.f * kCell * kCell) continue;
        const float y = (pa.y + pb.y + pc.y) / 3.f;
        for (float x = x0; x <= x1 + kCell; x += kCell)
          for (float z = z0; z <= z1 + kCell; z += kCell) {
            auto [it, fresh] = water.emplace(cell_of({std::min(x, x1), 0, std::min(z, z1)}), y);
            if (!fresh) it->second = std::max(it->second, y);
          }
      }
    }
  }
  auto under_water = [&](const Vec3& v) {
    auto it = water.find(cell_of(v));
    return it != water.end() && v.y < it->second && v.y > it->second - 20.f;
  };
  // the floors: the level's ground (its terrain pieces) when it has some, not its roofs
  // by material; the walls of the level's shell (its pieces) apart from those of its instanced decor
  // (the trees' sides, the crates')
  std::map<int, Bin> floors, ground, walls, shell_walls, ground_walls;
  double tilt_area[2] = {0, 0}, tilt_sum[2] = {0, 0};
  std::vector<float> edges[2];
  for (size_t i = 0; i < level.nodes.size(); i++) {
    const Node& n = level.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || cache.actor[i]) continue;
    const Mat4& w = cache.worlds[i];
    const int terrain = n.extras.value("og_terrain", false) ? 1 : 0;
    const bool shell = n.extras.contains("og_terrain");  // a piece of the level's shell (tfrag)
    for (const auto& p : n.mesh->prims) {
      if (p.material < 0 || p.material >= (int)level.materials.size()) continue;
      const Material& m = level.materials[p.material];
      // the decor's own surfaces, those the game blends over the rest too (the forest's grass, drawn
      // in its translucent tree), not its glows and light shafts (added or taken away)
      if (m.texture < 0 || (m.alpha == AlphaMode::Blend && m.blend != BlendMode::Alpha)) continue;
      const bool uv = p.uv.size() == p.pos.size(), col = p.col.size() == p.pos.size();
      for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
        const uint32_t a = p.idx[t], b = p.idx[t + 1], c = p.idx[t + 2];
        const Vec3 pa = w.point(p.pos[a]), pb = w.point(p.pos[b]), pc = w.point(p.pos[c]);
        const Vec3 cr = cross(pb - pa, pc - pa);
        const float area = length(cr) * 0.5f;
        if (area < 1e-4f) continue;
        const Vec3 nr = cr * (0.5f / area);
        const float up = std::fabs(nr.y);
        if (up > 0.85f && under_water((pa + pb + pc) * (1.f / 3.f))) continue;  // a bed under water
        // the steep faces: of the ground itself (its banks and cliffs) first, else of the shell
        Bin* bin = up > 0.85f   ? &(terrain ? ground : floors)[p.material]
                   : up < 0.7f  ? &(terrain ? ground_walls : shell ? shell_walls : walls)[p.material]
                                : nullptr;
        if (up > 0.5f) {
          // the ground's unevenness: how far its faces tilt, how large they are
          tilt_area[terrain] += area;
          tilt_sum[terrain] += area * std::acos(std::min(up, 1.f));
          if (edges[terrain].size() < 200000) edges[terrain].push_back(std::sqrt(area * 2.f));
        }
        if (!bin) continue;
        bin->area += area;
        if (uv) {
          const Vec2 ua = p.uv[a], ub = p.uv[b], uc = p.uv[c];
          const float uv_area = std::fabs((ub.x - ua.x) * (uc.y - ua.y) - (uc.x - ua.x) * (ub.y - ua.y)) * 0.5f;
          if (uv_area > 1e-6f) {
            const float tile = std::sqrt(area / uv_area);
            if (tile > 0.05f && tile < 200.f) {
              bin->log_tile += area * std::log(tile);
              bin->tile_area += area;
            }
          }
        }
        if (col) {
          const Vec4 cc = (p.col[a] + p.col[b] + p.col[c]) * (1.f / 3.f);
          bin->color = bin->color + cc * area;
        }
      }
    }
  }
  LevelStyle s;
  // by texture: a texture drawn by several materials (the forest's grass, split among four) counts
  // as one, its material the one drawing the most of it
  auto by_texture = [&](const std::map<int, Bin>& bins) {
    std::map<int, Bin> merged;    // the texture (else -1 - material) -> its bins summed
    std::map<int, std::pair<double, int>> main;  // -> the material drawing the most of it
    for (const auto& [mat, b] : bins) {
      const int t = level.materials[mat].texture >= 0 ? level.materials[mat].texture : -1 - mat;
      Bin& m = merged[t];
      m.area += b.area;
      m.log_tile += b.log_tile;
      m.tile_area += b.tile_area;
      m.color = m.color + b.color;
      auto& best = main[t];
      if (b.area > best.first) best = {b.area, mat};
    }
    std::map<int, Bin> out;
    for (const auto& [t, b] : merged) out[main[t].second] = b;
    return out;
  };
  // what a floor or a wall of a new place cannot be: a texture mostly see-through (an overlay the
  // game lays over the ground: its alpha under a quarter), a black filler, and by its name a pipe,
  // a cable, a trim, a roof, a bridge's planks... (only while something else is there)
  std::map<int, bool> see_through;
  auto overlay = [&](int m) {
    const int t = level.materials[m].texture;
    if (t < 0 || t >= (int)level.textures.size()) return false;
    auto it = see_through.find(t);
    if (it != see_through.end()) return it->second;
    const auto& px = level.textures[t].rgba;
    double alpha = 0;
    for (size_t k = 3; k < px.size(); k += 4) alpha += px[k];
    const bool thin = !px.empty() && alpha / (px.size() / 4) < 0.25 * 128;  // the PS2's 128: opaque
    return see_through[t] = thin;
  };
  auto unwanted = [&](int m, bool floor) {
    if (overlay(m)) return true;
    const Material& mat = level.materials[m];
    std::string name = mat.texture >= 0 && mat.texture < (int)level.textures.size() ? level.textures[mat.texture].name : mat.name;
    for (char& ch : name) ch = (char)std::tolower((unsigned char)ch);
    static const char* const kNever[] = {"black", "pipe", "tube", "cable", "bark", "trim", "rim", "bridge",
                                         "roof", "overhang", "tent", "glass", "awning", "window", "light",
                                         "screen", "monitor", "skull", "siding", "door", "stand", "beam"};
    for (const char* w : kNever)
      if (name.find(w) != std::string::npos) return true;
    return floor && name.find("wall") != std::string::npos;
  };
  // `strict`: only what is not unwanted; else anything not an overlay
  auto pick = [&](const std::map<int, Bin>& all, int skip, LevelStyle::Surface* out, bool strict) {
    const bool floor = skip < 0;
    std::map<int, Bin> per_material;
    for (const auto& [m, b] : all)
      if (strict ? !unwanted(m, floor) : !overlay(m)) per_material[m] = b;
    const int skip_texture = skip >= 0 ? level.materials[skip].texture : -2;
    for (const auto& [mat, b] : by_texture(per_material)) {
      if (mat == skip || (skip_texture >= 0 && level.materials[mat].texture == skip_texture) || b.area <= out->area)
        continue;
      out->material = mat;
      out->area = b.area;
      out->tile = b.tile_area > 0 ? (float)std::exp(b.log_tile / b.tile_area) : 2.f;
      out->color = b.color.w > 0 ? b.color * (float)(1.0 / b.area) : Vec4{0.5f, 0.5f, 0.5f, 1.f};
      out->color.w = 1.f;
      // its hue, not the darkness of a cave or a temple's halls (their lamps light them in the game,
      // a new place has none): the brightest channel at least 0.3 (0.5: the texture as it is)
      const float bright = std::max({out->color.x, out->color.y, out->color.z});
      if (bright < 0.3f) {
        const float k = bright > 1e-3f ? 0.3f / bright : 0.f;
        out->color = bright > 1e-3f ? Vec4{out->color.x * k, out->color.y * k, out->color.z * k, 1.f} : Vec4{0.3f, 0.3f, 0.3f, 1.f};
      }
    }
    if (out->material >= 0) {
      const Material& m = level.materials[out->material];
      out->texture = m.texture >= 0 && m.texture < (int)level.textures.size() ? level.textures[m.texture].name : m.name;
    }
  };
  // the ground, then the other floors; what is unwanted only when nothing else is
  for (bool strict : {true, false})
    for (const auto* bins : {&ground, &floors})
      if (s.floor.material < 0) pick(*bins, -1, &s.floor, strict);
  // the slopes and walls of a terrain: the level's ground's own banks and cliffs, else the walls of
  // its shell, else of its decor (not the trees' sides when there is better)
  // a texture drawn mostly flat (a floor's, on the edge of a bank) is not one of walls
  std::map<int, double> flat_by_texture, steep_by_texture;
  for (const auto* bins : {&ground, &floors})
    for (const auto& [m, b] : *bins) flat_by_texture[level.materials[m].texture] += b.area;
  for (const auto* bins : {&ground_walls, &shell_walls, &walls})
    for (const auto& [m, b] : *bins) steep_by_texture[level.materials[m].texture] += b.area;
  // nor a roof's (the farms' market roofs set in their ground are steep, not slopes of it)
  auto roof = [&](int m) {
    const Material& mat = level.materials[m];
    std::string name = mat.texture >= 0 && mat.texture < (int)level.textures.size() ? level.textures[mat.texture].name : mat.name;
    for (char& ch : name) ch = (char)std::tolower((unsigned char)ch);
    return name.find("roof") != std::string::npos;
  };
  auto steep_only = [&](const std::map<int, Bin>& bins) {
    std::map<int, Bin> out;
    for (const auto& [m, b] : bins) {
      const int t = level.materials[m].texture;
      if (flat_by_texture[t] <= steep_by_texture[t] && !roof(m)) out[m] = b;
    }
    return out;
  };
  const auto gw = steep_only(ground_walls), sw = steep_only(shell_walls), aw = steep_only(walls);
  double ground_walls_area = 0;
  for (const auto& [m, b] : gw) ground_walls_area += m == s.floor.material ? 0 : b.area;
  const std::map<int, Bin>* wall_sources[] = {ground_walls_area > 200 ? &gw : &sw, &sw, &aw, &walls};
  for (bool strict : {true, false})
    for (const auto* bins : wall_sources)
      if (s.wall.material < 0) pick(*bins, s.floor.material, &s.wall, strict);
  const int g = tilt_area[1] > 0 ? 1 : 0;
  if (tilt_area[g] > 0 && !edges[g].empty()) {
    auto& e = edges[g];
    std::nth_element(e.begin(), e.begin() + e.size() / 2, e.end());
    const float edge = std::clamp(e[e.size() / 2], 0.5f, 6.f);
    const float tilt = (float)(tilt_sum[g] / tilt_area[g]);
    s.bump_size = edge * 2.f;
    s.bumps = std::clamp(std::tan(tilt) * edge * 0.5f, 0.f, 1.5f);
  }
  return s;
}

}  // namespace ogle
