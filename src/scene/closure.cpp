#include "scene/closure.h"

#include <algorithm>
#include <cmath>
#include <deque>
#include <map>

namespace ogle {

namespace {

float hash01(int x, int z, int seed) {
  uint32_t h = (uint32_t)x * 374761393u + (uint32_t)z * 668265263u + (uint32_t)seed * 2246822519u;
  h = (h ^ (h >> 13)) * 1274126177u;
  return (float)((h ^ (h >> 16)) & 0xffff) / 65535.f;
}

float cross2(const Vec2& o, const Vec2& a, const Vec2& b) { return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x); }

// the convex hull of points of the ground plan, counter-clockwise (Andrew's monotone chain)
std::vector<Vec2> convex_hull(std::vector<Vec2> p) {
  std::sort(p.begin(), p.end(), [](const Vec2& a, const Vec2& b) { return a.x < b.x || (a.x == b.x && a.y < b.y); });
  if (p.size() < 3) return p;
  std::vector<Vec2> h(2 * p.size());
  size_t k = 0;
  for (size_t i = 0; i < p.size(); i++) {
    while (k >= 2 && cross2(h[k - 2], h[k - 1], p[i]) <= 0) k--;
    h[k++] = p[i];
  }
  for (size_t i = p.size() - 1, t = k + 1; i > 0; i--) {
    while (k >= t && cross2(h[k - 2], h[k - 1], p[i - 1]) <= 0) k--;
    h[k++] = p[i - 1];
  }
  h.resize(k - 1);
  return h;
}

bool inside_hull(const std::vector<Vec2>& h, const Vec2& p) {
  if (h.size() < 3) return false;
  for (size_t i = 0; i < h.size(); i++)
    if (cross2(h[i], h[(i + 1) % h.size()], p) < -1e-4f) return false;
  return true;
}

struct Faces {
  Primitive walls, tops;
  Vec3 origin;
  float tile = 2.f;
  // a quad a b c d (around its edge), facing n
  void quad(Primitive& p, Vec3 a, Vec3 b, Vec3 c, Vec3 d, Vec3 n, bool upright) {
    if (dot(cross(b - a, c - a), n) < 0) std::swap(b, d);
    const uint32_t base = (uint32_t)p.pos.size();
    for (const Vec3& v : {a, b, c, d}) {
      p.pos.push_back(v - origin);
      p.nrm.push_back(n);
      // walls: along them and up; tops: the ground plan (the texture's meters kept in the world)
      p.uv.push_back(upright ? Vec2{(std::fabs(n.x) > std::fabs(n.z) ? v.z : v.x) / tile, -v.y / tile}
                             : Vec2{v.x / tile, v.z / tile});
    }
    p.idx.insert(p.idx.end(), {base, base + 1, base + 2, base, base + 2, base + 3});
  }
  // upright, along a..b of the ground plan, from y0 to y1, facing n
  void wall(Vec2 a, Vec2 b, float y0, float y1, Vec3 n) {
    if (y1 - y0 < 1e-3f) return;
    quad(walls, {a.x, y0, a.y}, {b.x, y0, b.y}, {b.x, y1, b.y}, {a.x, y1, a.y}, n, true);
  }
  // a block standing out of a wall a..b (facing n) by `out`, from y0 to y1: its front, sides, top, bottom
  void band(Vec2 a, Vec2 b, float y0, float y1, Vec3 n, float out) {
    const Vec2 o{n.x * out, n.z * out};
    const Vec2 a2{a.x + o.x, a.y + o.y}, b2{b.x + o.x, b.y + o.y};
    wall(a2, b2, y0, y1, n);
    const Vec2 dir{b.x - a.x, b.y - a.y};
    const float len = std::sqrt(dir.x * dir.x + dir.y * dir.y);
    if (len < 1e-4f) return;
    const Vec3 side{dir.x / len, 0, dir.y / len};
    wall(a, a2, y0, y1, -side);
    wall(b, b2, y0, y1, side);
    quad(walls, {a.x, y1, a.y}, {b.x, y1, b.y}, {b2.x, y1, b2.y}, {a2.x, y1, a2.y}, {0, 1, 0}, true);
    quad(walls, {a.x, y0, a.y}, {b.x, y0, b.y}, {b2.x, y0, b2.y}, {a2.x, y0, a2.y}, {0, -1, 0}, true);
  }
};

}  // namespace

ClosureResult close_structure(Scene& scene, const std::vector<uint32_t>& roots, const ClosureOptions& o) {
  ClosureResult res;
  const SceneCache& cache = scene.cache();
  // its triangles in the world, the area of its materials facing sideways and up
  struct Tri {
    Vec3 a, b, c, n;
    float area;
  };
  std::vector<Tri> tris;
  std::map<int, double> wall_area, top_area;
  // the light on its walls and on its tops (vertex colors, by area): the closure's, until the level
  // around lights it
  Vec4 wall_light{0, 0, 0, 0}, top_light{0, 0, 0, 0};
  double wall_lit = 0, top_lit = 0;
  Vec3 facing{0, 0, 0};
  double upright = 0;
  AABB box;
  for (uint32_t root : roots) {
    const int ri = scene.index_of(root);
    if (ri < 0 || (size_t)ri >= cache.count) continue;
    std::vector<uint32_t> sub;
    scene.subtree((size_t)ri, sub);
    for (uint32_t i : sub) {
      const Node& n = scene.nodes[i];
      if (n.kind != NodeKind::Render || !n.mesh) continue;
      const Mat4& w = cache.worlds[i];
      for (const Primitive& p : n.mesh->prims) {
        // glass, water and glows are seen through: not a wall, not a material for one
        if (p.material >= 0 && p.material < (int)scene.materials.size() && translucent_in_game(scene.materials[p.material]))
          continue;
        for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
          const Vec3 a = w.point(p.pos[p.idx[t]]), b = w.point(p.pos[p.idx[t + 1]]), c = w.point(p.pos[p.idx[t + 2]]);
          Vec3 nn = cross(b - a, c - a);
          const float l = length(nn);
          if (l < 1e-6f) continue;
          nn = nn / l;
          const float area = l * 0.5f;
          box.add(a);
          box.add(b);
          box.add(c);
          tris.push_back({a, b, c, nn, area});
          Vec4 light{0, 0, 0, 0};
          const bool lit = p.col.size() == p.pos.size();
          if (lit)
            for (int k = 0; k < 3; k++) {
              const Vec4& cv = p.col[p.idx[t + k]];
              light = {light.x + cv.x * area / 3, light.y + cv.y * area / 3, light.z + cv.z * area / 3, light.w + cv.w * area / 3};
            }
          if (std::fabs(nn.y) < 0.4f) {
            wall_area[p.material] += area;
            upright += area;
            facing += Vec3{nn.x, 0, nn.z} * area;
            if (lit) {
              wall_light = {wall_light.x + light.x, wall_light.y + light.y, wall_light.z + light.z, wall_light.w + light.w};
              wall_lit += area;
            }
          } else if (nn.y > 0.7f) {
            top_area[p.material] += area;
            if (lit) {
              top_light = {top_light.x + light.x, top_light.y + light.y, top_light.z + light.z, top_light.w + light.w};
              top_lit += area;
            }
          }
        }
      }
    }
  }
  if (tris.empty()) {
    res.error = "nothing to close: no surface";
    return res;
  }
  // its own axes: the way most of its walls run (modulo a quarter turn); the grid follows them, so
  // that its walls are not cut in steps, and the closure is turned like it
  {
    double cs = 0, sn = 0;
    for (const Tri& t : tris)
      if (std::fabs(t.n.y) < 0.4f) {
        const double a = 4.0 * std::atan2((double)t.n.z, (double)t.n.x);
        cs += t.area * std::cos(a);
        sn += t.area * std::sin(a);
      }
    res.yaw = (cs * cs + sn * sn) > 1e-12 ? (float)(-std::atan2(sn, cs) / 4.0) : 0.f;
  }
  const Vec3 pivot = box.center();
  const Mat4 to_frame = Mat4::translate(pivot) * Mat4::rotate(Quat::axis_angle({0, 1, 0}, -res.yaw)) * Mat4::translate(-pivot);
  box = AABB{};
  facing = {0, 0, 0};
  for (Tri& t : tris) {
    t.a = to_frame.point(t.a);
    t.b = to_frame.point(t.b);
    t.c = to_frame.point(t.c);
    t.n = to_frame.dir(t.n);
    box.add(t.a);
    box.add(t.b);
    box.add(t.c);
    if (std::fabs(t.n.y) < 0.4f) facing += Vec3{t.n.x, 0, t.n.z} * t.area;
  }
  const Vec3 seen_from = to_frame.point(o.seen_from);
  auto most = [](const std::map<int, double>& m) {
    int best = -1;
    double a = 0;
    for (const auto& [k, v] : m)
      if (v > a) a = v, best = k;
    return best;
  };
  res.wall_material = o.wall_material >= 0 ? o.wall_material : most(wall_area);
  res.top_material = o.top_material >= 0 ? o.top_material : most(top_area);
  if (res.top_material < 0) res.top_material = res.wall_material;
  if (res.wall_material < 0) res.wall_material = res.top_material;

  const Vec3 size = box.size();
  const float base = box.lo.y;
  // where it is seen from: the way behind it is away from there (the faces' own way when not said:
  // the extracted game's triangles face either way, so only a strong majority counts)
  const Vec3 middle = box.center();
  const float fl = length(facing);
  Vec3 back{0, 0, 0};
  if (o.has_view) {
    back = Vec3{middle.x - seen_from.x, 0, middle.z - seen_from.z};
  } else if (upright > 0 && fl > 0.6 * upright) {
    back = -facing / fl;
  }
  // straight back along one of its axes
  if (std::fabs(back.x) > std::fabs(back.z)) back = {back.x > 0 ? 1.f : -1.f, 0, 0};
  else if (std::fabs(back.z) > 0) back = {0, 0, back.z > 0 ? 1.f : -1.f};
  res.facade = length(back) > 0.5f;
  res.back = Mat4::rotate(Quat::axis_angle({0, 1, 0}, res.yaw)).dir(back);
  const Vec3 eye = o.has_view ? seen_from : middle - back * 1000.f;
  const float width = std::max(size.x, size.z);
  const float depth = o.depth > 0 ? o.depth : std::clamp(0.6f * width, 4.f, 24.f);  // as deep as a building that wide
  // the grid over its ground plan, as far around as it may be filled
  AABB plan = box.expanded(depth);
  float c = o.cell > 0 ? o.cell : std::clamp(std::max(size.x, size.z) / 48.f, 0.5f, 4.f);
  c = std::max(c, std::sqrt(plan.size().x * plan.size().z / (200.f * 200.f)));  // 200 x 200 cells at most
  res.cell = c;
  const float x0 = plan.lo.x, z0 = plan.lo.z;
  const int nx = (int)std::ceil(plan.size().x / c) + 1, nz = (int)std::ceil(plan.size().z / c) + 1;
  const size_t count = (size_t)nx * nz;
  std::vector<float> top(count, -1e30f), bottom(count, 1e30f), sides(count, 0.f);  // sides: upright area
  std::vector<uint8_t> occ(count, 0);
  auto at = [&](int ix, int iz) { return (size_t)iz * nx + ix; };
  auto center = [&](int ix, int iz) { return Vec2{x0 + (ix + 0.5f) * c, z0 + (iz + 0.5f) * c}; };
  auto cell_of = [&](float x, float z, int* ix, int* iz) {
    *ix = (int)std::floor((x - x0) / c);
    *iz = (int)std::floor((z - z0) / c);
    return *ix >= 0 && *iz >= 0 && *ix < nx && *iz < nz;
  };
  auto mark = [&](const Vec3& p, float side) {
    int ix, iz;
    if (!cell_of(p.x, p.z, &ix, &iz)) return;
    const size_t k = at(ix, iz);
    top[k] = std::max(top[k], p.y);
    bottom[k] = std::min(bottom[k], p.y);
    occ[k] = 1;
    sides[k] += side;
  };
  for (const Tri& t : tris) {
    // its edges, half a cell at a time (an upright face is a line of the ground plan)
    const bool up = std::fabs(t.n.y) < 0.4f;
    const Vec3* v[3] = {&t.a, &t.b, &t.c};
    int marks = 0;
    for (int e = 0; e < 3; e++)
      marks += std::max(1, (int)std::ceil(std::hypot(v[(e + 1) % 3]->x - v[e]->x, v[(e + 1) % 3]->z - v[e]->z) / (c * 0.5f))) + 1;
    const float side = up ? t.area / marks : 0.f;
    for (int e = 0; e < 3; e++) {
      const Vec3 &p = *v[e], &q = *v[(e + 1) % 3];
      const int steps = std::max(1, (int)std::ceil(std::hypot(q.x - p.x, q.z - p.z) / (c * 0.5f)));
      for (int k = 0; k <= steps; k++) mark(p + (q - p) * ((float)k / steps), side);
    }
    // the inside of what faces up or down: the cells whose middle it covers, at its height there
    if (std::fabs(t.n.y) < 0.2f) continue;
    int ix0, iz0, ix1, iz1;
    cell_of(std::min({t.a.x, t.b.x, t.c.x}), std::min({t.a.z, t.b.z, t.c.z}), &ix0, &iz0);
    cell_of(std::max({t.a.x, t.b.x, t.c.x}), std::max({t.a.z, t.b.z, t.c.z}), &ix1, &iz1);
    const Vec2 A{t.a.x, t.a.z}, B{t.b.x, t.b.z}, C{t.c.x, t.c.z};
    const float area = cross2(A, B, C);
    if (std::fabs(area) < 1e-6f) continue;
    for (int iz = std::max(0, iz0); iz <= std::min(nz - 1, iz1); iz++)
      for (int ix = std::max(0, ix0); ix <= std::min(nx - 1, ix1); ix++) {
        const Vec2 p = center(ix, iz);
        const float w0 = cross2(B, C, p) / area, w1 = cross2(C, A, p) / area, w2 = 1.f - w0 - w1;
        if (w0 < 0 || w1 < 0 || w2 < 0) continue;
        mark({p.x, t.a.y * w0 + t.b.y * w1 + t.c.y * w2, p.y}, 0.f);
      }
  }
  // the columns standing on the ground (not a balcony, an overhang, a roof's eave), and the cells
  // with a wall (what hides what is behind it)
  const float footing = base + std::max(1.5f, 0.1f * size.y);
  std::vector<uint8_t> solid(count, 0), wall(count, 0);
  float most_side = 0;
  for (size_t k = 0; k < count; k++) most_side = std::max(most_side, sides[k]);
  for (size_t k = 0; k < count; k++) {
    solid[k] = occ[k] && bottom[k] <= footing;
    wall[k] = occ[k] && sides[k] > most_side * 0.05f;
  }
  // where it may be filled: inside the hull of what stands on the ground, carried back away from
  // where it is seen from as deep as a building that wide
  std::vector<uint8_t> fill(count, 0);
  std::vector<uint8_t> candidate(count, 0);
  {
    std::vector<Vec2> corners;
    const float h = c * 0.49f;
    for (int iz = 0; iz < nz; iz++)
      for (int ix = 0; ix < nx; ix++)
        if (solid[at(ix, iz)]) {
          for (float t : {0.f, depth}) {
            if (t > 0 && !res.facade) continue;
            const Vec2 p = center(ix, iz) + Vec2{back.x * t, back.z * t};
            corners.insert(corners.end(), {{p.x - h, p.y - h}, {p.x + h, p.y - h}, {p.x + h, p.y + h}, {p.x - h, p.y + h}});
          }
        }
    const std::vector<Vec2> hull = convex_hull(corners);
    for (int iz = 0; iz < nz; iz++)
      for (int ix = 0; ix < nx; ix++)
        if (inside_hull(hull, center(ix, iz))) candidate[at(ix, iz)] = 1;
  }
  // ... and hidden from there: the way to the eye crosses one of its walls (what can be seen from
  // the street, a courtyard facing it, stays open). Inside a building closed all round, everything
  // is hidden.
  for (int iz = 0; iz < nz; iz++)
    for (int ix = 0; ix < nx; ix++) {
      const size_t k = at(ix, iz);
      if (!candidate[k] || occ[k]) continue;
      const Vec2 p = center(ix, iz);
      Vec2 to{eye.x - p.x, eye.z - p.y};
      const float far = to.length();
      if (far < 1e-3f) continue;
      to = to * (1.f / far);
      bool hidden = false;
      for (float t = c * 0.3f; t <= far && !hidden; t += c * 0.3f) {
        int jx, jz;
        if (!cell_of(p.x + to.x * t, p.y + to.y * t, &jx, &jz)) break;
        hidden = wall[at(jx, jz)] && solid[at(jx, jz)];  // under a sign or a balcony, one sees through
      }
      fill[k] = hidden;
    }
  // its own cells along what was filled: their top closes the gap between it and the fill
  for (int iz = 0; iz < nz; iz++)
    for (int ix = 0; ix < nx; ix++) {
      const size_t k = at(ix, iz);
      if (!solid[k] || fill[k]) continue;
      const int dx[4] = {1, -1, 0, 0}, dz[4] = {0, 0, 1, -1};
      for (int d = 0; d < 4; d++) {
        const int jx = ix + dx[d], jz = iz + dz[d];
        if (jx >= 0 && jz >= 0 && jx < nx && jz < nz && fill[at(jx, jz)] && !occ[at(jx, jz)]) fill[k] = 2;
      }
    }
  // the height of each cell: the structure's own where it has some, else that of the nearest of its
  // cells (its silhouette carried back), lower terrace by terrace going away from it
  std::vector<float> height(count, 0.f);
  std::vector<int> dist(count, -1);
  std::deque<size_t> queue;
  for (size_t k = 0; k < count; k++)
    if (solid[k] && top[k] > base + 0.5f) {
      height[k] = top[k];
      dist[k] = 0;
      queue.push_back(k);
    }
  if (queue.empty()) {
    res.error = "nothing to close: no part of it stands on the ground";
    return res;
  }
  while (!queue.empty()) {
    const size_t k = queue.front();
    queue.pop_front();
    const int ix = (int)(k % nx), iz = (int)(k / nx);
    const int dx[4] = {1, -1, 0, 0}, dz[4] = {0, 0, 1, -1};
    for (int d = 0; d < 4; d++) {
      const int jx = ix + dx[d], jz = iz + dz[d];
      if (jx < 0 || jz < 0 || jx >= nx || jz >= nz) continue;
      const size_t j = at(jx, jz);
      if (!fill[j] || dist[j] >= 0) continue;
      dist[j] = dist[k] + 1;
      height[j] = height[k];
      queue.push_back(j);
    }
  }
  const float terrace = std::max(4.f, depth / 3.f) * (1.f + 0.15f * (float)(o.seed % 3));  // between two setbacks
  for (size_t k = 0; k < count; k++) {
    if (!fill[k]) continue;
    if (dist[k] < 0) {  // cut off from the structure: not filled
      fill[k] = 0;
      continue;
    }
    res.cells++;
    if (occ[k]) {
      height[k] = std::max(base + 0.5f, height[k] - 0.05f);  // its own top stays the one seen
      continue;
    }
    res.added++;
    float h = height[k];
    if (o.relief) {
      // two setbacks at most going away from it, each a story or two
      const float tall = h - base;
      const float step = std::clamp(std::round(tall * 0.15f / 0.5f) * 0.5f, 1.f, 6.f);
      h -= std::min(std::floor(dist[k] * c / terrace) * step, 0.4f * tall);
    }
    height[k] = std::max(base + 2.f, std::round(h * 4.f) / 4.f);
  }
  if (!res.added) {
    res.error = "nothing to close: it is already closed (or seen from both sides)";
    return res;
  }

  Faces f;
  f.origin = {box.center().x, base, box.center().z};
  f.tile = o.tile;
  res.origin = to_frame.inverse().point(f.origin);  // in the world (the mesh stays in the turned frame)
  const float sunk = base - 0.5f;  // into the ground: no gap under it on a slope
  // its top, the cells of a row at the same height as one
  for (int iz = 0; iz < nz; iz++)
    for (int ix = 0; ix < nx;) {
      const size_t k = at(ix, iz);
      if (!fill[k]) {
        ix++;
        continue;
      }
      int end = ix + 1;
      while (end < nx && fill[at(end, iz)] && std::fabs(height[at(end, iz)] - height[k]) < 1e-3f) end++;
      const float xa = x0 + ix * c, xb = x0 + end * c, za = z0 + iz * c, zb = za + c, y = height[k];
      f.quad(f.tops, {xa, y, za}, {xb, y, za}, {xb, y, zb}, {xa, y, zb}, {0, 1, 0}, false);
      ix = end;
    }
  // its walls: where nothing of the structure closes it, and the steps of its top
  const int period = std::max(2, (int)std::round(4.f / c));
  for (int iz = 0; iz < nz; iz++)
    for (int ix = 0; ix < nx; ix++) {
      const size_t k = at(ix, iz);
      if (!fill[k]) continue;
      const float xa = x0 + ix * c, xb = xa + c, za = z0 + iz * c, zb = za + c, y = height[k];
      const struct {
        int dx, dz;
        Vec2 a, b;
        Vec3 n;
      } sides[4] = {{1, 0, {xb, za}, {xb, zb}, {1, 0, 0}},
                    {-1, 0, {xa, zb}, {xa, za}, {-1, 0, 0}},
                    {0, 1, {xb, zb}, {xa, zb}, {0, 0, 1}},
                    {0, -1, {xa, za}, {xb, za}, {0, 0, -1}}};
      for (const auto& s : sides) {
        const int jx = ix + s.dx, jz = iz + s.dz;
        const bool in = jx >= 0 && jz >= 0 && jx < nx && jz < nz && fill[at(jx, jz)];
        if (in) {
          const float hn = height[at(jx, jz)];
          if (hn < y - 1e-3f) f.wall(s.a, s.b, hn, y, s.n);
          continue;
        }
        if (occ[k]) continue;  // the structure's own side
        f.wall(s.a, s.b, sunk, y, s.n);
        if (!o.relief || y - base < 3.f) continue;
        // a cornice along its top, a pilaster every few meters
        f.band(s.a, s.b, y - 0.35f, y, s.n, 0.2f);
        const int along = s.dx ? iz : ix;
        if (along % period == 0) {
          const float w = std::min(0.5f, c * 0.6f) * 0.5f;
          const Vec2 mid{(s.a.x + s.b.x) * 0.5f, (s.a.y + s.b.y) * 0.5f};
          const Vec2 dir{(s.b.x - s.a.x) / c, (s.b.y - s.a.y) / c};
          f.band({mid.x - dir.x * w, mid.y - dir.y * w}, {mid.x + dir.x * w, mid.y + dir.y * w}, sunk, y - 0.35f, s.n, 0.25f);
        }
      }
    }
  f.walls.material = res.wall_material;
  f.tops.material = res.top_material;
  // lit like the structure (its walls, its tops) when it has vertex colors
  auto average = [](Vec4 sum, double w) {
    return w > 0 ? Vec4{(float)(sum.x / w), (float)(sum.y / w), (float)(sum.z / w), 1.f} : Vec4{0, 0, 0, 0};
  };
  const Vec4 wl = average(wall_light, wall_lit);
  const Vec4 tl = top_lit > 0 ? average(top_light, top_lit) : wl;
  if (wl.w > 0) f.walls.col.assign(f.walls.pos.size(), wl);
  if (tl.w > 0) f.tops.col.assign(f.tops.pos.size(), tl);
  auto mesh = std::make_shared<Mesh>();
  mesh->name = "closure";
  if (!f.walls.idx.empty()) mesh->prims.push_back(std::move(f.walls));
  if (!f.tops.idx.empty()) mesh->prims.push_back(std::move(f.tops));
  res.mesh = mesh;
  return res;
}

}  // namespace ogle
