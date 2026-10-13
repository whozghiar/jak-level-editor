#include "scene/pick.h"

#include <algorithm>

#include "scene/bvh.h"

namespace ogle {

namespace {

bool excluded(const Scene& scene, const PickFilter& f, uint32_t id) {
  if (!f.exclude_list) return false;
  for (auto e : *f.exclude_list)
    if (id == e || scene.is_ancestor(e, id)) return true;
  return false;
}

// a glow or a light beam (the PS2's additive and subtractive blends): drawn faintly over what is
// behind it, so it does not take the click from it
bool glow(const Scene& scene, int material) {
  return material >= 0 && material < (int)scene.materials.size() &&
         scene.materials[material].alpha == AlphaMode::Blend && scene.materials[material].blend != BlendMode::Alpha;
}

// Every node the ray hits before `tmax`, its nearest point each, given to `hit` (which returns the
// distance the next nodes must beat). Glows go to `glow_hit` instead.
template <typename Hit, typename GlowHit>
void cast(Scene& scene, const Ray& ray, const PickFilter& f, Hit&& hit, GlowHit&& glow_hit, float tmax) {
  const SceneCache& cache = scene.cache();
  const bool references = cache.reference.size() == cache.count;
  for (size_t i = 0; i < scene.nodes.size() && i < cache.count; i++) {
    auto& n = scene.nodes[i];
    if (!cache.visible[i]) continue;
    if (references && cache.reference[i]) continue;  // a level shown to compare with: not part of the document
    if (n.locked && !f.include_locked) continue;
    const bool allowed = (n.kind == NodeKind::Render && f.render) || (n.kind == NodeKind::Collision && f.collision) ||
                         (n.kind == NodeKind::NavMesh && f.navmesh) || (n.kind == NodeKind::Actor && f.actors);
    if (!allowed) continue;
    const Mat4& w = cache.worlds[i];
    // cheap rejection first: most nodes of a level are far from the ray
    if (n.mesh && !ray_aabb(ray, cache.bounds[i].expanded(0.01f), tmax)) continue;
    if (excluded(scene, f, n.id)) continue;
    PickHit h;
    if (n.kind == NodeKind::Actor) {
      const float t = ray_sphere(ray, w.col3(3), kMarkerRadius * 1.3f);
      if (t > 0 && t < tmax) {
        h.node = n.id;
        h.t = t;
        h.point = ray.at(t);
        h.normal = {0, 1, 0};
        h.prim = -1;
        tmax = hit(h);
      }
      continue;
    }
    if (!n.mesh) continue;
    const Ray local = transform_ray(w.inverse(), ray);
    PickHit soft;
    for (size_t p = 0; p < n.mesh->prims.size(); p++) {
      const auto& prim = n.mesh->prims[p];
      // the see-through surfaces of the terrain (water, glass) do not hide what is behind them
      if (n.locked && prim.material >= 0 && prim.material < (int)scene.materials.size() &&
          scene.materials[prim.material].alpha == AlphaMode::Blend)
        continue;
      const bool is_glow = glow(scene, prim.material);
      PickHit& best = is_glow ? soft : h;
      float t;
      uint32_t tri;
      if (n.mesh->bvh(p).raycast(local, prim.pos, prim.idx, std::min(tmax, best.t), &t, &tri)) {
        const Vec3 a = w.point(prim.pos[prim.idx[tri * 3]]);
        const Vec3 b = w.point(prim.pos[prim.idx[tri * 3 + 1]]);
        const Vec3 c = w.point(prim.pos[prim.idx[tri * 3 + 2]]);
        Vec3 nrm = normalize(cross(b - a, c - a));
        if (dot(nrm, ray.d) > 0) nrm = -nrm;
        best.node = n.id;
        best.t = t;
        best.point = ray.at(t);
        best.normal = nrm;
        best.prim = (int)p;
        best.tri = tri;
      }
    }
    if (h.hit()) tmax = hit(h);
    else if (soft.hit()) glow_hit(soft);
  }
}

}  // namespace

PickHit pick(Scene& scene, const Ray& ray, const PickFilter& f) {
  PickHit best, glow_best;
  cast(
      scene, ray, f,
      [&](const PickHit& h) {
        if (h.t < best.t) best = h;
        return best.t;
      },
      [&](const PickHit& h) {
        if (h.t < glow_best.t) glow_best = h;
      },
      best.t);
  // a glow is chosen when nothing else is under it
  return best.hit() ? best : glow_best;
}

std::vector<PickHit> pick_all(Scene& scene, const Ray& ray, const PickFilter& f, size_t max) {
  std::vector<PickHit> hits, glows;
  cast(
      scene, ray, f,
      [&](const PickHit& h) {
        hits.push_back(h);
        return std::numeric_limits<float>::max();
      },
      [&](const PickHit& h) { glows.push_back(h); }, std::numeric_limits<float>::max());
  auto nearest = [](const PickHit& a, const PickHit& b) { return a.t < b.t; };
  std::sort(hits.begin(), hits.end(), nearest);
  std::sort(glows.begin(), glows.end(), nearest);
  // the glows after what they are in front of: they are reached by clicking again
  hits.insert(hits.end(), glows.begin(), glows.end());
  if (hits.size() > max) hits.resize(max);
  return hits;
}

float covered_below(Scene& scene, size_t index, float gap) {
  if (index >= scene.nodes.size() || !scene.nodes[index].mesh) return 0.f;
  const Node& n = scene.nodes[index];
  const Mat4 w = scene.cache().worlds[index];
  // up to 64 points, one every 64th of the flat area: the middle of the triangle reached
  struct Flat {
    Vec3 mid;
    float area;
  };
  std::vector<Flat> flats;
  double total = 0;
  for (const auto& p : n.mesh->prims)
    for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
      const Vec3 a = w.point(p.pos[p.idx[t]]), b = w.point(p.pos[p.idx[t + 1]]), c = w.point(p.pos[p.idx[t + 2]]);
      const Vec3 nr = cross(b - a, c - a);
      const float l = length(nr);
      if (l < 1e-8f || std::fabs(nr.y) < 0.7f * l) continue;
      flats.push_back({(a + b + c) * (1.f / 3.f), l * 0.5f});
      total += l * 0.5f;
    }
  if (flats.empty()) return 0.f;
  const std::vector<uint32_t> self{n.id};
  PickFilter f;
  f.actors = false;
  f.include_locked = true;
  f.exclude_list = &self;
  const double step = total / 64.0;
  double acc = 0, next = step * 0.5;
  int samples = 0, covered = 0;
  for (const Flat& fl : flats) {
    acc += fl.area;
    if (acc < next) continue;
    while (next <= acc) next += step;
    samples++;
    const PickHit h = pick(scene, Ray{fl.mid - Vec3{0, 0.05f, 0}, {0, -1, 0}}, f);
    if (!h.hit() || h.t <= gap || h.t > 80.f) continue;
    const Node* under = scene.find(h.node);
    if (under && under->mesh && under->mesh->extras.contains("og_backdrop")) continue;
    covered++;
  }
  return samples ? (float)covered / samples : 0.f;
}

}  // namespace ogle
