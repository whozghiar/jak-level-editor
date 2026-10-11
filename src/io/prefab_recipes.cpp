#include "io/prefab_recipes.h"

#include <filesystem>
#include <fstream>

#include "io/game_data.h"
#include "io/prefabs.h"

namespace ogle {

namespace {

namespace fs = std::filesystem;

fs::path to_path(const std::string& s) { return fs::path(std::u8string((const char8_t*)s.c_str())); }
std::string from_path(const fs::path& p) {
  auto u = p.generic_u8string();
  return std::string(u.begin(), u.end());
}

Vec3 vec3_of(const json& j, Vec3 fallback) {
  if (!j.is_array() || j.size() != 3) return fallback;
  return {j[0].get<float>(), j[1].get<float>(), j[2].get<float>()};
}

// a file name made of the recipe's name: "Slum house with fans" -> "slum-house-with-fans.glb"
std::string slug(const std::string& name) {
  std::string out;
  for (char c : name) {
    const char l = (char)std::tolower((unsigned char)c);
    if (std::isalnum((unsigned char)l)) out += l;
    else if (!out.empty() && out.back() != '-') out += '-';
  }
  while (!out.empty() && out.back() == '-') out.pop_back();
  return out.empty() ? "prefab" : out;
}

}  // namespace

std::string prefab_recipes_path(const std::string& game) {
  const fs::path jak3 = to_path(game_data_path()).parent_path();
  return from_path((game.empty() ? jak3 : jak3.parent_path() / to_path(game)) / "prefabs.json");
}

std::vector<PrefabRecipe> load_prefab_recipes(const std::string& path, std::string* error, const std::string& game) {
  std::vector<PrefabRecipe> out;
  std::ifstream f(to_path(path), std::ios::binary);
  const json j = f ? json::parse(f, nullptr, false) : json();
  if (!j.is_object() || !j.contains("prefabs") || !j["prefabs"].is_array()) {
    if (error) *error = "cannot read " + path;
    return out;
  }
  for (const auto& p : j["prefabs"]) {
    if (!p.is_object()) continue;
    PrefabRecipe r;
    r.game = game;
    r.name = p.value("name", std::string());
    r.category = p.value("category", std::string("other"));
    r.description = p.value("description", std::string());
    r.level = p.value("level", std::string());
    r.anchor = p.value("anchor", std::string());
    r.index = p.value("index", 0);
    r.margin = p.value("margin", 1.f);
    if (p.contains("grow") && p["grow"].is_array() && p["grow"].size() == 2) {
      r.grow_lo = vec3_of(p["grow"][0], r.grow_lo);
      r.grow_hi = vec3_of(p["grow"][1], r.grow_hi);
    }
    if (p.contains("exclude") && p["exclude"].is_array())
      for (const auto& e : p["exclude"])
        if (e.is_string()) r.exclude.push_back(e.get<std::string>());
    if (p.contains("box") && p["box"].is_array() && p["box"].size() == 2) {
      r.box.add(vec3_of(p["box"][0], {0, 0, 0}));
      r.box.add(vec3_of(p["box"][1], {0, 0, 0}));
    }
    if (r.name.empty() || r.level.empty() || (r.anchor.empty() && !r.box.valid())) continue;
    r.file = (game.empty() ? std::string() : game + "/") + slug(r.name) + ".glb";
    out.push_back(std::move(r));
  }
  return out;
}

std::vector<uint32_t> recipe_members(Scene& level, const PrefabRecipe& r, std::string* error) {
  std::vector<uint32_t> out;
  level.begin_frame();
  const SceneCache& c = level.cache();
  // the anchor: the index-th instance of the prototype (or the node of that name: a piece)
  int anchor = -1, seen = 0;
  for (size_t i = 0; i < level.nodes.size() && anchor < 0 && !r.anchor.empty(); i++) {
    const Node& n = level.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh) continue;
    const bool is_it = n.name == r.anchor || (c.parent[i] >= 0 && level.nodes[c.parent[i]].name == r.anchor);
    if (is_it && seen++ == r.index) anchor = (int)i;
  }
  if (anchor < 0 && !r.box.valid()) {
    if (error) *error = "no " + r.anchor + " (" + std::to_string(r.index) + ") in " + r.level;
    return out;
  }
  AABB box = anchor >= 0 ? c.bounds[anchor].expanded(r.margin) : r.box;
  box.lo = box.lo - r.grow_lo;
  box.hi = box.hi + r.grow_hi;
  const Vec3 room = box.size() * 1.25f;
  for (size_t i = 0; i < level.nodes.size(); i++) {
    const Node& n = level.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || n.locked || !c.visible[i] || c.actor[i]) continue;
    const AABB& b = c.bounds[i];
    if (!b.valid() || !box.contains(b.center())) continue;
    const Vec3 s = b.size();
    if ((int)i != anchor && (s.x > room.x || s.y > room.y || s.z > room.z)) continue;  // a wall going on far away
    bool left_out = false;
    for (int p = c.parent[i]; p >= 0 && !left_out; p = c.parent[p])
      left_out = level.nodes[p].name.find("-lowres") != std::string::npos;  // the far scenery
    for (const auto& e : r.exclude) left_out |= n.name.find(e) != std::string::npos;
    if (!left_out || (int)i == anchor) out.push_back(n.id);
  }
  return out;
}

bool save_recipe_prefab(Scene& level, const PrefabRecipe& r, const std::string& path, size_t* elements,
                        std::string* error) {
  const auto members = recipe_members(level, r, error);
  if (members.empty()) return false;
  if (elements) *elements = members.size();
  return save_prefab(level, members, path, error);
}

}  // namespace ogle
