#include "io/game_export.h"

#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <limits>
#include <map>
#include <set>
#include <sstream>
#include <unordered_map>
#include <unordered_set>

#include "core/i18n.h"
#include "core/log.h"
#include "io/behaviors.h"
#include "io/decor_behaviors.h"
#include "io/game_data.h"
#include "io/gltf_io.h"
#include "io/nav_export.h"
#include "scene/bvh.h"

namespace ogle {

namespace fs = std::filesystem;

namespace {

fs::path to_path(const std::string& s) { return fs::path(std::u8string((const char8_t*)s.c_str())); }
std::string from_path(const fs::path& p) {
  const std::u8string u = p.generic_u8string();
  return std::string((const char*)u.data(), u.size());
}

bool write_text(const fs::path& p, const std::string& text, std::string* error) {
  std::ofstream f(p, std::ios::binary);
  if (f) f << text;
  if (!f) {
    if (error) *error = "cannot write " + from_path(p);
    return false;
  }
  return true;
}

// numbers as the game's files have them: 4 decimals at most, no exponent
std::string num(double v) {
  if (std::fabs(v) < 5e-5) v = 0;
  char buf[64];
  std::snprintf(buf, sizeof(buf), "%.4f", v);
  std::string s = buf;
  while (s.size() > 1 && s.back() == '0') s.pop_back();
  if (s.back() == '.') s += "0";
  return s;
}

// what the export changed, by note then detail
struct Notes {
  std::map<std::string, std::map<std::string, size_t>> by_key;
  std::map<std::string, size_t> counts;
  void add(const std::string& key, const std::string& detail = "", size_t n = 1) {
    counts[key] += n;
    if (!detail.empty()) by_key[key][detail] += n;
  }
  std::vector<ExportNote> list() const {
    std::vector<ExportNote> out;
    for (const auto& [key, count] : counts) {
      ExportNote note{key, count, ""};
      auto it = by_key.find(key);
      if (it != by_key.end()) {
        std::vector<std::pair<std::string, size_t>> items(it->second.begin(), it->second.end());
        std::stable_sort(items.begin(), items.end(), [](const auto& a, const auto& b) { return a.second > b.second; });
        for (size_t i = 0; i < items.size() && i < 12; i++) {
          if (i) note.detail += ", ";
          note.detail += items[i].first;
          if (items[i].second > 1) note.detail += " (" + std::to_string(items[i].second) + ")";
        }
        if (items.size() > 12) note.detail += ", ...";
      }
      out.push_back(std::move(note));
    }
    return out;
  }
};

// --- collision surfaces --------------------------------------------------------------------------

const char* kMaterials[] = {"none",   "ice",     "quicksand", "waterbottom", "tar",     "sand",   "wood",
                                "grass",  "pcmetal", "snow",      "deepsnow",    "hotcoals", "lava",  "crwood",
                                "gravel", "dirt",    "metal",     "straw",       "tube",    "swamp",  "stopproj",
                                "rotate", "neutral", "stone",     "crmetal",     "carpet",  "grmetal", "shmetal",
                                "hdwood", "squish",  "mhshroom",  "forest",      "mhswamp", "dmaker"};
const char* kEvents[] = {"none", "deadly", "endlessfall", "burn",   "deadlyup", "burnup",   "melt",
                             "slide", "lip",   "lipramp",     "shock",  "shockup",  "hide",     "rail",
                             "slippery", "drag", "waterfloor", "hang",  "fry",      "slime"};

std::string material_name(int m) {
  return m >= 0 && m < (int)(sizeof(kMaterials) / sizeof(*kMaterials)) ? kMaterials[m]
                                                                       : "material " + std::to_string(m);
}
std::string event_name(int e) {
  return e >= 0 && e < (int)(sizeof(kEvents) / sizeof(*kEvents)) ? kEvents[e] : "event " + std::to_string(e);
}

// --- actors --------------------------------------------------------------------------------------

std::string strip_quote(const std::string& s) { return !s.empty() && s[0] == '\'' ? s.substr(1) : s; }

// the numbers of a lump value, flattened
void numbers(const json& v, std::vector<const json*>& out) {
  if (v.is_number()) out.push_back(&v);
  else if (v.is_array())
    for (const auto& x : v) numbers(x, out);
}

// a guess of the type of an untyped lump value (projects of older extractions)
std::string guess_type(const json& v) {
  if (v.is_string()) return strip_quote(v.get<std::string>()) != v.get<std::string>() ? "symbol" : "string";
  if (v.is_number_integer()) return "int32";
  if (v.is_number()) return "float";
  if (v.is_array() && !v.empty()) {
    if (v[0].is_string()) return guess_type(v[0]);
    if (v[0].is_array()) return "vector";
    bool ints = true;
    for (const auto& x : v) ints &= x.is_number_integer();
    return ints ? "int32" : "float";
  }
  return "";
}

// --- regions -------------------------------------------------------------------------------------

// A script of a region as the builder writes it: its integers as floats (the builder would make
// them a basic, where the game has a fixnum; the commands read a float as well)
std::string builder_script(const std::string& script) {
  std::string out;
  bool in_string = false;
  for (size_t i = 0; i < script.size(); i++) {
    const char c = script[i];
    if (c == '"' && (i == 0 || script[i - 1] != '\\')) in_string = !in_string;
    const bool start = !in_string && (std::isdigit((unsigned char)c) || (c == '-' && i + 1 < script.size() &&
                                                                       std::isdigit((unsigned char)script[i + 1]))) &&
                       (i == 0 || script[i - 1] == ' ' || script[i - 1] == '(');
    if (!start) {
      out += c;
      continue;
    }
    size_t end = i + 1;
    while (end < script.size() && script[end] != ' ' && script[end] != ')') end++;
    std::string token = script.substr(i, end - i);
    const bool integer = token.find_first_not_of("-0123456789") == std::string::npos;
    out += integer ? token + ".0" : token;
    i = end - 1;
  }
  return out;
}

// The regions for the builder: one prim per id (a region of the game with several faces gets a new
// id for each other face, from `next_id`), the scripts as it writes them
json builder_regions(const json& trees, uint32_t next_id, size_t* count) {
  json out = json::object();
  std::set<uint32_t> taken;
  for (const auto& [name, tree] : trees.items())
    if (tree.is_object() && tree.contains("regions"))
      for (const auto& r : tree["regions"]) taken.insert(r.value("id", 0u));
  std::set<uint32_t> seen;
  for (const auto& [name, tree] : trees.items()) {
    if (!tree.is_object() || !tree.contains("bsphere") || !tree.contains("regions")) continue;
    json regions = json::array();
    for (json r : tree["regions"]) {
      uint32_t id = r.value("id", 0u);
      if (!seen.insert(id).second) {
        while (taken.count(next_id)) next_id++;
        id = next_id++;
        taken.insert(id);
        seen.insert(id);
      }
      r["id"] = id;
      for (const char* k : {"on-enter", "on-inside", "on-exit"})
        if (r.contains(k) && r[k].is_string()) r[k] = builder_script(r[k].get<std::string>());
      regions.push_back(r);
    }
    if (count) *count += regions.size();
    out[name] = {{"bsphere", tree["bsphere"]}, {"regions", regions}};
  }
  return out;
}

// --- the level's definition in the game's code ---------------------------------------------------

std::string meters(float v) { return "(meters " + num(v) + ")"; }

std::string level_info(const LevelExportOptions& o, float bottom) {
  const Vec3 s = o.start, c = o.start + Vec3{0, 3, -6};
  std::ostringstream t;
  t << "(define " << o.name << "\n"
    << "  (new 'static 'level-load-info\n"
    << "    :name '" << o.name << "\n"
    << "    :visname '" << o.name << "-vis\n"
    << "    :nickname '" << o.nickname << "\n"
    << "    :dbname '" << o.name << "\n"
    << "    :taskname 'default\n"
    << "    :index #x128 ;; the test-zone's: give it a number no other level has\n"
    << "    :master-level #f\n"
    << "    :packages '()\n"
    << "    :run-packages '(\"common\")\n"
    << "    :memory-mode (level-memory-mode large)\n"
    << "    :music-bank #f\n"
    << "    :extra-sound-bank #f\n"
    << "    :mood-func 'update-mood-default\n"
    << "    :special-mood #f\n"
    << "    :ocean #f\n"
    << "    :ocean-alpha 1.0\n"
    << "    :priority 100\n"
    << "    :draw-priority 10.0\n"
    << "    :base-task-mask (task-mask task0)\n"
    << "    :bigmap-id (bigmap-id none)\n"
    << "    :continues '((new 'static 'continue-point\n"
    << "        :name \"" << o.name << "-start\"\n"
    << "        :level '" << o.name << "\n"
    << "        :trans (static-vectorm " << num(s.x) << " " << num(s.y) << " " << num(s.z) << ")\n"
    << "        :camera-trans (static-vectorm " << num(c.x) << " " << num(c.y) << " " << num(c.z) << ")\n"
    << "        :quat (new 'static 'vector4h :data (new 'static 'array int16 4 0 0 0 #x7fff))\n"
    << "        :camera-rot (new 'static 'array int16 9 #x7fff 0 0 0 #x7fff 0 0 0 #x7fff)\n"
    << "        :on-goto #f\n"
    << "        :vis-nick '" << o.nickname << "\n"
    << "        :vehicle-type #x1b\n"
    << "        :want-count 1\n"
    << "        :want (new 'static 'inline-array level-buffer-state-small 1\n"
    << "          (new 'static 'level-buffer-state-small :name '" << o.name << " :display? 'display)\n"
    << "          )\n"
    << "        :want-sound (new 'static 'array symbol 3 #f #f #f)\n"
    << "        )\n"
    << "      )\n"
    << "    :callback-list '()\n"
    << "    :borrow #f\n"
    << "    :bottom-height " << meters(bottom) << "\n"
    << "    :fog-height (meters 80)\n"
    << "    :max-rain 1.0\n"
    << "    :fog-mult 1.0\n"
    << "    :mood-range (new 'static 'mood-range :max-cloud 1.0 :min-fog 0.5 :max-fog 1.0)\n"
    << "    )\n"
    << "  )\n\n"
    << "(#when PC_PORT\n"
    << "  (cons! *level-load-list* '" << o.name << ")\n"
    << "  )\n";
  return t.str();
}

// a surface nothing stands on: glows and light shafts, water and glass drawn translucent (the
// levels' own materials are blended with their texture's alpha almost everywhere: that alone does
// not make them see-through), the same rule as the nav meshes'
bool see_through(const Scene& scene, const Mesh& mesh) {
  bool any = false;
  for (const auto& p : mesh.prims) {
    if (p.idx.empty()) continue;
    if (p.material < 0 || p.material >= (int)scene.materials.size() || !navmesh_ignores(scene.materials[p.material]))
      return false;
    any = true;
  }
  return any;
}

// The point of triangle abc nearest to p (Ericson, Real-Time Collision Detection, 5.1.5)
Vec3 closest_on_triangle(const Vec3& p, const Vec3& a, const Vec3& b, const Vec3& c) {
  const Vec3 ab = b - a, ac = c - a, ap = p - a;
  const float d1 = dot(ab, ap), d2 = dot(ac, ap);
  if (d1 <= 0 && d2 <= 0) return a;
  const Vec3 bp = p - b;
  const float d3 = dot(ab, bp), d4 = dot(ac, bp);
  if (d3 >= 0 && d4 <= d3) return b;
  const float vc = d1 * d4 - d3 * d2;
  if (vc <= 0 && d1 >= 0 && d3 <= 0) return a + ab * (d1 / (d1 - d3));
  const Vec3 cp = p - c;
  const float d5 = dot(ab, cp), d6 = dot(ac, cp);
  if (d6 >= 0 && d5 <= d6) return c;
  const float vb = d5 * d2 - d1 * d6;
  if (vb <= 0 && d2 >= 0 && d6 <= 0) return a + ac * (d2 / (d2 - d6));
  const float va = d3 * d6 - d5 * d4;
  if (va <= 0 && d4 - d3 >= 0 && d5 - d6 >= 0) return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
  const float k = 1.f / (va + vb + vc);
  return a + ab * (vb * k) + ac * (vc * k);
}

// Triangles of the game's space hashed on a grid of `cell` meters: the distance from a point to the
// nearest, how many meet a box.
struct TriGrid {
  explicit TriGrid(float cell_size) : cell(cell_size) {}
  float cell;
  std::vector<std::array<Vec3, 3>> tris;
  std::vector<AABB> boxes;
  std::unordered_map<uint64_t, std::vector<uint32_t>> cells;
  mutable std::vector<uint32_t> seen;  // the last query that met each triangle
  mutable uint32_t query = 0;

  static uint64_t key(int x, int y, int z) {
    return ((uint64_t)(uint32_t)(x + (1 << 20)) << 42) | ((uint64_t)(uint32_t)(y + (1 << 20)) << 21) |
           (uint64_t)(uint32_t)(z + (1 << 20));
  }
  int at(float v) const { return (int)std::floor(v / cell); }
  bool empty() const { return tris.empty(); }
  void add(const Vec3& a, const Vec3& b, const Vec3& c) {
    AABB box;
    box.add(a);
    box.add(b);
    box.add(c);
    const uint32_t t = (uint32_t)tris.size();
    tris.push_back({a, b, c});
    boxes.push_back(box);
    const int64_t span = (int64_t)(at(box.hi.x) - at(box.lo.x) + 1) * (at(box.hi.y) - at(box.lo.y) + 1) *
                         (at(box.hi.z) - at(box.lo.z) + 1);
    if (span <= 64) {
      for (int x = at(box.lo.x); x <= at(box.hi.x); x++)
        for (int y = at(box.lo.y); y <= at(box.hi.y); y++)
          for (int z = at(box.lo.z); z <= at(box.hi.z); z++) cells[key(x, y, z)].push_back(t);
      return;
    }
    // a large one (a level's ground, a far mountain): the cells its surface crosses, not all the
    // cells of its box (a slanted triangle of a kilometer would fill millions)
    const float longest = std::max({length(b - a), length(c - b), length(a - c)});
    const int n = std::clamp((int)std::ceil(longest / (cell * 0.5f)), 1, 2048);
    std::unordered_set<uint64_t> crossed;
    for (int i = 0; i <= n; i++)
      for (int j = 0; i + j <= n; j++) {
        const Vec3 p = a + (b - a) * ((float)i / n) + (c - a) * ((float)j / n);
        crossed.insert(key(at(p.x), at(p.y), at(p.z)));
      }
    for (uint64_t k : crossed) cells[k].push_back(t);
  }
  // f(t) once for each triangle in the cells the box covers
  template <typename F>
  void each_near(const AABB& box, F&& f) const {
    if (seen.size() < tris.size()) seen.resize(tris.size(), 0);
    query++;
    for (int x = at(box.lo.x); x <= at(box.hi.x); x++)
      for (int y = at(box.lo.y); y <= at(box.hi.y); y++)
        for (int z = at(box.lo.z); z <= at(box.hi.z); z++) {
          auto it = cells.find(key(x, y, z));
          if (it == cells.end()) continue;
          for (uint32_t t : it->second)
            if (seen[t] != query) {
              seen[t] = query;
              f(t);
            }
        }
  }
  // the distance to the nearest triangle, `limit` when none is nearer
  float distance(const Vec3& p, float limit) const {
    float best = limit;
    AABB box;
    box.add(p - Vec3{limit, limit, limit});
    box.add(p + Vec3{limit, limit, limit});
    each_near(box, [&](uint32_t t) {
      const auto& tr = tris[t];
      best = std::min(best, length(closest_on_triangle(p, tr[0], tr[1], tr[2]) - p));
    });
    return best;
  }
  size_t count(const AABB& box) const {
    size_t n = 0;
    each_near(box, [&](uint32_t t) { n += boxes[t].overlaps(box); });
    return n;
  }
  // at least count(box): the triangles of the cells it covers, a triangle over several cells
  // counted in each (cheap)
  size_t bound(const AABB& box) const {
    size_t n = 0;
    for (int x = at(box.lo.x); x <= at(box.hi.x); x++)
      for (int y = at(box.lo.y); y <= at(box.hi.y); y++)
        for (int z = at(box.lo.z); z <= at(box.hi.z); z++)
          if (auto it = cells.find(key(x, y, z)); it != cells.end()) n += it->second.size();
    return n;
  }
};

// The level whose decor a node is part of: <level> of its outermost ancestor <level>-tfrag-*,
// -tie-* or -shrub-* (the draw groups of an extracted level), and that group's parent (which copy
// of the level, in a document); "" for none
std::pair<uint32_t, std::string> piece_level(const Scene& scene, const Node& n) {
  std::pair<uint32_t, std::string> found{0, ""};
  int guard = 0;
  for (const Node* p = n.parent ? scene.find(n.parent) : nullptr; p && guard++ < 1000;
       p = p->parent ? scene.find(p->parent) : nullptr)
    for (const char* kind : {"-tfrag-", "-tie-", "-shrub-"})
      if (const size_t at = p->name.find(kind); at != std::string::npos && at > 0) found = {p->parent, p->name.substr(0, at)};
  return found;
}

// The parts of triangle abc to keep, `gone(point)` saying where the collision has no piece left:
// all of it, none, or (when its corners and its middle disagree) its quarters, each decided again,
// down to 1 m
template <typename Gone, typename Out>
void keep_parts(const Vec3& a, const Vec3& b, const Vec3& c, int depth, Gone& gone, Out& out) {
  const bool middle = gone((a + b + c) * (1.f / 3.f));
  const float longest = std::max(std::max(length2(b - a), length2(c - b)), length2(a - c));
  if (depth < 6 && longest > 1.f && (gone(a) != middle || gone(b) != middle || gone(c) != middle)) {
    const Vec3 ab = (a + b) * 0.5f, bc = (b + c) * 0.5f, ca = (c + a) * 0.5f;
    keep_parts(a, ab, ca, depth + 1, gone, out);
    keep_parts(ab, b, bc, depth + 1, gone, out);
    keep_parts(ca, bc, c, depth + 1, gone, out);
    keep_parts(ab, bc, ca, depth + 1, gone, out);
    return;
  }
  if (!middle) out(a, b, c);
}

// Jak 3 gathers the collision around Jak in a cache of 461 triangles (collide-cache-h.gc); beyond
// it, the triangles of the place are dropped and Jak can fall through the floor. The box it fills,
// around his feet: his root sphere (2.2 m, 1.2 m up) and 1 m more (fill-cache-for-shape).
constexpr size_t kCollideCacheTris = 460;
// the pieces of decor colliding by their shape are 4 m across at least (walls, floors, catwalks,
// stairs); the smaller ones (unless given a collision in the editor, or shapes of the editor) none
constexpr float kSmallPiece = 4.f;
const Vec3 kCacheBoxLo{-3.2f, -2.f, -3.2f}, kCacheBoxHi{3.2f, 4.4f, 3.2f};

// The places of the floors (the faces up, a point every 3 m) where the cache would get more
// triangles than it holds (`sides`: 2 when the builder doubles every face): how many, and where,
// the densest first
std::vector<std::pair<size_t, Vec3>> collision_hot_spots(const TriGrid& all, size_t sides) {
  std::unordered_map<uint64_t, Vec3> floors;
  auto sample = [&](const Vec3& p) {
    floors.emplace(TriGrid::key((int)std::floor(p.x / 3.f), (int)std::floor(p.y), (int)std::floor(p.z / 3.f)), p);
  };
  for (const auto& t : all.tris) {
    const Vec3 n = cross(t[1] - t[0], t[2] - t[0]);
    const float len = length(n);
    if (len <= 0.f || n.y < 0.7f * len) continue;  // facing up
    const float longest = std::sqrt(std::max(std::max(length2(t[1] - t[0]), length2(t[2] - t[1])), length2(t[0] - t[2])));
    const int steps = std::min(64, (int)std::ceil(longest / 3.f));
    if (steps <= 1) {
      sample((t[0] + t[1] + t[2]) * (1.f / 3.f));
      continue;
    }
    for (int i = 0; i <= steps; i++)
      for (int j = 0; i + j <= steps; j++)
        sample(t[0] + (t[1] - t[0]) * ((float)i / steps) + (t[2] - t[0]) * ((float)j / steps));
  }
  std::vector<std::pair<size_t, Vec3>> out;
  for (const auto& [key, at] : floors) {
    AABB box;
    box.add(at + kCacheBoxLo);
    box.add(at + kCacheBoxHi);
    if (all.bound(box) * sides <= kCollideCacheTris) continue;
    const size_t n = all.count(box) * sides;
    if (n > kCollideCacheTris) out.push_back({n, at});
  }
  std::sort(out.begin(), out.end(), [](const auto& a, const auto& b) { return a.first > b.first; });
  return out;
}

}  // namespace

// ------------------------------------------------------------------------------------------------

std::string default_nickname(const std::string& name) {
  // "m" (mine: apart from the game's levels) and the first letters of the name, after "my-"
  std::string out = "m";
  const std::string base = name.rfind("my-", 0) == 0 ? name.substr(3) : name;
  for (char c : base)
    if (std::isalnum((unsigned char)c) && out.size() < 3) out += (char)std::tolower((unsigned char)c);
  while (out.size() < 3) out += 'x';
  return out;
}

std::string default_iso_name(const std::string& name) {
  std::string out;
  for (char c : name)
    if (std::isalnum((unsigned char)c) && out.size() < 8) out += (char)std::toupper((unsigned char)c);
  return out.empty() ? "CUSTOM" : out;
}

std::string check_export_names(const LevelExportOptions& o, const GameData* data) {
  auto symbol_ok = [](const std::string& s) {
    if (s.empty() || !std::islower((unsigned char)s[0])) return false;
    for (char c : s)
      if (!(std::islower((unsigned char)c) || std::isdigit((unsigned char)c) || c == '-')) return false;
    return s.back() != '-';
  };
  if (!symbol_ok(o.name) || o.name.size() < 3 || o.name.size() > 10) return "export.bad.name";
  if (o.nickname.size() != 3 || !symbol_ok(o.nickname)) return "export.bad.nickname";
  if (o.iso_name.empty() || o.iso_name.size() > 8) return "export.bad.iso_name";
  for (char c : o.iso_name)
    if (!(std::isupper((unsigned char)c) || std::isdigit((unsigned char)c))) return "export.bad.iso_name";
  if (o.folder.empty()) return "export.bad.folder";
  if (data) {
    // the game's own levels (the ones whose DGO the game builds): their files would be written over
    for (const auto& [name, info] : data->levels()) {
      if (info.dgo.empty()) continue;
      if (name == o.name) return "export.bad.name_taken";
      if (info.nickname == o.nickname) return "export.bad.nickname_taken";
    }
    if (!o.replaces.empty()) {
      const LevelInfo* info = data->level(o.replaces);
      if (!info || info->dgo.empty() || info->layer) return "export.bad.replaces";
    }
  }
  return {};
}

DefaultCollision default_decor_collision(const Scene& scene, size_t index) {
  const SceneCache& cache = scene.cache();
  if (index >= scene.nodes.size() || index >= cache.bounds.size() || !scene.nodes[index].mesh) return DefaultCollision::Shape;
  const Node& n = scene.nodes[index];
  // a piece of a level whose collision is in the document: its "<level>-collision" group, next to it
  const auto level = piece_level(scene, n);
  if (!level.second.empty()) {
    const std::string group = level.second + "-collision";
    for (const auto& g : scene.nodes)
      if (g.kind == NodeKind::Group && g.parent == level.first && g.name == group) return DefaultCollision::Game;
  }
  if (see_through(scene, *n.mesh)) return DefaultCollision::SeeThrough;
  const AABB& b = cache.bounds[index];
  if (!n.extras.contains("og_shape") && b.valid() && std::max({b.size().x, b.size().y, b.size().z}) < kSmallPiece)
    return DefaultCollision::Small;
  return DefaultCollision::Shape;
}

bool own_level_node(const Scene& scene, size_t i) {
  const SceneCache& c = scene.cache();
  if (i >= scene.nodes.size()) return false;
  auto marked = [](const Node& n) {
    return n.kind == NodeKind::Group && !n.extras.empty() &&
           (n.extras.contains("og_level") || n.extras.contains("og_companion") || n.extras.contains("og_layer") ||
            n.extras.contains("og_reference"));
  };
  if (c.parent.size() == c.count && i < c.count) {
    for (int32_t k = (int32_t)i, guard = 0; k >= 0 && guard < 1000; k = c.parent[k], guard++)
      if (marked(scene.nodes[k])) return false;
    return true;
  }
  int guard = 0;
  for (const Node* n = &scene.nodes[i]; n && guard < 1000; n = n->parent ? scene.find(n->parent) : nullptr, guard++)
    if (marked(*n)) return false;
  return true;
}

std::vector<ContinueCheck> check_continues(const Scene& scene, const GameData* data, const std::string& level) {
  std::vector<ContinueCheck> out;
  const LevelInfo* info = data ? data->level(level) : nullptr;
  if (!info) return out;
  const SceneCache& cache = scene.cache();
  const size_t count = std::min(scene.nodes.size(), cache.count);
  std::vector<size_t> ground;  // the document's own decor shown
  for (size_t i = 0; i < count; i++)
    if (scene.nodes[i].kind == NodeKind::Render && scene.nodes[i].mesh && cache.visible[i] && own_level_node(scene, i))
      ground.push_back(i);
  for (const auto& cp : info->continues) {
    ContinueCheck c;
    c.name = cp.name;
    c.position = cp.position;
    c.ground = std::numeric_limits<float>::quiet_NaN();
    // from a little above the point, down to a few meters below it
    const Ray ray{cp.position + Vec3{0, 2.f, 0}, {0, -1, 0}};
    float best = 8.f;
    for (size_t i : ground) {
      if (!ray_aabb(ray, cache.bounds[i].expanded(0.01f), best)) continue;
      const Node& n = scene.nodes[i];
      const Ray local = transform_ray(cache.worlds[i].inverse(), ray);
      for (size_t p = 0; p < n.mesh->prims.size(); p++) {
        float t;
        uint32_t tri;
        if (n.mesh->bvh(p).raycast(local, n.mesh->prims[p].pos, n.mesh->prims[p].idx, best, &t, &tri)) best = t;
      }
    }
    if (best < 8.f) c.ground = ray.at(best).y;
    out.push_back(std::move(c));
  }
  return out;
}

bool BuilderSurface::operator<(const BuilderSurface& o) const {
  auto key = [](const BuilderSurface& s) {
    return std::tuple(s.material, s.event, s.mode, s.noentity, s.nocamera, s.noedge, s.nolineofsight);
  };
  return key(*this) < key(o);
}

BuilderSurface builder_surface(uint32_t pat, std::string* loss) {
  BuilderSurface s;
  std::string lost;
  s.noentity = pat & 1;
  s.nocamera = (pat >> 1) & 1;
  s.noedge = (pat >> 2) & 1;
  // goal_src/jak3/engine/collide/pat-h.gc: the builder takes Jak 1's fields and converts them,
  // Jak 1's stone (0) becoming Jak 3's stone (23); what Jak 1 does not have becomes the nearest
  const int mode = (pat >> 7) & 7, material = (pat >> 10) & 63, event = (pat >> 18) & 63;
  s.nolineofsight = (pat >> 16) & 1;
  s.mode = mode;
  if (mode > 2) {
    s.mode = 0;  // the halfpipes of the boards: ground
    lost = "halfpipe";
  }
  static const int kNearest[] = {
      0,  1,  2,  3,  4,  5,  6,  7,  8,  9,  10, 11, 12, 13, 14, 15, 16,
      17, 18, 19, 20, 21, 22,
      0,   // stone
      16,  // crmetal: metal
      17,  // carpet: straw
      16,  // grmetal: metal
      16,  // shmetal: metal
      6,   // hdwood: wood
      19,  // squish: swamp
      7,   // mhshroom: grass
      7,   // forest: grass
      19,  // mhswamp: swamp
      16,  // dmaker: metal
  };
  const int count = (int)(sizeof(kNearest) / sizeof(*kNearest));
  s.material = material < count ? kNearest[material] : 0;
  // the level builder's collision fields are written in Jak 1's own layout (the builder converts
  // them: goalc/build_level/collide/common); a surface without a material (none) becomes stone
  const bool kept = material == 23 || (material >= 1 && material <= 22);
  if (!kept) lost = material_name(material);
  s.event = event <= 6 ? event : 0;
  if (event > 6) lost = event_name(event);
  // the flags Jak 1's fields do not have (boards, vehicles): the builder sets none of them.
  // nogrind is on nearly every surface (the board grinds rails only), and off on every surface
  // of the levels it builds: not said
  if (lost.empty()) {
    static const std::pair<int, const char*> kFlags[] = {{4, "nojak"},          {5, "noboard"}, {6, "nopilot"},
                                                         {24, "probe"},         {25, "nomech"}, {26, "noproj"},
                                                         {27, "noendlessfall"}, {28, "noprobe"}};
    for (const auto& [bit, name] : kFlags)
      if (pat >> bit & 1) lost += (lost.empty() ? "" : "+") + std::string(name);
  }
  if (loss) *loss = lost;
  return s;
}

json builder_lump_value(const json& value, const std::string& type_in) {
  const std::string type = type_in.empty() ? guess_type(value) : type_in;
  auto strings = [&](const char* tag, bool strip) -> json {
    json out = json::array({tag});
    if (value.is_string()) out.push_back(strip ? strip_quote(value.get<std::string>()) : value.get<std::string>());
    else if (value.is_array())
      for (const auto& x : value)
        if (x.is_string()) out.push_back(strip ? strip_quote(x.get<std::string>()) : x.get<std::string>());
    return out.size() > 1 ? out : json();
  };
  if (type == "string") {
    // a plain string is one; one beginning with a quote would be read as a symbol
    if (value.is_string() && value.get<std::string>().rfind('\'', 0) != 0) return value;
    return strings("string", false);
  }
  if (type == "symbol") {
    if (value.is_string()) return "'" + strip_quote(value.get<std::string>());
    return strings("symbol", true);
  }
  if (type == "type") return strings("type", true);
  if (type == "pair") return value.is_string() ? json::array({"pair", value}) : json();
  if (type == "float" || type == "int32" || type == "int16" || type == "int8" || type == "uint32" ||
      type == "uint16" || type == "uint8") {
    std::vector<const json*> ns;
    numbers(value, ns);
    if (ns.empty()) return json();
    const bool is_float = type == "float", is_signed = type[0] == 'i';
    json out = json::array({is_float ? "float" : is_signed ? "int32" : "uint32"});
    for (const json* n : ns) {
      const double d = n->get<double>();
      if (is_float) out.push_back(d);
      else if (is_signed) out.push_back((int32_t)std::llround(d));
      else out.push_back((uint32_t)(int64_t)std::llround(d));
    }
    return out;
  }
  if (type == "vector") {
    if (!value.is_array() || value.empty()) return json();
    json out = json::array({"vector"});
    auto vec4 = [](const json& v) {
      json r = json::array();
      for (size_t i = 0; i < 4; i++) r.push_back(i < v.size() && v[i].is_number() ? v[i].get<double>() : (i == 3 ? 1.0 : 0.0));
      return r;
    };
    if (value[0].is_array()) {
      for (const auto& v : value)
        if (v.is_array()) out.push_back(vec4(v));
    } else {
      out.push_back(vec4(value));
    }
    return out;
  }
  return json();
}

bool export_level_for_game(const Scene& scene, const GameData* data, const LevelExportOptions& o,
                           LevelExportReport* report, std::string* error) {
  LevelExportReport rep;
  if (const std::string bad = check_export_names(o, data); !bad.empty()) {
    if (error) *error = tr(bad.c_str());
    return false;
  }
  // a level replacing one of the game's: the document's own level alone (the levels added next to
  // it are the game's, loaded by the game as they are); never a level shown for reference
  const bool replacing = !o.replaces.empty();
  const LevelInfo* replaced = replacing && data ? data->level(o.replaces) : nullptr;
  // the export's folder, laid out like the jak-project checkout it is copied into
  const std::string install_dir = "custom_assets/jak3/levels/" + o.name + "/";
  const std::string code_dir = "goal_src/jak3/levels/" + o.name + "/";
  const fs::path top = to_path(o.folder);
  const fs::path dir = top / "custom_assets" / "jak3" / "levels" / to_path(o.name);
  std::error_code ec;
  fs::create_directories(dir, ec);
  if (!fs::is_directory(dir, ec)) {
    if (error) *error = "cannot create " + from_path(dir);
    return false;
  }
  rep.dir = from_path(top);
  rep.level_dir = from_path(dir);
  const uint32_t base_id = o.base_id ? o.base_id : kDefaultBaseId;
  Notes notes;

  const SceneCache& cache = scene.cache();
  const size_t count = std::min(scene.nodes.size(), cache.visible.size());
  auto exported_part = [&](size_t i) {
    if (replacing) return own_level_node(scene, i);
    return !(i < cache.reference.size() && cache.reference[i]);
  };
  size_t left_out = 0;  // the elements of the levels next to it (replacing)
  std::unordered_map<uint32_t, size_t> index;
  for (size_t i = 0; i < count; i++) index[scene.nodes[i].id] = i;
  auto parent_index = [&](const Node& n) -> int {
    auto it = index.find(n.parent);
    return it == index.end() ? -1 : (int)it->second;
  };

  // the levels with the game's collision: the parent of their "<level>-collision" group, and <level>
  // (a level is told from another copy of it, a reference, by that parent)
  std::set<std::pair<uint32_t, std::string>> with_collision;
  for (size_t i = 0; i < count; i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Collision || !n.mesh || !exported_part(i)) continue;
    const int p = parent_index(n);
    if (p < 0) continue;
    const Node& g = scene.nodes[p];
    const std::string suffix = "-collision";
    if (g.name.size() > suffix.size() && g.name.compare(g.name.size() - suffix.size(), suffix.size(), suffix) == 0)
      with_collision.insert({g.parent, g.name.substr(0, g.name.size() - suffix.size())});
  }
  // The pieces of a level's decor still where the game has them: its collision holds for them. The
  // game's collision of the others (deleted, moved, hidden, or given a collision of their own in
  // the editor) is taken out where they were: the level as extracted (o.originals) tells them apart.
  struct Pieces {
    bool known = false;  // compared with the level as extracted
    std::unordered_set<std::string> kept;
    TriGrid kept_tris{4.f}, removed_tris{4.f};  // 4 m cells: searched 8 m around
    size_t removed = 0;
  };
  std::map<std::pair<uint32_t, std::string>, Pieces> pieces;
  if (o.game_collision) {
    for (const auto& level : with_collision) {
      auto orig_it = o.originals.find(level.second);
      if (orig_it == o.originals.end() || !orig_it->second) continue;
      const Scene& orig = *orig_it->second;
      // the document's pieces of that level, shown, where they are
      std::unordered_map<std::string, AABB> here;
      for (size_t i = 0; i < count; i++) {
        const Node& n = scene.nodes[i];
        if (n.kind != NodeKind::Render || !n.mesh || cache.actor[i] || !cache.visible[i]) continue;
        if (decor_behavior_of(n.extras).collision.set) continue;
        if (piece_level(scene, n) == level) here[n.name] = cache.bounds[i];
      }
      Pieces& ps = pieces[level];
      ps.known = true;
      std::vector<const Node*> kept, removed;
      for (const Node& n : orig.nodes) {
        if (n.kind != NodeKind::Render || !n.mesh || piece_level(orig, n).second != level.second) continue;
        const AABB was = orig.world_bounds(n);
        auto h = here.find(n.name);
        const bool same =
            h != here.end() && length(h->second.lo - was.lo) < 0.05f && length(h->second.hi - was.hi) < 0.05f;
        if (same) {
          ps.kept.insert(n.name);
          kept.push_back(&n);
        } else {
          removed.push_back(&n);
        }
      }
      ps.removed = removed.size();
      if (removed.empty()) continue;  // nothing to take out
      auto add = [&](TriGrid& grid, const Node& n) {
        const Mat4 w = orig.world(n);
        for (const auto& prim : n.mesh->prims)
          for (size_t t = 0; t + 2 < prim.idx.size(); t += 3)
            grid.add(w.point(prim.pos[prim.idx[t]]), w.point(prim.pos[prim.idx[t + 1]]),
                     w.point(prim.pos[prim.idx[t + 2]]));
      };
      for (const Node* n : removed) add(ps.removed_tris, *n);
      for (const Node* n : kept) add(ps.kept_tris, *n);
    }
  }
  // the game's collision holds for a piece of a level that has it, still in place
  auto covered_by_game = [&](size_t i) {
    const Node& n = scene.nodes[i];
    const auto level = piece_level(scene, n);
    if (level.second.empty() || !with_collision.count(level)) return false;
    auto it = pieces.find(level);
    return it == pieces.end() || !it->second.known || it->second.kept.count(n.name) != 0;
  };

  // --- the decor -------------------------------------------------------------------------------
  ExportOptions eo;
  eo.mode = ExportMode::Builder;
  size_t hidden = 0;
  std::vector<size_t> decor_actions;  // the elements that bounce or launch Jak: actors of the level's code
  std::vector<size_t> by_shape;       // the elements whose collision the builder makes from their shape
  AABB bounds;
  for (size_t i = 0; i < count; i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || cache.actor[i]) continue;
    if (!exported_part(i)) {
      left_out += replacing && cache.visible[i] && !cache.reference[i];
      continue;
    }
    if (!cache.visible[i]) {
      hidden++;
      continue;
    }
    const bool covered = covered_by_game(i);
    eo.only.push_back(n.id);
    rep.meshes++;
    rep.triangles += n.mesh->tri_count();
    if (i < cache.bounds.size() && cache.bounds[i].valid()) {
      bounds.add(cache.bounds[i].lo);
      bounds.add(cache.bounds[i].hi);
    }
    // the game's collision there, or none for see-through surfaces (water, glass): the builder
    // makes the collision of the others from their shape; a collision set in the editor (its
    // surface, an invisible wall, none) where the game's does not already hold it
    const DecorBehavior decor = decor_behavior_of(n.extras);
    if (decor.collision.set && !(o.game_collision && covered)) {
      eo.node_extras[n.id] = collision_extras(decor.collision);
      if (decor.collision.kind != "none") {
        rep.collision_from_decor++;
        by_shape.push_back(i);
        eo.node_extras[n.id]["double_sided"] = 1;
      }
    } else if ((o.game_collision && covered) || see_through(scene, *n.mesh)) {
      eo.node_extras[n.id] = json{{"set_collision", 1}, {"ignore", 1}};
      if (decor.collision.set) notes.add("export.note.decor_collision_game", n.name);
    } else if (!n.extras.contains("og_shape") && i < cache.bounds.size() && cache.bounds[i].valid() &&
               std::max({cache.bounds[i].size().x, cache.bounds[i].size().y, cache.bounds[i].size().z}) < kSmallPiece) {
      // a small piece (a pipe, a lamp, a window, a decal): the game gives most of its details no
      // collision, and theirs would fill the cache around Jak (he falls through the floor there)
      eo.node_extras[n.id] = json{{"set_collision", 1}, {"ignore", 1}};
      notes.add("export.note.collision_small", n.name);
    } else {
      rep.collision_from_decor++;
      by_shape.push_back(i);
      // the decor faces either way (the game draws both sides, its ground often faces down): a
      // collision made from it holds from both sides (the game's own collision stays one-sided)
      eo.node_extras[n.id] = json{{"double_sided", 1}};
    }
    if (decor.bounce.set || decor.launch.set) decor_actions.push_back(i);
  }
  if (rep.collision_from_decor) notes.add("export.note.collision_from_decor", "", rep.collision_from_decor);

  // --- the game's collision: one mesh per surface, in the game's space ---------------------------
  std::vector<std::shared_ptr<Mesh>> game_collision;
  if (o.game_collision) {
    std::map<BuilderSurface, std::shared_ptr<Mesh>> surfaces;
    size_t approximated = 0, taken_out = 0;
    for (size_t i = 0; i < count; i++) {
      const Node& n = scene.nodes[i];
      if (n.kind != NodeKind::Collision || !n.mesh || !exported_part(i)) continue;
      const int p = parent_index(n);
      if (p >= 0 && !cache.visible[p]) continue;  // its level is hidden
      // the pieces of its level that were taken out: their collision with them
      const Pieces* level_pieces = nullptr;
      if (p >= 0) {
        const Node& g = scene.nodes[p];
        const std::string suffix = "-collision";
        if (g.name.size() > suffix.size()) {
          auto it = pieces.find({g.parent, g.name.substr(0, g.name.size() - suffix.size())});
          if (it != pieces.end() && !it->second.removed_tris.empty()) level_pieces = &it->second;
        }
      }
      // nearer to a piece taken out than to the pieces in place, by 5 cm at least, and within 8 m of
      // it: the game's collision of a piece in place lies on it, so it stays; around rubble it is
      // coarser than its pieces (the fallen palace's in the port: 1 to 5 m off them, which a closer
      // limit left standing as invisible walls across the street)
      constexpr float kNear = 8.f;
      auto gone = [&](const Vec3& at) {
        const float to_removed = level_pieces->removed_tris.distance(at, kNear);
        return to_removed < kNear && to_removed + 0.05f < level_pieces->kept_tris.distance(at, kNear);
      };
      const Mat4& w = cache.worlds[i];
      for (const auto& prim : n.mesh->prims) {
        const bool normals = prim.nrm.size() == prim.pos.size();
        for (size_t t = 0; t + 2 < prim.idx.size(); t += 3) {
          if (prim.idx[t] >= prim.pos.size() || prim.idx[t + 1] >= prim.pos.size() || prim.idx[t + 2] >= prim.pos.size())
            continue;
          const uint32_t pat = t / 3 < prim.tri_pat.size() ? prim.tri_pat[t / 3] : 0;
          std::string loss;
          const BuilderSurface s = builder_surface(pat, &loss);
          auto& mesh = surfaces[s];
          if (!mesh) {
            mesh = std::make_shared<Mesh>();
            char name[64];
            std::snprintf(name, sizeof(name), "%s-collision-%02d-%d-%d", o.name.c_str(), s.material, s.event, s.mode);
            mesh->name = name;
            mesh->prims.emplace_back();
          }
          Primitive& out = mesh->prims[0];
          Vec3 v[3], nrm{0, 0, 0};
          for (int k = 0; k < 3; k++) {
            v[k] = w.point(prim.pos[prim.idx[t + k]]);
            // the game's normals: the builder turns the triangles that face the other way
            if (normals) nrm += w.dir(prim.nrm[prim.idx[t + k]]);
          }
          nrm = normals && length2(nrm) > 0 ? normalize(nrm) : Vec3{0, 1, 0};
          size_t written = 0;
          auto write = [&](const Vec3& a, const Vec3& b, const Vec3& c) {
            for (const Vec3& q : {a, b, c}) {
              out.idx.push_back((uint32_t)out.pos.size());
              out.pos.push_back(q);
              out.nrm.push_back(nrm);
            }
            written++;
          };
          if (level_pieces) keep_parts(v[0], v[1], v[2], 0, gone, write);
          else write(v[0], v[1], v[2]);
          if (!written) {
            taken_out++;
            continue;
          }
          rep.collision_triangles += written;
          if (!loss.empty()) {
            approximated++;
            notes.add("export.note.surfaces", loss);
          }
        }
      }
    }
    for (auto& [s, mesh] : surfaces) {
      Primitive& p = mesh->prims[0];
      p.idx.resize(p.idx.size() / 3 * 3);
      if (p.idx.empty()) continue;
      bool flat = true;
      for (const auto& nrm : p.nrm) flat &= nrm.x == 0 && nrm.y == 1 && nrm.z == 0;
      if (flat) p.compute_normals();
      game_collision.push_back(mesh);
      eo.extra_meshes.push_back({mesh, json{{"set_collision", 1},
                                            {"set_invisible", 1},
                                            {"collide_material", s.material},
                                            {"collide_event", s.event},
                                            {"collide_mode", s.mode},
                                            {"noentity", (int)s.noentity},
                                            {"nocamera", (int)s.nocamera},
                                            {"noedge", (int)s.noedge},
                                            {"nolineofsight", (int)s.nolineofsight}}});
    }
    size_t pieces_out = 0;
    for (const auto& [level, ps] : pieces) pieces_out += ps.removed;
    if (taken_out) {
      notes.add("export.note.collision_taken_out", std::to_string(pieces_out));
      if (taken_out > 1) notes.add("export.note.collision_taken_out", "", taken_out - 1);
    }
    (void)approximated;
  }

  // --- where Jak would meet more collision than the game gathers at once -----------------------
  {
    TriGrid all(2.f);
    for (const auto& mesh : game_collision)
      for (const auto& prim : mesh->prims)
        for (size_t t = 0; t + 2 < prim.idx.size(); t += 3)
          all.add(prim.pos[prim.idx[t]], prim.pos[prim.idx[t + 1]], prim.pos[prim.idx[t + 2]]);
    // the faces made from the decor's shape twice: the builder doubles them
    for (size_t i : by_shape) {
      const Mat4& w = cache.worlds[i];
      for (const auto& prim : scene.nodes[i].mesh->prims)
        for (size_t t = 0; t + 2 < prim.idx.size(); t += 3)
          for (int side = 0; side < 2; side++)
            all.add(w.point(prim.pos[prim.idx[t]]), w.point(prim.pos[prim.idx[t + 1]]), w.point(prim.pos[prim.idx[t + 2]]));
    }
    const auto dense = collision_hot_spots(all, 1);
    for (size_t k = 0; k < dense.size() && k < 3; k++) {
      char at[96];
      std::snprintf(at, sizeof(at), "(%.0f, %.0f, %.0f): %zu", dense[k].second.x, dense[k].second.y, dense[k].second.z,
                    dense[k].first);
      notes.add("export.note.collision_dense", at);
    }
    if (dense.size() > 3) notes.add("export.note.collision_dense", "", dense.size() - 3);
    rep.dense_places = dense.size();
    rep.densest.assign(dense.begin(), dense.begin() + std::min<size_t>(dense.size(), 100));
  }

  // --- the actors ------------------------------------------------------------------------------
  // first the actors exported and their names (no other actor of the level has it), by their id
  // in the game: the links between actors by id become links by name (the builder numbers the
  // actors again)
  struct Exported {
    size_t index;
    std::string type, name;
  };
  std::vector<Exported> exported;
  std::unordered_map<uint32_t, std::string> name_of_aid;
  std::unordered_map<std::string, std::string> name_of_node;  // the name in the editor -> in the level
  size_t absent = 0;
  {
    std::set<std::string> names;
    for (size_t i = 0; i < count; i++) {
      const Node& n = scene.nodes[i];
      if (n.kind != NodeKind::Actor) continue;
      const std::string etype = n.extras.value("etype", std::string());
      // without actors: decor alone, but the platforms and bridges needed to get around stay
      // (the same rule the document itself loads a level by)
      if (!o.actors && !(data && data->object_family(etype) == "platform")) continue;
      if (!exported_part(i)) {
        left_out += replacing && cache.visible[i] && !cache.reference[i];
        continue;
      }
      if (!cache.visible[i]) {
        hidden++;
        continue;
      }
      if (etype.empty()) continue;
      if (data && !data->has_type(etype)) {
        notes.add("export.note.types_missing", etype);
        continue;
      }
      // never there (its behavior): not in the level
      if (condition_of(n.extras.value("og_behavior", json::object()).value("present", json())).kind ==
          Condition::Never) {
        absent++;
        continue;
      }
      std::string name = n.name.empty() ? etype : n.name;
      for (int k = 2; names.count(name); k++) name = (n.name.empty() ? etype : n.name) + "-" + std::to_string(k);
      names.insert(name);
      if (!n.name.empty()) name_of_node.emplace(n.name, name);
      auto aid = n.extras.find("aid");
      if (aid != n.extras.end() && aid->is_number_integer()) name_of_aid[aid->get<uint32_t>()] = name;
      exported.push_back({i, etype, name});
    }
  }
  std::vector<std::string> actor_lines;
  std::set<std::string> art_groups;
  size_t kill_masks = 0;
  std::string generated;                // the code of the level (<name>-obs.gc)
  std::vector<std::string> code_files;  // the code of the game the level needs, in order
  // The code the game already has around the level replaced: the levels it shows with it (its
  // neighbors, the levels of its continue points; an alias of a level, as ctywide-kg, is that
  // level, ctywide). The DGO does not bring it again (the effects of the city draw particle
  // groups of ctywide-part, which needs the code of the whole city).
  std::set<std::string> loaded_around;
  if (replaced && data) {
    std::set<std::string> around(replaced->neighbors.begin(), replaced->neighbors.end());
    for (const auto& c : replaced->continues) around.insert(c.shows.begin(), c.shows.end());
    around.erase(o.replaces);
    for (std::string name : around) {
      const LevelInfo* l = data->level(name);
      while ((!l || l->code.empty()) && name.find('-') != std::string::npos) {
        name = name.substr(0, name.rfind('-'));
        l = data->level(name);
      }
      if (l && name != o.replaces) loaded_around.insert(l->code.begin(), l->code.end());
    }
  }
  auto need = [&](const std::vector<std::string>& files) {
    for (const auto& f : files)
      if (!loaded_around.count(f) && std::find(code_files.begin(), code_files.end(), f) == code_files.end())
        code_files.push_back(f);
  };
  // the level replaced: its own code (its particles, its objects, what its missions spawn there)
  if (replaced) need(replaced->code);
  for (const auto& [i, type, name] : exported) {
    const Node& n = scene.nodes[i];
    // the code of its type, and of the particle group it draws (a part-spawner's art-name)
    if (data) {
      need(data->code_files(type));
      if (auto l = n.extras.find("lump"); l != n.extras.end() && l->is_object())
        if (auto art = l->find("art-name"); art != l->end() && art->is_string()) {
          std::string group = art->get<std::string>();
          if (!group.empty() && group[0] == '\'') group = group.substr(1);
          need(data->effect_code_files(group));
        }
    }
    // its behavior, as a type of the level deriving its own, the actors it names by their names in
    // the level
    std::string etype = type;
    json behavior = n.extras.value("og_behavior", json::object());
    if (behavior.is_object()) {
      for (auto& [key, c] : behavior.items())
        if (c.is_object() && c.contains("arg") && c["arg"].is_string()) {
          auto it = name_of_node.find(c["arg"].get<std::string>());
          if (it != name_of_node.end()) c["arg"] = it->second;
        }
      const std::string own = behavior_type_name(o.name, name);
      const std::string code = behavior_code(data, type, behavior, own, name + " (" + type + ")");
      if (!code.empty()) {
        generated += code + "\n";
        etype = own;
        rep.behaviors++;
      }
    }
    // where it was in the game: the positions inside its data move with it
    Mat4 moved = Mat4::identity();
    bool has_origin = false;
    if (auto og = n.extras.find("og_origin"); og != n.extras.end() && og->is_array() && og->size() == 7) {
      Transform t0;
      t0.t = {(*og)[0].get<float>(), (*og)[1].get<float>(), (*og)[2].get<float>()};
      t0.r = Quat{(*og)[3].get<float>(), (*og)[4].get<float>(), (*og)[5].get<float>(), (*og)[6].get<float>()};
      Transform t1 = Transform::from_matrix(cache.worlds[i]);
      t1.s = {1, 1, 1};
      moved = t1.matrix() * t0.matrix().inverse();
      has_origin = true;
    }
    std::set<std::string> path_tags;
    if (data)
      if (const auto* ps = data->params(type))
        for (const auto& p : *ps)
          if (p.kind == "path") path_tags.insert(p.tag);

    const Mat4& w = cache.worlds[i];
    const Vec3 pos = w.point({0, 0, 0});
    const Quat q = Transform::from_matrix(w).r.normalized();
    float radius = 0;
    auto bs = n.extras.find("bsphere");
    if (bs != n.extras.end() && bs->is_array() && bs->size() == 4 && (*bs)[3].is_number()) radius = (*bs)[3].get<float>();
    if (radius <= 0) {
      // a placed one: its model's
      for (auto c : scene.children(n.id))
        if (const Node* m = scene.find(c); m && m->mesh) radius = std::max(radius, m->mesh->bounds().radius());
      if (radius <= 0) radius = 5.f;
    }

    // its data (res lumps)
    json lump = json::object();
    lump["name"] = name;
    const json* types = nullptr;
    auto lt = n.extras.find("lump_types");
    if (lt != n.extras.end() && lt->is_object()) types = &*lt;
    auto lj = n.extras.find("lump");
    if (lj != n.extras.end() && lj->is_object()) {
      for (const auto& [tag, v] : lj->items()) {
        if (tag == "name" || tag == "visvol") continue;  // its name is set, its visibility box is the game's
        const std::string ltype = types && types->contains(tag) && (*types)[tag].is_string() ? (*types)[tag].get<std::string>() : "";
        if (tag == "kill-mask") {
          // when the actor is there along the story of its level: the new level has its own
          if (!v.is_number() || v.get<double>() != 0) kill_masks++;
          continue;
        }
        if (ltype == "actor-name") {
          // a link set in the editor: another actor by its name in the editor, then in the level
          auto it = v.is_string() ? name_of_node.find(v.get<std::string>()) : name_of_node.end();
          if (it == name_of_node.end()) {
            notes.add("export.note.links_dropped", tag);
            continue;
          }
          lump[tag] = json::array({"string", it->second});
          continue;
        }
        if ((ltype == "uint32" || ltype == "int32" || ltype.empty()) && tag.size() > 6 &&
            tag.compare(tag.size() - 6, 6, "-actor") == 0 && tag != "task-actor") {
          // other actors by their ids: by their names, when they are in the level
          std::vector<const json*> ids;
          numbers(v, ids);
          json linked = json::array({"string"});
          for (const json* id : ids) {
            auto it = name_of_aid.find((uint32_t)(int64_t)id->get<double>());
            if (it == name_of_aid.end()) break;
            linked.push_back(it->second);
          }
          if (ids.empty() || linked.size() != ids.size() + 1) {
            notes.add("export.note.links_dropped", tag);
            continue;
          }
          lump[tag] = linked;
          continue;
        }
        if (ltype == "pair") {
          // a script of the game: the level builder cannot write them (the behavior set in the
          // editor is the level's code instead)
          notes.add("export.note.scripts_dropped", tag);
          continue;
        }
        if (ltype == "int8" || ltype == "int16" || ltype == "uint8" || ltype == "uint16")
          notes.add("export.note.lumps_widened", tag);
        json bv = builder_lump_value(v, ltype);
        if (has_origin && path_tags.count(tag) && bv.is_array() && bv.size() > 1 && bv[0] == "vector") {
          // a path, in game units: moved as the actor was
          for (size_t k = 1; k < bv.size(); k++) {
            auto& pt = bv[k];
            const Vec3 p = moved.point(Vec3{pt[0].get<float>(), pt[1].get<float>(), pt[2].get<float>()} / 4096.f) * 4096.f;
            pt[0] = p.x, pt[1] = p.y, pt[2] = p.z;
          }
        }
        if (bv.is_null()) {
          notes.add("export.note.lumps_dropped", tag + (ltype.empty() ? "" : " (" + ltype + ")"));
          continue;
        }
        lump[tag] = bv;
      }
    }
    int game_task = 0;
    auto gt = n.extras.find("game_task");
    if (gt != n.extras.end() && gt->is_number_integer()) game_task = gt->get<int>();
    std::ostringstream a;
    a << "{\"trans\": [" << num(pos.x) << ", " << num(pos.y) << ", " << num(pos.z) << "], \"etype\": \"" << etype
      << "\", \"game_task\": " << game_task << ", \"quat\": [" << num(q.x) << ", " << num(q.y) << ", " << num(q.z)
      << ", " << num(q.w) << "], \"bsphere\": [" << num(pos.x) << ", " << num(pos.y) << ", " << num(pos.z) << ", "
      << num(radius) << "], \"lump\": " << lump.dump() << "}";
    actor_lines.push_back(a.str());
    rep.actors++;
    // the models its type draws: their art groups, from the game's DGOs
    if (data)
      if (const auto* models = data->type_models(type))
        for (const auto& m : *models)
          if (auto ag = data->level_art_group(m); !ag.empty()) art_groups.insert(ag + "-ag");
  }
  // the decor that bounces or launches Jak: an actor of the level's code on its top
  {
    std::set<std::string> taken;
    for (const auto& e : exported) taken.insert(e.name);
    for (size_t i : decor_actions) {
      const Node& n = scene.nodes[i];
      const DecorBehavior decor = decor_behavior_of(n.extras);
      const AABB& b = cache.bounds[i];
      if (!b.valid()) continue;
      const Vec3 top{b.center().x, b.hi.y, b.center().z};
      const float radius = std::clamp(std::max(b.size().x, b.size().z) * 0.5f, 1.f, 20.f);
      auto add_actor = [&](const char* what, const std::string& code_of_type, const json& lump_in, std::string type) {
        std::string name = n.name + "-" + what;
        for (int k = 2; taken.count(name); k++) name = n.name + "-" + what + "-" + std::to_string(k);
        taken.insert(name);
        generated += code_of_type + "\n";
        rep.behaviors++;
        json lump = lump_in;
        lump["name"] = name;
        std::ostringstream a;
        a << "{\"trans\": [" << num(top.x) << ", " << num(top.y) << ", " << num(top.z) << "], \"etype\": \"" << type
          << "\", \"game_task\": 0, \"quat\": [0.0, 0.0, 0.0, 1.0], \"bsphere\": [" << num(top.x) << ", " << num(top.y)
          << ", " << num(top.z) << ", " << num(radius) << "], \"lump\": " << lump.dump() << "}";
        actor_lines.push_back(a.str());
        rep.actors++;
      };
      if (decor.bounce.set) {
        const std::string type = behavior_type_name(o.name, n.name + "-bounce");
        add_actor("bounce", bounce_code(type, radius, n.name + ": throws Jak up " + num(decor.bounce.height) + " m"),
                  json{{"spring-height", json::array({"float", decor.bounce.height * 4096.f})}}, type);
      }
      if (decor.launch.set) {
        // where Jak lands: `distance` along the element's facing turned by `yaw`, `height` above
        const Transform t = Transform::from_matrix(cache.worlds[i]);
        Vec3 facing = t.r.rotate({0, 0, 1});
        facing.y = 0;
        facing = length2(facing) > 1e-6f ? normalize(facing) : Vec3{0, 0, 1};
        const Vec3 dir = Quat::axis_angle({0, 1, 0}, radians(decor.launch.yaw)).rotate(facing);
        const Vec3 to = top + dir * decor.launch.distance + Vec3{0, decor.launch.height, 0};
        const float to_units[3] = {to.x * 4096.f, to.y * 4096.f, to.z * 4096.f};
        const std::string type = behavior_type_name(o.name, n.name + "-launch");
        add_actor("launch",
                  launch_code(type, radius, to_units, decor.launch.time,
                              n.name + ": throws Jak " + num(decor.launch.distance) + " m away, " + num(decor.launch.height) +
                                  " m up"),
                  json::object(), type);
      }
    }
  }
  if (kill_masks) notes.add("export.note.kill_masks", "", kill_masks);
  if (left_out) notes.add("export.note.replace_left_out", "", left_out);
  // where the game starts the player in the level replaced: on the ground of the new one?
  if (replaced)
    for (const auto& c : check_continues(scene, data, o.replaces))
      if (!c.ok()) notes.add("export.note.continue_no_ground", c.name);
  if (absent) notes.add("export.note.absent", "", absent);
  if (hidden) notes.add("export.note.hidden", "", hidden);
  rep.art_groups = art_groups.size();

  // --- the files -------------------------------------------------------------------------------
  const fs::path glb = dir / to_path(o.name + ".glb");
  if (!export_glb(scene, from_path(glb), eo, error)) return false;
  rep.files.push_back(from_path(glb));

  std::string nav_line;
  if (!o.navmeshes.empty()) {
    // their ids after the actors', far from them
    const uint32_t first = base_id + (uint32_t)rep.actors + 1000;
    std::vector<NavExportEntry> entries;
    for (size_t i = 0; i < o.navmeshes.size(); i++)
      entries.push_back({&o.navmeshes[i], first + (uint32_t)i,
                         i < o.navmesh_names.size() ? o.navmesh_names[i] : "nav-mesh-" + std::to_string(i)});
    const fs::path nav = dir / to_path(o.name + "-nav.json");
    if (!write_navmesh_blob(entries, from_path(nav), error)) return false;
    rep.files.push_back(from_path(nav));
    rep.navmeshes = entries.size();
    nav_line = "  \"nav_data\": \"" + install_dir + o.name + "-nav.json\",\n";
  }

  // the regions of the level replaced: the levels the game loads and shows around it
  std::string regions_line;
  if (replacing && o.region_trees.is_object() && !o.region_trees.empty()) {
    const json regions = builder_regions(o.region_trees, base_id + 2000, &rep.regions);
    regions_line = "  // where the game loads and shows the levels around this one (the level replaced's)\n"
                   "  \"region_trees\": " + regions.dump() + ",\n";
  } else if (replacing && !o.region_trees.is_object()) {
    // unknown (extracted before the regions were): {} is a level that has none (ctywide)
    notes.add("export.note.regions_missing", o.replaces);
  }

  // the level for the builder
  const bool auto_walls = rep.collision_from_decor > 0 || !o.game_collision;
  std::ostringstream j;
  j << "// " << o.name << ": exported by the OpenGOAL Level Editor for Jak 3 from "
    << (scene.settings.level.empty() ? std::string("a new level") : scene.settings.level) << ".\n"
    << "// To add it to the game: README.md, at the top of the export's folder.\n"
    << "{\n"
    << "  \"long_name\": \"" << o.name << "\",\n"
    << "  \"iso_name\": \"" << o.iso_name << "\",\n"
    << "  \"nickname\": \"" << o.nickname << "\",\n"
    << "  \"gltf_file\": \"" << install_dir << o.name << ".glb\",\n"
    << "  // the game's collision is in the file; the builder makes the rest from the decor's shape\n"
    << "  \"automatic_wall_detection\": " << (auto_walls ? "true" : "false") << ",\n"
    << "  \"automatic_wall_angle\": 45.0,\n"
    // the faces made from the decor are doubled each (extras double_sided, the builder of the
    // editor's fork); all of them when the level has no collision of the game's
    << "  \"double_sided_collide\": " << (rep.collision_from_decor > 0 && !o.game_collision ? "true" : "false") << ",\n"
    << "  \"base_id\": " << base_id << ",\n"
    << "  \"base_region_id\": " << base_id << ",\n"
    << "  \"cameras\": [],\n"
    << regions_line << nav_line << "  \"custom_models\": [],\n"
    << "  \"textures\": [],\n"
    << "  \"art_groups\": [";
  {
    bool first = true;
    for (const auto& ag : art_groups) {
      j << (first ? "" : ", ") << "\"" << ag << "\"";
      first = false;
    }
  }
  j << "],\n"
    << "  \"actors\": [\n";
  for (size_t i = 0; i < actor_lines.size(); i++) j << "    " << actor_lines[i] << (i + 1 < actor_lines.size() ? ",\n" : "\n");
  j << "  ]\n"
    << "}\n";
  const fs::path jsonc = dir / to_path(o.name + ".jsonc");
  if (!write_text(jsonc, j.str(), error)) return false;
  rep.files.push_back(from_path(jsonc));

  // the level's code: the behaviors set in the editor
  const std::string code_stem = o.name + "-obs";
  if (!generated.empty()) {
    std::ostringstream gc;
    gc << ";;-*-Lisp-*-\n"
       << "(in-package goal)\n\n"
       << ";; " << o.name << ": the behaviors of its objects, set in the OpenGOAL Level Editor. Each object\n"
       << ";; whose behavior differs from the game's has a type of its own deriving the game's.\n\n"
       << generated;
    const fs::path gc_path = top / "goal_src" / "jak3" / "levels" / to_path(o.name) / to_path(code_stem + ".gc");
    fs::create_directories(gc_path.parent_path(), ec);
    if (!write_text(gc_path, gc.str(), error)) return false;
    rep.files.push_back(from_path(gc_path));
    rep.code_file = from_path(gc_path);
  }

  // the DGO
  std::string nick_upper = o.nickname;
  for (auto& c : nick_upper) c = (char)std::toupper((unsigned char)c);
  std::ostringstream gd;
  gd << ";; DGO definition file for " << o.name << ", exported by the OpenGOAL Level Editor for Jak 3.\n"
     << "(\"" << nick_upper << ".DGO\"\n"
     << " (\n";
  // the code of the game its objects need beyond the common code, then its own
  for (const auto& f : code_files) gd << "  \"" << f << ".o\"\n";
  if (!generated.empty()) gd << "  \"" << code_stem << ".o\"\n";
  for (const auto& ag : art_groups) gd << "  \"" << ag << ".go\"\n";
  gd << "  \"" << o.name << ".go\"\n"
     << "  ))\n";
  const fs::path gd_path = dir / to_path(o.name + ".gd");
  if (!write_text(gd_path, gd.str(), error)) return false;
  rep.files.push_back(from_path(gd_path));

  // the level's definition in the game's code
  const float bottom = (bounds.valid() ? bounds.lo.y : o.start.y) - 20.f;
  const fs::path info_path = dir / to_path("level-info.gc");
  std::string info_text;
  if (replaced) {
    // the game's own definition stays (continue points, music, missions...): its nickname names the
    // DGO the game loads for it
    info_text = ";; " + o.name + " replaces " + o.replaces + " in the game. In goal_src/jak3/engine/level/level-info.gc,\n" +
                ";; in (define " + o.replaces + " ...), the game loads <nickname>.DGO for the level: change its\n" +
                ";; nickname so that it loads " + nick_upper + ".DGO, this level (put '" + replaced->nickname +
                " back to play the game's own " + o.replaces + "):\n;;\n;;   :nickname '" + replaced->nickname +
                "\n;; becomes\n    :nickname '" + o.nickname + "\n";
  } else {
    info_text = ";; add this to goal_src/jak3/engine/level/level-info.gc\n\n" + level_info(o, bottom);
  }
  if (!write_text(info_path, info_text, error)) return false;
  rep.files.push_back(from_path(info_path));

  rep.notes = notes.list();

  // how to add it to the game
  std::ostringstream md;
  md << "# " << o.name << " for Jak 3" << (replaced ? ", in place of " + o.replaces : std::string()) << "\n\n"
     << "Exported by the OpenGOAL Level Editor from " << (scene.settings.level.empty() ? "a new level" : scene.settings.level)
     << ": " << rep.meshes << " meshes (" << rep.triangles << " triangles), "
     << rep.collision_triangles << " collision triangles of the game, " << rep.actors << " actors, "
     << rep.art_groups << " art groups";
  if (rep.navmeshes) md << ", " << rep.navmeshes << " nav meshes";
  if (rep.behaviors) md << ", " << rep.behaviors << " objects with a behavior of their own (`" << code_stem << ".gc`)";
  md << ".\n\n"
     << "## What is in this folder\n\n"
     << "It is laid out like the jak-project checkout (OpenGOAL's source folder, the one with `goal_src` and\n"
     << "`custom_assets`) it goes into:\n\n"
     << "- `" << install_dir << "`: the level for OpenGOAL's level builder: `" << o.name
     << ".glb` (its decor and collision), `" << o.name << ".jsonc` (its names and actors), `" << o.name
     << ".gd` (its DGO, " << nick_upper << ".DGO), `level-info.gc` (its definition in the game's code)"
     << (rep.navmeshes ? ", `" + o.name + "-nav.json` (its nav meshes)" : std::string()) << ".\n";
  if (!generated.empty())
    md << "- `" << code_dir << code_stem << ".gc`: the behaviors set in the editor (the level's code).\n";
  md << "\n## Adding it to the game\n\n"
     << "1. Copy the folder" << (generated.empty() ? " `custom_assets`" : "s `custom_assets` and `goal_src`")
     << " of this folder into your jak-project checkout: "
     << (generated.empty() ? "it merges" : "they merge") << " with the folder"
     << (generated.empty() ? "" : "s") << " of the same name there (nothing of the game is replaced: the level has a\n"
     << "   folder of its own).\n"
     << "2. In `goal_src/jak3/game.gp`, next to the lines of the test-zone:\n\n"
     << "   ```\n"
     << "   (build-custom-level \"" << o.name << "\")\n";
  if (!generated.empty()) {
    // after the common code and the code of the game it derives from
    md << "   (goal-src \"levels/" << o.name << "/" << code_stem << ".gc\" \"entity-debug\"";
    for (const auto& f : code_files) md << " \"" << f << "\"";
    md << ")\n";
  }
  md << "   (custom-level-cgo \"" << nick_upper << ".DGO\" \"" << o.name << "/" << o.name << ".gd\")\n"
     << "   ```\n\n";
  if (replaced) {
    const std::string first = replaced->continues.empty() ? o.replaces + "-start" : replaced->continues.front().name;
    md << "3. In `goal_src/jak3/engine/level/level-info.gc`, in `(define " << o.replaces << " ...)`, change\n"
       << "   `:nickname '" << replaced->nickname << "` to `:nickname '" << o.nickname << "` (see `" << install_dir
       << "level-info.gc`). The game loads `<nickname>.DGO` for a level: it now loads " << nick_upper << ".DGO, this level,\n"
       << "   wherever it loads " << o.replaces << ", and keeps everything else of " << o.replaces
       << " (its continue points, its music,\n"
       << "   its missions, the levels around it). The game's own `" << replaced->dgo
       << "` is still built: put the nickname\n"
       << "   back to play the game's " << o.replaces << " again.\n"
       << "4. Build the game: start the compiler (`goalc`, or `task repl` in the checkout), type `(mi)`, then\n"
       << "   start the game (`gk`). Go there as in the game (from a level next to it), or in the REPL:\n"
       << "   `(lt)` then `(start 'play (get-continue-by-name *game-info* \"" << first << "\"))`.\n\n";
  } else {
    md << "3. In `goal_src/jak3/engine/level/level-info.gc`, add the definition of `" << install_dir
       << "level-info.gc`\n"
       << "   and give it an `:index` no other level has. The player starts at\n"
       << "   (" << num(o.start.x) << ", " << num(o.start.y) << ", " << num(o.start.z) << ") m.\n"
       << "4. Build the game: start the compiler (`goalc`, or `task repl` in the checkout), type `(mi)`, then\n"
       << "   start the game (`gk`) and, in the REPL, `(lt)` then go there:\n"
       << "   `(start 'play (get-continue-by-name *game-info* \"" << o.name << "-start\"))`.\n"
       << "   (`bg` loads it next to the levels already there: from the city, no memory is left.)\n\n";
  }
  if (!rep.notes.empty()) {
    md << "## What the export changed\n\n";
    for (const auto& note : rep.notes) md << "- " << strf(tr_en(note.key.c_str()), note.count, note.detail.c_str()) << "\n";
    md << "\n";
  }
  md << "## Good to know\n\n"
     << "- The collision of the levels' own decor is the game's where the decor is still in place (exact,\n"
     << "  and light enough for the game: Jak 3 gathers at most 461 collision triangles around Jak, more\n"
     << "  and he falls through the floor); where a piece was deleted, moved or hidden, it is taken out.\n"
     << "  The pieces placed or moved collide by their shape, made by the builder.\n"
     << "- The code of the game's objects that live in a level's own files is in `" << o.name << ".gd`, with\n"
     << "  the files before it in that level's DGO (`goal_src/jak3/dgos/`).\n"
     << "- The paths of the actors move with them; the other positions inside their data (targets) are\n"
     << "  the game's.\n"
     << "- The scripts of the game's objects (their `on-...` data) are left out: OpenGOAL's level\n"
     << "  builder cannot write them. What an object does is its behavior"
     << (generated.empty() ? std::string(" (here, the game's own).\n") : ", in `" + code_stem + ".gc`.\n");
  if (rep.navmeshes)
    md << "- The nav meshes are in `" << o.name << "-nav.json` (`nav_data` of the level): OpenGOAL's level builder\n"
       << "  for Jak 3 writes them in the level only when it reads `nav_data` (see the editor's README).\n";
  md << "- The lighting of the decor is the game's: the 8 palettes of each vertex (`_SUNRISE`...), which the\n"
     << "  game mixes with the hour of the day and the level's lights; the pieces made in the editor are lit\n"
     << "  like the level's from their color.\n"
     << "- The surfaces seen through (stains, decals, glass, water, glows) are blended over what is behind\n"
     << "  them by the level builder of the editor's jak-project branch (the translucent tree, and the PS2\n"
     << "  blend of each material: `extras.og_blend`); OpenGOAL's own builder does not draw them.\n"
     << "- That builder also: makes the collision of the decor colliding by its shape hold from both sides\n"
     << "  (`extras.double_sided`, the game's collision staying one-sided), lists everything visible in the\n"
     << "  level (no visibility data: with the PC settings' PS2 actor visibility, the actors of a level are\n"
     << "  born only where it says they are seen) and keeps the grid of a collision spread over a whole\n"
     << "  city to 2 million cells (the level's memory).\n";
  if (rep.regions)
    md << "- The level's regions are the ones of " << o.replaces << " (" << rep.regions
       << "): where the game loads and shows the levels around it as Jak or the camera goes by.\n";
  const fs::path md_path = top / "README.md";
  if (!write_text(md_path, md.str(), error)) return false;
  rep.files.push_back(from_path(md_path));

  if (report) *report = std::move(rep);
  return true;
}

}  // namespace ogle
