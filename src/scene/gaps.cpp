#include "scene/gaps.h"

#include <algorithm>
#include <cmath>
#include <map>
#include <unordered_set>

#include "scene/bvh.h"

namespace ogle {

namespace {

// The surfaces the player sees, in a grid of 16 m cells over the ground plan: a ray tests the
// elements of the cells it crosses, the nearest first, instead of every element of the level.
struct Caster {
  struct Item {
    uint32_t index;
    AABB box;
    Mat4 inverse;
  };
  std::vector<Item> items;
  std::map<std::pair<int, int>, std::vector<uint32_t>> cells;  // into items
  std::vector<uint32_t> stamp;
  uint32_t ray_id = 0;
  Scene& scene;
  float ocean;
  static constexpr float kCell = 16.f;

  Caster(Scene& s, const AABB& area, const std::vector<uint32_t>& ignore, float ocean_height)
      : scene(s), ocean(ocean_height) {
    const SceneCache& cache = s.cache();
    std::unordered_set<uint32_t> skip;
    for (auto id : ignore) {
      const int i = s.index_of(id);
      if (i < 0) continue;
      std::vector<uint32_t> sub;
      s.subtree((size_t)i, sub);
      skip.insert(sub.begin(), sub.end());
    }
    for (size_t i = 0; i < s.nodes.size() && i < cache.count; i++) {
      const Node& n = s.nodes[i];
      if (n.kind != NodeKind::Render || !n.mesh || !cache.visible[i] || skip.count((uint32_t)i)) continue;
      if (cache.reference.size() == cache.count && cache.reference[i]) continue;
      const AABB& b = cache.bounds[i];
      if (!b.valid() || b.hi.x < area.lo.x || b.lo.x > area.hi.x || b.hi.z < area.lo.z || b.lo.z > area.hi.z)
        continue;
      const uint32_t k = (uint32_t)items.size();
      items.push_back({(uint32_t)i, b, cache.worlds[i].inverse()});
      for (int x = (int)std::floor(b.lo.x / kCell); x <= (int)std::floor(b.hi.x / kCell); x++)
        for (int z = (int)std::floor(b.lo.z / kCell); z <= (int)std::floor(b.hi.z / kCell); z++) cells[{x, z}].push_back(k);
    }
    stamp.assign(items.size(), 0);
  }

  // the nearest surface the ray meets before `reach` (glass, water and glows are seen through)
  float cast(const Ray& ray, float reach, Vec3* normal = nullptr) {
    ray_id++;
    float best = reach;
    // the ocean hides what is under it
    if (ray.d.y < 0 && ray.o.y > ocean) best = std::min(best, (ocean - ray.o.y) / ray.d.y);
    // the cells under the ray, in order (a walk along its ground plan, half a cell at a time)
    const Vec2 flat{ray.d.x, ray.d.z};
    const float flat_len = std::sqrt(flat.x * flat.x + flat.y * flat.y);
    const float step = flat_len > 1e-4f ? kCell * 0.5f / flat_len : reach;
    std::pair<int, int> last{INT32_MIN, INT32_MIN};
    for (float t = 0; t <= best + step; t += step) {
      const Vec3 p = ray.at(std::min(t, best));
      for (int dx = -1; dx <= 1; dx++)
        for (int dz = -1; dz <= 1; dz++) {
          const std::pair<int, int> c{(int)std::floor(p.x / kCell) + dx, (int)std::floor(p.z / kCell) + dz};
          if (c == last && dx == 0 && dz == 0) continue;
          auto it = cells.find(c);
          if (it == cells.end()) continue;
          for (uint32_t k : it->second) {
            if (stamp[k] == ray_id) continue;
            stamp[k] = ray_id;
            const Item& item = items[k];
            if (!ray_aabb(ray, item.box.expanded(0.01f), best)) continue;
            Node& n = scene.nodes[item.index];
            const Ray local = transform_ray(item.inverse, ray);
            for (size_t pi = 0; pi < n.mesh->prims.size(); pi++) {
              const Primitive& prim = n.mesh->prims[pi];
              if (prim.idx.empty()) continue;
              if (prim.material >= 0 && prim.material < (int)scene.materials.size()) {
                const Material& m = scene.materials[prim.material];
                if (m.alpha == AlphaMode::Blend && (m.blend != BlendMode::Alpha || m.translucent)) continue;
              }
              float th;
              uint32_t tri;
              if (n.mesh->bvh(pi).raycast(local, prim.pos, prim.idx, best, &th, &tri)) {
                best = th;
                if (normal) {
                  const Mat4& w = scene.cache().worlds[item.index];
                  const Vec3 a = w.point(prim.pos[prim.idx[tri * 3]]), b = w.point(prim.pos[prim.idx[tri * 3 + 1]]),
                             cc = w.point(prim.pos[prim.idx[tri * 3 + 2]]);
                  *normal = normalize(cross(b - a, cc - a));
                  if (dot(*normal, ray.d) > 0) *normal = -*normal;
                }
              }
            }
          }
        }
      last = {(int)std::floor(p.x / kCell), (int)std::floor(p.z / kCell)};
      if (t >= best) break;
    }
    return best;
  }
};

Vec3 direction(float yaw, float pitch) {
  return {std::cos(pitch) * std::sin(yaw), std::sin(pitch), std::cos(pitch) * std::cos(yaw)};
}

}  // namespace

GapReport find_gaps(Scene& scene, const GapOptions& o) {
  GapReport report;
  if (!o.region.valid()) return report;
  const AABB& r = o.region;
  // what a ray may meet: the region and as far around as it reaches
  AABB area = r;
  area.lo = area.lo - Vec3{o.reach, o.reach, o.reach};
  area.hi = area.hi + Vec3{o.reach, o.reach, o.reach};
  Caster caster(scene, area, o.ignore, o.ocean);
  const float sx = r.hi.x - r.lo.x, sz = r.hi.z - r.lo.z;
  const float spacing = o.spacing > 0 ? o.spacing : std::max(2.f, std::sqrt(std::max(sx * sz, 1.f) / 300.f));
  // radians down: from under the feet to the horizon (a slit of a floor is seen at one angle only)
  const float pitches[] = {-1.2f, -0.8f, -0.55f, -0.4f, -0.3f, -0.22f, -0.15f, -0.09f, -0.04f};
  const int yaws = 32;
  std::map<std::pair<int, int>, GapSpot> spots;  // by 2 m cell of where the hole is
  for (float x = r.lo.x + spacing * 0.5f; x < r.hi.x; x += spacing)
    for (float z = r.lo.z + spacing * 0.5f; z < r.hi.z; z += spacing) {
      // the ground there: walkable (mostly facing up), inside the region
      Vec3 normal{0, 1, 0};
      const Ray down{{x, r.hi.y + 2.f, z}, {0, -1, 0}};
      const float t = caster.cast(down, r.hi.y - r.lo.y + 4.f, &normal);
      if (t >= r.hi.y - r.lo.y + 4.f || normal.y < 0.7f) continue;
      const Vec3 eye = down.at(t) + Vec3{0, o.eye, 0};
      report.viewpoints++;
      for (int k = 0; k < yaws; k++)
        for (float pitch : pitches) {
          const float yaw = (float)k / yaws * 6.2831853f;
          const Ray ray{eye, direction(yaw, pitch)};
          report.rays++;
          if (caster.cast(ray, o.reach) < o.reach) continue;
          report.escaped++;
          // the rays on each side of it: a gap has something on both sides (left and right of a slit
          // between two walls, before and after a hole in a floor), an open edge on one side at most
          // (a little farther each time: a hole can be wider than the first step)
          float hit[4] = {o.reach, o.reach, o.reach, o.reach};
          for (float a : {0.03f, 0.08f, 0.16f}) {
            const float yaws[4] = {yaw - a, yaw + a, yaw, yaw}, tilts[4] = {pitch, pitch, pitch - a, pitch + a};
            for (int k = 0; k < 4; k++)
              if (hit[k] >= o.reach) hit[k] = caster.cast(Ray{eye, direction(yaws[k], tilts[k])}, o.reach);
          }
          const bool across = hit[0] < o.reach && hit[1] < o.reach, along = hit[2] < o.reach && hit[3] < o.reach;
          const float near = std::min({across ? std::min(hit[0], hit[1]) : o.reach, along ? std::min(hit[2], hit[3]) : o.reach});
          GapSpot s;
          s.seen_from = eye;
          s.direction = ray.d;
          s.edge = !across && !along;
          if (!s.edge) {
            s.at = ray.at(near);
          } else {
            // an open edge: where the ray leaves the region
            const AABB big = r.expanded(1.f);
            float tt = 0;
            while (tt < o.reach && big.contains(ray.at(tt))) tt += 1.f;
            s.at = ray.at(tt);
          }
          auto key = std::make_pair((int)std::floor(s.at.x / 2.f), (int)std::floor(s.at.z / 2.f));
          auto [it, fresh] = spots.emplace(key, s);
          it->second.rays++;
        }
    }
  for (auto& [k, s] : spots) report.spots.push_back(s);
  std::sort(report.spots.begin(), report.spots.end(), [](const GapSpot& a, const GapSpot& b) {
    if (a.edge != b.edge) return !a.edge;  // the gaps between pieces first: the edges may be wanted
    return a.rays > b.rays;
  });
  return report;
}

}  // namespace ogle
