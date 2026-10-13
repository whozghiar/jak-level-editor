#include "scene/terrain.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <unordered_map>

#include "scene/shapes.h"

namespace ogle {
namespace {

float smoothstep01(float t) {
  t = std::clamp(t, 0.f, 1.f);
  return t * t * (3 - 2 * t);
}

// the brush's weight at (dx, dz) from its middle: 1 there, fading to 0 at the square's edge along a
// cosine (a hill, a hollow, not a step)
float weight(float dx, float dz, float half) {
  const float t = std::min(std::max(std::fabs(dx), std::fabs(dz)) / std::max(half, 1e-3f), 1.f);
  return 0.5f + 0.5f * std::cos(t * 3.14159265f);
}

// a triangle pair a b c d (a quad: a and d opposite), facing `out`
void add_quad(Primitive& p, uint32_t a, uint32_t b, uint32_t c, uint32_t d, Vec3 out) {
  if (dot(cross(p.pos[b] - p.pos[a], p.pos[c] - p.pos[a]), out) < 0) std::swap(b, c);
  p.idx.insert(p.idx.end(), {a, b, c, c, b, d});
}

// a triangle a b c, facing `out`
void add_tri(Primitive& p, uint32_t a, uint32_t b, uint32_t c, Vec3 out) {
  if (dot(cross(p.pos[b] - p.pos[a], p.pos[c] - p.pos[a]), out) < 0) std::swap(b, c);
  p.idx.insert(p.idx.end(), {a, b, c});
}

uint32_t add_vertex(Primitive& p, Vec3 pos, Vec2 uv) {
  p.pos.push_back(pos);
  p.uv.push_back(uv);
  return (uint32_t)p.pos.size() - 1;
}

}  // namespace

bool TerrainGrid::empty() const {
  return std::all_of(cells.begin(), cells.end(), [](uint8_t c) { return c == None; });
}

uint8_t TerrainGrid::at(int i, int k) const {
  i -= x0;
  k -= z0;
  if (i < 0 || k < 0 || i >= w || k >= d) return None;
  return cells[(size_t)k * w + i];
}

float TerrainGrid::corner(int i, int k) const {
  i -= x0;
  k -= z0;
  if (i < 0 || k < 0 || i > w || k > d) return 0.f;
  return height[(size_t)k * (w + 1) + i];
}

bool TerrainGrid::corner_on_ground(int i, int k) const {
  return at(i - 1, k - 1) || at(i, k - 1) || at(i - 1, k) || at(i, k);
}

void TerrainGrid::grow(int i0, int k0, int i1, int k1) {
  if (w > 0 && d > 0) {
    if (i0 >= x0 && k0 >= z0 && i1 < x0 + w && k1 < z0 + d) return;
    i0 = std::min(i0, x0);
    k0 = std::min(k0, z0);
    i1 = std::max(i1, x0 + w - 1);
    k1 = std::max(k1, z0 + d - 1);
  }
  const int nw = i1 - i0 + 1, nd = k1 - k0 + 1;
  std::vector<float> nh((size_t)(nw + 1) * (nd + 1), 0.f), nwall((size_t)nw * nd, 0.f);
  std::vector<uint8_t> nc((size_t)nw * nd, None), nl((size_t)nw * nd, 0);
  if (w > 0 && d > 0) {
    for (int k = 0; k <= d; k++)
      for (int i = 0; i <= w; i++) nh[(size_t)(k + z0 - k0) * (nw + 1) + (i + x0 - i0)] = height[(size_t)k * (w + 1) + i];
    for (int k = 0; k < d; k++)
      for (int i = 0; i < w; i++) {
        const size_t to = (size_t)(k + z0 - k0) * nw + (i + x0 - i0);
        nc[to] = cells[(size_t)k * w + i];
        nl[to] = look[(size_t)k * w + i];
        nwall[to] = wall[(size_t)k * w + i];
      }
  }
  x0 = i0;
  z0 = k0;
  w = nw;
  d = nd;
  height.swap(nh);
  cells.swap(nc);
  look.swap(nl);
  wall.swap(nwall);
}

int TerrainGrid::add_look(const Look& l) {
  for (size_t i = 0; i < looks.size(); i++)
    if (looks[i] == l) return (int)i;
  if (looks.size() >= 64) return 0;
  looks.push_back(l);
  return (int)looks.size() - 1;
}

json TerrainGrid::to_json() const {
  // heights in centimeters, the cells one digit each (their looks one character each), the walls
  // [cell, centimeters]
  std::vector<int> h;
  h.reserve(height.size());
  for (float v : height) h.push_back((int)std::lround(v * 100.f));
  std::string c(cells.size(), '0'), lk(cells.size(), '0');
  json walls = json::array();
  for (size_t i = 0; i < cells.size(); i++) {
    c[i] = (char)('0' + cells[i]);
    lk[i] = (char)('0' + look[i]);
    if (cells[i] == Wall) walls.push_back({i, (int)std::lround(wall[i] * 100.f)});
  }
  json jl = json::array();
  for (const Look& l : looks)
    jl.push_back({{"floor_material", l.floor_material},
                  {"wall_material", l.wall_material},
                  {"floor_tile", l.floor_tile},
                  {"wall_tile", l.wall_tile},
                  {"color", {l.color.x, l.color.y, l.color.z, l.color.w}}});
  return {{"cell", cell},
          {"x0", x0},
          {"z0", z0},
          {"w", w},
          {"d", d},
          {"heights", h},
          {"cells", c},
          {"walls", walls},
          {"looks", jl},
          {"look", lk}};
}

bool TerrainGrid::from_json(const json& j, TerrainGrid* out) {
  if (!j.is_object()) return false;
  TerrainGrid g;
  g.cell = std::clamp(j.value("cell", 1.f), 0.25f, 8.f);
  g.x0 = j.value("x0", 0);
  g.z0 = j.value("z0", 0);
  g.w = j.value("w", 0);
  g.d = j.value("d", 0);
  if (g.w < 0 || g.d < 0 || g.w > 4096 || g.d > 4096) return false;
  if (!g.w || !g.d) g.w = g.d = 0;  // nothing painted yet
  const auto& h = j.value("heights", json::array());
  const std::string c = j.value("cells", std::string());
  const size_t corners = g.w ? (size_t)(g.w + 1) * (g.d + 1) : 0;
  if (!h.is_array() || h.size() != corners || c.size() != (size_t)g.w * g.d) return false;
  for (const auto& v : h) g.height.push_back(v.is_number() ? v.get<float>() / 100.f : 0.f);
  g.cells.resize(c.size());
  g.wall.assign(c.size(), 0.f);
  for (size_t i = 0; i < c.size(); i++) g.cells[i] = (uint8_t)std::clamp(c[i] - '0', 0, 2);
  for (const auto& wv : j.value("walls", json::array()))
    if (wv.is_array() && wv.size() == 2 && wv[0].is_number_unsigned() && wv[0].get<size_t>() < g.wall.size())
      g.wall[wv[0].get<size_t>()] = wv[1].get<float>() / 100.f;
  // the looks (a terrain of before them: one, its fields beside the grid's)
  auto read_look = [](const json& o) {
    Look l;
    l.floor_material = o.value("floor_material", -1);
    l.wall_material = o.value("wall_material", -1);
    l.floor_tile = std::max(0.05f, o.value("floor_tile", 2.f));
    l.wall_tile = std::max(0.05f, o.value("wall_tile", 2.f));
    const auto& col = o.value("color", json::array());
    if (col.is_array() && col.size() == 4) l.color = {col[0].get<float>(), col[1].get<float>(), col[2].get<float>(), col[3].get<float>()};
    return l;
  };
  g.looks.clear();
  for (const auto& o : j.value("looks", json::array()))
    if (o.is_object() && g.looks.size() < 64) g.looks.push_back(read_look(o));
  if (g.looks.empty()) g.looks.push_back(read_look(j));
  const std::string lk = j.value("look", std::string());
  g.look.assign(c.size(), 0);
  if (lk.size() == c.size())
    for (size_t i = 0; i < c.size(); i++) g.look[i] = (uint8_t)std::clamp(lk[i] - '0', 0, (int)g.looks.size() - 1);
  *out = std::move(g);
  return true;
}

bool apply_brush(TerrainGrid& g, const BrushStroke& s) {
  const float c = g.cell;
  const float half = std::max(s.size * 0.5f, c * 0.5f);
  const Vec3 m = s.center;
  // the cells whose middle is in the square (the one under the brush at least), the corners in it
  int ci0 = (int)std::ceil((m.x - half) / c - 0.5f), ci1 = (int)std::floor((m.x + half) / c - 0.5f);
  int ck0 = (int)std::ceil((m.z - half) / c - 0.5f), ck1 = (int)std::floor((m.z + half) / c - 0.5f);
  if (ci0 > ci1) ci0 = ci1 = (int)std::floor(m.x / c);
  if (ck0 > ck1) ck0 = ck1 = (int)std::floor(m.z / c);
  const int pi0 = (int)std::ceil((m.x - half) / c), pi1 = (int)std::floor((m.x + half) / c);
  const int pk0 = (int)std::ceil((m.z - half) / c), pk1 = (int)std::floor((m.z + half) / c);
  bool changed = false;
  auto corner = [&](int i, int k) -> float& { return g.height[(size_t)(k - g.z0) * (g.w + 1) + (i - g.x0)]; };
  auto cell = [&](int i, int k) -> uint8_t& { return g.cells[(size_t)(k - g.z0) * g.w + (i - g.x0)]; };
  auto look = [&](int i, int k) -> uint8_t& { return g.look[(size_t)(k - g.z0) * g.w + (i - g.x0)]; };
  if ((s.mode == BrushMode::Raise || s.mode == BrushMode::Dig) && s.lay &&
      g.at((int)std::floor(m.x / c), (int)std::floor(m.z / c)) == TerrainGrid::None) {
    BrushStroke ground = s;
    ground.mode = BrushMode::Paint;
    changed |= apply_brush(g, ground);
  }
  switch (s.mode) {
    case BrushMode::Paint:
    case BrushMode::Wall: {
      g.grow(ci0, ck0, ci1, ck1);
      // the corners of new ground: at the stroke's height, with the relief
      for (int k = ck0; k <= ck1 + 1; k++)
        for (int i = ci0; i <= ci1 + 1; i++) {
          if (g.corner_on_ground(i, k)) continue;
          float y = s.height;
          if (s.mode == BrushMode::Paint && s.bumps > 0) {
            const float u = i * c / s.bump_size, v = k * c / s.bump_size;
            y += s.bumps * (smooth_noise(u, v, s.seed) + 0.35f * smooth_noise(u * 2.3f + 17.f, v * 2.3f + 5.f, s.seed));
          }
          corner(i, k) = y;
          changed = true;
        }
      for (int k = ck0; k <= ck1; k++)
        for (int i = ci0; i <= ci1; i++) {
          uint8_t& t = cell(i, k);
          if (look(i, k) != s.look && (t != TerrainGrid::None || s.mode == BrushMode::Wall)) changed = true;
          look(i, k) = (uint8_t)s.look;
          if (s.mode == BrushMode::Paint && t == TerrainGrid::None) {
            t = TerrainGrid::Ground;
            changed = true;
          } else if (s.mode == BrushMode::Wall) {
            float& wh = g.wall[(size_t)(k - g.z0) * g.w + (i - g.x0)];
            if (t != TerrainGrid::Wall || wh != s.amount) changed = true;
            t = TerrainGrid::Wall;
            wh = s.amount;
          }
        }
      break;
    }
    case BrushMode::Erase:
    case BrushMode::Unwall:
      for (int k = ck0; k <= ck1; k++)
        for (int i = ci0; i <= ci1; i++) {
          const uint8_t t = g.at(i, k);
          if (t == TerrainGrid::None || (s.mode == BrushMode::Unwall && t != TerrainGrid::Wall)) continue;
          cell(i, k) = s.mode == BrushMode::Erase ? TerrainGrid::None : TerrainGrid::Ground;
          changed = true;
        }
      break;
    case BrushMode::Raise:
    case BrushMode::Dig:
    case BrushMode::Flatten:
    case BrushMode::Smooth: {
      const std::vector<float> before = g.height;  // smoothing reads the heights before this step
      auto old = [&](int i, int k) { return before[(size_t)(k - g.z0) * (g.w + 1) + (i - g.x0)]; };
      for (int k = std::max(pk0, g.z0); k <= std::min(pk1, g.z0 + g.d); k++)
        for (int i = std::max(pi0, g.x0); i <= std::min(pi1, g.x0 + g.w); i++) {
          if (!g.corner_on_ground(i, k)) continue;
          const float wgt = weight(i * c - m.x, k * c - m.z, half);
          if (wgt <= 0) continue;
          float& h = corner(i, k);
          const float was = h;
          if (s.mode == BrushMode::Raise) h += s.amount * wgt;
          else if (s.mode == BrushMode::Dig) h -= s.amount * wgt;
          else if (s.mode == BrushMode::Flatten) h += (s.height - h) * std::clamp(s.amount * wgt, 0.f, 1.f);
          else {
            float sum = 0;
            int n = 0;
            for (auto [di, dk] : {std::pair{-1, 0}, {1, 0}, {0, -1}, {0, 1}}) {
              const int ni = i + di, nk = k + dk;
              if (ni < g.x0 || nk < g.z0 || ni > g.x0 + g.w || nk > g.z0 + g.d || !g.corner_on_ground(ni, nk)) continue;
              sum += old(ni, nk);
              n++;
            }
            if (n) h += (sum / n - old(i, k)) * std::clamp(s.amount * wgt, 0.f, 1.f);
          }
          changed |= h != was;
        }
      break;
    }
  }
  return changed;
}

std::vector<TerrainChange> terrain_changes(const TerrainGrid& before, const TerrainGrid& after, int i0, int k0, int i1, int k1) {
  std::vector<TerrainChange> out;
  auto wall = [](const TerrainGrid& g, int i, int k) { return g.wall[(size_t)(k - g.z0) * g.w + (i - g.x0)]; };
  for (int k = k0; k <= k1; k++)
    for (int i = i0; i <= i1; i++) {
      const uint8_t a = before.at(i, k), b = after.at(i, k);
      if (b == TerrainGrid::None) {
        if (a != TerrainGrid::None) out.push_back({i, k, true});
        continue;
      }
      bool changed = a != b || (b == TerrainGrid::Wall && wall(before, i, k) != wall(after, i, k));
      for (int dk = 0; dk <= 1 && !changed; dk++)
        for (int di = 0; di <= 1; di++) changed |= std::fabs(before.corner(i + di, k + dk) - after.corner(i + di, k + dk)) > 1e-3f;
      if (changed) out.push_back({i, k, false});
    }
  return out;
}

void generate_terrain(TerrainGrid& g, Vec3 center, Vec2 size, float hills, float roughness, int seed, int look) {
  const float c = g.cell;
  const int i0 = (int)std::floor((center.x - size.x * 0.5f) / c), i1 = (int)std::ceil((center.x + size.x * 0.5f) / c) - 1;
  const int k0 = (int)std::floor((center.z - size.y * 0.5f) / c), k1 = (int)std::ceil((center.z + size.y * 0.5f) / c) - 1;
  if (i1 < i0 || k1 < k0) return;
  g.grow(i0, k0, i1, k1);
  // a few hills across, finer and finer octaves (more of them the rougher)
  const float scale = std::max(std::max(size.x, size.y) / 3.f, 4.f);
  const float persistence = 0.3f + 0.35f * std::clamp(roughness, 0.f, 1.f);
  for (int k = k0; k <= k1 + 1; k++)
    for (int i = i0; i <= i1 + 1; i++) {
      const float x = i * c, z = k * c;
      float sum = 0, amp = 1, total = 0, freq = 1.f / scale;
      for (int o = 0; o < 5; o++) {
        sum += amp * smooth_noise(x * freq + o * 31.7f, z * freq - o * 17.3f, seed + o);
        total += amp;
        amp *= persistence;
        freq *= 2.f;
      }
      // 0 at its border, to meet the flat ground around it
      const float bx = std::min(i - i0, i1 + 1 - i) / std::max(1.f, (i1 + 1 - i0) * 0.15f);
      const float bz = std::min(k - k0, k1 + 1 - k) / std::max(1.f, (k1 + 1 - k0) * 0.15f);
      const float edge = smoothstep01(std::min(bx, bz));
      g.height[(size_t)(k - g.z0) * (g.w + 1) + (i - g.x0)] = center.y + hills * (sum / total + 0.5f) * edge;
    }
  for (int k = k0; k <= k1; k++)
    for (int i = i0; i <= i1; i++) {
      const size_t at = (size_t)(k - g.z0) * g.w + (i - g.x0);
      if (g.cells[at] == TerrainGrid::None) {
        g.cells[at] = TerrainGrid::Ground;
        g.look[at] = (uint8_t)look;
      }
    }
}

bool nearest_decor_edge(const Scene& scene, Vec3 at, float reach, float window, uint32_t skip, Vec3* edge,
                        uint32_t* node) {
  const SceneCache& cache = scene.cache();
  float best = std::numeric_limits<float>::max();
  bool found = false;
  for (size_t i = 0; i < scene.nodes.size() && i < cache.bounds.size(); i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || n.id == skip || !cache.visible[i] || cache.actor[i]) continue;
    const AABB& b = cache.bounds[i];
    if (!b.valid() || b.lo.x > at.x + reach || b.hi.x < at.x - reach || b.lo.z > at.z + reach || b.hi.z < at.z - reach ||
        b.lo.y > at.y + window || b.hi.y < at.y - window)
      continue;
    // of this piece: the point of its top nearest to `at` across; none when `at` is over it
    float near = std::numeric_limits<float>::max(), top = 0;  // top: its surface facing up (m2)
    Vec3 point{0, 0, 0};
    bool under = false;
    const Mat4& w = cache.worlds[i];
    for (const Primitive& p : n.mesh->prims) {
      for (size_t t = 0; t + 2 < p.idx.size() && !under; t += 3) {
        const Vec3 a = w.point(p.pos[p.idx[t]]), bb = w.point(p.pos[p.idx[t + 1]]), c = w.point(p.pos[p.idx[t + 2]]);
        const Vec3 nrm = cross(bb - a, c - a);
        const float len = length(nrm);
        if (len < 1e-6f || std::fabs(nrm.y) < 0.7f * len) continue;  // walls and slopes are no top
        if (std::max({a.y, bb.y, c.y}) < at.y - window || std::min({a.y, bb.y, c.y}) > at.y + window) continue;
        top += len * 0.5f;
        if (std::min({a.x, bb.x, c.x}) > at.x + reach || std::max({a.x, bb.x, c.x}) < at.x - reach ||
            std::min({a.z, bb.z, c.z}) > at.z + reach || std::max({a.z, bb.z, c.z}) < at.z - reach)
          continue;
        // the nearest point of the triangle across (x, z), and its height there
        const float d = (bb.x - a.x) * (c.z - a.z) - (c.x - a.x) * (bb.z - a.z);
        auto height = [&](float x, float z) {
          const float u = ((bb.x - x) * (c.z - z) - (c.x - x) * (bb.z - z)) / d;
          const float v = ((c.x - x) * (a.z - z) - (a.x - x) * (c.z - z)) / d;
          return u * a.y + v * bb.y + (1 - u - v) * c.y;
        };
        const float u = ((bb.x - at.x) * (c.z - at.z) - (c.x - at.x) * (bb.z - at.z)) / d;
        const float v = ((c.x - at.x) * (a.z - at.z) - (a.x - at.x) * (c.z - at.z)) / d;
        if (u >= 0 && v >= 0 && u + v <= 1) {
          under = true;  // it is under (or over) the square's middle: not beside it
          break;
        }
        const Vec3 tri[3] = {a, bb, c};
        for (int e = 0; e < 3; e++) {
          const Vec3 p0 = tri[e], p1 = tri[(e + 1) % 3];
          const float ex = p1.x - p0.x, ez = p1.z - p0.z, l2 = ex * ex + ez * ez;
          const float s = l2 > 1e-12f ? std::clamp(((at.x - p0.x) * ex + (at.z - p0.z) * ez) / l2, 0.f, 1.f) : 0.f;
          const float qx = p0.x + ex * s, qz = p0.z + ez * s;
          const float dist = std::hypot(at.x - qx, at.z - qz);
          const float y = std::fabs(d) > 1e-9f ? height(qx, qz) : p0.y + (p1.y - p0.y) * s;
          // as near: the highest (a box's top, not its bottom)
          if (dist < near - 1e-3f || (dist < near + 1e-3f && y > point.y)) {
            near = std::min(near, dist);
            point = {qx, y, qz};
          }
        }
      }
      if (under) break;
    }
    // not a prop (a plate, a crate's lid): a top of 4 m2 at least (a box's bottom counted too)
    if (under || near > reach || near >= best || top < 8.f) continue;
    best = near;
    *edge = point;
    if (node) *node = n.id;
    found = true;
  }
  return found;
}

bool terrain_level_near(const TerrainGrid& g, Vec3 at, float reach, Vec3* point) {
  const float c = g.cell;
  float best = std::numeric_limits<float>::max();
  const int i0 = (int)std::floor((at.x - reach) / c), i1 = (int)std::floor((at.x + reach) / c);
  const int k0 = (int)std::floor((at.z - reach) / c), k1 = (int)std::floor((at.z + reach) / c);
  for (int k = std::max(k0, g.z0); k <= std::min(k1, g.z0 + g.d - 1); k++)
    for (int i = std::max(i0, g.x0); i <= std::min(i1, g.x0 + g.w - 1); i++) {
      if (g.at(i, k) == TerrainGrid::None) continue;
      const float x = (i + 0.5f) * c, z = (k + 0.5f) * c, d = std::hypot(x - at.x, z - at.z);
      if (d > reach || d >= best) continue;
      best = d;
      *point = {x, (g.corner(i, k) + g.corner(i + 1, k) + g.corner(i, k + 1) + g.corner(i + 1, k + 1)) * 0.25f, z};
    }
  return best < std::numeric_limits<float>::max();
}

bool terrain_height(const TerrainGrid& g, float x, float z, float* y) {
  const float fi = x / g.cell, fk = z / g.cell;
  const int i = (int)std::floor(fi), k = (int)std::floor(fk);
  if (g.at(i, k) == TerrainGrid::None) return false;
  const float u = fi - i, v = fk - k;
  const float a = g.corner(i, k) + (g.corner(i + 1, k) - g.corner(i, k)) * u;
  const float b = g.corner(i, k + 1) + (g.corner(i + 1, k + 1) - g.corner(i, k + 1)) * u;
  *y = a + (b - a) * v;
  return true;
}

std::shared_ptr<Mesh> make_terrain_mesh(const TerrainGrid& g) {
  auto mesh = std::make_shared<Mesh>();
  mesh->name = "terrain";
  const float c = g.cell;
  // per look: its flat ground, its slopes and walls
  std::vector<Primitive> flats(g.looks.size()), steeps(g.looks.size());
  for (size_t l = 0; l < g.looks.size(); l++) {
    const auto& lk = g.looks[l];
    flats[l].material = lk.floor_material >= 0 ? lk.floor_material : lk.wall_material;
    steeps[l].material = lk.wall_material >= 0 ? lk.wall_material : lk.floor_material;
  }
  // the lowest of the ground: the edges go down below it
  float lowest = std::numeric_limits<float>::max();
  for (int k = g.z0; k <= g.z0 + g.d; k++)
    for (int i = g.x0; i <= g.x0 + g.w; i++)
      if (g.corner_on_ground(i, k)) lowest = std::min(lowest, g.corner(i, k));
  if (lowest == std::numeric_limits<float>::max()) return mesh;
  const float bottom = lowest - 1.f;
  // the ground smoothed over about 3 m (the cells within; at least one each side): what tells a
  // steep slope from a bump
  const int r = std::max(1, (int)std::lround(1.5f / c));
  std::vector<float> smoothed((size_t)(g.w + 1) * (g.d + 1), 0.f);
  for (int k = g.z0; k <= g.z0 + g.d; k++)
    for (int i = g.x0; i <= g.x0 + g.w; i++) {
      float sum = 0;
      int n = 0;
      for (int dk = -r; dk <= r; dk++)
        for (int di = -r; di <= r; di++) {
          const int ni = i + di, nk = k + dk;
          if (ni < g.x0 || nk < g.z0 || ni > g.x0 + g.w || nk > g.z0 + g.d || !g.corner_on_ground(ni, nk)) continue;
          sum += g.corner(ni, nk);
          n++;
        }
      smoothed[(size_t)(k - g.z0) * (g.w + 1) + (i - g.x0)] = n ? sum / n : g.corner(i, k);
    }
  auto smooth_corner = [&](int i, int k) { return smoothed[(size_t)(k - g.z0) * (g.w + 1) + (i - g.x0)]; };
  // the flat ground of a look shares its corners (smooth light over it)
  std::vector<std::unordered_map<uint64_t, uint32_t>> shared(g.looks.size());
  Primitive* flat = nullptr;
  Primitive* steep = nullptr;
  float ft = 2.f, wt = 2.f;
  int cur = 0;
  auto flat_corner = [&](int i, int k) {
    const uint64_t key = ((uint64_t)(uint32_t)i << 32) | (uint32_t)k;
    auto it = shared[cur].find(key);
    if (it != shared[cur].end()) return it->second;
    const Vec3 p{i * c, g.corner(i, k), k * c};
    return shared[cur][key] = add_vertex(*flat, p, {p.x / ft, p.z / ft});
  };
  // an upright face from (a, b) at the bottom to (a, b) at the top, its texture along it
  auto upright = [&](Vec3 a, Vec3 b, float a_top, float b_top, Vec3 out) {
    Primitive& steep_p = *steep;
    const float along_a = std::fabs(out.x) > 0.5f ? a.z : a.x, along_b = std::fabs(out.x) > 0.5f ? b.z : b.x;
    const uint32_t v0 = add_vertex(steep_p, a, {along_a / wt, a.y / wt});
    const uint32_t v1 = add_vertex(steep_p, b, {along_b / wt, b.y / wt});
    const uint32_t v2 = add_vertex(steep_p, {a.x, a_top, a.z}, {along_a / wt, a_top / wt});
    const uint32_t v3 = add_vertex(steep_p, {b.x, b_top, b.z}, {along_b / wt, b_top / wt});
    add_quad(steep_p, v0, v1, v2, v3, out);
  };
  for (int k = g.z0; k < g.z0 + g.d; k++)
    for (int i = g.x0; i < g.x0 + g.w; i++) {
      const uint8_t t = g.at(i, k);
      if (t == TerrainGrid::None) continue;
      cur = g.look[(size_t)(k - g.z0) * g.w + (i - g.x0)];
      flat = &flats[cur];
      steep = &steeps[cur];
      ft = g.looks[cur].floor_tile;
      wt = g.looks[cur].wall_tile;
      const float h00 = g.corner(i, k), h10 = g.corner(i + 1, k), h01 = g.corner(i, k + 1), h11 = g.corner(i + 1, k + 1);
      const float x = i * c, z = k * c;
      // the sides of each edge: down below the ground where it ends, from the ground up a wall
      const struct {
        int di, dk;
        Vec3 a, b;
        float ha, hb;
        Vec3 out;
      } sides[4] = {{-1, 0, {x, 0, z}, {x, 0, z + c}, h00, h01, {-1, 0, 0}},
                    {1, 0, {x + c, 0, z}, {x + c, 0, z + c}, h10, h11, {1, 0, 0}},
                    {0, -1, {x, 0, z}, {x + c, 0, z}, h00, h10, {0, 0, -1}},
                    {0, 1, {x, 0, z + c}, {x + c, 0, z + c}, h01, h11, {0, 0, 1}}};
      if (t == TerrainGrid::Wall) {
        // a column: from under the ground to its top, flat
        const float top = std::max({h00, h10, h01, h11}) + g.wall[(size_t)(k - g.z0) * g.w + (i - g.x0)];
        const float base = std::min({h00, h10, h01, h11}) - 0.3f;
        const uint32_t v0 = add_vertex(*steep, {x, top, z}, {x / wt, z / wt});
        const uint32_t v1 = add_vertex(*steep, {x + c, top, z}, {(x + c) / wt, z / wt});
        const uint32_t v2 = add_vertex(*steep, {x, top, z + c}, {x / wt, (z + c) / wt});
        const uint32_t v3 = add_vertex(*steep, {x + c, top, z + c}, {(x + c) / wt, (z + c) / wt});
        add_quad(*steep, v0, v1, v2, v3, {0, 1, 0});
        for (const auto& s : sides) {
          float from = base;
          if (g.at(i + s.di, k + s.dk) == TerrainGrid::Wall) {
            const int ni = i + s.di, nk = k + s.dk;
            const float ntop = std::max({g.corner(ni, nk), g.corner(ni + 1, nk), g.corner(ni, nk + 1), g.corner(ni + 1, nk + 1)}) +
                               g.wall[(size_t)(nk - g.z0) * g.w + (ni - g.x0)];
            if (ntop >= top) continue;
            from = ntop;
          }
          upright({s.a.x, from, s.a.z}, {s.b.x, from, s.b.z}, top, top, s.out);
        }
        continue;
      }
      // its two triangles, each with the floor texture or, steeper than 45 degrees, the wall
      // texture along its slope (rock showing where the ground is steep, its edges diagonal, not
      // square cell by cell)
      const int tri_corners[2][3][2] = {{{0, 0}, {0, 1}, {1, 0}}, {{1, 0}, {0, 1}, {1, 1}}};
      for (const auto& t : tri_corners) {
        Vec3 p3[3], q3[3];
        for (int v = 0; v < 3; v++) {
          const int ci = i + t[v][0], ck = k + t[v][1];
          p3[v] = {ci * c, g.corner(ci, ck), ck * c};
          q3[v] = {ci * c, smooth_corner(ci, ck), ck * c};
        }
        // its slope on the ground smoothed over a few meters: the rock of the broad steep slopes,
        // not of every small bump of a rough ground
        const Vec3 n = cross(q3[1] - q3[0], q3[2] - q3[0]);
        const float len = length(n);
        const bool is_steep = len > 0 && std::fabs(n.y) < 0.7071f * len;
        uint32_t v[3];
        if (!is_steep) {
          for (int q = 0; q < 3; q++) v[q] = flat_corner(i + t[q][0], k + t[q][1]);
          add_tri(*flat, v[0], v[1], v[2], {0, 1, 0});
        } else {
          // its texture along the slope, not stretched: projected on the side it faces most
          const bool along_z = std::fabs(n.x) > std::fabs(n.z);
          for (int q = 0; q < 3; q++)
            v[q] = add_vertex(*steep, p3[q], along_z ? Vec2{p3[q].z / wt, p3[q].y / wt} : Vec2{p3[q].x / wt, p3[q].y / wt});
          add_tri(*steep, v[0], v[1], v[2], {0, 1, 0});
        }
      }
      for (const auto& s : sides)
        if (g.at(i + s.di, k + s.dk) == TerrainGrid::None)
          upright({s.a.x, bottom, s.a.z}, {s.b.x, bottom, s.b.z}, s.ha, s.hb, s.out);
    }
  for (size_t l = 0; l < g.looks.size(); l++)
    for (Primitive* p : {&flats[l], &steeps[l]}) {
      if (p->idx.empty()) continue;
      p->compute_normals();
      if (g.looks[l].color.w > 0) p->col.assign(p->pos.size(), g.looks[l].color);
      mesh->prims.push_back(std::move(*p));
    }
  return mesh;
}

bool node_terrain(const Node& n, TerrainGrid* out) {
  auto it = n.extras.find("og_terrain_grid");
  return it != n.extras.end() && TerrainGrid::from_json(*it, out);
}

}  // namespace ogle
