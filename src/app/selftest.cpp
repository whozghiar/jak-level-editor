// --selftest: exercises the editor without a window. Writes a small library in ogle-extract's
// layout, lists and opens its level (decor instances, locked terrain, actor with its model,
// collision), copies and pastes elements, makes and places a prefab, round-trips a project, opens
// a level with game data (creatures left out, water models, companion and layer levels, the story
// view, the catalog of objects and decor parts), checks the remote control of the MCP server, the
// translations and the game data shipped with the editor.

#include <SDL3/SDL.h>

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <functional>
#include <regex>
#include <set>
#include <thread>

#include "app/app.h"
#include "app/remote.h"
#include "core/i18n.h"
#include "core/log.h"
#include "glad/glad.h"
#include "io/asset_library.h"
#include "io/behaviors.h"
#include "io/catalog.h"
#include "io/game_data.h"
#include "io/game_export.h"
#include "io/jak2_import.h"
#include "io/gltf_io.h"
#include "io/library_index.h"
#include "io/nav_export.h"
#include "io/prefab_recipes.h"
#include "io/prefabs.h"
#include "render/offscreen.h"
#include "scene/assemblies.h"
#include "scene/bvh.h"
#include "scene/gaps.h"
#include "game_view/game_mood.h"
#include "scene/particles.h"
#include "scene/relight.h"
#include "scene/level_style.h"
#include "scene/shapes.h"
#include "scene/skeleton.h"
#include "scene/story.h"
#include "scene/terrain.h"
#include "scene/topology.h"
#include "stb_image_write.h"

namespace ogle {

namespace {

namespace fs = std::filesystem;

int g_failures = 0;

void check(bool ok, const char* what) {
  std::printf("  [%s] %s\n", ok ? " ok " : "FAIL", what);
  if (!ok) g_failures++;
}

// a box of size 2, from y = 0 to 2
std::shared_ptr<Mesh> make_box(const std::string& name, int material) {
  auto mesh = std::make_shared<Mesh>();
  mesh->name = name;
  Primitive p;
  for (int i = 0; i < 8; i++) p.pos.push_back({(i & 1) ? 1.f : -1.f, (i & 2) ? 2.f : 0.f, (i & 4) ? 1.f : -1.f});
  const uint32_t faces[6][4] = {{0, 1, 3, 2}, {4, 6, 7, 5}, {0, 4, 5, 1}, {2, 3, 7, 6}, {0, 2, 6, 4}, {1, 5, 7, 3}};
  for (auto& f : faces) p.idx.insert(p.idx.end(), {f[0], f[1], f[2], f[0], f[2], f[3]});
  p.material = material;
  p.compute_normals();
  mesh->prims.push_back(std::move(p));
  return mesh;
}

std::shared_ptr<Mesh> make_floor(int material) {
  auto mesh = std::make_shared<Mesh>();
  mesh->name = "floor";
  Primitive p;
  p.pos = {{-50, 0, -50}, {50, 0, -50}, {50, 0, 50}, {-50, 0, 50}};
  p.uv = {{0, 0}, {1, 0}, {1, 1}, {0, 1}};
  p.idx = {0, 2, 1, 0, 3, 2};
  p.material = material;
  p.compute_normals();
  mesh->prims.push_back(std::move(p));
  return mesh;
}

// Copies a .glb with its JSON chunk edited (the binary chunk is kept as is).
bool rewrite_glb_json(const fs::path& in, const fs::path& out, const std::function<void(json&)>& edit) {
  std::ifstream f(in, std::ios::binary);
  std::vector<uint8_t> bytes((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
  if (bytes.size() < 20) return false;
  uint32_t json_len;
  std::memcpy(&json_len, &bytes[12], 4);
  json j = json::parse(std::string(bytes.begin() + 20, bytes.begin() + 20 + json_len));
  edit(j);
  std::string text = j.dump();
  while (text.size() % 4) text += ' ';
  std::vector<uint8_t> rest(bytes.begin() + 20 + json_len, bytes.end());
  uint32_t total = (uint32_t)(12 + 8 + text.size() + rest.size());
  uint32_t header[5] = {0x46546C67u, 2, total, (uint32_t)text.size(), 0x4E4F534Au};
  std::ofstream o(out, std::ios::binary);
  o.write((const char*)header, sizeof(header));
  o.write(text.data(), (std::streamsize)text.size());
  o.write((const char*)rest.data(), (std::streamsize)rest.size());
  return (bool)o;
}

void write_text(const fs::path& path, const std::string& text) {
  fs::create_directories(path.parent_path());
  std::ofstream(path, std::ios::binary) << text;
}

// the printf arguments of a text: "%zu objects, %s" -> "zs"
std::string format_args(const std::string& s) {
  static const std::regex spec("%[-+ #0]*[0-9]*(\\.[0-9]+)?(hh|h|ll|l|z|j|t|L)?([diouxXeEfgGcsp%])");
  std::string out;
  for (auto it = std::sregex_iterator(s.begin(), s.end(), spec); it != std::sregex_iterator(); ++it) {
    const std::string conv = (*it)[3].str();
    if (conv != "%") out += (*it)[2].str() + conv;
  }
  return out;
}

json read_json_file(const fs::path& path) {
  std::ifstream f(path, std::ios::binary);
  std::string text((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
  return json::parse(text, nullptr, false);
}

// a .jsonc of the level builder: its comment lines left out
std::string read_text_file(const fs::path& path) {
  std::ifstream f(path, std::ios::binary);
  return std::string((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}

json read_jsonc_file(const fs::path& path) {
  std::ifstream f(path, std::ios::binary);
  std::string text, line;
  while (std::getline(f, line)) {
    const size_t at = line.find_first_not_of(" \t");
    if (at == std::string::npos || line.compare(at, 2, "//") != 0) text += line + "\n";
  }
  return json::parse(text, nullptr, false);
}

// the JSON chunk of a .glb
json read_glb_json(const fs::path& path) {
  std::ifstream f(path, std::ios::binary);
  std::vector<uint8_t> bytes((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
  if (bytes.size() < 20) return json();
  uint32_t json_len;
  std::memcpy(&json_len, &bytes[12], 4);
  if (20 + (size_t)json_len > bytes.size()) return json();
  return json::parse(std::string(bytes.begin() + 20, bytes.begin() + 20 + json_len), nullptr, false);
}

// The bytes of an accessor of a .glb (its binary chunk)
std::vector<uint8_t> glb_accessor_bytes(const fs::path& path, const json& glb, int accessor) {
  std::ifstream f(path, std::ios::binary);
  std::vector<uint8_t> bytes((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
  if (bytes.size() < 20 || !glb.contains("accessors") || accessor < 0 || accessor >= (int)glb["accessors"].size())
    return {};
  uint32_t json_len;
  std::memcpy(&json_len, &bytes[12], 4);
  const size_t bin = 20 + (size_t)json_len + 8;
  const json& a = glb["accessors"][accessor];
  const json& v = glb["bufferViews"][a.value("bufferView", 0)];
  const size_t start = bin + v.value("byteOffset", (size_t)0) + a.value("byteOffset", (size_t)0);
  const size_t len = v.value("byteLength", (size_t)0);
  if (start + len > bytes.size()) return {};
  return std::vector<uint8_t>(bytes.begin() + start, bytes.begin() + start + len);
}

// A level written like ogle-extract writes one: a background with the terrain (tfrag), a decor
// prototype with two instances (TIE), collision and an actor; the model of its actor type; the
// models every level shares.
bool write_library(const fs::path& root) {
  std::string err;
  const fs::path game = root / "jak3";
  write_text(game / "library.json", R"({"game": "jak3", "name": "Jak 3", "levels": ["LEV.DGO"]})");
  write_text(game / "lev" / "level.json", R"({"level": "lev", "dgo": "LEV.DGO", "actors": 1})");
  fs::create_directories(game / "_disc" / "DGO");
  write_text(game / "_disc" / "DGO" / "stray.glb", "not a level");

  Scene bg;
  Material stone;
  stone.name = "stone";
  const int mat = bg.add_material(stone);
  Node tfrag{};
  tfrag.name = "lev-tfrag-0";
  const uint32_t tfrag_id = bg.add(tfrag);
  Node floor{};
  floor.name = "floor-0";
  floor.kind = NodeKind::Render;
  floor.parent = tfrag_id;
  floor.mesh = make_floor(mat);
  bg.add(floor);
  Node tie{};
  tie.name = "lev-tie-0";
  const uint32_t tie_id = bg.add(tie);
  Node proto{};
  proto.name = "rock.mb";
  proto.parent = tie_id;
  const uint32_t proto_id = bg.add(proto);
  auto rock = make_box("rock.mb", mat);
  for (int i = 0; i < 2; i++) {
    Node inst{};
    inst.name = "rock.mb-" + std::to_string(i);
    inst.kind = NodeKind::Render;
    inst.parent = proto_id;
    inst.mesh = rock;
    inst.local.t = {i * 10.f, 0, 0};
    bg.add(inst);
  }
  Node col{};
  col.name = "lev-collision";
  col.kind = NodeKind::Collision;
  col.mesh = make_floor(-1);
  bg.add(col);
  const fs::path project = root / "bg-project.glb";
  ExportOptions eo;
  if (!export_glb(bg, project.string(), eo, &err)) return false;
  // as a rip has them: no editor extras, collision marked for the builder, an actor node
  fs::create_directories(game / "lev" / "decor");
  bool ok = rewrite_glb_json(project, game / "lev" / "decor" / "lev-background.glb", [](json& j) {
    for (auto& n : j["nodes"]) {
      const std::string kind = n["extras"].value("og_kind", std::string());
      n["extras"] = json::object();
      if (kind == "collision") n["extras"] = {{"set_collision", 1}, {"set_invisible", 1}, {"collide_pat", 4660}};
    }
    j["nodes"].push_back({{"name", "crate-9"},
                          {"translation", {1.0, 2.0, 3.0}},
                          {"extras",
                           {{"kind", "actor"},
                            {"etype", "crate"},
                            {"aid", 123},
                            {"game_task", 0},
                            {"bsphere", {1.0, 2.0, 3.0, 5.0}},
                            {"lump", {{"name", "crate-9"}}}}}});
    j["scenes"][0]["nodes"].push_back((int)j["nodes"].size() - 1);
    j["asset"]["extras"] = {{"opengoal", {{"game", "jak3"}, {"level", "lev"}}}};
  });
  // the models: one for the actor, one shared by every level
  for (auto [folder, name] : {std::pair<const char*, const char*>{"lev", "crate-krimson-lod0"},
                             {"lev", "crate-krimson-lod1"},
                             {"common", "collectables-skill-lod0"}}) {
    Scene m;
    Node n{};
    n.name = name;
    n.kind = NodeKind::Render;
    n.mesh = make_box(name, -1);
    m.add(n);
    fs::create_directories(game / folder / "models");
    ok &= export_glb(m, (game / folder / "models" / (std::string(name) + ".glb")).string(), eo, &err);
  }
  return ok;
}

// A level as ogle-extract writes one, with the decor of `bg` and rip actors (glTF nodes with
// their extras).
bool write_rip_level(const fs::path& game, const std::string& name, const Scene& bg, const json& actors,
                     int collision_triangles, const json& node_extras = json::object(),
                     const json& actor_types = json::object()) {
  std::string err;
  const fs::path tmp = game / (name + "-project.glb");
  ExportOptions eo;
  if (!export_glb(bg, tmp.string(), eo, &err)) return false;
  fs::create_directories(game / name / "decor");
  const bool ok = rewrite_glb_json(tmp, game / name / "decor" / (name + "-background.glb"), [&](json& j) {
    for (auto& n : j["nodes"]) n["extras"] = node_extras.value(n.value("name", std::string()), json::object());
    for (const auto& a : actors) {
      j["nodes"].push_back(a);
      j["scenes"][0]["nodes"].push_back((int)j["nodes"].size() - 1);
    }
    j["asset"]["extras"] = {{"opengoal", {{"game", "jak3"}, {"level", name}}}};
  });
  fs::remove(tmp);
  write_text(game / name / "level.json", json{{"level", name},
                                              {"dgo", "X.DGO"},
                                              {"actors", actors.size()},
                                              {"actor_types", actor_types},
                                              {"collision_triangles", collision_triangles}}
                                             .dump());
  return ok;
}

json rip_actor(const std::string& name, const std::string& etype, const json& lump, Vec3 at = {}) {
  json l = lump;
  l["name"] = name;
  return {{"name", name},
          {"translation", {at.x, at.y, at.z}},
          {"extras", {{"kind", "actor"}, {"etype", etype}, {"aid", 1}, {"game_task", 0}, {"lump", l}}}};
}

// A game with a story: the level "town" (a rock the mission breaks into rubble, a crate there
// until the mission ends, a statue broken by it, a creature, a logic actor, a cutscene character,
// a water pool), the hut the game shows with it, the layer of the mission's actors, and "port",
// the level next to it (100 m away along x).
const char* kStoryGameData = R"({
 "format": 1, "game": "jak3",
 "actors": {"roles": {"grunt": "creature", "part-spawner": "logic"}, "creature_suffixes": ["-highres"],
            "effects": ["part-spawner"],
            "water_types": ["water-anim"], "water_looks": ["water-anim-test-pool"],
            "objects": {"crate": "crate", "statue-broken": "breakable", "no-model-thing": "other"}},
 "oceans": {"*ocean-map-test*": {"corner": [-96, -1.5, -96], "cell": 96, "cells": ["11", "01"], "color": [0.1, 0.2, 0.3]}},
 "levels": {
  "town": {"taskname": "town", "base_mask": 1, "ocean": "*ocean-map-test*", "layer": false, "traffic": false, "companions": ["hut"],
           "neighbors": ["hut", "port"],
           "continues": [{"name": "town-start", "position": [1, 0, 2], "shows": ["hut", "port", "town"]}]},
  "hut": {"taskname": "town", "base_mask": 1, "ocean": null, "layer": false, "traffic": false, "companions": [],
          "neighbors": ["port", "town"], "continues": []},
  "port": {"taskname": "port", "base_mask": 1, "ocean": null, "layer": false, "traffic": false, "companions": [],
           "neighbors": ["hut", "town"],
           "continues": [{"name": "port-start", "position": [100, 0, 5], "shows": ["port", "town"]}]},
  "lmission": {"taskname": "lmission", "base_mask": 0, "ocean": null, "layer": true, "traffic": false, "companions": []}
 },
 "story": {
  "reset_node": "start",
  "mask_bits": {"task0": 0, "task1": 1, "done": 8, "special": 12, "primary0": 13, "ctywide": 14, "never": 15},
  "nodes": [
   {"name": "none", "task": "", "level": ""},
   {"name": "start", "task": "intro", "level": "", "closed": true, "borrow": [["town", 0, null, null]]},
   {"name": "town-mission-introduction", "task": "town-mission", "level": "town", "parents": [1],
    "borrow": [["town", 0, "lmission", "display"]]},
   {"name": "town-mission-resolution", "task": "town-mission", "level": "town", "mask_op": "abs", "mask": 2,
    "close_task": true, "parents": [2], "borrow": [["town", 0, "lmission", "display"]]},
   {"name": "end", "task": "end", "level": "", "parents": [3]}
  ],
  "prototypes": {"town": [{"names": ["rock"], "when": ["not", ["complete", "town-mission"]]},
                          {"names": ["rubble"], "when": ["closed", "town-mission-resolution"]}]},
  "types": {"statue-broken": ["closed", "town-mission-resolution"]},
  "ocean_heights": {}
 }
})";

bool write_story_library(const fs::path& root) {
  const fs::path game = root / "jak3";
  write_text(game / "library.json", R"({"game": "jak3", "name": "Jak 3", "levels": ["X.DGO"]})");
  bool ok = true;
  auto decor = [](const std::string& level, std::vector<std::string> protos) {
    Scene bg;
    Node tfrag{};
    tfrag.name = level + "-tfrag-0";
    const uint32_t t = bg.add(tfrag);
    Node floor{};
    floor.name = level + "-floor";
    floor.kind = NodeKind::Render;
    floor.parent = t;
    floor.mesh = make_floor(-1);
    bg.add(floor);
    // a piece of the shell standing apart (the extractor splits the tfrag): a small wall
    Node wall{};
    wall.name = level + "-wall";
    wall.kind = NodeKind::Render;
    wall.parent = t;
    wall.mesh = make_box(level + "-wall", -1);
    for (auto& v : wall.mesh->prims[0].pos) v = v + Vec3{20, 0, 10};  // in the level's space, as ripped
    bg.add(wall);
    Node tie{};
    tie.name = level + "-tie-0";
    const uint32_t tie_id = bg.add(tie);
    for (const auto& name : protos) {
      Node proto{};
      proto.name = name + ".mb";
      proto.parent = tie_id;
      const uint32_t id = bg.add(proto);
      Node inst{};
      inst.name = name + ".mb-0";
      inst.kind = NodeKind::Render;
      inst.parent = id;
      inst.mesh = make_box(name, -1);
      bg.add(inst);
    }
    return bg;
  };
  const json town_actors = json::array({
      rip_actor("crate-1", "crate", {{"kill-mask", 4294905854.0}}),  // 0xffff0ffe: while the mask is task0
      rip_actor("grunt-2", "grunt", json::object()),
      rip_actor("part-spawner-3", "part-spawner", {{"art-name", "group-town-torch"}}),
      rip_actor("jak-highres-4", "jak-highres", {{"kill-mask", 32768}}),
      rip_actor("pool-5", "water-anim", {{"look", 0}}),
      rip_actor("statue-broken-6", "statue-broken", json::object()),
  });
  const json terrain = {{"town-floor", {{"og_terrain", true}}}, {"town-wall", {{"og_terrain", false}}}};
  const json town_types = {{"crate", {{"count", 1}, {"lump", {{"name", "crate-1"}, {"eco-info", {13, 10}},
                                                            {"kill-mask", 4294905854.0}, {"next-actor", "x"}}}}}};
  ok &= write_rip_level(game, "town", decor("town", {"rock", "rubble"}), town_actors, 2, terrain, town_types);
  ok &= write_rip_level(game, "hut", decor("hut", {}), json::array(), 2);
  // port: its shell 100 m away along x, as ripped (in the game's space)
  Scene port = decor("port", {"crane"});
  for (auto& n : port.nodes)
    if (n.mesh && n.name != "crane.mb-0")
      for (auto& v : n.mesh->prims[0].pos) v = v + Vec3{100, 0, 0};
  ok &= write_rip_level(game, "port", port, json::array({rip_actor("barrel-1", "crate", json::object(), {100, 0, 3})}),
                        2, {{"port-floor", {{"og_terrain", true}}}, {"port-wall", {{"og_terrain", false}}}});
  ok &= write_rip_level(game, "lmission", Scene(), json::array({rip_actor("lcrate-1", "crate", json::object())}), 0);
  // the models: the pool's look, the crate
  std::string err;
  fs::create_directories(game / "town" / "models");
  ExportOptions eo;
  for (const char* model : {"water-anim-test-pool-lod0", "crate-test-lod0"}) {
    Scene m;
    Node n{};
    n.name = model;
    n.kind = NodeKind::Render;
    n.mesh = make_box(n.name, -1);
    m.add(n);
    ok &= export_glb(m, (game / "town" / "models" / (std::string(model) + ".glb")).string(), eo, &err);
  }
  write_text(root / "game-data.json", kStoryGameData);
  return ok;
}

}  // namespace

int run_selftest() {
  std::printf("open-goal-level-editor selftest\n");
  const fs::path tmp = fs::temp_directory_path() / "ogle-selftest";
  std::error_code ec;
  fs::remove_all(tmp, ec);
  fs::create_directories(tmp);
  std::string err;

  // math
  {
    Quat q = Quat::from_euler_deg({10, 35, -20});
    Vec3 e = q.to_euler_deg();
    check(std::fabs(e.x - 10) < 0.01f && std::fabs(e.y - 35) < 0.01f && std::fabs(e.z + 20) < 0.01f,
          "euler <-> quaternion");
    // the angles read back give the same rotation, also turned a quarter around y and tilted (a
    // ramp turned 90 degrees): what the inspector writes back when a field is edited
    bool same = true;
    const Vec3 axes[] = {{1, 0, 0}, {0, 1, 0}, {0, 0, 1}};
    for (float yaw : {90.f, -90.f, 89.99f})
      for (const Quat& tilt : {Quat::axis_angle({1, 0, 0}, 0.4f), Quat::axis_angle({0, 0, 1}, -0.3f),
                               Quat::axis_angle(normalize(Vec3{1, 0, 1}), 0.5f)}) {
        const Quat r = (Quat::axis_angle({0, 1, 0}, radians(yaw)) * tilt).normalized();
        const Quat back = Quat::from_euler_deg(r.to_euler_deg());
        for (const Vec3& a : axes) same = same && length(back.rotate(a) - r.rotate(a)) < 1e-3f;
      }
    check(same, "euler: a quarter turn around y with a tilt reads back as the same rotation");
    Transform t{{1, 2, 3}, q, {2, 3, 4}};
    Transform back = Transform::from_matrix(t.matrix());
    check(length(back.t - t.t) < 1e-4f && length(back.s - t.s) < 1e-4f &&
              std::fabs(std::fabs(back.r.w) - std::fabs(q.w)) < 1e-4f,
          "TRS -> matrix -> TRS");
    Mat4 m = t.matrix();
    Mat4 id = m * m.inverse();
    check(std::fabs(id.at(0, 0) - 1) < 1e-4f && std::fabs(id.at(1, 3)) < 1e-4f, "matrix inverse");
  }

  // the library
  const fs::path lib_root = tmp / "library";
  check(write_library(lib_root), "library written in ogle-extract's layout");
  const std::string folder = game_folder(lib_root.string());
  check(fs::path(folder) == fs::absolute(lib_root / "jak3") && game_folder(folder) == folder &&
            game_folder((tmp / "nothing").string()).empty(),
        "library: the game's folder (library.json), a game folder chosen means its parent");
  AssetLibrary library;
  bool scanned = !folder.empty() && library.scan(folder, &err);
  const LevelEntry* level = library.level("lev");
  check(scanned && library.levels().size() == 1 && level && level->dgo == "LEV.DGO" && level->actors == 1,
        "library: one level with its level.json, _disc skipped");
  auto model = [&](const char* etype) {
    const ModelAsset* m = library.model_for_etype(etype, "lev");
    return m ? m->name + ":" + std::to_string(m->lod) : std::string();
  };
  check(model("crate") == "crate-krimson:0" && model("skill") == "collectables-skill:0" && model("babak").empty(),
        "actor models: by name start and end, lowest lod first");

  // the level
  Scene scene;
  bool opened = level && open_level(scene, library, nullptr, *level, &err);
  const Node* actor = nullptr;
  std::vector<uint32_t> instances;
  uint32_t floor = 0, collision = 0;
  for (const auto& n : scene.nodes) {
    if (n.kind == NodeKind::Actor) actor = &n;
    if (n.kind == NodeKind::Collision) collision = n.id;
    if (n.name == "floor-0") floor = n.id;
    if (n.name.rfind("rock.mb-", 0) == 0) instances.push_back(n.id);
  }
  check(opened && scene.settings.level == "lev", "level opened: its name");
  check(floor && scene.find(floor)->locked && instances.size() == 2 && !scene.find(instances[0])->locked &&
            scene.find(instances[0])->mesh == scene.find(instances[1])->mesh,
        "level: terrain locked, decor instances selectable and sharing their mesh");
  bool actor_model = false;
  if (actor)
    for (auto c : scene.children(actor->id))
      actor_model |= scene.find(c)->extras.value("og_actor_visual", std::string()) == "crate-krimson";
  check(actor && actor->extras.value("etype", std::string()) == "crate" && actor->extras.value("aid", 0) == 123 &&
            actor_model && length(actor->local.t - Vec3{1, 2, 3}) < 1e-5f,
        "level: actor with its data, position and model");
  check(collision && !scene.find(collision)->mesh->prims.empty() &&
            scene.find(collision)->mesh->prims[0].tri_pat == std::vector<uint32_t>(2, 4660u),
        "level: collision kept with its surface");

  // copy and paste
  if (instances.size() == 2 && actor) {
    NodeChange change(scene);
    const size_t before = scene.nodes.size();
    auto pasted = paste_nodes(scene, copy_nodes(scene, {instances[0], actor->id}));
    const Node* copy = pasted.size() == 2 ? scene.find(pasted[0]) : nullptr;
    const Node* actor_copy = pasted.size() == 2 ? scene.find(pasted[1]) : nullptr;
    const Node* original = scene.find(instances[0]);
    check(copy && copy->mesh == original->mesh && copy->parent == original->parent && copy->name != original->name &&
              length(scene.world(*copy).col3(3) - scene.world(*original).col3(3)) < 1e-5f,
          "paste: a copy in place, sharing the mesh, under the same parent");
    check(actor_copy && actor_copy->kind == NodeKind::Actor && !actor_copy->extras.contains("aid") &&
              scene.children(actor_copy->id).size() == 1,
          "paste: an actor copy with its model and without the original's id");
    HistoryEntry e = change.commit("paste");
    e.undo();
    check(scene.nodes.size() == before, "paste undone");
    e.redo();
    check(scene.nodes.size() == before + 3, "paste redone (the instance, the actor and its model)");
  }

  // prefabs
  const fs::path prefabs = tmp / "prefabs";
  const std::string prefab_path = new_prefab_path(prefabs.string(), "Rocks & crate");
  check(fs::path(prefab_path).filename() == "Rocks_crate.glb", "prefab: a file name from its name");
  bool saved = instances.size() == 2 && save_prefab(scene, instances, prefab_path, &err);
  check(saved && fs::path(new_prefab_path(prefabs.string(), "Rocks & crate")).filename() == "Rocks_crate-2.glb",
        "prefab saved, the next one of that name gets another file");
  auto listed = list_prefabs(prefabs.string());
  check(listed.size() == 1 && listed[0].name == "Rocks_crate", "prefabs listed");
  Scene prefab;
  bool loaded = saved && load_prefab(prefab_path, &prefab, &err);
  AABB pb = prefab.bounds();
  check(loaded && prefab.nodes.size() == 2 && pb.valid() && std::fabs(pb.lo.y) < 1e-4f &&
            std::fabs(pb.center().x) < 1e-4f && std::fabs(pb.center().z) < 1e-4f,
        "prefab: its elements around a pivot at the bottom center");
  if (loaded) {
    std::map<const Mesh*, std::shared_ptr<Mesh>> merged;
    const size_t before = scene.nodes.size();
    auto a = place_prefab(scene, prefab, Mat4::translate({100, 5, 0}), &merged);
    auto b = place_prefab(scene, prefab, Mat4::translate({200, 5, 0}), &merged);
    AABB placed;
    for (auto id : a) placed.add(scene.world_bounds(*scene.find(id)));
    check(a.size() == 2 && b.size() == 2 && scene.nodes.size() == before + 4 &&
              scene.find(a[0])->mesh == scene.find(b[0])->mesh && std::fabs(placed.lo.y - 5) < 1e-4f &&
              std::fabs(placed.center().x - 100) < 1e-3f,
          "prefab placed twice: on the point given, meshes shared");
  }
  {
    // an actor of a prefab placed twice: two actors of a level cannot share a name
    Scene with_actor, doc;
    Node crate{};
    crate.name = "crate-1";
    crate.kind = NodeKind::Actor;
    crate.extras = json{{"etype", "crate"}, {"lump", {{"name", "crate-1"}}}};
    with_actor.add(crate);
    std::map<const Mesh*, std::shared_ptr<Mesh>> merged;
    const auto first = place_prefab(doc, with_actor, Mat4::identity(), &merged);
    const auto second = place_prefab(doc, with_actor, Mat4::translate({4, 0, 0}), &merged);
    const Node* x = first.empty() ? nullptr : doc.find(first[0]);
    const Node* y = second.empty() ? nullptr : doc.find(second[0]);
    check(x && y && x->name == "crate-1" && y->name == "crate-2" &&
              y->extras["lump"].value("name", std::string()) == "crate-2",
          "prefab: its actors named like the game names them, in their data too");
  }

  // project round trip
  {
    Material glow;
    glow.name = "glow";
    glow.alpha = AlphaMode::Blend;
    glow.blend = BlendMode::Add;
    glow.cutoff = 0.15f;  // the alpha test of the PS2, kept by blended surfaces too
    Node n{};
    n.name = "glow";
    n.kind = NodeKind::Render;
    n.mesh = make_box("glow", scene.add_material(glow));
    n.local.s = {3.f, 1.f, 0.5f};  // stretched along x, flattened along z
    scene.add(n);
    const std::string project = (tmp / "project.glb").string();
    Scene back;
    bool ok = save_project(scene, project, &err) && open_project(back, project, &err);
    size_t kinds[(int)NodeKind::Count] = {}, kinds_back[(int)NodeKind::Count] = {};
    for (const auto& x : scene.nodes) kinds[(int)x.kind]++;
    for (const auto& x : back.nodes) kinds_back[(int)x.kind]++;
    check(ok && std::equal(std::begin(kinds), std::end(kinds), std::begin(kinds_back)) &&
              back.settings.level == "lev",
          "project: saved and opened again, same nodes and level");
    std::set<const Mesh*> rocks;
    bool locked = false, pats = false, blend = false, aid = false, stretched = false;
    for (const auto& x : back.nodes) {
      // the level's instances and the pasted copy (the prefab placements have their own mesh)
      if (x.name.rfind("rock.mb", 0) == 0 && x.kind == NodeKind::Render && x.parent) rocks.insert(x.mesh.get());
      if (x.name == "floor-0") locked = x.locked;
      if (x.kind == NodeKind::Collision) pats = x.mesh->prims[0].tri_pat == std::vector<uint32_t>(2, 4660u);
      if (x.kind == NodeKind::Actor && x.extras.value("aid", 0) == 123) aid = true;
      if (x.name == "glow") {
        const Material& m = back.materials[x.mesh->prims[0].material];
        blend = m.blend == BlendMode::Add && std::fabs(m.cutoff - 0.15f) < 1e-6f;
        const AABB b = back.world_bounds(x);
        stretched = length(x.local.s - Vec3{3.f, 1.f, 0.5f}) < 1e-4f && std::fabs(b.hi.x - b.lo.x - 6.f) < 1e-3f &&
                    std::fabs(b.hi.z - b.lo.z - 1.f) < 1e-3f;
      }
    }
    check(stretched, "project: the scale of a stretched element kept (and its bounds)");
    check(rocks.size() == 1 && locked && pats && aid && blend,
          "project: instancing, locks, collision surfaces, actor data, additive material and its alpha test kept");
    Scene empty, empty_back;
    const std::string empty_path = (tmp / "empty.glb").string();
    check(save_project(empty, empty_path, &err) && open_project(empty_back, empty_path, &err) &&
              empty_back.nodes.empty(),
          "project: an empty document saved and opened again");
  }

  // a level with game data: creatures and logic left out, water models, companion and layer
  // levels, the story view
  try {
    const fs::path story_root = tmp / "story-library";
    check(write_story_library(story_root), "story: a library with a story written");
    GameData data;
    const bool read = data.load((story_root / "game-data.json").string(), &err);
    check(read && data.role("grunt") == ActorRole::Creature && data.role("part-spawner") == ActorRole::Logic &&
              data.role("jak-highres") == ActorRole::Creature && data.role("crate") == ActorRole::Object &&
              data.water_model(0) == "water-anim-test-pool" && data.step_count() == 5,
          "game data: roles, water looks, story nodes");
    AssetLibrary story_lib;
    const bool scanned_story = story_lib.scan((story_root / "jak3").string(), &err);
    std::string listed_levels;
    for (const auto& l : story_lib.levels())
      if (level_listed(l, &data)) listed_levels += l.name + " ";
    check(scanned_story && listed_levels == "hut port town ", "levels listed: the layer of a mission is not");
    Scene town;
    const LevelEntry* town_entry = story_lib.level("town");
    const bool town_opened = town_entry && open_level(town, story_lib, &data, *town_entry, &err);
    auto node_named = [&](const std::string& name) -> Node* {
      for (auto& x : town.nodes)
        if (x.name == name) return &x;
      return nullptr;
    };
    std::string actors;
    for (const auto& x : town.nodes)
      if (x.kind == NodeKind::Actor) actors += x.name + " ";
    check(town_opened && actors.find("grunt") == std::string::npos && actors.find("part-spawner") != std::string::npos &&
              actors.find("jak-highres") == std::string::npos && node_named("crate-1") && node_named("statue-broken-6"),
          "level with game data: creatures and cutscene characters left out, its particle effects kept");
    bool pool_model = false;
    if (Node* pool = node_named("pool-5"))
      for (auto c : town.children(pool->id))
        pool_model |= town.find(c)->extras.value("og_actor_visual", std::string()) == "water-anim-test-pool";
    check(pool_model, "water actor: the model of its look");
    Node* hut = nullptr;
    Node* layer = nullptr;
    for (auto& x : town.nodes) {
      if (x.extras.value("og_companion", std::string()) == "hut") hut = &x;
      if (x.extras.value("og_layer", std::string()) == "lmission") layer = &x;
    }
    check(hut && layer && !town.children(hut->id).empty() && node_named("lcrate-1"),
          "level with game data: its companion and the layer of its mission under their groups");
    // the story: step 1 (the mission under way), step 4 (the mission over)
    auto shown = [&](const char* name) {
      Node* x = node_named(name);
      return x && town.effectively_visible(*x);
    };
    StoryView view;
    view.step = 1;
    apply_story(town, &data, view);
    const bool start = shown("crate-1") && !shown("statue-broken-6") && shown("rock.mb") && !shown("rubble.mb") &&
                       shown("lcrate-1") && shown("hut");
    view.step = 4;
    apply_story(town, &data, view);
    const bool after = !shown("crate-1") && shown("statue-broken-6") && !shown("rock.mb") && shown("rubble.mb") &&
                       !shown("lcrate-1") && shown("hut");
    view.enabled = false;
    apply_story(town, &data, view);
    const bool all = shown("crate-1") && shown("statue-broken-6") && shown("rock.mb") && shown("rubble.mb") &&
                     shown("lcrate-1");
    check(start && after && all,
          "story: kill masks, type and prototype rules, borrowed layer at two steps; everything when off");
    check(story_changes(town, data) == std::vector<int>{4}, "story: the steps where the level changes");
    // several levels: port opened, town added at its place with its companion and its mission's
    // layer under its group, shown as when the player is in town (its layer borrowed at the start)
    {
      Scene doc;
      const LevelEntry* port_entry = story_lib.level("port");
      const bool port_opened = port_entry && open_level(doc, story_lib, &data, *port_entry, &err);
      const uint32_t town_group = port_opened ? add_level(doc, story_lib, &data, *town_entry, &err) : 0;
      auto in_doc = [&](const std::string& name) -> const Node* {
        for (const auto& x : doc.nodes)
          if (x.name == name) return &x;
        return nullptr;
      };
      const Node* hut_group = nullptr;
      const Node* layer_group = nullptr;
      for (const auto& x : doc.nodes) {
        if (x.extras.value("og_companion", std::string()) == "hut") hut_group = &x;
        if (x.extras.value("og_layer", std::string()) == "lmission") layer_group = &x;
      }
      const Node* wall_alone = node_named("town-wall");
      const Node* wall_added = in_doc("town-wall");
      const Node* port_wall = in_doc("port-wall");
      const bool placed = town_group && doc.find(town_group)->extras.value("og_level", std::string()) == "town" &&
                          hut_group && layer_group && doc.is_ancestor(town_group, hut_group->id) &&
                          doc.is_ancestor(town_group, layer_group->id) && wall_alone && wall_added && port_wall &&
                          length(doc.world_bounds(*wall_added).center() - town.world_bounds(*wall_alone).center()) < 1e-4f &&
                          std::fabs(doc.world_bounds(*port_wall).center().x - 120.f) < 1e-3f;
      check(placed, "levels: one added at its place, its companion and mission layer under its group");
      auto doc_shown = [&](const char* name) {
        const Node* x = in_doc(name);
        return x && doc.effectively_visible(*x);
      };
      StoryView v;
      v.step = 1;
      apply_story(doc, &data, v);
      const bool at_start = doc_shown("crate-1") && doc_shown("lcrate-1") && doc_shown("barrel-1");
      v.step = 4;
      apply_story(doc, &data, v);
      const bool at_end = !doc_shown("crate-1") && !doc_shown("lcrate-1") && doc_shown("rubble.mb");
      check(at_start && at_end, "levels: an added level shown as when the player is there, its layer too");
      // each level at its own point of the story: town still at the start while port is at the end
      v.step = 4;
      v.level_steps["town"] = 1;
      apply_story(doc, &data, v);
      const bool town_early = doc_shown("crate-1") && doc_shown("lcrate-1");
      check(town_early && story_changes(doc, data, "town") == std::vector<int>{4} &&
                story_changes(doc, data, "no-such-level").empty(),
            "story: each level at its own step, and the steps where an added level changes");
      Scene bare;
      const bool bare_opened = open_level(bare, story_lib, &data, *town_entry, &err, false);
      size_t bare_actors = 0, bare_decor = 0;
      for (const auto& x : bare.nodes) {
        bare_actors += x.kind == NodeKind::Actor;
        bare_decor += x.kind == NodeKind::Render;
      }
      check(bare_opened && bare_actors == 0 && bare_decor > 0, "levels: opened without their actors, the decor alone");
      const LevelInfo* town_info = data.level("town");
      const LevelInfo* port_info = data.level("port");
      check(town_info && std::find(town_info->neighbors.begin(), town_info->neighbors.end(), "port") !=
                             town_info->neighbors.end() &&
                port_info && port_info->continues.size() == 1 && port_info->continues[0].name == "port-start" &&
                length(port_info->continues[0].position - Vec3{100, 0, 5}) < 1e-4f &&
                port_info->continues[0].shows == std::vector<std::string>{"port", "town"},
            "game data: a level's neighbors and continue points");
    }

    check(data.ocean("*ocean-map-test*") && data.ocean("*ocean-map-test*")->cells == std::vector<uint8_t>{1, 1, 0, 1} &&
              std::fabs(data.ocean_height("town", 1) + 1.5f) < 1e-5f,
          "game data: the ocean of a level, its cells and height");

    // the shell split in pieces: the terrain locked, the others free, around their own origin
    Node* floor_piece = node_named("town-floor");
    Node* wall_piece = node_named("town-wall");
    check(floor_piece && floor_piece->locked && wall_piece && !wall_piece->locked &&
              length(wall_piece->local.t - Vec3{20, 0, 10}) < 1e-4f &&
              std::fabs(wall_piece->mesh->bounds().center().x) < 1e-4f && std::fabs(wall_piece->mesh->bounds().lo.y) < 1e-4f,
          "level pieces: the terrain locked, a piece of the shell selectable, its origin at its base");

    // the catalog: the objects with a model, the data of an actor of the game without its level's
    const auto objects = build_catalog(story_lib, &data);
    const CatalogObject* crate = nullptr;
    bool water = false, no_model = false;
    for (const auto& o : objects) {
      if (o.etype == "crate") crate = &o;
      if (o.family == "water" && o.model == "water-anim-test-pool" && o.lump.value("look", -1) == 0) water = true;
      if (o.etype == "no-model-thing") no_model = true;
    }
    check(crate && crate->model == "crate-test" && crate->family == "crate" && crate->count == 1 &&
              crate->lump.contains("eco-info") && !crate->lump.contains("kill-mask") &&
              !crate->lump.contains("next-actor") && !crate->lump.contains("name") && water && !no_model,
          "catalog: objects with their model, family and data (without their level's), water by look");
    Node placed = crate ? make_catalog_actor(*crate, "crate-7") : Node{};
    check(placed.kind == NodeKind::Actor && placed.extras.value("etype", std::string()) == "crate" &&
              placed.extras["lump"].value("name", std::string()) == "crate-7",
          "catalog: a new actor of an object, named in its data");

    // the decor parts of a level file: its prototypes and the pieces of its shell
    Scene town_file;
    ImportOptions io;
    ImportResult ir;
    const bool read_town = import_gltf(town_file, town_entry->background, io, &ir, &err);
    const auto parts = list_parts(town_file);
    std::string part_names;
    for (const auto& p : parts)
      part_names += p.name + (p.kind == CatalogPart::Kind::Piece ? "(piece) " : " ");
    std::shared_ptr<Mesh> wall_mesh;
    for (const auto& p : parts)
      if (p.kind == CatalogPart::Kind::Piece) wall_mesh = part_mesh(town_file, p);
    check(read_town && part_names == "rock.mb rubble.mb town-wall(piece) " && wall_mesh &&
              std::fabs(wall_mesh->bounds().lo.y) < 1e-4f && std::fabs(wall_mesh->bounds().center().z) < 1e-4f,
          "decor parts: prototypes and pieces (not the terrain), each around its base");

    // the index of the library: the same parts without loading the level, the art-names of its
    // actors; the catalog with it: a particle effect per group, the decor of every level
    LevelIndex town_index;
    bool same_parts = index_level(town_entry->background, &town_index, &err) && town_index.parts.size() == parts.size();
    for (size_t i = 0; same_parts && i < parts.size(); i++) {
      const CatalogPart &a = town_index.parts[i], &b = parts[i];
      same_parts = a.name == b.name && a.mesh_node == b.mesh_node && a.kind == b.kind && a.triangles == b.triangles &&
                   a.copies == b.copies;
    }
    check(same_parts && town_index.art_names["part-spawner"]["group-town-torch"] == 1,
          "library index: the decor parts of a level without loading it, the art-names of its actors");
    LibraryIndex lib_index;
    lib_index.start(story_lib, (tmp / "index").string());
    for (int i = 0; i < 500 && !lib_index.ready(); i++) std::this_thread::sleep_for(std::chrono::milliseconds(10));
    const auto with_index = build_catalog(story_lib, &data, &lib_index);
    const CatalogObject* torch = nullptr;
    for (const auto& o : with_index)
      if (o.family == "effect") torch = &o;
    const auto every_part = all_decor(lib_index, story_lib);
    size_t town_parts = 0;
    for (const auto& e : every_part) town_parts += e.level == "town";
    check(lib_index.ready() && torch && torch->name == "town-torch" && torch->etype == "part-spawner" &&
              torch->lump.value("art-name", std::string()) == "group-town-torch" && torch->count == 1 &&
              town_parts == parts.size(),
          "catalog: a particle effect per particle group of the levels, the decor of every level");

    // a copied actor is named like the others of its type, in its data too
    if (Node* c = node_named("crate-1")) {
      auto copies = paste_nodes(town, copy_nodes(town, {c->id}));
      const Node* copy = copies.empty() ? nullptr : town.find(copies[0]);
      check(copy && copy->name == "crate-2" && copy->extras["lump"].value("name", std::string()) == "crate-2",
            "paste: an actor copy named <type>-<number>, in its data too");
    }
  } catch (const std::exception& e) {
    std::printf("  exception: %s\n", e.what());
    check(false, "story: no exception");
  }

  // a level loaded apart, appended to a document: new ids, its parents kept, the textures and
  // materials the document has shared, its meshes on the document's materials
  {
    auto texture = [](const char* name, uint8_t value) {
      Texture t;
      t.name = name;
      t.width = t.height = 2;
      t.rgba.assign(16, value);
      return t;
    };
    Scene doc, other;
    doc.add_texture(texture("wall", 10));
    Material wall_mat;
    wall_mat.name = "wall";
    wall_mat.texture = 0;
    doc.add_material(wall_mat);
    Node a{};
    a.name = "a";
    a.kind = NodeKind::Render;
    a.mesh = make_box("a", 0);
    doc.add(a);
    other.add_texture(texture("roof", 20));
    other.add_texture(texture("wall", 10));
    Material roof_mat;
    roof_mat.name = "roof";
    roof_mat.texture = 0;
    Material other_wall = wall_mat;
    other_wall.texture = 1;
    other.add_material(roof_mat);
    other.add_material(other_wall);
    Node group{};
    group.name = "group";
    const uint32_t g = other.add(group);
    Node roof{}, wall{};
    roof.name = "roof";
    roof.kind = wall.kind = NodeKind::Render;
    roof.parent = wall.parent = g;
    roof.mesh = make_box("roof", 0);
    wall.name = "wall";
    wall.mesh = make_box("wall", 1);
    other.add(roof);
    other.add(wall);
    const auto roots = doc.append(std::move(other));
    std::set<uint32_t> ids;
    for (const auto& n : doc.nodes) ids.insert(n.id);
    const Node* new_group = roots.size() == 1 ? doc.find(roots[0]) : nullptr;
    const Node* new_roof = nullptr;
    const Node* new_wall = nullptr;
    for (const auto& n : doc.nodes) {
      if (n.name == "roof") new_roof = &n;
      if (n.name == "wall") new_wall = &n;
    }
    const bool appended =
        ids.size() == 4 && new_group && new_roof && new_wall && new_roof->parent == new_group->id &&
        new_wall->parent == new_group->id && doc.textures.size() == 2 && doc.materials.size() == 2 &&
        new_wall->mesh->prims[0].material == 0 && doc.materials[new_roof->mesh->prims[0].material].name == "roof" &&
        doc.textures[doc.materials[new_roof->mesh->prims[0].material].texture].name == "roof" && other.nodes.empty();
    check(appended, "levels: a level appended to a document, its textures and materials shared or renumbered");
  }

  // nav meshes: a floor around a block, in the game's limits; a floor too big for one mesh cut in
  // several; the route table reaches every polygon; a node and a project keep them; the blob for
  // the game reads back; the game's own mesh (OGLE_NAV_REFERENCE: a decompiler_out/jak3/entities/
  // <level>-nav.json) passes the checks and gets the same polygon grid
  {
    auto floor_with_blocks = [](float half, int blocks, std::vector<Vec3>* v, std::vector<uint32_t>* t) {
      *v = {{-half, 0, -half}, {half, 0, -half}, {half, 0, half}, {-half, 0, half}};
      *t = {0, 2, 1, 0, 3, 2};
      const uint32_t faces[6][4] = {{0, 1, 3, 2}, {4, 6, 7, 5}, {0, 4, 5, 1}, {2, 3, 7, 6}, {0, 2, 6, 4}, {1, 5, 7, 3}};
      for (int bx = 0; bx < blocks; bx++)
        for (int bz = 0; bz < blocks; bz++) {
          // blocks 4 m wide, 3 m high, on a grid (one block in the middle when blocks == 1)
          const float step = blocks > 1 ? 2.f * half / (float)(blocks + 1) : 0.f;
          const float cx = blocks > 1 ? -half + step * (float)(bx + 1) : 0.f;
          const float cz = blocks > 1 ? -half + step * (float)(bz + 1) : 0.f;
          const uint32_t b = (uint32_t)v->size();
          for (int i = 0; i < 8; i++)
            v->push_back({cx + ((i & 1) ? 2.f : -2.f), (i & 2) ? 3.f : 0.f, cz + ((i & 4) ? 2.f : -2.f)});
          for (auto& f : faces) t->insert(t->end(), {b + f[0], b + f[1], b + f[2], b + f[0], b + f[2], b + f[3]});
        }
    };
    auto route_reaches_all = [](const NavMeshData& m) {
      const auto route = navmesh_route_table(m);
      const int n = (int)m.polys.size();
      for (int from = 0; from < n; from++)
        for (int to = 0; to < n; to++) {
          int cur = from, steps = 0;
          while (cur != to && steps++ < n) {
            const size_t bit = ((size_t)to + (size_t)cur * n) * 2;
            const int edge = (route[bit >> 3] >> (bit & 7)) & 3;
            if (edge >= m.polys[cur].count || m.polys[cur].adj[edge] < 0) return false;
            cur = m.polys[cur].adj[edge];
          }
          if (cur != to) return false;
        }
      return true;
    };
    std::vector<Vec3> v;
    std::vector<uint32_t> t;
    floor_with_blocks(20.f, 1, &v, &t);
    AABB region;
    region.add({-20, -1, -20});
    region.add({20, 4, 20});
    std::vector<NavMeshData> meshes;
    NavBuildStats st;
    std::string nav_err;
    const bool built = build_navmeshes(v, t, region, NavParams{}, &meshes, &st, &nav_err);
    bool fine = built && !meshes.empty(), block_free = true;
    float area = 0;
    for (const auto& m : meshes) {
      fine &= navmesh_problem(m).empty();
      area += m.area();
      for (const auto& p : m.polys) {
        Vec3 c{0, 0, 0};
        for (int k = 0; k < p.count; k++) c += m.verts[p.v[k]];
        c = c / (float)p.count + m.origin;
        if (std::fabs(c.x) < 2.f && std::fabs(c.z) < 2.f) block_free = false;
      }
    }
    // the floor without its border and the block, both widened by the creature's radius
    check(fine && block_free && area > 1300.f && area < 1520.f,
          "nav mesh: a floor around a block, convex polygons of 3 or 4 corners facing up, in one piece");
    check(built && route_reaches_all(meshes[0]), "nav mesh: the route table leads from every polygon to every other");
    // what the extractor writes for the decor (blended with its texture's alpha, color x2) is walked
    // on; the water the editor draws and the glows are not
    Material decor_mat, water_mat, glow_mat;
    decor_mat.alpha = AlphaMode::Blend;
    decor_mat.base_color = {2, 2, 2, 2};
    water_mat.alpha = AlphaMode::Blend;
    water_mat.base_color = {1.1f, 1.6f, 2.f, 0.75f};
    glow_mat.alpha = AlphaMode::Blend;
    glow_mat.base_color = {2, 2, 2, 2};
    glow_mat.blend = BlendMode::Add;
    check(!navmesh_ignores(decor_mat) && navmesh_ignores(water_mat) && navmesh_ignores(glow_mat),
          "nav mesh: the levels' blended decor is walkable, water and glows are not");
    // a roof apart from the floor: kept, unless only the ground next to a point of the floor is
    // wanted (a point in the block, over a hole of the nav mesh, finds the floor around it)
    {
      std::vector<Vec3> two_v = v;
      std::vector<uint32_t> two_t = t;
      const uint32_t b = (uint32_t)two_v.size();
      two_v.insert(two_v.end(), {{30, 6, -10}, {50, 6, -10}, {50, 6, 10}, {30, 6, 10}});
      two_t.insert(two_t.end(), {b, b + 2, b + 1, b, b + 3, b + 2});
      AABB two_region = region;
      two_region.add({50, 8, 10});
      std::vector<NavMeshData> both, floor_only;
      NavBuildStats both_st, floor_st;
      NavParams from_floor;
      from_floor.keep_from = {{0, 0, 0}};
      const bool made = build_navmeshes(two_v, two_t, two_region, NavParams{}, &both, &both_st, &nav_err) &&
                        build_navmeshes(two_v, two_t, two_region, from_floor, &floor_only, &floor_st, &nav_err);
      bool floor_kept = made && floor_only.size() == 1 && floor_st.left_out == 1 && both.size() == 2;
      for (const auto& m : floor_only) floor_kept = floor_kept && m.origin.y < 1.f;
      check(floor_kept, "nav mesh: only the ground next to a point, the roof apart left out");
    }

    std::vector<Vec3> big_v;
    std::vector<uint32_t> big_t;
    floor_with_blocks(150.f, 12, &big_v, &big_t);
    AABB big_region;
    big_region.add({-150, -1, -150});
    big_region.add({150, 4, 150});
    NavParams fine_params;
    fine_params.max_edge = 4.f;
    std::vector<NavMeshData> big;
    NavBuildStats big_st;
    const bool big_built = build_navmeshes(big_v, big_t, big_region, fine_params, &big, &big_st, &nav_err);
    bool big_fine = big_built && big.size() >= 2 && big_st.split >= 1;
    for (const auto& m : big) big_fine &= m.polys.size() <= 255 && navmesh_problem(m).empty();
    check(big_fine, "nav mesh: ground of more than 255 polygons cut in several meshes, each suiting the game");

    // a node, a project
    NavMeshData back;
    const Node nav_node = built ? navmesh_node(meshes[0], "navmesh-test") : Node{};
    bool same = built && navmesh_of_node(nav_node, &back) && back.polys.size() == meshes[0].polys.size() &&
                back.verts.size() == meshes[0].verts.size();
    for (size_t i = 0; same && i < back.polys.size(); i++)
      same = back.polys[i].count == meshes[0].polys[i].count &&
             std::equal(back.polys[i].adj, back.polys[i].adj + 4, meshes[0].polys[i].adj);
    Scene nav_scene, nav_back;
    if (built) nav_scene.add(nav_node);
    const std::string nav_project = (tmp / "navmesh.glb").string();
    bool kept = built && save_project(nav_scene, nav_project, &err) && open_project(nav_back, nav_project, &err);
    NavMeshData reopened;
    const Node* reopened_node = nullptr;
    for (const auto& n : nav_back.nodes)
      if (n.kind == NodeKind::NavMesh) reopened_node = &n;
    kept = kept && reopened_node && navmesh_of_node(*reopened_node, &reopened) &&
           reopened.polys.size() == meshes[0].polys.size() &&
           length(nav_back.world(*reopened_node).col3(3) - meshes[0].origin) < 1e-3f;
    check(same && kept, "nav mesh: kept by its node and by a project");

    // the blob for the game
    NavBlobView view;
    std::vector<NavMeshData> from_blob;
    bool blob_ok = built && read_navmesh_blob(navmesh_blob({{&meshes[0], 61000, "navmesh-test"}}), &view) &&
                   navmeshes_of_blob(view, &from_blob) && from_blob.size() == 1 &&
                   from_blob[0].polys.size() == meshes[0].polys.size();
    if (blob_ok) {
      const int ent = view.roots.at(0).second;
      const int mesh_ptr = view.ptr(ent + 48);
      blob_ok = view.u32(ent + 44) == 61000 && mesh_ptr > 0 && view.u8(mesh_ptr + 9) == meshes[0].polys.size() &&
                view.ptr(mesh_ptr + 12) > 0 && view.ptr(mesh_ptr + 16) > 0 &&
                length(from_blob[0].origin - meshes[0].origin) < 1e-3f;
      for (size_t i = 0; blob_ok && i < from_blob[0].polys.size(); i++)
        for (int k = 0; k < from_blob[0].polys[i].count; k++)
          blob_ok = blob_ok && from_blob[0].polys[i].adj[k] == meshes[0].polys[i].adj[k] &&
                    length(from_blob[0].verts[from_blob[0].polys[i].v[k]] - meshes[0].verts[meshes[0].polys[i].v[k]]) < 1e-3f;
      // res tags sorted as the game looks them up
      std::vector<std::string> tag_names;
      for (const auto& [word, name] : view.symbols)
        if (name != "#f") tag_names.push_back(name);
      blob_ok = blob_ok && tag_names == std::vector<std::string>{"name", "nearest-y-threshold", "nav-max-users"};
    }
    check(blob_ok, "nav mesh: the entity-nav-mesh blob for the level builder reads back (polygons, route, tags)");

    if (const char* ref = std::getenv("OGLE_NAV_REFERENCE")) {
      std::ifstream f(ref);
      const json j = json::parse(f, nullptr, false);
      NavBlobView game;
      std::vector<NavMeshData> game_meshes;
      bool same_grid = !j.is_discarded() && read_navmesh_blob(j, &game) && navmeshes_of_blob(game, &game_meshes) &&
                       !game_meshes.empty();
      // every mesh of the level (the roots are in the order navmeshes_of_blob reads them); what
      // differs is said
      std::vector<int> roots;
      for (const auto& [name, ent] : game.roots)
        if (name.rfind("nav-mesh-", 0) == 0) roots.push_back(ent);
      for (size_t m = 0; same_grid && m < game_meshes.size(); m++) {
        // what the editor asks of its own meshes, the game's do not always have (a polygon facing
        // down, a mesh in several pieces): said, not a failure
        std::string note = navmesh_problem(game_meshes[m]);
        if (note.empty() && !route_reaches_all(game_meshes[m])) note = "its route does not lead everywhere";
        if (!note.empty()) std::printf("    nav mesh %zu of %zu, the game's own: %s\n", m, game_meshes.size(), note.c_str());
        std::string why;
        {
          const NavGrid g = navmesh_grid(game_meshes[m]);
          const int hash = game.ptr(game.ptr(roots[m] + 48) + 16);
          const int buckets = game.ptr(hash - 4 + 32);
          const int dims[3] = {game.u8(hash - 4 + 28), game.u8(hash - 4 + 29), game.u8(hash - 4 + 30)};
          if (dims[0] != g.dims[0] || dims[1] != g.dims[1] || dims[2] != g.dims[2])
            why = strf("grid %dx%dx%d, the game's %dx%dx%d", g.dims[0], g.dims[1], g.dims[2], dims[0], dims[1], dims[2]);
          for (size_t i = 0; why.empty() && i < g.buckets.size(); i++)
            if (game.u8(buckets + (int)i) != g.buckets[i]) why = strf("grid byte %zu differs", i);
        }
        if (!why.empty()) {
          std::printf("    nav mesh %zu of %zu (%zu polygons): %s\n", m, game_meshes.size(),
                      game_meshes[m].polys.size(), why.c_str());
          same_grid = false;
        }
      }
      check(same_grid, "nav mesh: the game's own meshes read back, the same grid-hash for each");
    }
  }

  // the scale gizmo, driven like the mouse would: an axis stretches along it, the center scales
  // the three, snap by steps of 0.1
  {
    Camera cam;
    cam.pos = {0, 3, 12};
    cam.look_at({0, 0, 0});
    Renderer r;  // not initialized (no GL): only its view, for project()
    r.set_view(cam, 0, 0, 800, 600);
    Gizmo g;
    g.op = GizmoOp::Scale;
    const float size = length(cam.pos) * std::tan(radians(cam.fov_deg) * 0.5f) * 0.24f;  // the gizmo's
    auto at = [&](const Vec3& p) {
      Vec2 v;
      r.project(p, &v);
      return v;
    };
    auto step = [&](const Vec2& mouse, bool pressed, bool down) {
      GizmoInput in;
      in.mouse = mouse;
      in.viewport_size = {800, 600};
      in.mouse_in_viewport = true;
      in.pressed = pressed;
      in.down = down;
      in.released = !down;
      return g.update(cam, r, in, Mat4::identity(), nullptr);
    };
    // the X axis grabbed at 60% of its length and pulled to 160%: twice as long
    const GizmoResult a = step(at({size * 0.6f, 0, 0}), true, true);
    const GizmoResult b = step(at({size * 1.6f, 0, 0}), false, true);
    const GizmoResult c = step(at({size * 1.6f, 0, 0}), false, false);
    const bool axis = a.started && b.dragging && std::fabs(b.scale.x - 2.f) < 0.01f && b.scale.y == 1.f &&
                      b.scale.z == 1.f && c.finished;
    // the center, dragged 50 px right and 50 px up: e times bigger
    const Vec2 o = at({0, 0, 0});
    const GizmoResult d = step(o, true, true);
    const GizmoResult e = step(Vec2{o.x + 50, o.y - 50}, false, true);
    step(Vec2{o.x + 50, o.y - 50}, false, false);
    const bool center = d.started && std::fabs(e.scale.x - std::exp(1.f)) < 1e-3f && e.scale.y == e.scale.x &&
                        e.scale.z == e.scale.x;
    // snapped: 1.77 becomes 1.8
    g.snap.enabled = true;
    step(at({size * 0.6f, 0, 0}), true, true);
    const GizmoResult f = step(at({size * 1.37f, 0, 0}), false, true);
    step(at({size * 1.37f, 0, 0}), false, false);
    check(axis && center && std::fabs(f.scale.x - 1.8f) < 1e-4f,
          "scale gizmo: an axis stretches along it, the center scales the three, snap by 0.1");

    // the move gizmo: Shift ten times finer, the snap putting the pivot on the grid (world), Ctrl
    // snapping while the snap is off
    g = Gizmo{};
    g.op = GizmoOp::Translate;
    const Mat4 pivot = Mat4::translate({0.3f, 0, 0});
    auto move = [&](const Vec2& mouse, bool pressed, bool down, bool fine, bool ctrl) {
      GizmoInput in;
      in.mouse = mouse;
      in.viewport_size = {800, 600};
      in.mouse_in_viewport = true;
      in.pressed = pressed;
      in.down = down;
      in.fine = fine;
      in.snap_toggle = ctrl;
      return g.update(cam, r, in, pivot, nullptr);
    };
    auto dx = [](const GizmoResult& res) { return res.delta.col3(3).x; };
    move(at({0.3f + size * 0.6f, 0, 0}), true, true, false, false);
    const float plain = dx(move(at({0.3f + size * 0.6f + 2.f, 0, 0}), false, true, false, false));
    move(at({0.3f + size * 0.6f + 2.f, 0, 0}), false, false, false, false);
    move(at({0.3f + size * 0.6f, 0, 0}), true, true, true, false);
    const float fine = dx(move(at({0.3f + size * 0.6f + 2.f, 0, 0}), false, true, true, false));
    move(at({0.3f + size * 0.6f + 2.f, 0, 0}), false, false, true, false);
    move(at({0.3f + size * 0.6f, 0, 0}), true, true, false, true);
    const float snapped = dx(move(at({0.3f + size * 0.6f + 1.1f, 0, 0}), false, true, false, true));
    move(at({0.3f + size * 0.6f + 1.1f, 0, 0}), false, false, false, true);
    check(std::fabs(plain - 2.f) < 0.02f && std::fabs(fine - 0.2f) < 0.01f && std::fabs(0.3f + snapped - 1.5f) < 1e-4f,
          "move gizmo: Shift ten times finer, Ctrl snaps the pivot to the grid (0.3 + 1.1 m lands on 1.5 m)");
  }

  // a large selection: the descendants of thousands of roots in one pass, right and fast; the
  // ground of a level locked, not its large walls; the glows do not take the click
  {
    Scene s;
    std::vector<uint32_t> groups, leaves;
    for (int g = 0; g < 1000; g++) {
      Node group{};
      group.name = "g" + std::to_string(g);
      const uint32_t gid = s.add(group);
      groups.push_back(gid);
      for (int k = 0; k < 99; k++) {
        Node leaf{};
        leaf.parent = gid;
        leaf.kind = NodeKind::Render;
        leaves.push_back(s.add(leaf));
      }
    }
    std::vector<uint32_t> roots(groups.begin(), groups.begin() + 500);
    roots.push_back(leaves[700 * 99]);  // a leaf of a group not among the roots
    const auto t0 = std::chrono::steady_clock::now();
    const auto below = s.descendants_of(roots);
    std::vector<uint32_t> tree;
    for (size_t g = 0; g < 1000; g++) {
      tree.clear();
      s.subtree((size_t)s.index_of(groups[g]), tree);
    }
    const double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
    bool right = below.size() == 501 && below[500].empty();
    for (size_t r = 0; r < 500 && right; r++) right = below[r].size() == 99 && s.find(below[r][0])->parent == groups[r];
    check(right && tree.size() == 100 && ms < 500.0,
          strf("large selection: the descendants of 500 groups of a 100,000-node scene in one pass (%.0f ms)", ms).c_str());

    auto quad = [](float w, float h, bool upright) {
      auto mesh = std::make_shared<Mesh>();
      Primitive p;
      p.pos = upright ? std::vector<Vec3>{{0, 0, 0}, {w, 0, 0}, {w, h, 0}, {0, h, 0}}
                      : std::vector<Vec3>{{0, 0, 0}, {w, 0, 0}, {w, 0, h}, {0, 0, h}};
      p.idx = {0, 1, 2, 0, 2, 3};
      mesh->prims.push_back(p);
      return mesh;
    };
    check(ground_piece(*quad(100, 100, false)) && !ground_piece(*quad(81, 30, true)) && !ground_piece(*quad(20, 20, false)),
          "level pieces: a large flat piece is ground (locked), a facade as wide is not, nor a small slab");

    // a ground the game made one with a 20 m wall standing on it: the wall becomes a piece of its own;
    // a hill stays whole
    {
      Scene gs;
      Node yard{};
      yard.kind = NodeKind::Render;
      yard.name = "yard";
      yard.mesh = quad(100, 100, false);
      Primitive wall = quad(60, 20, true)->prims[0];
      for (Vec3& p : wall.pos) p.z += 50;
      yard.mesh->prims.push_back(wall);
      gs.add(yard);
      Node hill{};
      hill.kind = NodeKind::Render;
      hill.mesh = quad(100, 100, false);
      for (Vec3& p : hill.mesh->prims[0].pos) p.y = p.z * 0.2f;
      gs.add(hill);
      const int walls = split_ground_piece(gs, 0);
      const bool split = walls == 2 && gs.nodes[2].name == "yard-walls" && gs.nodes[0].mesh->prims.size() == 1 &&
                         gs.nodes[2].mesh->prims.size() == 1 && gs.nodes[2].mesh->prims[0].pos[2].y == 20.f &&
                         ground_piece(*gs.nodes[0].mesh) && !ground_piece(*gs.nodes[2].mesh);
      check(split && split_ground_piece(gs, 1) < 0 && gs.nodes.size() == 3,
            "level pieces: a wall the game merged with the ground is split off (selectable), a hill is not cut");
    }

    // a box behind a glow (an additive light beam): the click goes to the box, a second click to the glow
    Scene ps;
    Material glow_mat;
    glow_mat.alpha = AlphaMode::Blend;
    glow_mat.blend = BlendMode::Add;
    const int glow_material = ps.add_material(glow_mat);
    Node box{};
    box.kind = NodeKind::Render;
    box.mesh = make_box("box", -1);
    const uint32_t box_id = ps.add(box);
    Node beam{};
    beam.kind = NodeKind::Render;
    beam.mesh = quad(4, 4, true);
    beam.mesh->prims[0].material = glow_material;
    beam.local.t = {-2, 0, 3};
    const uint32_t beam_id = ps.add(beam);
    ps.begin_frame();
    const Ray ray{{0, 1, 10}, {0, 0, -1}};
    const PickHit hit = pick(ps, ray, PickFilter{});
    const auto all = pick_all(ps, ray, PickFilter{});
    const PickHit beam_alone = pick(ps, Ray{{1.5f, 3.5f, 10}, {0, 0, -1}}, PickFilter{});
    check(hit.node == box_id && all.size() == 2 && all[0].node == box_id && all[1].node == beam_id &&
              beam_alone.node == beam_id,
          "picking: a glow in front does not take the click, a second click reaches it, alone it is picked");
  }

  // the ocean: no water where the land of the levels around hides it in the game (a slab above it,
  // not one under it nor a wall); of a view from afar, the one placed like the document's levels,
  // as far as their land goes; not under thin land, nor in the document's own box
  {
    OceanMap map;
    map.cell = 96;
    map.size = 2;  // 64 squares of 3 m a side
    auto slab = [](float x0, float x1, float z0, float z1, float y) {
      Primitive p;
      p.pos = {{x0, y, z0}, {x1, y, z0}, {x1, y, z1}, {x0, y, z1}};
      p.idx = {0, 1, 2, 0, 2, 3};
      return p;
    };
    Scene land;
    Node piece{};
    piece.kind = NodeKind::Render;
    piece.mesh = std::make_shared<Mesh>();
    piece.mesh->prims = {slab(30, 60, 30, 60, 2), slab(90, 120, 90, 120, -1)};
    Primitive wall;
    wall.pos = {{0, 0, 150}, {90, 0, 150}, {90, 20, 150}, {0, 20, 150}};
    wall.idx = {0, 1, 2, 0, 2, 3};
    piece.mesh->prims.push_back(wall);
    land.add(piece);
    const auto mask = ocean_land(map, 0.f, land);
    size_t covered = 0;
    for (uint8_t v : mask) covered += v;
    check(mask.size() == 64 * 64 && covered == 100 && mask[15 * 64 + 15] && !mask[35 * 64 + 35],
          "ocean: the squares under a slab above the water are land (not under one below it, nor a wall)");

    const int side = 64;
    auto block = [&](std::vector<uint8_t>& m, int x0, int x1, int z0, int z1) {
      if (m.empty()) m.assign(side * side, 0);
      for (int z = z0; z <= z1; z++)
        for (int x = x0; x <= x1; x++) m[z * side + x] = 1;
    };
    std::vector<uint8_t> doc, placed, moved, around;
    block(doc, 20, 29, 20, 29);
    block(placed, 20, 29, 20, 29);
    block(placed, 30, 63, 0, 63);   // the city on, from the document's land
    block(placed, 0, 9, 50, 59);    // an island
    block(moved, 40, 49, 40, 49);
    block(moved, 0, 15, 30, 45);    // land of a model moved away: not taken
    block(around, 0, 15, 0, 15);    // a neighbor
    block(around, 0, 19, 47, 47);   // a wall in the sea, one square thick
    const auto out = ocean_land_around(side, {doc}, {around}, {moved, placed});
    auto at = [&](int x, int z) { return out[z * side + x] != 0; };
    check(at(40, 10) && at(8, 8) && !at(25, 25) && !at(5, 55) && !at(10, 47) && !at(8, 38),
          "ocean: the city seen from afar hides it beyond the level (not an island, not a moved model), "
          "a neighbor too, not a thin wall nor the level's own box");
    // a pond the city closes in (water of the map under it) goes; the sea open to the edge, the water
    // of the level's own box stay
    std::vector<uint8_t> pond_city, all_water(side * side, 1);
    block(pond_city, 20, 29, 20, 29);
    block(pond_city, 30, 63, 0, 63);
    for (int z = 20; z <= 29; z++)
      for (int x = 45; x <= 54; x++) pond_city[z * side + x] = 0;  // a pond 30 m across in the city
    const auto open = ocean_land_around(side, {doc}, {}, {pond_city});
    const auto closed = ocean_land_around(side, {doc}, {}, {pond_city}, all_water);
    auto shut = [&](int x, int z) { return closed[z * side + x] != 0; };
    check(!open[25 * side + 50] && shut(50, 25) && shut(40, 10) && !shut(10, 30) && !shut(25, 25) && !shut(5, 55),
          "ocean: the water the land around closes in is left out, the sea open to the edge and the level's own kept");
  }

  // the remote control of the MCP server: a request and its answer, one line each
  {
    RemoteServer server;
    RemoteClient client;
    std::string err_remote;
    const int port = 47899;
    bool round_trip = false;
    if (server.start(port, &err_remote) && client.connect(port, &err_remote)) {
      std::thread answer([&] {
        for (int i = 0; i < 200; i++) {
          auto requests = server.take();
          if (!requests.empty()) {
            const RemoteRequest& r = requests[0];
            server.reply(r.client, json{{"id", r.id}, {"ok", true}, {"result", {{"tool", r.tool}, {"x", r.args.value("x", 0)}}}});
            return;
          }
          std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
      });
      json reply;
      round_trip = client.call(json{{"id", 7}, {"tool", "echo"}, {"args", {{"x", 42}}}}, &reply, 5000, &err_remote) &&
                   reply.value("id", 0) == 7 && reply["result"].value("tool", std::string()) == "echo" &&
                   reply["result"].value("x", 0) == 42;
      answer.join();
    }
    const uint8_t bytes[] = {'M', 'a', 'n'};
    check(round_trip && base64_encode(bytes, 3) == "TWFu" && base64_encode(bytes, 2) == "TWE=",
          "remote control: a request and its answer over the local connection");
  }

  // the export for the game: surfaces, data of actors, types, and a small level
  {
    std::string loss;
    const BuilderSurface wall = builder_surface(0x5cc8, &loss);  // stone wall, nopilot
    const bool wall_ok = wall.material == 0 && wall.mode == 1 && wall.event == 0 && loss == "nopilot";
    builder_surface((28u << 10) | (13u << 18), &loss);  // hdwood, rail
    const bool rail_lost = loss == "rail";
    const BuilderSurface hdwood = builder_surface(28u << 10, &loss);
    const bool hdwood_ok = hdwood.material == 6 && loss == "hdwood";
    const BuilderSurface wood = builder_surface((6u << 10) | (1u << 16) | 4u, &loss);  // wood, nolos, noedge
    const bool wood_ok = wood.material == 6 && wood.nolineofsight && wood.noedge && loss.empty();
    check(wall_ok && rail_lost && hdwood_ok && wood_ok,
          "export: collision surfaces as the builder takes them (Jak 1's fields), what is lost said");

    const bool lumps_ok =
        builder_lump_value("abc", "string") == json("abc") && builder_lump_value("'steel", "symbol") == json("'steel") &&
        builder_lump_value(json::array({"'a", "'b"}), "symbol") == json::array({"symbol", "a", "b"}) &&
        builder_lump_value(1.5, "float") == json::array({"float", 1.5}) &&
        builder_lump_value(json::array({3, 4}), "uint8") == json::array({"uint32", 3, 4}) &&
        builder_lump_value(json::array({1, 2, 3, 4}), "vector") == json::array({"vector", json::array({1.0, 2.0, 3.0, 4.0})}) &&
        builder_lump_value("'citb-bunny", "type") == json::array({"type", "citb-bunny"}) &&
        builder_lump_value("(want-load 'a)", "pair") == json::array({"pair", "(want-load 'a)"}) &&
        builder_lump_value(3, "") == json::array({"int32", 3}) && builder_lump_value(json::array({1}), "actor-group").is_null();
    check(lumps_ok, "export: the data of actors as the builder reads them (typed, symbols, vectors, scripts)");

    GameData data;
    const bool read = data.load(game_data_path(), &err);
    check(read && data.has_type("crate") && data.has_type("cty-door") && !data.has_type("cty-fruit-stand"),
          "export: the game's actor types (a placeholder is not one)");

    // what configures the objects: the lumps their code reads, what they are, where their code is
    {
      const auto* vent = read ? data.params("industrial-board-vent") : nullptr;
      bool height = false;
      if (vent)
        for (const auto& p : *vent) height |= p.tag == "spring-height" && p.kind == "float" && p.def == 45056.0;
      const auto* door = read ? data.params("cty-door") : nullptr;
      bool notice = false, options = false;
      if (door)
        for (const auto& p : *door) {
          notice |= p.tag == "on-notice" && p.kind == "script";
          options |= p.tag == "options" && p.kind == "enum:airlock-options";
        }
      const auto* opts = read ? data.enumeration("airlock-options") : nullptr;
      check(read && height && notice && options && opts && opts->bitfield &&
                data.derives_from("industrial-board-vent", "bouncer") && data.derives_from("cty-door", "com-airlock") &&
                !data.derives_from("crate", "com-airlock") &&
                data.code_files("industrial-board-vent") ==
                    std::vector<std::string>{"elec-gate", "ctyinda-part", "ctyinda-obs"} &&
                data.code_files("cty-door").empty() && behavior_kind(&data, "wascity-awning-a") == BehaviorKind::Bouncer &&
                data.effect_code_files("group-ctyindb-red-floor-vent").back() == "ctyindb-part",
            "game data: the parameters of a type (lumps its code reads), what it derives from, the code it needs");
    }

    // the behaviors as the game's code
    {
      const Condition task = condition_of(json{{"when", "task"}, {"arg", "arena-fight-1-resolution"}});
      const Condition none = condition_of(json{{"when", "pressed"}, {"arg", "\"; (evil)"}});
      auto has = [](const std::string& code, std::initializer_list<const char*> parts) {
        for (const char* p : parts)
          if (code.find(p) == std::string::npos) return false;
        return true;
      };
      const std::string door = behavior_code(&data, "cty-door", json{{"open", "approach"}}, "lv-door", "");
      const std::string locked = behavior_code(&data, "cty-door", json{{"open", "locked"}}, "lv-door", "");
      const std::string always = behavior_code(&data, "cty-door", json{{"open", "always"}}, "lv-door", "");
      const std::string when = behavior_code(&data, "cty-door", json{{"open", "when"}, {"when", condition_json(task)}, {"distance", 8}},
                                             "lv-door", "the door");
      const std::string vent = behavior_code(&data, "industrial-board-vent", json{{"active", {{"when", "never"}}}}, "lv-vent", "");
      const std::string elev = behavior_code(&data, "elevator", json{{"leave", condition_json(task)}}, "lv-elev", "");
      const std::string there = behavior_code(&data, "crate", json{{"present", {{"when", "gone"}, {"arg", "crate-9"}}}}, "lv-crate", "");
      const std::string plain = behavior_code(&data, "crate", json::object(), "lv-crate", "");
      // a door loading the rooms behind it (a level of their own): wanted with the others, shown,
      // the door waiting for it (no "#t")
      const std::string rooms = behavior_code(&data, "hip-door-a", json{{"open", "approach"}, {"loads", "My Rooms!"}}, "lv-home", "");
      const bool loads = has(rooms, {"(set! (-> this level-name) '(quote (myrooms)))", "(want-levels *load-state* (-> names data))",
                                     "(want-display-level *load-state* 'myrooms 'display)", "(call-parent-method this arg0)"}) &&
                         rooms.find("  #t\n") == std::string::npos;
      check(task.kind == Condition::Task && condition_script(task) == "(task-closed? \"arena-fight-1-resolution\")" &&
                condition_script(none) == "(entity-status? \"evil\" subtask-complete)" &&
                condition_name(condition_of(condition_json(task)).kind) == std::string("task") &&
                has(door, {"(deftype lv-door (cty-door) ())", "(defmethod destination-loaded? ((this lv-door) (arg0 symbol))"}) &&
                locked.empty() && has(always, {"(set! (-> this open-test) '(not #f))", "(meters 1000.00)"}) &&
                has(when, {";; the door", "(task-closed? \"arena-fight-1-resolution\")", "(meters 8.00)", "(meters 43.00)",
                           "(call-parent-method this)"}) &&
                has(vent, {"(defstate idle (lv-vent)", "(when #f", "call-parent-state-handler event"}) &&
                has(elev, {"(defstate waiting (lv-elev)", "(set! (-> self activate-test) '(task-closed?", "call-parent-state-handler enter"}) &&
                has(there, {"(defmethod init-from-entity! ((this lv-crate) (arg0 entity-actor))", "(entity-status? \"crate-9\" dead)",
                            "(deactivate this)"}) &&
                plain.empty() && behavior_type_name("my-level", "Cty Door_1") == "my-level-cty-door-1" && loads,
            "behaviors: doors, bouncers, elevators and presence as the game's code, conditions safe in a script, a door "
            "loading the rooms behind it");
    }

    // a small level: its shell with the game's collision, a box placed, actors
    Scene s;
    s.settings.level = "testlvl";
    Material m;
    m.name = "stone";
    // blended with its texture's alpha, as the levels' materials nearly all are: still a wall
    m.alpha = AlphaMode::Blend;
    m.base_color = {2, 2, 2, 2};  // a level's (the PS2's colors)
    const int mat = s.add_material(m);
    Material glass_mat = m;
    glass_mat.name = "glass";
    glass_mat.translucent = true;  // drawn by the game without writing its depth
    const int glass = s.add_material(glass_mat);
    Node tfrag;
    tfrag.name = "testlvl-tfrag-normal";
    const uint32_t tfrag_id = s.add(tfrag);
    Node floor_node;
    floor_node.name = "testlvl-floor";
    floor_node.kind = NodeKind::Render;
    floor_node.parent = tfrag_id;
    floor_node.mesh = make_floor(mat);
    {
      // its lighting in the game: one color, each of its 8 palettes recognizable
      auto pal = std::make_shared<TodPalette>();
      for (int k = 0; k < 8; k++) pal->rgba.insert(pal->rgba.end(), {(uint8_t)(10 + k), (uint8_t)(20 + k), (uint8_t)(30 + k), 128});
      auto tod = std::make_shared<TodColors>();
      tod->palette = pal;
      tod->index = {std::vector<uint16_t>(floor_node.mesh->prims[0].pos.size(), 0)};
      floor_node.tod = tod;
    }
    const uint32_t floor_id = s.add(floor_node);
    Node col_group;
    col_group.name = "testlvl-collision";
    const uint32_t col_group_id = s.add(col_group);
    Node col;
    col.name = "testlvl-collision-0";
    col.kind = NodeKind::Collision;
    col.parent = col_group_id;
    col.mesh = make_floor(-1);
    col.mesh->prims[0].tri_pat = {0x5cc8u, 28u << 10};
    s.add(col);
    Node box;
    box.name = "box-1";
    box.kind = NodeKind::Render;
    box.mesh = make_box("box", mat);
    box.local.t = {10, 0, 0};
    box.local.s = {3, 3, 3};  // 6 m: a piece that collides by its shape
    const uint32_t box_id = s.add(box);
    Node window;  // see through: the builder's translucent tree
    window.name = "glass-1";
    window.kind = NodeKind::Render;
    window.mesh = make_box("glass", glass);
    window.local.t = {30, 0, 0};
    s.add(window);
    Node lamp;  // 2 m: a detail, without collision
    lamp.name = "lamp-1";
    lamp.kind = NodeKind::Render;
    lamp.mesh = make_box("lamp", mat);
    lamp.local.t = {20, 0, 0};
    s.add(lamp);
    auto actor = [&](const std::string& name, const std::string& etype, uint32_t aid, json lump, json types,
                     bool visible = true) {
      Node a;
      a.name = name;
      a.kind = NodeKind::Actor;
      a.visible = visible;
      a.local.t = {5, 1, 5};
      lump["name"] = name;
      a.extras = json{{"etype", etype}, {"aid", aid}, {"game_task", 7}, {"bsphere", {5, 1, 5, 2}},
                      {"lump", lump}, {"lump_types", types}};
      s.add(a);
    };
    actor("crate-1", "crate", 101,
          {{"eco-info", {9, 5}}, {"kill-mask", 16384}, {"task-actor", 3}, {"next-actor", 102},
           {"visvol", {{0, 0, 0, 1}, {1, 1, 1, 1}}}},
          {{"eco-info", "int32"}, {"kill-mask", "uint32"}, {"task-actor", "uint8"}, {"next-actor", "uint32"},
           {"visvol", "vector"}, {"name", "string"}});
    actor("cty-door-1", "cty-door", 102, {{"on-activate", "(want-load 'x)"}}, {{"on-activate", "pair"}});
    actor("cty-fruit-stand-1", "cty-fruit-stand", 103, json::object(), json::object());
    actor("crate-2", "crate", 104, json::object(), json::object(), false);
    actor("switch-a", "sew-floor-switch", 105, json::object(), json::object());
    actor("vent-1", "industrial-board-vent", 106, {{"spring-height", 40960.0}}, {{"spring-height", "float"}});
    actor("ghost-1", "crate", 107, json::object(), json::object());
    actor("plat-1", "plat", 108, {{"path", {{0, 0, 0, 1}, {40960, 0, 0, 1}}}}, {{"path", "vector"}});
    for (auto& a : s.nodes) {
      if (a.name == "cty-door-1")
        a.extras["og_behavior"] = {{"open", "when"}, {"when", {{"when", "pressed"}, {"arg", "switch-a"}}}, {"distance", 12}};
      if (a.name == "vent-1")
        a.extras["og_behavior"] = {{"active", {{"when", "task"}, {"arg", "arena-fight-1-resolution"}}}};
      if (a.name == "ghost-1") a.extras["og_behavior"] = {{"present", {{"when", "never"}}}};
      if (a.name == "crate-1") {
        a.extras["lump"]["alt-actor"] = "switch-a";
        a.extras["lump_types"]["alt-actor"] = "actor-name";
      }
      if (a.name == "plat-1") {
        // found at (5, 1, 5) in the game, moved 10 m along x since
        a.extras["og_origin"] = {5, 1, 5, 0, 0, 0, 1};
        a.local.t = {15, 1, 5};
      }
    }
    s.begin_frame();

    const fs::path out = tmp / "export";
    LevelExportOptions o;
    o.name = "my-test";
    o.nickname = default_nickname(o.name);
    o.iso_name = default_iso_name(o.name);
    o.folder = out.generic_string();
    o.start = {1, 2, 3};
    LevelExportReport r;
    const bool exported = read && check_export_names(o).empty() && export_level_for_game(s, &data, o, &r, &err);
    // laid out like the jak-project checkout: the level, its code, the README at the top
    const fs::path dir = out / "custom_assets" / "jak3" / "levels" / "my-test";
    const fs::path code = out / "goal_src" / "jak3" / "levels" / "my-test" / "my-test-obs.gc";
    bool files = exported && r.files.size() == 6 && r.behaviors == 2 && fs::is_regular_file(out / "README.md", ec) &&
                 fs::is_regular_file(code, ec) && fs::path(r.level_dir) == dir;
    for (const char* f : {"my-test.glb", "my-test.jsonc", "my-test.gd", "level-info.gc"})
      files &= fs::is_regular_file(dir / f, ec);
    const json level = read_jsonc_file(dir / "my-test.jsonc");
    std::map<std::string, const json*> by_name;
    if (level.is_object() && level.contains("actors"))
      for (const auto& a : level["actors"]) by_name[a["lump"].value("name", std::string())] = &a;
    const json* crate_j = by_name.count("crate-1") ? by_name["crate-1"] : nullptr;
    const json* door_j = by_name.count("cty-door-1") ? by_name["cty-door-1"] : nullptr;
    const json* vent_j = by_name.count("vent-1") ? by_name["vent-1"] : nullptr;
    const json* plat_j = by_name.count("plat-1") ? by_name["plat-1"] : nullptr;
    const bool actors_ok =
        level.is_object() && level["actors"].size() == 5 && crate_j && door_j && vent_j && plat_j &&
        !by_name.count("ghost-1") && (*crate_j)["etype"] == "crate" &&
        (*crate_j)["lump"]["eco-info"] == json::array({"int32", 9, 5}) &&
        (*crate_j)["lump"]["next-actor"] == json::array({"string", "cty-door-1"}) &&
        (*crate_j)["lump"]["alt-actor"] == json::array({"string", "switch-a"}) &&
        (*crate_j)["lump"]["task-actor"] == json::array({"uint32", 3}) && (*crate_j).value("game_task", -1) == 7 &&
        !(*crate_j)["lump"].contains("kill-mask") && !(*crate_j)["lump"].contains("visvol") &&
        (*door_j)["etype"] == "my-test-cty-door-1" && !(*door_j)["lump"].contains("on-activate") &&
        (*vent_j)["etype"] == "my-test-vent-1" && (*vent_j)["lump"]["spring-height"] == json::array({"float", 40960.0}) &&
        (*plat_j)["etype"] == "plat" &&
        (*plat_j)["lump"]["path"] == json::array({"vector", {40960.0, 0.0, 0.0, 1.0}, {81920.0, 0.0, 0.0, 1.0}}) &&
        level.value("automatic_wall_detection", false) && level.value("nickname", std::string()) == "mte" &&
        level.value("base_id", 0) == (int)kDefaultBaseId;
    check(files && actors_ok,
          "export: the files, links by name, a path moved with its actor, the data kept, the scripts, the kill "
          "mask and visibility box left out, an object never there left out");
    // the level's code and the code of the game it needs, in its DGO
    const std::string gc = read_text_file(code), gd = read_text_file(dir / "my-test.gd"),
                      install = read_text_file(out / "README.md");
    const size_t at_part = gd.find("\"ctyinda-part.o\""), at_obs = gd.find("\"ctyinda-obs.o\""),
                 at_own = gd.find("\"my-test-obs.o\""), at_level = gd.find("\"my-test.go\"");
    check(gc.find("(in-package goal)") != std::string::npos &&
              gc.find("(deftype my-test-cty-door-1 (cty-door) ())") != std::string::npos &&
              gc.find("(entity-status? \"switch-a\" subtask-complete)") != std::string::npos &&
              gc.find("(deftype my-test-vent-1 (industrial-board-vent) ())") != std::string::npos &&
              gd.find("\"sewer-obs.o\"") != std::string::npos && at_part != std::string::npos &&
              at_part < at_obs && at_obs < at_own && at_own < at_level &&
              install.find("(goal-src \"levels/my-test/my-test-obs.gc\" \"entity-debug\"") != std::string::npos &&
              install.find("(get-continue-by-name *game-info* \"my-test-start\")") != std::string::npos,
          "export: the behaviors as the level's code, its DGO with the game's code its objects need");
    const json glb = read_glb_json(dir / "my-test.glb");
    size_t surfaces = 0;
    bool floor_ignored = false, box_collides = false, box_double = false, lamp_ignored = false;
    if (glb.is_object())
      for (const auto& n : glb["nodes"]) {
        const json e = n.value("extras", json::object());
        if (e.value("set_invisible", 0) == 1) surfaces++;
        if (n.value("name", std::string()) == "testlvl-floor") floor_ignored = e.value("ignore", 0) == 1;
        if (n.value("name", std::string()) == "box-1") {
          box_collides = !e.contains("set_collision");
          box_double = e.value("double_sided", 0) == 1;
        }
        if (n.value("name", std::string()) == "lamp-1") lamp_ignored = e.value("ignore", 0) == 1;
      }
    std::set<std::string> keys;
    for (const auto& note : r.notes) keys.insert(note.key);
    const bool notes_ok = keys.count("export.note.types_missing") && keys.count("export.note.surfaces") &&
                          keys.count("export.note.kill_masks") && keys.count("export.note.hidden") &&
                          keys.count("export.note.scripts_dropped") && keys.count("export.note.lumps_widened") &&
                          keys.count("export.note.absent") && keys.count("export.note.collision_small") &&
                          r.collision_triangles == 2 && r.collision_from_decor == 1 && r.dense_places == 0;
    check(surfaces == 2 && floor_ignored && box_collides && lamp_ignored && notes_ok,
          "export: the game's collision one node per surface, the shell not colliding twice, a 6 m box by its "
          "shape, a 2 m lamp without collision");
    // a level's blended walls (their depth written) cut out in the builder's solid tree; the decor
    // faces either way, its collision holds from both sides (the game's own stays one-sided)
    bool walls_cut = glb.is_object() && glb.contains("materials"), glass_blended = false;
    if (walls_cut)
      for (const auto& m : glb["materials"]) {
        const bool blend = m.value("alphaMode", std::string()) == "BLEND";
        if (m.value("name", std::string()) == "glass")
          glass_blended = blend && m.value("extras", json::object()).value("og_blend", std::string()) == "alpha";
        else
          walls_cut &= !blend;
      }
    check(walls_cut && glass_blended && box_double && !level.value("double_sided_collide", true),
          "export: a level's blended walls cut out, its glass in the builder's translucent tree (og_blend), the "
          "decor's collision from both sides");
    // the game's lighting: every mesh with the 8 palettes the builder reads, a node's own as it has
    // them (the floor's), the new geometry's made from its color
    {
      bool all_lit = glb.contains("meshes"), floor_exact = false;
      for (const auto& n : glb.value("nodes", json::array())) {
        // (the game's collision is not drawn)
        if (!n.contains("mesh") || n.value("extras", json::object()).value("set_invisible", 0)) continue;
        for (const auto& p : glb["meshes"][n["mesh"].get<int>()]["primitives"])
          for (const char* k : {"_SUNRISE", "_MORNING", "_NOON", "_AFTERNOON", "_SUNSET", "_TWILIGHT", "_EVENING", "_GREENSUN"})
            all_lit &= p["attributes"].contains(k);
      }
      for (const auto& n : glb.value("nodes", json::array()))
        if (n.value("name", std::string()) == "testlvl-floor" && n.contains("mesh")) {
          const json& p = glb["meshes"][n["mesh"].get<int>()]["primitives"][0];
          const auto noon = glb_accessor_bytes(dir / "my-test.glb", glb, p["attributes"]["_NOON"].get<int>());
          floor_exact = noon.size() >= 4 && noon[0] == 12 && noon[1] == 22 && noon[2] == 32 && noon[3] == 255;
        }
      if (!all_lit || !floor_exact) std::printf("    all lit %d, the floor's exact %d\n", (int)all_lit, (int)floor_exact);
      check(all_lit && floor_exact, "export: the 8 palettes of the game's lighting on every mesh, a piece's own exactly");
    }
    (void)floor_id;
    (void)box_id;

    // the same level in place of ctyindb: a level of the game next to it and the original shown for
    // reference are left out; ground under ctyindb-start only
    {
      LevelExportOptions bad = o;
      bad.name = "ctyindb";
      const bool name_refused = check_export_names(bad, &data) == "export.bad.name_taken";
      bad = o;
      bad.nickname = "cib";
      const bool nick_refused = check_export_names(bad, &data) == "export.bad.nickname_taken";
      bad = o;
      bad.replaces = "no-such-level";
      const bool replaces_refused = check_export_names(bad, &data) == "export.bad.replaces";
      auto group = [&](const char* name, const char* key) {
        Node g;
        g.name = name;
        g.extras = json{{"og_level", name}};
        if (key) g.extras[key] = true;
        const uint32_t gid = s.add(g);
        Node b;
        b.name = std::string(name) + "-box";
        b.kind = NodeKind::Render;
        b.mesh = make_box("box", mat);
        b.parent = gid;
        s.add(b);
      };
      group("ctyinda", nullptr);
      group("ctyindb", "og_reference");
      Node ground;
      ground.name = "start-ground";
      ground.kind = NodeKind::Render;
      ground.mesh = make_floor(mat);
      ground.local.t = {971.42f, 7.f, 546.11f};
      s.add(ground);
      s.begin_frame();
      const auto checks = check_continues(s, &data, "ctyindb");
      bool start_ok = false;
      size_t falling = 0;
      for (const auto& c : checks) {
        if (c.name == "ctyindb-start") start_ok = c.ok() && std::fabs(c.ground - 7.f) < 1e-3f;
        else falling += !c.ok();
      }
      LevelExportOptions ro = o;
      ro.name = "my-ctyb";
      ro.nickname = "mcy";
      ro.replaces = "ctyindb";
      ro.folder = (tmp / "export-replace").generic_string();
      LevelExportReport rr;
      const bool replaced = check_export_names(ro, &data).empty() && export_level_for_game(s, &data, ro, &rr, &err);
      const fs::path rdir = tmp / "export-replace" / "custom_assets" / "jak3" / "levels" / "my-ctyb";
      const std::string info = read_text_file(rdir / "level-info.gc"), rgd = read_text_file(rdir / "my-ctyb.gd"),
                        rinstall = read_text_file(tmp / "export-replace" / "README.md");
      const json rglb = read_glb_json(rdir / "my-ctyb.glb");
      bool own_only = rglb.is_object(), ground_kept = false;
      if (own_only)
        for (const auto& n : rglb["nodes"]) {
          const std::string name = n.value("name", std::string());
          own_only &= name != "ctyinda-box" && name != "ctyindb-box";
          ground_kept |= name == "start-ground";
        }
      std::map<std::string, size_t> counts;
      for (const auto& note : rr.notes) counts[note.key] = note.count;
      const size_t at_part = rgd.find("\"ctyindb-part.o\""), at_obs = rgd.find("\"ctyindb-obs.o\"");
      // the floor of 100 m under ctyindb-start also holds ctyindb-hijack-vehicle; the three others fall
      check(name_refused && nick_refused && replaces_refused && start_ok && checks.size() == 5 && falling == 3 &&
                replaced && own_only && ground_kept && info.find("level-load-info") == std::string::npos &&
                info.find(":nickname 'cib") != std::string::npos && info.find(":nickname 'mcy") != std::string::npos &&
                at_part != std::string::npos && at_part < at_obs &&
                rinstall.find("(get-continue-by-name *game-info* \"ctyindb-start\")") != std::string::npos &&
                counts["export.note.replace_left_out"] == 1 && counts["export.note.continue_no_ground"] == falling,
            "export in place of a level of the game: its own level only, the replaced level's code in its DGO, the "
            "nickname to change, the continue points without ground said, a game's name refused");
    }

    // the decor's behaviors: a slab of deadly ice, a cable Jak goes through, a pad that bounces, a
    // ramp that launches
    {
      Scene d;
      auto piece = [&](const char* name, Vec3 at, json decor) {
        Node n;
        n.name = name;
        n.kind = NodeKind::Render;
        n.mesh = make_box(name, -1);
        n.local.t = at;
        n.extras = json{{"og_decor", decor}};
        d.add(n);
      };
      piece("ice-slab", {0, 0, 0}, {{"collision", {{"material", "ice"}, {"event", "deadly"}, {"noedge", true}}}});
      piece("cable", {5, 0, 0}, {{"collision", {{"kind", "none"}}}});
      piece("pad", {10, 0, 0}, {{"bounce", {{"height", 15}}}});
      piece("ramp", {15, 0, 0}, {{"launch", {{"distance", 30}, {"height", 5}, {"yaw", 90}, {"time", 1.5}}}});
      d.begin_frame();
      LevelExportOptions dop = o;
      dop.name = "my-decor";
      dop.nickname = "mdc";
      dop.folder = (tmp / "export-decor").generic_string();
      LevelExportReport dr;
      const bool done = export_level_for_game(d, &data, dop, &dr, &err);
      const fs::path dd = tmp / "export-decor" / "custom_assets" / "jak3" / "levels" / "my-decor";
      const json dglb = read_glb_json(dd / "my-decor.glb");
      std::map<std::string, json> extras;
      if (dglb.is_object())
        for (const auto& n : dglb["nodes"]) extras[n.value("name", std::string())] = n.value("extras", json::object());
      const json dlevel = read_jsonc_file(dd / "my-decor.jsonc");
      std::map<std::string, json> actors;
      if (dlevel.is_object())
        for (const auto& a : dlevel["actors"]) actors[a["lump"].value("name", std::string())] = a;
      const std::string dgc = read_text_file(tmp / "export-decor" / "goal_src" / "jak3" / "levels" / "my-decor" / "my-decor-obs.gc");
      const bool ice = extras["ice-slab"].value("collide_material", -1) == 1 && extras["ice-slab"].value("collide_event", -1) == 1 &&
                       extras["ice-slab"].value("noedge", 0) == 1;
      const bool through = extras["cable"].value("ignore", 0) == 1;
      const bool pad = actors.count("pad-bounce") && actors["pad-bounce"]["etype"] == "my-decor-pad-bounce" &&
                       actors["pad-bounce"]["lump"]["spring-height"] == json::array({"float", 15 * 4096.0}) &&
                       std::fabs(actors["pad-bounce"]["trans"][1].get<float>() - 2.f) < 1e-3f;
      const bool ramp = actors.count("ramp-launch") && actors["ramp-launch"]["etype"] == "my-decor-ramp-launch";
      // the ramp faces +z: turned 90 degrees, Jak lands toward +x, 30 m away, 5 m above its top
      const bool landing = dgc.find("(set-vector! (-> this landing) 184320.0 28672.0 0.0 1.0)") != std::string::npos;
      const bool code = dgc.find("(deftype my-decor-pad-bounce (bouncer) ())") != std::string::npos &&
                        dgc.find("(deftype my-decor-ramp-launch (process-drawable)") != std::string::npos &&
                        dgc.find("'launch-dir") != std::string::npos && dgc.find("450") != std::string::npos;
      check(done && ice && through && pad && ramp && landing && code && dr.behaviors == 2,
            "export: the decor's collision (deadly ice, none), a bounce and a launch as actors of the level's code");
    }

    // the game's collision follows its decor: a level as extracted (a floor, a wall, a block, their
    // collision), and the document made from it, the wall deleted and the block moved 30 m
    {
      auto level_scene = [&](bool edited) {
        auto sc = std::make_shared<Scene>();
        Node tfrag;
        tfrag.name = "lvx-tfrag-normal";
        const uint32_t tfrag_g = sc->add(tfrag);
        auto piece = [&](const char* name, std::shared_ptr<Mesh> mesh, Vec3 at, Vec3 scale) {
          Node n;
          n.name = name;
          n.kind = NodeKind::Render;
          n.parent = tfrag_g;
          n.mesh = mesh;
          n.local.t = at;
          n.local.s = scale;
          sc->add(n);
        };
        piece("lvx-floor", make_floor(-1), {0, 0, 0}, {1, 1, 1});
        if (!edited) piece("lvx-wall", make_box("wall", -1), {-20, 0, 0}, {5, 5, 0.5f});
        // rubble, deleted: its collision is coarser than it (a box 1.5 to 3 m off its faces)
        if (!edited) piece("lvx-rubble", make_box("rubble", -1), {0, 10, 20}, {2, 2, 2});
        piece("lvx-block", make_box("block", -1), edited ? Vec3{20, 0, 30} : Vec3{20, 0, 0}, {3, 3, 3});
        Node colg;
        colg.name = "lvx-collision";
        const uint32_t colg_id = sc->add(colg);
        // the game's collision: the floor, and the wall and the block as they were
        Node col;
        col.name = "lvx-collision-0";
        col.kind = NodeKind::Collision;
        col.parent = colg_id;
        col.mesh = make_floor(-1);
        Primitive& cp = col.mesh->prims[0];
        cp.uv.clear();
        auto add_box = [&](Vec3 at, Vec3 scale) {
          const Primitive b = make_box("c", -1)->prims[0];
          const uint32_t base = (uint32_t)cp.pos.size();
          for (size_t k = 0; k < b.pos.size(); k++) {
            cp.pos.push_back(Vec3{b.pos[k].x * scale.x, b.pos[k].y * scale.y, b.pos[k].z * scale.z} + at);
            cp.nrm.push_back(b.nrm[k]);
          }
          for (uint32_t v : b.idx) cp.idx.push_back(base + v);
        };
        add_box({-20, 0, 0}, {5, 5, 0.5f});
        add_box({20, 0, 0}, {3, 3, 3});
        add_box({0, 10, 20}, {5, 5, 5});
        cp.tri_pat.assign(cp.idx.size() / 3, 0);
        sc->add(col);
        sc->begin_frame();
        return sc;
      };
      auto original = level_scene(false);
      auto doc = level_scene(true);
      LevelExportOptions fo = o;
      fo.name = "my-follow";
      fo.nickname = "mfl";
      fo.folder = (tmp / "export-follow").generic_string();
      fo.originals["lvx"] = original;
      LevelExportReport fr;
      const bool done = export_level_for_game(*doc, &data, fo, &fr, &err);
      size_t taken_out = 0;
      for (const auto& note : fr.notes)
        if (note.key == "export.note.collision_taken_out") taken_out = note.count;
      std::map<std::string, json> extras;
      const json fglb =
          read_glb_json(tmp / "export-follow" / "custom_assets" / "jak3" / "levels" / "my-follow" / "my-follow.glb");
      if (fglb.is_object())
        for (const auto& n : fglb["nodes"]) extras[n.value("name", std::string())] = n.value("extras", json::object());
      // the floor's collision kept (and the bottoms of the wall and the block, on it), the rest of
      // theirs taken out, and all of the rubble's though it lies off it; the floor still the game's,
      // the block colliding by its shape where it is
      const bool follows = done && fr.collision_triangles >= 2 && fr.collision_triangles <= 8 && taken_out >= 16 &&
                           extras["lvx-floor"].value("ignore", 0) == 1 && !extras["lvx-block"].contains("set_collision");
      // and a dense piece (5 m, 3,200 triangles) on the floor: said as a place where Jak would fall
      auto dense = std::make_shared<Mesh>();
      Primitive dp;
      for (int z = 0; z <= 40; z++)
        for (int x = 0; x <= 40; x++) dp.pos.push_back({x * 0.125f - 2.5f, 0.3f + 0.01f * ((x + z) % 2), z * 0.125f - 2.5f});
      for (int z = 0; z < 40; z++)
        for (int x = 0; x < 40; x++) {
          const uint32_t a = z * 41 + x;
          dp.idx.insert(dp.idx.end(), {a, a + 41, a + 1, a + 1, a + 41, a + 42});
        }
      dp.compute_normals();
      dense->prims.push_back(dp);
      Node dn;
      dn.name = "grating";
      dn.kind = NodeKind::Render;
      dn.mesh = dense;
      dn.local.t = {0, 0, -30};
      doc->add(dn);
      doc->begin_frame();
      LevelExportReport fr2;
      fo.name = "my-dense";
      fo.nickname = "mdn";
      const bool done2 = export_level_for_game(*doc, &data, fo, &fr2, &err);
      bool dense_said = false;
      for (const auto& note : fr2.notes) dense_said |= note.key == "export.note.collision_dense";
      if (!(follows && done2 && fr2.dense_places >= 1 && dense_said))
        std::printf("    follow: done %d, collision %zu, taken out %zu, floor ignored %d, block extras %s; dense %d %zu %d (%s)\n",
                    (int)done, fr.collision_triangles, taken_out, (int)(extras["lvx-floor"].value("ignore", 0) == 1),
                    extras["lvx-block"].dump().c_str(), (int)done2, fr2.dense_places, (int)dense_said, err.c_str());
      check(follows && done2 && fr2.dense_places >= 1 && dense_said,
            "export: the game's collision taken out with the pieces deleted or moved (rubble's coarser one too), a moved piece colliding by its "
            "shape; a place denser than Jak 3 gathers (461 triangles) said");
    }
  }

  // the game's prefabs: their recipes shipped with the editor, what one takes around its anchor
  {
    std::string err;
    const auto recipes = load_prefab_recipes(prefab_recipes_path(), &err);
    std::set<std::string> categories;
    for (const auto& r : recipes) categories.insert(r.category);
    Scene lvl;
    Node proto{};
    proto.name = "fan.mb";
    const uint32_t proto_id = lvl.add(proto);
    auto put = [&](const std::string& name, uint32_t parent, Vec3 at, float size) {
      Node n{};
      n.name = name;
      n.kind = NodeKind::Render;
      n.parent = parent;
      n.mesh = make_box(name, -1);
      n.local.t = at;
      n.local.s = {size, size, size};
      return lvl.add(n);
    };
    put("fan.mb-0", proto_id, {50, 0, 0}, 1);  // the first fan elsewhere
    const uint32_t anchor = put("fan.mb-1", proto_id, {0, 0, 0}, 1);
    const uint32_t bolt = put("bolt", 0, {1.5f, 0.5f, 0}, 0.2f);
    put("far-crate", 0, {10, 0, 0}, 1);
    put("street-dirt", 0, {0.5f, 0, 0.5f}, 0.2f);
    put("huge-wall", 0, {0, 0, 0.5f}, 40);
    PrefabRecipe r;
    r.level = "test";
    r.anchor = "fan.mb";
    r.index = 1;
    r.margin = 1.f;
    r.exclude = {"dirt"};
    const auto members = recipe_members(lvl, r, &err);
    // Jak II's: their own files, apart from Jak 3's
    const auto jak2 = load_prefab_recipes(prefab_recipes_path("jak2"), &err, "jak2");
    bool jak2_apart = !jak2.empty();
    for (const auto& j : jak2) jak2_apart &= j.game == "jak2" && j.file.rfind("jak2/", 0) == 0;
    // whole structures: most of them assemblies of their level, found by an element's name
    size_t assemblies = 0;
    for (const auto& r : recipes) assemblies += !r.assembly.empty();
    check(recipes.size() >= 120 && assemblies >= recipes.size() * 3 / 4 && categories.count("building") &&
              categories.count("house") && categories.count("bridge") && categories.count("tower") &&
              categories.count("industrial") && members == std::vector<uint32_t>{anchor, bolt} && jak2_apart,
          "game prefabs: the recipes shipped by category, most of them whole assemblies, Jak II's apart; a recipe takes "
          "its anchor and what is on it, not what is far, too large or left out by name");
  }

  // a structure of Jak II brought into a Jak 3 level: the image Jak 3 has shared, another renamed,
  // its lighting compared with the level's and matched
  {
    Scene doc;
    Texture wall;
    wall.name = "wall";
    wall.width = wall.height = 2;
    wall.rgba.assign(16, 100);
    Material wm;
    wm.name = "wall";
    wm.texture = doc.add_texture(wall);
    const int doc_mat = doc.add_material(wm);
    Node decor;
    decor.kind = NodeKind::Render;
    decor.mesh = make_box("jak3-box", doc_mat);
    decor.mesh->prims[0].col.assign(decor.mesh->prims[0].pos.size(), Vec4{0.3f, 0.3f, 0.3f, 1});
    doc.add(decor);
    // the Jak II part: the same "wall" image, a "roof" Jak 3 has under that name with other pixels
    Texture roof = wall;
    roof.name = "roof";
    roof.rgba.assign(16, 200);
    Texture doc_roof = wall;
    doc_roof.name = "roof";
    doc.add_texture(doc_roof);
    std::vector<Texture> textures{wall, roof};
    Material m0, m1;
    m0.name = "wall";
    m0.texture = 0;
    m1.name = "roof";
    m1.texture = 1;
    m1.alpha = AlphaMode::Blend;
    std::vector<Material> materials{m0, m1};
    Mesh part;
    part.name = "palace-pillar";
    Primitive a = make_box("p", 0)->prims[0], b = make_box("p", 1)->prims[0];
    for (Primitive* p : {&a, &b}) {
      p->uv.assign(p->pos.size(), Vec2{0, 0});
      p->col.assign(p->pos.size(), Vec4{0.6f, 0.6f, 0.6f, 1});
    }
    part.prims = {a, b};
    json checks;
    auto mesh = import_jak2_mesh(doc, part, textures, materials, &checks);
    std::set<std::string> names;
    for (const auto& p : mesh->prims) names.insert(doc.textures[doc.materials[p.material].texture].name);
    std::map<std::string, json> by;
    for (const auto& c : checks) by[c.value("check", std::string())] = c;
    const bool converted = mesh->name == "jak2-palace-pillar" && names == std::set<std::string>{"wall", "jak2-roof"} &&
                           doc.textures.size() == 3 && by["textures"].value("renamed", 0) == 1 &&
                           by["materials"].value("blended", 0) == 1 && by["mesh"].value("ok", false) &&
                           std::fabs(by["lighting"].value("ratio", 0.0) - 2.0) < 1e-6 && !by["lighting"].value("ok", true);
    const float matched = lighting_ratio(*scaled_colors(*mesh, 0.5f), doc);
    check(converted && std::fabs(matched - 1.f) < 1e-4f && !jak2_structure(CatalogPart{CatalogPart::Kind::Prototype, "palcab-lowres-city.mb"}),
          "Jak II import: an image Jak 3 has shared, another renamed jak2-..., the lighting compared then matched");
  }

  // this editor's own tree: never jak-project's, which it neither builds against nor runs
  {
    const std::string library = default_library_root(), iso = default_iso_data_dir(), projects = default_projects_dir();
    auto not_jak_project = [](const std::string& p) {
      std::string s = p;
      for (auto& c : s) if (c == '\\') c = '/';
      return s.find("/jak-project/") == std::string::npos;
    };
    check(not_jak_project(library) && not_jak_project(iso) && not_jak_project(projects) &&
              fs::path(iso).parent_path() == fs::path(library).parent_path() &&
              fs::path(projects).parent_path() == fs::path(library) && fs::path(iso).filename() == "iso_data" &&
              fs::path(projects).filename() == "projects",
          "directories: the library, iso_data and projects defaults are this editor's own, siblings, never jak-project's");
  }

  // shapes made in the editor, an image as a texture, a material changed on some pieces only
  {
    auto near3 = [](Vec3 a, Vec3 b) { return length(a - b) < 1e-4f; };
    ShapeParams box;
    box.size = {4, 2, 6};
    box.tile = 2;
    auto bm = make_shape(box, 0);
    const AABB bb = bm->bounds();
    const Primitive& bp = bm->prims[0];
    float umin = 1e9f, umax = -1e9f;
    for (const auto& uv : bp.uv) umin = std::min(umin, uv.x), umax = std::max(umax, uv.x);
    bool outward = true;
    for (size_t t = 0; t + 2 < bp.idx.size(); t += 3) {
      const Vec3 a = bp.pos[bp.idx[t]], b = bp.pos[bp.idx[t + 1]], c = bp.pos[bp.idx[t + 2]];
      const Vec3 nrm = bp.nrm[bp.idx[t]];
      outward &= dot(cross(b - a, c - a), nrm) > 0 && dot(nrm, (a + b + c) / 3.f - Vec3{0, 1, 0}) > 0;
    }
    check(bm->tri_count() == 12 && near3(bb.lo, {-2, 0, -3}) && near3(bb.hi, {2, 2, 3}) &&
              std::fabs(umax - umin - 3.f) < 1e-4f && outward,
          "shapes: a box of its size on its base, its texture every 2 m, its faces outwards");
    ShapeParams stairs, ramp, cyl;
    stairs.kind = "stairs", stairs.size = {2, 2, 4}, stairs.steps = 4;
    ramp.kind = "ramp";
    cyl.kind = "cylinder", cyl.sides = 16;
    const ShapeParams odd = ShapeParams::from_json({{"kind", "nope"}, {"size", {-1, 5, 5}}, {"tile", 0}});
    check(make_shape(stairs, 0)->tri_count() == 48 && std::fabs(make_shape(stairs, 0)->bounds().hi.y - 2.f) < 1e-4f &&
              make_shape(ramp, 0)->tri_count() == 8 && make_shape(cyl, 0)->tri_count() == 60 && odd.kind == "box" &&
              odd.size.x == 0.05f && odd.tile == 0.1f,
          "shapes: stairs, ramp and cylinder; sizes read back kept sensible");
    // modular building: a door and a window go through their wall, a roof's ridge in its middle
    auto through = [](const std::shared_ptr<Mesh>& m, Vec3 at) {
      const Ray r{{at.x, at.y, at.z - 5}, {0, 0, 1}};
      float t;
      uint32_t tri;
      return !m->bvh(0).raycast(r, m->prims[0].pos, m->prims[0].idx, 100.f, &t, &tri);
    };
    const ShapeParams door = ShapeParams::from_json({{"kind", "doorway"}, {"size", {4, 4, 0.5}}, {"opening", {2, 3}}});
    const ShapeParams window = ShapeParams::from_json({{"kind", "window"}, {"size", {4, 4, 0.5}}, {"opening", {2, 2}}});
    const auto dm = make_shape(door, 0), wm = make_shape(window, 0);
    ShapeParams roof, hip;
    roof.kind = "roof", roof.size = {8, 3, 8};
    hip.kind = "roof-hip", hip.size = {8, 3, 10};
    const auto rm = make_shape(roof, 0), hm = make_shape(hip, 0);
    float ridge = 0;
    for (const auto& v : rm->prims[0].pos)
      if (std::fabs(v.y - 3.f) < 1e-4f) ridge = std::max(ridge, std::fabs(v.x));
    float hip_ridge = 0;
    for (const auto& v : hm->prims[0].pos)
      if (std::fabs(v.y - 3.f) < 1e-4f) hip_ridge = std::max(hip_ridge, std::fabs(v.z));
    check(through(dm, {0, 1, 0}) && !through(dm, {1.5f, 1, 0}) && !through(dm, {0, 3.5f, 0}) && through(wm, {0, 2, 0}) &&
              !through(wm, {0, 0.5f, 0}) && door.opening.x == 2.f && ridge < 1e-4f &&
              std::fabs(rm->bounds().hi.y - 3.f) < 1e-4f && std::fabs(hip_ridge - 1.f) < 1e-4f &&
              shape_preset("roof") && shape_preset("doorway") && shape_preset("pillar"),
          "shapes: a door and a window open through their wall, a roof's ridge in its middle, a hip roof's shorter");

    Scene s;
    const int grid = shape_default_material(s);
    Node piece;
    piece.kind = NodeKind::Render;
    piece.mesh = make_shape(box, grid);
    piece.extras = json{{"og_shape", box.to_json()}};
    const uint32_t ia = s.add(piece), ib = s.add(piece), ic = s.add(piece);
    const uint8_t px[2 * 2 * 4] = {200, 50, 50, 255, 50, 200, 50, 255, 50, 50, 200, 255, 200, 200, 200, 255};
    const fs::path image = tmp / "brick.png";
    stbi_write_png(image.string().c_str(), 2, 2, 4, px, 8);
    const int ti = texture_from_image(s, image.string(), &err), again = texture_from_image(s, image.string(), &err);
    s.begin_frame();
    s.cache();
    NodeChange change(s);
    change.save(ia);
    change.save(ib);
    const size_t changed = edit_material(s, {ia, ib}, grid, [ti](Material& m) { m.texture = ti; });
    // what is drawn: the new meshes, the node set being the same
    s.begin_frame();
    bool drawn_new = false, drawn_old = false;
    for (const auto& [mesh, idx] : s.cache().mesh_groups) {
      drawn_new |= mesh == s.find(ia)->mesh.get() && idx.size() == 2;
      drawn_old |= mesh == s.find(ic)->mesh.get() && idx.size() == 1;
    }
    check(drawn_new && drawn_old, "scene: a node given another mesh is drawn with it");
    HistoryEntry entry = change.commit("texture");
    const int textured = s.find(ia)->mesh->prims[0].material;
    const bool applied = changed == 2 && s.find(ia)->mesh == s.find(ib)->mesh && s.materials[textured].texture == ti &&
                         s.find(ic)->mesh->prims[0].material == grid;
    entry.undo();
    const bool undone = s.find(ia)->mesh->prims[0].material == grid;
    entry.redo();
    check(ti >= 0 && ti == again && s.textures[ti].name == "brick" && s.textures[ti].width == 2 && applied && undone &&
              shape_default_material(s) == grid && texture_from_image(s, (tmp / "none.png").string(), &err) < 0,
          "materials: an image once as a texture, on the pieces chosen only, undone");
    ShapeParams bigger = box;
    bigger.size = {10, 1, 10};
    reshape(s, *s.find(ia), bigger);
    ShapeParams read;
    check(std::fabs(s.find(ia)->mesh->bounds().hi.x - 5.f) < 1e-4f && s.find(ia)->mesh->prims[0].material == textured &&
              node_shape(*s.find(ia), &read) && read.size.x == 10.f && s.find(ib)->mesh->bounds().hi.x == 2.f,
          "shapes: resized alone, its texture kept");
  }

  // the game data shipped with the editor
  {
    GameData data;
    const bool read = data.load(game_data_path(), &err);
    const StoryCond* bridge = read ? data.type_rule("cav-break-bridge") : nullptr;
    const LevelInfo* desert = read ? data.level("desert") : nullptr;
    bool bridge_gone = false;
    if (bridge) {
      int closed_at = 0;
      for (int i = 0; i < data.step_count(); i++)
        if (data.node(i).name == "mine-boss-introduction") closed_at = i + 1;
      bridge_gone = closed_at > 1 && data.eval(*bridge, data.moment(closed_at - 1)) &&
                    !data.eval(*bridge, data.moment(closed_at));
    }
    // the city's ocean: the port's basins (masked for the mid ocean) are the near ocean's water, and
    // by the shores its squares of 3 m
    const OceanMap* city = read ? data.ocean("*ocean-map-city*") : nullptr;
    const int port = city ? 33 * city->size + 10 : 0;
    bool shore = false;
    if (city)
      for (const auto& [cell, rows] : city->near) shore |= rows.size() == 32;
    const bool port_water = city && !city->cells[port] && city->near.count(port) && city->near.at(port).empty() && shore;
    check(read && data.role("mech") == ActorRole::Creature && data.role("marauder") == ActorRole::Creature &&
              data.role("part-spawner") == ActorRole::Logic && data.role("crate") == ActorRole::Object &&
              data.is_water("water-anim") && bridge_gone && desert && !desert->ocean.empty() &&
              std::count(desert->companions.begin(), desert->companions.end(), "deserta") == 1 && port_water,
          "game data: roles, water, the mine's bridge broken by the story, the desert's ocean and companions, the "
          "near ocean of the city's port");
    // the catalog: props and doors, the interactive objects apart, no collectible; the living ones
    // the ancestry misses are creatures (left out of the levels too)
    check(read && data.catalog_category("wascity-windmill") == "static" &&
              data.catalog_category("vin-door-ctyinda") == "door" && data.catalog_category("crate") == "interactive" &&
              data.catalog_category("skill") == "collectible" && data.role("tizard") == ActorRole::Creature &&
              data.role("pecker-ingame") == ActorRole::Creature && data.role("citizen-norm-rider") == ActorRole::Creature,
          "game data: the catalog's categories (props, doors, interactive, collectibles), the animals and riders creatures");
    // a level's DGO takes the art group holding a model, none from the common code
    check(read && data.level_art_group("ctycrate-blue") == "ctycrate" &&
              data.level_art_group("collectables-skill").empty() &&
              data.level_art_group("vin-door-ctyinda") == "vin-door-ctyinda",
          "game data: the art groups a level's DGO needs (the skill's is common)");
  }

  // the particle effects of the game (data/jak3/particles.json): ctyindb's fast steam launches
  // puffs every frame that rise along its vent, slow down and fade out (their launcher hands over
  // to another at a third of a second); a light of one frame is drawn once, a glow
  {
    const char* base = SDL_GetBasePath();
    ParticleData pd;
    std::string perr;
    const bool loaded = pd.load(std::string(base ? base : "") + "data/jak3/particles.json", &perr);
    const SpGroup* steam = loaded ? pd.group("'group-ctyindb-fast-steam") : nullptr;
    bool rises = false, fades = false;
    if (steam) {
      ParticleSystem ps;
      for (int i = 0; i < 3; i++) {  // 30 frames (10 at most a call)
        ps.begin_frame();
        ps.emit(1, steam, Mat4::identity());
        ps.update(pd, 1.0 / 6.0, {0, 0, 0});
      }
      std::vector<ParticleSprite> sprites;
      ps.sprites(sprites);
      float far = 0, faint = 1;
      for (const auto& sp : sprites) {
        far = std::max(far, sp.pos.z);  // along the vent: its z
        faint = std::min(faint, sp.color.w);
      }
      rises = ps.count() > 20 && far > 3.f;
      // later: the first puffs gone (half a second of life)
      for (int i = 0; i < 3; i++) {
        ps.begin_frame();
        ps.emit(1, steam, Mat4::identity());
        ps.update(pd, 1.0 / 6.0, {0, 0, 0});
      }
      ps.sprites(sprites);
      fades = !sprites.empty() && faint < 0.2f && ps.count() < 200;
      if (!rises || !fades) std::printf("    steam: %zu particles, %.2f m along its vent, faintest %.3f\n", ps.count(), far, faint);
    }
    bool glow = false;
    if (const SpGroup* light = loaded ? pd.group("group-citylights-newslums") : nullptr) {
      ParticleSystem ps;
      ps.begin_frame();
      ps.emit(1, light, Mat4::identity());
      ps.update(pd, 1.0 / 60.0, {0, 0, 0});
      std::vector<ParticleSprite> sprites;
      ps.sprites(sprites);
      glow = sprites.size() == 1 && sprites[0].additive && std::fabs(sprites[0].sx - 4.f) < 0.01f;
    }
    if (!loaded) std::printf("    %s\n", perr.c_str());
    check(steam && rises && fades, "particles: a steam vent launches puffs that rise along it and fade out");
    check(glow, "particles: a light of one frame drawn once, added to what is behind (a glow)");
  }

  // translations: every language has every key of English, with the same printf arguments
  {
    const char* base = SDL_GetBasePath();
    const fs::path dir = fs::path(base ? base : "") / "lang";
    const json en = read_json_file(dir / "en.json");
    check(en.is_object() && en.size() > 50, "translations: lang/en.json next to the executable");
    size_t languages = 0;
    std::string problems;
    for (const auto& e : fs::directory_iterator(dir, ec)) {
      if (e.path().extension() != ".json" || e.path().stem() == "en") continue;
      languages++;
      const json other = read_json_file(e.path());
      for (const auto& [k, v] : en.items()) {
        if (!other.contains(k)) problems += e.path().stem().string() + ": no " + k + "\n";
        else if (format_args(v.get<std::string>()) != format_args(other[k].get<std::string>()))
          problems += e.path().stem().string() + ": other arguments in " + k + "\n";
      }
      for (const auto& [k, v] : other.items())
        if (!en.contains(k)) problems += e.path().stem().string() + ": unknown key " + k + "\n";
    }
    if (!problems.empty()) std::printf("%s", problems.c_str());
    check(languages >= 1 && problems.empty(), "translations: same keys and arguments as English");
    i18n_init(dir.string(), "fr");
    const bool french = std::string(tr("menu.file")) == "Fichier";
    i18n_select("en");
    check(french && std::string(tr("menu.file")) == "File" && std::string(tr("no.such.key")) == "no.such.key",
          "translations: switching language, a missing key shows itself");
  }

  // the guide's animations, drawn in a hidden window: each draws its scene (more than the clear
  // color) and names what it shows with a text of the translations; the Build panel's icons too
  if (SDL_Init(SDL_INIT_VIDEO)) {
    // OpenGL 4.3 like the editor (the game view's shaders), else 3.3
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 4);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 3);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_CORE);
    SDL_Window* window = SDL_CreateWindow("selftest", 64, 64, SDL_WINDOW_OPENGL | SDL_WINDOW_HIDDEN);
    SDL_GLContext gl = window ? SDL_GL_CreateContext(window) : nullptr;
    if (window && !gl) {
      SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 3);
      gl = SDL_GL_CreateContext(window);
    }
    ToolDemo demos;
    std::string err;
    if (gl && gladLoadGLLoader(reinterpret_cast<GLADloadproc>(SDL_GL_GetProcAddress)) && demos.init(&err)) {
      // the pixels of a GL texture of w x h that are not the demos' clear color
      auto drawn = [](unsigned tex, int w, int h) {
        std::vector<uint8_t> px((size_t)w * h * 4);
        glBindTexture(GL_TEXTURE_2D, tex);
        glGetTexImage(GL_TEXTURE_2D, 0, GL_RGBA, GL_UNSIGNED_BYTE, px.data());
        size_t n = 0;
        for (size_t i = 0; i < px.size(); i += 4)
          n += std::abs(px[i] - 33) + std::abs(px[i + 1] - 38) + std::abs(px[i + 2] - 46) > 12;
        return n;
      };
      std::string bad;
      for (const auto& name : ToolDemo::names()) {
        std::vector<DemoOverlay> overlay;
        const unsigned tex = demos.draw(name, 1.7, 160, 90, &overlay);
        bool named = false;
        for (const auto& o : overlay)
          named |= o.kind == DemoOverlay::Kind::Label && o.text.rfind("guide.demo.", 0) == 0 && o.text != tr_en(o.text.c_str());
        if (!tex || drawn(tex, 160, 90) < 160 * 90 / 10 || !named) bad += " " + name;
      }
      size_t icons = 0;
      for (const auto& p : shape_presets())
        if (const unsigned tex = demos.icon(p.params, p.name, 48); tex && drawn(tex, 48, 48) > 48 * 48 / 10) icons++;
      if (!bad.empty()) std::printf("    not drawn or not named:%s\n", bad.c_str());
      check(bad.empty() && demos.draw("no-such-demo", 0, 160, 90, nullptr) == 0 && icons == shape_presets().size(),
            "guide: every tool's animation draws its scene and names what it shows; a shape's icon for each");
      demos.shutdown();

      // the game view: a document made into the game's chunks (a ground lit by a palette of its
      // own, a box of new geometry lit from its color) and drawn by the game's renderer; a piece
      // moved makes its chunk again, alone; the game camera places things where the editor's does
      GameView gv;
      const char* base = SDL_GetBasePath();
      std::string gerr;
      if (gv.init(std::string(base ? base : "") + "game-shaders", &gerr)) {
        Scene doc;
        Texture white;
        white.name = "white";
        white.width = white.height = 2;
        white.rgba.assign(16, 255);
        Material ground_mat;
        ground_mat.texture = doc.add_texture(white);
        ground_mat.base_color = {2, 2, 2, 2};  // a game material
        const int gm = doc.add_material(ground_mat);
        auto quad = std::make_shared<Mesh>();
        Primitive& qp = quad->prims.emplace_back();
        qp.pos = {{-20, 0, -20}, {20, 0, -20}, {20, 0, 20}, {-20, 0, 20}};
        qp.uv = {{0, 0}, {1, 0}, {1, 1}, {0, 1}};
        qp.idx = {0, 1, 2, 0, 2, 3};
        qp.material = gm;
        qp.compute_normals();
        auto palette = std::make_shared<TodPalette>();
        palette->rgba.assign(32, 0);
        for (int ch = 0; ch < 4; ch++) palette->rgba[ch] = 128;  // the ambient light alone, 1.0
        auto tod = std::make_shared<TodColors>();
        tod->palette = palette;
        tod->index = {{0, 0, 0, 0}};
        Node ground;
        ground.kind = NodeKind::Render;
        ground.name = "ground";
        ground.mesh = quad;
        ground.tod = tod;
        doc.add(ground);
        ShapeParams sp;
        Node box;
        box.kind = NodeKind::Render;
        box.name = "box";
        box.mesh = make_shape(sp, -1);
        box.local.t = {0, 1, 0};
        const uint32_t box_id = doc.add(box);
        doc.begin_frame();
        OffscreenTarget target;
        const int tw = 256, th = 144;
        bool drew = false, moved_alone = false, placed = false;
        if (target.create(tw, th)) {
          Camera cam;
          cam.pos = {0, 8, 25};
          cam.look_at({0, 0, 0});
          Mat4 clip;
          gv.draw_opaque(doc, cam, 0, 0, tw, th, &clip);
          gv.draw_translucent();
          const auto px = target.read_rgba();
          const GameMood& mood = gv.current_mood();
          size_t lit = 0;
          for (size_t i = 0; i + 3 < px.size(); i += 4)
            lit += std::abs(px[i] - mood.sky_color[0]) + std::abs(px[i + 1] - mood.sky_color[1]) +
                       std::abs(px[i + 2] - mood.sky_color[2]) > 24;
          drew = gv.stats.chunks == 2 && gv.stats.error.empty() && lit > (size_t)tw * th / 4;
          doc.find(box_id)->local.t.x += 1.f;
          doc.begin_frame();
          gv.draw_opaque(doc, cam, 0, 0, tw, th, &clip);
          moved_alone = gv.stats.rebuilt == 1 && gv.stats.chunks == 2;
          const Mat4 editor = cam.proj((float)tw / th) * cam.view();
          placed = true;
          for (const Vec3& p : {Vec3{5, 1, -3}, Vec3{-8, 4, 2}, Vec3{0, 0, 0}}) {
            const Vec4 a = clip * Vec4(p, 1.f), b = editor * Vec4(p, 1.f);
            placed &= a.w > 0 && std::fabs(a.x / a.w - b.x / b.w) < 1e-3f && std::fabs(a.y / a.w - b.y / b.w) < 1e-3f &&
                      a.z / a.w > -1 && a.z / a.w < 1;
          }
          glBindFramebuffer(GL_FRAMEBUFFER, 0);
        }
        if (!drew || !gv.stats.error.empty())
          std::printf("    chunks %zu, triangles %zu, error '%s'\n", gv.stats.chunks, gv.stats.triangles, gv.stats.error.c_str());
        check(drew, "game view: the document drawn by the game's renderer, a chunk per palette");
        check(moved_alone, "game view: a piece moved makes its chunk again, alone");
        check(placed, "game view: the game's camera places the level where the editor's camera does");
        gv.shutdown();
      } else {
        std::printf("  [skip] game view: %s\n", gerr.c_str());
      }
    } else {
      std::printf("  [skip] guide animations: no OpenGL 3.3 (%s)\n", gl ? err.c_str() : SDL_GetError());
    }
    if (gl) SDL_GL_DestroyContext(gl);
    if (window) SDL_DestroyWindow(window);
    SDL_Quit();
  }

  {
    // the collision a piece of decor has when none is set, as the export gives it: the game's for a
    // level's piece (its level's collision in the document), none for a small piece, its shape else
    Scene s;
    auto group = [&](const char* name, uint32_t parent) {
      Node g;
      g.name = name;
      g.parent = parent;
      return s.add(g);
    };
    auto piece = [&](const char* name, uint32_t parent, float size) {
      ShapeParams sp;
      sp.size = {size, size, size};
      Node n;
      n.kind = NodeKind::Render;
      n.name = name;
      n.parent = parent;
      n.mesh = make_shape(sp, -1);
      return s.add(n);
    };
    const uint32_t lvx = group("lvx", 0);
    const uint32_t tfrag = group("lvx-tfrag-0", lvx);
    group("lvx-collision", lvx);
    const uint32_t a = piece("wall", tfrag, 8.f), b = piece("lamp", 0, 1.f), c = piece("slab", 0, 8.f);
    s.begin_frame();
    check(default_decor_collision(s, s.index_of(a)) == DefaultCollision::Game &&
              default_decor_collision(s, s.index_of(b)) == DefaultCollision::Small &&
              default_decor_collision(s, s.index_of(c)) == DefaultCollision::Shape,
          "in the game: the collision a piece has by default (the game's, none for a small one, its shape)");
  }

  {
    // an element placed on a lit floor takes the floor's light (its 8 palettes), its own palette; one
    // placed far from any lit decor keeps its own light
    Scene s;
    auto quad = [](float y, float half) {
      auto m = std::make_shared<Mesh>();
      Primitive p;
      p.pos = {{-half, y, -half}, {half, y, -half}, {half, y, half}, {-half, y, half}};
      p.nrm.assign(4, Vec3{0, 1, 0});
      p.idx = {0, 2, 1, 0, 3, 2};
      m->prims.push_back(std::move(p));
      return m;
    };
    auto palette = std::make_shared<TodPalette>();
    for (int k = 0; k < 8; k++) palette->rgba.insert(palette->rgba.end(), {100, 50, (uint8_t)(10 * k), 128});
    auto tod = std::make_shared<TodColors>();
    tod->palette = palette;
    tod->index = {{0, 0, 0, 0}};
    Node floor;
    floor.kind = NodeKind::Render;
    floor.name = "floor";
    floor.mesh = quad(0.f, 6.f);
    floor.tod = tod;
    s.add(floor);
    Node piece;
    piece.kind = NodeKind::Render;
    piece.name = "piece";
    piece.mesh = quad(0.5f, 1.f);
    const uint32_t near_id = s.add(piece);
    piece.local.t = {200, 0, 0};
    const uint32_t far_id = s.add(piece);
    const size_t lit = light_like_surroundings(s, {near_id, far_id});
    const Node* a = s.find(near_id);
    const Node* b = s.find(far_id);
    bool same = a && a->tod && a->tod->fits(*a->mesh) && a->tod->palette->local && a->tod->palette->size() == 1;
    if (same)
      for (int k = 0; k < 32; k++) same &= a->tod->palette->rgba[k] == palette->rgba[k];
    check(lit == 1 && same && a->extras.value(kLitHere, false) && a->mesh->prims[0].col.size() == 4 &&
              std::fabs(a->mesh->prims[0].col[0].x - 100.f / 255.f) < 0.01f && b && !b->tod &&
              lit_here_nodes(s, {near_id, far_id}) == std::vector<uint32_t>{near_id},
          "placing: an element takes the light of the level around it, one far from it keeps its own");
  }

  {
    // the models' lights (merc's light-group 0): at noon the ambient light and the sun from 2 of its
    // directions, half each
    MoodSettings ms;
    ms.hour = 12.f;
    const GameMood m = game_mood(ms);
    check(m.light_ambient[0] == m.times[0][0] && m.light_color[0][0] == m.times[2][0] * 0.5f &&
              m.light_color[1][0] == m.times[3][0] * 0.5f && m.light_dir[1][0] == -0.5f && m.light_color[2][0] == 0.f,
          "game view: the models lit by the mood's ambient light and its suns, as merc lights them");
  }

  {
    // the trees of instanced decor by their names: not a prototype whose name says tie or shrub
    check(instance_tree("ctyfarma-tie-0") && instance_tree("atoll-shrub-12") && !instance_tree("atoll-bigvents-tie-01.mb") &&
              !instance_tree("des-stone-shrub-01.mb") && !instance_tree("ctyfarma-tie-0-wind") && !instance_tree("tie-0"),
          "catalog: the decor's trees of instances told from prototypes named like them (no duplicates)");
  }

  {
    // topology: a box's points welded, a face extruded with its walls, cut in four, deleted
    ShapeParams p;
    p.size = {2, 2, 2};
    auto box = make_shape(p, -1);
    const MeshPoints pts = mesh_points(*box);
    std::set<MeshRef> top;  // the faces looking up
    for (size_t t = 0; t * 3 + 2 < box->prims[0].idx.size(); t++) {
      const auto& q = box->prims[0];
      const Vec3 n = cross(q.pos[q.idx[t * 3 + 1]] - q.pos[q.idx[t * 3]], q.pos[q.idx[t * 3 + 2]] - q.pos[q.idx[t * 3]]);
      if (normalize(n).y > 0.9f) top.insert(mesh_ref(0, t));
    }
    Mesh m(*box);
    const size_t before = m.prims[0].idx.size() / 3;
    const auto moved = extrude_faces(m, top, 1.f);
    float high = -1e9f;
    for (MeshRef f : moved)
      for (int k = 0; k < 3; k++) high = std::max(high, m.prims[0].pos[m.prims[0].idx[ref_index(f) * 3 + k]].y);
    const size_t extruded = m.prims[0].idx.size() / 3;
    const auto cut = subdivide_faces(m, moved);
    const size_t subdivided = m.prims[0].idx.size() / 3;
    const size_t gone = delete_faces(m, cut);
    check(pts.pos.size() == 8 && top.size() == 2 && moved.size() == 2 && std::fabs(high - 3.f) < 1e-4f &&
              extruded == before + 8 && subdivided == extruded + 6 && gone == 8 &&
              m.prims[0].idx.size() / 3 == extruded - 2 && m.prims[0].nrm.size() == m.prims[0].pos.size(),
          "topology: a box's 8 points, its top extruded 1 m with 4 walls, cut in four, deleted");
  }

  {
    // shapes: a wall bent a quarter turn ends turned 90 degrees; rounded corners; a ground's bumps
    ShapeParams wall;
    wall.size = {8, 4, 0.5f};
    wall.bend = 90.f;
    auto bent = make_shape(wall, -1);
    float min_x = 1e9f, max_x = -1e9f, max_z = -1e9f;
    for (const Vec3& v : bent->prims[0].pos) {
      min_x = std::min(min_x, v.x);
      max_x = std::max(max_x, v.x);
      max_z = std::max(max_z, v.z);
    }
    const float r = 8.f / radians(90.f);  // the arc's radius
    ShapeParams pillar;
    pillar.size = {2, 4, 2};
    pillar.round = 1.f;
    auto round = make_shape(pillar, -1);
    float corner = 0;  // the farthest point from the axis: 1 when it is round
    for (const Vec3& v : round->prims[0].pos) corner = std::max(corner, std::sqrt(v.x * v.x + v.z * v.z));
    ShapeParams ground = shape_preset("ground")->params;
    ground.bumps = 0.5f;
    ground.color = {0.2f, 0.3f, 0.4f, 1.f};
    auto g = make_shape(ground, -1);
    float lo = 1e9f, hi = -1e9f;
    for (size_t i = 0; i < g->prims[0].pos.size(); i++)
      if (g->prims[0].pos[i].y > 0.01f) {
        lo = std::min(lo, g->prims[0].pos[i].y);
        hi = std::max(hi, g->prims[0].pos[i].y);
      }
    const ShapeParams back = ShapeParams::from_json(ground.to_json());
    check(bent->prims[0].pos.size() > 100 && std::fabs(max_x - r * std::sin(radians(45.f)) - 0.25f * std::sin(radians(45.f))) < 0.3f &&
              std::fabs(min_x + max_x) < 0.01f && max_z > 1.f && corner < 1.01f && corner > 0.99f && hi - lo > 0.2f &&
              hi - lo < 1.5f && g->prims[0].col.size() == g->prims[0].pos.size() && back.bumps == 0.5f &&
              back.color.x == 0.2f && ShapeParams::from_json(wall.to_json()).bend == 90.f,
          "shapes: a wall bent 90 degrees along an arc, a pillar with round corners, a ground with its bumps and light");
  }

  {
    // the preview's skinning: a joint moved moves its vertices, the others stay; the rest pose is the bind pose
    Skeleton s;
    s.parent = {-1, 0};
    s.rest = {Mat4::identity(), Mat4::translate({0, 1, 0})};
    s.joints = {0, 1};
    s.inverse_bind = {Mat4::identity(), Mat4::translate({0, -1, 0})};
    Skeleton::Animation a;
    a.name = "lift";
    a.duration = 1.f;
    Skeleton::Channel c;
    c.node = 1;
    c.path = 0;
    c.times = {0.f, 1.f};
    c.values = {{0, 1, 0, 0}, {0, 3, 0, 0}};
    a.channels.push_back(c);
    s.animations.push_back(a);
    Mesh bind;
    bind.prims.emplace_back();
    auto& p = bind.prims[0];
    p.pos = {{0, 0, 0}, {0, 1, 0}};
    p.nrm = {{0, 1, 0}, {0, 1, 0}};
    p.extra_attrs["JOINTS_0"] = {{0, 0, 0, 0}, {1, 0, 0, 0}};
    p.extra_attrs["WEIGHTS_0"] = {{1, 0, 0, 0}, {1, 0, 0, 0}};
    Mesh out(bind);
    const bool rest = skin_mesh(bind, joint_matrices(s, skeleton_pose(s, -1, 0)), out) &&
                      length(out.prims[0].pos[1] - Vec3{0, 1, 0}) < 1e-5f;
    skin_mesh(bind, joint_matrices(s, skeleton_pose(s, 0, 0.5f)), out);
    check(rest && length(out.prims[0].pos[0]) < 1e-5f && length(out.prims[0].pos[1] - Vec3{0, 2, 0}) < 1e-4f,
          "preview: a model's vertices follow its joints (the rest pose, a joint half-way up)");
  }

  {
    // a level's style: its floor's texture and its size (1 repeat over 4 m), its walls'
    Scene s;
    Texture t;
    t.name = "floor-tex";
    t.width = t.height = 1;
    t.rgba = {1, 2, 3, 255};
    Material mf;
    mf.name = "floor";
    mf.texture = s.add_texture(t);
    const int floor_mat = s.add_material(mf);
    t.name = "wall-tex";
    Material mw;
    mw.name = "wall";
    mw.texture = s.add_texture(t);
    const int wall_mat = s.add_material(mw);
    auto quad = [](Vec3 a, Vec3 b, Vec3 c, Vec3 d, float uv, int mat) {
      auto m = std::make_shared<Mesh>();
      m->prims.emplace_back();
      auto& p = m->prims[0];
      p.pos = {a, b, c, d};
      p.uv = {{0, 0}, {uv, 0}, {uv, uv}, {0, uv}};
      p.col.assign(4, Vec4{0.3f, 0.3f, 0.3f, 1});
      p.idx = {0, 2, 1, 0, 3, 2};
      p.material = mat;
      p.compute_normals();
      return m;
    };
    Node f;
    f.kind = NodeKind::Render;
    f.mesh = quad({0, 0, 0}, {8, 0, 0}, {8, 0, 8}, {0, 0, 8}, 2.f, floor_mat);
    f.extras["og_terrain"] = true;
    s.add(f);
    Node w;
    w.kind = NodeKind::Render;
    w.mesh = quad({0, 0, 0}, {6, 0, 0}, {6, 6, 0}, {0, 6, 0}, 2.f, wall_mat);
    s.add(w);
    const LevelStyle st = measure_style(s);
    check(st.floor.texture == "floor-tex" && std::fabs(st.floor.tile - 4.f) < 0.01f && st.wall.texture == "wall-tex" &&
              std::fabs(st.wall.tile - 3.f) < 0.01f && std::fabs(st.floor.color.x - 0.3f) < 0.01f && st.bumps < 0.01f,
          "build: a level's style, its floor's and walls' textures, their size, its light, a flat ground");
  }

  {
    // the catalog's objects without a model: a picture of what they do
    const auto vent = object_icon("industrial-board-vent", "movement");
    const auto gate = object_icon("sewer-elec-gate", "hazard");
    const auto any = object_icon("some-type", "other");
    check(vent->prims.size() == 3 && gate->prims.size() == 2 && !any->prims.empty() && !vent->prims[0].col.empty(),
          "catalog: an object without a model drawn as what it does (a vent blowing up, an electric gate)");
  }

  {
    // assemblies: two houses (four walls and a roof each) on one sidewalk, a pipe from one to the
    // other, a lamp post in the first, dirt on the ground; a row of blocks 200 m long
    Scene s;
    auto put = [&](const std::string& name, Vec3 at, Vec3 size) {  // a box of that size, its base at `at`
      Node n{};
      n.name = name;
      n.kind = NodeKind::Render;
      n.mesh = make_box(name, -1);
      n.local.t = at;
      n.local.s = size * 0.5f;
      return s.add(n);
    };
    std::vector<uint32_t> house[2];
    for (int h = 0; h < 2; h++) {
      const float x = h * 30.f;
      const std::string p = h ? "B-" : "A-";
      house[h] = {put(p + "front", {x, 0, 0}, {10, 6, 1}), put(p + "back", {x, 0, 8}, {10, 6, 1}),
                  put(p + "left", {x - 5, 0, 4}, {1, 6, 9}), put(p + "right", {x + 5, 0, 4}, {1, 6, 9}),
                  put(p + "roof", {x, 6, 4}, {11, 0.5f, 10})};
    }
    const uint32_t lamp = put("A-lamp-post", {2, 0, 4}, {0.3f, 4, 0.3f});
    house[0].push_back(lamp);
    const uint32_t sidewalk = put("sidewalk", {15, -0.3f, -1}, {40, 0.3f, 10});
    const uint32_t pipe = put("pipe", {15, 3, -0.75f}, {30, 0.5f, 0.5f});
    const uint32_t dirt = put("street-dirt-01", {0, 0, 4}, {2, 0.05f, 2});
    std::vector<uint32_t> row;
    for (int b = 0; b < 20; b++) row.push_back(put("block-" + std::to_string(b), {100.f + b * 10.f, 0, 50}, {10, 5, 4}));
    const auto found = find_assemblies(s);
    auto sorted = [](std::vector<uint32_t> v) {
      std::sort(v.begin(), v.end());
      return v;
    };
    const auto a = assembly_of(found, house[0][0]), b = assembly_of(found, house[1][0]);
    bool row_ok = true;
    std::set<uint32_t> row_seen;
    size_t row_parts = 0;
    for (const auto& as : found) {
      if (std::find(row.begin(), row.end(), as[0]) == row.end()) continue;
      row_parts++;
      AABB box;
      for (uint32_t id : as) {
        row_ok &= std::find(row.begin(), row.end(), id) != row.end() && row_seen.insert(id).second;
        box.add(s.cache().bounds[s.index_of(id)]);
      }
      row_ok &= box.size().x <= 60.5f;
    }
    // a recipe of the game's prefabs finds the same assembly by the name of one of its elements
    PrefabRecipe r;
    r.level = "test";
    r.assembly = "A-roof";
    std::string err;
    const auto members = recipe_members(s, r, &err);
    std::printf("    assemblies: %zu; house A %zu, B %zu; row %zu parts\n", found.size(), a.size(), b.size(), row_parts);
    check(a == sorted(house[0]) && b == sorted(house[1]) && assembly_of(found, sidewalk) == std::vector<uint32_t>{sidewalk} &&
              assembly_of(found, pipe) == std::vector<uint32_t>{pipe} && assembly_of(found, dirt) == std::vector<uint32_t>{dirt} &&
              row_ok && row_seen.size() == row.size() && row_parts >= 4 && members == a,
          "assemblies: a house its walls, roof and lamp, not the sidewalk under both nor the pipe between them nor "
          "the dirt; a row 200 m long cut in parts of 60 m at most; a recipe finds it by an element's name");
  }

  {
    // the terrain brush: ground painted at the stroke's height, raised and dug in its middle, walls
    // raised and taken away, the grid kept through its node's extras, hills generated
    TerrainGrid g;
    g.cell = 1.f;
    g.looks[0].floor_material = 0;
    g.looks[0].wall_material = 1;
    BrushStroke st;
    st.mode = BrushMode::Paint;
    st.center = {0, 0, 0};
    st.size = 4.f;
    st.height = 1.f;
    const bool painted = apply_brush(g, st);
    size_t cells = 0;
    for (uint8_t c : g.cells) cells += c == TerrainGrid::Ground;
    st.mode = BrushMode::Raise;
    st.amount = 2.f;
    apply_brush(g, st);
    const float raised = g.corner(0, 0), edge = g.corner(-2, -2);
    st.mode = BrushMode::Dig;
    st.amount = 0.5f;
    apply_brush(g, st);
    float middle = 0;
    const bool on = terrain_height(g, 0.f, 0.f, &middle);
    float none = 0;
    const bool off = terrain_height(g, 20.f, 0.f, &none);
    st.mode = BrushMode::Wall;
    st.center = {10.f, 0, 0.f};
    st.size = 2.f;
    st.height = 0.f;
    st.amount = 3.f;
    apply_brush(g, st);
    size_t walls = 0;
    for (uint8_t c : g.cells) walls += c == TerrainGrid::Wall;
    const auto mesh = make_terrain_mesh(g);
    float top = -1e9f;
    for (const auto& p : mesh->prims)
      if (p.material == 1)
        for (const auto& v : p.pos) top = std::max(top, v.y);
    TerrainGrid back;
    const bool read = TerrainGrid::from_json(g.to_json(), &back);
    bool same = read && back.cells == g.cells && back.height.size() == g.height.size();
    for (size_t i = 0; same && i < g.height.size(); i++) same &= std::fabs(back.height[i] - g.height[i]) < 0.006f;
    // another style painted over a part: its own floor and walls, kept in the node; raise where
    // there is no ground lays some
    TerrainGrid two = g;
    TerrainGrid::Look other;
    other.floor_material = 2;
    other.wall_material = 3;
    BrushStroke ps;
    ps.center = {0, 0, 0};
    ps.size = 2.f;
    ps.height = 1.f;
    ps.look = two.add_look(other);
    const bool repainted = apply_brush(two, ps) && two.add_look(other) == ps.look;
    bool floor2 = false, floor0 = false;
    const auto mesh_two = make_terrain_mesh(two);
    for (const auto& p : mesh_two->prims) {
      floor2 |= p.material == 2;
      floor0 |= p.material == 0;
    }
    TerrainGrid two_back;
    const bool looks_kept = TerrainGrid::from_json(two.to_json(), &two_back) && two_back.look == two.look &&
                            two_back.looks.size() == 2 && two_back.looks[1].wall_material == 3;
    BrushStroke rs;
    rs.mode = BrushMode::Raise;
    rs.center = {30.f, 0, 30.f};
    rs.size = 2.f;
    rs.amount = 1.f;
    const bool raised_new = apply_brush(two, rs) && two.at(30, 30) == TerrainGrid::Ground && two.corner(30, 30) > 0.9f;
    st.mode = BrushMode::Erase;
    apply_brush(g, st);
    size_t walls_after = 0;
    for (uint8_t c : g.cells) walls_after += c == TerrainGrid::Wall;
    TerrainGrid hills;
    generate_terrain(hills, {0, 5, 0}, {32, 32}, 10.f, 0.5f, 3);
    float lo = 1e9f, hi = -1e9f;
    for (float h : hills.height) {
      lo = std::min(lo, h);
      hi = std::max(hi, h);
    }
    const bool border = std::fabs(hills.corner(hills.x0, hills.z0) - 5.f) < 0.01f;
    std::printf("    terrain: %zu cells, raised %.2f edge %.2f, middle %.2f, %zu walls (top %.2f), hills %.2f..%.2f; styles: "
                "repainted %d floors %d/%d kept %d, raised new %d\n",
                cells, raised, edge, middle, walls, top, lo, hi, repainted, floor0, floor2, looks_kept, raised_new);
    check(painted && cells == 16 && std::fabs(raised - 3.f) < 0.01f && edge < raised && on && std::fabs(middle - 2.5f) < 0.01f &&
              !off && walls == 4 && std::fabs(top - 3.f) < 0.01f && mesh->prims.size() == 2 && same && walls_after == 0 &&
              lo >= 4.99f && hi <= 15.01f && hi - lo > 2.f && border && repainted && floor2 && floor0 && looks_kept &&
              raised_new,
          "terrain brush: ground painted at the stroke's height, raised and dug in the square's middle, walls up and "
          "away, kept in the node, hills generated meeting the ground around, another style painted over a part, "
          "raise laying ground where there is none");

  }
  {
    // what a stamp would change, shown before it is done: the cells laid, raised, walled, taken away
    TerrainGrid none;
    TerrainGrid laid = none;
    BrushStroke stamp;
    stamp.size = 4.f;
    stamp.center = {2, 0, 2};
    apply_brush(laid, stamp);
    TerrainGrid raised = laid, walled = laid, erased = laid, repainted = laid;
    stamp.mode = BrushMode::Raise;
    stamp.amount = 1.f;
    apply_brush(raised, stamp);
    stamp.mode = BrushMode::Wall;
    stamp.amount = 3.f;
    apply_brush(walled, stamp);
    stamp.mode = BrushMode::Erase;
    apply_brush(erased, stamp);
    stamp.mode = BrushMode::Paint;
    apply_brush(repainted, stamp);
    const auto new_cells = terrain_changes(none, laid, -2, -2, 6, 6), up = terrain_changes(laid, raised, -2, -2, 6, 6);
    const auto walls_up = terrain_changes(laid, walled, -2, -2, 6, 6), gone = terrain_changes(laid, erased, -2, -2, 6, 6);
    check(new_cells.size() == 16 && !new_cells[0].removed && !up.empty() && up.size() <= 16 && walls_up.size() == 16 &&
              gone.size() == 16 && gone[0].removed && terrain_changes(laid, repainted, -2, -2, 6, 6).empty(),
          "terrain brush preview: the cells a stamp lays, raises, walls or takes away, none painted over again");
  }

  {
    // building without visual bugs: holes in the decor
    // holes: two slabs of a floor 1 m apart, walls all around: the gap between them is found and
    // where it is; the floor whole: none
    Scene g;
    auto slab = [&](Scene& sc, const std::string& name, Vec3 at, Vec3 size) {
      Node n{};
      n.name = name;
      n.kind = NodeKind::Render;
      n.mesh = make_box(name, -1);
      n.local.t = at;
      n.local.s = size * 0.5f;
      return sc.add(n);
    };
    auto walls_around = [&](Scene& sc) {
      slab(sc, "wall-n", {0, -1, -21}, {44, 12, 2});
      slab(sc, "wall-s", {0, -1, 21}, {44, 12, 2});
      slab(sc, "wall-w", {-21, -1, 0}, {2, 12, 44});
      slab(sc, "wall-e", {21, -1, 0}, {2, 12, 44});
    };
    slab(g, "left", {-10.25f, -0.2f, 0}, {19.5f, 0.2f, 40});
    slab(g, "right", {10.25f, -0.2f, 0}, {19.5f, 0.2f, 40});
    walls_around(g);
    GapOptions go;
    go.region.add({-18, -1, -18});
    go.region.add({18, 4, 18});
    go.spacing = 4.f;
    const GapReport holes = find_gaps(g, go);
    size_t gaps = 0, at_gap = 0;
    for (const auto& sp : holes.spots)
      if (!sp.edge) {
        gaps++;
        at_gap += std::fabs(sp.at.x) < 1.5f;
      }
    Scene whole;
    slab(whole, "floor", {0, -0.2f, 0}, {40, 0.2f, 40});
    walls_around(whole);
    const GapReport none = find_gaps(whole, go);
    std::printf("    holes: %zu viewpoints, %zu rays, %zu escaped, %zu gaps (%zu at the slit); whole floor %zu spots\n",
                holes.viewpoints, holes.rays, holes.escaped, gaps, at_gap, none.spots.size());
    check(holes.viewpoints > 0 && gaps > 0 && at_gap == gaps && none.spots.empty(),
          "holes in the decor: the slit between two slabs found where it is, a whole floor walled around has none");

    // the weathers: a storm darker than a clear sky, its fog nearer; every mix of cloud and fog read
    MoodSettings clear, storm;
    clear.cloud = 0.f;
    storm.cloud = 1.f;
    storm.fog = 1.f;
    const GameMood mc = game_mood(clear), ms = game_mood(storm);
    bool all = true;
    for (float c = 0; c <= 1.001f; c += 0.125f)
      for (float f = 0; f <= 1.001f; f += 0.25f) {
        MoodSettings m;
        m.cloud = c;
        m.fog = f;
        const GameMood gm = game_mood(m);
        all &= std::isfinite(gm.fog_end) && gm.fog_end > 0 && std::isfinite(gm.light_ambient[0]);
      }
    int presets = 0;
    weather_presets(&presets);
    check(ms.times[1][0] < mc.times[1][0] && ms.fog_end < mc.fog_end && mc.cloud_min > ms.cloud_min && all && presets == 10,
          "weather: a storm's sun dimmer and its fog nearer than a clear sky's, every mix of the game's tables read");

    // groups: a copy of a group is a group of its own; a prefab placed forgets the groups it was made of
    Scene d;
    Node a{};
    a.name = "stall";
    a.kind = NodeKind::Render;
    a.mesh = make_box("stall", -1);
    set_group_tag(a, 1, "market", false);
    const uint32_t ida = d.add(a);
    const auto copies = paste_nodes(d, copy_nodes(d, {ida}));
    Scene pre;
    pre.add(a);
    std::map<const Mesh*, std::shared_ptr<Mesh>> merged;
    const auto placed = place_prefab(d, pre, Mat4::identity(), &merged);
    const Node* copy = copies.empty() ? nullptr : d.find(copies[0]);
    const Node* p0 = placed.empty() ? nullptr : d.find(placed[0]);
    check(copy && group_tag(*copy) == 2 && group_tag(*d.find(ida)) == 1 && copy->extras["og_group"]["name"] == "market" &&
              p0 && group_tag(*p0) == 0 && new_group_id(d) == 3,
          "groups: a copy of a group is a new group of the same name, a prefab placed forgets the groups it was made of");
  }

  fs::remove_all(tmp, ec);
  std::printf("%s (%d failure%s)\n", g_failures ? "FAILED" : "OK", g_failures, g_failures == 1 ? "" : "s");
  return g_failures ? 1 : 0;
}

}  // namespace ogle
