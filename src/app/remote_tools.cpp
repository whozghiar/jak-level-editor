// The editor's side of its MCP server (see remote.h, mcp.cpp): the tools an assistant calls,
// carried out on the main thread between frames, like the user's actions (shown as they happen,
// undoable). Positions are in meters, Y up, like the editor's view.

#include <SDL3/SDL.h>

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <optional>
#include <set>
#include <unordered_map>
#include <unordered_set>

#include "app/app.h"
#include "io/behaviors.h"
#include "io/decor_behaviors.h"
#include "io/jak2_import.h"
#include "scene/assemblies.h"
#include "scene/relight.h"
#include "core/i18n.h"
#include "core/log.h"
#include "core/prefs.h"
#include "glad/glad.h"
#include "render/offscreen.h"

namespace ogle {

namespace fs = std::filesystem;

namespace {

json ok(json result = json::object()) { return {{"ok", true}, {"result", std::move(result)}}; }
json fail(const std::string& error) { return {{"ok", false}, {"error", error}}; }

float round3(float v) { return std::round(v * 1000.f) / 1000.f; }
json vec3_json(const Vec3& v) { return json::array({round3(v.x), round3(v.y), round3(v.z)}); }

bool vec3_arg(const json& args, const char* key, Vec3* out) {
  auto it = args.find(key);
  if (it == args.end() || !it->is_array() || it->size() != 3) return false;
  for (int i = 0; i < 3; i++)
    if (!(*it)[i].is_number()) return false;
  *out = {(*it)[0].get<float>(), (*it)[1].get<float>(), (*it)[2].get<float>()};
  return true;
}

std::vector<uint32_t> ids_arg(const json& args, const char* key = "ids") {
  std::vector<uint32_t> out;
  auto it = args.find(key);
  if (it == args.end()) return out;
  if (it->is_number_integer()) out.push_back(it->get<uint32_t>());
  if (it->is_array())
    for (const auto& v : *it)
      if (v.is_number_integer()) out.push_back(v.get<uint32_t>());
  return out;
}

std::string lower(std::string s) {
  for (auto& c : s) c = (char)std::tolower((unsigned char)c);
  return s;
}

const char* kind_word(NodeKind k) { return kind_name(k); }

}  // namespace

// ------------------------------------------------------------------------------------------------
// server
// ------------------------------------------------------------------------------------------------

void App::remote_start(int port) {
  if (port <= 0) return;
  std::string err;
  if (m_remote.start(port, &err)) LOG_INFO(tr("log.remote_ready"), port);
  else LOG_WARN(tr("log.remote_failed"), err.c_str());
}

void App::ui_input_step() {
  // one event per frame, as a person would give them; then the answer (with the last picture)
  if (m_ui_wait > 0) {
    m_ui_wait--;
    return;
  }
  if (m_ui_shot_pending) return;
  if (m_ui_events.empty()) {
    if (m_ui_reply) {
      json answer = ok({{"mouse", {m_ui_mouse.x, m_ui_mouse.y}}});
      if (!m_ui_shot.empty()) answer["image"] = m_ui_shot;
      m_ui_shot.clear();
      auto reply = std::move(m_ui_reply);
      m_ui_reply = nullptr;
      reply(answer);
    }
    return;
  }
  const json e = m_ui_events.front();
  m_ui_events.pop_front();
  SDL_Event ev{};
  const SDL_WindowID window = SDL_GetWindowID(m_window);
  auto motion = [&](float x, float y, float dx, float dy) {
    ev.type = SDL_EVENT_MOUSE_MOTION;
    ev.motion.windowID = window;
    ev.motion.which = 0;
    ev.motion.x = x;
    ev.motion.y = y;
    ev.motion.xrel = dx;
    ev.motion.yrel = dy;
    SDL_PushEvent(&ev);
  };
  auto button = [&](const std::string& b, bool down) {
    ev.type = down ? SDL_EVENT_MOUSE_BUTTON_DOWN : SDL_EVENT_MOUSE_BUTTON_UP;
    ev.button.windowID = window;
    ev.button.which = 0;
    ev.button.button = b == "right" ? SDL_BUTTON_RIGHT : b == "middle" ? SDL_BUTTON_MIDDLE : SDL_BUTTON_LEFT;
    ev.button.down = down;
    ev.button.clicks = 1;
    ev.button.x = m_ui_mouse.x;
    ev.button.y = m_ui_mouse.y;
    SDL_PushEvent(&ev);
  };
  if (e.contains("move") && e["move"].is_array() && e["move"].size() == 2) {
    const Vec2 to{e["move"][0].get<float>(), e["move"][1].get<float>()};
    motion(to.x, to.y, to.x - m_ui_mouse.x, to.y - m_ui_mouse.y);
    m_ui_mouse = to;
  } else if (e.contains("move_to_node")) {
    // over a point of the element the view shows (not hidden by something in front)
    const uint32_t id = e["move_to_node"].get<uint32_t>();
    const Node* n = scene.find(id);
    PickFilter pf;
    pf.actors = rs.show_actors;
    pf.include_locked = true;
    bool found = false;
    if (n && n->mesh) {
      const Mat4 w = scene.world(*n);
      for (const auto& p : n->mesh->prims)
        for (size_t t = 0; t + 2 < p.idx.size() && !found; t += 3) {
          const Vec3 c = w.point((p.pos[p.idx[t]] + p.pos[p.idx[t + 1]] + p.pos[p.idx[t + 2]]) * (1.f / 3.f));
          Vec2 s;
          if (!renderer.project(c, &s) || s.x < 2 || s.y < 2 || s.x > m_vp_w - 2 || s.y > m_vp_h - 2) continue;
          if (selectable(pick(scene, camera.ray(s.x, s.y, m_vp_w, m_vp_h), pf).node) != id) continue;
          const Vec2 to{m_vp_x + s.x, m_vp_y + s.y};
          motion(to.x, to.y, to.x - m_ui_mouse.x, to.y - m_ui_mouse.y);
          m_ui_mouse = to;
          found = true;
        }
    }
  } else if (e.contains("rel") && e["rel"].is_array() && e["rel"].size() == 2) {
    // the mouse moved while the cursor stays (relative mode: turning, looking around)
    motion(m_ui_mouse.x, m_ui_mouse.y, e["rel"][0].get<float>(), e["rel"][1].get<float>());
  } else if (e.contains("down")) {
    button(e["down"].get<std::string>(), true);
  } else if (e.contains("up")) {
    button(e["up"].get<std::string>(), false);
  } else if (e.contains("click")) {
    button(e["click"].get<std::string>(), true);
    m_ui_events.push_front(json{{"up", e["click"]}});
  } else if (e.contains("wheel")) {
    ev.type = SDL_EVENT_MOUSE_WHEEL;
    ev.wheel.windowID = window;
    ev.wheel.y = e["wheel"].get<float>();
    ev.wheel.mouse_x = m_ui_mouse.x;
    ev.wheel.mouse_y = m_ui_mouse.y;
    SDL_PushEvent(&ev);
  } else if (e.contains("text")) {
    // typed (kept until the event is read)
    m_ui_text = e["text"].get<std::string>();
    ev.type = SDL_EVENT_TEXT_INPUT;
    ev.text.windowID = window;
    ev.text.text = m_ui_text.c_str();
    SDL_PushEvent(&ev);
  } else if (e.contains("key")) {
    // a key pressed and let go ("Escape", "Return", "Backspace", "G"...), with "mods" held
    // (["ctrl", "shift", "alt"])
    const std::string name = e["key"].get<std::string>();
    SDL_Keymod mod = SDL_KMOD_NONE;
    if (e.contains("mods") && e["mods"].is_array())
      for (const auto& m : e["mods"]) {
        const std::string k = m.is_string() ? m.get<std::string>() : std::string();
        mod |= k == "ctrl" ? SDL_KMOD_LCTRL : k == "shift" ? SDL_KMOD_LSHIFT : k == "alt" ? SDL_KMOD_LALT : 0;
      }
    for (bool down : {true, false}) {
      ev = SDL_Event{};
      ev.type = down ? SDL_EVENT_KEY_DOWN : SDL_EVENT_KEY_UP;
      ev.key.windowID = window;
      ev.key.key = SDL_GetKeyFromName(name.c_str());
      ev.key.scancode = SDL_GetScancodeFromName(name.c_str());
      ev.key.mod = mod;
      ev.key.down = down;
      SDL_PushEvent(&ev);
    }
  } else if (e.contains("wait")) {
    m_ui_wait = std::max(0, e["wait"].get<int>());
  } else if (e.contains("shot")) {
    m_ui_shot_pending = true;
  }
}

void App::remote_poll() {
  ui_input_step();
  // requests that wait for something (a level loading...): answered when it is there
  for (size_t i = 0; i < m_remote_waits.size();) {
    if (m_remote_waits[i]()) m_remote_waits.erase(m_remote_waits.begin() + (long)i);
    else i++;
  }
  for (auto& r : m_remote.take()) {
    const int client = r.client;
    const json id = r.id;
    const std::string tool = r.tool;
    RemoteReply reply = [this, client, id](json answer) {
      answer["id"] = id;
      m_remote.reply(client, answer);
    };
    try {
      remote_dispatch(tool, r.args, reply);
    } catch (const std::exception& e) {
      reply(fail(std::string("error in ") + tool + ": " + e.what()));
    }
  }
}

// ------------------------------------------------------------------------------------------------
// helpers
// ------------------------------------------------------------------------------------------------

json App::remote_node_json(const Node& n) {
  json j = {{"id", n.id}, {"name", n.name}, {"kind", kind_word(n.kind)}};
  if (n.kind == NodeKind::Actor) j["etype"] = n.extras.value("etype", std::string());
  j["position"] = vec3_json(scene.world(n).col3(3));
  const AABB b = subtree_bounds(n.id);
  if (b.valid()) {
    j["size"] = vec3_json(b.size());
    j["box"] = {vec3_json(b.lo), vec3_json(b.hi)};
  }
  if (n.locked) j["locked"] = true;
  if (!scene.effectively_visible(n)) j["hidden"] = true;
  if (const uint32_t g = group_of(n.id)) j["group"] = g;  // a prefab placed, a group named
  NavMeshData nav;
  if (n.kind == NodeKind::NavMesh && navmesh_of_node(n, &nav)) {
    j["polygons"] = nav.polys.size();
    j["area_m2"] = std::round(nav.area());
    const std::string problem = navmesh_problem(nav);
    if (!problem.empty()) j["problem"] = problem;
  }
  return j;
}

bool App::remote_unsaved(const json& args) const {
  return history.revision() != saved_revision && document_open() && !args.value("discard_changes", false);
}

std::string App::capture_view_png(int w, int h, float game_hour) {
  OffscreenTarget target;
  if (!target.create(w, h)) return {};
  GLint viewport[4];
  glGetIntegerv(GL_VIEWPORT, viewport);
  glViewport(0, 0, w, h);
  glClearColor(0.07f, 0.08f, 0.09f, 1.f);
  glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
  scene.begin_frame();
  set_ocean_backdrop();
  // the game view at that hour (game_hour >= 0), or the editor's
  const bool game = game_hour >= 0 && game_view.ready() && document_open();
  RenderSettings view_rs = rs;
  const float hour = game_view.mood.hour;
  if (game) {
    game_view.mood.hour = game_hour;
    game_view_settings(view_rs);
  }
  renderer.begin(camera, 0, 0, w, h, view_rs, (float)w, (float)h);
  if (game) {
    Mat4 clip;
    game_view.draw_opaque(scene, camera, 0, 0, w, h, &clip);
    renderer.set_game_depth(&clip);
  }
  // what the cursor points at is not in the picture
  const auto hover = std::move(renderer.outline_hover), group = std::move(renderer.outline_group);
  renderer.outline_hover.clear();
  renderer.outline_group.clear();
  renderer.draw_scene(scene, view_rs, selection, 0);
  renderer.outline_hover = hover;
  renderer.outline_group = group;
  if (game) game_view.draw_translucent();
  if (rs.show_ocean && m_ocean && document_open())
    renderer.draw_water(*m_ocean, Mat4::translate({0, m_ocean_height, 0}), ocean_look(view_rs), view_rs);
  draw_effects();
  if (game) draw_rain();
  draw_navmesh_overlay();
  draw_gaps_overlay();
  renderer.set_game_depth(nullptr);
  game_view.mood.hour = hour;
  const std::vector<uint8_t> png = target.png();
  glBindFramebuffer(GL_FRAMEBUFFER, 0);
  glViewport(viewport[0], viewport[1], viewport[2], viewport[3]);
  return base64_encode(png.data(), png.size());
}

json App::remote_status() {
  size_t actors = 0;
  for (const auto& n : scene.nodes) actors += n.kind == NodeKind::Actor;
  json j = {{"library", library_root},
            {"levels", library.levels().size()},
            {"document",
             {{"open", document_open()},
              {"level", scene.settings.level},
              {"project", project_path},
              {"elements", scene.nodes.size()},
              {"actors", actors},
              {"unsaved", history.revision() != saved_revision && document_open()},
              {"loading", m_level_future.valid() || level_adding() || m_ocean_land_future.valid()}}},
            {"camera",
             {{"position", vec3_json(camera.pos)},
              {"yaw_deg", round3(camera.yaw * 57.29578f)},
              {"pitch_deg", round3(camera.pitch * 57.29578f)},
              {"forward", vec3_json(camera.forward())}}},
            {"selection", std::vector<uint32_t>(selection.begin(), selection.end())},
            {"can_undo", history.can_undo()},
            {"can_redo", history.can_redo()},
            // the time the editor takes for a frame (interface, tools, drawing), averaged, and the
            // last drawing's steps
            {"frame_ms", round3(m_frame_ms)},
            {"view",
             {{"game_view", game_view_on && game_view.ready()},
              {"hour", round3(game_view.mood.hour)},
              {"weather", [&] {
                 int count = 0;
                 const WeatherPreset* p = weather_presets(&count);
                 const int i = weather_preset_index();
                 return json{{"name", i >= 0 ? p[i].id : "custom"},
                             {"cloud", round3(game_view.mood.cloud)},
                             {"fog", round3(game_view.mood.fog)},
                             {"rain", round3(game_view.mood.rain)},
                             {"lightning", lightning}};
               }()},
              {"level_lights", game_view.mood.level_lights},
              {"effects", show_effects},
              {"particles", particles.count()},
              {"game_chunks", game_view.stats.chunks},
              {"game_triangles", game_view.stats.triangles},
              {"game_build_ms", round3((float)game_view.stats.ms_build)}}},
            {"draw",
             {{"draws", renderer.stats.draws},
              {"triangles", renderer.stats.triangles},
              {"cull_ms", round3((float)renderer.stats.ms_cull)},
              {"instances_ms", round3((float)renderer.stats.ms_instances)},
              {"draw_ms", round3((float)renderer.stats.ms_draw)},
              {"overlays_ms", round3((float)renderer.stats.ms_overlays)}}}};
  if (const GameData* data = document_game_data(); data && data->has_story()) {
    json s = {{"enabled", story.enabled}, {"step", story.step}, {"steps", data->step_count()}};
    if (story.step > 1) s["after_node"] = data->node(story.step - 1).name;
    // each level at its point: its states (the steps where it changes) and the one shown
    json levels = json::object();
    auto level_json = [&](const std::string& name, int step) {
      const auto states = story_states(name == scene.settings.level ? std::string() : name);
      int state = 1;
      for (size_t i = 1; i < states.size(); i++)
        if (states[i] <= step) state = (int)i + 1;
      json l = {{"step", step}, {"state", state}, {"states", states}};
      if (step > 1) l["after_node"] = data->node(step - 1).name;
      return l;
    };
    levels[scene.settings.level] = level_json(scene.settings.level, story.step);
    for (const auto& [name, changes] : level_story_steps) levels[name] = level_json(name, story.step_of(name));
    s["levels"] = levels;
    j["story"] = s;
  }
  if (document_open()) {
    const AABB b = scene.focus_bounds();
    if (b.valid()) j["document"]["bounds"] = {{"min", vec3_json(b.lo)}, {"max", vec3_json(b.hi)}};
    // its levels: the one opened, the ones added, the companions (layers left out)
    json levels = json::array();
    for (const auto& l : document_levels()) {
      if (l.role == DocumentLevel::Layer) continue;
      json e = {{"name", l.name},
                {"role", l.role == DocumentLevel::Main ? "opened" : l.role == DocumentLevel::Added ? "added" : "companion"}};
      if (l.group) {
        e["group"] = l.group;
        if (const Node* g = scene.find(l.group)) e["visible"] = g->visible;
      }
      levels.push_back(e);
    }
    j["document"]["levels"] = levels;
    if (level_adding()) j["document"]["adding"] = m_adding_level;
  }
  return j;
}

json App::remote_level_json(const std::string& name) {
  json j = {{"name", name}};
  const LevelEntry* entry = library.level(name);
  j["in_library"] = entry != nullptr;
  if (entry) j["dgo"] = entry->dgo;
  for (const auto& l : document_levels()) {
    if (l.name != name || l.role == DocumentLevel::Layer) continue;
    j["in_document"] = l.role == DocumentLevel::Main ? "opened" : l.role == DocumentLevel::Added ? "added" : "companion";
    if (l.group) j["group"] = l.group;
    const AABB b = document_level_bounds(l.group);
    if (b.valid()) j["bounds"] = {{"min", vec3_json(b.lo)}, {"max", vec3_json(b.hi)}};
    break;
  }
  if (!j.contains("in_document")) j["in_document"] = false;
  // what moves it whole (transform_nodes): its group, or the roots of the level opened
  if (j["in_document"] == "opened" || j["in_document"] == "added")
    j["roots"] = document_level_roots(j.value("group", 0u));
  const GameData* data = game_data().get();
  if (const LevelInfo* info = data ? data->level(name) : nullptr) {
    j["neighbors"] = info->neighbors;
    j["companions"] = info->companions;
    json cps = json::array();
    for (const auto& c : info->continues)
      cps.push_back({{"name", c.name}, {"position", vec3_json(c.position)}, {"shows", c.shows}});
    j["continue_points"] = cps;
  }
  return j;
}

// ------------------------------------------------------------------------------------------------
// the tools
// ------------------------------------------------------------------------------------------------

void App::remote_dispatch(const std::string& tool, const json& args, RemoteReply reply) {
  if (tool == "get_status") {
    reply(ok(remote_status()));

  } else if (tool == "list_levels") {
    const std::string f = lower(args.value("filter", std::string()));
    // near: only the levels the game displays together with that one
    const std::string near = args.value("near", std::string());
    const GameData* data = game_data().get();
    const LevelInfo* around = data && !near.empty() ? data->level(near) : nullptr;
    if (!near.empty() && !around) return reply(fail("near: no game data for the level '" + near + "'"));
    std::set<std::string> in_document;
    for (const auto& l : document_levels())
      if (l.role != DocumentLevel::Layer) in_document.insert(l.name);
    json levels = json::array();
    const bool jak2 = args.value("game", std::string()) == "jak2";
    for (const auto& l : (jak2 ? library_jak2 : library).levels()) {
      if (!l.listed || !(f.empty() || l.name.find(f) != std::string::npos || lower(l.dgo).find(f) != std::string::npos))
        continue;
      if (around && std::find(around->neighbors.begin(), around->neighbors.end(), l.name) == around->neighbors.end())
        continue;
      json e = {{"name", l.name}, {"dgo", l.dgo}};
      if (in_document.count(jak2 ? "jak2-" + l.name : l.name)) e["in_document"] = true;
      levels.push_back(e);
    }
    reply(ok({{"levels", levels}}));

  } else if (tool == "get_level_info") {
    const std::string name = args.value("name", scene.settings.level);
    if (name.empty()) return reply(fail("name: a level (see list_levels)"));
    reply(ok(remote_level_json(name)));

  } else if (tool == "add_level") {
    const std::string name = args.value("name", std::string());
    const bool jak2 = args.value("game", std::string()) == "jak2";
    const LevelEntry* level = (jak2 ? library_jak2 : library).level(name);
    if (!level) return reply(fail("no level '" + name + "' in the library (see list_levels)"));
    std::string err;
    const bool with_actors = load_actors;
    load_actors = args.value("actors", true);
    const bool started = add_level_to_document(*level, &err, args.value("reference", false), jak2);
    load_actors = with_actors;
    if (!started) return reply(fail(err));
    const uint64_t before = m_levels_added;
    // Jak II's is named jak2-<name> in the document
    m_remote_waits.push_back([this, reply, name = jak2 ? "jak2-" + name : name, before]() {
      if (m_levels_added == before) return false;
      if (!m_add_error.empty()) reply(fail(m_add_error));
      else reply(ok(remote_level_json(name)));
      return true;
    });

  } else if (tool == "remove_level") {
    const std::string name = args.value("name", std::string());
    const bool reference = args.value("reference", false);
    uint32_t group = 0;
    for (const auto& l : document_levels())
      if (l.name == name && l.role == DocumentLevel::Added && l.reference == reference) group = l.group;
    if (!group) return reply(fail("no level '" + name + "' added to the document (the level opened stays)"));
    std::string err;
    if (!remove_document_level(group, &err)) return reply(fail(err));
    reply(ok(remote_status()));

  } else if (tool == "merge_level") {
    // a level added made part of the document's own: no longer a level next to it (the export
    // writes it with the document's, with its collision: a level built from another's pieces)
    const std::string name = args.value("name", std::string());
    uint32_t group = 0;
    for (const auto& l : document_levels())
      if (l.name == name && l.role == DocumentLevel::Added && !l.reference) group = l.group;
    if (!group) return reply(fail("name: a level added to the document (add_level), not a reference"));
    NodeChange c(scene);
    c.save(group);
    scene.find(group)->extras.erase("og_level");
    history.push(c.commit(strf(tr("undo.merge_level"), name.c_str()), [this]() { story_document_changed(); }));
    story_document_changed();
    reply(ok({{"group", group}, {"status", remote_status()}}));

  } else if (tool == "set_shape") {
    Node* n = scene.find(args.value("id", 0u));
    ShapeParams p;
    if (!n || !node_shape(*n, &p)) return reply(fail("id: a shape (place kind 'shape')"));
    json j = p.to_json();
    for (const char* k : {"size", "tile", "steps", "sides", "opening", "bend", "round"})
      if (args.contains(k)) j[k] = args[k];
    NodeChange c(scene);
    c.save(n->id);
    reshape(scene, *n, ShapeParams::from_json(j));
    history.push(c.commit(tr("undo.shape")));
    reply(ok(remote_node_json(*n)));

  } else if (tool == "set_material") {
    const auto ids = ids_arg(args);
    if (ids.empty()) return reply(fail("ids: the elements"));
    int texture = -1;
    const std::string tex = args.value("texture", std::string());
    if (tex.empty()) return reply(fail("texture: an image file, or a texture name (list_textures)"));
    for (int i = 0; i < (int)scene.textures.size() && texture < 0; i++)
      if (scene.textures[i].name == tex) texture = i;
    // a texture of the games' levels (Jak II's named jak2-<name>), read now
    for (const auto& t : game_textures.empty() ? palette_textures : game_textures)
      if (texture < 0 && t.name == tex) texture = palette_texture(t, true);
    for (const auto& t : game_textures_jak2)
      if (texture < 0 && "jak2-" + t.name == tex) texture = palette_texture(t, true);
    std::string err;
    if (texture < 0) texture = texture_from_image(scene, tex, &err);
    if (texture < 0) return reply(fail("texture: no texture '" + tex + "' in the document or the games' levels (list_textures), nor an image file (" + err + ")"));
    NodeChange c(scene);
    for (auto id : ids) c.save(id);
    const size_t changed =
        edit_material(scene, ids, args.value("material", -1), [&](Material& m) { m.texture = texture; });
    if (changed) history.push(c.commit(tr("undo.texture")));
    reply(ok({{"changed", changed}}));

  } else if (tool == "list_textures") {
    const std::string f = lower(args.value("filter", std::string()));
    const int offset = std::max(0, args.value("offset", 0));
    const int limit = std::clamp(args.value("limit", 100), 1, 1000);
    json list = json::array();
    int matching = 0;
    if (args.value("palette", false)) {
      // the textures Jak 3's levels use again and again, the most shared first
      for (const auto& t : palette_textures) {
        if (!f.empty() && lower(t.name).find(f) == std::string::npos && t.theme != f) continue;
        if (matching++ < offset || (int)list.size() >= limit) continue;
        list.push_back({{"name", t.name}, {"theme", t.theme}, {"levels", t.levels}, {"pieces", t.uses}});
      }
    } else {
      for (const auto& t : scene.textures) {
        if (!f.empty() && lower(t.name).find(f) == std::string::npos) continue;
        if (matching++ < offset || (int)list.size() >= limit) continue;
        list.push_back({{"name", t.name}, {"size", {t.width, t.height}}});
      }
    }
    reply(ok({{"total", matching}, {"offset", offset}, {"textures", list}}));

  } else if (tool == "set_actor") {
    // an actor's behavior (merged into og_behavior) and parameters (its lumps, typed by its type's code)
    Node* n = scene.find(args.value("id", 0u));
    if (!n || n->kind != NodeKind::Actor) return reply(fail("id: an actor (find_nodes kind 'actor')"));
    const GameData* data = game_data().get();
    const std::string etype = n->extras.value("etype", std::string());
    const auto* ps = data ? data->params(etype) : nullptr;
    NodeChange c(scene);
    c.save(n->id);
    json b = n->extras.value("og_behavior", json::object());
    if (!b.is_object()) b = json::object();
    if (args.contains("behavior")) {
      if (!args["behavior"].is_object()) return reply(fail("behavior: an object (see the tool's description)"));
      // checked here: a value of the wrong kind would stop the inspector and the export
      for (const auto& [k, v] : args["behavior"].items()) {
        if (v.is_null()) continue;
        const bool condition = v.is_object() && v.contains("when") && v["when"].is_string();
        if (k == "open" && !(v.is_string() && (v == "approach" || v == "locked" || v == "always" || v == "when")))
          return reply(fail("behavior.open: \"approach\", \"locked\", \"always\" or \"when\" (with \"when\": a condition, "
                            "beside it: {\"open\": \"when\", \"when\": {\"when\": \"gone\", \"arg\": \"crate-1\"}, \"distance\": 10})"));
        if ((k == "when" || k == "active" || k == "leave" || k == "present") && !condition)
          return reply(fail("behavior." + k + ": a condition {\"when\": always|never|task|pressed|gone, \"arg\": ...}"));
        if (k == "distance" && !v.is_number()) return reply(fail("behavior.distance: meters"));
        if (k == "loads" && !v.is_string()) return reply(fail("behavior.loads: a level's name"));
      }
      for (const auto& [k, v] : args["behavior"].items()) {
        if (v.is_null()) b.erase(k);
        else b[k] = v;
      }
    }
    json lump = n->extras.value("lump", json::object()), types = n->extras.value("lump_types", json::object());
    if (!lump.is_object()) lump = json::object();
    if (!types.is_object()) types = json::object();
    json unknown = json::array();
    if (args.contains("params")) {
      if (!args["params"].is_object()) return reply(fail("params: an object, tag -> value (get_node: params)"));
      for (const auto& [tag, v] : args["params"].items()) {
        const GameData::Param* p = nullptr;
        if (ps)
          for (const auto& q : *ps)
            if (q.tag == tag) p = &q;
        if (!p) {
          unknown.push_back(tag);
          continue;
        }
        if (v.is_null()) {
          lump.erase(tag);
          types.erase(tag);
          continue;
        }
        std::string type = "float";
        json value = v;
        if (p->kind == "ints" || p->kind == "eco-info" || p->kind == "int") type = "int32";
        else if (p->kind.rfind("enum:", 0) == 0) type = "uint32";
        else if (p->kind == "vector") type = "vector";
        else if (p->kind == "string") type = "string";
        else if (p->kind == "actor") type = "actor-name";
        else if (p->kind == "symbol") {
          type = "symbol";
          if (v.is_string() && v.get<std::string>().rfind('\'', 0) != 0) value = "'" + v.get<std::string>();
        } else if (p->kind != "float" && p->kind != "floats") {
          unknown.push_back(tag + " (" + p->kind + ": not settable)");
          continue;
        }
        lump[tag] = value;
        types[tag] = type;
      }
    }
    if (b.empty()) n->extras.erase("og_behavior");
    else n->extras["og_behavior"] = b;
    n->extras["lump"] = lump;
    n->extras["lump_types"] = types;
    history.push(c.commit(tr("undo.behavior")));
    json out = {{"behavior", b}, {"lump", lump}};
    if (!unknown.empty()) out["not_set"] = unknown;
    reply(ok(out));

  } else if (tool == "set_visibility") {
    const auto ids = ids_arg(args);
    if (ids.empty() || !args.contains("visible") || !args["visible"].is_boolean())
      return reply(fail("ids and visible (true or false)"));
    const bool visible = args["visible"].get<bool>();
    NodeChange c(scene);
    size_t changed = 0;
    for (auto id : ids)
      if (Node* n = scene.find(id); n && n->visible != visible) {
        c.save(id);
        n->visible = visible;
        changed++;
      }
    if (changed) history.push(c.commit(tr("undo.visibility")));
    reply(ok({{"changed", changed}}));

  } else if (tool == "open_level") {
    const std::string name = args.value("name", std::string());
    if (m_level_future.valid()) return reply(fail("a level is already loading"));
    if (remote_unsaved(args)) return reply(fail("the document has unsaved changes: save_project first, or discard_changes: true"));
    // Jak II's (game "jak2"): its decor made Jak 3's, the document's level named jak2-<name>
    const bool jak2 = args.value("game", std::string()) == "jak2";
    if (jak2 && library_jak2.empty()) return reply(fail("Jak II was not extracted (File > Extract game assets with a Jak II disc)"));
    const LevelEntry* level = (jak2 ? library_jak2 : library).level(name);
    if (!level) return reply(fail("no level '" + name + "' in the library (see list_levels)"));
    const bool before = load_actors;
    load_actors = args.value("actors", true);
    load_level(*level, jak2);
    load_actors = before;
    const std::string opened = jak2 ? "jak2-" + name : name;
    m_remote_waits.push_back([this, reply, name, opened]() {
      if (m_level_future.valid()) return false;
      if (scene.settings.source_group != opened) reply(fail("the level " + name + " could not be opened (see the editor's log)"));
      else reply(ok(remote_status()));
      return true;
    });

  } else if (tool == "new_level") {
    if (remote_unsaved(args)) return reply(fail("the document has unsaved changes: save_project first, or discard_changes: true"));
    new_level();
    reply(ok(remote_status()));

  } else if ((tool == "list_catalog" || tool == "list_models") && !m_index_applied) {
    // the whole catalog: once the editor has read the levels (their variants and effects)
    m_remote_waits.push_back([this, tool, args, reply]() {
      if (!m_index_applied) return false;
      remote_dispatch(tool, args, reply);
      return true;
    });

  } else if (tool == "list_catalog") {
    const std::string family = args.value("family", std::string());
    const std::string f = lower(args.value("filter", std::string()));
    const int offset = std::max(0, args.value("offset", 0));
    const int limit = std::clamp(args.value("limit", 100), 1, 1000);
    // the objects that make up a place (props, doors, water, effects), or the interactive ones
    const std::string group = args.value("group", std::string());
    json objects = json::array();
    int matching = 0;
    for (const auto& o : catalog) {
      if (!family.empty() && o.family != family) continue;
      if ((group == "objects" && (o.interactive || o.family == "effect")) || (group == "interactive" && !o.interactive) ||
          (group == "effects" && o.family != "effect"))
        continue;
      if (!f.empty() && o.name.find(f) == std::string::npos && o.etype.find(f) == std::string::npos) continue;
      if (matching++ < offset || (int)objects.size() >= limit) continue;
      json j = {{"name", o.name}, {"etype", o.etype}, {"family", o.family},
                {"group", o.family == "effect" ? "effects" : o.interactive ? "interactive" : "objects"}};
      if (o.family == "effect") j["effect_kind"] = effect_kind(o);
      if (!o.model.empty()) j["model"] = o.model;
      else j["drawn_without_model"] = true;
      if (o.count) j["in_game"] = o.count;
      objects.push_back(j);
    }
    json families = json::array();
    for (const auto& o : catalog)
      if (families.empty() || families.back() != o.family) families.push_back(o.family);
    reply(ok({{"total", matching}, {"offset", offset}, {"objects", objects}, {"families", families}}));

  } else if (tool == "list_models") {
    // Jak II's objects by their model (game: "jak2"), placed as decor, or Jak 3's models
    const bool jak2 = args.value("game", std::string()) == "jak2";
    if (jak2 && library_jak2.empty()) return reply(fail("Jak II was not extracted (File > Extract game assets with a Jak II disc)"));
    const std::string f = lower(args.value("filter", std::string()));
    const int offset = std::max(0, args.value("offset", 0));
    const int limit = std::clamp(args.value("limit", 100), 1, 1000);
    json models = json::array();
    int matching = 0;
    for (const auto& m : jak2 ? catalog_models_jak2 : catalog_models) {
      if (!f.empty() && m.name.find(f) == std::string::npos && m.group.find(f) == std::string::npos) continue;
      if (matching++ < offset || (int)models.size() >= limit) continue;
      models.push_back({{"name", m.name}, {"from", m.group}});
    }
    reply(ok({{"total", matching}, {"offset", offset}, {"models", models}}));

  } else if (tool == "list_decor") {
    // Jak II's structures (game: "jak2"), brought into Jak 3 levels, or Jak 3's decor
    const bool jak2 = args.value("game", std::string()) == "jak2";
    if (jak2 && library_jak2.empty()) return reply(fail("Jak II was not extracted (File > Extract game assets with a Jak II disc)"));
    const std::string name = args.value("level", std::string());
    const std::string f = lower(args.value("filter", std::string()));
    const int offset = std::max(0, args.value("offset", 0));
    const int limit = std::clamp(args.value("limit", 100), 1, 1000);
    if (name.empty() || name == "all") {
      // the decor of every level: once the editor has read them
      m_remote_waits.push_back([this, reply, f, offset, limit, jak2]() {
        if (!(jak2 ? m_index_jak2_applied : m_index_applied)) return false;
        json parts = json::array();
        int matching = 0;
        for (const auto& e : jak2 ? all_parts_jak2 : all_parts) {
          if (!f.empty() && e.key.find(f) == std::string::npos) continue;
          if (matching++ < offset || (int)parts.size() >= limit) continue;
          json j = {{"name", e.part.name},
                    {"level", e.level},
                    {"kind", e.part.kind == CatalogPart::Kind::Prototype ? "prototype" : "piece"},
                    {"triangles", e.part.triangles}};
          if (e.levels > 1) j["in_levels"] = e.levels;
          parts.push_back(j);
        }
        reply(ok({{"total", matching}, {"offset", offset}, {"parts", parts}}));
        return true;
      });
      return;
    }
    const LevelEntry* level = (jak2 ? library_jak2 : library).level(name);
    if (!level) return reply(fail("no level '" + name + "' in the library (see list_levels)"));
    const std::string background = level->background;
    m_remote_waits.push_back([this, reply, background, f, offset, limit, name]() {
      auto scene_ptr = store.level(background, true);
      if (!scene_ptr) {
        if (!store.failed_level(background)) return false;
        reply(fail("the decor of " + name + " cannot be read"));
        return true;
      }
      json parts = json::array();
      int matching = 0;
      for (const auto& p : list_parts(*scene_ptr)) {
        if (!f.empty() && lower(p.name).find(f) == std::string::npos) continue;
        if (library_jak2.level_of(background) && !jak2_structure(p)) continue;
        if (matching++ < offset || (int)parts.size() >= limit) continue;
        json j = {{"name", p.name},
                  {"kind", p.kind == CatalogPart::Kind::Prototype ? "prototype" : "piece"},
                  {"triangles", p.triangles}};
        if (p.kind == CatalogPart::Kind::Prototype) j["copies_in_level"] = p.copies;
        parts.push_back(j);
      }
      reply(ok({{"level", name}, {"total", matching}, {"offset", offset}, {"parts", parts}}));
      return true;
    });

  } else if (tool == "list_prefabs") {
    refresh_prefabs();
    // make: the game's prefabs not made yet made first from the extracted levels (waits); else
    // each is made with its level's when it is placed
    const bool rebuild = args.value("rebuild", false);
    if (rebuild || args.value("make", false)) start_game_prefabs(rebuild);
    const uint64_t runs = m_game_prefab_runs;
    const bool waiting = game_prefabs_busy();
    m_remote_waits.push_back([this, reply, runs, waiting]() {
      if (waiting && m_game_prefab_runs == runs) return false;
      json mine = json::array(), game = json::array();
      for (const auto& p : prefabs) mine.push_back(p.name);
      for (const auto& p : game_prefabs)
        game.push_back({{"name", p.name}, {"category", p.category}, {"description", p.description},
                        {"game", p.game.empty() ? "jak3" : p.game}, {"level", p.level}, {"made", p.made}});
      reply(ok({{"prefabs", mine}, {"game_prefabs", game}}));
      return true;
    });

  } else if (tool == "place") {
    remote_place(args, reply);

  } else if (tool == "place_many") {
    auto it = args.find("items");
    if (it == args.end() || !it->is_array() || it->empty()) return reply(fail("items: a list of placements"));
    if (it->size() > 2000) return reply(fail("items: at most 2000 at once"));
    if (!document_open()) return reply(fail("no document: open_level or new_level first"));
    remote_place_many(*it, 0, json::array(), reply);

  } else if (tool == "generate_navmesh") {
    // the parameters given, the others as the Nav meshes window has them
    auto num = [&](const char* key, float& v) {
      if (args.contains(key) && args[key].is_number()) v = args[key].get<float>();
    };
    num("cell_size", nav_params.cell_size);
    num("agent_radius", nav_params.agent_radius);
    num("agent_height", nav_params.agent_height);
    num("max_climb", nav_params.max_climb);
    num("max_slope", nav_params.max_slope);
    num("max_edge", nav_params.max_edge);
    num("max_error", nav_params.max_error);
    num("min_region", nav_params.min_region);
    AABB region;
    const json r = args.contains("region") ? args["region"] : json("all");
    if (r.is_string() && r.get<std::string>() == "selection") {
      region = navmesh_region_of_selection();
      if (!region.valid()) return reply(fail("region 'selection': nothing is selected"));
    } else if (r.is_object()) {
      Vec3 lo, hi;
      if (!vec3_arg(r, "min", &lo) || !vec3_arg(r, "max", &hi))
        return reply(fail("region: \"all\", \"selection\" or {\"min\": [x, y, z], \"max\": [x, y, z]}"));
      region.add(lo);
      region.add(hi);
    }
    // the pieces of ground to keep: next to these points, or to the selected elements
    std::vector<Vec3> keep_from;
    if (args.contains("from")) {
      const json& f = args["from"];
      if (f.is_string() && f.get<std::string>() == "selection") {
        keep_from = navmesh_points_of_selection();
        if (keep_from.empty()) return reply(fail("from 'selection': nothing is selected"));
      } else if (f.is_array()) {
        for (const auto& p : f) {
          if (!p.is_array() || p.size() != 3 || !p[0].is_number() || !p[1].is_number() || !p[2].is_number())
            return reply(fail("from: \"selection\" or a list of points [x, y, z]"));
          keep_from.push_back({p[0].get<float>(), p[1].get<float>(), p[2].get<float>()});
        }
      } else {
        return reply(fail("from: \"selection\" or a list of points [x, y, z]"));
      }
    }
    std::string err;
    if (!start_navmesh_generation(region, &err, std::move(keep_from))) return reply(fail(err));
    const uint64_t generation = m_nav_generation;
    m_remote_waits.push_back([this, generation, reply]() {
      if (m_nav_generation == generation) return false;
      if (!m_nav_last.ok) {
        reply(fail("nav mesh not generated: " + m_nav_last.error));
        return true;
      }
      // the new meshes are the selection
      json meshes = json::array();
      for (auto id : selection)
        if (const Node* n = scene.find(id)) meshes.push_back(remote_node_json(*n));
      const NavBuildStats& s = m_nav_last.stats;
      reply(ok({{"meshes", meshes},
                {"polygons", s.polys},
                {"pieces_split", s.split},
                {"pieces_left_out", s.left_out},
                {"triangles_read", s.triangles},
                {"grid", {s.grid_width, s.grid_depth}},
                {"seconds", std::round(s.ms / 100.0) / 10.0}}));
      return true;
    });

  } else if (tool == "export_navmesh") {
    const std::string path = args.value("path", std::string());
    if (path.empty()) return reply(fail("path: the nav_data file to write (.json)"));
    const uint32_t first = args.value("first_id", nav_first_aid);
    std::string err;
    if (!export_navmeshes(path, first, &err)) return reply(fail(err));
    reply(ok({{"path", path}, {"meshes", navmesh_nodes().size()}, {"first_id", first}}));

  } else if (tool == "match_lighting") {
    // the inspector's Match the lighting: the vertex colors of the pieces (and those under them)
    // scaled to the brightness of the document's Jak 3 decor, or by `factor`
    const auto ids = ids_arg(args);
    if (ids.empty()) return reply(fail("ids: the elements"));
    const float factor = args.value("factor", 0.f);
    std::vector<uint32_t> all = ids;
    for (const auto& below : scene.descendants_of(ids)) all.insert(all.end(), below.begin(), below.end());
    NodeChange c(scene);
    size_t changed = 0;
    for (auto id : all) {
      Node* n = scene.find(id);
      if (!n || n->kind != NodeKind::Render || !n->mesh || !mesh_lighting(*n->mesh)) continue;
      const float k = factor > 0 ? factor : 1.f / lighting_ratio(*n->mesh, scene);
      if (k <= 0 || std::fabs(k - 1.f) < 1e-3f) continue;
      c.save(id);
      n->mesh = scaled_colors(*n->mesh, k);
      changed++;
    }
    if (changed) history.push(c.commit(tr("undo.match_lighting")));
    reply(ok({{"changed", changed}}));

  } else if (tool == "set_decor") {
    // what the game does with elements of the decor: their collision, bounce, launch; a part given
    // false goes back to the game's
    const auto ids = ids_arg(args);
    if (ids.empty()) return reply(fail("ids: elements of the decor"));
    NodeChange c(scene);
    json changed = json::array();
    for (auto id : ids) {
      Node* n = scene.find(id);
      if (!n || n->kind != NodeKind::Render || !n->mesh || scene.actor_of(*n)) continue;
      c.save(id);
      json d = n->extras.value("og_decor", json::object());
      if (!d.is_object()) d = json::object();
      for (const char* part : {"collision", "bounce", "launch"}) {
        if (!args.contains(part)) continue;
        if (args[part].is_object()) d[part] = args[part];
        else d.erase(part);
      }
      // read back as the editor keeps them (names checked, values clamped)
      const json clean = decor_behavior_json(decor_behavior_of(json{{"og_decor", d}}));
      if (clean.is_null()) n->extras.erase("og_decor");
      else n->extras["og_decor"] = clean;
      changed.push_back({{"id", id}, {"decor", clean}});
    }
    if (changed.empty()) return reply(fail("ids: elements of the decor (not actors)"));
    history.push(c.commit(tr("undo.decor")));
    reply(ok({{"nodes", changed}}));

  } else if (tool == "replace_level") {
    if (!document_open()) return reply(fail("no level open: open_level, open_project or new_level first"));
    const GameData* data = document_game_data();
    if (args.contains("level")) {
      const std::string level = args.value("level", std::string());
      const LevelInfo* info = data ? data->level(level) : nullptr;
      if (!level.empty() && (!info || info->dgo.empty() || info->layer))
        return reply(fail("level: a level of the game (see list_levels), or \"\" to replace none"));
      const std::string before = scene.settings.replaces;
      if (before != level) {
        scene.settings.replaces = level;
        LevelSettings* s = &scene.settings;
        history.push({tr("undo.replaces"), [s, before]() { s->replaces = before; }, [s, level]() { s->replaces = level; }});
        m_export_level.clear();
      }
    }
    const std::string level = scene.settings.replaces;
    // what is asked once the original is there: placed where it is, turned
    auto finish = [this, args, reply, level]() {
      std::string err;
      if (args.value("align", false) && !align_to_replaced(&err)) return reply(fail(err));
      if (args.contains("turn_deg") && args["turn_deg"].is_number()) turn_own_level(args["turn_deg"].get<float>());
      json checks = json::array();
      for (const auto& c : check_continues(scene, document_game_data(), level)) {
        json e = {{"name", c.name}, {"position", vec3_json(c.position)}};
        e["ground"] = c.ok() ? json(round3(c.ground)) : json();
        checks.push_back(e);
      }
      json r = {{"replaces", level}, {"continues", checks}};
      const AABB own = own_level_bounds();
      if (own.valid()) r["bounds"] = {{"min", vec3_json(own.lo)}, {"max", vec3_json(own.hi)}};
      for (const auto& l : document_levels())
        if (l.reference && l.name == level) {
          const AABB b = document_level_bounds(l.group);
          if (b.valid()) r["original_bounds"] = {{"min", vec3_json(b.lo)}, {"max", vec3_json(b.hi)}};
        }
      reply(ok(r));
    };
    bool shown = false;
    for (const auto& l : document_levels()) shown |= l.reference && l.name == level;
    if (args.value("show_original", false) && !level.empty() && !shown) {
      const LevelEntry* entry = library.level(level);
      if (!entry) return reply(fail("the level " + level + " is not in the library"));
      std::string err;
      if (!add_level_to_document(*entry, &err, true)) return reply(fail(err));
      const uint64_t before = m_levels_added;
      m_remote_waits.push_back([this, before, finish, reply]() {
        if (m_levels_added == before) return false;
        if (!m_add_error.empty()) reply(fail(m_add_error));
        else finish();
        return true;
      });
      return;
    }
    finish();

  } else if (tool == "export_level") {
    if (!document_open()) return reply(fail("no level open: open_level or open_project first"));
    LevelExportOptions o = default_export_options();
    o.name = args.value("name", std::string());
    // the default nickname is one no level of the game has
    o.nickname = args.contains("nickname") ? args.value("nickname", std::string()) : untaken_nickname(default_nickname(o.name));
    o.iso_name = args.value("iso_name", default_iso_name(o.name));
    o.replaces = args.value("replaces", scene.settings.replaces);
    if (args.contains("base_id") && args["base_id"].is_number_integer()) o.base_id = args["base_id"].get<uint32_t>();
    o.folder = args.value("folder", o.folder);
    o.actors = args.value("actors", true);
    const std::string collision = args.value("collision", std::string("game"));
    if (collision != "game" && collision != "decor") return reply(fail("collision: 'game' or 'decor'"));
    o.game_collision = collision == "game";
    vec3_arg(args, "start", &o.start);
    if (const std::string bad = check_export_names(o, document_game_data()); !bad.empty())
      return reply(fail(tr_en(bad.c_str())));
    LevelExportReport r;
    std::string err;
    if (!export_for_game(o, &r, &err)) return reply(fail(err));
    json notes = json::array();
    for (const auto& n : r.notes)
      notes.push_back({{"key", n.key}, {"count", n.count}, {"detail", n.detail},
                       {"text", strf(tr_en(n.key.c_str()), n.count, n.detail.c_str())}});
    json dense = json::array();
    for (const auto& [n, at] : r.densest) dense.push_back({{"triangles", n}, {"position", vec3_json(at)}});
    reply(ok({{"folder", r.dir},
              {"dense_places", r.dense_places},
              {"densest", dense},
              {"level_folder", r.level_dir},
              {"files", r.files},
              {"meshes", r.meshes},
              {"triangles", r.triangles},
              {"collision_triangles", r.collision_triangles},
              {"meshes_colliding_by_shape", r.collision_from_decor},
              {"actors", r.actors},
              {"art_groups", r.art_groups},
              {"navmeshes", r.navmeshes},
              {"start", vec3_json(o.start)},
              {"notes", notes}}));

  } else if (tool == "find_nodes") {
    const std::string name = lower(args.value("name", std::string()));
    const std::string etype = args.value("etype", std::string());
    const std::string kind = args.value("kind", std::string());
    const bool include_locked = args.value("include_locked", false);
    Vec3 near;
    const bool has_near = vec3_arg(args, "near", &near);
    const float radius = args.value("radius", 20.f);
    const size_t limit = (size_t)std::clamp(args.value("limit", 50), 1, 20000);
    // inside a box (its middle), or outside it; under a group (a level of the document)
    AABB box;
    const bool has_box = args.contains("box") && args["box"].is_array() && args["box"].size() == 2 &&
                         vec3_arg({{"v", args["box"][0]}}, "v", &box.lo) && vec3_arg({{"v", args["box"][1]}}, "v", &box.hi);
    const bool outside = args.value("outside", false);
    const uint32_t under = args.value("under", 0u);
    const SceneCache& cache = scene.cache();
    std::vector<std::pair<float, const Node*>> found;
    for (size_t i = 0; i < scene.nodes.size(); i++) {
      const Node& n = scene.nodes[i];
      if (n.extras.contains("og_actor_visual")) continue;  // an actor's model: the actor stands for it
      if (n.kind == NodeKind::Collision) continue;
      if (n.locked && !include_locked) continue;
      if (!kind.empty() && kind != kind_word(n.kind)) continue;
      if (!name.empty() && lower(n.name).find(name) == std::string::npos) continue;
      if (!etype.empty() && (n.kind != NodeKind::Actor || n.extras.value("etype", std::string()) != etype)) continue;
      if (n.kind == NodeKind::Group && name.empty() && etype.empty() && kind.empty()) continue;
      if (under) {
        const Node* a = scene.find(n.parent);
        while (a && a->id != under) a = scene.find(a->parent);
        if (!a) continue;
      }
      const AABB nb = n.mesh ? cache.bounds[i] : AABB{};
      const Vec3 mid = nb.valid() ? nb.center() : cache.worlds[i].col3(3);
      if (has_box) {
        const bool in = mid.x >= box.lo.x && mid.x <= box.hi.x && mid.y >= box.lo.y && mid.y <= box.hi.y &&
                        mid.z >= box.lo.z && mid.z <= box.hi.z;
        if (in == outside) continue;
      }
      float d = 0;
      if (has_near) {
        d = length(mid - near);
        if (d > radius) continue;
      }
      found.push_back({d, &n});
    }
    std::sort(found.begin(), found.end(), [](const auto& a, const auto& b) { return a.first < b.first; });
    json list = json::array();
    for (size_t k = 0; k < found.size() && k < limit; k++) list.push_back(remote_node_json(*found[k].second));
    reply(ok({{"total", found.size()}, {"nodes", list}}));

  } else if (tool == "get_node") {
    const Node* n = scene.find(args.value("id", 0u));
    if (!n) return reply(fail("no element with that id"));
    json j = remote_node_json(*n);
    // what the inspector's Preview shows of it (an actor, a model of the catalog)
    if (const PreviewOptions o = preview_options(*n); o.any()) j["preview"] = o.to_json();
    j["parent"] = n->parent;
    j["children"] = scene.children(n->id);
    j["local"] = {{"position", vec3_json(n->local.t)},
                  {"rotation_deg", vec3_json(n->local.r.to_euler_deg())},
                  {"scale", vec3_json(n->local.s)}};
    if (n->mesh) {
      j["mesh"] = {{"name", n->mesh->name}, {"triangles", n->mesh->tri_count()}};
      // its materials (set_material), their textures
      json mats = json::array();
      std::set<int> seen;
      for (const auto& p : n->mesh->prims) {
        if (!seen.insert(p.material).second || p.material < 0 || p.material >= (int)scene.materials.size()) continue;
        const Material& m = scene.materials[p.material];
        json e = {{"material", p.material},
                  {"color", {m.base_color.x, m.base_color.y, m.base_color.z, m.base_color.w}}};
        if (m.texture >= 0 && m.texture < (int)scene.textures.size()) e["texture"] = scene.textures[m.texture].name;
        mats.push_back(e);
      }
      j["materials"] = mats;
    }
    ShapeParams shape;
    if (node_shape(*n, &shape)) j["shape"] = shape.to_json();
    // a structure of Jak II: where it comes from and what was checked when it came in
    if (auto im = n->extras.find("og_import"); im != n->extras.end()) j["import"] = *im;
    // what the game does with it (collision, bounce, launch)
    if (auto d = n->extras.find("og_decor"); d != n->extras.end()) j["decor"] = *d;
    if (n->kind == NodeKind::Actor) {
      j["game_data"] = n->extras;
      // what configures it: the lumps its type's code reads (set_actor), and its behavior
      const GameData* data = game_data().get();
      const std::string etype = n->extras.value("etype", std::string());
      if (const auto* ps = data ? data->params(etype) : nullptr) {
        json params = json::array();
        for (const auto& p : *ps) params.push_back({{"tag", p.tag}, {"kind", p.kind}, {"default", p.def}});
        j["params"] = params;
      }
      static const char* kinds[] = {"other", "door", "bouncer", "elevator"};
      j["behavior_kind"] = kinds[(int)behavior_kind(data, etype)];
      for (auto c : scene.children(n->id))
        if (const Node* cn = scene.find(c); cn && cn->extras.contains("og_actor_visual")) j["model"] = cn->name;
    }
    json children = json::array();
    for (auto c : scene.children(n->id)) children.push_back(c);
    if (!children.empty()) j["children"] = children;
    reply(ok(j));

  } else if (tool == "transform_nodes") {
    const auto ids = ids_arg(args);
    Vec3 position, translate, rotation;
    const bool set_position = vec3_arg(args, "position", &position);
    const bool add_offset = vec3_arg(args, "translate", &translate);
    const bool set_rotation = vec3_arg(args, "rotation_deg", &rotation);
    const float turn = args.value("rotate_y_deg", 0.f);
    // scale: [x, y, z] or one number for the three; scale_by multiplies the scale they have
    auto scale_arg = [&](const char* key, Vec3* out) {
      auto it = args.find(key);
      if (it != args.end() && it->is_number()) {
        const float k = it->get<float>();
        *out = {k, k, k};
        return true;
      }
      return vec3_arg(args, key, out);
    };
    Vec3 scale, scale_by;
    const bool set_scale = scale_arg("scale", &scale);
    const bool mul_scale = scale_arg("scale_by", &scale_by);
    if (ids.empty()) return reply(fail("ids: the elements to move"));
    NodeChange c(scene);
    json moved = json::array();
    for (auto id : ids) {
      Node* n = scene.find(id);
      if (!n) continue;
      c.save(id);
      if (set_rotation) n->local.r = Quat::from_euler_deg(rotation);
      Mat4 w = scene.world(*n);
      if (turn != 0.f) {
        const Vec3 p = w.col3(3);
        w = Mat4::translate(p) * Mat4::rotate(Quat::axis_angle({0, 1, 0}, radians(turn))) * Mat4::translate(-p) * w;
      }
      if (set_position) w = Mat4::translate(position - w.col3(3)) * w;
      if (add_offset) w = Mat4::translate(translate) * w;
      scene.set_world(*n, w);
      if (set_scale) n->local.s = scale;
      if (mul_scale) n->local.s = {n->local.s.x * scale_by.x, n->local.s.y * scale_by.y, n->local.s.z * scale_by.z};
      n->local.s = {std::max(n->local.s.x, 0.01f), std::max(n->local.s.y, 0.01f), std::max(n->local.s.z, 0.01f)};
      moved.push_back(remote_node_json(*n));
    }
    relight_moved(c, ids);
    history.push(c.commit(tr(set_scale || mul_scale ? "undo.scale"
                             : set_rotation || turn != 0.f ? "undo.rotate"
                                                           : "undo.move")));
    reply(ok({{"nodes", moved}}));

  } else if (tool == "delete_nodes") {
    auto ids = ids_arg(args);
    ids.erase(std::remove_if(ids.begin(), ids.end(), [&](uint32_t id) { return !scene.find(id); }), ids.end());
    if (ids.empty()) return reply(fail("ids: the elements to delete"));
    NodeChange c(scene);
    for (auto id : ids) c.save_subtree(id);
    scene.remove(ids);
    history.push(c.commit(strf(tr("undo.delete"), ids.size())));
    for (auto id : ids) selection.erase(id);
    reply(ok({{"deleted", ids.size()}}));

  } else if (tool == "duplicate_nodes") {
    auto ids = ids_arg(args);
    ids.erase(std::remove_if(ids.begin(), ids.end(), [&](uint32_t id) { return !scene.find(id); }), ids.end());
    if (ids.empty()) return reply(fail("ids: the elements to copy"));
    Vec3 offset{0, 0, 0};
    vec3_arg(args, "offset", &offset);
    NodeChange c(scene);
    auto copies = paste_nodes(scene, copy_nodes(scene, ids));
    for (auto id : copies)
      if (Node* n = scene.find(id)) scene.set_world(*n, Mat4::translate(offset) * scene.world(*n));
    history.push(c.commit(strf(tr("undo.duplicate"), copies.size())));
    json list = json::array();
    for (auto id : copies)
      if (const Node* n = scene.find(id)) list.push_back(remote_node_json(*n));
    reply(ok({{"nodes", list}}));

  } else if (tool == "raycast") {
    Vec3 origin, direction{0, -1, 0};
    if (!vec3_arg(args, "origin", &origin)) return reply(fail("origin: [x, y, z]"));
    vec3_arg(args, "direction", &direction);
    if (length(direction) < 1e-6f) return reply(fail("direction: a vector"));
    PickFilter f;
    f.actors = args.value("actors", false);
    f.include_locked = true;
    const PickHit hit = pick(scene, Ray{origin, normalize(direction)}, f);
    if (!hit.hit()) return reply(ok({{"hit", false}}));
    json j = {{"hit", true}, {"point", vec3_json(hit.point)}, {"normal", vec3_json(hit.normal)}, {"distance", round3(hit.t)}};
    if (const Node* n = scene.find(hit.node)) j["node"] = remote_node_json(*n);
    reply(ok(j));

  } else if (tool == "set_view") {
    // the game view (the game's renderer), its hour and fog, the effects
    if (args.contains("game_view") && args["game_view"].is_boolean()) {
      if (args["game_view"].get<bool>() && !game_view.ready()) return reply(fail("no game view: " + game_view_error));
      game_view_on = args["game_view"].get<bool>();
    }
    if (args.contains("hour") && args["hour"].is_number())
      game_view.mood.hour = std::clamp(args["hour"].get<float>(), 0.f, 24.f);
    // the weather: one of the game's by its name, or its cloud, fog and rain
    if (args.contains("weather") && args["weather"].is_string() && !set_weather(args["weather"].get<std::string>())) {
      int count = 0;
      const WeatherPreset* p = weather_presets(&count);
      std::string names;
      for (int i = 0; i < count; i++) names += std::string(i ? ", " : "") + p[i].id;
      return reply(fail("weather: one of " + names));
    }
    auto unit = [&](const char* key, float* out) {
      if (args.contains(key) && args[key].is_number()) *out = std::clamp(args[key].get<float>(), 0.f, 1.f);
    };
    unit("cloud", &game_view.mood.cloud);
    unit("fog", &game_view.mood.fog);
    unit("rain", &game_view.mood.rain);
    if (args.contains("lightning") && args["lightning"].is_boolean()) lightning = args["lightning"].get<bool>();
    if (args.contains("level_lights") && args["level_lights"].is_boolean())
      game_view.mood.level_lights = args["level_lights"].get<bool>();
    if (args.contains("effects") && args["effects"].is_boolean()) show_effects = args["effects"].get<bool>();
    reply(ok(remote_status()["view"]));

  } else if (tool == "set_camera") {
    Vec3 position, target;
    if (args.contains("frame")) {
      const json& f = args["frame"];
      // a level of the document by its name, its group (0 for the one opened) or -1
      auto group_of = [&](const json& name) -> int64_t {
        if (!name.is_string()) return -1;
        for (const auto& l : document_levels())
          if (l.name == name.get<std::string>() && l.role != DocumentLevel::Layer) return l.group;
        return -1;
      };
      if (f.is_string() && f.get<std::string>() == "all") {
        frame_all();
      } else if (f.is_object() && f.contains("level")) {
        const int64_t g = group_of(f["level"]);
        if (g < 0) return reply(fail("frame.level: a level of the document (get_status: document.levels)"));
        const AABB b = document_level_bounds((uint32_t)g);
        if (!b.valid()) return reply(fail("frame.level: that level has nothing to frame"));
        camera.frame(b);
      } else if (f.is_object() && f.contains("junction")) {
        const json& pair = f["junction"];
        const int64_t a = pair.is_array() && pair.size() == 2 ? group_of(pair[0]) : -1;
        const int64_t b = pair.is_array() && pair.size() == 2 ? group_of(pair[1]) : -1;
        if (a < 0 || b < 0 || a == b) return reply(fail("frame.junction: two levels of the document"));
        const AABB j = document_junction((uint32_t)a, (uint32_t)b);
        if (!j.valid()) return reply(fail("frame.junction: these two levels do not meet"));
        camera.pitch = -0.6f;
        camera.frame(j);
      } else {
        auto ids = f.is_array() ? ids_arg(args, "frame") : std::vector<uint32_t>(selection.begin(), selection.end());
        AABB b;
        for (auto id : ids) b.add(subtree_bounds(id));
        if (b.valid()) camera.frame(b.expanded(2.f));
      }
    }
    if (vec3_arg(args, "position", &position)) camera.pos = position;
    if (vec3_arg(args, "look_at", &target)) camera.look_at(target);
    if (args.contains("yaw_deg") && args["yaw_deg"].is_number()) camera.yaw = radians(args["yaw_deg"].get<float>());
    if (args.contains("pitch_deg") && args["pitch_deg"].is_number())
      camera.pitch = std::clamp(radians(args["pitch_deg"].get<float>()), -1.55f, 1.55f);
    reply(ok(remote_status()["camera"]));

  } else if (tool == "capture_view") {
    const int w = std::clamp(args.value("width", 960), 64, 2048);
    const int h = std::clamp(args.value("height", 600), 64, 2048);
    // what the user's view shows by default: the game rendering at its hour when it is on
    const float shown = game_view_on && game_view.ready() ? game_view.mood.hour : -1.f;
    const float hour = args.contains("game_hour") && args["game_hour"].is_number() ? args["game_hour"].get<float>() : shown;
    if (hour >= 0 && !game_view.ready()) return reply(fail("no game view: " + game_view_error));
    // only: these elements (and what is under them) alone, the others hidden for the picture
    std::vector<uint32_t> hidden;
    if (args.contains("only")) {
      std::unordered_set<uint32_t> keep;
      for (auto id : ids_arg(args, "only")) {
        keep.insert(id);
        for (auto d : scene.descendants(id)) keep.insert(d);
      }
      for (auto& n : scene.nodes)
        if ((n.kind == NodeKind::Render || n.kind == NodeKind::Actor) && n.visible && !keep.count(n.id)) {
          n.visible = false;
          hidden.push_back(n.id);
        }
    }
    const std::string png = capture_view_png(w, h, hour);
    for (auto id : hidden)
      if (Node* n = scene.find(id)) n->visible = true;
    if (!hidden.empty()) scene.begin_frame();
    if (png.empty()) return reply(fail("the view could not be captured"));
    json answer = ok({{"width", w}, {"height", h}, {"camera", remote_status()["camera"]}});
    answer["image"] = png;
    reply(answer);

  } else if (tool == "find_gaps") {
    // where the player would see the void: from the ground every few meters, rays around and down
    if (!document_open()) return reply(fail("no document"));
    AABB region;
    if (args.contains("box") && args["box"].is_array() && args["box"].size() == 2) {
      Vec3 a, b;
      json box = {{"a", args["box"][0]}, {"b", args["box"][1]}};
      if (!vec3_arg(box, "a", &a) || !vec3_arg(box, "b", &b)) return reply(fail("box: [[x, y, z], [x, y, z]]"));
      region.add(a);
      region.add(b);
    } else if (args.contains("ids")) {
      for (auto id : ids_arg(args)) region.add(subtree_bounds(id));
      region = region.expanded(args.value("margin", 12.f));
    } else {
      region = own_level_bounds();
    }
    if (!region.valid()) return reply(fail("no region: box, ids, or a document with decor"));
    GapOptions o;
    o.region = region;
    o.spacing = std::max(0.f, args.value("spacing", 0.f));
    o.eye = args.value("eye", 1.6f);
    if (m_ocean && rs.show_ocean) o.ocean = m_ocean_height;
    m_gaps = find_gaps(scene, o);
    m_gaps_done = true;
    if (args.value("show", true)) show_gaps_window = true;
    const int limit = std::clamp(args.value("limit", 40), 1, 500);
    json spots = json::array();
    size_t gaps = 0;
    for (const auto& s : m_gaps.spots) gaps += !s.edge;
    for (const auto& s : m_gaps.spots) {
      if ((int)spots.size() >= limit) break;
      spots.push_back({{"kind", s.edge ? "edge" : "gap"},
                       {"at", vec3_json(s.at)},
                       {"seen_from", vec3_json(s.seen_from)},
                       {"direction", vec3_json(s.direction)},
                       {"rays", s.rays}});
    }
    reply(ok({{"gaps", gaps},
              {"edges", m_gaps.spots.size() - gaps},
              {"viewpoints", m_gaps.viewpoints},
              {"rays", m_gaps.rays},
              {"region", json::array({vec3_json(region.lo), vec3_json(region.hi)})},
              {"spots", spots}}));

  } else if (tool == "group") {
    const std::string name = args.value("name", std::string());
    if (name.empty()) return reply(fail("name: the group's"));
    selection.clear();
    for (auto id : ids_arg(args))
      if (scene.find(id)) selection.insert(id);
    if (selection.empty()) return reply(fail("ids: the elements to group"));
    const uint32_t gid = group_selection(name);
    reply(ok({{"group", gid}, {"name", name}, {"elements", selection_roots().size()}}));

  } else if (tool == "ungroup") {
    const uint32_t gid = args.value("group", 0u);
    if (!group_info(gid)) return reply(fail("group: a group's id (list_groups)"));
    ungroup(gid);
    reply(ok({{"ungrouped", gid}}));

  } else if (tool == "list_groups") {
    json groups = json::array();
    for (const auto& g : document_groups()) {
      AABB b;
      for (auto r : g.roots) b.add(subtree_bounds(r));
      groups.push_back({{"group", g.id},
                        {"name", g.name},
                        {"prefab", g.prefab},
                        {"ids", g.roots},
                        {"box", b.valid() ? json::array({vec3_json(b.lo), vec3_json(b.hi)}) : json()}});
    }
    reply(ok({{"groups", groups}}));

  } else if (tool == "select") {
    if (args.contains("group")) {
      const uint32_t gid = args.value("group", 0u);
      if (!group_info(gid)) return reply(fail("group: a group's id (list_groups, or 'group' of an element)"));
      select_group(gid);
      return reply(ok({{"selection", std::vector<uint32_t>(selection.begin(), selection.end())}}));
    }
    if (args.contains("level")) {
      // a whole level: its group, or the root elements of the level opened
      const std::string name = args.value("level", std::string());
      int64_t group = -1;
      for (const auto& l : document_levels())
        if (l.name == name && (l.role == DocumentLevel::Main || l.role == DocumentLevel::Added)) group = l.group;
      if (group < 0) return reply(fail("level: the level opened or a level added (get_status: document.levels)"));
      select_level((uint32_t)group);
      return reply(ok({{"selection", std::vector<uint32_t>(selection.begin(), selection.end())}}));
    }
    if (args.contains("assembly")) {
      const uint32_t id = args.value("assembly", 0u);
      if (!scene.find(id)) return reply(fail("assembly: an element's id"));
      select_assembly(id);
      return reply(ok({{"selection", std::vector<uint32_t>(selection.begin(), selection.end())}}));
    }
    selection.clear();
    for (auto id : ids_arg(args))
      if (scene.find(id)) selection.insert(id);
    primary = selection.empty() ? 0 : *selection.begin();
    reply(ok({{"selection", std::vector<uint32_t>(selection.begin(), selection.end())}}));

  } else if (tool == "terrain") {
    // the terrain brush (terrain_tool.cpp): one stroke, along a path when `to` is given, or hills
    if (!document_open()) return reply(fail("no document: open_level or new_level first"));
    const std::string style = args.value("style", std::string());
    if (!style.empty()) {
      // the level style: measured once its decor is read
      const LevelEntry* level = library.level(style);
      if (!level) return reply(fail("style: no level '" + style + "' (see list_levels)"));
      if (!m_styles.count(style)) {
        const std::string bg = level->background;
        if (!store.level(bg, true)) {
          if (store.failed_level(bg)) return reply(fail("style: the decor of " + style + " cannot be read"));
          m_remote_waits.push_back([this, tool, args, reply, bg]() {
            if (!store.level(bg, true) && !store.failed_level(bg)) return false;
            remote_dispatch(tool, args, reply);
            return true;
          });
          return;
        }
        m_styles.emplace(style, measure_style(*store.level(bg, true)));
      }
      m_style_level = style;
    }
    // textures of that level instead of the style's (the brush window's choice)
    if (args.contains("floor_texture")) brush.floor_texture = args.value("floor_texture", std::string());
    if (args.contains("wall_texture")) brush.wall_texture = args.value("wall_texture", std::string());
    static const std::map<std::string, BrushMode> kModes = {
        {"paint", BrushMode::Paint},   {"erase", BrushMode::Erase},     {"raise", BrushMode::Raise},
        {"dig", BrushMode::Dig},       {"smooth", BrushMode::Smooth},   {"flatten", BrushMode::Flatten},
        {"wall", BrushMode::Wall},     {"unwall", BrushMode::Unwall}};
    const std::string mode = args.value("mode", std::string("paint"));
    if (mode != "generate" && !kModes.count(mode))
      return reply(fail("mode: paint, erase, raise, dig, smooth, flatten, wall, unwall or generate"));
    Vec3 at;
    if (!vec3_arg(args, "position", &at)) return reply(fail("position: [x, y, z] in meters"));
    const uint32_t id = args.value("id", 0u);
    Node* node = id ? scene.find(id) : nullptr;
    TerrainGrid g;
    if (id && (!node || !node_terrain(*node, &g))) return reply(fail("id: a terrain (made by this tool or the brush)"));
    NodeChange change(scene);
    const bool made = !node;
    if (made) {
      Node t;
      t.id = scene.next_id;
      std::string name = "Terrain";
      for (int k = 2; std::any_of(scene.nodes.begin(), scene.nodes.end(), [&](const Node& n) { return n.name == name; }); k++)
        name = "Terrain-" + std::to_string(k);
      t.name = name;
      t.kind = NodeKind::Render;
      change.save(t.id);
      g.cell = std::clamp(args.value("cell", brush.cell), 0.25f, 8.f);
      g.looks = {terrain_look()};
      node = scene.find(scene.add(std::move(t)));
    } else {
      change.save(id);
    }
    const Mat4 inv = scene.world(*node).inverse();
    const Vec3 local = inv.point(at);
    bool changed = false;
    if (mode == "generate") {
      Vec3 area{64, 0, 64};
      if (args.contains("area") && args["area"].is_array() && args["area"].size() == 2 && args["area"][0].is_number() &&
          args["area"][1].is_number())
        area = {args["area"][0].get<float>(), 0, args["area"][1].get<float>()};
      generate_terrain(g, local, {std::clamp(area.x, 4.f, 1024.f), std::clamp(area.z, 4.f, 1024.f)},
                       std::clamp(args.value("hills", 12.f), 0.f, 200.f), std::clamp(args.value("roughness", 0.5f), 0.f, 1.f),
                       args.value("seed", 1), g.add_look(terrain_look()));
      changed = true;
    } else {
      BrushStroke st;
      st.mode = kModes.at(mode);
      st.size = std::clamp(args.value("size", 4.f), 0.25f, 256.f);
      st.height = args.contains("height") ? inv.point({at.x, args.value("height", 0.f), at.z}).y : local.y;
      st.seed = args.value("seed", 1);
      st.look = g.add_look(terrain_look());  // the cells painted take the style asked
      const bool shaping = st.mode == BrushMode::Smooth || st.mode == BrushMode::Flatten;
      st.amount = args.value("amount", st.mode == BrushMode::Wall ? 4.f : shaping ? 0.5f : 1.f);
      const auto look = m_styles.find(m_style_level);
      if (st.mode == BrushMode::Paint && args.value("relief", true) && look != m_styles.end()) {
        st.bumps = look->second.bumps;
        st.bump_size = std::max(look->second.bump_size, 0.5f);
      }
      // along a path (a road, a wall): a stamp every quarter of the brush's size
      Vec3 to_world = at;
      vec3_arg(args, "to", &to_world);
      const Vec3 to = inv.point(to_world);
      const float dist = length(Vec3{to.x - local.x, 0, to.z - local.z});
      const int stamps = std::clamp((int)std::ceil(dist / std::max(st.size * 0.25f, g.cell * 0.5f)), 0, 4096);
      for (int k = 0; k <= stamps; k++) {
        st.center = stamps ? local + (to - local) * ((float)k / stamps) : local;
        changed |= apply_brush(g, st);
      }
    }
    node->extras["og_terrain_grid"] = g.to_json();
    node->mesh = make_terrain_mesh(g);
    light_like_surroundings(scene, {node->id});  // lit like the level around it
    if (made || changed) history.push(change.commit(tr(made ? "undo.terrain.new" : "undo.terrain")));
    size_t cells = 0, walls = 0;
    for (uint8_t c : g.cells) {
      cells += c != TerrainGrid::None;
      walls += c == TerrainGrid::Wall;
    }
    reply(ok({{"id", node->id}, {"changed", changed}, {"cells", cells}, {"walls", walls}, {"cell", g.cell},
              {"style", m_style_level}}));

  } else if (tool == "list_assemblies") {
    const size_t min_elements = (size_t)std::max(1, args.value("min_elements", 2));
    const float min_size = args.value("min_size", 4.f);
    const float max_size = std::max(10.f, args.value("max_size", 60.f));
    const size_t limit = (size_t)std::max(1, args.value("limit", 200));
    scene.begin_frame();
    const SceneCache& c = scene.cache();
    // seed: the largest element whose name no other has (a recipe finds the assembly by it)
    std::unordered_map<std::string, int> named;
    for (const auto& n : scene.nodes) named[n.name]++;
    json list = json::array();
    size_t total = 0;
    for (const auto& a : find_assemblies(scene, max_size)) {
      if (a.size() < min_elements) continue;
      AABB box;
      std::string main, seed;
      float main_volume = -1, seed_volume = -1;
      std::map<std::string, std::pair<int, float>> parts;  // a part's name without its number: how many, their volume
      size_t triangles = 0;
      float largest = 0;  // the largest element's size (meters)
      for (uint32_t id : a) {
        const int i = scene.index_of(id);
        if (i < 0) continue;
        const AABB& b = c.bounds[i];
        box.add(b);
        const Vec3 s = b.size();
        const std::string& name = scene.nodes[i].name;
        if (s.x * s.y * s.z > main_volume) {
          main_volume = s.x * s.y * s.z;
          main = name;
        }
        if (named[name] == 1 && s.x * s.y * s.z > seed_volume) {
          seed_volume = s.x * s.y * s.z;
          seed = name;
        }
        std::string stem = name;
        const size_t dash = stem.rfind('-');
        if (dash != std::string::npos && dash + 1 < stem.size() &&
            stem.find_first_not_of("0123456789", dash + 1) == std::string::npos)
          stem.resize(dash);
        largest = std::max({largest, s.x, s.y, s.z});
        parts[stem].first++;
        parts[stem].second += s.x * s.y * s.z;
        if (scene.nodes[i].mesh) triangles += scene.nodes[i].mesh->tri_count();
      }
      if (!box.valid() || std::max({box.size().x, box.size().y, box.size().z}) < min_size) continue;
      total++;
      if (list.size() >= limit) continue;
      std::vector<std::pair<int, std::string>> by_count;
      for (const auto& [stem, n] : parts) by_count.push_back({-n.first, stem});
      std::sort(by_count.begin(), by_count.end());
      json names = json::array();
      for (size_t k = 0; k < by_count.size() && k < 6; k++)
        names.push_back({by_count[k].second, -by_count[k].first, std::round(parts[by_count[k].second].second)});
      list.push_back({{"ids", a},
                      {"elements", a.size()},
                      {"triangles", triangles},
                      {"largest", largest},
                      {"box", {{box.lo.x, box.lo.y, box.lo.z}, {box.hi.x, box.hi.y, box.hi.z}}},
                      {"main", main},
                      {"seed", seed},
                      {"parts", names}});
    }
    reply(ok({{"assemblies", list}, {"count", total}}));

  } else if (tool == "set_story") {
    const GameData* data = document_game_data();
    if (!data || !data->has_story()) return reply(fail("no story view for this document (a new level has none)"));
    StoryView v = story;
    // which level: the one opened (the default), or a level added, at its own point of the story
    std::string level = args.value("level", std::string());
    if (level == scene.settings.level) level.clear();
    if (!level.empty() && !level_story_steps.count(level))
      return reply(fail("level: the level opened or a level added (get_status: document.levels)"));
    auto set_step = [&](int s) {
      if (level.empty()) v.step = s;
      else v.level_steps[level] = s;
    };
    if (args.contains("enabled") && args["enabled"].is_boolean()) v.enabled = args["enabled"].get<bool>();
    if (args.contains("step") && args["step"].is_number_integer()) set_step(args["step"].get<int>());
    if (args.contains("state") && args["state"].is_number_integer()) {
      const auto states = story_states(level);
      const int s = args["state"].get<int>();
      if (s < 1 || s > (int)states.size())
        return reply(fail("state: 1 to " + std::to_string(states.size()) + " for this level"));
      set_step(states[s - 1]);
      v.enabled = true;
    }
    if (args.contains("after_node") && args["after_node"].is_string()) {
      const std::string node = args["after_node"].get<std::string>();
      int found = -1;
      for (int i = 0; i < data->step_count(); i++)
        if (data->node(i).name == node) found = i;
      if (found < 0) return reply(fail("no task node '" + node + "'"));
      set_step(found + 1);
      v.enabled = true;
    }
    story_level = level;
    set_story_view(v);
    reply(ok(remote_status()["story"]));

  } else if (tool == "create_prefab") {
    const auto ids = ids_arg(args);
    const std::string name = args.value("name", std::string());
    if (ids.empty() || name.empty()) return reply(fail("ids and name"));
    selection.clear();
    for (auto id : ids)
      if (scene.find(id)) selection.insert(id);
    if (selection.empty()) return reply(fail("no element with those ids"));
    // the small pieces left out (bolts, debris, stains): 0.5 m by default
    std::vector<std::string> left_out;
    const std::string path = create_prefab(name, std::max(0.f, args.value("min_size", 0.5f)), &left_out);
    if (path.empty()) return reply(fail("the prefab could not be saved (see the editor's log)"));
    // its name (<folder>/<name>.glb): the one asked, or with -2, -3... when a prefab has it already
    const std::string file = path.substr(path.find_last_of('/') + 1);
    reply(ok({{"name", file.substr(0, file.size() - 4)},
              {"left_out", left_out},
              {"prefabs", [&] {
                 json l = json::array();
                 for (const auto& p : prefabs) l.push_back(p.name);
                 return l;
               }()}}));

  } else if (tool == "check_catalog_thumbnails") {
    // for the tests of the interface (not offered to the assistant): the picture of every object of
    // the catalog made as its panel makes it, then how many are pictures and which failed
    if (!m_index_applied) {
      m_remote_waits.push_back([this, tool, args, reply]() {
        if (!m_index_applied) return false;
        remote_dispatch(tool, args, reply);
        return true;
      });
      return;
    }
    auto frames = std::make_shared<int>(0);  // asked once a frame
    m_remote_waits.push_back([this, reply, frames]() {
      size_t done = 0, failed = 0;
      std::map<std::string, size_t> kinds;
      json bad = json::array();
      for (const auto& o : catalog) {
        const ThumbRequest req = catalog_thumb(o);
        const bool ok = thumbs.get(req) != 0;  // asked again each frame: kept in its queue
        const bool fail = thumbs.failed(req);
        done += ok;
        failed += fail;
        if (fail) bad.push_back(o.name);
        if (ok) kinds[req.kind == ThumbRequest::Kind::Model ? "model" : req.kind == ThumbRequest::Kind::Effect ? "effect" : "icon"]++;
      }
      if (done + failed < catalog.size() && ++*frames < 20000) return false;
      reply(ok({{"objects", catalog.size()}, {"pictures", done}, {"failed", failed}, {"by_kind", kinds}, {"failed_names", bad}}));
      return true;
    });

  } else if (tool == "ground_report") {
    // for the studies of the level pieces (not offered to the assistant): the large pieces of the
    // levels' shell (og_terrain), the ground locked or not, what stands more than 3 m above their
    // floor (flat: roofs, upright: walls), the walls split off
    json pieces = json::array();
    const SceneCache& cache = scene.cache();
    for (size_t i = 0; i < scene.nodes.size(); i++) {
      const Node& n = scene.nodes[i];
      if (n.kind != NodeKind::Render || !n.mesh || !n.extras.contains("og_terrain")) continue;
      const Mat4& w = cache.worlds[i];
      std::vector<std::pair<float, float>> flats;  // height, area
      double all = 0, flat = 0, walk = 0;
      struct T { float area, ny, top, mid; };
      std::vector<T> tris;
      AABB b;
      for (const auto& pr : n.mesh->prims)
        for (size_t t = 0; t + 2 < pr.idx.size(); t += 3) {
          const Vec3 a = w.point(pr.pos[pr.idx[t]]), bb = w.point(pr.pos[pr.idx[t + 1]]), c = w.point(pr.pos[pr.idx[t + 2]]);
          b.add(a), b.add(bb), b.add(c);
          const Vec3 nr = cross(bb - a, c - a);
          const float l = length(nr);
          if (l < 1e-8f) continue;
          const T tri{l * 0.5f, std::fabs(nr.y) / l, std::max({a.y, bb.y, c.y}), (a.y + bb.y + c.y) / 3.f};
          tris.push_back(tri);
          all += tri.area;
          if (tri.ny > 0.7f) flat += tri.area, flats.push_back({tri.mid, tri.area});
          if (tri.ny > 0.5f) walk += tri.area;
        }
      if (all <= 0) continue;
      std::sort(flats.begin(), flats.end());
      float base = flats.empty() ? b.lo.y : flats.front().first;
      for (double acc = 0; const auto& [y, ar] : flats) {
        base = y;
        if ((acc += ar) >= 0.2 * flat) break;
      }
      double above_flat = 0, above_up = 0;
      float high = base;
      for (const T& t : tris) {
        const bool is_flat = t.ny >= 0.7f;
        if (!(is_flat ? t.mid > base + 3.f : t.top > base + 3.f)) continue;
        if (is_flat) above_flat += t.area;
        else if (t.ny < 0.3f) above_up += t.area;
        high = std::max(high, t.top);
      }
      pieces.push_back({{"id", n.id},
                        {"name", n.name},
                        {"locked", n.locked},
                        {"split", n.extras.contains("og_split")},
                        {"walls_piece", n.name.size() > 6 && n.name.compare(n.name.size() - 6, 6, "-walls") == 0},
                        {"box", {vec3_json(b.lo), vec3_json(b.hi)}},
                        {"area", all},
                        {"flat", flat / all},
                        {"walkable", walk / all},
                        {"base", base},
                        {"above_flat", above_flat},
                        {"above_upright", above_up},
                        {"high", high - base},
                        {"covered1", covered_below(scene, i, 1.f)},
                        {"covered3", covered_below(scene, i, 3.f)}});
    }
    reply(ok({{"level", scene.settings.level}, {"pieces", pieces}}));

  } else if (tool == "ui_input") {
    // for the tests of the interface (not offered to the assistant): mouse events one frame after
    // the other ({"move": [x, y]} in the window's pixels, {"move_to_node": id}, {"rel": [dx, dy]},
    // {"down"|"up"|"click": "left"|"right"|"middle"}, {"wheel": clicks}, {"text": "typed"}, {"key": "Escape"},
    // {"wait": frames}, {"shot": true}: a picture of
    // the whole window, given back with the answer)
    if (m_ui_reply) return reply(fail("ui_input: events are still being given"));
    const json events = args.value("events", json::array());
    if (!events.is_array()) return reply(fail("events: a list"));
    for (const auto& e : events) m_ui_events.push_back(e);
    m_ui_reply = reply;

  } else if (tool == "save_project") {
    // a name: its folder in <library>/projects; a path: that file; else where it was saved
    std::string err;
    const std::string name = args.value("name", std::string());
    std::string path = args.value("path", std::string());
    if (!name.empty()) {
      if (!save_project_as(name, &err)) return reply(fail("cannot save: " + err));
    } else {
      if (path.empty()) path = project_path;
      if (path.empty()) return reply(fail("name: the project's name (saved in <library>/projects/<name>/)"));
      if (fs::path(path).extension() != ".glb") path += ".glb";
      if (!save_project_files(path, &err)) return reply(fail("cannot save: " + err));
    }
    reply(ok({{"path", project_path}}));

  } else if (tool == "open_project") {
    const std::string path = args.value("path", std::string());
    if (path.empty()) return reply(fail("path: the project (.glb)"));
    if (remote_unsaved(args)) return reply(fail("the document has unsaved changes: save_project first, or discard_changes: true"));
    open_project_file(path);
    if (project_path != path) return reply(fail("cannot open " + path + " (see the editor's log)"));
    reply(ok(remote_status()));

  } else if (tool == "restore_effects") {
    if (!document_open()) return reply(fail("no document open"));
    size_t left_out = 0;
    const size_t added = restore_level_effects(&left_out);
    reply(ok({{"added", added}, {"left_out", left_out}}));
  } else if (tool == "undo" || tool == "redo") {
    const bool can = tool == "undo" ? history.can_undo() : history.can_redo();
    const std::string label = tool == "undo" ? history.undo_label() : history.redo_label();
    if (can) {
      if (tool == "undo") history.undo();
      else history.redo();
    }
    reply(ok({{"done", can}, {"change", label}}));

  } else {
    reply(fail("unknown tool '" + tool + "'"));
  }
}

void App::remote_place(const json& args, RemoteReply reply) {
  const std::string kind = args.value("kind", std::string("object"));
  const std::string name = args.value("name", std::string());
  Vec3 position;
  if (!vec3_arg(args, "position", &position)) return reply(fail("position: [x, y, z] in meters"));
  if (!document_open()) return reply(fail("no document: open_level or new_level first"));
  const float yaw = radians(args.value("yaw_deg", 0.f));
  const bool on_ground = args.value("on_ground", false);
  // a full rotation (degrees around x, y, z) instead of the yaw, a scale ([x, y, z] or one number)
  Vec3 rotation_deg, scale{1, 1, 1};
  const bool has_rotation = vec3_arg(args, "rotation_deg", &rotation_deg);
  if (args.contains("scale") && args["scale"].is_number()) {
    const float k = args["scale"].get<float>();
    scale = {k, k, k};
  } else {
    vec3_arg(args, "scale", &scale);
  }
  scale = {std::max(scale.x, 0.01f), std::max(scale.y, 0.01f), std::max(scale.z, 0.01f)};

  // what to place: by name
  if (kind == "model" && args.contains("path")) {
    // a model made elsewhere
    const std::string path = args.value("path", std::string());
    const fs::path file(std::u8string((const char8_t*)path.c_str()));
    std::error_code ec;
    if (!fs::is_regular_file(file, ec)) return reply(fail("path: no file " + path));
    const std::u8string u = file.stem().u8string();
    const std::string stem(u.begin(), u.end());
    start_model_placement(CatalogModel{name.empty() ? stem : name, path, ""}, false);
  } else if (kind == "model") {
    const bool jak2 = args.value("game", std::string()) == "jak2";
    const CatalogModel* found = nullptr;
    for (const auto& m : jak2 ? catalog_models_jak2 : catalog_models)
      if (m.name == name) found = &m;
    if (!found) return reply(fail("no model '" + name + "' in the catalog (see list_models)"));
    start_model_placement(*found, false);
  } else if (kind == "shape") {
    const ShapePreset* preset = shape_preset(name);
    if (!preset)
      return reply(fail("name: box, floor, wall, doorway, window, pillar, ramp, stairs, roof, roof-hip, cylinder or ground"));
    // in the style of a level: its floors' (a floor, a ground) or its walls' texture, size and light,
    // its ground's bumps; measured once its decor is read
    const std::string style = args.value("style", std::string());
    if (!style.empty()) {
      const LevelEntry* level = library.level(style);
      if (!level) return reply(fail("style: no level '" + style + "' (see list_levels)"));
      if (!m_styles.count(style)) {
        const std::string bg = level->background;
        if (!store.level(bg, true)) {
          if (store.failed_level(bg)) return reply(fail("style: the decor of " + style + " cannot be read"));
          m_remote_waits.push_back([this, args, reply, bg]() {
            if (!store.level(bg, true) && !store.failed_level(bg)) return false;
            remote_place(args, reply);
            return true;
          });
          return;
        }
        m_styles.emplace(style, measure_style(*store.level(bg, true)));
      }
    }
    if (!style.empty()) start_style_placement(style, name == "floor" || name == "ground", name);
    json j = (style.empty() ? preset->params : placement.shape).to_json();
    for (const char* k : {"size", "tile", "steps", "sides", "opening", "bend", "round", "bumps", "bump_size", "seed"})
      if (args.contains(k)) j[k] = args[k];
    ShapePreset p = *preset;
    p.params = ShapeParams::from_json(j);
    const int material = style.empty() ? -1 : placement.material;
    start_shape_placement(p, false);
    placement.material = material;
  } else if (kind == "object") {
    const CatalogObject* found = nullptr;
    for (const auto& o : catalog)
      if (o.name == name) found = &o;
    if (!found)
      for (const auto& o : catalog)
        if (o.etype == name && !found) found = &o;
    if (!found) return reply(fail("no object '" + name + "' in the catalog (see list_catalog)"));
    start_object_placement(*found, false);
  } else if (kind == "prefab") {
    refresh_prefabs();
    const PrefabInfo* found = nullptr;
    for (const auto* list : {&prefabs, &game_prefabs})
      for (const auto& p : *list)
        if (p.name == name) found = &p;
    if (!found) return reply(fail("no prefab '" + name + "' (see list_prefabs)"));
    if (!found->made) {
      // a game's prefab not made yet: made with its level's, then placed
      const uint64_t runs = m_game_prefab_runs;
      want_game_prefabs(found->game, found->level);
      m_remote_waits.push_back([this, args, reply, runs]() {
        if (m_game_prefab_runs == runs || game_prefabs_busy()) return false;
        const std::string wanted = args.value("name", std::string());
        bool made = false;
        for (const auto& p : game_prefabs)
          if (p.name == wanted) made = p.made;
        if (!made) reply(fail("the prefab could not be made from its level (see the editor's log)"));
        else remote_place(args, reply);
        return true;
      });
      return;
    }
    start_placement(found->path, found->name, false);
  } else if (kind == "decor") {
    // a structure of Jak II (game: "jak2"): converted for Jak 3 (io/jak2_import.h)
    const bool jak2 = args.value("game", std::string()) == "jak2";
    const AssetLibrary& lib = jak2 ? library_jak2 : library;
    std::string level_name = args.value("level", std::string());
    if (level_name.empty()) {
      // the first level with a part of that name
      if (!(jak2 ? m_index_jak2_applied : m_index_applied)) {
        m_remote_waits.push_back([this, args, reply, jak2]() {
          if (!(jak2 ? m_index_jak2_applied : m_index_applied)) return false;
          remote_place(args, reply);
          return true;
        });
        return;
      }
      for (const auto& e : jak2 ? all_parts_jak2 : all_parts)
        if (e.part.name == name || e.part.name == name + ".mb") {
          level_name = e.level;
          break;
        }
      if (level_name.empty()) return reply(fail("no decor part '" + name + "' in the levels (see list_decor)"));
    }
    const LevelEntry* level = lib.level(level_name);
    if (!level) return reply(fail("level: the level the decor part comes from (see list_decor)"));
    auto source = store.level(level->background, true);
    if (!source) {
      if (store.failed_level(level->background)) return reply(fail("the decor of that level cannot be read"));
      // loading: try again next frame
      m_remote_waits.push_back([this, args, reply, level_name, jak2]() {
        const LevelEntry* level = (jak2 ? library_jak2 : library).level(level_name);
        if (level && !store.level(level->background, true) && !store.failed_level(level->background)) return false;
        remote_place(args, reply);
        return true;
      });
      return;
    }
    const CatalogPart* found = nullptr;
    const auto parts = list_parts(*source);
    for (const auto& p : parts)
      if (p.name == name || p.name == name + ".mb") found = &p;
    if (!found) return reply(fail("no decor part '" + name + "' in " + level->name + " (see list_decor)"));
    start_part_placement(level->background, *found, false);
  } else {
    return reply(fail("kind: object, model, decor, prefab or shape"));
  }
  placement.yaw = yaw;

  // where: on the surface below, when asked
  Vec3 p = position;
  if (on_ground) {
    PickFilter f;
    f.actors = false;
    f.include_locked = true;
    const PickHit hit = pick(scene, Ray{position + Vec3{0, 1.f, 0}, {0, -1, 0}}, f);
    if (hit.hit()) p = hit.point;
  }
  const Quat rot = has_rotation ? Quat::from_euler_deg(rotation_deg) : Quat::axis_angle({0, 1, 0}, yaw);
  const Mat4 m = Mat4::translate(p) * Mat4::rotate(rot) * Mat4::scale(scale);
  // its model or its prefab may still be loading
  m_remote_waits.push_back([this, m, reply, kind]() {
    const bool markerless = placement.kind == Placement::Kind::Object && placement.path.empty();
    if (kind != "prefab" && !markerless && !placement_mesh()) {
      const bool failed = placement.kind == Placement::Kind::Part ? store.failed_level(placement.path)
                                                                  : store.failed_model(placement.path);
      if (!failed) return false;
      stop_placement();
      reply(fail("its model cannot be read"));
      return true;
    }
    const auto roots = place_at(m);
    stop_placement();
    if (roots.empty()) {
      reply(fail("nothing was placed"));
      return true;
    }
    json list = json::array();
    for (auto id : roots)
      if (const Node* n = scene.find(id)) list.push_back(remote_node_json(*n));
    reply(ok({{"nodes", list}}));
    return true;
  });
}

void App::remote_place_many(const json& items, size_t next, json placed, RemoteReply reply) {
  // one undo entry for the whole batch
  if (next == 0) m_batch_change = std::make_unique<NodeChange>(scene);
  if (next >= items.size()) {
    size_t count = 0;
    for (const auto& p : placed) count += p.contains("id");
    if (m_batch_change) {
      // lit like the level around them, all at once
      if (!m_batch_placed.empty()) light_like_surroundings(scene, m_batch_placed);
      m_batch_placed.clear();
      if (count) history.push(m_batch_change->commit(strf(tr("undo.place_many"), count)));
      m_batch_change.reset();
    }
    reply(ok({{"placed", count}, {"nodes", placed}}));
    return;
  }
  remote_place(items[next], [this, items, next, placed, reply](json answer) mutable {
    if (answer.value("ok", false)) {
      for (const auto& n : answer["result"]["nodes"]) placed.push_back(n);
    } else {
      placed.push_back({{"index", next}, {"error", answer.value("error", std::string())}});
    }
    remote_place_many(items, next + 1, std::move(placed), reply);
  });
}

}  // namespace ogle
