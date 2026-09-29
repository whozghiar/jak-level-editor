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

const char* game_title(Game g) {
  switch (g) {
    case Game::Jak1: return "Jak and Daxter";
    case Game::Jak2: return "Jak II";
    case Game::Jak3: return "Jak 3";
  }
  return "?";
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

const char* kJak2Materials[] = {"none",   "ice",     "quicksand", "waterbottom", "tar",     "sand",   "wood",
                                "grass",  "pcmetal", "snow",      "deepsnow",    "hotcoals", "lava",  "crwood",
                                "gravel", "dirt",    "metal",     "straw",       "tube",    "swamp",  "stopproj",
                                "rotate", "neutral", "stone",     "crmetal",     "carpet",  "grmetal", "shmetal",
                                "hdwood", "squish",  "mhshroom",  "forest",      "mhswamp", "dmaker"};
const char* kJak2Events[] = {"none", "deadly", "endlessfall", "burn",   "deadlyup", "burnup",   "melt",
                             "slide", "lip",   "lipramp",     "shock",  "shockup",  "hide",     "rail",
                             "slippery", "drag", "waterfloor", "hang",  "fry",      "slime"};

std::string material_name(int m) {
  return m >= 0 && m < (int)(sizeof(kJak2Materials) / sizeof(*kJak2Materials)) ? kJak2Materials[m]
                                                                               : "material " + std::to_string(m);
}
std::string event_name(int e) {
  return e >= 0 && e < (int)(sizeof(kJak2Events) / sizeof(*kJak2Events)) ? kJak2Events[e] : "event " + std::to_string(e);
}

// --- actors --------------------------------------------------------------------------------------

// Types of Jak II whose Jak 3 counterpart has another name (the doors: scripts/level_port).
const std::map<std::string, std::string>& jak2_to_jak3_types() {
  static const std::map<std::string, std::string> m = {
      {"hip-door-b", "hip-door-a"},         {"hide-door-a", "cty-door"},
      {"hide-door-b", "hip-door-a"},        {"oracle-door", "cty-door"},
      {"vin-door", "vin-door-ctyinda"},     {"gar-door", "com-airlock-outer"},
      {"pal-throne-door", "com-airlock-outer"}, {"pal-ent-door", "cty-door"},
      {"cas-front-door", "com-airlock-outer"}};
  return m;
}

// The pickup of the destination with the same name, or a close one (-1 when none).
int map_pickup(const GameData& from, const GameData& to, int value, std::string* name) {
  *name = from.enum_name("pickup-type", value);
  if (name->empty()) return -1;
  int v = to.enum_value("pickup-type", *name);
  if (v < 0) v = to.enum_value("pickup-type", *name + "-1");  // Jak 3's guns: gun-red-1...
  if (v < 0 && *name == "eco-pill") v = to.enum_value("pickup-type", "eco-pill-green");
  if (v < 0 && *name == "eco-pill-green") v = to.enum_value("pickup-type", "eco-pill");
  if (v < 0 && name->size() > 2 && name->compare(name->size() - 2, 2, "-1") == 0)
    v = to.enum_value("pickup-type", name->substr(0, name->size() - 2));
  return v;
}

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

std::string level_info_jak1(const LevelExportOptions& o, float bottom) {
  const Vec3 s = o.start, c = o.start + Vec3{0, 3, -6};
  std::ostringstream t;
  t << "(define " << o.name << "\n"
    << "  (new 'static 'level-load-info\n"
    << "       :index 26 ;; the test-zone's: give it a number no other level has\n"
    << "       :name '" << o.name << "\n"
    << "       :visname '" << o.name << "-vis\n"
    << "       :nickname '" << o.nickname << "\n"
    << "       :packages '()\n"
    << "       :sound-banks '()\n"
    << "       :music-bank #f\n"
    << "       :ambient-sounds '()\n"
    << "       :mood '*village1-mood*\n"
    << "       :mood-func 'update-mood-village1\n"
    << "       :ocean '*ocean-map-village1*\n"
    << "       :sky #t\n"
    << "       :sun-fade 1.0\n"
    << "       :continues\n"
    << "       '((new 'static 'continue-point\n"
    << "             :name \"" << o.name << "-start\"\n"
    << "             :level '" << o.name << "\n"
    << "             :trans (new 'static 'vector :x " << meters(s.x) << " :y " << meters(s.y) << " :z "
    << meters(s.z) << " :w 1.0)\n"
    << "             :quat (new 'static 'quaternion :w 1.0)\n"
    << "             :camera-trans (new 'static 'vector :x " << meters(c.x) << " :y " << meters(c.y) << " :z "
    << meters(c.z) << " :w 1.0)\n"
    << "             :camera-rot (new 'static 'array float 9 1.0 0.0 0.0 0.0 1.0 0.0 0.0 0.0 1.0)\n"
    << "             :load-commands '()\n"
    << "             :vis-nick 'none\n"
    << "             :lev0 '" << o.name << "\n"
    << "             :disp0 'display\n"
    << "             :lev1 #f\n"
    << "             :disp1 #f))\n"
    << "       :tasks '()\n"
    << "       :priority 100\n"
    << "       :load-commands '()\n"
    << "       :alt-load-commands '()\n"
    << "       :bsp-mask #xffffffffffffffff\n"
    << "       :bsphere (new 'static 'sphere :w 167772160000.0)\n"
    << "       :bottom-height " << meters(bottom) << "\n"
    << "       :run-packages '()\n"
    << "       :wait-for-load #t))\n\n"
    << "(cons! *level-load-list* '" << o.name << ")\n";
  return t.str();
}

std::string level_info_jak2(const LevelExportOptions& o, float bottom) {
  const Vec3 s = o.start, c = o.start + Vec3{0, 3, -6};
  std::ostringstream t;
  t << "(define " << o.name << "\n"
    << "  (new 'static 'level-load-info\n"
    << "    :index #x9e ;; the test-zone's: give it a number no other level has\n"
    << "    :name '" << o.name << "\n"
    << "    :visname '" << o.name << "-vis\n"
    << "    :nickname '" << o.nickname << "\n"
    << "    :dbname '" << o.name << "\n"
    << "    :taskname 'default\n"
    << "    :packages '()\n"
    << "    :music-bank #f\n"
    << "    :ambient-sounds '()\n"
    << "    :mood-func 'update-mood-default\n"
    << "    :mood-init #f\n"
    << "    :ocean #f\n"
    << "    :sky #t\n"
    << "    :use-camera-other #f\n"
    << "    :part-engine-max 16\n"
    << "    :continues '((new 'static 'continue-point\n"
    << "        :name \"" << o.name << "-start\"\n"
    << "        :level '" << o.name << "\n"
    << "        :trans (new 'static 'vector :x " << meters(s.x) << " :y " << meters(s.y) << " :z " << meters(s.z)
    << " :w 1.0)\n"
    << "        :quat (new 'static 'vector :w 1.0)\n"
    << "        :camera-trans (new 'static 'vector :x " << meters(c.x) << " :y " << meters(c.y) << " :z "
    << meters(c.z) << " :w 1.0)\n"
    << "        :camera-rot (new 'static 'inline-array vector3s 3\n"
    << "          (new 'static 'vector3s :data (new 'static 'array float 3 1.0 0.0 0.0))\n"
    << "          (new 'static 'vector3s :data (new 'static 'array float 3 0.0 1.0 0.0))\n"
    << "          (new 'static 'vector3s :data (new 'static 'array float 3 0.0 0.0 1.0))\n"
    << "          )\n"
    << "        :on-goto #f\n"
    << "        :vis-nick #f\n"
    << "        :want (new 'static 'inline-array level-buffer-state 6\n"
    << "          (new 'static 'level-buffer-state :name '" << o.name
    << " :display? 'display :force-vis? #f :force-inside? #f)\n";
  for (int i = 0; i < 5; i++)
    t << "          (new 'static 'level-buffer-state :name #f :display? #f :force-vis? #f :force-inside? #f)\n";
  t << "          )\n"
    << "        :want-sound (new 'static 'array symbol 3 #f #f #f)\n"
    << "        )\n"
    << "      )\n"
    << "    :tasks '()\n"
    << "    :priority 100\n"
    << "    :load-commands '()\n"
    << "    :alt-load-commands '()\n"
    << "    :bsp-mask #xffffffffffffffff\n"
    << "    :buttom-height " << meters(bottom) << "\n"
    << "    :run-packages '()\n"
    << "    :wait-for-load #t\n"
    << "    :login-func #f\n"
    << "    :activate-func #f\n"
    << "    :deactivate-func #f\n"
    << "    :kill-func #f\n"
    << "    :borrow-level (new 'static 'array symbol 2 #f #f)\n"
    << "    :borrow-display? (new 'static 'array symbol 2 #f #f)\n"
    << "    :base-task-mask (task-mask task0)\n"
    << "    :texture-anim-tfrag #f\n"
    << "    :texture-anim-pris #f\n"
    << "    :texture-anim-shrub #f\n"
    << "    :texture-anim-alpha #f\n"
    << "    :texture-anim-water #f\n"
    << "    :texture-anim-twarp #f\n"
    << "    :texture-anim-pris2 #f\n"
    << "    :texture-anim-sprite #f\n"
    << "    :texture-anim-map #f\n"
    << "    :texture-anim-sky #f\n"
    << "    :draw-priority 10.0\n"
    << "    :fog-height 327680.0\n"
    << "    :bigmap-id (bigmap-id city)\n"
    << "    :ocean-near-translucent? #t\n"
    << "    :ocean-far? #t\n"
    << "    :mood-range (new 'static 'mood-range :data (new 'static 'array float 4 0.0 1.0 0.0 1.0))\n"
    << "    :max-rain 1.0\n"
    << "    :fog-mult 1.0\n"
    << "    :ocean-alpha 1.0\n"
    << "    :extra-sound-bank #f\n"
    << "    )\n"
    << "  )\n\n"
    << "(#when PC_PORT\n"
    << "  (cons! *level-load-list* '" << o.name << ")\n"
    << "  )\n";
  return t.str();
}

std::string level_info_jak3(const LevelExportOptions& o, float bottom) {
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

bool see_through(const Scene& scene, const Mesh& mesh) {
  bool any = false;
  for (const auto& p : mesh.prims) {
    if (p.idx.empty()) continue;
    if (p.material < 0 || p.material >= (int)scene.materials.size() ||
        scene.materials[p.material].alpha != AlphaMode::Blend)
      return false;
    any = true;
  }
  return any;
}

}  // namespace

// ------------------------------------------------------------------------------------------------

uint32_t default_base_id(Game game) {
  // above the ids of the game's own actors (Jak 1: 24,576 at most, Jak II: 36,521, Jak 3: 72,851)
  switch (game) {
    case Game::Jak1: return 40000;
    case Game::Jak2: return 50000;
    case Game::Jak3: return 90000;
  }
  return 50000;
}

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

BuilderSurface builder_surface(uint32_t pat, Game source, Game destination, std::string* loss) {
  BuilderSurface s;
  std::string lost;
  s.noentity = pat & 1;
  s.nocamera = (pat >> 1) & 1;
  s.noedge = (pat >> 2) & 1;
  if (source == Game::Jak1) {
    // goal_src/jak1/engine/collide/pat-h.gc: the builder's own fields
    s.mode = (pat >> 3) & 7;
    s.material = (pat >> 6) & 63;
    s.nolineofsight = (pat >> 12) & 1;
    s.event = (pat >> 14) & 63;
    if (s.mode > 2) lost = "mode " + std::to_string(s.mode), s.mode = 0;
    if (s.material > 22) lost = "material " + std::to_string(s.material), s.material = 0;
    if (s.event > 6) lost = "event " + std::to_string(s.event), s.event = 0;
  } else {
    // Jak II and 3 (engine/collide/pat-h.gc): the builder takes Jak 1's fields and converts them,
    // Jak 1's stone (0) becoming their stone (23); what Jak 1 does not have becomes the nearest
    const int mode = (pat >> 7) & 7, material = (pat >> 10) & 63, event = (pat >> 18) & 63;
    s.nolineofsight = (pat >> 16) & 1;
    s.mode = mode;
    if (mode > 2) {
      s.mode = 0;  // the halfpipes of Jak II's boards: ground
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
    // the level builder's collision fields are always written in Jak 1's own layout, for every
    // destination game (its own builder converts them: goalc/build_level/collide/common); a
    // surface without a material (none) becomes stone there, whichever game reads it back
    const bool kept = material == 23 || (material >= 1 && material <= 22);
    if (!kept) lost = material_name(material);
    s.event = event <= 6 ? event : 0;
    if (event > 6) lost = event_name(event);
    // the flags of Jak II and 3 only (Jak 1 has no boards, vehicles nor mechs): the builders set
    // none of them. nogrind is on nearly every surface (the board grinds rails only), and off on
    // every surface of the levels they build: not said
    if (destination != Game::Jak1 && lost.empty()) {
      static const std::pair<int, const char*> kFlags[] = {{4, "nojak"},     {5, "noboard"},       {6, "nopilot"},
                                                           {24, "probe"},    {25, "nomech"},       {26, "noproj"},
                                                           {27, "noendlessfall"}, {28, "noprobe"}};
      for (const auto& [bit, name] : kFlags)
        if (pat >> bit & 1) lost += (lost.empty() ? "" : "+") + std::string(name);
    }
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

std::string export_actor_type(const std::string& etype, Game source, Game destination, const GameData* to) {
  std::string t = etype;
  if (source == Game::Jak2 && destination == Game::Jak3) {
    auto it = jak2_to_jak3_types().find(etype);
    if (it != jak2_to_jak3_types().end() && (!to || !to->has_type(etype))) t = it->second;
  }
  if (to && !to->has_type(t)) return {};
  return t;
}

bool export_level_for_game(const Scene& scene, const GameData* from, const GameData* to,
                           const LevelExportOptions& o, LevelExportReport* report, std::string* error,
                           const std::map<std::string, const GameData*>* by_game) {
  LevelExportReport rep;
  if (const std::string bad = check_export_names(o); !bad.empty()) {
    if (error) *error = tr(bad.c_str());
    return false;
  }
  const Game src = scene.settings.game, dst = o.game;
  const std::string game = game_name(dst);
  const fs::path dir = to_path(o.folder) / to_path(o.name);
  std::error_code ec;
  fs::create_directories(dir, ec);
  if (!fs::is_directory(dir, ec)) {
    if (error) *error = "cannot create " + from_path(dir);
    return false;
  }
  rep.dir = from_path(dir);
  const std::string install_dir = "custom_assets/" + game + "/levels/" + o.name + "/";
  const uint32_t base_id = o.base_id ? o.base_id : default_base_id(dst);
  Notes notes;

  const SceneCache& cache = scene.cache();
  const size_t count = std::min(scene.nodes.size(), cache.visible.size());
  std::unordered_map<uint32_t, size_t> index;
  for (size_t i = 0; i < count; i++) index[scene.nodes[i].id] = i;
  auto parent_index = [&](const Node& n) -> int {
    auto it = index.find(n.parent);
    return it == index.end() ? -1 : (int)it->second;
  };
  // og_game, on the node itself (a catalog object placed from another game) or the nearest
  // ancestor (the group of a level added from another game): "" for the document's own
  auto source_tag_of = [&](size_t i) -> std::string {
    for (int p = (int)i; p >= 0; p = parent_index(scene.nodes[p])) {
      const std::string tag = scene.nodes[p].extras.value("og_game", std::string());
      if (!tag.empty()) return tag;
    }
    return {};
  };
  auto game_of_tag = [&](const std::string& tag) { return tag.empty() ? src : game_from_name(tag, src); };
  auto data_of_tag = [&](const std::string& tag) -> const GameData* {
    if (!tag.empty() && by_game) {
      auto it = by_game->find(tag);
      if (it != by_game->end()) return it->second;
    }
    return from;
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
      // the extractor tags each collision node with the game whose layout its pat bits follow
      // (collide_game): a level added from another game keeps its own, not the document's
      const std::string collide_tag = n.extras.value("collide_game", std::string());
      const Game node_src = collide_tag.empty() ? game_of_tag(source_tag_of(i)) : game_from_name(collide_tag, src);
      const Mat4& w = cache.worlds[i];
      for (const auto& prim : n.mesh->prims) {
        const bool normals = prim.nrm.size() == prim.pos.size();
        for (size_t t = 0; t + 2 < prim.idx.size(); t += 3) {
          const uint32_t pat = t / 3 < prim.tri_pat.size() ? prim.tri_pat[t / 3] : 0;
          std::string loss;
          const BuilderSurface s = builder_surface(pat, node_src, dst, &loss);
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
    Game src;
    const GameData* from;
  };
  std::vector<Exported> exported;
  std::unordered_map<uint32_t, std::string> name_of_aid;
  {
    std::set<std::string> names;
    for (size_t i = 0; i < count; i++) {
      const Node& n = scene.nodes[i];
      if (n.kind != NodeKind::Actor) continue;
      const std::string etype = n.extras.value("etype", std::string());
      // its own game (a level added from another game, or an object placed from one), else the
      // document's: what checks its type and maps its data
      const std::string tag = source_tag_of(i);
      const Game actor_src = game_of_tag(tag);
      const GameData* actor_from = data_of_tag(tag);
      // without actors: decor alone, but the platforms and bridges needed to get around stay
      // (the same rule the document itself loads a level by)
      if (!o.actors && !(actor_from && actor_from->object_family(etype) == "platform")) continue;
      if (!cache.visible[i]) {
        hidden++;
        continue;
      }
      if (etype.empty()) continue;
      const std::string type = export_actor_type(etype, actor_src, dst, to);
      if (type.empty()) {
        notes.add("export.note.types_missing", etype);
        continue;
      }
      if (type != etype) notes.add("export.note.types_mapped", etype + " > " + type);
      std::string name = n.name.empty() ? type : n.name;
      for (int k = 2; names.count(name); k++) name = (n.name.empty() ? type : n.name) + "-" + std::to_string(k);
      names.insert(name);
      auto aid = n.extras.find("aid");
      if (aid != n.extras.end() && aid->is_number_integer()) name_of_aid[aid->get<uint32_t>()] = name;
      exported.push_back({i, type, name, actor_src, actor_from});
    }
  }
  std::vector<std::string> actor_lines;
  std::set<std::string> art_groups;
  size_t kill_masks = 0;
  for (const auto& [i, type, name, actor_src, actor_from] : exported) {
    const Node& n = scene.nodes[i];

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

    // its data (res lumps), for the destination
    json lump = json::object();
    lump["name"] = name;
    const json* types = nullptr;
    auto lt = n.extras.find("lump_types");
    if (lt != n.extras.end() && lt->is_object()) types = &*lt;
    auto lj = n.extras.find("lump");
    if (lj != n.extras.end() && lj->is_object()) {
      for (const auto& [tag, v0] : lj->items()) {
        if (tag == "name" || tag == "visvol") continue;  // its name is set, its visibility box is the game's
        const std::string ltype = types && types->contains(tag) && (*types)[tag].is_string() ? (*types)[tag].get<std::string>() : "";
        json v = v0;
        if (tag == "kill-mask") {
          // when the actor is there along the story of its level: the new level has its own
          if (!v.is_number() || v.get<double>() != 0) kill_masks++;
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
        if (actor_src != dst && (tag == "task-actor" || tag == "task" || ltype == "pair")) {
          // bound to the story of the source game: its missions, its scripts
          notes.add("export.note.story_data", tag);
          continue;
        }
        if ((tag == "eco-info" || tag == "pickup-type") && actor_src != dst && actor_from && to) {
          // what a crate or a pickup holds (eco-info: the pickup then its amount), what a
          // security wall asks for: the destination's pickup of the same name
          std::vector<const json*> ns;
          numbers(v, ns);
          const size_t mapped = tag == "eco-info" ? std::min<size_t>(1, ns.size()) : ns.size();
          json nv = json::array();
          bool missing = false;
          for (size_t k = 0; k < ns.size(); k++) {
            if (k >= mapped) {
              nv.push_back(*ns[k]);
              continue;
            }
            std::string pname;
            const int before = (int)ns[k]->get<double>();
            const int after = map_pickup(*actor_from, *to, before, &pname);
            if (tag == "eco-info" && (pname == "fuel-cell" || pname == "buzzer")) {
              // a power cell, a scout fly: its amount is a task of the source game
              notes.add("export.note.story_data", "eco-info (" + pname + ")");
              missing = true;
              break;
            }
            if (after < 0) {
              notes.add("export.note.pickups_missing", pname.empty() ? std::to_string(before) : pname);
              missing = true;
              break;
            }
            if (after != before) notes.add("export.note.pickups_mapped", pname);
            nv.push_back(after);
          }
          if (missing) continue;
          if (!ns.empty()) v = nv;
        }
        if (ltype == "pair") notes.add("export.note.scripts_kept", tag);
        if (ltype == "int8" || ltype == "int16" || ltype == "uint8" || ltype == "uint16")
          notes.add("export.note.lumps_widened", tag);
        json bv = builder_lump_value(v, ltype);
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
    if (actor_src != dst && game_task) {
      notes.add("export.note.story_data", "game_task");
      game_task = 0;
    }
    std::ostringstream a;
    a << "{\"trans\": [" << num(pos.x) << ", " << num(pos.y) << ", " << num(pos.z) << "], \"etype\": \"" << type
      << "\", \"game_task\": " << game_task << ", \"quat\": [" << num(q.x) << ", " << num(q.y) << ", " << num(q.z)
      << ", " << num(q.w) << "], \"bsphere\": [" << num(pos.x) << ", " << num(pos.y) << ", " << num(pos.z) << ", "
      << num(radius) << "], \"lump\": " << lump.dump() << "}";
    actor_lines.push_back(a.str());
    rep.actors++;
    // the models its type draws: their art groups, from the destination's DGOs
    if (to)
      if (const auto* models = to->type_models(type))
        for (const auto& m : *models) art_groups.insert(m + "-ag");
  }
  if (kill_masks) notes.add("export.note.kill_masks", "", kill_masks);
  if (hidden) notes.add("export.note.hidden", "", hidden);
  rep.art_groups = art_groups.size();

  // --- the files -------------------------------------------------------------------------------
  const fs::path glb = dir / to_path(o.name + ".glb");
  if (!export_glb(scene, from_path(glb), eo, error)) return false;
  rep.files.push_back(from_path(glb));

  std::string nav_line;
  if (!o.navmeshes.empty()) {
    if (dst == Game::Jak3) {
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
    } else {
      notes.add("export.note.navmeshes_left_out", "", o.navmeshes.size());
    }
  }

  // the level for the builder
  const bool auto_walls = rep.collision_from_decor > 0 || !o.game_collision;
  std::ostringstream j;
  j << "// " << o.name << ": exported by the OpenGOAL Level Editor for " << game_title(dst) << " from "
    << (scene.settings.level.empty() ? std::string("a new level") : scene.settings.level) << " ("
    << game_title(src) << ").\n"
    << "// To add it to the game: INSTALL.md, next to this file.\n"
    << "{\n"
    << "  \"long_name\": \"" << o.name << "\",\n"
    << "  \"iso_name\": \"" << o.iso_name << "\",\n"
    << "  \"nickname\": \"" << o.nickname << "\",\n"
    << "  \"gltf_file\": \"" << install_dir << o.name << ".glb\",\n"
    << "  // the game's collision is in the file; the builder makes the rest from the decor's shape\n"
    << "  \"automatic_wall_detection\": " << (auto_walls ? "true" : "false") << ",\n"
    << "  \"automatic_wall_angle\": 45.0,\n"
    << "  \"double_sided_collide\": false,\n"
    << "  \"base_id\": " << base_id << ",\n";
  if (dst != Game::Jak1) j << "  \"base_region_id\": " << base_id << ",\n";
  if (dst == Game::Jak1)
    j << "  \"sky\": \"village1\",\n"
      << "  \"tex_remap\": \"village1\",\n"
      << "  \"tpages\": [],\n"
      << "  \"ambients\": [],\n";
  if (dst != Game::Jak2) j << "  \"cameras\": [],\n";
  j << nav_line << "  \"custom_models\": [],\n"
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

  // the DGO
  std::string nick_upper = o.nickname;
  for (auto& c : nick_upper) c = (char)std::toupper((unsigned char)c);
  std::ostringstream gd;
  gd << ";; DGO definition file for " << o.name << ", exported by the OpenGOAL Level Editor for " << game_title(dst)
     << ".\n"
     << "(\"" << nick_upper << ".DGO\"\n"
     << " (\n";
  if (dst == Game::Jak1) {
    gd << "  ;; the textures of village1 (its sky: \"sky\" and \"tex_remap\" in " << o.name << ".jsonc)\n";
    for (const char* t : {"tpage-398.go", "tpage-400.go", "tpage-399.go", "tpage-401.go", "tpage-1470.go"})
      gd << "  \"" << t << "\"\n";
  }
  for (const auto& ag : art_groups) gd << "  \"" << ag << ".go\"\n";
  gd << "  \"" << o.name << ".go\"\n"
     << "  ))\n";
  const fs::path gd_path = dir / to_path(o.name + ".gd");
  if (!write_text(gd_path, gd.str(), error)) return false;
  rep.files.push_back(from_path(gd_path));

  // the level's definition in the game's code
  const float bottom = (bounds.valid() ? bounds.lo.y : o.start.y) - 20.f;
  const std::string info = dst == Game::Jak1 ? level_info_jak1(o, bottom)
                           : dst == Game::Jak2 ? level_info_jak2(o, bottom)
                                               : level_info_jak3(o, bottom);
  const fs::path info_path = dir / to_path("level-info.gc");
  if (!write_text(info_path, ";; add this to goal_src/" + game + "/engine/level/level-info.gc\n\n" + info, error))
    return false;
  rep.files.push_back(from_path(info_path));

  rep.notes = notes.list();

  // how to add it to the game
  std::ostringstream md;
  md << "# " << o.name << " for " << game_title(dst) << "\n\n"
     << "Exported by the OpenGOAL Level Editor from " << (scene.settings.level.empty() ? "a new level" : scene.settings.level)
     << " (" << game_title(src) << "): " << rep.meshes << " meshes (" << rep.triangles << " triangles), "
     << rep.collision_triangles << " collision triangles of the game, " << rep.actors << " actors, "
     << rep.art_groups << " art groups";
  if (rep.navmeshes) md << ", " << rep.navmeshes << " nav meshes";
  md << ".\n\n"
     << "## Adding it to the game\n\n"
     << "1. Copy this folder to `" << install_dir << "` in the jak-project checkout.\n"
     << "2. In `goal_src/" << game << "/game.gp`, next to the lines of the test-zone:\n\n"
     << "   ```\n"
     << "   (build-custom-level \"" << o.name << "\")\n"
     << "   (custom-level-cgo \"" << nick_upper << ".DGO\" \"" << o.name << "/" << o.name << ".gd\")\n"
     << "   ```\n\n"
     << "3. In `goal_src/" << game << "/engine/level/level-info.gc`, add the definition of `level-info.gc`\n"
     << "   (next to this file) and give it an `:index` no other level has. The player starts at\n"
     << "   (" << num(o.start.x) << ", " << num(o.start.y) << ", " << num(o.start.z) << ") m.\n"
     << "4. Build the game (`(mi)` in the REPL), then go there: `(bg-custom '" << o.name << "-vis)`.\n\n";
  if (!rep.notes.empty()) {
    md << "## What the export changed for " << game_title(dst) << "\n\n";
    for (const auto& note : rep.notes) md << "- " << strf(tr_en(note.key.c_str()), note.count, note.detail.c_str()) << "\n";
    md << "\n";
  }
  md << "## Good to know\n\n"
     << "- The collision of the levels' own decor is the game's: a piece of it moved or deleted keeps its\n"
     << "  collision where the game had it. Export with the collision made from the decor to change that.\n"
     << "- The actors whose code is in a level's own files (particle effects, the logic of a level) need\n"
     << "  that code: add its `.o` files to `" << o.name << ".gd` (see `goal_src/" << game
     << "/dgos/` for the levels that have them).\n"
     << "- The positions inside the actors' data (paths, targets) are the game's: an actor moved keeps\n"
     << "  its path where it was.\n";
  const fs::path md_path = dir / to_path("INSTALL.md");
  if (!write_text(md_path, md.str(), error)) return false;
  rep.files.push_back(from_path(md_path));

  if (report) *report = std::move(rep);
  return true;
}

}  // namespace ogle
