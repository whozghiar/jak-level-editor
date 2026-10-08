#include <SDL3/SDL.h>

#include <algorithm>
#include <cstring>
#include <ctime>
#include <filesystem>
#include <set>
#include <unordered_map>
#include <unordered_set>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "core/prefs.h"
#include "io/gltf_io.h"
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
// and logic actors (without any actor when not `actors`), the terrain locked, a model under each
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
      } else if (data->role(etype) != ActorRole::Object) {
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
      n.locked = terrain->get<bool>();
      // a piece is written in the level's space: its origin goes to the bottom center of its
      // geometry, so that it moves and turns around itself
      if (!n.locked && n.mesh && n.mesh.use_count() == 1 && length(n.local.t) == 0.f) {
        const AABB b = n.mesh->bounds();
        if (b.valid()) {
          const Vec3 origin{b.center().x, b.lo.y, b.center().z};
          for (auto& prim : n.mesh->prims)
            for (auto& p2 : prim.pos) p2 = p2 - origin;
          n.mesh->touch();
          n.local.t = origin;
        }
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
  return import_gltf(scene, path, o, &r, error);
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
  for (auto root : roots) {
    const Node* n = scene.find(root);
    if (!n) continue;
    clip.roots.push_back(root);
    clip.root_worlds[root] = scene.world(*n);
    clip.nodes.push_back(*n);
    for (auto d : scene.descendants(root))
      if (const Node* dn = scene.find(d)) clip.nodes.push_back(*dn);
  }
  return clip;
}

std::vector<uint32_t> paste_nodes(Scene& scene, const Clipboard& clip) {
  std::unordered_map<uint32_t, uint32_t> ids;
  for (const auto& n : clip.nodes) ids[n.id] = scene.next_id++;
  std::unordered_set<std::string> names;
  for (const auto& n : scene.nodes) names.insert(n.name);
  std::vector<uint32_t> roots;
  for (const auto& src : clip.nodes) {
    Node n = src;
    n.id = ids[src.id];
    const bool is_root = clip.root_worlds.count(src.id) != 0;
    if (is_root) {
      // under its old parent when it is still in the document
      n.parent = scene.find(src.parent) ? src.parent : 0;
      n.name = unique_name(src.name, names);
    } else {
      n.parent = ids[src.parent];
    }
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
      scene.set_world(*scene.find(id), clip.root_worlds.at(src.id));
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
  if (idx) {
    all_parts = all_decor(*idx, library);
    for (const auto& p : all_parts) (p.part.kind == CatalogPart::Kind::Prototype ? m_all_prototypes : m_all_pieces)++;
  }
}

void App::apply_library_index() {
  if (m_index_applied || !library_index.ready()) return;
  m_index_applied = true;
  rebuild_catalog();
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
  if (m_ocean && m_ocean_map == info->ocean) return;
  m_ocean_map = info->ocean;
  // a quad per run of ocean cells along x, at height 0 (drawn at the ocean's height)
  auto mesh = std::make_shared<Mesh>();
  mesh->name = "ocean";
  Primitive p;
  const int size = map->size;
  for (int r = 0; r < size; r++) {
    for (int c = 0; c < size;) {
      if (!map->cells[(size_t)r * size + c]) {
        c++;
        continue;
      }
      const int c0 = c;
      while (c < size && map->cells[(size_t)r * size + c]) c++;
      const float x0 = map->corner.x + c0 * map->cell, x1 = map->corner.x + c * map->cell;
      const float z0 = map->corner.z + r * map->cell, z1 = z0 + map->cell;
      const uint32_t base = (uint32_t)p.pos.size();
      p.pos.insert(p.pos.end(), {{x0, 0, z0}, {x1, 0, z0}, {x1, 0, z1}, {x0, 0, z1}});
      p.nrm.insert(p.nrm.end(), 4, Vec3{0, 1, 0});
      p.idx.insert(p.idx.end(), {base, base + 2, base + 1, base, base + 3, base + 2});
    }
  }
  p.material = 0;
  mesh->prims.push_back(std::move(p));
  m_ocean = mesh;
  Material m;
  m.name = "ocean";
  auto lift = [](float v) { return std::min(1.f, v * 1.6f + 0.08f); };
  m.base_color = {lift(map->color.x), lift(map->color.y), lift(map->color.z), 0.55f};
  m.alpha = AlphaMode::Blend;
  m_ocean_materials = {m};
}

void App::load_level(const LevelEntry& level) {
  if (m_level_future.valid()) {
    LOG_WARN("%s", tr("log.level_busy"));
    return;
  }
  stop_placement();
  m_level_name = level.name;
  AssetLibrary lib = library;  // the worker reads its own copy
  LevelEntry entry = level;
  auto data = game_data();
  const bool actors = load_actors;
  m_level_future = std::async(std::launch::async, [lib, entry, data, actors]() -> std::unique_ptr<Scene> {
    auto s = std::make_unique<Scene>();
    std::string err;
    if (!open_level(*s, lib, data.get(), entry, &err, actors)) {
      LOG_ERROR("%s: %s", entry.background.c_str(), err.c_str());
      return nullptr;
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
      out.push_back(l);
    }
  }
  return out;
}

bool App::level_in_document(const std::string& name) const {
  for (const auto& l : document_levels())
    if (l.name == name && l.role != DocumentLevel::Layer) return true;
  return false;
}

bool App::add_level_to_document(const LevelEntry& level, std::string* error) {
  auto fail = [&](const char* key) {
    if (error) *error = strf(tr(key), level.name.c_str());
    return false;
  };
  if (!document_open()) return fail("levels.add.no_document");
  if (m_add_future.valid() || m_level_future.valid()) return fail("levels.add.busy");
  if (level_in_document(level.name)) return fail("levels.add.already");
  stop_placement();
  m_adding_level = level.name;
  AssetLibrary lib = library;  // the worker reads its own copy
  LevelEntry entry = level;
  auto data = game_data();
  const bool actors = load_actors;
  m_add_future = std::async(std::launch::async, [lib, entry, data, actors]() -> std::unique_ptr<Scene> {
    auto s = std::make_unique<Scene>();
    std::string err;
    if (!add_level(*s, lib, data.get(), entry, &err, actors)) {
      LOG_ERROR("%s", err.c_str());
      return nullptr;
    }
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
}

void App::save(bool save_as) {
  if (save_as || project_path.empty()) {
    open_dialog(DialogAction::SaveProjectAs);
    return;
  }
  std::string err;
  if (save_project(scene, project_path, &err)) {
    saved_revision = history.revision();
    Prefs::get().add_recent_project(project_path);
    Prefs::get().save();
    LOG_INFO(tr("log.project_saved"), project_path.c_str());
  } else {
    LOG_ERROR(tr("log.save_failed"), err.c_str());
  }
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
  std::vector<uint32_t> out;
  for (auto id : selection) {
    if (!scene.find(id)) continue;
    bool has_selected_ancestor = false;
    for (auto other : selection)
      if (other != id && scene.is_ancestor(other, id)) has_selected_ancestor = true;
    if (!has_selected_ancestor) out.push_back(id);
  }
  return out;
}

Vec3 App::group_center(uint32_t id, const std::vector<uint32_t>& descendants) const {
  auto it = m_group_centers.find(id);
  const Mat4 world = scene.world(id);
  if (it != m_group_centers.end() && it->second.first == history.revision()) return world.point(it->second.second);
  std::vector<AABB> boxes;
  boxes.reserve(descendants.size());
  for (auto d : descendants)
    if (const Node* dn = scene.find(d); dn && dn->kind != NodeKind::Group) {
      const AABB b = scene.world_bounds(*dn);
      if (b.valid()) boxes.push_back(b);
    }
  const AABB focus = focus_box(boxes);
  const Vec3 center = focus.valid() ? focus.center() : world.col3(3);
  m_group_centers[id] = {history.revision(), world.inverse().point(center)};
  return center;
}

Mat4 App::selection_frame() const {
  auto roots = selection_roots();
  if (roots.empty()) return Mat4::identity();
  Vec3 p{0, 0, 0};
  for (auto id : roots) {
    const Node* n = scene.find(id);
    Vec3 origin = scene.world(id).col3(3);
    const auto descendants = scene.descendants(id);
    if (descendants.size() > 64) {
      // a large group (a whole level): the middle of most of it, a stray piece far away left out
      p += group_center(id, descendants);
      continue;
    }
    // the node's origin, unless it is far from its geometry (meshes in world space have their
    // origin at 0,0,0): then the center of its box
    AABB b = n ? scene.world_bounds(*n) : AABB{};
    for (auto d : descendants)
      if (const Node* dn = scene.find(d)) b.add(scene.world_bounds(*dn));
    p += (b.valid() && !b.expanded(1.f).contains(origin)) ? b.center() : origin;
  }
  p = p / (float)roots.size();
  uint32_t ref = selection.count(primary) ? primary : roots.front();
  Transform t = Transform::from_matrix(scene.world(ref));
  return Mat4::translate(p) * Mat4::rotate(t.r);
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
  for (auto id : roots) c.save_subtree(id);
  scene.remove(roots);
  history.push(c.commit(strf(tr("undo.delete"), roots.size())));
  selection.clear();
  primary = 0;
}

void App::copy_selection() {
  auto roots = selection_roots();
  if (roots.empty()) return;
  clipboard = copy_nodes(scene, roots);
  LOG_INFO(tr("log.copied"), roots.size());
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
}

void App::create_prefab(const std::string& name) {
  auto roots = selection_roots();
  if (roots.empty()) return;
  const std::string folder = prefab_folder();
  if (folder.empty()) {
    LOG_WARN("%s", tr("log.prefab_no_library"));
    return;
  }
  const std::string path = new_prefab_path(folder, name);
  std::string err;
  if (!save_prefab(scene, roots, path, &err)) {
    LOG_ERROR(tr("log.prefab_failed"), err.c_str());
    return;
  }
  refresh_prefabs();
  LOG_INFO(tr("log.prefab_saved"), from_path(to_path(path).stem()).c_str(), roots.size());
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
  placement.once = once;
  tool = ToolMode::Place;
  store.level(level_background, true);
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
      if (auto part = part_mesh(*level, placement.part)) mesh = merge_mesh(scene, *part, level->textures, level->materials);
      break;
    }
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
      n.name = unique_name(placement.name, names);
      n.kind = NodeKind::Render;
      n.mesh = mesh;
      n.local = Transform::from_matrix(m);
      roots.push_back(scene.add(std::move(n)));
    } else {
      std::string base = placement.part.name;
      if (base.size() > 3 && base.compare(base.size() - 3, 3, ".mb") == 0) base.resize(base.size() - 3);
      Node n;
      n.name = unique_name(base, names);
      n.kind = NodeKind::Render;
      n.mesh = mesh;
      n.local = Transform::from_matrix(m);
      roots.push_back(scene.add(std::move(n)));
    }
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
  story_document_changed();
  set_title();
  LOG_INFO("%s", tr("log.new_level"));
}

void App::stop_placement() {
  tool = ToolMode::Select;
  placement = Placement{};
  m_place_valid = false;
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
  return Mat4::translate(p) * Mat4::rotate(Quat::axis_angle({0, 1, 0}, placement.yaw));
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
    if (!project_path.empty()) return from_path(to_path(project_path).parent_path());
    const auto& recent = Prefs::get().recent_projects;
    if (!recent.empty()) return from_path(to_path(recent.front()).parent_path());
    fs::create_directories(to_path(default_projects_dir()), ec);
    return default_projects_dir();
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
    case DialogAction::SaveProjectAs:
      SDL_ShowSaveFileDialog(dialog_callback, req, m_window, project_filter, 1, projects_dir.c_str());
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
      case DialogAction::SaveProjectAs: {
        std::string p = r.path;
        if (lower_ext(p) != ".glb") p += ".glb";
        project_path = p;
        save(false);
        break;
      }
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

// ------------------------------------------------------------------------------------------------
// backups: a snapshot a few seconds after the last change, 10 kept
// ------------------------------------------------------------------------------------------------

void App::autosave_tick() {
  uint64_t rev = history.revision();
  if (rev != m_last_seen_revision) {
    m_last_seen_revision = rev;
    m_last_change_time = m_time;
  }
  if (project_path.empty() || rev == m_backup_revision || rev == saved_revision) return;
  if (m_time - m_last_change_time < 5.0 || m_time - m_last_backup_time < 60.0) return;
  fs::path dir = to_path(project_path + ".backups");
  std::error_code ec;
  fs::create_directories(dir, ec);
  std::time_t now = std::time(nullptr);
  char stamp[32];
  std::strftime(stamp, sizeof(stamp), "%Y%m%d-%H%M%S", std::localtime(&now));
  std::string err;
  if (save_project(scene, from_path(dir / (std::string(stamp) + ".glb")), &err)) {
    m_backup_revision = rev;
    m_last_backup_time = m_time;
    std::vector<fs::path> files;
    for (auto& e : fs::directory_iterator(dir, ec))
      if (e.path().extension() == ".glb") files.push_back(e.path());
    std::sort(files.begin(), files.end());
    while (files.size() > 10) {
      fs::remove(files.front(), ec);
      files.erase(files.begin());
    }
  }
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
  AABB b;
  for (auto id : selection) {
    const Node* n = scene.find(id);
    if (!n) continue;
    b.add(scene.world_bounds(*n));
    for (auto d : scene.descendants(id))
      if (const Node* dn = scene.find(d)) b.add(scene.world_bounds(*dn));
  }
  return b.valid() ? b.expanded(1.f) : b;
}

std::vector<Vec3> App::navmesh_points_of_selection() const {
  std::vector<Vec3> out;
  for (auto id : selection) {
    const Node* n = scene.find(id);
    if (!n || n->kind == NodeKind::NavMesh) continue;
    AABB b = scene.world_bounds(*n);
    for (auto d : scene.descendants(id))
      if (const Node* dn = scene.find(d)) b.add(scene.world_bounds(*dn));
    if (b.valid()) out.push_back({b.center().x, b.lo.y, b.center().z});
  }
  return out;
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
    if (n.kind != NodeKind::Render || !n.mesh || !cache.visible[i] || cache.actor[i]) continue;
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

LevelExportOptions App::default_export_options() {
  LevelExportOptions o;
  std::string base = scene.settings.level;
  if (base.empty() || base == "custom-level") base = "level";
  // a name the game does not have: "my-" and the level's, 10 characters at most
  std::string name = "my-";
  for (char c : base)
    if (std::isalnum((unsigned char)c) || c == '-') name += (char)std::tolower((unsigned char)c);
  if (name.size() > 10) name.resize(10);
  while (name.back() == '-') name.pop_back();
  o.name = name;
  o.nickname = default_nickname(name);
  o.iso_name = default_iso_name(name);
  o.base_id = kDefaultBaseId;
  o.folder = Prefs::get().export_folder;
  if (o.folder.empty()) {
    // next to the extracted game (not in the editor's sources: the level holds its textures)
    const std::string root = library_root.empty() ? Prefs::dir() : library_root;
    o.folder = from_path(to_path(root) / "exports");
  }
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
  if (!document_open()) {
    if (error) *error = tr("export.no_document");
    return false;
  }
  LevelExportOptions o = in;
  o.navmeshes.clear();
  o.navmesh_names.clear();
  world_navmeshes(&o.navmeshes, &o.navmesh_names);
  scene.begin_frame();  // the world matrices and what is shown, after the last changes
  LevelExportReport rep;
  if (!export_level_for_game(scene, document_game_data(), o, &rep, error)) return false;
  Prefs::get().export_folder = o.folder;
  if (!Prefs::read_only) Prefs::get().save();
  LOG_INFO(tr("log.exported_for_game"), o.name.c_str(), rep.dir.c_str(), rep.meshes, rep.actors);
  if (report) *report = std::move(rep);
  return true;
}

}  // namespace ogle
