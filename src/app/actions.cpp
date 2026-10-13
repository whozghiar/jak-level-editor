#include <SDL3/SDL.h>

#include <algorithm>
#include <cstring>
#include <ctime>
#include <filesystem>
#include <fstream>
#include <set>
#include <unordered_map>
#include <unordered_set>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "core/prefs.h"
#include "io/gltf_io.h"
#include "io/jak2_import.h"
#include "scene/assemblies.h"
#include "scene/relight.h"
#include "io/nav_export.h"
#include "scene/story.h"

namespace ogle {

namespace fs = std::filesystem;

namespace {

fs::path to_path(const std::string& s) { return fs::path(std::u8string((const char8_t*)s.c_str())); }
std::string from_path(const fs::path& p) {
  auto u = p.u8string();
  return std::string(u.begin(), u.end());
}

std::string lower_ext(const std::string& path) {
  std::string e = to_path(path).extension().string();
  for (auto& c : e) c = (char)std::tolower((unsigned char)c);
  return e;
}

// The model of an actor (catalog.h: its look, its art-name, the models of its type...).
const ModelAsset* actor_model(const Node& actor, const AssetLibrary& library, const GameData* data,
                              const std::string& group) {
  auto lump = actor.extras.find("lump");
  return resolve_actor_model(library, data, actor.extras.value("etype", std::string()),
                             lump != actor.extras.end() ? *lump : json(), group);
}

// The game draws water and dark eco as a see-through layer lit by a reflection map, which is not
// drawn here: water becomes a bluish translucent surface, dark eco (a black layer) a dark purple
// one. Lava keeps its own look.
void style_water_mesh(Scene& scene, const Mesh& mesh, const std::string& model) {
  if (model.find("lava") != std::string::npos) return;
  const bool dark_eco = model.find("dark-eco") != std::string::npos;
  std::set<int> done;
  for (const auto& prim : mesh.prims) {
    if (prim.material < 0 || prim.material >= (int)scene.materials.size() || !done.insert(prim.material).second)
      continue;
    Material& m = scene.materials[prim.material];
    m.alpha = AlphaMode::Blend;
    m.water = true;
    if (dark_eco) {
      m.texture = -1;
      m.base_color = {0.2f, 0.07f, 0.3f, 0.88f};
    } else {
      m.base_color = {m.base_color.x * 0.55f, m.base_color.y * 0.8f, m.base_color.z, std::min(m.base_color.w, 0.75f)};
    }
  }
}

// Draws under each actor of scene.nodes[first..] the model the library has for it (from the
// folder `group` first). Returns the number of actors with one.
size_t attach_actor_models(Scene& scene, const AssetLibrary& library, const GameData* data, size_t first,
                           const std::string& group) {
  std::map<std::string, std::shared_ptr<Mesh>> meshes;  // model file -> mesh of the document
  std::vector<uint32_t> actors;
  for (size_t i = first; i < scene.nodes.size(); i++)
    if (scene.nodes[i].kind == NodeKind::Actor) actors.push_back(scene.nodes[i].id);
  size_t attached = 0;
  for (auto id : actors) {
    const ModelAsset* model = actor_model(*scene.find(id), library, data, group);
    if (!model) continue;
    auto it = meshes.find(model->path);
    if (it == meshes.end()) {
      LoadedMesh lm;
      std::string err;
      std::shared_ptr<Mesh> mesh;
      if (load_model_asset(model->path, &lm, &err)) mesh = merge_mesh(scene, *lm.mesh, lm.textures, lm.materials);
      else LOG_WARN("%s: %s", model->path.c_str(), err.c_str());
      const std::string etype = scene.find(id)->extras.value("etype", std::string());
      if (mesh && data && data->is_water(etype)) style_water_mesh(scene, *mesh, model->name);
      it = meshes.emplace(model->path, mesh).first;
    }
    if (!it->second) continue;
    Node v;
    v.parent = id;
    v.name = model->name;
    v.kind = NodeKind::Render;
    v.mesh = it->second;
    v.extras = json{{"og_actor_visual", model->name}};
    scene.add(std::move(v));
    attached++;
  }
  return attached;
}

// A piece of a level's shell is written in the level's space: its origin goes to the bottom center
// of its geometry, so that it moves, turns and scales around itself (its own mesh only).
void center_piece_origin(Node& n) {
  if (!n.mesh || n.mesh.use_count() != 1 || length(n.local.t) != 0.f) return;
  const AABB b = n.mesh->bounds();
  if (!b.valid()) return;
  const Vec3 origin{b.center().x, b.lo.y, b.center().z};
  for (auto& prim : n.mesh->prims)
    for (auto& p : prim.pos) p = p - origin;
  n.mesh->touch();
  n.local.t = origin;
}

// Removes the nodes of `doomed` and their descendants.
void remove_subtrees(Scene& scene, std::unordered_set<uint32_t> doomed) {
  if (doomed.empty()) return;
  // parents may come after their children: repeat until nothing is added
  for (bool grew = true; grew;) {
    grew = false;
    for (const auto& n : scene.nodes)
      if (n.parent && !doomed.count(n.id) && doomed.count(n.parent)) grew = doomed.insert(n.id).second || grew;
  }
  scene.nodes.erase(std::remove_if(scene.nodes.begin(), scene.nodes.end(),
                                   [&](const Node& n) { return doomed.count(n.id) != 0; }),
                    scene.nodes.end());
}

// What a level file brings, made ready for editing (scene.nodes[first..]): without its creatures
// and logic actors (without any actor when not `actors`), but with its particle effects (steam,
// sparks, lights: part of its look, exported with it), the terrain locked, a model under each
// actor.
void prepare_level_nodes(Scene& scene, const AssetLibrary& library, const GameData* data, size_t first,
                         const std::string& group, bool actors = true) {
  if (data || !actors) {
    std::unordered_set<uint32_t> doomed;
    for (size_t i = first; i < scene.nodes.size(); i++) {
      const Node& n = scene.nodes[i];
      if (n.kind != NodeKind::Actor) continue;
      const std::string etype = n.extras.value("etype", std::string());
      if (!actors) {
        // decor alone: without objects, but the platforms and bridges that are part of getting
        // around the level stay (a rope bridge to a hut is part of its shape, not a gameplay item)
        if (data && data->object_family(etype) == "platform") continue;
        doomed.insert(n.id);
      } else if (data->role(etype) != ActorRole::Object && !data->is_effect(etype)) {
        doomed.insert(n.id);
      }
    }
    remove_subtrees(scene, std::move(doomed));
  }
  // where each actor is in the game: the positions inside its data (paths) are the game's, and
  // follow it when it moves (the export moves them by as much)
  for (size_t i = first; i < scene.nodes.size(); i++) {
    Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Actor || n.extras.contains("og_origin")) continue;
    const Transform w = Transform::from_matrix(scene.world(n));
    n.extras["og_origin"] = {w.t.x, w.t.y, w.t.z, w.r.x, w.r.y, w.r.z, w.r.w};
  }
  // the terrain is locked: clicks go to what is on it. The extractor splits the level's shell
  // (tfrag) in pieces and marks the large ones og_terrain; older extractions have one mesh per
  // texture over the whole level, all terrain.
  std::unordered_map<uint32_t, size_t> index;
  for (size_t i = first; i < scene.nodes.size(); i++) index[scene.nodes[i].id] = i;
  for (size_t i = first; i < scene.nodes.size(); i++) {
    Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render) continue;
    // the distant, low-detail backdrop (mountains beyond the playable area): made to be seen
    // through the game's fog, which the editor has none of; hazed at render time instead
    // (og_backdrop persists through save/reopen and export, on the mesh: it round-trips there)
    auto p = index.find(n.parent);
    if (n.mesh && p != index.end() && scene.nodes[p->second].name.find("-tfrag-lowres") != std::string::npos)
      n.mesh->extras["og_backdrop"] = true;
    auto terrain = n.extras.find("og_terrain");
    if (terrain != n.extras.end() && terrain->is_boolean()) {
      // the extractor marks every piece 60 m across; only the ground stays locked (a facade or a
      // door of the shell as large is selected like the rest), the walls the game made one with a
      // ground given a piece of their own (selected like a facade)
      if (terrain->get<bool>() && n.mesh && ground_piece(*n.mesh)) {
        if (split_ground_piece(scene, i) >= 0) scene.nodes[i].extras["og_split"] = true;
        Node& g = scene.nodes[i];
        g.locked = ground_piece(*g.mesh);
        if (!g.locked) center_piece_origin(g);
      } else {
        n.locked = false;
        center_piece_origin(n);
      }
      continue;
    }
    if (p != index.end() && scene.nodes[p->second].name.find("-tfrag-") != std::string::npos) n.locked = true;
  }
  attach_actor_models(scene, library, data, first, group);
}

// Adds a level of the library under a group telling what it is (a layer, a companion, a level
// added: see scene/story.h), under `parent`. Returns the group, 0 when there is nothing to add.
uint32_t add_extra_level(Scene& scene, const AssetLibrary& library, const GameData* data, const LevelEntry& level,
                         const char* key, uint32_t parent = 0, bool actors = true) {
  Node group;
  group.name = level.name;
  group.kind = NodeKind::Group;
  group.parent = parent;
  group.extras = json{{key, level.name}};
  const uint32_t id = scene.add(std::move(group));
  const size_t first = scene.nodes.size();
  ImportOptions o;
  o.parent = id;
  ImportResult r;
  std::string err;
  if (!import_gltf(scene, level.background, o, &r, &err)) {
    LOG_WARN("%s: %s", level.background.c_str(), err.c_str());
    scene.remove({id});
    return 0;
  }
  prepare_level_nodes(scene, library, data, first, level.name, actors);
  // a layer of creatures only brings nothing
  bool content = false;
  for (size_t i = first; i < scene.nodes.size() && !content; i++)
    content = scene.nodes[i].kind == NodeKind::Actor || scene.nodes[i].kind == NodeKind::Render;
  if (!content) {
    scene.remove({id});
    return 0;
  }
  return id;
}

// A level of Jak II made decor of Jak 3 (its nodes from `first`): without its actors (their code is
// Jak II's), with its collision (the surfaces of Jak II are written as Jak 3's: the same bits, the
// same materials and events, Jak 3 has more), its textures and materials renamed apart from Jak 3's
// (jak2-...), its pieces marked as Jak II's, and its groups named jak2-<level>-...: nothing takes
// it for a level of Jak 3 of the same name (Jak II has a ctyport, a stadium...).
void make_jak2_level(Scene& scene, size_t first, const std::string& level) {
  std::unordered_set<uint32_t> doomed;
  for (size_t i = first; i < scene.nodes.size(); i++)
    if (scene.nodes[i].kind == NodeKind::Actor) doomed.insert(scene.nodes[i].id);
  remove_subtrees(scene, std::move(doomed));
  auto prefixed = [](const std::string& s) { return s.rfind("jak2-", 0) == 0; };
  for (auto& t : scene.textures)
    if (!prefixed(t.name)) t.name = "jak2-" + t.name;
  for (auto& m : scene.materials)
    if (!prefixed(m.name)) m.name = "jak2-" + m.name;
  const std::string group = level + "-";
  for (size_t i = first; i < scene.nodes.size(); i++) {
    Node& n = scene.nodes[i];
    if (n.name.rfind(group, 0) == 0 && (instance_tree(n.name) || n.name.find("-tfrag-") != std::string::npos ||
                                         n.name == group + "collision" || n.name == group + "actors"))
      n.name = "jak2-" + n.name;
    if (n.kind == NodeKind::Render && n.mesh) n.extras["og_import"] = {{"game", "jak2"}, {"level", level}};
  }
}

std::string unique_name(const std::string& base, std::unordered_set<std::string>& used) {
  std::string name = base;
  for (int i = 2; used.count(name); i++) name = base + "-" + std::to_string(i);
  used.insert(name);
  return name;
}

}  // namespace

// ------------------------------------------------------------------------------------------------
// document operations (no window needed)
// ------------------------------------------------------------------------------------------------

bool open_project(Scene& scene, const std::string& path, std::string* error) {
  const std::string ext = lower_ext(path);
  if (ext != ".glb" && ext != ".gltf") {
    if (error) *error = "not a .glb file";
    return false;
  }
  ImportOptions o;
  o.read_settings = true;
  ImportResult r;
  if (!import_gltf(scene, path, o, &r, error)) return false;
  // projects saved before the ground alone was locked: their large walls and doors are freed; and
  // before the walls standing on a ground had a piece of their own: given one, as a level opened
  for (size_t i = 0; i < scene.nodes.size(); i++) {
    Node& n = scene.nodes[i];
    if (!n.locked || n.kind != NodeKind::Render || !n.mesh || !n.extras.contains("og_terrain")) continue;
    if (!ground_piece(*n.mesh)) {
      n.locked = false;
      center_piece_origin(n);
      continue;
    }
    if (n.extras.contains("og_split")) continue;
    if (split_ground_piece(scene, i) < 0) continue;
    Node& g = scene.nodes[i];
    g.extras["og_split"] = true;
    g.locked = ground_piece(*g.mesh);
  }
  return true;
}

bool save_project(const Scene& scene, const std::string& path, std::string* error) {
  ExportOptions o;
  o.mode = ExportMode::Project;
  // write to a temporary file first: a crash while saving must not destroy the project
  const std::string tmp = path + ".tmp";
  if (!export_glb(scene, tmp, o, error)) return false;
  std::error_code ec;
  fs::rename(to_path(tmp), to_path(path), ec);
  if (ec) {
    fs::remove(to_path(path), ec);
    fs::rename(to_path(tmp), to_path(path), ec);
    if (ec) {
      if (error) *error = "cannot replace " + path;
      return false;
    }
  }
  return true;
}

bool open_level(Scene& scene, const AssetLibrary& library, const GameData* data, const LevelEntry& level,
                std::string* error, bool actors) {
  ImportOptions o;
  ImportResult r;
  if (!import_gltf(scene, level.background, o, &r, error)) return false;
  // the game's name of the level (a copy found in another DGO has its folder named after both)
  scene.settings.level = level.level.empty() ? level.name : level.level;
  scene.settings.source_group = level.name;
  prepare_level_nodes(scene, library, data, 0, level.name, actors);
  if (!data) return true;
  // the levels the game shows with it, and the layers the story lends it
  if (const LevelInfo* info = data->level(scene.settings.level))
    for (const auto& name : info->companions)
      if (const LevelEntry* c = library.level(name)) add_extra_level(scene, library, data, *c, "og_companion", 0, actors);
  for (const auto& name : data->layers_of(scene.settings.level))
    if (const LevelEntry* l = library.level(name)) add_extra_level(scene, library, data, *l, "og_layer", 0, actors);
  return true;
}

uint32_t add_level(Scene& scene, const AssetLibrary& library, const GameData* data, const LevelEntry& level,
                   std::string* error, bool actors) {
  const std::string name = level.level.empty() ? level.name : level.level;
  const uint32_t group = add_extra_level(scene, library, data, level, "og_level", 0, actors);
  if (!group) {
    if (error) *error = "cannot read " + level.background;
    return 0;
  }
  if (!data) return group;
  if (const LevelInfo* info = data->level(name))
    for (const auto& c : info->companions)
      if (const LevelEntry* l = library.level(c))
        add_extra_level(scene, library, data, *l, "og_companion", group, actors);
  for (const auto& layer : data->layers_of(name))
    if (const LevelEntry* l = library.level(layer)) add_extra_level(scene, library, data, *l, "og_layer", group, actors);
  return group;
}

bool level_listed(const LevelEntry& level, const GameData* data) {
  if (!level.level.empty() && level.level != level.name) return false;
  if (level.collision_triangles == 0) return false;
  const LevelInfo* info = data ? data->level(level.name) : nullptr;
  return !(info && info->layer);
}

Clipboard copy_nodes(const Scene& scene, const std::vector<uint32_t>& roots) {
  Clipboard clip;
  const auto below = scene.descendants_of(roots);
  for (size_t r = 0; r < roots.size(); r++) {
    const Node* n = scene.find(roots[r]);
    if (!n) continue;
    clip.roots.push_back(roots[r]);
    clip.root_worlds[roots[r]] = scene.world(*n);
    clip.nodes.push_back(*n);
    for (auto d : below[r])
      if (const Node* dn = scene.find(d)) clip.nodes.push_back(*dn);
  }
  return clip;
}

std::vector<uint32_t> paste_nodes(Scene& scene, const Clipboard& clip) {
  std::unordered_map<uint32_t, uint32_t> ids;
  for (const auto& n : clip.nodes) ids[n.id] = scene.next_id++;
  std::unordered_set<std::string> names;
  for (const auto& n : scene.nodes) names.insert(n.name);
  // a copy of a group is a group of its own
  std::unordered_map<uint32_t, uint32_t> groups;
  uint32_t next_group = new_group_id(scene);
  std::vector<uint32_t> roots;
  for (const auto& src : clip.nodes) {
    Node n = src;
    n.id = ids[src.id];
    if (const uint32_t g = group_tag(src)) {
      auto [it, fresh] = groups.emplace(g, next_group);
      if (fresh) next_group++;
      n.extras["og_group"]["id"] = it->second;
    }
    const bool is_root = clip.root_worlds.count(src.id) != 0;
    if (is_root) {
      // under its old parent when it is still in the document
      n.parent = scene.find(src.parent) ? src.parent : 0;
      n.name = unique_name(src.name, names);
    } else {
      n.parent = ids[src.parent];
    }
    if (n.kind == NodeKind::Render && n.mesh) n.extras[kLitHere] = true;
    if (n.kind == NodeKind::Actor) {
      // a copied actor is a new one: the game gives it its id, and names it like the others of
      // its type (<type>-<number>)
      n.extras.erase("aid");
      const std::string etype = n.extras.value("etype", std::string());
      if (is_root && !etype.empty()) {
        names.erase(n.name);
        std::string name;
        for (int i = 1; name.empty() || names.count(name); i++) name = etype + "-" + std::to_string(i);
        n.name = name;
        names.insert(name);
      }
      if (n.extras.contains("lump") && n.extras["lump"].is_object()) n.extras["lump"]["name"] = n.name;
    }
    const uint32_t id = scene.add(std::move(n));
    if (is_root) {
      // the node just added (looking it up by its new id would index the whole scene again)
      scene.set_world(scene.nodes.back(), clip.root_worlds.at(src.id));
      roots.push_back(id);
    }
  }
  return roots;
}

// ------------------------------------------------------------------------------------------------
// library and document
// ------------------------------------------------------------------------------------------------

void App::set_library(const std::string& root) {
  library_root = root.empty() ? std::string() : library_root_of(root);
  store.clear();
  library.clear();
  m_export_originals.clear();
  if (const std::string folder = game_folder(library_root); !folder.empty()) {
    std::string err;
    if (library.scan(folder, &err)) {
      auto data = game_data();
      library.set_listed([&](const LevelEntry& l) { return level_listed(l, data.get()); });
    } else {
      LOG_ERROR("%s", err.c_str());
    }
  }
  library_index.start(library, Prefs::dir() + "index/jak3", !Prefs::read_only);
  m_index_applied = false;
  // the textures of the particles (ogle-extract writes them with the common models)
  renderer.set_sprite_folder(library_root.empty() ? std::string() : library_root + "/jak3/common/sprites");
  thumbs.set_sprite_folder(library_root.empty() ? std::string() : library_root + "/jak3/common/sprites");
  particles.clear();
  palette.clear();
  palette_textures.clear();
  game_textures.clear();
  game_textures_jak2.clear();
  m_tex_level.clear();
  // Jak II's structures, extracted to import them (<library>/jak2): the levels the game has, none of
  // its actors (Jak 3's game data says nothing of them)
  library_jak2.clear();
  if (!library_root.empty()) {
    std::error_code ec;
    const std::string jak2 = library_root + "/jak2";
    std::string err;
    if (fs::exists(to_path(jak2 + "/library.json"), ec) && library_jak2.scan(jak2, &err))
      library_jak2.set_listed([](const LevelEntry& l) { return level_listed(l, nullptr); });
  }
  library_index_jak2.start(library_jak2, Prefs::dir() + "index/jak2", !Prefs::read_only);
  m_index_jak2_applied = false;
  all_parts_jak2.clear();
  rebuild_catalog();
  m_all_parts_shown.clear();
  m_all_parts_filter = "";
  m_decor_level.clear();
  m_decor_scene.reset();
  m_decor_parts.clear();
  m_selected_level.clear();
  refresh_prefabs();
  Prefs& prefs = Prefs::get();
  prefs.library_root = library_root;
  prefs.save();
}

void App::rebuild_catalog() {
  catalog.clear();
  catalog_models.clear();
  all_parts.clear();
  m_all_prototypes = m_all_pieces = 0;
  if (library.empty()) return;
  auto data = game_data();
  const LibraryIndex* idx = library_index.ready() ? &library_index : nullptr;
  catalog = build_catalog(library, data.get(), idx);
  catalog_models = list_catalog_models(library, data.get(), catalog);
  // Jak II's: its models alone (its objects' code is not in Jak 3), placed as decor, but its living
  // things' (data/jak2/creatures.json, from its type table)
  catalog_models_jak2.clear();
  if (!library_jak2.empty()) {
    std::set<std::string> creatures;
    std::ifstream f(to_path(game_data_path()).parent_path().parent_path() / "jak2" / "creatures.json");
    const json c = f ? json::parse(f, nullptr, false) : json();
    if (c.is_object() && c.contains("creatures") && c["creatures"].is_array())
      for (const auto& t : c["creatures"])
        if (t.is_string()) creatures.insert(t.get<std::string>());
    catalog_models_jak2 = list_catalog_models(library_jak2, nullptr, {}, creatures);
  }
  for (auto& m : catalog_models_jak2) m.game = "jak2";
  if (idx) {
    all_parts = all_decor(*idx, library);
    for (const auto& p : all_parts) (p.part.kind == CatalogPart::Kind::Prototype ? m_all_prototypes : m_all_pieces)++;
  }
}

void App::apply_library_index() {
  if (!m_index_jak2_applied && library_index_jak2.ready()) {
    m_index_jak2_applied = true;
    all_parts_jak2 = all_decor(library_index_jak2, library_jak2);
    all_parts_jak2.erase(std::remove_if(all_parts_jak2.begin(), all_parts_jak2.end(),
                                        [](const DecorEntry& e) { return !jak2_structure(e.part); }),
                         all_parts_jak2.end());
    game_textures_jak2 = all_textures(library_index_jak2, library_jak2, "jak2");
  }
  if (m_index_applied || !library_index.ready()) return;
  m_index_applied = true;
  rebuild_catalog();
  palette_textures = recurring_textures(library_index, library);
  game_textures = all_textures(library_index, library);
}

int App::palette_texture(const PaletteTexture& t, bool wait) {
  const Texture* img = palette.image(t);
  std::vector<Texture> read;
  if (!img && wait) {
    std::string err;
    if (read_level_textures(t.background, {t.name}, &read, &err) && !read.empty()) img = &read[0];
  }
  if (!img) return -1;
  if (t.game.empty()) return add_texture_once(scene, *img);
  // Jak II's: named apart from Jak 3's of the same name (as io/jak2_import.h names them)
  Texture copy = *img;
  copy.name = t.game + "-" + copy.name;
  return add_texture_once(scene, copy);
}

std::shared_ptr<const GameData> App::game_data() {
  if (m_game_data_read) return m_game_data;
  m_game_data_read = true;
  auto data = std::make_shared<GameData>();
  std::string err;
  if (data->load(game_data_path(), &err)) m_game_data = data;
  else LOG_WARN(tr("log.game_data_missing"), err.c_str());
  return m_game_data;
}

const GameData* App::document_game_data() { return document_open() ? game_data().get() : nullptr; }

// ------------------------------------------------------------------------------------------------
// story view, ocean
// ------------------------------------------------------------------------------------------------

std::vector<int> App::story_states(const std::string& level) const {
  std::vector<int> out{1};
  const std::vector<int>* changes = &story_steps;
  if (!level.empty()) {
    auto it = level_story_steps.find(level);
    changes = it != level_story_steps.end() ? &it->second : nullptr;
  }
  if (changes) out.insert(out.end(), changes->begin(), changes->end());
  return out;
}

void App::story_document_changed() {
  const GameData* data = document_game_data();
  story_steps = data ? story_changes(scene, *data) : std::vector<int>{};
  // each level added changes at its own steps
  level_story_steps.clear();
  if (data)
    for (const auto& l : document_levels())
      if (l.role == DocumentLevel::Added) level_story_steps[l.name] = story_changes(scene, *data, l.name);
  if (!story_level.empty() && !level_story_steps.count(story_level)) story_level.clear();
  if (data && data->has_story()) story.step = std::clamp(story.step, 1, data->step_count());
  apply_story_view();
  update_ocean();
}

void App::set_story_view(const StoryView& view) {
  story = view;
  if (const GameData* data = document_game_data(); data && data->has_story())
    story.step = std::clamp(story.step, 1, data->step_count());
  apply_story_view();
  update_ocean();
}

void App::apply_story_view() {
  apply_story(scene, document_game_data(), story);
  m_story_revision = history.revision();
  // what the story hides cannot stay selected
  for (auto it = selection.begin(); it != selection.end();) {
    const Node* n = scene.find(*it);
    if (n && scene.effectively_visible(*n)) ++it;
    else it = selection.erase(it);
  }
  if (!selection.count(primary)) primary = selection.empty() ? 0 : *selection.begin();
}

void App::set_ocean_backdrop() {
  rs.far_ocean = m_ocean_far && m_ocean && !scene.nodes.empty();
  rs.far_ocean_y = m_ocean_height;
  rs.far_ocean_color[0] = m_ocean_far_color.x;
  rs.far_ocean_color[1] = m_ocean_far_color.y;
  rs.far_ocean_color[2] = m_ocean_far_color.z;
}

std::vector<uint8_t> ocean_land(const OceanMap& map, float ocean_y, const Scene& land) {
  const int side = map.size * 32;
  const float sq = map.cell / 32.f;
  std::vector<uint8_t> out((size_t)side * side, 0);
  const SceneCache& cache = land.cache();
  for (size_t i = 0; i < land.nodes.size(); i++) {
    const Node& n = land.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh) continue;
    const Mat4& w = cache.worlds[i];
    for (const Primitive& p : n.mesh->prims)
      for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
        const Vec3 a = w.point(p.pos[p.idx[t]]), b = w.point(p.pos[p.idx[t + 1]]), c = w.point(p.pos[p.idx[t + 2]]);
        if (std::max({a.y, b.y, c.y}) <= ocean_y + 0.3f) continue;
        // the squares whose middle it covers, seen from above (a wall covers none)
        const float d = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
        if (std::fabs(d) < 1e-6f) continue;
        auto first = [&](float v, float o) { return std::max(0, (int)std::ceil((v - o) / sq - 0.5f)); };
        auto last = [&](float v, float o) { return std::min(side - 1, (int)std::floor((v - o) / sq - 0.5f)); };
        const int x0 = first(std::min({a.x, b.x, c.x}), map.corner.x), x1 = last(std::max({a.x, b.x, c.x}), map.corner.x);
        const int z0 = first(std::min({a.z, b.z, c.z}), map.corner.z), z1 = last(std::max({a.z, b.z, c.z}), map.corner.z);
        for (int z = z0; z <= z1; z++)
          for (int x = x0; x <= x1; x++) {
            const float px = map.corner.x + (x + 0.5f) * sq, pz = map.corner.z + (z + 0.5f) * sq;
            const float u = ((b.x - px) * (c.z - pz) - (c.x - px) * (b.z - pz)) / d;
            const float v = ((c.x - px) * (a.z - pz) - (a.x - px) * (c.z - pz)) / d;
            if (u < 0 || v < 0 || u + v > 1) continue;
            if (u * a.y + v * b.y + (1 - u - v) * c.y > ocean_y + 0.3f) out[(size_t)z * side + x] = 1;
          }
      }
  }
  return out;
}

std::vector<uint8_t> ocean_water(const OceanMap& map) {
  const int size = map.size;
  const size_t side = (size_t)size * 32;
  std::vector<uint8_t> out(side * side, 0);
  for (int r = 0; r < size; r++)
    for (int c = 0; c < size; c++) {
      const auto it = map.near.find(r * size + c);
      const bool near = it != map.near.end() && it->second.size() >= 32;
      const bool all = it != map.near.end() ? it->second.empty() : map.cells[(size_t)r * size + c] != 0;
      for (int row = 0; row < 32; row++)
        for (int x = 0; x < 32; x++)
          out[((size_t)r * 32 + row) * side + (size_t)c * 32 + x] = all || (near && (it->second[row] >> x & 1u));
    }
  return out;
}

std::vector<uint8_t> ocean_land_around(int side, const std::vector<std::vector<uint8_t>>& doc,
                                       const std::vector<std::vector<uint8_t>>& around,
                                       const std::vector<std::vector<uint8_t>>& far, const std::vector<uint8_t>& water) {
  const size_t count = (size_t)side * side;
  std::vector<uint8_t> own(count, 0), out(count, 0);
  for (const auto& d : doc)
    if (d.size() == count)
      for (size_t i = 0; i < count; i++) own[i] |= d[i];
  for (const auto& a : around)
    if (a.size() == count)
      for (size_t i = 0; i < count; i++) out[i] |= a[i];
  // the document's area: the box of its land
  int x0 = side, x1 = -1, z0 = side, z1 = -1;
  for (int z = 0; z < side; z++)
    for (int x = 0; x < side; x++)
      if (own[(size_t)z * side + x]) {
        x0 = std::min(x0, x), x1 = std::max(x1, x);
        z0 = std::min(z0, z), z1 = std::max(z1, z);
      }
  // the view from afar placed like the document's levels (some are models moved or scaled): the
  // one whose land matches theirs best in their box, if it does
  // ponytail: a match of 0.35, from the city's (0.41 placed, 0.30 scaled, 0.16 moved)
  const std::vector<uint8_t>* best = nullptr;
  double best_match = 0.35;
  for (const auto& f : far) {
    if (f.size() != count) continue;
    size_t both = 0, either = 0;
    for (int z = z0; z <= z1; z++)
      for (int x = x0; x <= x1; x++) {
        const size_t i = (size_t)z * side + x;
        both += own[i] && f[i];
        either += own[i] || f[i];
      }
    if (either && (double)both / either > best_match) {
      best_match = (double)both / either;
      best = &f;
    }
  }
  // of it, the land the document's stands on, as far as it goes (not islands the game does not
  // show from there)
  if (best) {
    std::vector<uint8_t> seen(count, 0);
    std::vector<size_t> todo;
    for (size_t i = 0; i < count; i++)
      if (own[i]) seen[i] = 1, todo.push_back(i);
    while (!todo.empty()) {
      const size_t i = todo.back();
      todo.pop_back();
      if ((*best)[i]) out[i] = 1;
      const int x = (int)(i % side), z = (int)(i / side);
      for (const auto [dx, dz] : {std::pair{1, 0}, {-1, 0}, {0, 1}, {0, -1}}) {
        if (x + dx < 0 || x + dx >= side || z + dz < 0 || z + dz >= side) continue;
        const size_t j = (size_t)(z + dz) * side + x + dx;
        if (!seen[j] && (*best)[j]) seen[j] = 1, todo.push_back(j);
      }
    }
  }
  // in their box, the document's own pieces hide the water, or show it (under a bridge, where the
  // user took a piece away)
  auto clear_box = [&] {
    for (int z = z0; z <= z1; z++) std::fill_n(&out[(size_t)z * side + x0], x1 - x0 + 1, 0);
  };
  clear_box();
  // land thinner than 27 m (a wall in the sea, a bridge) keeps the water around it, a gap as thin
  // in the land (a street of a view from afar) none
  auto grow = [&](bool land) {  // by 4 squares
    std::vector<uint8_t> tmp(count);
    for (int pass = 0; pass < 2; pass++) {
      const std::vector<uint8_t>& from = pass ? tmp : out;
      std::vector<uint8_t>& to = pass ? out : tmp;
      for (int z = 0; z < side; z++)
        for (int x = 0; x < side; x++) {
          bool v = !land;
          for (int k = -4; k <= 4; k++) {
            const int xx = pass ? x : std::clamp(x + k, 0, side - 1), zz = pass ? std::clamp(z + k, 0, side - 1) : z;
            if ((from[(size_t)zz * side + xx] != 0) == land) v = land;
          }
          to[(size_t)z * side + x] = v;
        }
    }
  };
  grow(false), grow(true), grow(true), grow(false);
  clear_box();
  // the water the land closes in (squares of the map's ocean under the city, a canal cut off): none;
  // the sea goes on to the map's edge, or into the document's own box
  if (water.size() == count) {
    std::vector<uint8_t> seen(count, 0);
    std::vector<size_t> todo;
    auto reach = [&](int x, int z) {
      const size_t i = (size_t)z * side + x;
      if (water[i] && !out[i] && !seen[i]) seen[i] = 1, todo.push_back(i);
    };
    for (int k = 0; k < side; k++) reach(k, 0), reach(k, side - 1), reach(0, k), reach(side - 1, k);
    for (int z = z0; z <= z1; z++)
      for (int x = x0; x <= x1; x++) reach(x, z);
    while (!todo.empty()) {
      const int x = (int)(todo.back() % side), z = (int)(todo.back() / side);
      todo.pop_back();
      if (x > 0) reach(x - 1, z);
      if (x < side - 1) reach(x + 1, z);
      if (z > 0) reach(x, z - 1);
      if (z < side - 1) reach(x, z + 1);
    }
    for (size_t i = 0; i < count; i++)
      if (water[i] && !out[i] && !seen[i]) out[i] = 1;
  }
  return out;
}

void App::poll_ocean_land() {
  using namespace std::chrono_literals;
  if (!m_ocean_land_future.valid() || m_ocean_land_future.wait_for(0s) != std::future_status::ready) return;
  m_ocean_land = m_ocean_land_future.get();
  m_ocean_land_key = m_ocean_land_want;
  m_ocean_land_want.clear();
  update_ocean();
}

void App::update_ocean() {
  const GameData* data = document_game_data();
  // like the game: the ocean of a level shown with an ocean map (the level opened first, then the
  // others of the document), the far ocean when one of them asks for it
  const LevelInfo* info = data ? data->level(scene.settings.level) : nullptr;
  std::string ocean_level = scene.settings.level;
  m_ocean_far = false;
  if (data) {
    for (const auto& l : document_levels()) {
      if (l.role == DocumentLevel::Layer) continue;
      if (const Node* g = l.group ? scene.find(l.group) : nullptr; g && !g->visible) continue;
      const LevelInfo* li = data->level(l.name);
      if (!li) continue;
      m_ocean_far |= li->ocean_far;
      if ((!info || info->ocean.empty()) && !li->ocean.empty()) {
        info = li;
        ocean_level = l.name;
      }
    }
  }
  const OceanMap* map = info ? data->ocean(info->ocean) : nullptr;
  if (!map || map->size <= 0) {
    m_ocean.reset();
    m_ocean_map.clear();
    m_ocean_far = false;
    return;
  }
  m_ocean_far_color = map->far_color;
  m_ocean_height = data->ocean_height(ocean_level, story.enabled ? story.step_of(ocean_level) : 1);
  // the land of the levels the game shows around the document's (their neighbors, and the city
  // seen from afar on the same map) hides the water there in the game; read on a worker from the
  // library, with the document's levels as extracted (where they are)
  const auto levels = document_levels();
  auto in_document = [&](const std::string& name) {
    for (const auto& l : levels)
      if (l.name == name) return true;
    return false;
  };
  std::vector<std::string> doc, around, far;
  std::string want = strf("%s@%.2f", info->ocean.c_str(), m_ocean_height);
  auto add = [&](std::vector<std::string>& to, const std::string& name) {
    // a level the game loads under several names (ctywide-ff, ctywide-kg): its library's
    const LevelEntry* e = library.level(name);
    if (!e) e = library.level(name.substr(0, name.find('-')));
    if (!e || (&to != &doc && (in_document(e->name) || in_document(name)))) return;
    for (const auto* v : {&doc, &around, &far})
      if (std::find(v->begin(), v->end(), e->background) != v->end()) return;
    to.push_back(e->background);
    want += (&to == &doc ? " =" : &to == &around ? " +" : " ~") + e->name;
  };
  for (const auto& l : levels)
    if (l.role != DocumentLevel::Layer) add(doc, l.name);
  for (const auto& l : levels)
    if (const LevelInfo* li = l.role != DocumentLevel::Layer ? data->level(l.name) : nullptr)
      for (const auto& n : li->neighbors) add(around, n);
  for (const auto& [name, li] : data->levels())
    if (li.ocean == info->ocean) add(far, name);
  if (want != m_ocean_land_key && want != m_ocean_land_want && !m_ocean_land_future.valid()) {
    m_ocean_land_want = want;
    m_ocean_land_future = std::async(std::launch::async, [doc, around, far, m = *map, y = m_ocean_height]() {
      auto read = [&](const std::vector<std::string>& paths, bool afar) {
        std::vector<std::vector<uint8_t>> out;
        for (const auto& path : paths) {
          Scene s;
          ImportResult r;
          std::string err;
          if (!import_gltf(s, path, ImportOptions{}, &r, &err)) continue;
          // a view from afar has hills kilometers wide around: a backdrop, not the land there
          if (afar)
            for (auto& n : s.nodes)
              if (const AABB b = n.mesh ? s.world_bounds(n) : AABB{}; b.valid() && std::max(b.hi.x - b.lo.x, b.hi.z - b.lo.z) > 2000.f)
                n.mesh.reset();
          out.push_back(ocean_land(m, y, s));
        }
        return out;
      };
      return ocean_land_around(m.size * 32, read(doc, false), read(around, false), read(far, true), ocean_water(m));
    });
  }
  const bool masked = want == m_ocean_land_key && !m_ocean_land.empty();
  const std::string key = masked ? want : info->ocean;
  if (m_ocean && m_ocean_map == key) return;
  m_ocean_map = key;
  // the water: the mid ocean's cells, and the near ocean's water by the shores (3 m squares) where
  // the game draws it instead, less the land around; a quad per run of water along x, at height 0
  // (drawn at the ocean's height)
  auto mesh = std::make_shared<Mesh>();
  mesh->name = "ocean";
  Primitive p;
  const int size = map->size;
  const size_t side = (size_t)size * 32;
  auto quad = [&](float x0, float x1, float z0, float z1) {
    const uint32_t base = (uint32_t)p.pos.size();
    p.pos.insert(p.pos.end(), {{x0, 0, z0}, {x1, 0, z0}, {x1, 0, z1}, {x0, 0, z1}});
    p.nrm.insert(p.nrm.end(), 4, Vec3{0, 1, 0});
    p.idx.insert(p.idx.end(), {base, base + 2, base + 1, base, base + 3, base + 2});
  };
  std::vector<uint8_t> water = ocean_water(*map);
  if (masked)
    for (size_t i = 0; i < water.size(); i++) water[i] &= !m_ocean_land[i];
  // the 32 rows of water squares of a cell (bit x of row z); true when it is water everywhere
  auto rows = [&](int r, int c, uint32_t out[32]) {
    bool whole = true;
    for (int row = 0; row < 32; row++) {
      const uint8_t* w = &water[((size_t)r * 32 + row) * side + (size_t)c * 32];
      uint32_t bits = 0;
      for (int x = 0; x < 32; x++) bits |= (uint32_t)w[x] << x;
      out[row] = bits;
      whole &= bits == ~0u;
    }
    return whole;
  };
  const float sq = map->cell / 32.f;
  uint32_t bits[32];
  for (int r = 0; r < size; r++) {
    for (int c = 0; c < size;) {
      if (!rows(r, c, bits)) {
        // part of the cell: its rows of water squares
        const float cx = map->corner.x + c * map->cell, cz = map->corner.z + r * map->cell;
        for (int row = 0; row < 32; row++)
          for (int x = 0; x < 32;) {
            if (!(bits[row] >> x & 1u)) {
              x++;
              continue;
            }
            const int x0 = x;
            while (x < 32 && (bits[row] >> x & 1u)) x++;
            quad(cx + x0 * sq, cx + x * sq, cz + row * sq, cz + (row + 1) * sq);
          }
        c++;
        continue;
      }
      const int c0 = c;
      while (c < size && rows(r, c, bits)) c++;
      const float z0 = map->corner.z + r * map->cell;
      quad(map->corner.x + c0 * map->cell, map->corner.x + c * map->cell, z0, z0 + map->cell);
    }
  }
  p.material = 0;
  mesh->prims.push_back(std::move(p));
  m_ocean = mesh;
  // its colors: the map's (0x80 is 1.0), lifted, and the far ocean's
  auto lift = [](float v) { return std::min(1.f, v * 1.6f + 0.08f); };
  const float near[3] = {lift(map->color.x), lift(map->color.y), lift(map->color.z)};
  const float deep[3] = {map->far_color.x, map->far_color.y, map->far_color.z};
  for (int i = 0; i < 3; i++) {
    m_ocean_look.color[i] = near[i];
    m_ocean_look.deep[i] = deep[i];
  }
}

WaterLook App::ocean_look(const RenderSettings& view_rs) const {
  WaterLook look = m_ocean_look;
  if (!view_rs.game_view) return look;
  // the game's hour: the sky it reflects, the sun (or the moon) glinting, as bright as its light
  const GameMood m = game_mood(game_view.mood);
  int best = -1;
  float strongest = 0;
  for (int i = 0; i < 3; i++) {
    const float s = m.light_color[i][0] + m.light_color[i][1] + m.light_color[i][2];
    if (s > strongest) {
      strongest = s;
      best = i;
    }
  }
  const float ambient = (m.light_ambient[0] + m.light_ambient[1] + m.light_ambient[2]) / 3.f;
  const float k = std::clamp(ambient + strongest / 6.f, 0.15f, 1.1f);
  for (int c = 0; c < 3; c++) {
    look.sky[c] = m.sky_color[c] / 255.f;
    look.color[c] *= k;
    look.deep[c] *= k;
    if (best >= 0) {
      look.sun_dir[c] = m.light_dir[best][c];
      look.sun_color[c] = std::min(1.f, m.light_color[best][c] * 0.6f);
    } else {
      look.sun_color[c] = 0.f;
    }
  }
  return look;
}

void App::load_level(const LevelEntry& level, bool jak2) {
  if (m_level_future.valid()) {
    LOG_WARN("%s", tr("log.level_busy"));
    return;
  }
  stop_placement();
  m_level_name = jak2 ? "jak2-" + level.name : level.name;
  game_view_on = game_view.ready();  // a level of the game: seen as the game draws it
  AssetLibrary lib = jak2 ? library_jak2 : library;  // the worker reads its own copy
  LevelEntry entry = level;
  auto data = jak2 ? std::shared_ptr<GameData>() : game_data();
  const bool actors = load_actors && !jak2;
  m_level_future = std::async(std::launch::async, [lib, entry, data, actors, jak2]() -> std::unique_ptr<Scene> {
    auto s = std::make_unique<Scene>();
    std::string err;
    if (!open_level(*s, lib, data.get(), entry, &err, actors)) {
      LOG_ERROR("%s: %s", entry.background.c_str(), err.c_str());
      return nullptr;
    }
    if (jak2) {
      // Jak II's: its decor, made Jak 3's, a level of its own
      make_jak2_level(*s, 0, entry.name);
      s->settings.level = s->settings.source_group = "jak2-" + entry.name;
    }
    return s;
  });
  LOG_INFO(tr("log.level_opening"), level.name.c_str());
}

// ------------------------------------------------------------------------------------------------
// several levels
// ------------------------------------------------------------------------------------------------

std::vector<App::DocumentLevel> App::document_levels() const {
  std::vector<DocumentLevel> out;
  if (!document_open()) return out;
  DocumentLevel main;
  main.name = scene.settings.level;
  out.push_back(main);
  std::unordered_map<uint32_t, const Node*> by_id;
  for (const auto& n : scene.nodes) by_id[n.id] = &n;
  auto host_of = [&](const Node& n) -> uint32_t {
    for (auto it = by_id.find(n.parent); it != by_id.end(); it = by_id.find(it->second->parent))
      if (it->second->extras.contains("og_level")) return it->first;
    return 0;
  };
  for (const auto& n : scene.nodes) {
    if (n.kind != NodeKind::Group || n.extras.empty()) continue;
    for (const auto& [key, role] : {std::pair{"og_level", DocumentLevel::Added},
                                    {"og_companion", DocumentLevel::Companion}, {"og_layer", DocumentLevel::Layer}}) {
      auto v = n.extras.find(key);
      if (v == n.extras.end() || !v->is_string()) continue;
      DocumentLevel l;
      l.name = v->get<std::string>();
      l.role = role;
      l.group = n.id;
      l.host = role == DocumentLevel::Added ? 0 : host_of(n);
      l.reference = role == DocumentLevel::Added && n.extras.value("og_reference", false);
      out.push_back(l);
    }
  }
  return out;
}

bool App::level_in_document(const std::string& name) const {
  for (const auto& l : document_levels())
    if (l.name == name && l.role != DocumentLevel::Layer && !l.reference) return true;
  return false;
}

bool App::add_level_to_document(const LevelEntry& level, std::string* error, bool reference, bool jak2) {
  auto fail = [&](const char* key) {
    if (error) *error = strf(tr(key), level.name.c_str());
    return false;
  };
  if (!document_open()) return fail("levels.add.no_document");
  if (m_add_future.valid() || m_level_future.valid()) return fail("levels.add.busy");
  // a level is in the document once; the reference of the level replaced may join a document made
  // from that very level
  const std::string name = jak2 ? "jak2-" + level.name : level.name;
  for (const auto& l : document_levels())
    if (l.name == name && l.role != DocumentLevel::Layer && (reference ? l.reference : !l.reference))
      return fail("levels.add.already");
  if (!reference && level_in_document(name)) return fail("levels.add.already");
  stop_placement();
  m_adding_level = name;
  m_adding_reference = reference;
  AssetLibrary lib = jak2 ? library_jak2 : library;  // the worker reads its own copy
  LevelEntry entry = level;
  auto data = jak2 ? std::shared_ptr<GameData>() : game_data();
  const bool actors = load_actors && !jak2;
  m_add_future = std::async(std::launch::async, [lib, entry, data, actors, reference, jak2, name]() -> std::unique_ptr<Scene> {
    auto s = std::make_unique<Scene>();
    std::string err;
    const uint32_t group = add_level(*s, lib, data.get(), entry, &err, actors);
    if (!group) {
      LOG_ERROR("%s", err.c_str());
      return nullptr;
    }
    if (jak2) {
      make_jak2_level(*s, 0, entry.name);
      Node* g = s->find(group);
      g->name = name;
      g->extras["og_level"] = name;
    }
    if (reference) s->find(group)->extras["og_reference"] = true;
    return s;
  });
  LOG_INFO(tr("log.level_adding"), level.name.c_str());
  return true;
}

void App::poll_level_adding() {
  using namespace std::chrono_literals;
  if (!m_add_future.valid() || m_add_future.wait_for(0s) != std::future_status::ready) return;
  std::unique_ptr<Scene> loaded = m_add_future.get();
  m_levels_added++;
  if (!loaded) {
    m_add_error = strf(tr("log.level_add_failed"), m_adding_level.c_str());
    LOG_ERROR("%s", m_add_error.c_str());
    return;
  }
  m_add_error.clear();
  size_t elements = 0;
  for (const auto& n : loaded->nodes) elements += n.kind == NodeKind::Render || n.kind == NodeKind::Actor;
  NodeChange change(scene);
  scene.append(std::move(*loaded));
  history.push(change.commit(strf(tr("undo.add_level"), m_adding_level.c_str()), [this]() { story_document_changed(); }));
  story_document_changed();
  frame_all();
  LOG_INFO(tr("log.level_added"), m_adding_level.c_str(), elements);
}

bool App::remove_document_level(uint32_t group, std::string* error) {
  const Node* g = scene.find(group);
  if (!g || !g->extras.contains("og_level")) {
    if (error) *error = tr("levels.remove.not_added");
    return false;
  }
  const std::string name = g->name;
  NodeChange change(scene);
  change.save_subtree(group);
  const auto gone = scene.descendants(group);
  scene.remove({group});
  history.push(change.commit(strf(tr("undo.remove_level"), name.c_str()), [this]() { story_document_changed(); }));
  for (auto id : gone) selection.erase(id);
  selection.erase(group);
  if (!selection.count(primary)) primary = selection.empty() ? 0 : *selection.begin();
  story_document_changed();
  LOG_INFO(tr("log.level_removed"), name.c_str());
  return true;
}

AABB App::document_level_bounds(uint32_t group) const {
  const SceneCache& cache = scene.cache();
  std::unordered_map<uint32_t, size_t> index;
  for (size_t i = 0; i < scene.nodes.size(); i++) index[scene.nodes[i].id] = i;
  // the level each node belongs to: the first level group above it (companions and layers go
  // with their own group), else the level opened
  auto level_of = [&](size_t i) -> uint32_t {
    for (size_t cur = i;;) {
      const Node& n = scene.nodes[cur];
      if (n.kind == NodeKind::Group &&
          (n.extras.contains("og_level") || n.extras.contains("og_companion") || n.extras.contains("og_layer")))
        return n.id;
      auto p = n.parent ? index.find(n.parent) : index.end();
      if (p == index.end()) return 0;
      cur = p->second;
    }
  };
  std::vector<AABB> boxes;
  for (size_t i = 0; i < scene.nodes.size(); i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || i >= cache.bounds.size() || !cache.bounds[i].valid()) continue;
    if (level_of(i) == group) boxes.push_back(cache.bounds[i]);
  }
  // where most of the level is: some levels hold a piece far away from the rest
  return focus_box(boxes);
}

std::vector<uint32_t> App::document_level_roots(uint32_t group) const {
  if (group) return scene.find(group) ? std::vector<uint32_t>{group} : std::vector<uint32_t>{};
  std::vector<uint32_t> out;
  for (const auto& n : scene.nodes)
    if (!n.parent && n.kind != NodeKind::NavMesh && !n.extras.contains("og_level")) out.push_back(n.id);
  return out;
}

std::vector<uint32_t> App::own_level_roots() const { return document_level_roots(0); }

AABB App::own_level_bounds() const { return document_level_bounds(0); }

bool App::align_to_replaced(std::string* error) {
  const std::string level = scene.settings.replaces;
  auto fail = [&](const char* key) {
    if (error) *error = strf(tr(key), level.c_str());
    return false;
  };
  if (level.empty()) return fail("replace.none");
  // where the level replaced is: the reference shown (the game's level as it is)
  AABB target;
  for (const auto& l : document_levels())
    if (l.reference && l.name == level) target = document_level_bounds(l.group);
  if (!target.valid()) return fail("replace.show_first");
  const AABB own = own_level_bounds();
  const auto roots = own_level_roots();
  if (!own.valid() || roots.empty()) return fail("replace.empty");
  const Vec3 offset{target.center().x - own.center().x, target.lo.y - own.lo.y, target.center().z - own.center().z};
  NodeChange c(scene);
  for (auto id : roots)
    if (Node* n = scene.find(id)) {
      c.save(id);
      scene.set_world(*n, Mat4::translate(offset) * scene.world(*n));
    }
  history.push(c.commit(strf(tr("undo.align_replaced"), level.c_str())));
  LOG_INFO(tr("log.aligned_replaced"), level.c_str(), offset.x, offset.y, offset.z);
  return true;
}

void App::turn_own_level(float degrees) {
  const AABB own = own_level_bounds();
  const auto roots = own_level_roots();
  if (!own.valid() || roots.empty()) return;
  const Vec3 c = own.center();
  const Mat4 turn = Mat4::translate(c) * Mat4::rotate(Quat::axis_angle({0, 1, 0}, radians(degrees))) * Mat4::translate(-c);
  NodeChange change(scene);
  for (auto id : roots)
    if (Node* n = scene.find(id)) {
      change.save(id);
      scene.set_world(*n, turn * scene.world(*n));
    }
  history.push(change.commit(tr("undo.turn_level")));
}

uint32_t App::level_of_node(uint32_t id) const {
  for (const Node* n = scene.find(id); n; n = n->parent ? scene.find(n->parent) : nullptr)
    if (n->kind == NodeKind::Group && n->extras.contains("og_level")) return n->id;
  return 0;
}

bool App::camera_sees(const Vec3& p) const {
  const Vec3 to = p - camera.pos;
  const float dist = length(to);
  if (dist < 1e-3f) return true;
  if (dist > camera.zfar) return false;
  // a bit tighter than the actual field of view: near the edge, "visible" is not "easy to grab"
  const float cos_fov = std::cos(radians(camera.fov_deg) * 0.5f * 0.85f);
  return dot(to / dist, camera.forward()) > cos_fov;
}

void App::select_level(uint32_t group) {
  const auto roots = document_level_roots(group);
  selection = std::set<uint32_t>(roots.begin(), roots.end());
  primary = roots.empty() ? 0 : roots.front();
  if (!roots.empty()) {
    const Node* g = group ? scene.find(group) : nullptr;
    LOG_INFO(tr("log.level_selected"), g ? g->name.c_str() : scene.settings.level.c_str(), roots.size());
    // its gizmo stands in the middle of the level: without this, it can land off-screen (a level
    // is large, and the camera was looking at only part of it, or away from it)
    const AABB b = document_level_bounds(group);
    if (b.valid() && !camera_sees(b.center())) camera.frame(b);
  }
}

void App::select_assembly(uint32_t id) {
  std::vector<uint32_t> members = assembly_of(find_assemblies(scene), id);
  if (members.empty()) members = {id};  // the ground, a detail of it, an actor: itself
  selection = std::set<uint32_t>(members.begin(), members.end());
  primary = id;
  LOG_INFO(tr("log.assembly_selected"), members.size());
  AABB b;
  for (uint32_t m : members)
    if (int i = scene.index_of(m); i >= 0) b.add(scene.cache().bounds[i]);
  if (b.valid() && !camera_sees(b.center())) camera.frame(b);
}

AABB App::document_junction(uint32_t group_a, uint32_t group_b) const {
  const AABB a = document_level_bounds(group_a).expanded(8.f), b = document_level_bounds(group_b).expanded(8.f);
  AABB out;
  if (!a.valid() || !b.valid()) return out;
  const Vec3 lo{std::max(a.lo.x, b.lo.x), std::max(a.lo.y, b.lo.y), std::max(a.lo.z, b.lo.z)};
  const Vec3 hi{std::min(a.hi.x, b.hi.x), std::min(a.hi.y, b.hi.y), std::min(a.hi.z, b.hi.z)};
  if (lo.x > hi.x || lo.y > hi.y || lo.z > hi.z) return out;
  out.add(lo);
  out.add(hi);
  return out;
}

std::vector<std::string> App::document_neighbors() {
  std::vector<std::string> out;
  const GameData* data = document_game_data();
  if (!data) return out;
  const auto levels = document_levels();
  for (const auto& l : levels) {
    if (l.role == DocumentLevel::Layer) continue;
    const LevelInfo* info = data->level(l.name);
    if (!info) continue;
    for (const auto& n : info->neighbors) {
      const LevelEntry* e = library.level(n);
      if (!e || !e->listed || std::find(out.begin(), out.end(), n) != out.end()) continue;
      bool in = false;
      for (const auto& d : levels) in |= d.name == n && d.role != DocumentLevel::Layer;
      if (!in) out.push_back(n);
    }
  }
  return out;
}

void App::open_project_file(const std::string& path) {
  Scene loaded;
  std::string err;
  if (!open_project(loaded, path, &err)) {
    LOG_ERROR("%s: %s", path.c_str(), err.c_str());
    return;
  }
  new_document();
  scene = std::move(loaded);
  history.clear();
  saved_revision = history.revision();
  project_path = path;
  Prefs::get().add_recent_project(path);
  Prefs::get().save();
  story_document_changed();
  frame_all();
  set_title();
  LOG_INFO(tr("log.project_opened"), path.c_str());
  if (const size_t lit = restore_level_lighting()) LOG_INFO(tr("log.lighting_restored"), lit);
}

std::string App::projects_root() const {
  return library_root.empty() ? default_projects_dir() : from_path(to_path(library_root) / "projects");
}

std::string App::project_folder_name(const std::string& name) {
  // what a folder's name can hold on every system: no separators nor reserved characters, no space
  std::string out;
  for (char c : name) {
    const unsigned char u = (unsigned char)c;
    if (u < 32 || std::strchr("<>:\"/\\|?*", c)) continue;
    out += c == ' ' ? '-' : c;
  }
  while (!out.empty() && (out.back() == '.' || out.back() == '-')) out.pop_back();
  while (!out.empty() && (out.front() == '.' || out.front() == '-')) out.erase(out.begin());
  return out;
}

std::string App::project_name() const {
  if (!project_path.empty()) return from_path(to_path(project_path).stem());
  const std::string base = !scene.settings.replaces.empty() ? scene.settings.replaces : scene.settings.level;
  return base.empty() || base == "custom-level" ? std::string("my-level") : "my-" + base;
}

void App::save(bool save_as) {
  if (save_as || project_path.empty()) {
    // the user names it: its folder in <library>/projects
    m_save_name = project_name();
    m_show_save_as = true;
    return;
  }
  std::string err;
  if (!save_project_files(project_path, &err)) LOG_ERROR(tr("log.save_failed"), err.c_str());
}

bool App::save_project_as(const std::string& name, std::string* error) {
  const std::string folder = project_folder_name(name);
  if (folder.empty()) {
    if (error) *error = tr("save.bad_name");
    return false;
  }
  const fs::path dir = to_path(projects_root()) / to_path(folder);
  std::error_code ec;
  fs::create_directories(dir, ec);
  if (!fs::is_directory(dir, ec)) {
    if (error) *error = "cannot create " + from_path(dir);
    return false;
  }
  return save_project_files(from_path(dir / to_path(folder + ".glb")), error);
}

bool App::save_project_files(const std::string& glb_path, std::string* error) {
  stop_preview();  // the model as it is, not as the preview draws it
  if (!save_project(scene, glb_path, error)) return false;
  // its nav meshes as the game takes them, next to it (none: the file of an earlier save goes)
  const fs::path p = to_path(glb_path);
  const fs::path nav = p.parent_path() / to_path(from_path(p.stem()) + "-nav.json");
  std::error_code ec;
  if (!navmesh_nodes().empty()) {
    std::string err;
    if (!export_navmeshes(from_path(nav), nav_first_aid, &err)) LOG_WARN(tr("log.navmesh_export_failed"), err.c_str());
  } else if (fs::exists(nav, ec)) {
    fs::remove(nav, ec);
  }
  project_path = glb_path;
  saved_revision = history.revision();
  Prefs::get().add_recent_project(glb_path);
  Prefs::get().save();
  set_title();
  LOG_INFO(tr("log.project_saved"), glb_path.c_str());
  return true;
}

// ------------------------------------------------------------------------------------------------
// selection and editing
// ------------------------------------------------------------------------------------------------

void App::select_only(uint32_t id) {
  selection.clear();
  if (id) selection.insert(id);
  primary = id;
}

void App::toggle_select(uint32_t id) {
  if (!id) return;
  if (selection.count(id)) {
    selection.erase(id);
    if (primary == id) primary = selection.empty() ? 0 : *selection.begin();
  } else {
    selection.insert(id);
    primary = id;
  }
}

std::vector<uint32_t> App::selection_roots() const {
  // each selected node, unless one of its ancestors is selected too: a walk up from each, not a
  // test of every pair (thousands of elements are selected with a box)
  const SceneCache& c = scene.cache();
  std::vector<uint32_t> out;
  out.reserve(selection.size());
  for (auto id : selection) {
    const int i = scene.index_of(id);
    if (i < 0) continue;
    bool under_selected = false;
    if ((size_t)i < c.count && c.parent.size() == c.count) {
      for (int32_t p = c.parent[i], guard = 0; p >= 0 && !under_selected && guard < 1000; p = c.parent[p], guard++)
        under_selected = selection.count(scene.nodes[p].id) != 0;
    } else {
      for (const Node* n = &scene.nodes[i]; n->parent && !under_selected; n = scene.find(n->parent)) {
        under_selected = selection.count(n->parent) != 0;
        if (!scene.find(n->parent)) break;
      }
    }
    if (!under_selected) out.push_back(id);
  }
  return out;
}

size_t App::subtree_size(uint32_t id) const {
  const SceneCache& c = scene.cache();
  const int i = scene.index_of(id);
  if (i < 0 || (size_t)i >= c.count) return 0;
  static thread_local std::vector<uint32_t> nodes;
  nodes.clear();
  scene.subtree((size_t)i, nodes);
  return nodes.size() - 1;
}

AABB App::subtree_bounds(uint32_t id, size_t* count) const {
  // down the cache's tree (which node is under which: no walk of the whole scene per element),
  // with the matrices as they are now (a change made this frame counts)
  const SceneCache& c = scene.cache();
  AABB b;
  const int i = scene.index_of(id);
  if (i < 0 || (size_t)i >= c.count) return b;
  struct Item {
    uint32_t index;
    Mat4 world;
  };
  std::vector<Item> stack{{(uint32_t)i, scene.world(scene.nodes[i])}};
  size_t n = 0;
  while (!stack.empty()) {
    const Item it = stack.back();
    stack.pop_back();
    n++;
    const Node& node = scene.nodes[it.index];
    if (node.mesh) {
      b.add(node.mesh->bounds().transformed(it.world));
    } else if (node.kind == NodeKind::Actor) {
      const Vec3 p = it.world.col3(3);
      b.add(p - Vec3{kMarkerRadius, kMarkerRadius, kMarkerRadius});
      b.add(p + Vec3{kMarkerRadius, kMarkerRadius, kMarkerRadius});
    } else {
      b.add(it.world.col3(3));
    }
    for (uint32_t k = c.kid_start[it.index]; k < c.kid_start[it.index + 1]; k++)
      stack.push_back({c.kids[k], it.world * scene.nodes[c.kids[k]].local.matrix()});
  }
  if (count) *count = n - 1;
  return b;
}

Vec3 App::group_center(uint32_t id) const {
  auto it = m_group_centers.find(id);
  const Mat4 world = scene.world(id);
  if (it != m_group_centers.end() && it->second.first == history.revision()) return world.point(it->second.second);
  const SceneCache& c = scene.cache();
  std::vector<AABB> boxes;
  if (const int i = scene.index_of(id); i >= 0 && (size_t)i < c.count) {
    std::vector<uint32_t> nodes;
    scene.subtree((size_t)i, nodes);
    boxes.reserve(nodes.size());
    for (size_t k = 1; k < nodes.size(); k++)
      if (c.bounds[nodes[k]].valid()) boxes.push_back(c.bounds[nodes[k]]);
  }
  const AABB focus = focus_box(boxes);
  const Vec3 center = focus.valid() ? focus.center() : world.col3(3);
  m_group_centers[id] = {history.revision(), world.inverse().point(center)};
  return center;
}

Mat4 App::selection_frame() const {
  // once per frame: the gizmo, the orbit and the panels ask for it
  const uint64_t key = scene.cache().version;
  if (m_selection_frame_key == key && m_selection_frame_sel == selection && m_selection_frame_primary == primary)
    return m_selection_frame;
  auto roots = selection_roots();
  Mat4 frame = Mat4::identity();
  if (!roots.empty()) {
    Vec3 p{0, 0, 0};
    for (auto id : roots) {
      const Vec3 origin = scene.world(id).col3(3);
      if (subtree_size(id) > 64) {
        // a large group (a whole level): the middle of most of it, a stray piece far away left out
        p += group_center(id);
        continue;
      }
      const AABB b = subtree_bounds(id);
      // the node's origin, unless it is far from its geometry (meshes in world space have their
      // origin at 0,0,0): then the center of its box
      p += (b.valid() && !b.expanded(1.f).contains(origin)) ? b.center() : origin;
    }
    p = p / (float)roots.size();
    uint32_t ref = selection.count(primary) ? primary : roots.front();
    Transform t = Transform::from_matrix(scene.world(ref));
    frame = Mat4::translate(p) * Mat4::rotate(t.r);
  }
  m_selection_frame_key = key;
  m_selection_frame_sel = selection;
  m_selection_frame_primary = primary;
  m_selection_frame = frame;
  return frame;
}

PickHit App::pick_surface(const Vec2& mouse_vp) {
  PickFilter f;
  f.actors = false;
  f.include_locked = true;
  return pick(scene, camera.ray(mouse_vp.x, mouse_vp.y, m_vp_w, m_vp_h), f);
}

void App::delete_selection() {
  auto roots = selection_roots();
  if (roots.empty()) return;
  NodeChange c(scene);
  c.save_subtrees(roots);
  scene.remove(roots);
  history.push(c.commit(strf(tr("undo.delete"), roots.size())));
  selection.clear();
  primary = 0;
}

size_t App::restore_level_effects(size_t* left_out) {
  const GameData* data = document_game_data();
  if (left_out) *left_out = 0;
  if (!data) return 0;
  // the draw groups of an extracted level: <level>-tfrag-*, -tie-*, -shrub-*
  auto in_decor = [](const Scene& s, const Node& n) {
    for (const Node* p = n.parent ? s.find(n.parent) : nullptr; p; p = p->parent ? s.find(p->parent) : nullptr)
      if (p->name.find("-tfrag-") != std::string::npos || p->name.find("-tie-") != std::string::npos ||
          p->name.find("-shrub-") != std::string::npos)
        return true;
    return false;
  };
  NodeChange change(scene);
  size_t added = 0;
  for (const auto& level : document_levels()) {
    if (level.reference || (level.role != DocumentLevel::Main && level.role != DocumentLevel::Added)) continue;
    const LevelEntry* entry = library.level(level.name);
    if (!entry) continue;
    Scene original;
    ImportResult r;
    std::string err;
    if (!import_gltf(original, entry->background, ImportOptions{}, &r, &err)) {
      LOG_WARN("%s: %s", entry->background.c_str(), err.c_str());
      continue;
    }
    // what the document has of that level: its actors by name, its decor where it is
    std::unordered_set<std::string> actors;
    std::unordered_map<std::string, AABB> decor;
    uint32_t actors_group = 0;
    for (const auto& n : scene.nodes) {
      if (level_of_node(n.id) != level.group) continue;
      if (n.kind == NodeKind::Actor) actors.insert(n.name);
      else if (n.kind == NodeKind::Render && n.mesh && in_decor(scene, n)) decor[n.name] = scene.world_bounds(n);
      else if (n.kind == NodeKind::Group && n.name == level.name + "-actors") actors_group = n.id;
    }
    if (!actors_group) actors_group = level.group;
    // the decor of the level as the game has it, and whether the document still has each piece
    struct Piece {
      AABB box;
      bool kept;
    };
    std::vector<Piece> pieces;
    for (const auto& n : original.nodes) {
      if (n.kind != NodeKind::Render || !n.mesh || !in_decor(original, n)) continue;
      const AABB was = original.world_bounds(n);
      auto it = decor.find(n.name);
      const bool kept = it != decor.end() && length(it->second.lo - was.lo) < 0.05f && length(it->second.hi - was.hi) < 0.05f;
      pieces.push_back({was, kept});
    }
    const Mat4 into = actors_group ? scene.world(actors_group).inverse() : Mat4::identity();
    for (const auto& n : original.nodes) {
      if (n.kind != NodeKind::Actor || actors.count(n.name)) continue;
      if (!data->is_effect(n.extras.value("etype", std::string()))) continue;
      const Mat4 world = original.world(n);
      const Vec3 at = world.point({0, 0, 0});
      // the decor it comes out of (within 1.5 m): taken out of the level, the effect goes with it
      bool near = false, near_kept = false;
      for (const auto& p : pieces)
        if (p.box.expanded(1.5f).contains(at)) {
          near = true;
          near_kept |= p.kept;
        }
      if (near && !near_kept) {
        if (left_out) (*left_out)++;
        continue;
      }
      Node a = n;
      a.id = 0;
      a.parent = actors_group;
      a.local = Transform::from_matrix(into * world);
      const Transform w = Transform::from_matrix(world);
      a.extras["og_origin"] = {w.t.x, w.t.y, w.t.z, w.r.x, w.r.y, w.r.z, w.r.w};
      scene.add(std::move(a));
      added++;
    }
  }
  if (added) {
    history.push(change.commit(strf(tr("undo.restore_effects"), added)));
    story_document_changed();
  }
  return added;
}

void App::update_effects(float dt) {
  particles.begin_frame();
  if (show_effects && document_open() && !particle_data.empty()) {
    const SceneCache& cache = scene.cache();
    // the actors naming a particle group (part-spawner and its kind: their art-name), found again
    // when the document changes
    if (cache.version != m_effect_cache) {
      m_effect_cache = cache.version;
      m_effect_nodes.clear();
      for (size_t i = 0; i < cache.count && i < scene.nodes.size(); i++) {
        const Node& n = scene.nodes[i];
        if (n.kind != NodeKind::Actor) continue;
        auto lump = n.extras.find("lump");
        if (lump == n.extras.end() || !lump->is_object()) continue;
        auto art = lump->find("art-name");
        if (art == lump->end() || !art->is_string()) continue;
        if (const SpGroup* g = particle_data.group(art->get<std::string>())) m_effect_nodes.push_back({(uint32_t)i, g});
      }
    }
    for (const auto& [i, g] : m_effect_nodes)
      if (i < cache.visible.size() && cache.visible[i]) particles.emit(scene.nodes[i].id, g, cache.worlds[i]);
  }
  particles.update(particle_data, dt, camera.pos, game_view.mood.hour);
}

void App::draw_effects() {
  if (!show_effects) return;
  particles.sprites(m_sprites);
  fog_effects(m_sprites);
  renderer.draw_particles(m_sprites, particle_data.textures());
}

size_t App::restore_level_lighting() {
  // a piece's place in its level: the names from its draw group (<level>-tfrag-*, -tie-*,
  // -shrub-*) down to it, and that level
  auto place = [](const Scene& s, const Node& n, std::string* level) {
    std::string path = n.name;
    for (const Node* p = n.parent ? s.find(n.parent) : nullptr; p; p = p->parent ? s.find(p->parent) : nullptr) {
      path = p->name + "/" + path;
      for (const char* group : {"-tfrag-", "-tie-", "-shrub-"})
        if (const size_t at = p->name.rfind(group);
            at != std::string::npos && at > 0 && (std::string(group) == "-tfrag-" || instance_tree(p->name))) {
          *level = p->name.substr(0, at);
          return path;
        }
    }
    return std::string();
  };
  std::map<std::string, std::vector<size_t>> missing;  // level -> the pieces without their lighting
  for (size_t i = 0; i < scene.nodes.size(); i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || (n.tod && n.tod->fits(*n.mesh))) continue;
    std::string level;
    if (!place(scene, n, &level).empty()) missing[level].push_back(i);
  }
  size_t restored = 0;
  for (const auto& [level, pieces] : missing) {
    const LevelEntry* entry = library.level(level);
    if (!entry) continue;
    Scene original;
    ImportResult r;
    std::string err;
    if (!import_gltf(original, entry->background, ImportOptions{}, &r, &err)) {
      LOG_WARN("%s: %s", entry->background.c_str(), err.c_str());
      continue;
    }
    std::unordered_map<std::string, std::shared_ptr<const TodColors>> by_place;
    for (const auto& n : original.nodes) {
      std::string l;
      if (n.tod) by_place.emplace(place(original, n, &l), n.tod);
    }
    std::unordered_map<std::string, const Node*> originals;
    for (const auto& n : original.nodes) {
      std::string l;
      if (n.tod) originals.emplace(place(original, n, &l), &n);
    }
    std::set<int> materials_done;
    for (size_t i : pieces) {
      Node& n = scene.nodes[i];
      std::string l;
      auto it = by_place.find(place(scene, n, &l));
      if (it != by_place.end() && it->second->fits(*n.mesh)) {
        n.tod = it->second;
        restored++;
        // and how the game draws its materials, as extracted now (a wall blended with its depth
        // written, a see-through surface), each material once
        const Node* o = originals.at(it->first);
        for (size_t p = 0; p < n.mesh->prims.size() && p < o->mesh->prims.size(); p++) {
          const int dm = n.mesh->prims[p].material, om = o->mesh->prims[p].material;
          if (dm < 0 || dm >= (int)scene.materials.size() || om < 0 || om >= (int)original.materials.size() ||
              !materials_done.insert(dm).second)
            continue;
          Material& d = scene.materials[dm];
          const Material& was = original.materials[om];
          if (d.name != was.name || d.blend != was.blend) continue;
          d.translucent = was.translucent;
          if (d.alpha == AlphaMode::Blend && was.alpha == AlphaMode::Mask) d.alpha = AlphaMode::Mask;
        }
      }
    }
  }
  return restored;
}

void App::copy_selection() {
  auto roots = selection_roots();
  if (roots.empty()) return;
  clipboard = copy_nodes(scene, roots);
  LOG_INFO(tr("log.copied"), roots.size());
}

void App::relight_moved(NodeChange& change, const std::vector<uint32_t>& roots) {
  const std::vector<uint32_t> lit = lit_here_nodes(scene, roots);
  if (lit.empty()) return;
  for (auto id : lit) change.save(id);
  light_like_surroundings(scene, lit);
}

void App::relight_selection() {
  const auto roots = selection_roots();
  if (roots.empty()) return;
  NodeChange c(scene);
  c.save_subtrees(roots);
  const size_t lit = light_like_surroundings(scene, roots);
  if (lit) history.push(c.commit(tr("undo.relight")));
  LOG_INFO(tr("log.relit"), lit);
}

void App::paste() {
  if (clipboard.empty()) return;
  NodeChange c(scene);
  auto roots = paste_nodes(scene, clipboard);
  if (roots.empty()) return;
  history.push(c.commit(strf(tr("undo.paste"), roots.size())));
  selection = std::set<uint32_t>(roots.begin(), roots.end());
  primary = roots.front();
}

void App::duplicate_selection() {
  auto roots = selection_roots();
  if (roots.empty()) return;
  NodeChange c(scene);
  auto copies = paste_nodes(scene, copy_nodes(scene, roots));
  history.push(c.commit(strf(tr("undo.duplicate"), copies.size())));
  selection = std::set<uint32_t>(copies.begin(), copies.end());
  primary = copies.front();
}

// ------------------------------------------------------------------------------------------------
// prefabs
// ------------------------------------------------------------------------------------------------

std::string App::prefab_folder() const {
  return !m_prefab_dir_override.empty() ? m_prefab_dir_override : library.prefab_folder();
}

void App::refresh_prefabs() {
  const std::string folder = prefab_folder();
  prefabs = folder.empty() ? std::vector<PrefabInfo>{} : list_prefabs(folder);
  // the game's prefabs made so far, in the order of their recipes (Jak II's when it was extracted)
  {
    std::string err;
    prefab_recipes = load_prefab_recipes(prefab_recipes_path(), &err);
    if (!library_jak2.empty())
      for (auto& r : load_prefab_recipes(prefab_recipes_path("jak2"), &err, "jak2")) prefab_recipes.push_back(std::move(r));
  }
  game_prefabs.clear();
  if (folder.empty()) return;
  std::error_code ec;
  for (const auto& r : prefab_recipes) {
    const std::string path = folder + "/game/" + r.file;
    PrefabInfo p;
    p.name = r.name;
    p.path = path;
    p.made = fs::is_regular_file(to_path(path), ec);
    if (p.made) p.mtime = (int64_t)fs::last_write_time(to_path(path), ec).time_since_epoch().count();
    p.category = r.category;
    p.description = r.description;
    p.game = r.game;
    p.level = r.level;
    game_prefabs.push_back(std::move(p));
  }
}

void App::want_game_prefabs(const std::string& game, const std::string& level) {
  m_prefab_levels_wanted.insert({game, level});
}

void App::start_game_prefabs(bool rebuild, const std::set<std::pair<std::string, std::string>>& levels) {
  const std::string folder = prefab_folder();
  if (m_game_prefab_work.valid() || library.empty() || folder.empty()) return;
  refresh_prefabs();
  std::error_code ec;
  std::vector<PrefabRecipe> todo;
  for (const auto& r : prefab_recipes)
    if ((levels.empty() || levels.count({r.game, r.level})) &&
        (rebuild || !fs::is_regular_file(to_path(folder + "/game/" + r.file), ec)))
      todo.push_back(r);
  if (todo.empty()) return;
  m_game_prefab_total = (int)todo.size();
  auto done = m_game_prefab_done;
  *done = 0;
  AssetLibrary lib = library, lib2 = library_jak2;  // the worker reads its own copies
  auto data = game_data();
  LOG_INFO(tr("log.game_prefabs_started"), todo.size());
  m_game_prefab_work = std::async(std::launch::async, [lib, lib2, data, todo, folder, done]() {
    // each level opened once (without its actors) for all its recipes
    std::map<std::pair<std::string, std::string>, std::vector<const PrefabRecipe*>> by_level;
    for (const auto& r : todo) by_level[{r.game, r.level}].push_back(&r);
    size_t built = 0;
    for (const auto& [key, recipes] : by_level) {
      const auto& [game, name] = key;
      const bool jak2 = game == "jak2";
      Scene level;
      std::string err;
      const LevelEntry* entry = (jak2 ? lib2 : lib).level(name);
      const bool opened = entry && open_level(level, jak2 ? lib2 : lib, jak2 ? nullptr : data.get(), *entry, &err, false);
      if (opened && jak2) {
        // Jak II's textures and materials named apart from Jak 3's, where its pieces come from kept
        for (auto& t : level.textures)
          if (t.name.rfind("jak2-", 0) != 0) t.name = "jak2-" + t.name;
        for (auto& m : level.materials)
          if (m.name.rfind("jak2-", 0) != 0) m.name = "jak2-" + m.name;
        for (auto& n : level.nodes)
          if (n.kind == NodeKind::Render && n.mesh)
            n.extras["og_import"] = json{{"game", "jak2"}, {"level", name}, {"part", n.name}, {"checks", json::array()}};
      }
      // as the editor shows it at the start of the story (one state of what changes: a facade whole
      // or blasted, not both)
      if (opened && !jak2) apply_story(level, data.get(), StoryView{});
      LevelAssemblies assemblies;  // found once for all the recipes of the level
      for (const PrefabRecipe* r : recipes) {
        size_t elements = 0;
        if (opened && save_recipe_prefab(level, *r, folder + "/game/" + r->file, &elements, &err, &assemblies)) built++;
        else LOG_WARN("%s: %s", r->name.c_str(), err.empty() ? name.c_str() : err.c_str());
        (*done)++;
      }
    }
    return built;
  });
}

void App::poll_game_prefabs() {
  using namespace std::chrono_literals;
  if (!m_game_prefab_work.valid()) {
    // the levels whose prefabs were wanted meanwhile, all at once
    if (!m_prefab_levels_wanted.empty()) {
      const auto levels = std::move(m_prefab_levels_wanted);
      m_prefab_levels_wanted.clear();
      start_game_prefabs(false, levels);
      if (!m_game_prefab_work.valid()) m_game_prefab_runs++;  // nothing to make: they are there
    }
    return;
  }
  if (m_game_prefab_work.wait_for(0s) != std::future_status::ready) return;
  const size_t built = m_game_prefab_work.get();
  m_game_prefab_runs++;
  refresh_prefabs();
  LOG_INFO(tr("log.game_prefabs_done"), built, (size_t)m_game_prefab_total);
  // clicked before it was made: placed now
  if (!m_prefab_place_when_made.empty()) {
    for (const auto& p : game_prefabs)
      if (p.name == m_prefab_place_when_made && p.made) {
        start_placement(p.path, p.name, false);
        m_prefab_place_when_made.clear();
      }
  }
}

std::string App::create_prefab(const std::string& name, float min_size, std::vector<std::string>* left_out) {
  auto roots = without_small(selection_roots(), min_size, left_out);
  if (roots.empty()) return {};
  const std::string folder = prefab_folder();
  if (folder.empty()) {
    LOG_WARN("%s", tr("log.prefab_no_library"));
    return {};
  }
  const std::string path = new_prefab_path(folder, name);
  std::string err;
  if (!save_prefab(scene, roots, path, &err)) {
    LOG_ERROR(tr("log.prefab_failed"), err.c_str());
    return {};
  }
  refresh_prefabs();
  LOG_INFO(tr("log.prefab_saved"), from_path(to_path(path).stem()).c_str(), roots.size());
  return path;
}

const Scene* App::prefab_scene(const std::string& path) {
  auto& s = m_prefab_scenes[path];
  if (!s) {
    auto loaded = std::make_unique<Scene>();
    std::string err;
    if (!load_prefab(path, loaded.get(), &err)) {
      LOG_ERROR("%s: %s", path.c_str(), err.c_str());
      m_prefab_scenes.erase(path);
      return nullptr;
    }
    s = std::move(loaded);
  }
  return s.get();
}

std::string Placement::key() const {
  switch (kind) {
    case Kind::Object:
    case Kind::Model: return "model:" + path;
    case Kind::Part: return "part:" + path + "#" + part.mesh_node;
    case Kind::Shape: return "shape:" + shape.to_json().dump() + "#" + std::to_string(material);
    default: return path;
  }
}

void App::start_placement(const std::string& prefab_path, const std::string& name, bool once) {
  placement = Placement{};
  placement.path = prefab_path;
  placement.name = name;
  placement.once = once;
  tool = ToolMode::Place;
  store.model(prefab_path, true);  // the preview appears as soon as the file is read
}

void App::start_object_placement(const CatalogObject& object, bool once) {
  placement = Placement{};
  placement.kind = Placement::Kind::Object;
  placement.path = object.model_path;
  placement.name = object.name;
  placement.object = object;
  placement.once = once;
  tool = ToolMode::Place;
  store.model(object.model_path, true);
}

void App::start_model_placement(const CatalogModel& model, bool once) {
  placement = Placement{};
  placement.kind = Placement::Kind::Model;
  placement.path = model.path;
  placement.name = model.name;
  placement.game = model.game;
  placement.level = model.group;
  placement.once = once;
  tool = ToolMode::Place;
  store.model(model.path, true);
}

void App::start_part_placement(const std::string& level_background, const CatalogPart& part, bool once) {
  placement = Placement{};
  placement.kind = Placement::Kind::Part;
  placement.path = level_background;
  placement.name = part.name;
  placement.part = part;
  placement.game = library_jak2.level_of(level_background) ? "jak2" : "";
  placement.once = once;
  tool = ToolMode::Place;
  store.level(level_background, true);
}

void App::start_shape_placement(const ShapePreset& preset, bool once) {
  placement = Placement{};
  placement.kind = Placement::Kind::Shape;
  placement.name = preset.name;
  placement.shape = preset.params;
  placement.once = once;
  tool = ToolMode::Place;
}

std::shared_ptr<Mesh> App::placement_mesh() {
  std::shared_ptr<Mesh>& mesh = m_prefab_previews[placement.key()];
  if (mesh) return mesh;
  switch (placement.kind) {
    case Placement::Kind::Prefab:
    case Placement::Kind::Object:
    case Placement::Kind::Model:
      if (placement.path.empty()) break;  // an object drawn without a model: a marker
      if (auto lm = store.model(placement.path, true)) {
        if (placement.game == "jak2") {
          // a model of Jak II: its textures renamed, what could break checked (kept with the mesh)
          json checks;
          mesh = import_jak2_mesh(scene, *lm->mesh, lm->textures, lm->materials, &checks);
          mesh->extras["og_import_checks"] = checks;
          break;
        }
        mesh = merge_mesh(scene, *lm->mesh, lm->textures, lm->materials);
        if (placement.kind == Placement::Kind::Object) {
          const GameData* data = game_data().get();
          if (data && data->is_water(placement.object.etype)) style_water_mesh(scene, *mesh, placement.object.model);
        }
      }
      break;
    case Placement::Kind::Part: {
      std::shared_ptr<Scene> level = m_decor_scene && library.level_of(placement.path) &&
                                             library.level_of(placement.path)->name == m_decor_level
                                         ? m_decor_scene
                                         : store.level(placement.path, true);
      if (!level) break;
      if (auto part = part_mesh(*level, placement.part)) {
        if (placement.game == "jak2") {
          // a structure of Jak II: its textures renamed, what could break checked (kept with the mesh)
          json checks;
          mesh = import_jak2_mesh(scene, *part, level->textures, level->materials, &checks);
          mesh->extras["og_import_checks"] = checks;
        } else {
          mesh = merge_mesh(scene, *part, level->textures, level->materials);
        }
      }
      break;
    }
    case Placement::Kind::Shape:
      mesh = make_shape(placement.shape, placement.material >= 0 ? placement.material : shape_default_material(scene));
      break;
  }
  return mesh;
}

std::vector<uint32_t> App::place_at(const Mat4& m) {
  std::vector<uint32_t> roots;
  if (placement.kind == Placement::Kind::Prefab) {
    const Scene* prefab = prefab_scene(placement.path);
    if (!prefab) return roots;
    NodeChange c(scene);
    roots = place_prefab(scene, *prefab, m, &m_prefab_meshes[placement.path]);
    if (roots.empty()) return roots;
    // its elements stay together: a group named after the prefab (a double click selects it all)
    const uint32_t gid = new_group_id(scene);
    for (auto id : roots)
      if (Node* n = scene.find(id)) set_group_tag(*n, gid, placement.name, true);
    // lit like the level around it (a batch: once, at its end)
    if (m_batch_change) m_batch_placed.insert(m_batch_placed.end(), roots.begin(), roots.end());
    else light_like_surroundings(scene, roots);
    if (!m_batch_change) history.push(c.commit(strf(tr("undo.place"), placement.name.c_str())));
  } else {
    std::shared_ptr<Mesh> mesh = placement_mesh();
    const bool markerless = placement.kind == Placement::Kind::Object && placement.path.empty();
    if (!mesh && !markerless) return roots;  // still loading
    std::unordered_set<std::string> names;
    for (const auto& n : scene.nodes) names.insert(n.name);
    NodeChange c(scene);
    if (placement.kind == Placement::Kind::Object) {
      // an actor named like the game names them: <type>-<number>
      std::string name;
      for (int i = 1; name.empty() || names.count(name); i++) name = placement.object.etype + "-" + std::to_string(i);
      Node actor = make_catalog_actor(placement.object, name);
      actor.local = Transform::from_matrix(m);
      const uint32_t id = scene.add(std::move(actor));
      if (mesh) {
        Node visual;
        visual.parent = id;
        visual.name = placement.object.model;
        visual.kind = NodeKind::Render;
        visual.mesh = mesh;
        visual.extras = json{{"og_actor_visual", placement.object.model}};
        scene.add(std::move(visual));
      }
      roots.push_back(id);
    } else if (placement.kind == Placement::Kind::Model) {
      Node n;
      n.name = unique_name(placement.game == "jak2" ? "jak2-" + placement.name : placement.name, names);
      n.kind = NodeKind::Render;
      n.mesh = mesh;
      n.extras = json{{"og_model", placement.name}};  // which model (its animations: the preview)
      if (placement.game == "jak2")
        n.extras = json{{"og_import", {{"game", "jak2"},
                                       {"level", placement.level},
                                       {"part", placement.name},
                                       {"checks", mesh->extras.value("og_import_checks", json::array())}}}};
      n.local = Transform::from_matrix(m);
      roots.push_back(scene.add(std::move(n)));
    } else if (placement.kind == Placement::Kind::Shape) {
      // its own mesh: its size changes apart from the others'
      Node n;
      n.name = unique_name(placement.name, names);
      n.kind = NodeKind::Render;
      n.mesh = std::make_shared<Mesh>(*mesh);
      n.extras = json{{"og_shape", placement.shape.to_json()}};
      n.local = Transform::from_matrix(m);
      roots.push_back(scene.add(std::move(n)));
    } else {
      std::string base = placement.part.name;
      if (base.size() > 3 && base.compare(base.size() - 3, 3, ".mb") == 0) base.resize(base.size() - 3);
      Node n;
      n.kind = NodeKind::Render;
      n.mesh = mesh;
      n.local = Transform::from_matrix(m);
      if (placement.game == "jak2") {
        // named apart from Jak 3's parts of the same name; where it comes from and what was checked
        base = "jak2-" + base;
        const LevelEntry* from = library_jak2.level_of(placement.path);
        n.extras = json{{"og_import", {{"game", "jak2"},
                                       {"level", from ? from->name : std::string()},
                                       {"part", placement.part.name},
                                       {"checks", mesh->extras.value("og_import_checks", json::array())}}}};
      }
      n.name = unique_name(base, names);
      roots.push_back(scene.add(std::move(n)));
    }
    // lit like the level around it (not an actor's model: the game lights it); a batch: at its end
    if (m_batch_change) m_batch_placed.insert(m_batch_placed.end(), roots.begin(), roots.end());
    else light_like_surroundings(scene, roots);
    if (!m_batch_change) history.push(c.commit(strf(tr("undo.place"), placement.name.c_str())));
  }
  selection = std::set<uint32_t>(roots.begin(), roots.end());
  primary = roots.front();
  return roots;
}

void App::new_level() {
  new_document();
  // a document without a level of the game: no story, no ocean
  scene.settings.level = "custom-level";
  // nothing of the game's to light it: the editor's own view (the game rendering would show it all
  // blue, under the sky)
  game_view_on = false;
  story_document_changed();
  set_title();
  LOG_INFO("%s", tr("log.new_level"));
}

void App::stop_placement() {
  tool = ToolMode::Select;
  placement = Placement{};
  m_place_valid = false;
  m_place_drag = false;
}

Mat4 App::placement_matrix(const PickHit& hit, const Ray& ray) const {
  Vec3 p;
  if (hit.hit()) {
    p = hit.point;
  } else {
    float t;
    if (ray_plane(ray, {0, 0, 0}, {0, 1, 0}, &t) && t > 0 && t < 2000.f) p = ray.at(t);
    else p = ray.at(15.f);
  }
  if (gizmo.snap.enabled && gizmo.snap.translate > 0) {
    const float s = gizmo.snap.translate;
    p.x = std::round(p.x / s) * s;
    p.z = std::round(p.z / s) * s;
  }
  return Mat4::translate(p) * Mat4::rotate(placement.tilt) * Mat4::rotate(Quat::axis_angle({0, 1, 0}, placement.yaw));
}

// ------------------------------------------------------------------------------------------------
// native file dialogs (SDL3). The callback may run on another thread: results are queued.
// ------------------------------------------------------------------------------------------------

struct DialogRequest {
  App* app;
  DialogAction action;
};

void dialog_callback(void* userdata, const char* const* filelist, int /*filter*/) {
  auto* req = (DialogRequest*)userdata;
  if (filelist && filelist[0]) {
    std::lock_guard<std::mutex> lock(req->app->m_dialog_mutex);
    req->app->m_dialog_results.push_back({req->action, filelist[0]});
  }
  delete req;
}

void App::open_dialog(DialogAction action) {
  static const SDL_DialogFileFilter project_filter[] = {{"glTF (.glb)", "glb"}};
  static const SDL_DialogFileFilter iso_filter[] = {{"Disc image (.iso)", "iso"}};
  auto* req = new DialogRequest{this, action};
  // where each dialog starts: this editor's own tree (never jak-project's), unless the document
  // or a preference already points somewhere the user chose
  std::error_code ec;
  const std::string projects_dir = [&] {
    // the projects' folder (each project in a folder of its own there)
    fs::create_directories(to_path(projects_root()), ec);
    return projects_root();
  }();
  const std::string iso_dir = [&] {
    if (!extract.detected_source.empty()) return from_path(to_path(extract.detected_source).parent_path());
    const std::string& source = Prefs::get().extract_source;
    if (!source.empty()) return from_path(to_path(source).parent_path());
    fs::create_directories(to_path(default_iso_data_dir()), ec);
    return default_iso_data_dir();
  }();
  switch (action) {
    case DialogAction::OpenProject:
      SDL_ShowOpenFileDialog(dialog_callback, req, m_window, project_filter, 1, projects_dir.c_str(), false);
      break;
    case DialogAction::ExtractSourceIso:
      SDL_ShowOpenFileDialog(dialog_callback, req, m_window, iso_filter, 1, iso_dir.c_str(), false);
      break;
    case DialogAction::ExtractSourceFolder:
      SDL_ShowOpenFolderDialog(dialog_callback, req, m_window, iso_dir.c_str(), false);
      break;
    case DialogAction::ExtractOutput: {
      const std::string out_dir = library_root.empty() ? default_library_root() : library_root;
      fs::create_directories(to_path(out_dir), ec);
      SDL_ShowOpenFolderDialog(dialog_callback, req, m_window, out_dir.c_str(), false);
      break;
    }
    case DialogAction::ExportFolder:
      fs::create_directories(to_path(export_options.folder), ec);
      SDL_ShowOpenFolderDialog(dialog_callback, req, m_window, export_options.folder.c_str(), false);
      break;
    case DialogAction::ExportNavMeshes: {
      static const SDL_DialogFileFilter nav_filter[] = {{"nav_data (.json)", "json"}};
      SDL_ShowSaveFileDialog(dialog_callback, req, m_window, nav_filter, 1, projects_dir.c_str());
      break;
    }
    case DialogAction::ImportModel: {
      static const SDL_DialogFileFilter model_filter[] = {{"glTF (.glb, .gltf)", "glb;gltf"}};
      SDL_ShowOpenFileDialog(dialog_callback, req, m_window, model_filter, 1, projects_dir.c_str(), false);
      break;
    }
    case DialogAction::TextureImage: {
      static const SDL_DialogFileFilter image_filter[] = {{"Images (.png, .jpg, .tga, .bmp)", "png;jpg;jpeg;tga;bmp"}};
      SDL_ShowOpenFileDialog(dialog_callback, req, m_window, image_filter, 1, projects_dir.c_str(), false);
      break;
    }
  }
}

void App::process_dialog_results() {
  std::vector<DialogResult> results;
  {
    std::lock_guard<std::mutex> lock(m_dialog_mutex);
    results.swap(m_dialog_results);
  }
  for (auto& r : results) {
    switch (r.action) {
      case DialogAction::OpenProject:
        open_project_file(r.path);
        break;
      case DialogAction::ExtractSourceIso:
      case DialogAction::ExtractSourceFolder:
        extract.source = r.path;
        extract_detect(r.path);
        break;
      case DialogAction::ExtractOutput:
        extract.output = r.path;
        break;
      case DialogAction::ExportFolder:
        export_options.folder = r.path;
        break;
      case DialogAction::ExportNavMeshes: {
        std::string p = r.path, err;
        if (lower_ext(p) != ".json") p += ".json";
        if (!export_navmeshes(p, nav_first_aid, &err)) LOG_ERROR(tr("log.navmesh_export_failed"), err.c_str());
        break;
      }
      case DialogAction::ImportModel:
        start_model_placement(CatalogModel{from_path(to_path(r.path).stem()), r.path, ""}, false);
        break;
      case DialogAction::TextureImage: {
        std::string err;
        const int ti = texture_from_image(scene, r.path, &err);
        if (ti < 0) {
          LOG_ERROR(tr("log.texture_failed"), r.path.c_str(), err.c_str());
          break;
        }
        NodeChange c(scene);
        for (auto id : m_texture_target.ids) c.save(id);
        if (edit_material(scene, m_texture_target.ids, m_texture_target.material, [ti](Material& m) { m.texture = ti; }))
          history.push(c.commit(tr("undo.texture")));
        break;
      }
    }
  }
}

void App::confirm_discard(std::function<void()> action) {
  if (history.revision() == saved_revision || !document_open()) {
    action();
    return;
  }
  m_pending_discard = std::move(action);
  m_show_discard = true;
}

void App::request_quit() {
  if (m_quit_confirmed) {
    m_running = false;
    return;
  }
  confirm_discard([this]() {
    m_quit_confirmed = true;
    m_running = false;
  });
}

void App::set_language(const std::string& code) {
  if (!i18n_select(code)) return;
  Prefs::get().language = code;
  Prefs::get().save();
}

}  // namespace ogle

namespace ogle {

// ------------------------------------------------------------------------------------------------
// nav meshes
// ------------------------------------------------------------------------------------------------

std::vector<uint32_t> App::navmesh_nodes() const {
  std::vector<uint32_t> out;
  for (const auto& n : scene.nodes)
    if (n.kind == NodeKind::NavMesh) out.push_back(n.id);
  return out;
}

AABB App::navmesh_region_of_selection() const {
  const AABB b = selection_bounds();
  return b.valid() ? b.expanded(1.f) : b;
}

std::vector<Vec3> App::navmesh_points_of_selection() const {
  std::vector<Vec3> out;
  for (auto id : selection) {
    const Node* n = scene.find(id);
    if (!n || n->kind == NodeKind::NavMesh) continue;
    const AABB b = subtree_bounds(id);
    if (b.valid()) out.push_back({b.center().x, b.lo.y, b.center().z});
  }
  return out;
}

AABB App::selection_bounds() const {
  AABB b;
  for (auto id : selection_roots()) b.add(subtree_bounds(id));
  return b;
}

bool App::start_navmesh_generation(const AABB& region_in, std::string* error, std::vector<Vec3> keep_from) {
  if (m_nav_future.valid()) {
    if (error) *error = tr("navmesh.busy");
    return false;
  }
  if (!document_open()) {
    if (error) *error = tr("navmesh.no_document");
    return false;
  }
  // the walkable surfaces: the decor shown, in world space, without the see-through surfaces
  // (water, glows: navmesh_ignores) nor the objects (their models move, open, break)
  const SceneCache& cache = scene.cache();
  const AABB reach = region_in.valid() ? region_in.expanded(nav_params.agent_radius + 2.f) : AABB{};
  auto verts = std::make_shared<std::vector<Vec3>>();
  auto tris = std::make_shared<std::vector<uint32_t>>();
  AABB all;
  for (size_t i = 0; i < scene.nodes.size(); i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::Render || !n.mesh || !cache.visible[i] || cache.actor[i] || cache.reference[i]) continue;
    const AABB& nb = cache.bounds[i];
    if (reach.valid() && (nb.hi.x < reach.lo.x || nb.lo.x > reach.hi.x || nb.hi.z < reach.lo.z ||
                          nb.lo.z > reach.hi.z || nb.hi.y < reach.lo.y || nb.lo.y > reach.hi.y))
      continue;
    const Mat4& w = cache.worlds[i];
    for (const auto& p : n.mesh->prims) {
      if (p.material >= 0 && p.material < (int)scene.materials.size() && navmesh_ignores(scene.materials[p.material]))
        continue;
      const uint32_t base = (uint32_t)verts->size();
      for (const auto& v : p.pos) verts->push_back(w.point(v));
      for (auto ix : p.idx) tris->push_back(base + ix);
      all.add(nb);
    }
  }
  if (tris->empty()) {
    if (error) *error = tr("navmesh.nothing");
    return false;
  }
  AABB region = region_in.valid() ? region_in : all;
  NavParams params = nav_params;
  params.keep_from = std::move(keep_from);
  m_nav_future = std::async(std::launch::async, [verts, tris, region, params]() {
    NavJob job;
    job.region = region;
    job.ok = build_navmeshes(*verts, *tris, region, params, &job.meshes, &job.stats, &job.error);
    return job;
  });
  LOG_INFO(tr("log.navmesh_started"), tris->size() / 3);
  return true;
}

void App::poll_navmesh_generation() {
  using namespace std::chrono_literals;
  if (!m_nav_future.valid() || m_nav_future.wait_for(0s) != std::future_status::ready) return;
  NavJob job = m_nav_future.get();
  if (!job.ok) {
    LOG_WARN(tr("log.navmesh_failed"), job.error.c_str());
    m_nav_last = std::move(job);
    m_nav_generation++;
    return;
  }
  // the new meshes replace those of the region
  NodeChange c(scene);
  std::unordered_set<uint32_t> doomed;
  const AABB within = job.region.expanded(1.f);
  for (auto id : navmesh_nodes())
    if (within.contains(scene.world_bounds(*scene.find(id)).center())) doomed.insert(id);
  for (auto id : doomed) c.save_subtree(id);
  remove_subtrees(scene, doomed);
  std::unordered_set<std::string> names;
  for (const auto& n : scene.nodes) names.insert(n.name);
  std::vector<uint32_t> added;
  float area = 0;
  for (const auto& m : job.meshes) {
    area += m.area();
    added.push_back(scene.add(navmesh_node(m, unique_name("navmesh", names))));
  }
  history.push(c.commit(strf(tr("undo.navmesh"), added.size())));
  selection = std::set<uint32_t>(added.begin(), added.end());
  primary = added.empty() ? 0 : added.front();
  show_navmesh = true;
  LOG_INFO(tr("log.navmesh_done"), job.stats.meshes, job.stats.polys, area, job.stats.ms / 1000.0);
  job.meshes.clear();  // in the document now
  m_nav_last = std::move(job);
  m_nav_generation++;
}

void App::world_navmeshes(std::vector<NavMeshData>* meshes, std::vector<std::string>* names) const {
  // each mesh where it is now (moved, turned), relative to the center of its corners
  for (auto id : navmesh_nodes()) {
    const Node* n = scene.find(id);
    NavMeshData m;
    if (!n || !navmesh_of_node(*n, &m)) continue;
    const Mat4 w = scene.world(*n);
    AABB b;
    for (auto& v : m.verts) b.add(v = w.point(v));
    m.origin = b.center();
    for (auto& v : m.verts) v = v - m.origin;
    meshes->push_back(std::move(m));
    names->push_back(n->name);
  }
}

bool App::export_navmeshes(const std::string& path, uint32_t first_aid, std::string* error) {
  std::vector<NavMeshData> meshes;
  std::vector<std::string> names;
  world_navmeshes(&meshes, &names);
  if (meshes.empty()) {
    if (error) *error = tr("navmesh.none");
    return false;
  }
  std::vector<NavExportEntry> entries;
  for (size_t i = 0; i < meshes.size(); i++) entries.push_back({&meshes[i], first_aid + (uint32_t)i, names[i]});
  if (!write_navmesh_blob(entries, path, error)) return false;
  LOG_INFO(tr("log.navmesh_exported"), meshes.size(), path.c_str());
  return true;
}

Vec3 App::export_start_point() {
  // the level opened where it is now: the group of its shell (moved with it)
  Mat4 placed = Mat4::identity();
  const std::string prefix = scene.settings.level + "-";
  for (const auto& n : scene.nodes)
    if (!n.parent && n.kind == NodeKind::Group && n.name.rfind(prefix, 0) == 0) {
      placed = scene.world(n);
      break;
    }
  scene.begin_frame();
  const AABB b = scene.focus_bounds();
  if (const GameData* data = document_game_data())
    if (const LevelInfo* info = data->level(scene.settings.level))
      for (const auto& c : info->continues) {
        const Vec3 p = placed.point(c.position);
        if (!b.valid() || b.expanded(50.f).contains(p)) return p;
      }
  if (!b.valid()) return {0, 2, 0};
  // else the ground at the middle of the document
  const Vec3 top{b.center().x, b.hi.y + 10.f, b.center().z};
  PickFilter f;
  f.actors = false;
  f.include_locked = true;
  const PickHit hit = pick(scene, Ray{top, {0, -1, 0}}, f);
  return hit.hit() ? hit.point + Vec3{0, 1, 0} : Vec3{top.x, b.lo.y + 2.f, top.z};
}

std::string App::untaken_nickname(const std::string& nickname) {
  // a DGO named like one of the game's would be built over the game's own
  const GameData* data = game_data().get();
  auto taken = [&](const std::string& nick) {
    if (!data) return false;
    for (const auto& [level, info] : data->levels())
      if (!info.dgo.empty() && info.nickname == nick) return true;
    return false;
  };
  std::string out = nickname;
  const char* digits = "abcdefghijklmnopqrstuvwxyz0123456789";
  for (int k = 0; taken(out) && k < 36 * 36; k++) out = std::string("m") + digits[(k / 36) % 36] + digits[k % 36];
  return out;
}

LevelExportOptions App::default_export_options() {
  LevelExportOptions o;
  o.replaces = scene.settings.replaces;
  std::string base = !o.replaces.empty() ? o.replaces : scene.settings.level;
  if (base.empty() || base == "custom-level") base = "level";
  // a name the game does not have: "my-" and the level's, 10 characters at most
  std::string name = "my-";
  for (char c : base)
    if (std::isalnum((unsigned char)c) || c == '-') name += (char)std::tolower((unsigned char)c);
  if (name.size() > 10) name.resize(10);
  while (name.back() == '-') name.pop_back();
  o.name = name;
  o.nickname = untaken_nickname(default_nickname(name));
  o.iso_name = default_iso_name(name);
  o.base_id = kDefaultBaseId;
  // the project's export, next to the extracted game (not in the editor's sources: the level holds
  // its textures): <library>/export/<project>/
  const std::string root = library_root.empty() ? Prefs::dir() : library_root;
  std::string project = project_folder_name(project_name());
  if (project.empty()) project = name;
  o.folder = from_path(to_path(root) / "export" / to_path(project));
  o.start = export_start_point();
  return o;
}

void App::open_export_window() {
  // the options of the last export of this level, else new ones
  if (export_options.name.empty() || m_export_level != scene.settings.level) {
    export_options = default_export_options();
    m_export_level = scene.settings.level;
  }
  m_export_report.reset();
  m_export_error.clear();
  show_export_window = true;
}

bool App::export_for_game(const LevelExportOptions& in, LevelExportReport* report, std::string* error) {
  stop_preview();
  if (!document_open()) {
    if (error) *error = tr("export.no_document");
    return false;
  }
  LevelExportOptions o = in;
  o.navmeshes.clear();
  o.navmesh_names.clear();
  // the game's lighting of the levels' decor, for a document made before the extraction kept it
  if (const size_t lit = restore_level_lighting()) LOG_INFO(tr("log.lighting_restored"), lit);
  world_navmeshes(&o.navmeshes, &o.navmesh_names);
  // the levels as extracted (read once): the pieces of their decor deleted, moved or hidden take the
  // game's collision with them
  if (o.game_collision)
    for (const auto& n : scene.nodes) {
      const std::string suffix = "-collision";
      if (n.kind != NodeKind::Group || n.name.size() <= suffix.size() ||
          n.name.compare(n.name.size() - suffix.size(), suffix.size(), suffix) != 0)
        continue;
      const std::string level = n.name.substr(0, n.name.size() - suffix.size());
      if (o.originals.count(level)) continue;
      auto& original = m_export_originals[level];
      // a level of Jak II (jak2-<name>): read from its library, named as the document has it
      const bool jak2 = level.rfind("jak2-", 0) == 0;
      const LevelEntry* entry = jak2 ? library_jak2.level(level.substr(5)) : library.level(level);
      if (!original && entry) {
        auto read = std::make_shared<Scene>();
        ImportResult r;
        std::string err;
        if (import_gltf(*read, entry->background, ImportOptions{}, &r, &err)) {
          if (jak2) make_jak2_level(*read, 0, level.substr(5));
          original = read;
        } else {
          LOG_WARN("%s: %s", entry->background.c_str(), err.c_str());
        }
      }
      if (original) o.originals[level] = original;
    }
  // the regions of the level replaced, as extracted
  if (!o.replaces.empty() && o.region_trees.is_null())
    if (const LevelEntry* entry = library.level(o.replaces)) o.region_trees = entry->region_trees;
  scene.begin_frame();  // the world matrices and what is shown, after the last changes
  LevelExportReport rep;
  if (!export_level_for_game(scene, document_game_data(), o, &rep, error)) return false;
  LOG_INFO(tr("log.exported_for_game"), o.name.c_str(), rep.dir.c_str(), rep.meshes, rep.actors);
  if (report) *report = std::move(rep);
  return true;
}

}  // namespace ogle
