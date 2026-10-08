#include "io/nav_export.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>

namespace ogle {

namespace {

constexpr float kUnits = 4096.f;          // game units per meter
constexpr float kMinCell = 16384.f;       // the smallest grid cell: 4 m
constexpr float kGridMargin = 1024.f;     // the grid's cells cover its box widened by this
constexpr uint32_t kDefaultResTime = 0xce6e6b28;  // -1e9: a res tag without key frame
constexpr uint32_t kScratchpad = 0x70000000;      // grid-hash spr-bucket-array

uint32_t bits(float f) {
  uint32_t u;
  std::memcpy(&u, &f, 4);
  return u;
}

// Words being laid out, with how each links (see the header): objects 16-byte aligned, basics
// with their type tag first and their pointer 4 bytes after it.
struct BlobWriter {
  std::vector<uint32_t> words;
  std::vector<std::pair<int, int>> pointers;
  std::vector<std::pair<int, std::string>> types, symbols;

  int size() const { return (int)words.size() * 4; }
  void align16() {
    while (words.size() % 4) words.push_back(0);
  }
  // zeroed room for `bytes`, 16-aligned; returns its first byte
  int reserve(int bytes) {
    align16();
    const int at = size();
    words.resize(words.size() + (bytes + 3) / 4, 0);
    return at;
  }
  // a basic of `bytes` (type tag included); returns its pointer (tag + 4)
  int basic(const std::string& type, int bytes) {
    const int tag = reserve(bytes);
    types.push_back({tag / 4, type});
    return tag + 4;
  }
  void u32(int byte, uint32_t v) { words.at(byte / 4) = v; }
  void f32(int byte, float v) { u32(byte, bits(v)); }
  void u8(int byte, uint8_t v) {
    uint32_t& w = words.at(byte / 4);
    const int s = 8 * (byte % 4);
    w = (w & ~(0xffu << s)) | ((uint32_t)v << s);
  }
  void u16(int byte, uint16_t v) {
    u8(byte, (uint8_t)v);
    u8(byte + 1, (uint8_t)(v >> 8));
  }
  void ptr(int byte, int to) { pointers.push_back({byte / 4, to}); }
  void sym(int byte, const std::string& s) { symbols.push_back({byte / 4, s}); }
  void type(int byte, const std::string& t) { types.push_back({byte / 4, t}); }
};

// res tags of an entity sorted as the game looks them up: by their first 8 characters read as a
// little-endian number (goalc/build_level/common/ResLump.cpp)
uint64_t res_key(const std::string& name) {
  uint64_t k = 0;
  std::memcpy(&k, name.data(), std::min<size_t>(8, name.size()));
  return k;
}

int write_mesh(BlobWriter& w, const NavExportEntry& e, float nearest_y, int max_users) {
  const NavMeshData& m = *e.mesh;
  const int n = (int)m.polys.size();
  AABB vb;
  for (const auto& v : m.verts) vb.add(v);
  const float radius = vb.valid() ? std::max(length(vb.lo), length(vb.hi)) : 0.f;

  // entity-nav-mesh (res-lump, entity, the mesh), then the mesh
  const int ent = w.basic("entity-nav-mesh", 56);
  const int T = ent - 4;  // offsets from the type tag, as in the type's fields
  const int mesh = w.basic("nav-mesh", 112);
  const int M = mesh - 4;
  w.f32(T + 44, 1.f);  // trans w (trans at the origin: the mesh has its own)
  w.u32(T + 48, e.aid);
  w.ptr(T + 52, mesh);

  // the route table
  const auto route = navmesh_route_table(m);
  const int route_at = w.reserve((int)route.size());
  for (size_t i = 0; i < route.size(); i++) w.u8(route_at + (int)i, route[i]);

  // the grid-hash of the polygons and its buckets
  const NavGrid g = navmesh_grid(m);
  const int grid = w.basic("grid-hash", 88);
  const int G = grid - 4;
  w.u16(G + 14, (uint16_t)g.bucket_size);
  for (int k = 0; k < 3; k++) {
    w.f32(G + 16 + 4 * k, g.axis_scale[k]);
    w.u8(G + 28 + k, (uint8_t)g.dims[k]);
    w.f32(G + 36 + 4 * k, g.box_min[k]);
    w.f32(G + 48 + 4 * k, g.box_max[k]);
  }
  w.u8(G + 31, 1);  // vertical-cell-count
  w.u16(G + 60, (uint16_t)n);
  w.u16(G + 62, (uint16_t)g.bucket_count);
  w.f32(G + 64, kMinCell);
  w.u32(G + 68, (uint32_t)g.buckets.size());
  w.u32(G + 76, kScratchpad);
  w.sym(G + 80, "#f");
  w.sym(G + 84, "#f");
  const int buckets = w.reserve((int)g.buckets.size());
  for (size_t i = 0; i < g.buckets.size(); i++) w.u8(buckets + (int)i, g.buckets[i]);
  w.ptr(G + 32, buckets);
  w.ptr(G + 72, buckets);

  // the polygons: 4 corners of 16 bytes, their w words holding id, pat, count, link, the adjacent
  // polygons, min-y and max-y
  const int polys = w.reserve(64 * n);
  for (int i = 0; i < n; i++) {
    const NavPoly& p = m.polys[i];
    const int q = polys + 64 * i;
    float lo = 1e30f, hi = -1e30f;
    for (int k = 0; k < 4; k++) {
      const Vec3 v = m.verts[p.v[k < p.count ? k : 0]] * kUnits;  // a triangle repeats its first corner
      w.f32(q + 16 * k, v.x);
      w.f32(q + 16 * k + 4, v.y);
      w.f32(q + 16 * k + 8, v.z);
      if (k < p.count) lo = std::min(lo, v.y), hi = std::max(hi, v.y);
    }
    w.u8(q + 12, (uint8_t)i);
    w.u8(q + 13, p.pat);
    w.u8(q + 14, (uint8_t)p.count);
    w.u8(q + 15, 255);  // link: none
    for (int k = 0; k < 4; k++) w.u8(q + 28 + k, (uint8_t)(k < p.count && p.adj[k] >= 0 ? p.adj[k] : 255));
    w.f32(q + 44, lo);
    w.f32(q + 60, hi);
  }

  // res tags (name, nearest-y-threshold, nav-max-users) and their data, then the name
  struct Tag {
    std::string name, type;
    int offset;
    bool inlined;
  };
  std::vector<Tag> tags = {{"name", "string", 0, false},
                           {"nearest-y-threshold", "float", 4, true},
                           {"nav-max-users", "int32", 8, true}};
  std::stable_sort(tags.begin(), tags.end(), [](const Tag& a, const Tag& b) { return res_key(a.name) < res_key(b.name); });
  const int tag_at = w.reserve(16 * (int)tags.size());
  for (size_t i = 0; i < tags.size(); i++) {
    const int t = tag_at + 16 * (int)i;
    w.sym(t, tags[i].name);
    w.u32(t + 4, kDefaultResTime);
    w.type(t + 8, tags[i].type);
    w.u32(t + 12, (uint32_t)tags[i].offset | (1u << 16) | (tags[i].inlined ? 0x80000000u : 0u));
  }
  const int data = w.reserve(16);
  w.f32(data + 4, nearest_y * kUnits);
  w.u32(data + 8, (uint32_t)max_users);
  // the string: type tag, length, characters
  const int str = w.basic("string", 8 + (int)e.name.size() + 1);
  w.u32(str, (uint32_t)e.name.size());
  for (size_t i = 0; i < e.name.size(); i++) w.u8(str + 4 + (int)i, (uint8_t)e.name[i]);
  w.ptr(data, str);
  w.align16();

  w.u32(T + 4, (uint32_t)tags.size());  // length
  w.u32(T + 8, (uint32_t)tags.size());  // allocated-length
  w.ptr(T + 12, data);                   // data-base
  w.ptr(T + 16, data + 16);              // data-top
  w.u32(T + 20, 16);                     // data-size
  w.ptr(T + 28, tag_at);                 // tags

  // the mesh
  w.ptr(M + 8, polys);
  w.u8(M + 12, 0);  // static-sphere-count (the game reads them from 'nav-mesh-sphere)
  w.u8(M + 13, (uint8_t)n);
  w.ptr(M + 16, route_at);
  w.ptr(M + 20, grid);
  const Vec3 o = m.origin * kUnits;
  w.f32(M + 48, o.x);
  w.f32(M + 52, o.y);
  w.f32(M + 56, o.z);
  w.f32(M + 60, radius * kUnits);
  return ent;
}

}  // namespace

NavGrid navmesh_grid(const NavMeshData& m) {
  NavGrid g;
  const int n = (int)m.polys.size();
  AABB vb;
  for (const auto& v : m.verts) vb.add(v * kUnits);
  if (!vb.valid() || n == 0) return g;
  // the box written reaches the farthest corner on each side of the origin (the corners' own box
  // when the origin is their center); the cells divide it widened by a quarter meter on each side
  float size[3];
  for (int k = 0; k < 3; k++) {
    const float half = std::max(std::fabs(vb.lo[k]), std::fabs(vb.hi[k]));
    g.box_min[k] = -half;
    g.box_max[k] = half;
    size[k] = std::max(kMinCell, 2 * half + 2 * kGridMargin);
  }
  // about one cell per polygon, the cells as square as the box allows (the rule the game's own
  // nav meshes follow)
  g.bucket_size = (n + 7) / 8;
  const float f = std::sqrt((float)n / (size[0] * size[2]));
  const int cap = std::min(126, n);
  g.dims[0] = std::max(1, std::min((int)(f * size[0]), cap));
  g.dims[1] = 1;
  g.dims[2] = std::max(1, std::min((int)(f * size[2]), cap));
  for (int k : {0, 2}) g.dims[k] = std::min(g.dims[k], std::max(1, (int)(size[k] / kMinCell)));
  for (int k = 0; k < 3; k++) g.axis_scale[k] = (float)g.dims[k] / size[k];
  g.axis_scale[1] = 0.f;  // one layer: every height falls in it
  g.bucket_count = g.dims[0] * g.dims[1] * g.dims[2];
  g.buckets.assign((size_t)g.bucket_count * g.bucket_size, 0);
  auto cell = [&](float p, int k) {
    return (int)std::max(0.f, std::min((p - g.box_min[k]) * g.axis_scale[k], (float)(g.dims[k] - 1)));
  };
  for (int i = 0; i < n; i++) {
    const NavPoly& p = m.polys[i];
    if (p.pat & 1) continue;
    AABB b;
    for (int k = 0; k < p.count; k++) b.add(m.verts[p.v[k]] * kUnits);
    for (int y = cell(b.lo.y, 1); y <= cell(b.hi.y, 1); y++)
      for (int z = cell(b.lo.z, 2); z <= cell(b.hi.z, 2); z++)
        for (int x = cell(b.lo.x, 0); x <= cell(b.hi.x, 0); x++) {
          const size_t c = (size_t)(x + g.dims[0] * (z + g.dims[2] * y));
          g.buckets[c * g.bucket_size + i / 8] |= (uint8_t)(1 << (i % 8));
        }
  }
  return g;
}

json navmesh_blob(const std::vector<NavExportEntry>& meshes, float nearest_y_threshold, int max_users) {
  BlobWriter w;
  json roots = json::object();
  for (size_t i = 0; i < meshes.size(); i++)
    roots["nav-mesh-" + std::to_string(i)] = write_mesh(w, meshes[i], nearest_y_threshold, max_users);
  json pointers = json::array(), types = json::array(), symbols = json::array();
  for (auto& [word, byte] : w.pointers) pointers.push_back({word, byte});
  for (auto& [word, t] : w.types) types.push_back({word, t});
  for (auto& [word, s] : w.symbols) symbols.push_back({word, s});
  return {{"words", w.words},   {"pointers", pointers},        {"types", types},
          {"symbols", symbols}, {"empty_lists", json::array()}, {"roots", roots}};
}

bool write_navmesh_blob(const std::vector<NavExportEntry>& meshes, const std::string& path, std::string* error,
                        float nearest_y_threshold, int max_users) {
  for (const auto& e : meshes) {
    const std::string problem = navmesh_problem(*e.mesh);
    if (!problem.empty()) {
      if (error) *error = e.name + ": " + problem;
      return false;
    }
  }
  std::ofstream f(std::filesystem::path(std::u8string((const char8_t*)path.c_str())), std::ios::binary);
  if (!f) {
    if (error) *error = "cannot write " + path;
    return false;
  }
  f << navmesh_blob(meshes, nearest_y_threshold, max_users).dump();
  return (bool)f;
}

float NavBlobView::f32(int byte) const {
  float f;
  const uint32_t u = u32(byte);
  std::memcpy(&f, &u, 4);
  return f;
}

int NavBlobView::ptr(int byte) const {
  for (const auto& [word, to] : pointers)
    if (word == byte / 4) return to;
  return -1;
}

bool read_navmesh_blob(const json& j, NavBlobView* out) {
  try {
    out->words = j.at("words").get<std::vector<uint32_t>>();
    out->pointers = j.at("pointers").get<std::vector<std::pair<int, int>>>();
    out->types = j.at("types").get<std::vector<std::pair<int, std::string>>>();
    out->symbols = j.at("symbols").get<std::vector<std::pair<int, std::string>>>();
    out->roots.clear();
    for (const auto& [name, byte] : j.at("roots").items()) out->roots.push_back({name, byte.get<int>()});
  } catch (...) {
    return false;
  }
  return true;
}

bool navmeshes_of_blob(const NavBlobView& b, std::vector<NavMeshData>* out) {
  out->clear();
  for (const auto& [name, ent] : b.roots) {
    if (name.rfind("nav-mesh-", 0) != 0) continue;
    const int mesh = b.ptr(ent + 48);
    if (mesh < 0) return false;
    const int polys = b.ptr(mesh + 4);
    const int n = b.u8(mesh + 9);
    NavMeshData m;
    m.origin = Vec3{b.f32(mesh + 44), b.f32(mesh + 48), b.f32(mesh + 52)} / kUnits;
    std::map<std::tuple<uint32_t, uint32_t, uint32_t>, int> index;  // shared corners
    for (int i = 0; i < n; i++) {
      const int q = polys + 64 * i;
      NavPoly p;
      p.count = b.u8(q + 14);
      p.pat = b.u8(q + 13);
      if (p.count < 3 || p.count > 4) return false;
      for (int k = 0; k < p.count; k++) {
        const auto key = std::make_tuple(b.u32(q + 16 * k), b.u32(q + 16 * k + 4), b.u32(q + 16 * k + 8));
        auto it = index.find(key);
        if (it == index.end()) {
          it = index.emplace(key, (int)m.verts.size()).first;
          m.verts.push_back(Vec3{b.f32(q + 16 * k), b.f32(q + 16 * k + 4), b.f32(q + 16 * k + 8)} / kUnits);
        }
        p.v[k] = it->second;
        const int a = b.u8(q + 28 + k);
        p.adj[k] = a == 255 ? -1 : a;
      }
      m.polys.push_back(p);
    }
    out->push_back(std::move(m));
  }
  return true;
}

}  // namespace ogle
