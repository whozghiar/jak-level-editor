#include "scene/relight.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <string>
#include <unordered_map>
#include <unordered_set>

namespace ogle {

namespace {

constexpr float kCell = 2.f;                         // meters: the grid of the level's vertices
constexpr float kRadii[] = {2.f, 4.f, 8.f, 16.f};    // searched in turn until enough are found
constexpr size_t kEnough = 4;

uint64_t cell_key(int x, int y, int z) {
  return ((uint64_t)(uint32_t)(x + 1048576) & 0x1fffff) | (((uint64_t)(uint32_t)(y + 1048576) & 0x1fffff) << 21) |
         (((uint64_t)(uint32_t)(z + 1048576) & 0x1fffff) << 42);
}

// a lit vertex of the level: where it is, the way it faces, its 8 palettes
struct Sample {
  Vec3 p, n;
  const uint8_t* rgba;
};

// the targets: the Render nodes among ids and under them, not an actor's model
std::vector<size_t> target_indices(const Scene& scene, const std::vector<uint32_t>& ids) {
  const SceneCache& cache = scene.cache();
  std::vector<size_t> out;
  std::unordered_set<uint32_t> seen;
  auto add = [&](uint32_t id) {
    if (!seen.insert(id).second) return;
    const int i = scene.index_of(id);
    if (i < 0) return;
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || ((size_t)i < cache.actor.size() && cache.actor[i])) return;
    out.push_back((size_t)i);
  };
  for (auto id : ids) add(id);
  for (const auto& below : scene.descendants_of(ids))
    for (auto id : below) add(id);
  return out;
}

}  // namespace

std::vector<uint32_t> lit_here_nodes(const Scene& scene, const std::vector<uint32_t>& ids) {
  std::vector<uint32_t> out;
  for (size_t i : target_indices(scene, ids))
    if (scene.nodes[i].extras.value(kLitHere, false)) out.push_back(scene.nodes[i].id);
  return out;
}

size_t light_like_surroundings(Scene& scene, const std::vector<uint32_t>& ids, size_t max_vertices) {
  scene.begin_frame();  // the world matrices after the last changes
  const SceneCache& cache = scene.cache();
  const std::vector<size_t> targets = target_indices(scene, ids);
  if (targets.empty()) return 0;
  size_t vertices = 0;
  AABB region;
  std::unordered_set<size_t> is_target(targets.begin(), targets.end());
  for (size_t t : targets) {
    for (const auto& p : scene.nodes[t].mesh->prims) vertices += p.pos.size();
    if (t < cache.bounds.size() && cache.bounds[t].valid()) region.add(cache.bounds[t]);
  }
  if (!region.valid() || vertices > max_vertices) return 0;
  region = region.expanded(kRadii[3]);

  // the lit vertices of the level around them
  std::vector<Sample> samples;
  std::unordered_map<uint64_t, std::vector<uint32_t>> grid;
  const size_t count = std::min(scene.nodes.size(), cache.visible.size());
  for (size_t i = 0; i < count; i++) {
    if (is_target.count(i)) continue;
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || !n.tod || !cache.visible[i] || cache.actor[i]) continue;
    if (i < cache.reference.size() && cache.reference[i]) continue;
    if (i >= cache.bounds.size() || !cache.bounds[i].valid() || !cache.bounds[i].overlaps(region)) continue;
    if (!n.tod->fits(*n.mesh)) continue;
    const Mat4& w = cache.worlds[i];
    const auto& rgba = n.tod->palette->rgba;
    for (size_t pi = 0; pi < n.mesh->prims.size(); pi++) {
      const Primitive& p = n.mesh->prims[pi];
      for (size_t v = 0; v < p.pos.size(); v++) {
        const Vec3 q = w.point(p.pos[v]);
        if (!region.contains(q)) continue;
        Vec3 nrm{0, 0, 0};
        if (v < p.nrm.size()) {
          nrm = w.dir(p.nrm[v]);
          const float l = length(nrm);
          nrm = l > 1e-6f ? nrm * (1.f / l) : Vec3{0, 0, 0};
        }
        grid[cell_key((int)std::floor(q.x / kCell), (int)std::floor(q.y / kCell), (int)std::floor(q.z / kCell))]
            .push_back((uint32_t)samples.size());
        samples.push_back({q, nrm, &rgba[(size_t)n.tod->index[pi][v] * 32]});
      }
    }
  }
  if (samples.empty()) return 0;

  size_t lit = 0;
  for (size_t t : targets) {
    Node& n = scene.nodes[t];
    const Mat4& w = cache.worlds[t];
    auto palette = std::make_shared<TodPalette>();
    palette->local = true;
    auto tod = std::make_shared<TodColors>();
    std::unordered_map<std::string, uint16_t> colors;
    std::vector<std::vector<float>> pending;  // the vertices with no sample near: the node's average
    size_t found = 0, all = 0;
    double sum[32] = {};
    tod->index.resize(n.mesh->prims.size());
    std::vector<std::vector<int>> missing(n.mesh->prims.size());
    for (size_t pi = 0; pi < n.mesh->prims.size(); pi++) {
      const Primitive& p = n.mesh->prims[pi];
      tod->index[pi].assign(p.pos.size(), 0);
      for (size_t v = 0; v < p.pos.size(); v++) {
        all++;
        const Vec3 q = w.point(p.pos[v]);
        Vec3 nrm{0, 0, 0};
        if (v < p.nrm.size()) {
          nrm = w.dir(p.nrm[v]);
          const float l = length(nrm);
          nrm = l > 1e-6f ? nrm * (1.f / l) : Vec3{0, 0, 0};
        }
        // the samples around, the nearer and the more alike in facing the more
        double acc[32] = {}, total = 0;
        size_t near = 0;
        for (float r : kRadii) {
          std::fill(std::begin(acc), std::end(acc), 0.0);
          total = 0;
          near = 0;
          const int x0 = (int)std::floor((q.x - r) / kCell), x1 = (int)std::floor((q.x + r) / kCell);
          const int y0 = (int)std::floor((q.y - r) / kCell), y1 = (int)std::floor((q.y + r) / kCell);
          const int z0 = (int)std::floor((q.z - r) / kCell), z1 = (int)std::floor((q.z + r) / kCell);
          for (int x = x0; x <= x1; x++)
            for (int y = y0; y <= y1; y++)
              for (int z = z0; z <= z1; z++) {
                auto it = grid.find(cell_key(x, y, z));
                if (it == grid.end()) continue;
                for (uint32_t s : it->second) {
                  const Sample& sm = samples[s];
                  const float d2 = length2(sm.p - q);
                  if (d2 > r * r) continue;
                  float facing = 1.f;
                  if (length2(nrm) > 0 && length2(sm.n) > 0) facing = std::max(dot(nrm, sm.n), 0.f) + 0.05f;
                  const double wgt = (double)(facing * facing) / (d2 + 0.5);
                  for (int k = 0; k < 32; k++) acc[k] += wgt * sm.rgba[k];
                  total += wgt;
                  near++;
                }
              }
          if (near >= kEnough && total > 0) break;
        }
        if (!near || total <= 0) {
          missing[pi].push_back((int)v);
          continue;
        }
        found++;
        uint8_t col[32];
        for (int k = 0; k < 32; k++) {
          const double c = acc[k] / total;
          sum[k] += c;
          col[k] = (uint8_t)std::clamp((int)std::lround(c), 0, 255);
        }
        auto [it, fresh] = colors.emplace(std::string((const char*)col, 32), (uint16_t)colors.size());
        if (fresh) palette->rgba.insert(palette->rgba.end(), col, col + 32);
        tod->index[pi][v] = it->second;
      }
    }
    // too far from the level's lit decor: it keeps its own light
    if (!found || found * 2 < all || colors.size() >= 65535) continue;
    bool any_missing = false;
    for (const auto& m : missing) any_missing |= !m.empty();
    if (any_missing) {
      uint8_t avg[32];
      for (int k = 0; k < 32; k++) avg[k] = (uint8_t)std::clamp((int)std::lround(sum[k] / (double)found), 0, 255);
      auto [it, fresh] = colors.emplace(std::string((const char*)avg, 32), (uint16_t)colors.size());
      if (fresh) palette->rgba.insert(palette->rgba.end(), avg, avg + 32);
      for (size_t pi = 0; pi < missing.size(); pi++)
        for (int v : missing[pi]) tod->index[pi][v] = it->second;
    }
    tod->palette = palette;
    // its colors in the editor's own view, as the extracted levels': each channel's brightest palette
    // (the editor's materials, at 1 where the game's are at 2, twice that)
    auto mesh = std::make_shared<Mesh>(*n.mesh);
    for (size_t pi = 0; pi < mesh->prims.size(); pi++) {
      Primitive& p = mesh->prims[pi];
      const Material* mat = p.material >= 0 && p.material < (int)scene.materials.size() ? &scene.materials[p.material] : nullptr;
      const float k = mat && mat->base_color.x <= 1.5f ? 2.f : 1.f;
      const bool had = p.col.size() == p.pos.size();
      p.col.resize(p.pos.size(), Vec4{1, 1, 1, 0.5f});
      for (size_t v = 0; v < p.pos.size(); v++) {
        const uint8_t* c = &palette->rgba[(size_t)tod->index[pi][v] * 32];
        float best[3] = {0, 0, 0};
        for (int pal = 0; pal < 8; pal++)
          for (int ch = 0; ch < 3; ch++) best[ch] = std::max(best[ch], (float)c[pal * 4 + ch]);
        p.col[v] = {std::min(1.f, best[0] / 255.f * k), std::min(1.f, best[1] / 255.f * k),
                    std::min(1.f, best[2] / 255.f * k), had ? p.col[v].w : 0.5f};
      }
    }
    mesh->touch();
    n.mesh = mesh;
    n.tod = tod;
    n.extras[kLitHere] = true;
    lit++;
  }
  return lit;
}

}  // namespace ogle
