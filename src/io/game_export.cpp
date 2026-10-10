#include "io/game_export.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <map>
#include <set>
#include <sstream>
#include <unordered_map>

#include "core/i18n.h"
#include "core/log.h"
#include "io/behaviors.h"
#include "io/game_data.h"
#include "io/gltf_io.h"
#include "io/nav_export.h"

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

std::string check_export_names(const LevelExportOptions& o) {
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
  return {};
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
  if (const std::string bad = check_export_names(o); !bad.empty()) {
    if (error) *error = tr(bad.c_str());
    return false;
  }
  const fs::path dir = to_path(o.folder) / to_path(o.name);
  std::error_code ec;
  fs::create_directories(dir, ec);
  if (!fs::is_directory(dir, ec)) {
    if (error) *error = "cannot create " + from_path(dir);
    return false;
  }
  rep.dir = from_path(dir);
  const std::string install_dir = "custom_assets/jak3/levels/" + o.name + "/";
  const uint32_t base_id = o.base_id ? o.base_id : kDefaultBaseId;
  Notes notes;

  const SceneCache& cache = scene.cache();
  const size_t count = std::min(scene.nodes.size(), cache.visible.size());
  std::unordered_map<uint32_t, size_t> index;
  for (size_t i = 0; i < count; i++) index[scene.nodes[i].id] = i;
  auto parent_index = [&](const Node& n) -> int {
    auto it = index.find(n.parent);
    return it == index.end() ? -1 : (int)it->second;
  };

  // the levels with the game's collision: the parent of their "<level>-collision" group, and <level>
  std::set<std::pair<uint32_t, std::string>> with_collision;
  for (size_t i = 0; i < count; i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Collision || !n.mesh) continue;
    const int p = parent_index(n);
    if (p < 0) continue;
    const Node& g = scene.nodes[p];
    const std::string suffix = "-collision";
    if (g.name.size() > suffix.size() && g.name.compare(g.name.size() - suffix.size(), suffix.size(), suffix) == 0)
      with_collision.insert({g.parent, g.name.substr(0, g.name.size() - suffix.size())});
  }
  // an ancestor is one of the draw groups of a level (<level>-tfrag-*, -tie-*, -shrub-*) whose
  // level has its collision
  auto covered_by_game = [&](size_t i) {
    for (int p = parent_index(scene.nodes[i]); p >= 0; p = parent_index(scene.nodes[p])) {
      const Node& g = scene.nodes[p];
      for (const char* kind : {"-tfrag-", "-tie-", "-shrub-"}) {
        const size_t at = g.name.find(kind);
        if (at != std::string::npos && with_collision.count({g.parent, g.name.substr(0, at)})) return true;
      }
    }
    return false;
  };

  // --- the decor -------------------------------------------------------------------------------
  ExportOptions eo;
  eo.mode = ExportMode::Builder;
  size_t hidden = 0;
  AABB bounds;
  for (size_t i = 0; i < count; i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || cache.actor[i]) continue;
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
    // makes the collision of the others from their shape
    if ((o.game_collision && covered) || see_through(scene, *n.mesh)) {
      eo.node_extras[n.id] = json{{"set_collision", 1}, {"ignore", 1}};
    } else {
      rep.collision_from_decor++;
    }
  }
  if (rep.collision_from_decor) notes.add("export.note.collision_from_decor", "", rep.collision_from_decor);

  // --- the game's collision: one mesh per surface, in the game's space ---------------------------
  if (o.game_collision) {
    std::map<BuilderSurface, std::shared_ptr<Mesh>> surfaces;
    size_t approximated = 0;
    for (size_t i = 0; i < count; i++) {
      const Node& n = scene.nodes[i];
      if (n.kind != NodeKind::Collision || !n.mesh) continue;
      const int p = parent_index(n);
      if (p >= 0 && !cache.visible[p]) continue;  // its level is hidden
      const Mat4& w = cache.worlds[i];
      for (const auto& prim : n.mesh->prims) {
        const bool normals = prim.nrm.size() == prim.pos.size();
        for (size_t t = 0; t + 2 < prim.idx.size(); t += 3) {
          const uint32_t pat = t / 3 < prim.tri_pat.size() ? prim.tri_pat[t / 3] : 0;
          std::string loss;
          const BuilderSurface s = builder_surface(pat, &loss);
          if (!loss.empty()) {
            approximated++;
            notes.add("export.note.surfaces", loss);
          }
          auto& mesh = surfaces[s];
          if (!mesh) {
            mesh = std::make_shared<Mesh>();
            char name[64];
            std::snprintf(name, sizeof(name), "%s-collision-%02d-%d-%d", o.name.c_str(), s.material, s.event, s.mode);
            mesh->name = name;
            mesh->prims.emplace_back();
          }
          Primitive& out = mesh->prims[0];
          for (int k = 0; k < 3; k++) {
            const uint32_t v = prim.idx[t + k];
            if (v >= prim.pos.size()) break;
            out.idx.push_back((uint32_t)out.pos.size());
            out.pos.push_back(w.point(prim.pos[v]));
            // the game's normals: the builder turns the triangles that face the other way
            out.nrm.push_back(normals ? normalize(w.dir(prim.nrm[v])) : Vec3{0, 1, 0});
          }
          rep.collision_triangles++;
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
    (void)approximated;
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
  auto need = [&](const std::vector<std::string>& files) {
    for (const auto& f : files)
      if (std::find(code_files.begin(), code_files.end(), f) == code_files.end()) code_files.push_back(f);
  };
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
        for (const auto& m : *models) art_groups.insert(m + "-ag");
  }
  if (kill_masks) notes.add("export.note.kill_masks", "", kill_masks);
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

  // the level for the builder
  const bool auto_walls = rep.collision_from_decor > 0 || !o.game_collision;
  std::ostringstream j;
  j << "// " << o.name << ": exported by the OpenGOAL Level Editor for Jak 3 from "
    << (scene.settings.level.empty() ? std::string("a new level") : scene.settings.level) << ".\n"
    << "// To add it to the game: INSTALL.md, next to this file.\n"
    << "{\n"
    << "  \"long_name\": \"" << o.name << "\",\n"
    << "  \"iso_name\": \"" << o.iso_name << "\",\n"
    << "  \"nickname\": \"" << o.nickname << "\",\n"
    << "  \"gltf_file\": \"" << install_dir << o.name << ".glb\",\n"
    << "  // the game's collision is in the file; the builder makes the rest from the decor's shape\n"
    << "  \"automatic_wall_detection\": " << (auto_walls ? "true" : "false") << ",\n"
    << "  \"automatic_wall_angle\": 45.0,\n"
    // the decor faces either way (the game draws both sides, its ground often faces down): a
    // collision made from it holds from both sides
    << "  \"double_sided_collide\": " << (rep.collision_from_decor > 0 ? "true" : "false") << ",\n"
    << "  \"base_id\": " << base_id << ",\n"
    << "  \"base_region_id\": " << base_id << ",\n"
    << "  \"cameras\": [],\n"
    << nav_line << "  \"custom_models\": [],\n"
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
    const fs::path gc_path = dir / to_path(code_stem + ".gc");
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
  if (!write_text(info_path, ";; add this to goal_src/jak3/engine/level/level-info.gc\n\n" + level_info(o, bottom), error))
    return false;
  rep.files.push_back(from_path(info_path));

  rep.notes = notes.list();

  // how to add it to the game
  std::ostringstream md;
  md << "# " << o.name << " for Jak 3\n\n"
     << "Exported by the OpenGOAL Level Editor from " << (scene.settings.level.empty() ? "a new level" : scene.settings.level)
     << ": " << rep.meshes << " meshes (" << rep.triangles << " triangles), "
     << rep.collision_triangles << " collision triangles of the game, " << rep.actors << " actors, "
     << rep.art_groups << " art groups";
  if (rep.navmeshes) md << ", " << rep.navmeshes << " nav meshes";
  if (rep.behaviors) md << ", " << rep.behaviors << " objects with a behavior of their own (`" << code_stem << ".gc`)";
  md << ".\n\n"
     << "## Adding it to the game\n\n"
     << "1. Copy this folder to `" << install_dir << "` in the jak-project checkout";
  if (!generated.empty())
    md << ", and `" << code_stem << ".gc` to `goal_src/jak3/levels/" << o.name << "/" << code_stem << ".gc`";
  md << ".\n"
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
     << "   ```\n\n"
     << "3. In `goal_src/jak3/engine/level/level-info.gc`, add the definition of `level-info.gc`\n"
     << "   (next to this file) and give it an `:index` no other level has. The player starts at\n"
     << "   (" << num(o.start.x) << ", " << num(o.start.y) << ", " << num(o.start.z) << ") m.\n"
     << "4. Build the game (`(mi)` in the REPL), then go there:\n"
     << "   `(start 'play (get-continue-by-name *game-info* \"" << o.name << "-start\"))`.\n"
     << "   (`bg` loads it next to the levels already there: from the city, no memory is left.)\n\n";
  if (!rep.notes.empty()) {
    md << "## What the export changed\n\n";
    for (const auto& note : rep.notes) md << "- " << strf(tr_en(note.key.c_str()), note.count, note.detail.c_str()) << "\n";
    md << "\n";
  }
  md << "## Good to know\n\n"
     << "- The collision of the levels' own decor is the game's: a piece of it moved or deleted keeps its\n"
     << "  collision where the game had it. Export with the collision made from the decor to change that.\n"
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
  const fs::path md_path = dir / to_path("INSTALL.md");
  if (!write_text(md_path, md.str(), error)) return false;
  rep.files.push_back(from_path(md_path));

  if (report) *report = std::move(rep);
  return true;
}

}  // namespace ogle
