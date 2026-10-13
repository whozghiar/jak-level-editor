// The terrain brush (B, its own small window): a square target over the view, its size changed with
// Ctrl + wheel or [ and ], what it would do shown see-through under it before it is done; the left
// button held paints ground (with the relief and the textures of the level style chosen, the open
// level's at first), raises walls from it or erases ground and walls (shown see-through until it is
// let go, Esc: nothing done), smooths or flattens it;
// the right button held and the mouse moved up raises the ground there, down digs it (shown until
// it is released). Its strokes go into the terrain under the square (else one of its own,
// scene/terrain.h), each one undone with Ctrl+Z.

#include <algorithm>
#include <cctype>
#include <cfloat>
#include <cmath>

#include <SDL3/SDL.h>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "imgui.h"
#include "imgui_stdlib.h"
#include "scene/pick.h"
#include "scene/relight.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::BeginItemTooltip()) {
    ImGui::PushTextWrapPos(ImGui::GetFontSize() * 28.f);
    ImGui::TextUnformatted(text);
    ImGui::PopTextWrapPos();
    ImGui::EndTooltip();
  }
}

// the window's modes (raising and digging: the right button; the MCP tool has them all)
const BrushMode kModes[] = {BrushMode::Paint, BrushMode::Wall, BrushMode::Erase, BrushMode::Smooth, BrushMode::Flatten};
const char* const kModeKeys[] = {"terrain.mode.paint", "terrain.mode.wall", "terrain.mode.erase", "terrain.mode.smooth",
                                 "terrain.mode.flatten"};

void disabled_wrapped(const std::string& text) {
  ImGui::PushStyleColor(ImGuiCol_Text, ImGui::GetStyleColorVec4(ImGuiCol_TextDisabled));
  ImGui::TextWrapped("%s", text.c_str());
  ImGui::PopStyleColor();
}

// "Terrain", else "Terrain-2"... (a name no element has)
std::string terrain_name(const Scene& scene) {
  std::string name = "Terrain";
  for (int i = 2;; i++) {
    bool taken = false;
    for (const auto& n : scene.nodes) taken |= n.name == name;
    if (!taken) return name;
    name = "Terrain-" + std::to_string(i);
  }
}

// Shift: the other way (paint or walls and erase)
BrushMode inverted(BrushMode m) {
  switch (m) {
    case BrushMode::Paint:
    case BrushMode::Wall: return BrushMode::Erase;
    case BrushMode::Erase: return BrushMode::Paint;
    default: return m;
  }
}

}  // namespace

const LevelStyle* App::level_style(const std::string& level_name) {
  auto it = m_styles.find(level_name);
  if (it != m_styles.end()) return &it->second;
  const LevelEntry* level = library.level(level_name);
  auto decor = level ? store.level(level->background, true) : nullptr;
  if (!decor) return nullptr;  // read in the background (a few seconds), or it cannot be
  return &m_styles.emplace(level_name, measure_style(*decor)).first->second;
}

int App::style_material(const std::string& level_name, bool floor) {
  const LevelEntry* level = library.level(level_name);
  auto it = m_styles.find(level_name);
  if (!level || it == m_styles.end()) return -1;
  auto decor = store.level(level->background, true);
  const LevelStyle::Surface& surface = floor ? it->second.floor : it->second.wall;
  if (!decor || surface.material < 0) return -1;
  // its material in the document (its texture once)
  Mesh one;
  one.prims.emplace_back();
  one.prims[0].material = surface.material;
  return merge_mesh(scene, one, decor->textures, decor->materials)->prims[0].material;
}

const std::vector<PaletteTexture>& App::level_textures(const std::string& level_name) {
  if (m_level_textures_of == level_name && !m_level_textures.empty()) return m_level_textures;
  m_level_textures.clear();
  m_level_textures_of = level_name;
  if (!library_index.ready()) {
    m_level_textures_of.clear();  // asked again once the index is there
    return m_level_textures;
  }
  for (const auto& li : library_index.levels())
    if (li.level == level_name)
      for (const auto& [name, n] : li.textures) {
        if (name.rfind("anim-slot", 0) == 0) continue;
        PaletteTexture t;
        t.name = name;
        t.background = li.background;
        t.levels = 1;
        t.uses = n;
        m_level_textures.push_back(std::move(t));
      }
  std::sort(m_level_textures.begin(), m_level_textures.end(),
            [](const PaletteTexture& a, const PaletteTexture& b) { return a.uses != b.uses ? a.uses > b.uses : a.name < b.name; });
  return m_level_textures;
}

int App::material_with_texture(const std::string& level_name, const std::string& texture, int like) {
  for (const auto& t : level_textures(level_name)) {
    if (t.name != texture) continue;
    const int ti = palette_texture(t, true);
    if (ti < 0) return -1;
    // the style's material (its blending, its sides) with that texture instead
    Material m = like >= 0 && like < (int)scene.materials.size() ? scene.materials[like]
                                                                  : scene.materials[shape_default_material(scene)];
    m.texture = ti;
    m.name = texture;
    for (int i = 0; i < (int)scene.materials.size(); i++) {
      const Material& d = scene.materials[i];
      if (d.name == m.name && d.texture == m.texture && d.alpha == m.alpha && d.blend == m.blend &&
          d.double_sided == m.double_sided && d.translucent == m.translucent)
        return i;
    }
    return scene.add_material(std::move(m));
  }
  return -1;
}

TerrainGrid::Look App::terrain_look() {
  // the level style chosen, else the editor's grid; the textures chosen among the level's instead
  // of the style's
  TerrainGrid::Look g;
  const LevelStyle* style = m_style_level.empty() ? nullptr : level_style(m_style_level);
  if (style) {
    g.floor_material = style_material(m_style_level, true);
    g.wall_material = style_material(m_style_level, false);
    g.floor_tile = style->floor.tile;
    g.wall_tile = style->wall.material >= 0 ? style->wall.tile : style->floor.tile;
    g.color = style->floor.material >= 0 ? style->floor.color : style->wall.color;
  }
  if (!m_style_level.empty()) {
    if (!brush.floor_texture.empty()) {
      const int m = material_with_texture(m_style_level, brush.floor_texture, g.floor_material);
      if (m >= 0) g.floor_material = m;
    }
    if (!brush.wall_texture.empty()) {
      const int m = material_with_texture(m_style_level, brush.wall_texture,
                                          g.wall_material >= 0 ? g.wall_material : g.floor_material);
      if (m >= 0) g.wall_material = m;
    }
  }
  if (g.floor_material < 0 && g.wall_material < 0) {
    g.floor_material = g.wall_material = shape_default_material(scene);
    g.floor_tile = g.wall_tile = 2.f;
    g.color = {0, 0, 0, 0};
  }
  return g;
}

bool App::start_terrain_brush() {
  if (!document_open()) return false;
  stop_placement();
  stop_preview();
  if (tool == ToolMode::Topology) stop_topology();
  // its look: the style chosen before, else the open level's
  if (m_style_level.empty() && library.level(scene.settings.level)) m_style_level = scene.settings.level;
  // the terrain selected is the one painted, else a new one at the first stroke
  TerrainGrid g;
  const Node* n = scene.find(primary);
  m_terrain_node = n && node_terrain(*n, &g) ? n->id : 0;
  select_only(0);  // its highlight would hide the ground under the square
  tool = ToolMode::Terrain;
  return true;
}

void App::stop_terrain_brush() {
  if (m_terrain_change) m_stroke_preview ? terrain_cancel_stroke() : terrain_end_stroke();
  if (m_terrain_sculpting) SDL_SetWindowRelativeMouseMode(m_window, false);  // the ground left as it was
  m_terrain_rmb = m_terrain_sculpting = false;
  if (tool == ToolMode::Terrain) tool = ToolMode::Select;
}

// the stroke going on: written into the element, one undo step
void App::terrain_end_stroke() {
  Node* n = scene.find(m_terrain_node);
  if (n && m_terrain_grid.empty() && m_terrain_new) {
    // nothing drawn on a new terrain: no element
    scene.remove({n->id});
    m_terrain_node = 0;
    m_terrain_change.reset();
    return;
  }
  if (n) n->extras["og_terrain_grid"] = m_terrain_grid.to_json();
  // what was shown, made
  if (n && m_stroke_preview && m_terrain_changed) {
    n->mesh->prims = std::move(make_terrain_mesh(m_terrain_grid)->prims);
    n->mesh->touch();
  }
  m_stroke_preview = false;
  // lit like the level around it, as what is placed (its palettes made again for its new shape)
  if (n && m_terrain_changed) light_like_surroundings(scene, {n->id});
  if (m_terrain_change && m_terrain_changed)
    history.push(m_terrain_change->commit(tr(m_terrain_new ? "undo.terrain.new" : "undo.terrain")));
  m_terrain_change.reset();
}

void App::terrain_cancel_stroke() {
  // the element made for it taken away; the one painted keeps its mesh (the stroke only showed)
  if (Node* n = scene.find(m_terrain_node); n && m_terrain_new) {
    scene.remove({n->id});
    m_terrain_node = 0;
  }
  m_terrain_change.reset();
  m_stroke_preview = false;
}

// a stroke starts where the brush is: the element painted (a new one when there is none), its state
// kept for undo
bool App::terrain_begin_stroke(BrushMode mode) {
  Node* node = scene.find(m_terrain_node);
  m_terrain_change = std::make_unique<NodeChange>(scene);
  m_terrain_new = false;
  m_terrain_changed = false;
  if (!node) {
    if (mode == BrushMode::Erase || mode == BrushMode::Unwall || mode == BrushMode::Smooth || mode == BrushMode::Flatten) {
      m_terrain_change.reset();
      return false;
    }
    Node t;
    t.id = scene.next_id;
    t.name = terrain_name(scene);
    t.kind = NodeKind::Render;
    m_terrain_change->save(t.id);
    TerrainGrid g;
    g.cell = brush.cell;
    g.looks = {terrain_look()};
    t.extras = json{{"og_terrain_grid", g.to_json()}};
    t.mesh = make_terrain_mesh(g);
    m_terrain_node = scene.add(std::move(t));
    m_terrain_new = true;
    node = scene.find(m_terrain_node);
  } else {
    m_terrain_change->save(node->id);
  }
  // its own mesh, changed in place during the stroke (the one before stays with the undo; the
  // scene's cache of this frame keeps pointing at a mesh that is there)
  node->mesh = std::make_shared<Mesh>(*node->mesh);
  m_terrain_grid = TerrainGrid{};
  if (!node_terrain(*node, &m_terrain_grid)) {
    m_terrain_grid.cell = brush.cell;
    m_terrain_grid.looks = {terrain_look()};
  }
  // the cells painted take the style chosen now
  m_stroke_look = m_terrain_grid.add_look(terrain_look());
  m_stroke_preview = mode == BrushMode::Paint || mode == BrushMode::Wall || mode == BrushMode::Erase;
  m_stroke_stuck = m_magnet_valid;
  m_stroke_before = m_stroke_preview ? m_terrain_grid : TerrainGrid{};
  m_stroke_cells[0] = m_stroke_cells[1] = INT_MAX;
  m_stroke_cells[2] = m_stroke_cells[3] = INT_MIN;
  m_stroke_height = terrain_start_height(m_terrain_grid, node);
  m_brush_plane = m_brush_point.y;
  m_last_stamp = scene.world(*node).inverse().point(m_brush_point);
  m_has_stamp = false;
  return true;
}

// the ground's height where a stroke starts: on the terrain itself, its ground there (not the side
// of its edge, half a cell nearer the camera)
float App::terrain_start_height(const TerrainGrid& g, const Node* node) const {
  const Mat4 to_node = node ? scene.world(*node).inverse() : Mat4::identity();
  const Vec3 at = to_node.point(m_brush_point);
  if (!node || m_brush_hit_node != node->id || m_magnet_valid) return at.y;
  Vec3 back = to_node.dir(camera.pos - m_brush_point);
  back.y = 0;
  const Vec3 probe = length(back) > 1e-4f ? at + normalize(back) * (g.cell * 0.5f) : at;
  float y;
  if (terrain_height(g, at.x, at.z, &y) || terrain_height(g, probe.x, probe.z, &y)) return y;
  return at.y;
}

BrushStroke App::terrain_stamp(BrushMode mode, Vec3 center, float amount, float height) {
  BrushStroke st;
  st.mode = mode;
  st.center = center;
  st.size = brush.size;
  st.height = height;
  st.seed = brush.seed;
  st.look = m_stroke_look;
  st.amount = amount;
  st.lay = brush.mode != BrushMode::Erase;  // the right button erasing: it raises or digs what is there only
  // new ground with the relief of the style (raise and dig lay some where there is none); flat where
  // it sticks to the decor (it meets it with no step)
  const LevelStyle* style = m_style_level.empty() ? nullptr : level_style(m_style_level);
  if (brush.relief && style && mode != BrushMode::Wall && !m_magnet_valid && !m_stroke_stuck) {
    st.bumps = style->bumps;
    st.bump_size = std::max(style->bump_size, 0.5f);
  }
  return st;
}

// the brush applied from `from` to `to` (the terrain's space), stamps a quarter of its size apart;
// `amount`: raise, dig, smooth, flatten: shared by the stamps; wall: its height
void App::terrain_apply(BrushMode mode, Vec3 from, Vec3 to, float amount) {
  Node* node = scene.find(m_terrain_node);
  if (!node) return;
  const float step = std::max(brush.size * 0.25f, m_terrain_grid.cell * 0.5f);
  const float dist = length(Vec3{to.x - from.x, 0, to.z - from.z});
  const int n_stamps = std::clamp((int)std::ceil(dist / step), 1, 64);
  bool changed = false;
  const float cl = m_terrain_grid.cell, half = std::max(brush.size * 0.5f, cl * 0.5f);
  for (int s = 1; s <= n_stamps; s++) {
    const Vec3 at = from + (to - from) * ((float)s / n_stamps);
    changed |= apply_brush(m_terrain_grid, terrain_stamp(mode, at, mode == BrushMode::Wall ? amount : amount / n_stamps, m_stroke_height));
    m_stroke_cells[0] = std::min(m_stroke_cells[0], (int)std::floor((at.x - half) / cl) - 1);
    m_stroke_cells[1] = std::min(m_stroke_cells[1], (int)std::floor((at.z - half) / cl) - 1);
    m_stroke_cells[2] = std::max(m_stroke_cells[2], (int)std::floor((at.x + half) / cl) + 1);
    m_stroke_cells[3] = std::max(m_stroke_cells[3], (int)std::floor((at.z + half) / cl) + 1);
  }
  if (changed) {
    m_terrain_changed = true;
    if (!m_stroke_preview) {
      node->mesh->prims = std::move(make_terrain_mesh(m_terrain_grid)->prims);
      node->mesh->touch();
    }
  }
}

void App::terrain_tool(bool clicked, bool released) {
  ImGuiIO& io = ImGui::GetIO();
  hovered = 0;
  if (!document_open()) {
    stop_terrain_brush();
    return;
  }
  // undone, redone, deleted: the element painted may be gone
  if (!scene.find(m_terrain_node) && !m_terrain_change) m_terrain_node = 0;
  // it stays where it is while the wheel raises or digs there
  if (m_terrain_sculpting) return;
  // where the brush is: on what the ray meets (the terrain itself, the level), else on the plane
  // of the last stroke
  m_brush_valid = false;
  if (m_mouse_in_vp) {
    const Ray ray = camera.ray(m_mouse_vp.x, m_mouse_vp.y, m_vp_w, m_vp_h);
    PickFilter pf;
    pf.actors = false;
    pf.include_locked = true;
    const PickHit hit = pick(scene, ray, pf);
    m_brush_hit_node = hit.node;
    if (hit.hit()) {
      m_brush_point = hit.point;
      m_brush_valid = true;
      // the magnet brush keeps the height of what it was over: off a quay, over the water, the square
      // stays at its level (and sticks to its edge)
      if (brush.magnet && !m_terrain_change) m_brush_plane = hit.point.y;
    } else if (std::fabs(ray.d.y) > 1e-4f) {
      const float t = (m_brush_plane - ray.o.y) / ray.d.y;
      if (t > 0) {
        m_brush_point = ray.o + ray.d * t;
        m_brush_valid = true;
      }
    }
  }
  // the terrain under the square is the one it changes
  if (!m_terrain_change && m_brush_hit_node && m_brush_hit_node != m_terrain_node)
    if (const Node* h = scene.find(m_brush_hit_node); h && h->extras.contains("og_terrain_grid")) m_terrain_node = h->id;
  const BrushMode mode = io.KeyShift ? inverted(brush.mode) : brush.mode;
  // the magnet brush (ground, walls, flattening): the square goes where the cursor is, the ground it
  // lays at the level of the ground already laid beside it: the terrain painted's nearest, else the
  // top of the decor beside it (just under it: no flicker where they meet)
  m_magnet_valid = false;
  if (brush.magnet && m_brush_valid && (mode == BrushMode::Paint || mode == BrushMode::Wall || mode == BrushMode::Flatten)) {
    const Node* tn = scene.find(m_terrain_node);
    const TerrainGrid* g = m_terrain_change ? &m_terrain_grid : tn ? &m_terrain_grid_view : nullptr;
    const float cell = g && g->w ? g->cell : brush.cell;
    const float reach = std::max(brush.size * 0.5f, cell * 0.5f) + 2.f;
    if (length(m_brush_point - m_magnet_asked) > 0.02f || brush.size != m_magnet_asked_size || m_terrain_change) {
      m_magnet_asked = m_brush_point;
      m_magnet_asked_size = brush.size;
      m_magnet_node = 0;
      Vec3 level;
      if (tn && g && g->w && terrain_level_near(*g, scene.world(*tn).inverse().point(m_brush_point), reach, &level)) {
        m_magnet_edge = scene.world(*tn).point(level);
        m_magnet_node = tn->id;
        m_magnet_below = 0.f;
      } else if (nearest_decor_edge(scene, m_brush_point, reach, 8.f, m_terrain_node, &m_magnet_edge, &m_magnet_node)) {
        // another terrain beside: the one painted from now on, made larger at its level
        const Node* found = scene.find(m_magnet_node);
        const bool terrain = found && found->extras.contains("og_terrain_grid");
        if (terrain && !m_terrain_change) m_terrain_node = found->id;
        m_magnet_below = terrain ? 0.f : 0.02f;
      } else {
        m_magnet_node = 0;
      }
    }
    if (m_magnet_node) {
      m_magnet_valid = true;
      m_brush_raw = m_brush_point;
      m_brush_point.y = m_magnet_edge.y - m_magnet_below;
      // a stroke going on lays its ground at the height of what it sticks to from there on, flat
      if (const Node* tn = m_terrain_change ? scene.find(m_terrain_node) : nullptr) {
        m_stroke_height = scene.world(*tn).inverse().point(m_brush_point).y;
        m_stroke_stuck = true;
      }
    }
  }
  if (clicked && m_brush_valid && !m_terrain_change && !terrain_begin_stroke(mode)) return;
  // held: the brush applied along the way
  const Node* node = scene.find(m_terrain_node);
  if (m_terrain_change && node && ImGui::IsMouseDown(ImGuiMouseButton_Left) && m_brush_valid) {
    const Vec3 to = scene.world(*node).inverse().point(m_brush_point);
    const float dt = std::min(io.DeltaTime, 0.1f);
    float amount = 0;
    switch (mode) {
      case BrushMode::Raise:
      case BrushMode::Dig: amount = brush.strength * dt; break;
      case BrushMode::Smooth:
      case BrushMode::Flatten: amount = std::min(1.f, brush.strength * 0.5f * dt); break;
      case BrushMode::Wall: amount = brush.wall_height; break;
      default: break;
    }
    terrain_apply(mode, m_has_stamp ? m_last_stamp : to, to, amount);
    m_last_stamp = to;
    m_has_stamp = true;
  }
  if (released && m_terrain_change) terrain_end_stroke();
}

float App::terrain_sculpt_height() const {
  // as far as the mouse went up (or down) on the screen, at the square's distance
  const float per_px = 2.f * length(camera.pos - m_brush_point) * std::tan(radians(camera.fov_deg) * 0.5f) /
                       std::max((float)m_vp_h, 1.f);
  return std::clamp(m_sculpt_px * per_px, -100.f, 100.f);
}

bool App::terrain_right_button(Vec2 rel) {
  if (!m_terrain_rmb && !m_terrain_sculpting) {
    // (not during a stroke of the left button: it looks around)
    if (m_looking || !m_mouse_in_vp || !m_brush_valid || m_terrain_change || !ImGui::IsMouseClicked(ImGuiMouseButton_Right))
      return false;
    m_terrain_rmb = true;
    m_terrain_rmb_drag = {0, 0};
  }
  if (!ImGui::IsMouseDown(ImGuiMouseButton_Right)) {
    // released: the ground raised or dug as shown
    m_terrain_rmb = false;
    if (m_terrain_sculpting) {
      m_terrain_sculpting = false;
      SDL_SetWindowRelativeMouseMode(m_window, false);
      const float h = terrain_sculpt_height();
      const BrushMode mode = h > 0 ? BrushMode::Raise : BrushMode::Dig;
      if (std::fabs(h) >= 0.01f && terrain_begin_stroke(mode)) {
        const Vec3 at = scene.world(*scene.find(m_terrain_node)).inverse().point(m_brush_point);
        terrain_apply(mode, at, at, std::fabs(h));
        terrain_end_stroke();
      }
    }
    return true;
  }
  if (m_terrain_sculpting) {
    m_sculpt_px -= rel.y;
    return true;
  }
  // not moved far yet: up or down raises or digs, sideways (or a key to fly) looks around
  m_terrain_rmb_drag = m_terrain_rmb_drag + rel;
  const bool* keys = SDL_GetKeyboardState(nullptr);
  bool flying = false;
  for (SDL_Scancode k : {SDL_SCANCODE_W, SDL_SCANCODE_A, SDL_SCANCODE_S, SDL_SCANCODE_D, SDL_SCANCODE_Q, SDL_SCANCODE_E,
                         SDL_SCANCODE_C, SDL_SCANCODE_SPACE})
    flying |= keys[k];
  const float ax = std::fabs(m_terrain_rmb_drag.x), ay = std::fabs(m_terrain_rmb_drag.y);
  if (!flying && ax + ay <= 6.f) return true;
  m_terrain_rmb = false;
  SDL_SetWindowRelativeMouseMode(m_window, true);
  if (!flying && ay > 2.f * ax) {
    m_terrain_sculpting = true;
    m_sculpt_px = -m_terrain_rmb_drag.y;
    return true;
  }
  m_looking = true;
  return false;
}

void App::draw_terrain_overlay() {
  if (tool != ToolMode::Terrain || !m_brush_valid) return;
  // the square, on the ground under it (the terrain's where it has some)
  const Node* node = scene.find(m_terrain_node);
  const Mat4 world = node ? scene.world(*node) : Mat4::identity();
  const Mat4 inv = world.inverse();
  const Vec3 c = inv.point(m_brush_point);
  const float half = brush.size * 0.5f;
  const BrushMode mode = ImGui::GetIO().KeyShift ? inverted(brush.mode) : brush.mode;
  // the right button held: green, the mouse up or down raises or digs there
  const uint32_t color = m_terrain_rmb || m_terrain_sculpting ? IM_COL32(120, 240, 120, 255)
                         : mode == BrushMode::Erase ? IM_COL32(255, 115, 75, 255)
                         : mode == BrushMode::Wall  ? IM_COL32(240, 205, 90, 255)
                                                    : IM_COL32(90, 230, 255, 255);
  // the grid as the element has it (during a stroke, the one being drawn)
  if (node && !m_terrain_change && m_terrain_view_mesh != node->mesh.get()) {
    node_terrain(*node, &m_terrain_grid_view);
    m_terrain_view_mesh = node->mesh.get();
  }
  const TerrainGrid& grid = m_terrain_change ? m_terrain_grid : m_terrain_grid_view;
  auto ground = [&](float x, float z) {
    float y = c.y;
    if (node) terrain_height(grid, x, z, &y);
    return world.point({x, y + 0.08f, z});
  };
  const Vec3 corners[4] = {{c.x - half, 0, c.z - half}, {c.x + half, 0, c.z - half}, {c.x + half, 0, c.z + half},
                           {c.x - half, 0, c.z + half}};
  const int steps = std::clamp((int)(brush.size / 0.5f), 4, 64);
  for (int e = 0; e < 4; e++) {
    const Vec3 a = corners[e], b = corners[(e + 1) % 4];
    Vec3 prev = ground(a.x, a.z);
    for (int s = 1; s <= steps; s++) {
      const float t = (float)s / steps;
      const Vec3 p = ground(a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t);
      renderer.line(prev, p, color);
      prev = p;
    }
  }
  // filled, lightly, over the ground it covers
  const uint32_t fill = (color & 0x00ffffffu) | (50u << 24);
  const int n = std::clamp(steps / 2, 2, 32);
  for (int i = 0; i < n; i++)
    for (int k = 0; k < n; k++) {
      const float x0 = c.x - half + brush.size * i / n, x1 = c.x - half + brush.size * (i + 1) / n;
      const float z0 = c.z - half + brush.size * k / n, z1 = c.z - half + brush.size * (k + 1) / n;
      const Vec3 a = ground(x0, z0), b = ground(x1, z0), d = ground(x0, z1), e = ground(x1, z1);
      renderer.tri(a, b, e, fill);
      renderer.tri(a, e, d, fill);
    }
  // its middle
  renderer.line(ground(c.x - 0.3f, c.z), ground(c.x + 0.3f, c.z), color);
  renderer.line(ground(c.x, c.z - 0.3f), ground(c.x, c.z + 0.3f), color);
  renderer.flush(true, true);
  // the magnet: from the cursor to the edge it sticks to, the piece's name next to it
  if (m_magnet_valid) {
    const uint32_t white = IM_COL32(255, 255, 255, 230);
    const Vec3 e = m_magnet_edge + Vec3{0, 0.05f, 0};
    renderer.line(m_brush_raw, e, white);
    renderer.line(e - Vec3{0.4f, 0, 0}, e + Vec3{0.4f, 0, 0}, white);
    renderer.line(e - Vec3{0, 0, 0.4f}, e + Vec3{0, 0, 0.4f}, white);
    renderer.line(e, e + Vec3{0, 0.8f, 0}, white);
    renderer.flush(false);
    Vec2 s;
    const Node* on = scene.find(m_magnet_node);
    if (on && renderer.project(e, &s)) {
      ImDrawList* dl = ImGui::GetForegroundDrawList();
      const std::string text = strf(tr("terrain.magnet.on"), on->name.c_str());
      const ImVec2 at(m_vp_x + s.x + 12, m_vp_y + s.y + 6), size = ImGui::CalcTextSize(text.c_str());
      dl->AddRectFilled(ImVec2(at.x - 5, at.y - 3), ImVec2(at.x + size.x + 5, at.y + size.y + 3), IM_COL32(20, 22, 26, 210), 4.f);
      dl->AddText(at, IM_COL32(235, 235, 225, 255), text.c_str());
    }
  }
  // what it would do there, before it is done: the right button held, the ground raised or dug that
  // much (its height next to it); else what the left button does (a smoothing or flattening done)
  if (m_terrain_change) {
    if (m_stroke_preview)
      draw_terrain_ghost(m_stroke_before, m_terrain_grid, m_stroke_cells[0], m_stroke_cells[1], m_stroke_cells[2],
                         m_stroke_cells[3], mode, world);
    return;
  }
  TerrainGrid empty;
  empty.cell = brush.cell;
  const TerrainGrid& before = node ? grid : empty;
  if (m_terrain_sculpting) {
    const float h = terrain_sculpt_height();
    // its height next to it (nothing when it changes nothing: erasing over the void)
    Vec2 s;
    if (draw_terrain_preview(before, terrain_stamp(h > 0 ? BrushMode::Raise : BrushMode::Dig, c, std::fabs(h), c.y), world) &&
        renderer.project(m_brush_point, &s)) {
      ImDrawList* dl = ImGui::GetForegroundDrawList();
      const std::string text = strf("%+.2f m", h);
      const ImVec2 at(m_vp_x + s.x + 16, m_vp_y + s.y - 8), size = ImGui::CalcTextSize(text.c_str());
      dl->AddRectFilled(ImVec2(at.x - 5, at.y - 3), ImVec2(at.x + size.x + 5, at.y + size.y + 3), IM_COL32(20, 22, 26, 210), 4.f);
      dl->AddText(at, IM_COL32(235, 235, 225, 255), text.c_str());
    }
    return;
  }
  if (m_terrain_rmb || (!node && mode != BrushMode::Paint && mode != BrushMode::Wall)) return;
  const float amount = mode == BrushMode::Wall ? brush.wall_height : mode == BrushMode::Smooth || mode == BrushMode::Flatten ? 1.f : 0.f;
  draw_terrain_preview(before, terrain_stamp(mode, c, amount, terrain_start_height(before, node)), world);
}

bool App::draw_terrain_preview(const TerrainGrid& before, const BrushStroke& stamp, const Mat4& world) {
  TerrainGrid after = before;
  if (!apply_brush(after, stamp)) return false;
  const float cl = after.cell, half = stamp.size * 0.5f;
  draw_terrain_ghost(before, after, (int)std::floor((stamp.center.x - half) / cl) - 1, (int)std::floor((stamp.center.z - half) / cl) - 1,
                     (int)std::floor((stamp.center.x + half) / cl) + 1, (int)std::floor((stamp.center.z + half) / cl) + 1,
                     stamp.mode, world);
  return true;
}

void App::draw_terrain_ghost(const TerrainGrid& before, const TerrainGrid& after, int i0, int k0, int i1, int k1,
                             BrushMode mode, const Mat4& world) {
  const auto changes = terrain_changes(before, after, i0, k0, i1, k1);
  const float cl = after.cell;
  const uint32_t color = mode == BrushMode::Wall  ? IM_COL32(240, 205, 90, 0)
                         : mode == BrushMode::Dig ? IM_COL32(255, 170, 60, 0)
                                                  : IM_COL32(90, 230, 255, 0);
  // seen where it is in front, and faintly through the ground (a hollow dug under it)
  for (const uint32_t alpha : {110u, 45u}) {
    for (const TerrainChange& ch : changes) {
      const TerrainGrid& g = ch.removed ? before : after;
      const uint32_t col = (ch.removed ? IM_COL32(255, 90, 60, 0) : color) | (alpha << 24);
      const float x = ch.i * cl, z = ch.k * cl;
      const float h[4] = {g.corner(ch.i, ch.k), g.corner(ch.i + 1, ch.k), g.corner(ch.i + 1, ch.k + 1), g.corner(ch.i, ch.k + 1)};
      const Vec3 xz[4] = {{x, 0, z}, {x + cl, 0, z}, {x + cl, 0, z + cl}, {x, 0, z + cl}};
      if (g.at(ch.i, ch.k) != TerrainGrid::Wall) {
        Vec3 p[4];
        for (int v = 0; v < 4; v++) p[v] = world.point({xz[v].x, h[v] + 0.05f, xz[v].z});
        renderer.tri(p[0], p[1], p[2], col);
        renderer.tri(p[0], p[2], p[3], col);
        continue;
      }
      // a wall: its column, from the ground to its top
      const float top = std::max({h[0], h[1], h[2], h[3]}) + g.wall[(size_t)(ch.k - g.z0) * g.w + (ch.i - g.x0)];
      const float base = std::min({h[0], h[1], h[2], h[3]});
      Vec3 lo[4], hi[4];
      for (int v = 0; v < 4; v++) {
        lo[v] = world.point({xz[v].x, base, xz[v].z});
        hi[v] = world.point({xz[v].x, top, xz[v].z});
      }
      renderer.tri(hi[0], hi[1], hi[2], col);
      renderer.tri(hi[0], hi[2], hi[3], col);
      for (int e = 0; e < 4; e++) {
        renderer.tri(lo[e], lo[(e + 1) % 4], hi[(e + 1) % 4], col);
        renderer.tri(lo[e], hi[(e + 1) % 4], hi[e], col);
      }
    }
    renderer.flush(alpha > 100u, true);
  }
}

bool App::ui_terrain_texture(const char* key, std::string* texture, const std::string& style_texture) {
  const std::vector<PaletteTexture>& list = level_textures(m_style_level);
  // the one chosen, else the style's (shown, marked as the style's)
  const PaletteTexture* current = nullptr;
  for (const auto& t : list)
    if (t.name == (texture->empty() ? style_texture : *texture)) current = &t;
  ImGui::PushID(key);
  // its picture (the style's: a mark) and its name; clicked, the level's textures to choose from
  const float size = ImGui::GetFrameHeight() * 1.6f;
  const unsigned gl = current ? palette.gl(*current) : 0;
  bool open = gl ? ImGui::ImageButton("##pic", (ImTextureID)(uint64_t)gl, ImVec2(size, size))
                 : ImGui::Button("--", ImVec2(size + 6, size + 6));
  tooltip(tr("terrain.texture.tip"));
  ImGui::SameLine();
  ImGui::BeginGroup();
  ImGui::TextUnformatted(tr(key));
  if (texture->empty()) ImGui::TextDisabled("%s", tr("terrain.texture.style"));
  else ImGui::TextDisabled("%s", texture->c_str());
  ImGui::EndGroup();
  open |= ImGui::IsItemClicked();
  if (open) {
    m_terrain_tex_filter.clear();
    ImGui::OpenPopup("choose");
  }
  bool changed = false;
  if (ImGui::BeginPopup("choose")) {
    if (ImGui::IsWindowAppearing()) ImGui::SetKeyboardFocusHere();
    ImGui::SetNextItemWidth(300);
    ImGui::InputTextWithHint("##filter", tr("textures.filter"), &m_terrain_tex_filter);
    if (ImGui::Selectable(tr("terrain.texture.style"), texture->empty())) {
      changed = !texture->empty();
      texture->clear();
    }
    std::string f = m_terrain_tex_filter;
    for (auto& c : f) c = (char)std::tolower((unsigned char)c);
    std::vector<const PaletteTexture*> shown;
    for (const auto& t : list)
      if (f.empty() || t.name.find(f) != std::string::npos) shown.push_back(&t);
    if (list.empty()) ImGui::TextDisabled("%s", tr("terrain.texture.none"));
    ImGui::BeginChild("grid", ImVec2(300, 320));
    const float cell = 64.f;
    const int per_row = std::max(1, (int)((ImGui::GetContentRegionAvail().x + ImGui::GetStyle().ItemSpacing.x) /
                                          (cell + 6 + ImGui::GetStyle().ItemSpacing.x)));
    ImGuiListClipper clipper;
    clipper.Begin((int)(shown.size() + per_row - 1) / per_row, cell + 6 + ImGui::GetStyle().ItemSpacing.y);
    while (clipper.Step())
      for (int r = clipper.DisplayStart; r < clipper.DisplayEnd; r++)
        for (int c = 0; c < per_row; c++) {
          const size_t i = (size_t)r * per_row + c;
          if (i >= shown.size()) break;
          if (c) ImGui::SameLine();
          ImGui::PushID((int)i);
          const unsigned tex = palette.gl(*shown[i]);
          const bool on = shown[i]->name == (texture->empty() ? style_texture : *texture);
          if (on) ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(0.22f, 0.6f, 0.66f, 1.f));
          ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(3, 3));
          const bool clicked = tex ? ImGui::ImageButton("##t", (ImTextureID)(uint64_t)tex, ImVec2(cell, cell))
                                   : ImGui::Button("...", ImVec2(cell + 6, cell + 6));
          ImGui::PopStyleVar();
          if (on) ImGui::PopStyleColor();
          if (ImGui::BeginItemTooltip()) {
            ImGui::TextUnformatted(shown[i]->name.c_str());
            ImGui::TextDisabled(tr("textures.used_level"), shown[i]->uses);
            ImGui::EndTooltip();
          }
          if (clicked) {
            changed = *texture != shown[i]->name;
            *texture = shown[i]->name;
            ImGui::CloseCurrentPopup();
          }
          ImGui::PopID();
        }
    ImGui::EndChild();
    ImGui::EndPopup();
  }
  ImGui::PopID();
  return changed;
}

void App::ui_terrain_window() {
  if (tool != ToolMode::Terrain) return;
  // a small window of its own over the view, at its top right at first; closed, the brush ends
  ImGui::SetNextWindowPos(ImVec2(m_vp_x + m_vp_w - 12.f, m_vp_y + 12.f), ImGuiCond_FirstUseEver, ImVec2(1.f, 0.f));
  ImGui::SetNextWindowSizeConstraints(ImVec2(270.f, 0.f), ImVec2(270.f, FLT_MAX));
  bool open = true;
  if (!ImGui::Begin(tr_id("terrain.title").c_str(), &open, ImGuiWindowFlags_AlwaysAutoResize | ImGuiWindowFlags_NoCollapse)) {
    ImGui::End();
    if (!open) stop_terrain_brush();
    return;
  }
  const Node* node = scene.find(m_terrain_node);
  // how to use it: the mouse's buttons and keys, on its "(?)"
  ImGui::TextDisabled("(?)");
  tooltip(tr("terrain.help"));
  ImGui::SameLine();
  disabled_wrapped(node ? strf(tr("terrain.painting"), node->name.c_str()) : std::string(tr("terrain.new")));
  if (node && ImGui::SmallButton(tr_id("terrain.start_new").c_str())) {
    m_terrain_node = 0;
    select_only(0);
  }
  if (node) tooltip(tr("terrain.start_new.tip"));
  // the level style painted with: the ground takes its floor texture, the walls and slopes its wall texture
  ImGui::SeparatorText(tr("terrain.style.title"));
  ImGui::SetNextItemWidth(-1);
  if (ImGui::BeginCombo("##terrain-style", m_style_level.empty() ? tr("terrain.style.grid") : m_style_level.c_str(),
                        ImGuiComboFlags_HeightLarge)) {
    if (ImGui::IsWindowAppearing()) ImGui::SetKeyboardFocusHere();
    ImGui::SetNextItemWidth(-1);
    ImGui::InputTextWithHint("##terrain-style-filter", tr("build.style.filter"), &m_style_filter);
    std::string f = m_style_filter;
    for (auto& c : f) c = (char)std::tolower((unsigned char)c);
    const std::string was = m_style_level;
    if (f.empty() && ImGui::Selectable(tr("terrain.style.grid"), m_style_level.empty())) m_style_level.clear();
    for (const auto& l : library.levels()) {
      if (!l.listed || (!f.empty() && l.name.find(f) == std::string::npos)) continue;
      if (ImGui::Selectable(l.name.c_str(), l.name == m_style_level)) m_style_level = l.name;
    }
    if (m_style_level != was) {  // the textures chosen were that level's
      brush.floor_texture.clear();
      brush.wall_texture.clear();
    }
    ImGui::EndCombo();
  }
  tooltip(tr("terrain.style.tip"));
  // the ground's and the walls' textures, among those of that level (the style's at first)
  if (!m_style_level.empty()) {
    const LevelStyle* style = level_style(m_style_level);
    const std::string floor = style && style->floor.material >= 0 ? style->floor.texture : std::string();
    const std::string wall = style && style->wall.material >= 0 ? style->wall.texture : floor;
    ui_terrain_texture("terrain.texture.floor", &brush.floor_texture, floor);
    ui_terrain_texture("terrain.texture.wall", &brush.wall_texture, wall);
  }
  if (!m_style_level.empty()) {
    const LevelStyle* style = level_style(m_style_level);
    const LevelEntry* level = library.level(m_style_level);
    if (!style)
      disabled_wrapped(tr(level && store.failed_level(level->background) ? "build.style.failed" : "build.style.reading"));
    else
      disabled_wrapped(strf(tr("terrain.style.textures"), style->floor.material >= 0 ? style->floor.texture.c_str() : "-",
                            style->wall.material >= 0 ? style->wall.texture.c_str() : style->floor.texture.c_str()));
  }
  if (node && ImGui::SmallButton(tr_id("terrain.style.apply").c_str())) {
    Node* n = scene.find(m_terrain_node);
    TerrainGrid g;
    if (n && node_terrain(*n, &g)) {
      const Node before = *n;
      g.looks = {terrain_look()};
      std::fill(g.look.begin(), g.look.end(), 0);
      n->extras["og_terrain_grid"] = g.to_json();
      n->mesh = make_terrain_mesh(g);
      light_like_surroundings(scene, {n->id});
      commit_node_edit(before, tr("undo.terrain"));
    }
  }
  if (node) tooltip(tr("terrain.style.apply.tip"));
  // the modes, two per line; the right button raises and digs in all of them
  ImGui::SeparatorText(tr("terrain.left_button"));
  for (int m = 0; m < (int)std::size(kModes); m++) {
    if (m % 2) ImGui::SameLine(ImGui::GetContentRegionAvail().x * 0.5f + ImGui::GetCursorStartPos().x);
    if (ImGui::RadioButton(tr_id(kModeKeys[m]).c_str(), brush.mode == kModes[m])) brush.mode = kModes[m];
    tooltip(tr((std::string(kModeKeys[m]) + ".tip").c_str()));
  }
  if (ImGui::RadioButton(tr_id("terrain.free").c_str(), !brush.magnet)) brush.magnet = false;
  tooltip(tr("terrain.free.tip"));
  ImGui::SameLine(ImGui::GetContentRegionAvail().x * 0.5f + ImGui::GetCursorStartPos().x);
  if (ImGui::RadioButton(tr_id("terrain.magnet").c_str(), brush.magnet)) brush.magnet = true;
  tooltip(tr("terrain.magnet.tip"));
  disabled_wrapped(tr("terrain.right_button"));
  ImGui::SetNextItemWidth(-100);
  ImGui::SliderFloat(tr_id("terrain.size").c_str(), &brush.size, 0.5f, 64.f, "%.1f m", ImGuiSliderFlags_Logarithmic);
  tooltip(tr("terrain.size.tip"));
  if (brush.mode == BrushMode::Smooth || brush.mode == BrushMode::Flatten) {
    ImGui::SetNextItemWidth(-100);
    ImGui::SliderFloat(tr_id("terrain.strength").c_str(), &brush.strength, 0.1f, 20.f, "%.1f", ImGuiSliderFlags_Logarithmic);
    tooltip(tr("terrain.strength.tip"));
  }
  if (brush.mode == BrushMode::Wall) {
    ImGui::SetNextItemWidth(-100);
    ImGui::SliderFloat(tr_id("terrain.wall_height").c_str(), &brush.wall_height, 0.5f, 30.f, "%.1f m");
  }
  if (brush.mode == BrushMode::Paint) {
    ImGui::Checkbox(tr_id("terrain.relief").c_str(), &brush.relief);
    tooltip(tr("terrain.relief.tip"));
  }
  if (!node) {
    ImGui::SetNextItemWidth(-100);
    ImGui::SliderFloat(tr_id("terrain.cell").c_str(), &brush.cell, 0.5f, 4.f, "%.1f m");
    tooltip(tr("terrain.cell.tip"));
  }
  // hills made at once (a fractal noise) where the brush is
  if (ImGui::TreeNode(tr_id("terrain.generate").c_str())) {
    ImGui::SetNextItemWidth(-100);
    ImGui::DragFloat2(tr_id("terrain.generate.size").c_str(), &brush.gen_size.x, 1.f, 8.f, 512.f, "%.0f m");
    ImGui::SetNextItemWidth(-100);
    ImGui::SliderFloat(tr_id("terrain.generate.hills").c_str(), &brush.hills, 0.f, 80.f, "%.1f m");
    ImGui::SetNextItemWidth(-100);
    ImGui::SliderFloat(tr_id("terrain.generate.roughness").c_str(), &brush.roughness, 0.f, 1.f, "%.2f");
    ImGui::SetNextItemWidth(-100);
    ImGui::InputInt(tr_id("terrain.generate.seed").c_str(), &brush.seed);
    if (ImGui::Button(tr_id("terrain.generate.go").c_str(), ImVec2(-1, 0))) {
      const Vec3 at = m_brush_valid ? m_brush_point : camera.pos + camera.forward() * 30.f;
      terrain_generate(at);
    }
    tooltip(tr("terrain.generate.tip"));
    ImGui::TreePop();
  }
  ImGui::End();
  if (!open) stop_terrain_brush();
}

void App::terrain_generate(Vec3 at) {
  if (!document_open()) return;
  NodeChange change(scene);
  Node* node = scene.find(m_terrain_node);
  bool made = false;
  TerrainGrid g;
  if (!node || !node_terrain(*node, &g)) {
    Node t;
    t.id = scene.next_id;
    t.name = terrain_name(scene);
    t.kind = NodeKind::Render;
    change.save(t.id);
    g = TerrainGrid{};
    g.cell = brush.cell;
    g.looks = {terrain_look()};
    m_terrain_node = scene.add(std::move(t));
    node = scene.find(m_terrain_node);
    made = true;
  } else {
    change.save(node->id);
  }
  const Vec3 local = scene.world(*node).inverse().point(at);
  generate_terrain(g, local, brush.gen_size, brush.hills, brush.roughness, brush.seed, g.add_look(terrain_look()));
  node->extras["og_terrain_grid"] = g.to_json();
  node->mesh = make_terrain_mesh(g);
  light_like_surroundings(scene, {node->id});
  history.push(change.commit(tr(made ? "undo.terrain.new" : "undo.terrain")));
}

}  // namespace ogle
