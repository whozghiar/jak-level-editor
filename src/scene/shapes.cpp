#include "scene/shapes.h"

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>

#include "stb_image.h"

namespace ogle {

namespace {

namespace fs = std::filesystem;

fs::path to_path(const std::string& s) { return fs::path(std::u8string((const char8_t*)s.c_str())); }
std::string from_path(const fs::path& p) {
  auto u = p.u8string();
  return std::string(u.begin(), u.end());
}

bool same_material(const Material& a, const Material& b) {
  return a.name == b.name && a.texture == b.texture && a.alpha == b.alpha && a.blend == b.blend &&
         a.translucent == b.translucent &&
         a.cutoff == b.cutoff && a.double_sided == b.double_sided && a.base_color.x == b.base_color.x &&
         a.base_color.y == b.base_color.y && a.base_color.z == b.base_color.z && a.base_color.w == b.base_color.w;
}



// While a bent shape is made: its faces cut in slices of this many meters along this axis (0 x,
// 2 z), so that it bends smoothly; 0: not cut
float g_slice = 0.f;
int g_slice_axis = 0;

// A convex polygon's part between two planes across `axis` (lo <= v[axis] <= hi)
std::vector<Vec3> clip_slab(const std::vector<Vec3>& poly, int axis, float lo, float hi) {
  auto clip = [&](const std::vector<Vec3>& in, float at, bool keep_above) {
    std::vector<Vec3> out;
    for (size_t i = 0; i < in.size(); i++) {
      const Vec3 a = in[i], b = in[(i + 1) % in.size()];
      const bool ia = keep_above ? a[axis] >= at : a[axis] <= at;
      const bool ib = keep_above ? b[axis] >= at : b[axis] <= at;
      if (ia) out.push_back(a);
      if (ia != ib) {
        const float k = (at - a[axis]) / (b[axis] - a[axis]);
        out.push_back(a + (b - a) * k);
      }
    }
    return out;
  };
  return clip(clip(poly, lo, true), hi, false);
}

// A flat convex face (corners in order, either way round), facing `n`: its texture repeats every
// `tile` meters, horizontal along its width and upright along its slope.
void add_face(Primitive& p, std::vector<Vec3> poly, Vec3 n, float tile) {
  if (g_slice > 0) {
    // in slices across the bending axis (the same planes for every face: no cracks)
    float lo = poly[0][g_slice_axis], hi = lo;
    for (const Vec3& v : poly) {
      lo = std::min(lo, v[g_slice_axis]);
      hi = std::max(hi, v[g_slice_axis]);
    }
    if (hi - lo > g_slice * 1.01f) {
      const float slice = g_slice;
      g_slice = 0;
      for (float a = std::floor(lo / slice) * slice; a < hi; a += slice) {
        std::vector<Vec3> part = clip_slab(poly, g_slice_axis, a, a + slice);
        // drop the corners repeated by the cut
        std::vector<Vec3> clean;
        for (const Vec3& v : part)
          if (clean.empty() || length(v - clean.back()) > 1e-5f) clean.push_back(v);
        while (clean.size() > 1 && length(clean.front() - clean.back()) <= 1e-5f) clean.pop_back();
        if (clean.size() >= 3) add_face(p, clean, n, tile);
      }
      g_slice = slice;
      return;
    }
  }
  n = normalize(n);
  if (dot(cross(poly[1] - poly[0], poly[2] - poly[0]), n) < 0) std::reverse(poly.begin(), poly.end());
  const Vec3 up{0, 1, 0};
  const Vec3 right = std::fabs(n.y) > 0.99f ? Vec3{1, 0, 0} : normalize(cross(up, n));
  const Vec3 vup = cross(n, right);
  const uint32_t first = (uint32_t)p.pos.size();
  for (const Vec3& v : poly) {
    p.pos.push_back(v);
    p.nrm.push_back(n);
    p.uv.push_back({dot(v, right) / tile, -dot(v, vup) / tile});
  }
  for (uint32_t i = 1; i + 1 < poly.size(); i++) p.idx.insert(p.idx.end(), {first, first + i, first + i + 1});
}

void add_box(Primitive& p, Vec3 lo, Vec3 hi, float tile) {
  const float x0 = lo.x, y0 = lo.y, z0 = lo.z, x1 = hi.x, y1 = hi.y, z1 = hi.z;
  add_face(p, {{x1, y0, z0}, {x1, y1, z0}, {x1, y1, z1}, {x1, y0, z1}}, {1, 0, 0}, tile);
  add_face(p, {{x0, y0, z0}, {x0, y1, z0}, {x0, y1, z1}, {x0, y0, z1}}, {-1, 0, 0}, tile);
  add_face(p, {{x0, y1, z0}, {x1, y1, z0}, {x1, y1, z1}, {x0, y1, z1}}, {0, 1, 0}, tile);
  add_face(p, {{x0, y0, z0}, {x1, y0, z0}, {x1, y0, z1}, {x0, y0, z1}}, {0, -1, 0}, tile);
  add_face(p, {{x0, y0, z1}, {x1, y0, z1}, {x1, y1, z1}, {x0, y1, z1}}, {0, 0, 1}, tile);
  add_face(p, {{x0, y0, z0}, {x1, y0, z0}, {x1, y1, z0}, {x0, y1, z0}}, {0, 0, -1}, tile);
}

// A cylinder from a to b
void add_tube(Primitive& p, Vec3 a, Vec3 b, float r, int sides = 10) {
  const Vec3 axis = normalize(b - a);
  const Vec3 u = normalize(cross(std::fabs(axis.y) > 0.9f ? Vec3{1, 0, 0} : Vec3{0, 1, 0}, axis));
  const Vec3 v = cross(axis, u);
  std::vector<Vec3> ca, cb;
  for (int i = 0; i < sides; i++) {
    const float t = 2 * kPi * i / sides;
    const Vec3 o = (u * std::cos(t) + v * std::sin(t)) * r;
    ca.push_back(a + o);
    cb.push_back(b + o);
  }
  for (int i = 0; i < sides; i++) {
    const int j = (i + 1) % sides;
    add_face(p, {ca[i], ca[j], cb[j], cb[i]}, (ca[i] + ca[j]) * 0.5f - a, 1.f);
  }
  add_face(p, ca, axis * -1.f, 1.f);
  add_face(p, cb, axis, 1.f);
}

// A cone standing on `base`, pointing along `dir` (its length)
void add_cone(Primitive& p, Vec3 base, Vec3 dir, float r, int sides = 12) {
  const Vec3 axis = normalize(dir);
  const Vec3 u = normalize(cross(std::fabs(axis.y) > 0.9f ? Vec3{1, 0, 0} : Vec3{0, 1, 0}, axis));
  const Vec3 v = cross(axis, u);
  const Vec3 tip = base + dir;
  std::vector<Vec3> ring;
  for (int i = 0; i < sides; i++) {
    const float t = 2 * kPi * i / sides;
    ring.push_back(base + (u * std::cos(t) + v * std::sin(t)) * r);
  }
  for (int i = 0; i < sides; i++) {
    const int j = (i + 1) % sides;
    add_face(p, {ring[i], ring[j], tip}, (ring[i] + ring[j]) * 0.5f - base + axis * r, 1.f);
  }
  add_face(p, ring, axis * -1.f, 1.f);
}

// A line of tubes through points
void add_path(Primitive& p, const std::vector<Vec3>& points, float r) {
  for (size_t i = 0; i + 1 < points.size(); i++) add_tube(p, points[i], points[i + 1], r, 6);
}

}  // namespace

std::shared_ptr<Mesh> object_icon(const std::string& etype, const std::string& family, int seed) {
  // a zigzag's offset at point i of bolt b: the same for a seed
  auto zig = [&](int b, int i) {
    uint32_t k = (uint32_t)(seed * 92821 + b * 689287 + i * 283923481);
    k = (k ^ (k >> 15)) * 2246822519u;
    return seed ? ((k >> 8) & 0xffff) / 65535.f - 0.5f : (i % 2 ? 0.5f : -0.5f);
  };
  auto mesh = std::make_shared<Mesh>();
  mesh->name = "icon-" + etype;
  // a piece of one color (the editor's vertex colors are doubled: 0.5 is the color as it is)
  auto part = [&](Vec3 color, const std::function<void(Primitive&)>& make) {
    Primitive p;
    make(p);
    p.col.assign(p.pos.size(), Vec4{color.x * 0.5f, color.y * 0.5f, color.z * 0.5f, 1.f});
    mesh->prims.push_back(std::move(p));
  };
  auto has = [&](const char* s) { return etype.find(s) != std::string::npos; };
  const Vec3 metal{0.45f, 0.47f, 0.5f}, dark{0.2f, 0.21f, 0.23f}, bolt{0.45f, 0.8f, 1.f}, lava{1.f, 0.45f, 0.1f};
  const Vec3 yellow{0.95f, 0.8f, 0.2f};
  auto spiral = [&](Vec3 color, Vec3 floor) {
    part(floor, [&](Primitive& p) { add_tube(p, {0, 0, 0}, {0, 0.1f, 0}, 2.f, 24); });
    part(color, [&](Primitive& p) {
      std::vector<Vec3> pts;
      for (int i = 0; i <= 48; i++) {
        const float t = i / 48.f, a = t * 4 * kPi;
        pts.push_back({std::cos(a) * 1.8f * (1 - t), 0.15f, std::sin(a) * 1.8f * (1 - t)});
      }
      add_path(p, pts, 0.08f);
    });
  };
  if (etype == "climber") {
    // someone climbing (Jak, as a figure)
    part({0.2f, 0.4f, 0.85f}, [&](Primitive& p) {
      add_tube(p, {0, 0.75f, 0}, {0, 1.35f, 0}, 0.25f);
      for (float x : {-0.12f, 0.12f}) add_tube(p, {x, 0, 0}, {x, 0.8f, 0}, 0.09f);
    });
    part({0.95f, 0.75f, 0.55f}, [&](Primitive& p) {
      add_tube(p, {0, 1.4f, 0}, {0, 1.75f, 0}, 0.18f);
      for (float x : {-0.32f, 0.32f}) add_tube(p, {x, 1.3f, 0}, {x, 1.9f, 0.1f}, 0.07f);
    });
  } else if (has("board-vent") || etype == "sew-vent") {
    // a grate that blows upwards
    part(dark, [&](Primitive& p) { add_tube(p, {0, 0, 0}, {0, 0.15f, 0}, 1.4f, 20); });
    part(metal, [&](Primitive& p) {
      for (int i = -2; i <= 2; i++) add_box(p, {i * 0.5f - 0.06f, 0.15f, -1.1f}, {i * 0.5f + 0.06f, 0.25f, 1.1f}, 1.f);
    });
    part(etype == "sew-vent" ? Vec3{0.9f, 0.95f, 1.f} : bolt, [&](Primitive& p) {
      add_tube(p, {0, 0.4f, 0}, {0, 2.4f, 0}, 0.18f);
      add_cone(p, {0, 2.4f, 0}, {0, 0.9f, 0}, 0.5f);
    });
  } else if (etype == "swingpole") {
    // the bar Jak swings around
    part(metal, [&](Primitive& p) { add_tube(p, {-2.5f, 3, 0}, {2.5f, 3, 0}, 0.12f, 12); });
    part(dark, [&](Primitive& p) {
      for (float x : {-2.5f, 2.5f}) add_tube(p, {x - 0.05f, 3, 0}, {x + 0.05f, 3, 0}, 0.25f, 12);
    });
    part(yellow, [&](Primitive& p) { add_tube(p, {-0.8f, 3, 0}, {0.8f, 3, 0}, 0.14f, 12); });
  } else if (has("elec-gate") || has("vingate")) {
    // two posts and the lightning between them
    part(dark, [&](Primitive& p) {
      for (float x : {-2.f, 2.f}) add_tube(p, {x, 0, 0}, {x, 4, 0}, 0.2f);
    });
    part(bolt, [&](Primitive& p) {
      for (int b = 0; b < 3; b++) {
        std::vector<Vec3> pts;
        for (int i = 0; i <= 8; i++)
          pts.push_back({-2.f + i * 0.5f, 1.f + b + (i % 8 ? 0.5f * zig(b, i) : 0.f), i % 8 ? 0.3f * zig(b + 7, i) : 0.f});
        add_path(p, pts, 0.05f);
      }
    });
  } else if (has("conveyor")) {
    // a belt and the way it goes
    part(dark, [&](Primitive& p) { add_box(p, {-3, 0, -1.2f}, {3, 0.4f, 1.2f}, 1.f); });
    part(yellow, [&](Primitive& p) {
      for (int i = -2; i <= 2; i++)
        add_face(p, {{i - 0.35f, 0.42f, -0.8f}, {i + 0.35f, 0.42f, 0}, {i - 0.35f, 0.42f, 0.8f}}, {0, 1, 0}, 1.f);
    });
  } else if (has("laser")) {
    part(metal, [&](Primitive& p) {
      for (float x : {-3.f, 3.f}) add_box(p, {x - 0.3f, 1.2f, -0.3f}, {x + 0.3f, 1.8f, 0.3f}, 1.f);
    });
    part({1.f, 0.15f, 0.1f}, [&](Primitive& p) {
      add_tube(p, {-2.7f, 1.5f, 0}, {2.7f, 1.5f, 0}, seed ? 0.04f + 0.04f * (zig(0, 1) + 0.5f) : 0.06f);
    });
  } else if (etype == "comb-turbo") {
    // a ring of energy
    part(bolt, [&](Primitive& p) {
      std::vector<Vec3> pts;
      for (int i = 0; i <= 24; i++) {
        const float r = 2.f + (i % 24 ? 0.2f * zig(0, i) : 0.f);
        pts.push_back({std::cos(2 * kPi * i / 24) * r, 2.2f + std::sin(2 * kPi * i / 24) * r, 0});
      }
      add_path(p, pts, 0.15f);
    });
  } else if (etype == "propa") {
    // a loudspeaker on its pole
    part(dark, [&](Primitive& p) { add_tube(p, {0, 0, 0}, {0, 3, 0}, 0.08f); });
    part(metal, [&](Primitive& p) { add_box(p, {-0.5f, 3, -0.4f}, {0.5f, 3.8f, 0.4f}, 1.f); });
    part({0.9f, 0.3f, 0.25f}, [&](Primitive& p) { add_cone(p, {0, 3.4f, 1.1f}, {0, 0, -0.7f}, 0.45f); });
  } else if (has("whirlpool")) {
    spiral({0.5f, 0.75f, 1.f}, {0.15f, 0.3f, 0.5f});
  } else if (has("spinning-hole")) {
    spiral({0.85f, 0.5f, 1.f}, dark);
  } else if (has("lava")) {
    // a spout of lava
    part(dark, [&](Primitive& p) { add_cone(p, {0, 0, 0}, {0, 0.8f, 0}, 1.2f); });
    part(lava, [&](Primitive& p) {
      add_tube(p, {0, 0.6f, 0}, {0, 2.5f, 0}, 0.25f);
      for (float y : {3.f, 3.8f}) add_box(p, {-0.25f, y, -0.25f}, {0.25f, y + 0.5f, 0.25f}, 1.f);
    });
  } else if (family == "water") {
    part({0.3f, 0.55f, 0.9f}, [&](Primitive& p) { add_tube(p, {0, 0, 0}, {0, 0.1f, 0}, 2.f, 24); });
  } else if (family == "platform") {
    part(metal, [&](Primitive& p) { add_box(p, {-2, 0, -2}, {2, 0.5f, 2}, 1.f); });
  } else if (family == "hazard") {
    part(metal, [&](Primitive& p) { add_box(p, {-2, 0, -2}, {2, 0.2f, 2}, 1.f); });
    part({0.85f, 0.25f, 0.2f}, [&](Primitive& p) {
      for (float x : {-1.f, 0.f, 1.f})
        for (float z : {-1.f, 0.f, 1.f}) add_cone(p, {x, 0.2f, z}, {0, 0.9f, 0}, 0.3f, 6);
    });
  } else {
    // something the game does there: a marker with an arrow
    part(metal, [&](Primitive& p) { add_tube(p, {0, 0, 0}, {0, 0.15f, 0}, 1.2f, 16); });
    part(bolt, [&](Primitive& p) {
      add_tube(p, {0, 0.15f, 0}, {0, 1.8f, 0}, 0.15f);
      add_cone(p, {0, 1.8f, 0}, {0, 0.8f, 0}, 0.45f);
    });
  }
  return mesh;
}

json ShapeParams::to_json() const {
  json j = {{"kind", kind}, {"size", {size.x, size.y, size.z}}, {"tile", tile}};
  if (kind == "stairs") j["steps"] = steps;
  if (kind == "cylinder") j["sides"] = sides;
  if (kind == "doorway" || kind == "window") j["opening"] = {opening.x, opening.y};
  if (bend != 0) j["bend"] = bend;
  if (round > 0) j["round"] = round;
  if (kind == "ground") {
    j["bumps"] = bumps;
    j["bump_size"] = bump_size;
    j["seed"] = seed;
  }
  if (color.w > 0) j["color"] = {color.x, color.y, color.z, color.w};
  return j;
}

ShapeParams ShapeParams::from_json(const json& j) {
  ShapeParams p;
  if (!j.is_object()) return p;
  const std::string k = j.value("kind", std::string("box"));
  for (const char* kind : {"ramp", "stairs", "cylinder", "roof", "roof-hip", "doorway", "window", "ground"})
    if (k == kind) p.kind = k;
  if (j.contains("size") && j["size"].is_array() && j["size"].size() == 3)
    for (int i = 0; i < 3; i++)
      if (j["size"][i].is_number()) (&p.size.x)[i] = j["size"][i].get<float>();
  for (int i = 0; i < 3; i++) (&p.size.x)[i] = std::clamp((&p.size.x)[i], 0.05f, 2000.f);
  if (j.contains("tile") && j["tile"].is_number()) p.tile = std::clamp(j["tile"].get<float>(), 0.1f, 100.f);
  if (j.contains("steps") && j["steps"].is_number()) p.steps = std::clamp(j["steps"].get<int>(), 1, 64);
  if (j.contains("sides") && j["sides"].is_number()) p.sides = std::clamp(j["sides"].get<int>(), 3, 64);
  if (j.contains("bend") && j["bend"].is_number() && p.kind != "cylinder")
    p.bend = std::clamp(j["bend"].get<float>(), -360.f, 360.f);
  if (j.contains("bumps") && j["bumps"].is_number()) p.bumps = std::clamp(j["bumps"].get<float>(), 0.f, 20.f);
  if (j.contains("bump_size") && j["bump_size"].is_number())
    p.bump_size = std::clamp(j["bump_size"].get<float>(), 0.25f, 200.f);
  if (j.contains("seed") && j["seed"].is_number_integer()) p.seed = j["seed"].get<int>();
  if (j.contains("color") && j["color"].is_array() && j["color"].size() == 4)
    for (int i = 0; i < 4; i++)
      if (j["color"][i].is_number()) p.color[i] = std::clamp(j["color"][i].get<float>(), 0.f, 1.f);
  if (j.contains("round") && j["round"].is_number() && p.kind == "box")
    p.round = std::clamp(j["round"].get<float>(), 0.f, std::min(p.size.x, p.size.z) / 2);
  if (j.contains("opening") && j["opening"].is_array() && j["opening"].size() == 2 && j["opening"][0].is_number() &&
      j["opening"][1].is_number())
    p.opening = {j["opening"][0].get<float>(), j["opening"][1].get<float>()};
  // an opening within its wall: a post of 0.1 m at least on each side, a lintel above
  p.opening.x = std::clamp(p.opening.x, 0.1f, std::max(0.1f, p.size.x - 0.2f));
  p.opening.y = std::clamp(p.opening.y, 0.1f, std::max(0.1f, p.size.y - 0.1f));
  return p;
}

const std::vector<ShapePreset>& shape_presets() {
  static const std::vector<ShapePreset> presets = [] {
    std::vector<ShapePreset> v;
    auto add = [&](const char* name, const char* kind, Vec3 size) {
      ShapeParams p;
      p.kind = kind;
      p.size = size;
      v.push_back({name, p});
    };
    add("box", "box", {4, 4, 4});
    add("floor", "box", {8, 0.5f, 8});
    add("wall", "box", {8, 4, 0.5f});
    add("doorway", "doorway", {4, 4, 0.5f});
    add("window", "window", {4, 4, 0.5f});
    v.back().params.opening = {1.6f, 1.4f};
    add("pillar", "box", {1, 6, 1});
    add("ramp", "ramp", {4, 2, 8});
    add("stairs", "stairs", {4, 3, 6});
    add("roof", "roof", {8, 3, 8});
    add("roof-hip", "roof-hip", {8, 3, 10});
    add("cylinder", "cylinder", {2, 6, 2});
    add("ground", "ground", {16, 0.5f, 16});
    return v;
  }();
  return presets;
}

const ShapePreset* shape_preset(const std::string& name) {
  for (const auto& p : shape_presets())
    if (name == p.name) return &p;
  return nullptr;
}

std::shared_ptr<Mesh> make_shape(const ShapeParams& s, int material) {
  const float w = s.size.x / 2, h = s.size.y, d = s.size.z / 2, t = s.tile;
  Primitive p;
  p.material = material;
  // bent: cut in slices along its length first, about 4 degrees each
  g_slice_axis = s.kind == "ramp" || s.kind == "stairs" ? 2 : 0;
  g_slice = 0;
  if (std::fabs(s.bend) > 0.5f) {
    const float length_along = g_slice_axis == 0 ? s.size.x : s.size.z;
    g_slice = length_along / std::clamp(std::fabs(s.bend) / 4.f, 4.f, 96.f);
  }
  if (s.kind == "ramp") {
    // rising towards +z
    add_face(p, {{-w, 0, -d}, {w, 0, -d}, {w, 0, d}, {-w, 0, d}}, {0, -1, 0}, t);
    add_face(p, {{-w, 0, d}, {w, 0, d}, {w, h, d}, {-w, h, d}}, {0, 0, 1}, t);
    add_face(p, {{-w, 0, -d}, {w, 0, -d}, {w, h, d}, {-w, h, d}}, {0, 2 * d, -h}, t);
    add_face(p, {{w, 0, -d}, {w, 0, d}, {w, h, d}}, {1, 0, 0}, t);
    add_face(p, {{-w, 0, -d}, {-w, 0, d}, {-w, h, d}}, {-1, 0, 0}, t);
  } else if (s.kind == "stairs") {
    // rising towards +z: one block under each step
    const int n = std::max(s.steps, 1);
    for (int i = 0; i < n; i++)
      add_box(p, {-w, 0, -d + 2 * d * i / n}, {w, h * (i + 1) / n, -d + 2 * d * (i + 1) / n}, t);
  } else if (s.kind == "roof" || s.kind == "roof-hip") {
    // two slopes meeting at a ridge along z, height h; a hip roof slopes at its ends too (its ridge
    // shorter by its width, a pyramid when it is square)
    const float r = s.kind == "roof-hip" ? std::max(0.f, d - w) : d;
    add_face(p, {{-w, 0, -d}, {0, h, -r}, {0, h, r}, {-w, 0, d}}, {-h, w, 0}, t);
    add_face(p, {{w, 0, -d}, {0, h, -r}, {0, h, r}, {w, 0, d}}, {h, w, 0}, t);
    if (s.kind == "roof-hip") {
      add_face(p, {{-w, 0, d}, {w, 0, d}, {0, h, r}}, {0, d - r, h}, t);
      add_face(p, {{-w, 0, -d}, {w, 0, -d}, {0, h, -r}}, {0, d - r, -h}, t);
    } else {
      add_face(p, {{-w, 0, d}, {w, 0, d}, {0, h, d}}, {0, 0, 1}, t);
      add_face(p, {{-w, 0, -d}, {w, 0, -d}, {0, h, -d}}, {0, 0, -1}, t);
    }
    add_face(p, {{-w, 0, -d}, {w, 0, -d}, {w, 0, d}, {-w, 0, d}}, {0, -1, 0}, t);
  } else if (s.kind == "doorway" || s.kind == "window") {
    // a wall around its opening: a door from the ground, a window in the middle of its height
    const float ow = std::min(s.opening.x, s.size.x - 0.2f) / 2, oh = std::min(s.opening.y, h - 0.1f);
    const float bottom = s.kind == "window" ? (h - oh) / 2 : 0.f, top = bottom + oh;
    add_box(p, {-w, 0, -d}, {-ow, h, d}, t);
    add_box(p, {ow, 0, -d}, {w, h, d}, t);
    add_box(p, {-ow, top, -d}, {ow, h, d}, t);
    if (bottom > 0) add_box(p, {-ow, 0, -d}, {ow, bottom, d}, t);
  } else if (s.kind == "cylinder") {
    const int n = std::max(s.sides, 3);
    const float rx = w, rz = d;
    std::vector<Vec3> top, bottom;
    for (int i = 0; i < n; i++) {
      const float a = 2 * kPi * i / n;
      top.push_back({rx * std::cos(a), h, rz * std::sin(a)});
      bottom.push_back({rx * std::cos(a), 0, rz * std::sin(a)});
    }
    for (int i = 0; i < n; i++) {
      const int j = (i + 1) % n;
      const Vec3 mid = (bottom[i] + bottom[j]) * 0.5f;
      add_face(p, {bottom[i], bottom[j], top[j], top[i]}, {mid.x, 0, mid.z}, t);
    }
    add_face(p, top, {0, 1, 0}, t);
    add_face(p, bottom, {0, -1, 0}, t);
  } else if (s.kind == "ground") {
    // its top on a grid, raised and lowered by a smooth noise; its sides follow it
    const float step = std::clamp(s.bump_size / 3.f, 0.25f, 4.f);
    const int nx = s.bumps > 0 ? std::clamp((int)std::ceil(s.size.x / step), 1, 128) : 1;
    const int nz = s.bumps > 0 ? std::clamp((int)std::ceil(s.size.z / step), 1, 128) : 1;
    auto noise = [&](float x, float z) { return smooth_noise(x, z, s.seed); };
    auto height = [&](float x, float z) {
      if (s.bumps <= 0) return h;
      const float u = x / s.bump_size, v = z / s.bump_size;
      return h + s.bumps * (noise(u, v) + 0.35f * noise(u * 2.3f + 17.f, v * 2.3f + 5.f));
    };
    const uint32_t first = 0;
    for (int iz = 0; iz <= nz; iz++)
      for (int ix = 0; ix <= nx; ix++) {
        const float x = -w + 2 * w * ix / nx, z = -d + 2 * d * iz / nz;
        p.pos.push_back({x, height(x, z), z});
        p.uv.push_back({x / t, z / t});
      }
    for (int iz = 0; iz < nz; iz++)
      for (int ix = 0; ix < nx; ix++) {
        const uint32_t a = first + iz * (nx + 1) + ix, b = a + 1, c = a + nx + 1, e = c + 1;
        p.idx.insert(p.idx.end(), {a, c, b, b, c, e});
      }
    p.compute_normals();
    // the sides, along its edges, and its bottom
    auto side = [&](Vec3 a, Vec3 b, Vec3 out) {
      const float ha = height(a.x, a.z), hb = height(b.x, b.z);
      const uint32_t at = (uint32_t)p.pos.size();
      const float along = length(b - a);
      for (const Vec3& v : {Vec3{a.x, 0, a.z}, Vec3{b.x, 0, b.z}, Vec3{b.x, hb, b.z}, Vec3{a.x, ha, a.z}}) {
        p.pos.push_back(v);
        p.nrm.push_back(out);
      }
      p.uv.insert(p.uv.end(), {{0, 0}, {along / t, 0}, {along / t, -hb / t}, {0, -ha / t}});
      if (dot(cross(b - a, Vec3{0, 1, 0}), out) > 0) p.idx.insert(p.idx.end(), {at, at + 1, at + 2, at, at + 2, at + 3});
      else p.idx.insert(p.idx.end(), {at, at + 2, at + 1, at, at + 3, at + 2});
    };
    for (int ix = 0; ix < nx; ix++) {
      const float x0 = -w + 2 * w * ix / nx, x1 = -w + 2 * w * (ix + 1) / nx;
      side({x0, 0, -d}, {x1, 0, -d}, {0, 0, -1});
      side({x0, 0, d}, {x1, 0, d}, {0, 0, 1});
    }
    for (int iz = 0; iz < nz; iz++) {
      const float z0 = -d + 2 * d * iz / nz, z1 = -d + 2 * d * (iz + 1) / nz;
      side({-w, 0, z0}, {-w, 0, z1}, {-1, 0, 0});
      side({w, 0, z0}, {w, 0, z1}, {1, 0, 0});
    }
    add_face(p, {{-w, 0, -d}, {w, 0, -d}, {w, 0, d}, {-w, 0, d}}, {0, -1, 0}, t);
  } else if (s.round > 0.01f) {
    // its upright edges rounded: a rounded rectangle, raised
    const float r = std::min({s.round, w, d});
    std::vector<Vec3> ring;
    const Vec2 corners[4] = {{w - r, d - r}, {-(w - r), d - r}, {-(w - r), -(d - r)}, {w - r, -(d - r)}};
    for (int c = 0; c < 4; c++)
      for (int i = 0; i <= 6; i++) {
        const float a = kPi * 0.5f * (c + i / 6.f);
        ring.push_back({corners[c].x + r * std::cos(a), 0, corners[c].y + r * std::sin(a)});
      }
    std::vector<Vec3> top, bottom;
    for (const Vec3& v : ring) {
      bottom.push_back(v);
      top.push_back({v.x, h, v.z});
    }
    for (size_t i = 0; i < ring.size(); i++) {
      const size_t j = (i + 1) % ring.size();
      if (length(ring[j] - ring[i]) < 1e-5f) continue;
      const Vec3 mid = (ring[i] + ring[j]) * 0.5f;
      const Vec3 along = ring[j] - ring[i];
      add_face(p, {bottom[i], bottom[j], top[j], top[i]}, Vec3{along.z, 0, -along.x} * (dot({along.z, 0, -along.x}, mid) < 0 ? -1.f : 1.f), t);
    }
    add_face(p, top, {0, 1, 0}, t);
    add_face(p, bottom, {0, -1, 0}, t);
  } else {
    add_box(p, {-w, 0, -d}, {w, h, d}, t);
  }
  g_slice = 0;
  if (std::fabs(s.bend) > 0.5f) {
    // around a vertical axis: x (or z) along an arc, the other across it; the arc's middle stays
    const int a = g_slice_axis, c = 2 - a;
    const float length_along = a == 0 ? s.size.x : s.size.z;
    const float radius = length_along / radians(s.bend);
    for (size_t i = 0; i < p.pos.size(); i++) {
      Vec3& v = p.pos[i];
      const float th = v[a] / radius, rr = radius - v[c];
      const float ca = std::cos(th), sa = std::sin(th);
      const Vec3 n = p.nrm[i];
      v[a] = rr * sa;
      v[c] = radius - rr * ca;
      Vec3 m = n;
      m[a] = n[a] * ca - n[c] * sa;
      m[c] = n[a] * sa + n[c] * ca;
      p.nrm[i] = normalize(m);
    }
  }
  if (s.color.w > 0) p.col.assign(p.pos.size(), s.color);
  auto mesh = std::make_shared<Mesh>();
  mesh->name = "shape-" + s.kind;
  mesh->prims.push_back(std::move(p));
  return mesh;
}

float smooth_noise(float x, float z, int seed) {
  auto hash = [seed](int x, int z) {
    uint32_t k = (uint32_t)x * 73856093u ^ (uint32_t)z * 19349663u ^ (uint32_t)seed * 83492791u;
    k = (k ^ (k >> 13)) * 1274126177u;
    return ((k ^ (k >> 16)) & 0xffff) / 65535.f - 0.5f;
  };
  const int x0 = (int)std::floor(x), z0 = (int)std::floor(z);
  float fx = x - x0, fz = z - z0;
  fx = fx * fx * (3 - 2 * fx);
  fz = fz * fz * (3 - 2 * fz);
  const float a = hash(x0, z0) + (hash(x0 + 1, z0) - hash(x0, z0)) * fx;
  const float b = hash(x0, z0 + 1) + (hash(x0 + 1, z0 + 1) - hash(x0, z0 + 1)) * fx;
  return a + (b - a) * fz;
}

bool node_shape(const Node& n, ShapeParams* out) {
  auto it = n.extras.find("og_shape");
  if (it == n.extras.end() || !it->is_object()) return false;
  if (out) *out = ShapeParams::from_json(*it);
  return true;
}

int shape_default_material(Scene& scene) {
  for (int i = 0; i < (int)scene.materials.size(); i++)
    if (scene.materials[i].name == "ogle-grid") return i;
  // light grey, a darker line around each cell
  Texture t;
  t.name = "ogle-grid";
  t.width = t.height = 64;
  t.rgba.resize(64 * 64 * 4);
  for (int y = 0; y < 64; y++)
    for (int x = 0; x < 64; x++) {
      const bool line = x < 2 || y < 2 || x > 61 || y > 61;
      const uint8_t v = line ? 96 : 168;
      uint8_t* px = &t.rgba[(y * 64 + x) * 4];
      px[0] = px[1] = px[2] = v;
      px[3] = 255;
    }
  Material m;
  m.name = "ogle-grid";
  m.texture = scene.add_texture(std::move(t));
  return scene.add_material(std::move(m));
}

int texture_from_image(Scene& scene, const std::string& path, std::string* error) {
  std::ifstream f(to_path(path), std::ios::binary);
  const std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
  int w = 0, h = 0, comp = 0;
  unsigned char* data = bytes.empty() ? nullptr : stbi_load_from_memory(bytes.data(), (int)bytes.size(), &w, &h, &comp, 4);
  if (!data) {
    if (error) *error = bytes.empty() ? "cannot read the file" : stbi_failure_reason();
    return -1;
  }
  Texture t;
  t.name = from_path(to_path(path).stem());
  t.width = w;
  t.height = h;
  t.rgba.assign(data, data + (size_t)w * h * 4);
  stbi_image_free(data);
  return add_texture_once(scene, std::move(t));
}

int add_texture_once(Scene& scene, Texture t) {
  for (int i = 0; i < (int)scene.textures.size(); i++) {
    const Texture& d = scene.textures[i];
    if (d.name == t.name && d.width == t.width && d.height == t.height && d.rgba == t.rgba) return i;
  }
  return scene.add_texture(std::move(t));
}

size_t edit_material(Scene& scene, const std::vector<uint32_t>& ids, int material, const std::function<void(Material&)>& edit) {
  std::map<int, int> remapped;  // material -> its edited copy
  auto remap = [&](int old) {
    auto it = remapped.find(old);
    if (it != remapped.end()) return it->second;
    Material m = old >= 0 && old < (int)scene.materials.size() ? scene.materials[old] : Material{};
    edit(m);
    int found = -1;
    for (int i = 0; i < (int)scene.materials.size() && found < 0; i++)
      if (same_material(scene.materials[i], m)) found = i;
    if (found < 0) found = scene.add_material(std::move(m));
    return remapped[old] = found;
  };
  std::map<const Mesh*, std::shared_ptr<Mesh>> copies;  // shared meshes stay shared
  size_t changed = 0;
  for (auto id : ids) {
    Node* n = scene.find(id);
    if (!n || n->kind != NodeKind::Render || !n->mesh) continue;
    auto& copy = copies[n->mesh.get()];
    if (!copy) {
      bool uses = false;
      for (const auto& p : n->mesh->prims) uses |= material < 0 || p.material == material;
      if (!uses) continue;
      copy = std::make_shared<Mesh>(*n->mesh);
      for (auto& p : copy->prims)
        if (material < 0 || p.material == material) p.material = remap(p.material);
      copy->touch();
    }
    n->mesh = copy;
    changed++;
  }
  return changed;
}

void reshape(Scene& scene, Node& n, const ShapeParams& p) {
  const int material = n.mesh && !n.mesh->prims.empty() ? n.mesh->prims[0].material : shape_default_material(scene);
  n.mesh = make_shape(p, material);
  n.extras["og_shape"] = p.to_json();
}

}  // namespace ogle
