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
         a.cutoff == b.cutoff && a.double_sided == b.double_sided && a.base_color.x == b.base_color.x &&
         a.base_color.y == b.base_color.y && a.base_color.z == b.base_color.z && a.base_color.w == b.base_color.w;
}



// A flat convex face (corners in order, either way round), facing `n`: its texture repeats every
// `tile` meters, horizontal along its width and upright along its slope.
void add_face(Primitive& p, std::vector<Vec3> poly, Vec3 n, float tile) {
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

}  // namespace

json ShapeParams::to_json() const {
  json j = {{"kind", kind}, {"size", {size.x, size.y, size.z}}, {"tile", tile}};
  if (kind == "stairs") j["steps"] = steps;
  if (kind == "cylinder") j["sides"] = sides;
  if (kind == "doorway" || kind == "window") j["opening"] = {opening.x, opening.y};
  return j;
}

ShapeParams ShapeParams::from_json(const json& j) {
  ShapeParams p;
  if (!j.is_object()) return p;
  const std::string k = j.value("kind", std::string("box"));
  for (const char* kind : {"ramp", "stairs", "cylinder", "roof", "roof-hip", "doorway", "window"})
    if (k == kind) p.kind = k;
  if (j.contains("size") && j["size"].is_array() && j["size"].size() == 3)
    for (int i = 0; i < 3; i++)
      if (j["size"][i].is_number()) (&p.size.x)[i] = j["size"][i].get<float>();
  for (int i = 0; i < 3; i++) (&p.size.x)[i] = std::clamp((&p.size.x)[i], 0.05f, 2000.f);
  if (j.contains("tile") && j["tile"].is_number()) p.tile = std::clamp(j["tile"].get<float>(), 0.1f, 100.f);
  if (j.contains("steps") && j["steps"].is_number()) p.steps = std::clamp(j["steps"].get<int>(), 1, 64);
  if (j.contains("sides") && j["sides"].is_number()) p.sides = std::clamp(j["sides"].get<int>(), 3, 64);
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
  } else {
    add_box(p, {-w, 0, -d}, {w, h, d}, t);
  }
  auto mesh = std::make_shared<Mesh>();
  mesh->name = "shape-" + s.kind;
  mesh->prims.push_back(std::move(p));
  return mesh;
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
