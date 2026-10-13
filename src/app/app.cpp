#include "app/app.h"
#include "scene/relight.h"

#include <SDL3/SDL.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <unordered_set>

#include "core/i18n.h"
#include "core/log.h"
#include "core/prefs.h"
#include "glad/glad.h"
#include "imgui.h"
#include "imgui_impl_opengl3.h"
#include "imgui_impl_sdl3.h"
#include "render/offscreen.h"

namespace ogle {

namespace fs = std::filesystem;

namespace {

fs::path to_path(const std::string& s) { return fs::path(std::u8string((const char8_t*)s.c_str())); }

void apply_style(float scale) {
  ImGui::StyleColorsDark();
  ImGuiStyle& s = ImGui::GetStyle();
  s.WindowRounding = 4.f;
  s.FrameRounding = 4.f;
  s.GrabRounding = 3.f;
  s.TabRounding = 4.f;
  s.WindowBorderSize = 1.f;
  s.FramePadding = ImVec2(8, 5);
  s.ItemSpacing = ImVec2(8, 6);
  ImVec4* c = s.Colors;
  c[ImGuiCol_WindowBg] = ImVec4(0.105f, 0.12f, 0.14f, 0.97f);
  c[ImGuiCol_TitleBg] = ImVec4(0.08f, 0.09f, 0.1f, 1.f);
  c[ImGuiCol_TitleBgActive] = ImVec4(0.1f, 0.2f, 0.23f, 1.f);
  c[ImGuiCol_Header] = ImVec4(0.12f, 0.36f, 0.4f, 0.55f);
  c[ImGuiCol_HeaderHovered] = ImVec4(0.14f, 0.45f, 0.5f, 0.8f);
  c[ImGuiCol_HeaderActive] = ImVec4(0.16f, 0.52f, 0.58f, 1.f);
  c[ImGuiCol_Button] = ImVec4(0.15f, 0.33f, 0.37f, 0.8f);
  c[ImGuiCol_ButtonHovered] = ImVec4(0.18f, 0.45f, 0.5f, 1.f);
  c[ImGuiCol_ButtonActive] = ImVec4(0.2f, 0.55f, 0.6f, 1.f);
  c[ImGuiCol_FrameBg] = ImVec4(0.16f, 0.19f, 0.22f, 1.f);
  c[ImGuiCol_CheckMark] = ImVec4(0.35f, 0.85f, 0.9f, 1.f);
  c[ImGuiCol_SliderGrab] = ImVec4(0.3f, 0.7f, 0.76f, 1.f);
  c[ImGuiCol_Tab] = ImVec4(0.13f, 0.2f, 0.23f, 1.f);
  c[ImGuiCol_TabHovered] = ImVec4(0.18f, 0.45f, 0.5f, 1.f);
  c[ImGuiCol_TabSelected] = ImVec4(0.16f, 0.4f, 0.45f, 1.f);
  s.ScaleAllSizes(scale);
}

void load_font(float scale) {
  ImGuiIO& io = ImGui::GetIO();
  const char* candidates[] = {"C:/Windows/Fonts/segoeui.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                              "/System/Library/Fonts/Supplemental/Arial.ttf"};
  for (const char* f : candidates) {
    std::error_code ec;
    if (fs::exists(f, ec)) {
      if (io.Fonts->AddFontFromFileTTF(f, 17.f)) break;
    }
  }
  ImGui::GetStyle().FontScaleDpi = scale;
}

}  // namespace

// ------------------------------------------------------------------------------------------------
// window
// ------------------------------------------------------------------------------------------------

bool App::init_window(bool hidden, int w, int h) {
  if (!SDL_Init(SDL_INIT_VIDEO)) {
    LOG_ERROR("SDL_Init: %s", SDL_GetError());
    return false;
  }
  m_hidden = hidden;
  // OpenGL 4.3 (4.1 on macOS) for the game's renderer (the game view), else 3.3 without it
  SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 4);
#ifdef __APPLE__
  SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 1);
#else
  SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 3);
#endif
  SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_CORE);
  SDL_GL_SetAttribute(SDL_GL_DOUBLEBUFFER, 1);
  SDL_GL_SetAttribute(SDL_GL_DEPTH_SIZE, 24);
  SDL_GL_SetAttribute(SDL_GL_STENCIL_SIZE, 8);
  float scale = SDL_GetDisplayContentScale(SDL_GetPrimaryDisplay());
  if (scale <= 0.f) scale = 1.f;
  SDL_WindowFlags flags = SDL_WINDOW_OPENGL | SDL_WINDOW_RESIZABLE | SDL_WINDOW_HIGH_PIXEL_DENSITY;
  if (hidden) flags |= SDL_WINDOW_HIDDEN;
  else flags |= SDL_WINDOW_MAXIMIZED;
  m_window = SDL_CreateWindow("OpenGOAL Level Editor", (int)(w * (hidden ? 1.f : scale)),
                              (int)(h * (hidden ? 1.f : scale)), flags);
  if (!m_window) {
    LOG_ERROR("SDL_CreateWindow: %s", SDL_GetError());
    return false;
  }
  m_gl = SDL_GL_CreateContext(m_window);
  if (!m_gl) {
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 3);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 3);
    m_gl = SDL_GL_CreateContext(m_window);
  }
  if (!m_gl) {
    LOG_ERROR("no OpenGL 3.3 context: %s", SDL_GetError());
    return false;
  }
  SDL_GL_MakeCurrent(m_window, m_gl);
  SDL_GL_SetSwapInterval(1);
  if (!gladLoadGLLoader(reinterpret_cast<GLADloadproc>(SDL_GL_GetProcAddress))) {
    LOG_ERROR("cannot load the OpenGL functions");
    return false;
  }
  LOG_INFO("OpenGL %s, %s", (const char*)glGetString(GL_VERSION), (const char*)glGetString(GL_RENDERER));
  IMGUI_CHECKVERSION();
  ImGui::CreateContext();
  ImGuiIO& io = ImGui::GetIO();
  io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
  io.IniFilename = nullptr;  // the layout is fixed by the editor
  apply_style(hidden ? 1.f : scale);
  load_font(hidden ? 1.f : scale);
  ImGui_ImplSDL3_InitForOpenGL(m_window, m_gl);
  ImGui_ImplOpenGL3_Init("#version 330");
  std::string err;
  if (!renderer.init(&err) || !thumbs.init(&err) || !demos.init(&err)) {
    LOG_ERROR("renderer: %s", err.c_str());
    return false;
  }
  // the game's renderer, its shaders next to the executable
  const char* base = SDL_GetBasePath();
  {
    std::string perr;
    if (!particle_data.load(std::string(base ? base : "") + "data/jak3/particles.json", &perr)) LOG_WARN("%s", perr.c_str());
    thumbs.set_particles(&particle_data);
  }
  if (!game_view.init(std::string(base ? base : "") + "game-shaders", &game_view_error)) {
    LOG_WARN(tr("log.game_view_unavailable"), game_view_error.c_str());
    game_view_on = false;
  }
  return true;
}

void App::shutdown_window() {
  game_view.shutdown();
  thumbs.shutdown();
  demos.shutdown();
  renderer.shutdown();
  ImGui_ImplOpenGL3_Shutdown();
  ImGui_ImplSDL3_Shutdown();
  ImGui::DestroyContext();
  if (m_gl) SDL_GL_DestroyContext(m_gl);
  if (m_window) SDL_DestroyWindow(m_window);
  SDL_Quit();
}

void App::apply_options(const AppOptions& opts) {
  Prefs& prefs = Prefs::get();
  // the interface's language: the session's, the one chosen before, else the system's
  const char* base = SDL_GetBasePath();
  i18n_init(std::string(base ? base : "") + "lang", "en");
  const std::string lang = !opts.language.empty() ? opts.language : prefs.language;
  i18n_select(lang.empty() ? i18n_system_language() : lang);
  // the library: the session's, else the one used last when it holds an extracted game, else
  // library/ in the editor's own tree
  std::string root = opts.library;
  if (root.empty()) root = game_folder(prefs.library_root).empty() ? default_library_root() : prefs.library_root;
  set_library(root);
}

void App::new_document() {
  scene.clear();
  scene.settings = LevelSettings{};
  history.clear();
  selection.clear();
  primary = hovered = 0;
  stop_placement();
  m_prefab_meshes.clear();
  m_prefab_previews.clear();
  renderer.reset();
  game_view.reset();
  m_ocean.reset();
  m_ocean_map.clear();
  story_steps.clear();
  project_path.clear();
  saved_revision = history.revision();
  camera = Camera{};
}

void App::frame_all() {
  camera.yaw = 0.6f;
  camera.pitch = -0.45f;
  camera.frame(scene.focus_bounds());
}

void App::frame_selection() {
  const AABB b = selection_bounds();
  if (b.valid()) camera.frame(b);
}

void App::set_title() {
  if (!m_window) return;
  std::string name = !project_path.empty() ? to_path(project_path).filename().string()
                     : !scene.settings.level.empty() ? scene.settings.level
                                                     : std::string(tr("title.untitled"));
  std::string t = name + (history.revision() != saved_revision ? " *" : "") + " - OpenGOAL Level Editor";
  if (t != m_title) {
    m_title = t;
    SDL_SetWindowTitle(m_window, t.c_str());
  }
}

void App::poll_level_opening() {
  using namespace std::chrono_literals;
  if (!m_level_future.valid() || m_level_future.wait_for(0s) != std::future_status::ready) return;
  std::unique_ptr<Scene> loaded = m_level_future.get();
  if (!loaded) {
    LOG_ERROR(tr("log.level_failed"), m_level_name.c_str());
    return;
  }
  new_document();
  scene = std::move(*loaded);
  history.clear();
  saved_revision = history.revision();
  story_document_changed();
  frame_all();
  set_title();
  size_t actors = 0;
  for (const auto& n : scene.nodes) actors += n.kind == NodeKind::Actor;
  LOG_INFO(tr("log.level_opened"), m_level_name.c_str(), actors);
}

// ------------------------------------------------------------------------------------------------
// main loop
// ------------------------------------------------------------------------------------------------

int App::run(const AppOptions& opts) {
  Prefs::get().load();
  if (!init_window(opts.hidden, 1600, 900)) return 1;
  apply_options(opts);
  remote_start(opts.remote_port);
  new_document();
  if (!opts.open_path.empty()) open_project_file(opts.open_path);
  // the guided tour, the first time the editor starts
  if (!opts.hidden && !Prefs::read_only && !Prefs::get().guide_seen) {
    Prefs::get().guide_seen = true;
    show_help = true;
    guide_tab_request = 0;
  }
  uint64_t last = SDL_GetTicksNS();
  while (m_running) {
    SDL_Event e;
    while (SDL_PollEvent(&e)) process_event(&e);
    uint64_t now = SDL_GetTicksNS();
    float dt = std::min((now - last) / 1e9f, 0.1f);
    last = now;
    glBindFramebuffer(GL_FRAMEBUFFER, 0);
    if (m_ui_shot_pending) {
      // a picture of the whole window asked by ui_input: this frame drawn apart too (a hidden
      // window has no pixels of its own); the answer waits for it
      OffscreenTarget shot;
      int fbw = 0, fbh = 0;
      SDL_GetWindowSizeInPixels(m_window, &fbw, &fbh);
      if (shot.create(fbw, fbh)) {
        glBindFramebuffer(GL_FRAMEBUFFER, shot.fbo);
        frame(dt);
        glFinish();
        const std::vector<uint8_t> png = shot.png();
        m_ui_shot = base64_encode(png.data(), png.size());
        glBindFramebuffer(GL_FRAMEBUFFER, 0);
      }
      m_ui_shot_pending = false;
    } else {
      frame(dt);
    }
    SDL_GL_SwapWindow(m_window);
    set_title();
  }
  extract_stop();
  Prefs::get().save();
  shutdown_window();
  return 0;
}

// One frame into the bound framebuffer: interface, input, tools, thumbnails, viewport.
void App::frame(float dt) {
  const uint64_t frame_start = SDL_GetTicksNS();
  struct Timer {
    const uint64_t& start;
    float& ms;
    ~Timer() { ms = ms * 0.9f + (SDL_GetTicksNS() - start) / 1e6f * 0.1f; }
  } timer{frame_start, m_frame_ms};
  m_water_time += dt;
  rs.time = m_water_time;
  update_preview(dt);
  update_weather(dt);
  // the scene cache (world matrices...) is kept while nothing changes: the history did not move,
  // no gizmo drag, no field being dragged (nodes added or removed are detected by the cache)
  if (history.revision() != m_frame_revision || gizmo.active() || m_free_rotating || ImGui::IsAnyItemActive())
    scene.begin_frame();
  m_frame_revision = history.revision();
  // nodes added or brought back by the history are shown as the story says
  if (history.revision() != m_story_revision) apply_story_view();
  process_dialog_results();
  store.update();
  palette.update();
  apply_library_index();
  poll_level_opening();
  poll_level_adding();
  poll_navmesh_generation();
  poll_ocean_land();
  poll_game_prefabs();
  remote_poll();
  extract_poll();

  ImGui_ImplOpenGL3_NewFrame();
  ImGui_ImplSDL3_NewFrame();
  ImGui::NewFrame();
  draw_ui();
  handle_shortcuts();
  update_camera(dt);
  viewport_interaction();
  draw_cursor_info();
  ui_labels();
  ui_viewport_hint();
  thumbs.update();

  int fbw, fbh, ww, wh;
  SDL_GetWindowSizeInPixels(m_window, &fbw, &fbh);
  SDL_GetWindowSize(m_window, &ww, &wh);
  float sx = ww > 0 ? (float)fbw / ww : 1.f, sy = wh > 0 ? (float)fbh / wh : 1.f;
  glViewport(0, 0, fbw, fbh);
  glClearColor(0.07f, 0.08f, 0.09f, 1.f);
  glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
  int px = (int)(m_vp_x * sx), pw = (int)(m_vp_w * sx), ph = (int)(m_vp_h * sy);
  int py = fbh - (int)((m_vp_y + m_vp_h) * sy);
  // what the tools changed this frame is drawn now
  if (history.revision() != m_frame_revision || gizmo.active() || m_free_rotating || ImGui::IsAnyItemActive())
    scene.begin_frame();
  m_frame_revision = history.revision();
  set_ocean_backdrop();
  // the game view: the decor drawn by the game's renderer, the editor's things over it with the
  // game's camera and depth
  const bool game = game_view_on && game_view.ready() && document_open();
  RenderSettings view_rs = rs;
  if (game) game_view_settings(view_rs);
  renderer.begin(camera, px, py, pw, ph, view_rs, m_vp_w, m_vp_h);
  if (game) {
    Mat4 clip;
    game_view.draw_opaque(scene, camera, px, py, pw, ph, &clip);
    renderer.set_game_depth(&clip);
  }
  update_outlines();
  renderer.draw_scene(scene, view_rs, selection, hovered);
  if (game) game_view.draw_translucent();
  if (rs.show_ocean && m_ocean && !scene.nodes.empty())
    renderer.draw_water(*m_ocean, Mat4::translate({0, m_ocean_height, 0}), ocean_look(view_rs), view_rs);
  update_effects(dt);
  draw_effects();
  draw_rain();
  draw_navmesh_overlay();
  draw_gaps_overlay();
  draw_tool_overlays();
  draw_preview_overlay();
  renderer.set_game_depth(nullptr);
  glViewport(0, 0, fbw, fbh);

  ImGui::Render();
  ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
}

void App::game_view_settings(RenderSettings& out) {
  out.game_view = true;
  out.show_grid = false;
  out.far_ocean = false;
  // the models lit and fogged as the game does at that hour (game_mood.h)
  const GameMood m = game_mood(game_view.mood);
  out.game_light = true;
  for (int i = 0; i < 3; i++) {
    out.light_ambient[i] = m.light_ambient[i];
    for (int k = 0; k < 3; k++) {
      out.light_dir[i][k] = m.light_dir[i][k];
      out.light_color[i][k] = m.light_color[i][k];
    }
    out.game_fog[i] = m.fog_color[i] / 255.f;
  }
  out.game_fog[3] = (m.fog_max - m.fog_min) / 255.f;
  out.game_fog_start = m.fog_start / 4096.f;
  out.game_fog_end = m.fog_end / 4096.f;
}

void App::process_event(const void* ev) {
  const SDL_Event& e = *(const SDL_Event*)ev;
  ImGui_ImplSDL3_ProcessEvent(&e);
  switch (e.type) {
    case SDL_EVENT_QUIT:
      request_quit();
      break;
    case SDL_EVENT_WINDOW_CLOSE_REQUESTED:
      if (e.window.windowID == SDL_GetWindowID(m_window)) request_quit();
      break;
    case SDL_EVENT_DROP_FILE:
      // a project dropped on the window opens; a folder is taken as the library
      if (e.drop.data) {
        std::string path = e.drop.data;
        std::error_code ec;
        if (fs::is_directory(to_path(path), ec)) set_library(path);
        else confirm_discard([this, path]() { open_project_file(path); });
      }
      break;
    case SDL_EVENT_MOUSE_MOTION:
      m_mouse_rel.x += e.motion.xrel;
      m_mouse_rel.y += e.motion.yrel;
      break;
    default:
      break;
  }
}

void App::update_camera(float dt) {
  ImGuiIO& io = ImGui::GetIO();
  Vec2 rel = m_mouse_rel;
  m_mouse_rel = {0, 0};
  // the terrain brush: the right button held and the wheel raise or dig where its square is
  if (tool == ToolMode::Terrain && terrain_right_button(rel)) return;
  const bool rmb = ImGui::IsMouseDown(ImGuiMouseButton_Right);
  // on the selection (or what is placed) the right button turns it, elsewhere it looks around
  if (!m_looking && !m_free_rotating && rmb && m_mouse_in_vp && ImGui::IsMouseClicked(ImGuiMouseButton_Right) &&
      !start_free_rotation()) {
    m_looking = true;
    SDL_SetWindowRelativeMouseMode(m_window, true);
  }
  if (m_free_rotating) {
    if (rmb) update_free_rotation(rel);
    else finish_free_rotation();
    return;
  }
  if (m_looking && !rmb) {
    m_looking = false;
    SDL_SetWindowRelativeMouseMode(m_window, false);
  }
  if (m_looking) {
    camera.yaw -= rel.x * 0.0035f;
    camera.pitch = std::clamp(camera.pitch - rel.y * 0.0035f, -1.55f, 1.55f);
    if (io.MouseWheel != 0) camera.speed = std::clamp(camera.speed * std::pow(1.2f, io.MouseWheel), 0.5f, 2000.f);
    const bool* keys = SDL_GetKeyboardState(nullptr);
    Vec3 mv{0, 0, 0};
    // physical keys: WASD on QWERTY is ZQSD on AZERTY
    if (keys[SDL_SCANCODE_W]) mv += camera.forward();
    if (keys[SDL_SCANCODE_S]) mv -= camera.forward();
    if (keys[SDL_SCANCODE_D]) mv += camera.right();
    if (keys[SDL_SCANCODE_A]) mv -= camera.right();
    if (keys[SDL_SCANCODE_E] || keys[SDL_SCANCODE_SPACE]) mv += Vec3{0, 1, 0};
    if (keys[SDL_SCANCODE_Q] || keys[SDL_SCANCODE_C]) mv -= Vec3{0, 1, 0};
    float sp = camera.speed;
    if (keys[SDL_SCANCODE_LSHIFT] || keys[SDL_SCANCODE_RSHIFT]) sp *= 4.f;
    if (keys[SDL_SCANCODE_LCTRL]) sp *= 0.25f;
    if (length2(mv) > 0) camera.pos += normalize(mv) * sp * dt;
    return;
  }
  if (!m_mouse_in_vp && !m_panning && !m_orbiting) return;
  // wheel: move forward (Ctrl + wheel turns the prefab being placed)
  if (m_mouse_in_vp && io.MouseWheel != 0) {
    if (tool == ToolMode::Place && io.KeyCtrl) {
      placement.yaw += io.MouseWheel * radians(15.f);
    } else if (tool == ToolMode::Terrain && io.KeyCtrl) {
      brush.size = std::clamp(brush.size * std::pow(1.15f, io.MouseWheel), 0.5f, 64.f);  // the square's size
    } else {
      camera.pos += camera.forward() * io.MouseWheel * std::max(camera.speed * 0.35f, 0.4f);
    }
  }
  // middle button: pan
  if (ImGui::IsMouseClicked(ImGuiMouseButton_Middle) && m_mouse_in_vp) m_panning = true;
  if (m_panning) {
    if (!ImGui::IsMouseDown(ImGuiMouseButton_Middle)) {
      m_panning = false;
    } else {
      float k = std::max(camera.speed, 1.f) * 0.006f;
      camera.pos -= camera.right() * (io.MouseDelta.x * k);
      camera.pos += camera.up() * (io.MouseDelta.y * k);
    }
  }
  // Alt + left button: orbit around the selection or the point under the cursor
  if (io.KeyAlt && ImGui::IsMouseClicked(ImGuiMouseButton_Left) && m_mouse_in_vp) {
    m_orbiting = true;
    if (!selection.empty()) {
      m_orbit_pivot = selection_frame().col3(3);
    } else {
      PickHit h = pick_surface(m_mouse_vp);
      m_orbit_pivot = h.hit() ? h.point : camera.pos + camera.forward() * 10.f;
    }
    m_orbit_dist = std::max(length(m_orbit_pivot - camera.pos), 0.5f);
    camera.look_at(m_orbit_pivot);
  }
  if (m_orbiting) {
    if (!ImGui::IsMouseDown(ImGuiMouseButton_Left)) {
      m_orbiting = false;
    } else {
      camera.yaw -= io.MouseDelta.x * 0.005f;
      camera.pitch = std::clamp(camera.pitch - io.MouseDelta.y * 0.005f, -1.55f, 1.55f);
      camera.pos = m_orbit_pivot - camera.forward() * m_orbit_dist;
    }
  }
}

bool App::start_free_rotation() {
  if (tool == ToolMode::Place) {
    if (!m_place_valid) return false;
    m_free_rotating_placement = true;
  } else {
    // the selection, under the cursor
    if (selection.empty() || gizmo.active()) return false;
    PickFilter pf;
    pf.actors = rs.show_actors;
    pf.navmesh = show_navmesh;
    pf.include_locked = true;
    const uint32_t under = selectable(pick(scene, camera.ray(m_mouse_vp.x, m_mouse_vp.y, m_vp_w, m_vp_h), pf).node);
    if (!under || !selection.count(under)) return false;
    m_drag_change = std::make_unique<NodeChange>(scene);
    m_drag_start.clear();
    for (auto id : selection_roots())
      if (const Node* n = scene.find(id)) {
        m_drag_change->save(id);
        m_drag_start[id] = scene.world(*n);
      }
    if (m_drag_start.empty()) {
      m_drag_change.reset();
      return false;
    }
    m_free_pivot = selection_frame().col3(3);
    m_free_rotating_placement = false;
  }
  m_free_rotating = true;
  m_free_right = camera.right();
  m_free_yaw = m_free_pitch = 0;
  m_free_tilt0 = placement.tilt;
  // the cursor stays where it is: the mouse turns it as far as wanted
  SDL_SetWindowRelativeMouseMode(m_window, true);
  return true;
}

void App::update_free_rotation(const Vec2& rel) {
  const ImGuiIO& io = ImGui::GetIO();
  const float k = io.KeyShift ? 0.002f : 0.008f;  // radians per pixel
  m_free_yaw += rel.x * k;
  m_free_pitch += rel.y * k;
  float yaw = m_free_yaw, pitch = m_free_pitch;
  if (gizmo.snap.enabled != io.KeyCtrl && gizmo.snap.rotate_deg > 0.f) {
    const float s = radians(gizmo.snap.rotate_deg);
    yaw = std::round(yaw / s) * s;
    pitch = std::round(pitch / s) * s;
  }
  const Quat r = (Quat::axis_angle({0, 1, 0}, yaw) * Quat::axis_angle(m_free_right, pitch)).normalized();
  if (m_free_rotating_placement) {
    placement.tilt = (r * m_free_tilt0).normalized();
    // the preview where it was, turned (the cursor does not move it while it turns)
    m_place_matrix = Mat4::translate(m_place_matrix.col3(3)) * Mat4::rotate(placement.tilt) *
                     Mat4::rotate(Quat::axis_angle({0, 1, 0}, placement.yaw));
    return;
  }
  const Mat4 turn = Mat4::translate(m_free_pivot) * Mat4::rotate(r) * Mat4::translate(-m_free_pivot);
  for (const auto& [id, start] : m_drag_start)
    if (Node* n = scene.find(id)) scene.set_world(*n, turn * start);
}

void App::finish_free_rotation() {
  m_free_rotating = false;
  SDL_SetWindowRelativeMouseMode(m_window, false);
  if (m_free_rotating_placement || !m_drag_change) return;
  // what was placed is lit like where it is now
  std::vector<uint32_t> roots;
  for (const auto& [id, start] : m_drag_start) roots.push_back(id);
  relight_moved(*m_drag_change, roots);
  history.push(m_drag_change->commit(tr("undo.rotate")));
  m_drag_change.reset();
  m_drag_start.clear();
}

void App::viewport_interaction() {
  ImGuiIO& io = ImGui::GetIO();
  m_mouse_vp = {io.MousePos.x - m_vp_x, io.MousePos.y - m_vp_y};
  bool inside = m_mouse_vp.x >= 0 && m_mouse_vp.y >= 0 && m_mouse_vp.x < m_vp_w && m_mouse_vp.y < m_vp_h;
  // while a prefab is dragged from its panel ImGui owns the mouse: the viewport is "under" it
  // when no window is
  const bool modal = m_show_discard || m_show_new_prefab || m_show_new_group || ImGui::IsPopupOpen(nullptr, ImGuiPopupFlags_AnyPopupId);
  m_mouse_over_vp = inside && !ImGui::IsWindowHovered(ImGuiHoveredFlags_AnyWindow) && !modal;
  m_mouse_in_vp = inside && !io.WantCaptureMouse && !modal;
  const ImGuiPayload* payload = ImGui::GetDragDropPayload();
  if (!payload || !payload->IsDataType("OGLE_PREFAB")) m_drag_from_panel = false;
  if (m_looking || m_orbiting || m_panning || m_free_rotating) {
    hovered = 0;
    return;
  }
  bool clicked = m_mouse_in_vp && ImGui::IsMouseClicked(ImGuiMouseButton_Left) && !io.KeyAlt;
  bool released = ImGui::IsMouseReleased(ImGuiMouseButton_Left);
  if (clicked) m_lmb_in_vp = true;
  if (tool == ToolMode::Place) place_tool(clicked);
  else if (tool == ToolMode::Topology) topology_tool(clicked, released);
  else if (tool == ToolMode::Terrain) terrain_tool(clicked, released);
  else select_tool(clicked, released);
  if (released) m_lmb_in_vp = false;
}

Vec3 App::drag_offset(Vec2 px, const Vec3& at) const {
  // a pixel of the mouse, a pixel of the view where it stands
  const float mpp = 2.f * length(at - camera.pos) * std::tan(radians(camera.fov_deg) * 0.5f) / std::max(1.f, m_vp_h);
  Vec3 side = camera.right();
  side.y = 0;
  side = length2(side) > 1e-6f ? normalize(side) : Vec3{1, 0, 0};
  Vec3 ahead{-side.z, 0, side.x};  // the view's forward, level
  if (dot(ahead, camera.forward()) < 0) ahead = -ahead;
  const bool depth = ImGui::GetIO().KeyCtrl;  // up and down: forward and back instead of higher and lower
  Vec3 off = side * (px.x * mpp) + (depth ? ahead : Vec3{0, 1, 0}) * (-px.y * mpp);
  if (gizmo.snap.enabled && gizmo.snap.translate > 0) {
    const float s = gizmo.snap.translate;
    off = {std::round(off.x / s) * s, std::round(off.y / s) * s, std::round(off.z / s) * s};
  }
  return off;
}

std::vector<uint32_t> App::box_nodes(Vec2 a, Vec2 b) const {
  const float x0 = std::min(a.x, b.x), x1 = std::max(a.x, b.x);
  const float y0 = std::min(a.y, b.y), y1 = std::max(a.y, b.y);
  std::vector<uint32_t> out;
  const SceneCache& cache = scene.cache();
  for (size_t i = 0; i < scene.nodes.size() && i < cache.count; i++) {
    const Node& n = scene.nodes[i];
    if ((n.kind != NodeKind::Render && n.kind != NodeKind::Actor && (n.kind != NodeKind::NavMesh || !show_navmesh)) ||
        (n.locked && !select_terrain) || !cache.visible[i] || cache.reference[i])
      continue;
    if (cache.actor[i]) continue;  // an actor's model: the actor is selected instead
    if (n.kind == NodeKind::Actor && !rs.show_actors) continue;
    const Vec3 center = cache.bounds[i].valid() ? cache.bounds[i].center() : cache.worlds[i].col3(3);
    Vec2 s;
    if (!renderer.project(center, &s)) continue;
    if (s.x >= x0 && s.x <= x1 && s.y >= y0 && s.y <= y1) out.push_back(n.id);
  }
  return out;
}

void App::outline_subtree(uint32_t id, std::vector<uint32_t>& out, size_t cap) const {
  const SceneCache& c = scene.cache();
  const int i = scene.index_of(id);
  if (i < 0 || (size_t)i >= c.count) return;
  static thread_local std::vector<uint32_t> nodes;
  nodes.clear();
  scene.subtree((size_t)i, nodes);
  for (uint32_t k : nodes) {
    if (out.size() >= cap) return;
    if (scene.nodes[k].kind == NodeKind::Render) out.push_back(scene.nodes[k].id);
  }
}

void App::update_outlines() {
  renderer.outline_hover.clear();
  renderer.outline_group.clear();
  // the selection with what is under it (a group of a whole level: its first thousands)
  const uint64_t key = scene.cache().version;
  if (m_outline_key != key || m_outline_sel != selection) {
    m_outline_key = key;
    m_outline_sel = selection;
    renderer.outline_selection.clear();
    for (auto id : selection_roots()) outline_subtree(id, renderer.outline_selection, 6000);
  }
  if (tool != ToolMode::Select) return;
  if (m_box_selecting) {
    for (auto id : m_box_preview) outline_subtree(id, renderer.outline_hover, 4000);
    return;
  }
  if (!hovered || selection.count(hovered)) return;
  outline_subtree(hovered, renderer.outline_hover, 4000);
  if (const uint32_t g = group_of(hovered))
    if (const GroupInfo* gi = group_info(g))
      for (auto r : gi->roots)
        if (r != hovered && !selection.count(r)) outline_subtree(r, renderer.outline_group, 4000);
}

void App::draw_cursor_info() {
  if (tool != ToolMode::Select || !m_mouse_over_vp) return;
  // what the cursor points at, next to it: the element, its group, what a click does
  ImDrawList* dl = ImGui::GetForegroundDrawList();
  const ImVec2 at(m_vp_x + m_mouse_vp.x + 18, m_vp_y + m_mouse_vp.y + 14);
  std::string text;
  if (m_box_selecting) {
    const ImGuiIO& io = ImGui::GetIO();
    text = strf(tr(io.KeyCtrl ? "cursor.box_remove" : io.KeyShift ? "cursor.box_add" : "cursor.box"), m_box_preview.size());
  } else if (m_long_press_ring > 0.f) {
    // the button held still: a ring filling up, then the whole group is selected
    const ImVec2 c(m_vp_x + m_mouse_vp.x, m_vp_y + m_mouse_vp.y);
    dl->AddCircle(c, 16, IM_COL32(255, 255, 255, 60), 32, 3.f);
    dl->PathArcTo(c, 16, -1.5708f, -1.5708f + 6.2832f * m_long_press_ring, 32);
    dl->PathStroke(IM_COL32(255, 170, 60, 230), 0, 3.f);
    text = tr("cursor.hold");
  } else if (Vec3 d; move_delta(&d)) {
    // how far it went, whatever the way
    text = strf(tr("cursor.move_delta"), d.x, d.y, d.z, length(d));
    if (m_moving_selection) text += "\n" + std::string(tr(ImGui::GetIO().KeyCtrl ? "cursor.moving_depth" : "cursor.moving"));
  } else if (hovered && ImGui::GetTime() - m_hover_since > 0.35) {
    const Node* n = scene.find(hovered);
    if (!n) return;
    text = n->name;
    if (const uint32_t g = group_of(hovered))
      if (const GroupInfo* gi = group_info(g)) {
        const bool all = std::all_of(gi->roots.begin(), gi->roots.end(), [&](uint32_t r) { return selection.count(r); });
        text += "\n" + strf(tr(gi->prefab ? "cursor.prefab" : "cursor.group"), gi->name.c_str(), gi->roots.size());
        if (!all) text += "\n" + std::string(tr("cursor.select_group"));
      }
  }
  if (text.empty()) return;
  const ImVec2 size = ImGui::CalcTextSize(text.c_str());
  dl->AddRectFilled(ImVec2(at.x - 5, at.y - 3), ImVec2(at.x + size.x + 5, at.y + size.y + 3), IM_COL32(20, 22, 26, 210), 4.f);
  dl->AddText(at, IM_COL32(235, 235, 225, 255), text.c_str());
}

bool App::move_delta(Vec3* d) const {
  if (m_drag_start.empty() || !(m_moving_selection || (gizmo.active() && gizmo.op == GizmoOp::Translate))) return false;
  const auto& [id, start] = *m_drag_start.begin();
  if (!scene.find(id)) return false;
  *d = scene.world(id).col3(3) - start.col3(3);
  return true;
}

void App::draw_move_preview() {
  // where it was, see-through, and the way from there: along x (red), z (blue), y (green)
  Vec3 d;
  if (!move_delta(&d) || length(d) < 1e-3f) return;
  const Mat4 back = Mat4::translate(d * -1.f);
  const SceneCache& cache = scene.cache();
  std::vector<uint32_t> tree;
  size_t drawn = 0;
  for (const auto& [id, start] : m_drag_start) {
    const int i = scene.index_of(id);
    if (i < 0) continue;
    tree.clear();
    scene.subtree((size_t)i, tree);
    for (uint32_t k : tree) {
      Node& n = scene.nodes[k];
      // ponytail: 3000 pieces at most (a whole level dragged: its first ones)
      if (n.kind != NodeKind::Render || !n.mesh || k >= cache.worlds.size() || ++drawn > 3000) continue;
      renderer.draw_mesh(*n.mesh, back * cache.worlds[k], scene.materials, scene.textures, rs, {0.35f, 0.75f, 1.f, 0.3f}, 0.35f);
    }
  }
  const Vec3 a = m_drag_pivot0, b{a.x + d.x, a.y, a.z}, c{a.x + d.x, a.y, a.z + d.z}, e = a + d;
  renderer.line(a, b, IM_COL32(235, 80, 70, 255));
  renderer.line(b, c, IM_COL32(80, 140, 255, 255));
  renderer.line(c, e, IM_COL32(110, 220, 90, 255));
  renderer.line(a, e, IM_COL32(255, 255, 255, 220));
  renderer.flush(false);
}

void App::select_tool(bool clicked, bool released) {
  ImGuiIO& io = ImGui::GetIO();
  ImDrawList* dl = ImGui::GetBackgroundDrawList();
  PickFilter pf;
  pf.actors = rs.show_actors;
  pf.navmesh = show_navmesh;
  pf.include_locked = true;
  m_long_press_ring = 0.f;
  // hover
  if (m_mouse_in_vp && !gizmo.active() && !m_box_selecting && !m_moving_selection) {
    const uint32_t h = selectable(pick(scene, camera.ray(m_mouse_vp.x, m_mouse_vp.y, m_vp_w, m_vp_h), pf).node);
    if (h != hovered) m_hover_since = ImGui::GetTime();
    hovered = h;
  } else if (!m_mouse_in_vp) {
    hovered = 0;
  }

  // a double click: the whole prefab or group (else the assembly) of the element under the cursor,
  // before the gizmo (after the first click, the element's gizmo is often right under it)
  if (clicked && hovered && ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left) && !io.KeyShift && !io.KeyCtrl) {
    select_whole(hovered);
    m_skip_release = true;
    return;
  }
  if (m_skip_release) {
    if (released || !ImGui::IsMouseDown(ImGuiMouseButton_Left)) m_skip_release = false;
    return;
  }

  // gizmo
  if (!selection_roots().empty() && !m_moving_selection) {
    GizmoInput gi;
    gi.mouse = m_mouse_vp;
    gi.viewport_pos = {m_vp_x, m_vp_y};
    gi.viewport_size = {m_vp_w, m_vp_h};
    gi.mouse_in_viewport = m_mouse_in_vp && !m_box_selecting;
    gi.pressed = clicked;
    gi.down = ImGui::IsMouseDown(ImGuiMouseButton_Left);
    gi.released = released;
    gi.fine = io.KeyShift;
    gi.snap_toggle = io.KeyCtrl;
    GizmoResult res = gizmo.update(camera, renderer, gi, selection_frame(), dl);
    if (res.started) {
      m_drag_change = std::make_unique<NodeChange>(scene);
      m_drag_start.clear();
      m_drag_scale.clear();
      m_drag_pivot0 = selection_frame().col3(3);
      for (auto id : selection_roots()) {
        const Node* n = scene.find(id);
        if (!n) continue;
        m_drag_change->save(id);
        m_drag_start[id] = scene.world(*n);
        m_drag_scale[id] = n->local.s;
      }
    }
    if (res.dragging) {
      if (gizmo.op == GizmoOp::Scale) {
        // each element along its own axes, around its origin
        for (auto& [id, s0] : m_drag_scale)
          if (Node* n = scene.find(id)) n->local.s = {s0.x * res.scale.x, s0.y * res.scale.y, s0.z * res.scale.z};
      } else {
        for (auto& [id, start] : m_drag_start)
          if (Node* n = scene.find(id)) scene.set_world(*n, res.delta * start);
      }
    }
    if (res.finished && m_drag_change) {
      const char* label = gizmo.op == GizmoOp::Translate ? "undo.move" : gizmo.op == GizmoOp::Rotate ? "undo.rotate" : "undo.scale";
      // what was placed is lit like where it goes
      std::vector<uint32_t> roots;
      for (const auto& [id, start] : m_drag_start) roots.push_back(id);
      relight_moved(*m_drag_change, roots);
      history.push(m_drag_change->commit(tr(label)));
      m_drag_change.reset();
      m_drag_start.clear();
    }
    if (gizmo.active() || (gizmo.hovering() && clicked)) return;
  }

  // the button pressed: a click, a box, the selection dragged, or held still (its whole group)
  if (clicked) {
    m_box_start = m_mouse_vp;
    m_box_selecting = false;
    m_long_pressed = false;
    m_press_time = ImGui::GetTime();
    m_press_target = hovered;
    m_press_on_selection = hovered && selection.count(hovered);
    m_box_preview.clear();
  }
  const bool down = m_lmb_in_vp && ImGui::IsMouseDown(ImGuiMouseButton_Left) && !gizmo.active();
  if (down && !m_long_pressed) {
    const float moved = (m_mouse_vp - m_box_start).length();
    if (!m_box_selecting && !m_moving_selection && moved > 6.f) {
      if (m_press_on_selection) {
        // the selection dragged: left and right, up and down (Ctrl: forward and back)
        m_moving_selection = true;
        m_move_px = {0, 0};
        m_drag_change = std::make_unique<NodeChange>(scene);
        m_drag_start.clear();
        for (auto id : selection_roots())
          if (const Node* n = scene.find(id)) {
            m_drag_change->save(id);
            m_drag_start[id] = scene.world(*n);
          }
        const PickHit h = pick(scene, camera.ray(m_box_start.x, m_box_start.y, m_vp_w, m_vp_h), pf);
        m_move_anchor = h.hit() ? h.point : selection_frame().col3(3);
        m_drag_pivot0 = selection_frame().col3(3);
      } else {
        m_box_selecting = true;
      }
    }
    if (m_moving_selection) {
      m_move_px = m_move_px + Vec2{io.MouseDelta.x, io.MouseDelta.y} * (io.KeyShift ? 0.25f : 1.f);
      const Mat4 t = Mat4::translate(drag_offset(m_move_px, m_move_anchor));
      for (auto& [id, start] : m_drag_start)
        if (Node* n = scene.find(id)) scene.set_world(*n, t * start);
    } else if (m_box_selecting) {
      ImVec2 a(m_vp_x + std::min(m_box_start.x, m_mouse_vp.x), m_vp_y + std::min(m_box_start.y, m_mouse_vp.y));
      ImVec2 b(m_vp_x + std::max(m_box_start.x, m_mouse_vp.x), m_vp_y + std::max(m_box_start.y, m_mouse_vp.y));
      dl->AddRectFilled(a, b, IM_COL32(80, 190, 210, 40));
      dl->AddRect(a, b, IM_COL32(80, 190, 210, 200));
      // what it would select, outlined as it grows (found again when the box changed, 20 times a second)
      if ((m_mouse_vp - m_box_preview_end).length() > 1.f && ImGui::GetTime() - m_box_preview_time > 0.05) {
        m_box_preview = box_nodes(m_box_start, m_mouse_vp);
        m_box_preview_end = m_mouse_vp;
        m_box_preview_time = ImGui::GetTime();
      }
    } else if (moved <= 6.f && m_press_target) {
      // held still on an element: after a moment, its whole group (or assembly)
      const double held = ImGui::GetTime() - m_press_time;
      if (held > 0.15) m_long_press_ring = (float)std::min(1.0, (held - 0.15) / 0.45);
      if (held > 0.6) {
        select_whole(m_press_target);
        m_long_pressed = true;
      }
    }
  }
  if (released && m_lmb_in_vp) {
    if (m_moving_selection) {
      std::vector<uint32_t> roots;
      for (const auto& [id, start] : m_drag_start) roots.push_back(id);
      if (m_drag_change) {
        relight_moved(*m_drag_change, roots);
        history.push(m_drag_change->commit(tr("undo.move")));
      }
      m_drag_change.reset();
      m_drag_start.clear();
      m_moving_selection = false;
    } else if (m_long_pressed) {
      // done while it was held
    } else if (m_box_selecting) {
      const std::vector<uint32_t> inside = box_nodes(m_box_start, m_mouse_vp);
      if (io.KeyCtrl) {
        for (auto id : inside) selection.erase(id);  // Ctrl: taken out of the selection
        if (!selection.count(primary)) primary = selection.empty() ? 0 : *selection.begin();
      } else {
        if (!io.KeyShift) selection.clear();
        for (auto id : inside) selection.insert(id);
        if (!inside.empty()) primary = inside.back();
      }
    } else if (m_mouse_in_vp) {
      uint32_t target = hovered;
      // a click again where the last one was, the camera still: the next element under the cursor
      // (what stands behind, inside or under the one selected)
      const bool again = (m_mouse_vp - m_last_click).length() < 4.f && length(camera.pos - m_last_click_cam) < 1e-4f &&
                         std::fabs(camera.yaw - m_last_click_yaw) < 1e-5f &&
                         std::fabs(camera.pitch - m_last_click_pitch) < 1e-5f;
      if (again && !io.KeyShift && !io.KeyCtrl && selection.size() == 1) {
        std::vector<uint32_t> under;
        for (const PickHit& h : pick_all(scene, camera.ray(m_mouse_vp.x, m_mouse_vp.y, m_vp_w, m_vp_h), pf)) {
          const Node* hn = scene.find(h.node);
          if (hn && hn->locked && !select_terrain) break;  // the ground hides what is under it
          const uint32_t id = selectable(h.node);
          if (id && std::find(under.begin(), under.end(), id) == under.end()) under.push_back(id);
        }
        auto it = std::find(under.begin(), under.end(), primary);
        if (it != under.end() && under.size() > 1) target = under[(it - under.begin() + 1) % under.size()];
      }
      m_last_click = m_mouse_vp;
      m_last_click_cam = camera.pos;
      m_last_click_yaw = camera.yaw;
      m_last_click_pitch = camera.pitch;
      if (target) {
        if (io.KeyShift || io.KeyCtrl) toggle_select(target);
        // a click on an element of a selection of several keeps them all (a drag would move them);
        // a click again there takes that element alone
        else if (selection.count(target) && selection.size() > 1 && !again) primary = target;
        else select_only(target);
      } else if (!io.KeyShift && !io.KeyCtrl) {
        select_only(0);
      }
    }
    m_box_selecting = false;
    m_box_preview.clear();
    m_box_preview_end = {-1, -1};
  }
}

uint32_t App::selectable(uint32_t node) const {
  const Node* n = scene.find(node);
  if (!n) return 0;
  // the ground (locked) hides what is behind it but is not chosen, unless asked
  if (n->locked && !select_terrain) return 0;
  // an actor's model selects the actor
  if (uint32_t actor = scene.actor_of(*n)) return actor;
  return n->id;
}

void App::place_tool(bool clicked) {
  hovered = 0;
  if (m_place_drag) {
    ImGuiIO& io = ImGui::GetIO();
    if (ImGui::IsMouseDown(ImGuiMouseButton_Left)) {
      // the mouse moves it left and right, up and down (Ctrl: forward and back), Shift finer
      m_place_drag_px = m_place_drag_px + Vec2{io.MouseDelta.x, io.MouseDelta.y} * (io.KeyShift ? 0.25f : 1.f);
      m_place_matrix = Mat4::translate(drag_offset(m_place_drag_px, m_place_anchor.col3(3))) * m_place_anchor;
      m_place_valid = true;
      return;
    }
    m_place_drag = false;
    // placed: selected (a prefab all of it) with the gizmo on it, to move it again at once; Shift
    // held when the button is released keeps placing copies
    if (!place_at(m_place_matrix).empty() && !io.KeyShift) {
      const std::set<uint32_t> placed = selection;
      const uint32_t first = primary;
      stop_placement();
      selection = placed;
      primary = first;
    }
    return;
  }
  m_place_valid = false;
  const bool over = placement.once ? m_mouse_over_vp : m_mouse_in_vp;
  if (over) {
    Ray r = camera.ray(m_mouse_vp.x, m_mouse_vp.y, m_vp_w, m_vp_h);
    m_place_matrix = placement_matrix(pick_surface(m_mouse_vp), r);
    m_place_valid = true;
  }
  if (placement.once) {
    // dragged from the Prefabs panel: dropped when the button is released
    if (ImGui::IsMouseReleased(ImGuiMouseButton_Left)) {
      if (m_place_valid) place_at(m_place_matrix);
      stop_placement();
    }
    return;
  }
  if (clicked && m_place_valid) {
    m_place_drag = true;
    m_place_anchor = m_place_matrix;
    m_place_drag_px = {0, 0};
  }
}

void App::draw_navmesh_overlay() {
  if (!show_navmesh) return;
  const SceneCache& cache = scene.cache();
  // a color per mesh, see-through, its borders (where creatures stop) brighter
  static const Vec3 kColors[] = {{0.15f, 0.85f, 0.75f}, {0.95f, 0.75f, 0.2f}, {0.55f, 0.6f, 1.f},
                                 {0.95f, 0.4f, 0.55f},  {0.5f, 0.9f, 0.3f},   {0.9f, 0.55f, 0.95f}};
  std::unordered_set<uint64_t> used;
  int index = 0;
  for (size_t i = 0; i < scene.nodes.size(); i++) {
    const Node& n = scene.nodes[i];
    if (n.kind != NodeKind::NavMesh || !n.mesh || !cache.visible[i]) continue;
    const uint64_t key = n.mesh->uid;
    auto it = m_nav_draw_cache.find(key);
    if (it == m_nav_draw_cache.end()) {
      NavMeshData m;
      if (!navmesh_of_node(n, &m)) continue;
      it = m_nav_draw_cache.emplace(key, std::move(m)).first;
    }
    const NavMeshData& m = it->second;
    used.insert(key);
    const Mat4& w = cache.worlds[i];
    const bool sel = selection.count(n.id) != 0 || hovered == n.id;
    const Vec3 c = kColors[index++ % 6];
    const uint32_t fill = pack_rgba(c.x, c.y, c.z, sel ? 0.5f : 0.3f);
    const uint32_t inner = pack_rgba(c.x, c.y, c.z, 0.55f);
    const uint32_t border = sel ? pack_rgba(1.f, 0.6f, 0.15f, 1.f) : pack_rgba(c.x * 0.6f + 0.4f, c.y * 0.6f + 0.4f, c.z * 0.6f + 0.4f, 1.f);
    const Vec3 lift{0, 0.06f, 0};
    for (const auto& p : m.polys) {
      Vec3 v[4];
      for (int k = 0; k < p.count; k++) v[k] = w.point(m.verts[p.v[k]]);
      for (int k = 1; k + 1 < p.count; k++) renderer.tri(v[0], v[k], v[k + 1], fill);
      for (int k = 0; k < p.count; k++) {
        // an inner edge is drawn once, from the polygon with the smaller index
        const int j = p.adj[k];
        if (j >= 0 && &m.polys[j] < &p) continue;
        renderer.line(v[k] + lift, v[(k + 1) % p.count] + lift, j < 0 ? border : inner);
      }
    }
  }
  for (auto e = m_nav_draw_cache.begin(); e != m_nav_draw_cache.end();)  // forget the meshes gone
    e = used.count(e->first) ? std::next(e) : m_nav_draw_cache.erase(e);
  renderer.flush(true, true);
}

void App::draw_tool_overlays() {
  if (tool == ToolMode::Topology) {
    draw_topology_overlay();
    return;
  }
  if (tool == ToolMode::Terrain) {
    draw_terrain_overlay();
    return;
  }
  if (tool == ToolMode::Select) {
    draw_move_preview();
    return;
  }
  if (tool != ToolMode::Place || !m_place_valid) return;
  // what is placed, see-through, where it would go (its mesh, merged into the document once)
  std::shared_ptr<Mesh> preview = placement_mesh();
  const Vec3 c = m_place_matrix.col3(3);
  if (preview) {
    renderer.draw_mesh(*preview, m_place_matrix, scene.materials, scene.textures, rs, {0.35f, 0.75f, 1.f, 0.25f}, 0.7f);
  } else {
    // a cross while the prefab loads
    const float r = 1.f;
    renderer.line(c - Vec3{r, 0, 0}, c + Vec3{r, 0, 0}, 0xff808080);
    renderer.line(c - Vec3{0, 0, r}, c + Vec3{0, 0, r}, 0xff808080);
    renderer.line(c, c + Vec3{0, r * 1.5f, 0}, 0xff808080);
  }
  renderer.circle(c + Vec3{0, 0.03f, 0}, {1, 0, 0}, {0, 0, 1}, 0.5f, 0xff40d0ff, 24);
  if (m_place_drag) {
    // where it was clicked, and the way to where it goes
    const Vec3 a = m_place_anchor.col3(3);
    renderer.circle(a + Vec3{0, 0.03f, 0}, {1, 0, 0}, {0, 0, 1}, 0.3f, 0xff808080, 16);
    renderer.line(a, {c.x, a.y, c.z}, 0xff4040ff);
    renderer.line({c.x, a.y, c.z}, c, 0xff40ff40);
  }
  renderer.flush(true);
}

void App::handle_shortcuts() {
  ImGuiIO& io = ImGui::GetIO();
  if (io.WantTextInput || m_looking || m_free_rotating) return;
  const bool ctrl = io.KeyCtrl, shift = io.KeyShift;
  auto pressed = [](ImGuiKey k) { return ImGui::IsKeyPressed(k, false); };
  if (ctrl && pressed(ImGuiKey_Z)) {
    if (shift) history.redo();
    else history.undo();
  } else if (ctrl && pressed(ImGuiKey_Y)) {
    history.redo();
  } else if (ctrl && pressed(ImGuiKey_S)) {
    save(shift);
  } else if (ctrl && pressed(ImGuiKey_O)) {
    confirm_discard([this]() { open_dialog(DialogAction::OpenProject); });
  } else if (ctrl && pressed(ImGuiKey_N)) {
    confirm_discard([this]() { new_level(); });
  } else if (ctrl && pressed(ImGuiKey_C)) {
    copy_selection();
  } else if (ctrl && pressed(ImGuiKey_V)) {
    paste();
  } else if (ctrl && pressed(ImGuiKey_D)) {
    duplicate_selection();
  } else if (ctrl && pressed(ImGuiKey_L)) {
    if (!selection.empty()) select_level(level_of_node(primary));
  } else if (ctrl && shift && pressed(ImGuiKey_G)) {
    if (!selection.empty()) {
      m_group_name = strf(tr("group.default_name"), document_groups().size() + 1);
      m_show_new_group = true;
    }
  } else if (ctrl && pressed(ImGuiKey_G)) {
    if (primary) select_assembly(primary);
  } else if (pressed(ImGuiKey_Escape)) {
    if (tool == ToolMode::Place) stop_placement();
    else if (tool == ToolMode::Topology) stop_topology();
    else if (tool == ToolMode::Terrain) stop_terrain_brush();
    else select_only(0);
  } else if (!ctrl && pressed(ImGuiKey_B)) {
    if (tool == ToolMode::Terrain) stop_terrain_brush();
    else start_terrain_brush();
  } else if (tool == ToolMode::Terrain && (pressed(ImGuiKey_LeftBracket) || pressed(ImGuiKey_RightBracket))) {
    brush.size = std::clamp(brush.size * (pressed(ImGuiKey_RightBracket) ? 1.25f : 0.8f), 0.5f, 64.f);
  } else if (!ctrl && pressed(ImGuiKey_T)) {
    if (tool == ToolMode::Topology) stop_topology();
    else start_topology();
  } else if (tool == ToolMode::Place && !ctrl && pressed(ImGuiKey_R)) {
    placement.yaw += radians(shift ? -45.f : 45.f);
  } else if (pressed(ImGuiKey_Delete)) {
    if (tool == ToolMode::Topology) topology_delete();
    else delete_selection();
  } else if (!ctrl && pressed(ImGuiKey_F)) {
    frame_selection();
  } else if (pressed(ImGuiKey_Home)) {
    frame_all();
  } else if (!ctrl && pressed(ImGuiKey_W)) {
    gizmo.op = GizmoOp::Translate;
  } else if (!ctrl && pressed(ImGuiKey_E)) {
    gizmo.op = GizmoOp::Rotate;
  } else if (!ctrl && pressed(ImGuiKey_R)) {
    gizmo.op = GizmoOp::Scale;
  } else if (pressed(ImGuiKey_F1)) {
    show_help = !show_help;
    guide_tab_request = 0;
  } else if (!ctrl && pressed(ImGuiKey_G) && game_view.ready()) {
    game_view_on = !game_view_on;
  }
}

// ------------------------------------------------------------------------------------------------
// captures (command line)
// ------------------------------------------------------------------------------------------------

// Setups (what the capture shows):
//   levels  : the Levels panel, the level `in` (a level name of the library) loaded
//   select  : the level `in`, one of its decor instances selected, the gizmo on it
//   scale   : a catwalk of the level `in` stretched along its own X axis, the gizmo scaling it;
//             for a project `in` (.glb), its first stretched element
//   project : the project `in` (a .glb saved by the editor), the objects of the Catalog panel
// select, scale and prefab also take a project; --select a,b,... chooses the elements they select
//   prefab  : the level `in`, a prefab made of a few instances (in a temporary folder) and placed
//             next to them, the Prefabs panel open
//   extract : the extraction window on a disc (`in`, an .iso or a folder), extracting it to a
//             temporary folder deleted after the capture
//   help    : the Getting started window
//   catalog : the level `in` loaded, the objects of the Catalog panel
//   models  : the level `in` loaded, the models of the Catalog panel
//   decor   : the level `in` loaded, its decor in the Catalog panel
//   decor-all : the level `in` loaded, the decor of every level searched for "catwalk"
//   materials : the level `in` loaded, the materials of the Catalog panel
//   textures : as select, the window of the inspector's Game textures... open for its first material
//   effects : the level `in` loaded, the particle effects of the Catalog panel
//   actors  : the level `in` loaded with its actors (its effects running)
//   navmesh : the level or project `in`, its nav meshes drawn (generated for the whole level when
//             it has none), the largest selected, the Nav meshes window open
int App::ui_screenshot(const AppOptions& opts, const std::string& in, const std::string& out, int w,
                       int h, const std::string& setup) {
  Prefs::get().load();
  if (!init_window(true, w, h)) return 1;
  int result = 1;
  const fs::path tmp = fs::temp_directory_path() / "ogle-capture";
  {
    std::error_code ec;
    fs::remove_all(tmp, ec);
    apply_options(opts);
    new_document();
    // the editor's own view, or the game rendering at an hour (--game)
    game_view_on = opts.game_hour >= 0 && game_view.ready();
    if (game_view_on) game_view.mood.hour = opts.game_hour;
    m_prefab_dir_override = (tmp / "prefabs").generic_string();
    refresh_prefabs();
    const bool project_in = in.size() > 4 && (in.compare(in.size() - 4, 4, ".glb") == 0 ||
                                              in.compare(in.size() - 4, 4, ".GLB") == 0);
    const bool with_project =
        setup == "project" ||
        (project_in && (setup == "select" || setup == "scale" || setup == "prefab" || setup == "navmesh"));
    const bool with_level = !with_project && (setup == "levels" || setup == "select" || setup == "scale" ||
                                              setup == "prefab" || setup == "catalog" || setup == "models" ||
                                              setup == "effects" || setup == "actors" ||
                                              setup == "decor" || setup == "decor-all" || setup == "navmesh" ||
                                              setup == "export" || setup == "guide" || setup == "tools" ||
                                              setup == "build" || setup == "materials" || setup == "textures");
    if (with_project) open_project_file(in);  // the preferences are not written in this mode
    if (setup == "project") left_tab_request = 1;
    if (with_level) {
      load_actors = setup == "actors";
      if (const LevelEntry* level = library.level(in)) {
        m_selected_level = level->name;
        load_level(*level);
      } else {
        LOG_ERROR("capture: no level %s in the library", in.c_str());
      }
    }
    if (setup == "extract") {
      open_extract_window();
      extract.output = (tmp / "library").generic_string();
      extract.source = in;
      extract_detect(in);
    }
    if (setup == "help" || setup == "guide" || setup == "tools") {
      show_help = true;
      guide_tab_request = setup == "help" ? 2 : setup == "tools" ? 1 : 0;
      guide_step = 4;  // the gizmo: four animations
      // --select <tutorial>[:<step>]: that tutorial instead
      if (!opts.select.empty()) {
        const size_t colon = opts.select.find(':');
        select_tutorial(opts.select.substr(0, colon), colon == std::string::npos ? 0 : std::atoi(opts.select.c_str() + colon + 1));
      }
    }
    if (setup == "build") left_tab_request = 3;
    if (setup == "catalog" || setup == "models" || setup == "decor" || setup == "decor-all" || setup == "effects" ||
        setup == "materials") {
      left_tab_request = 1;
      if (setup == "effects") m_catalog_filter = opts.select;  // --select: the effects searched for
      m_catalog_tab = setup == "decor" || setup == "decor-all" ? 2
                      : setup == "models"                      ? 1
                      : setup == "materials"                   ? 4
                      : setup == "effects"                     ? 5
                                                               : 0;
      if (setup == "decor") m_decor_level = in;
      if (setup == "decor-all") m_catalog_filter = "catwalk";
    }
    OffscreenTarget target;
    int fbw, fbh;
    SDL_GetWindowSizeInPixels(m_window, &fbw, &fbh);
    if (target.create(fbw, fbh)) {
      bool prepared = false, extraction_started = false;
      int settled = 0;
      std::vector<std::string> to_add;
      for (size_t a = 0, b; a < opts.add.size(); a = b + 1) {
        b = opts.add.find(',', a);
        if (b == std::string::npos) b = opts.add.size();
        if (b > a) to_add.push_back(opts.add.substr(a, b - a));
      }
      auto start = std::chrono::steady_clock::now();
      for (int frame_index = 0;; frame_index++) {
        SDL_Delay(2);  // frames are not paced by vsync here: let the loading threads work
        SDL_Event e;
        while (SDL_PollEvent(&e)) process_event(&e);
        glBindFramebuffer(GL_FRAMEBUFFER, target.fbo);
        frame(1.f / 60.f);
        // loading, or the catalog not complete yet (the index of the library)
        bool loading = m_level_future.valid() || store.pending() > 0 || thumbs.pending() > 0 || extract.process ||
                       !m_index_applied || navmesh_busy() || palette.busy() ||
                       level_adding() || !to_add.empty();
        // the levels added to it (--add), one after the other once it is loaded
        if (!to_add.empty() && !m_level_future.valid() && !level_adding() && frame_index > 1) {
          std::string err;
          const LevelEntry* next = library.level(to_add.front());
          if (!next) LOG_ERROR("capture: no level %s in the library", to_add.front().c_str());
          else if (!add_level_to_document(*next, &err)) LOG_ERROR("%s", err.c_str());
          to_add.erase(to_add.begin());
        }
        if (setup == "extract" && !extraction_started && !extract.process && extract.detected_source == in &&
            extract.detect_error.empty()) {
          extraction_started = true;
          start_extraction();
          loading = true;
        }
        if (!prepared && !m_level_future.valid() && !level_adding() && to_add.empty() && frame_index > 1) {
          prepared = true;
          // an instance of a decor prototype with copies, near the middle of the level (a catwalk
          // to stretch for `scale`): selected, then a prefab of it and its neighbours
          const Node* target_node = nullptr;
          const Vec3 middle = scene.focus_bounds().center();
          float best = 1e30f;
          // the elements asked for (--select), else the first stretched one of a project
          std::vector<uint32_t> named;
          if (!opts.select.empty()) {
            const std::string list = "," + opts.select + ",";
            for (const auto& n : scene.nodes)
              if (list.find("," + n.name + ",") != std::string::npos) named.push_back(n.id);
            if (!named.empty()) target_node = scene.find(named.front());
          }
          if (!target_node && with_project && setup == "scale")
            for (const auto& n : scene.nodes)
              if (!target_node && n.kind == NodeKind::Render && n.mesh &&
                  (n.local.s.x != 1.f || n.local.s.y != 1.f || n.local.s.z != 1.f))
                target_node = &n;
          for (const auto& n : scene.nodes) {
            if (with_project || target_node) break;
            const Node* p = scene.find(n.parent);
            if (n.kind == NodeKind::Render && n.mesh && !n.locked && p && p->kind == NodeKind::Group &&
                scene.children(p->id).size() > 3 && n.mesh->tri_count() > 40) {
              float d = length(scene.world_bounds(n).center() - middle);
              if (setup == "scale" && n.name.find("catwalk") == std::string::npos) d += 1e6f;
              if (d < best) {
                best = d;
                target_node = &n;
              }
            }
          }
          if (setup == "select" && target_node && target_node->kind == NodeKind::Group &&
              target_node->extras.contains("og_level")) {
            // a level added, named: selected whole, as Select its level does
            const uint32_t group = target_node->id;
            select_level(group);
            camera.yaw = 0.7f;
            camera.pitch = -0.5f;
            camera.frame(document_level_bounds(group));
          } else if (setup == "select" || setup == "scale" || setup == "prefab" || setup == "textures") {
            if (target_node) {
              select_only(target_node->id);
              for (auto id : named) selection.insert(id);
              if (setup == "textures") m_texture_window = {true, {target_node->id}, 0};
              if (setup == "scale") {
                gizmo.op = GizmoOp::Scale;
                if (!with_project && named.empty()) scene.find(target_node->id)->local.s = {2.5f, 1.f, 1.f};
              }
              AABB b = scene.world_bounds(*target_node);
              camera.yaw = 0.7f;
              camera.pitch = -0.4f;
              camera.frame(b.expanded(with_project ? 10.f : 6.f));
            }
            if (setup == "prefab" && target_node) {
              // the selected instance and the instances next to it (or the elements asked for)
              AABB near = scene.world_bounds(*target_node).expanded(4.f);
              for (const auto& n : scene.nodes)
                if (named.empty() && n.kind == NodeKind::Render && n.mesh && !n.locked && !scene.actor_of(n) &&
                    near.contains(scene.world_bounds(n).center()) && selection.size() < 6)
                  selection.insert(n.id);
              create_prefab("My prefab");
              left_tab_request = 2;
              AABB b;
              for (auto id : selection) b.add(scene.world_bounds(*scene.find(id)));
              if (!prefabs.empty()) {
                // a copy beside them, on the ground they stand on, where nothing hides it: toward
                // the camera first, then on either side, then behind
                const float step = std::max(b.size().x, b.size().z) + 3.f;
                Vec3 toward = -camera.forward();
                toward.y = 0;
                toward = length2(toward) > 1e-6f ? normalize(toward) : Vec3{0, 0, 1};
                const Vec3 side = camera.right();
                PickFilter f;
                f.actors = false;
                f.include_locked = true;
                Vec3 p = b.center() + side * step;
                p.y = b.lo.y;
                for (const Vec3& dir : {toward, -side, side, -toward}) {
                  const Vec3 q = b.center() + dir * step;
                  PickHit ground = pick(scene, Ray{Vec3{q.x, b.hi.y + 30.f, q.z}, {0, -1, 0}}, f);
                  if (ground.hit() && std::fabs(ground.point.y - b.lo.y) < 2.f) {
                    p = ground.point;
                    break;
                  }
                }
                start_placement(prefabs.front().path, prefabs.front().name, false);
                place_at(Mat4::translate(p));
                stop_placement();
                for (auto id : selection) b.add(scene.world_bounds(*scene.find(id)));
              }
              camera.pitch = -0.6f;
              camera.frame(b.expanded(3.f));
            }
          } else if (setup != "extract") {
            frame_all();
          }
          if (setup == "export") {
            // exported in the capture's folder: the window and its report
            open_export_window();
            export_options.folder = (tmp / "exports").generic_string();
            LevelExportReport rep;
            std::string err;
            if (export_for_game(export_options, &rep, &err)) m_export_report = std::move(rep);
            else LOG_ERROR(tr("log.export_failed"), err.c_str());
          }
          if (setup == "navmesh") {
            // the document's nav meshes, or new ones (selected when they are applied)
            show_navmesh = true;
            show_navmesh_window = true;
            const auto navs = navmesh_nodes();
            std::string err;
            if (!navs.empty()) select_only(navs.front());
            else if (!start_navmesh_generation(AABB{}, &err)) LOG_WARN("%s", err.c_str());
          }
          if (!opts.story.empty()) {
            StoryView v = story;
            v.enabled = opts.story != "off";
            if (const GameData* data = document_game_data(); data && v.enabled) {
              v.step = std::atoi(opts.story.c_str());
              for (int i = 0; i < data->step_count(); i++)
                if (data->node(i).name == opts.story) v.step = i + 1;
            }
            set_story_view(v);
          }
        }
        if (opts.camera_set && prepared) {
          camera.pos = opts.camera_pos;
          camera.yaw = radians(opts.camera_yaw);
          camera.pitch = radians(opts.camera_pitch);
        }
        if (prepared && !loading) settled++;
        else settled = 0;
        double elapsed = std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
        if (settled > 8 || elapsed > 180.0) break;
      }
      glBindFramebuffer(GL_FRAMEBUFFER, target.fbo);
      frame(1.f / 60.f);
      glFinish();
      result = target.save_png(out) ? 0 : 1;
      LOG_INFO("interface captured: %s (%zu nodes, %zu draws, %zu triangles)", out.c_str(), scene.nodes.size(),
               renderer.stats.draws, renderer.stats.triangles);
    }
    glBindFramebuffer(GL_FRAMEBUFFER, 0);
  }
  shutdown_window();  // the loading threads let go of the files
  std::error_code ec;
  fs::remove_all(tmp, ec);
  return result;
}

}  // namespace ogle
