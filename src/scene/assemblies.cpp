#include "scene/assemblies.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <numeric>
#include <string>

#include "scene/scene.h"

namespace ogle {
namespace {

enum class Shape : uint8_t { Solid, Flat, Thin };

constexpr float kTouch = 0.1f;  // meters between two boxes that touch

// the ground's details: never what holds an assembly together, nor a part of one
bool ground_detail(const std::string& name) {
  static const char* const kWords[] = {"ground", "dirt",  "pebble", "rubble", "grass",  "decal",   "stain",
                                       "blotch", "shadow", "puddle", "crack",  "moss",   "road",    "sand",
                                       "debris", "weed",  "blend",  "cattail", "leaves", "foliage"};
  std::string l = name;
  for (char& c : l) c = (char)std::tolower((unsigned char)c);
  for (const char* w : kWords)
    if (l.find(w) != std::string::npos) return true;
  return false;
}

Shape shape_of(const AABB& b) {
  const Vec3 s = b.size();
  float a[3] = {s.x, s.y, s.z};
  std::sort(a, a + 3);
  if (a[2] > 6.f * std::max(a[1], 0.3f)) return Shape::Thin;
  if (s.y < 1.f && std::max(s.x, s.z) > 3.f * std::max(s.y, 0.1f)) return Shape::Flat;
  return Shape::Solid;
}

bool touch(const AABB& a, const AABB& b) {
  return a.lo.x <= b.hi.x + kTouch && b.lo.x <= a.hi.x + kTouch && a.lo.y <= b.hi.y + kTouch &&
         b.lo.y <= a.hi.y + kTouch && a.lo.z <= b.hi.z + kTouch && b.lo.z <= a.hi.z + kTouch;
}

// what lies flat links to what holds it, not to what stands on it
bool links(const AABB& a, Shape sa, const AABB& b, Shape sb) {
  if (sa == Shape::Flat && b.center().y > a.center().y) return false;
  if (sb == Shape::Flat && a.center().y > b.center().y) return false;
  return true;
}

struct Elements {
  std::vector<AABB> box;
  std::vector<Shape> shape;
};

// the elements `items` (indices of `e`) touching each other, in groups
std::vector<std::vector<uint32_t>> components(const Elements& e, std::vector<uint32_t> items) {
  std::sort(items.begin(), items.end(), [&](uint32_t a, uint32_t b) { return e.box[a].lo.x < e.box[b].lo.x; });
  std::vector<uint32_t> root(items.size());
  std::iota(root.begin(), root.end(), 0u);
  auto find = [&](uint32_t a) {
    while (root[a] != a) a = root[a] = root[root[a]];
    return a;
  };
  std::vector<uint32_t> active;  // positions in items whose box reaches this far along x
  for (uint32_t k = 0; k < items.size(); k++) {
    const AABB& a = e.box[items[k]];
    active.erase(std::remove_if(active.begin(), active.end(),
                                [&](uint32_t j) { return e.box[items[j]].hi.x + kTouch < a.lo.x; }),
                 active.end());
    for (uint32_t j : active) {
      const AABB& b = e.box[items[j]];
      if (touch(a, b) && links(a, e.shape[items[k]], b, e.shape[items[j]])) root[find(k)] = find(j);
    }
    active.push_back(k);
  }
  std::vector<std::vector<uint32_t>> out;
  std::vector<int> group(items.size(), -1);
  for (uint32_t k = 0; k < items.size(); k++) {
    const uint32_t r = find(k);
    if (group[r] < 0) {
      group[r] = (int)out.size();
      out.emplace_back();
    }
    out[group[r]].push_back(items[k]);
  }
  return out;
}

// Where to cut `items` in two along `axis` (0: x, 2: z): the plane the fewest of their boxes cross,
// between the 20th and 80th percentiles of their middles (a street, the seam between two buildings,
// with elements on both sides), the nearest the middle among equals. Returns its cost (boxes
// crossing it).
float best_cut(const Elements& e, const std::vector<uint32_t>& items, int axis, float* at) {
  auto get = [axis](const Vec3& v) { return axis == 0 ? v.x : v.z; };
  std::vector<float> los, his, mids;
  for (uint32_t i : items) {
    los.push_back(get(e.box[i].lo) + kTouch);
    his.push_back(get(e.box[i].hi) - kTouch);
    mids.push_back(get(e.box[i].center()));
  }
  std::sort(los.begin(), los.end());
  std::sort(his.begin(), his.end());
  std::sort(mids.begin(), mids.end());
  const float from = mids[mids.size() / 5], to = mids[mids.size() * 4 / 5];
  const float mid = (from + to) * 0.5f;
  float best = 1e30f;
  for (float p = from; p <= to; p += 0.5f) {
    // crossing: starts before p and ends after it
    const float n = (float)(std::lower_bound(los.begin(), los.end(), p) - los.begin()) -
                    (float)(std::upper_bound(his.begin(), his.end(), p) - his.begin());
    const float cost = n + std::fabs(p - mid) / std::max(to - from, 1.f);  // the middle among equals
    if (cost < best) {
      best = cost;
      *at = p;
    }
  }
  return best;
}

// the groups of `items`; the ones wider than max_size cut in two where the fewest elements cross
// (each element on the side of its middle), again until they fit
void split(const Elements& e, const std::vector<uint32_t>& items, float max_size, std::vector<std::vector<uint32_t>>& out,
           int depth = 0) {
  for (auto& c : components(e, items)) {
    AABB b;
    for (uint32_t i : c) b.add(e.box[i]);
    const Vec3 s = b.size();
    if ((std::max(s.x, s.z) <= max_size && s.y <= 2.f * max_size) || c.size() < 2 || depth > 24) {
      out.push_back(std::move(c));
      continue;
    }
    float at_x = 0, at_z = 0;
    const float cost_x = s.x > max_size * 0.5f ? best_cut(e, c, 0, &at_x) : 1e30f;
    const float cost_z = s.z > max_size * 0.5f ? best_cut(e, c, 2, &at_z) : 1e30f;
    // the cheaper cut; along the longer side when both cost the same
    const bool along_x = cost_x < cost_z || (cost_x == cost_z && s.x >= s.z);
    std::vector<uint32_t> side[2];
    for (uint32_t i : c) {
      const Vec3 m = e.box[i].center();
      side[(along_x ? m.x : m.z) < (along_x ? at_x : at_z) ? 0 : 1].push_back(i);
    }
    if (side[0].empty() || side[1].empty()) {
      out.push_back(std::move(c));
      continue;
    }
    split(e, side[0], max_size, out, depth + 1);
    split(e, side[1], max_size, out, depth + 1);
  }
}

}  // namespace

std::vector<std::vector<uint32_t>> find_assemblies(Scene& scene, float max_size) {
  scene.begin_frame();
  const SceneCache& c = scene.cache();
  Elements e;
  std::vector<uint32_t> ids, linking, loose, details;
  for (size_t i = 0; i < scene.nodes.size(); i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || n.locked || !c.visible[i] || c.actor[i] || c.reference[i]) continue;
    if (n.mesh->extras.value("og_backdrop", false)) continue;  // the far scenery
    const AABB& b = c.bounds[i];
    if (!b.valid() || std::max({b.size().x, b.size().y, b.size().z}) > 3.f * max_size) continue;
    const uint32_t k = (uint32_t)ids.size();
    ids.push_back(n.id);
    e.box.push_back(b);
    e.shape.push_back(shape_of(b));
    if (ground_detail(n.name)) details.push_back(k);
    else (e.shape[k] == Shape::Thin ? loose : linking).push_back(k);
  }
  std::vector<std::vector<uint32_t>> groups;
  split(e, linking, max_size, groups);
  // the loose elements join the smallest group whose box holds their middle, and is not much
  // smaller than them
  std::vector<AABB> group_box(groups.size());
  std::vector<float> volume(groups.size());
  for (size_t g = 0; g < groups.size(); g++) {
    for (uint32_t i : groups[g]) group_box[g].add(e.box[i]);
    const Vec3 s = group_box[g].size();
    volume[g] = (s.x + 1.f) * (s.y + 1.f) * (s.z + 1.f);
  }
  std::vector<std::vector<uint32_t>> joined(groups.size());
  for (uint32_t i : loose) {
    const Vec3 m = e.box[i].center(), s = e.box[i].size();
    int best = -1;
    for (size_t g = 0; g < group_box.size(); g++) {
      const AABB& gb = group_box[g];
      const Vec3 gs = gb.size();
      if (m.x < gb.lo.x - 1.f || m.x > gb.hi.x + 1.f || m.y < gb.lo.y - 1.f || m.y > gb.hi.y + 1.f ||
          m.z < gb.lo.z - 1.f || m.z > gb.hi.z + 1.f)
        continue;
      if (s.x > gs.x * 1.25f + 1.f || s.y > gs.y * 1.25f + 1.f || s.z > gs.z * 1.25f + 1.f) continue;
      if (best < 0 || volume[g] < volume[best]) best = (int)g;
    }
    if (best >= 0) joined[best].push_back(i);
    else groups.push_back({i});
  }
  for (uint32_t i : details) groups.push_back({i});
  std::vector<std::vector<uint32_t>> out;
  out.reserve(groups.size());
  for (size_t g = 0; g < groups.size(); g++) {
    std::vector<uint32_t> members;
    for (uint32_t i : groups[g]) members.push_back(ids[i]);
    if (g < joined.size())
      for (uint32_t i : joined[g]) members.push_back(ids[i]);
    std::sort(members.begin(), members.end());
    out.push_back(std::move(members));
  }
  std::stable_sort(out.begin(), out.end(), [](const auto& a, const auto& b) { return a.size() > b.size(); });
  return out;
}

std::vector<uint32_t> assembly_of(const std::vector<std::vector<uint32_t>>& assemblies, uint32_t id) {
  for (const auto& a : assemblies)
    if (std::binary_search(a.begin(), a.end(), id)) return a;
  return {};
}

}  // namespace ogle
