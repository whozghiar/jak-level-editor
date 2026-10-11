#include "render/tool_demo.h"

#include <algorithm>
#include <cmath>
#include <cstdio>

#include "glad/glad.h"

namespace ogle {

namespace {

float clamp01(float v) { return v < 0 ? 0 : v > 1 ? 1 : v; }
// 0 before a, 1 after b, eased between
float ease(float t, float a, float b) {
  const float x = clamp01((t - a) / (b - a));
  return x * x * (3 - 2 * x);
}
Vec3 mix(const Vec3& a, const Vec3& b, float k) { return a + (b - a) * k; }

const Vec4 kSelected{1.f, 0.6f, 0.12f, 0.35f};
const Vec4 kWallColor{0.92f, 0.88f, 0.78f, 1};
const uint32_t kRed = 0xff4848e8, kGreen = 0xff5ace60, kBlue = 0xfff58450, kHot = 0xff46dcff, kOrange = 0xff269eff;

// the framebuffer and viewport drawn to before, given back at the end of the scope
struct KeepTarget {
  GLint fbo = 0, viewport[4] = {0, 0, 0, 0};
  KeepTarget() {
    glGetIntegerv(GL_FRAMEBUFFER_BINDING, &fbo);
    glGetIntegerv(GL_VIEWPORT, viewport);
  }
  ~KeepTarget() {
    glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)fbo);
    glViewport(viewport[0], viewport[1], viewport[2], viewport[3]);
  }
};

}  // namespace

const std::vector<std::string>& ToolDemo::names() {
  static const std::vector<std::string> n = {"select", "box_select", "move",   "rotate", "scale",   "snap",   "place",
                                             "build",  "prefab",     "camera", "bounce", "launch", "replace", "story"};
  return n;
}

bool ToolDemo::init(std::string* error) {
  if (!m_r.init(error)) return false;
  m_rs.show_grid = false;
  m_rs.fog = false;
  m_rs.clear_color[0] = 0.13f;
  m_rs.clear_color[1] = 0.15f;
  m_rs.clear_color[2] = 0.18f;
  return true;
}

void ToolDemo::shutdown() {
  m_targets.clear();
  m_used.clear();
  m_icons.clear();
  m_r.shutdown();
}

void ToolDemo::scene_once() {
  if (m_ready) return;
  m_ready = true;
  // the editor's grid texture, and a color per kind of thing
  Scene tmp;
  const int grid = shape_default_material(tmp);
  m_textures = tmp.textures;
  auto material = [&](Vec4 color) {
    Material m = tmp.materials[grid];
    m.base_color = color;
    m_materials.push_back(m);
    return (int)m_materials.size() - 1;
  };
  const int ground = material({0.8f, 0.82f, 0.85f, 1}), crate = material({1.f, 0.62f, 0.3f, 1}),
            pillar = material({0.55f, 0.72f, 1.f, 1}), wall = material(kWallColor), tower = material(kWallColor),
            pad = material({1.f, 0.38f, 0.32f, 1}), ball = material({1.f, 0.9f, 0.3f, 1});
  auto shape = [&](const char* kind, Vec3 size, int mat) {
    ShapeParams p;
    p.kind = kind;
    p.size = size;
    p.tile = 1.f;
    return Shape{make_shape(p, mat), mat};
  };
  m_ground = shape("box", {14, 0.2f, 14}, ground);
  m_box = shape("box", {1.6f, 1.6f, 1.6f}, crate);
  m_pillar = shape("cylinder", {1.2f, 3.f, 1.2f}, pillar);
  m_wall = shape("doorway", {4, 3, 0.4f}, wall);
  m_back = shape("box", {4, 3, 0.4f}, wall);
  m_side = shape("window", {3.2f, 3, 0.4f}, wall);
  m_pad = shape("cylinder", {1.8f, 0.3f, 1.8f}, pad);
  m_ball = shape("cylinder", {0.6f, 1.f, 0.6f}, ball);
  m_roof = shape("roof", {4.4f, 1.6f, 4.4f}, crate);
  m_tower = shape("box", {2, 5, 2}, tower);
  m_cap = shape("roof-hip", {2.6f, 1.4f, 2.6f}, pad);
}

bool ToolDemo::begin_target(std::unique_ptr<OffscreenTarget>& target, int w, int h) {
  if (!target || target->width != w || target->height != h) {
    target = std::make_unique<OffscreenTarget>();
    if (!target->create(w, h)) {
      target.reset();
      return false;
    }
    // shown by the interface: sampled without mipmaps
    glBindTexture(GL_TEXTURE_2D, target->color);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
  }
  glBindFramebuffer(GL_FRAMEBUFFER, target->fbo);
  return true;
}

void ToolDemo::mesh(const Shape& s, const Mat4& world, const Vec4& tint, float alpha) {
  m_r.draw_mesh(*s.mesh, world, m_materials, m_textures, m_rs, tint, alpha);
  if (tint.x == kSelected.x && tint.w == kSelected.w) {  // the selection's outline
    m_r.box_lines(world, s.mesh->bounds(), kOrange);
    m_r.flush(true, true);
  }
}

Vec2 ToolDemo::screen(const Vec3& world) const {
  Vec2 s{0, 0};
  m_r.project(world, &s);
  return s;
}

void ToolDemo::gizmo_axes(const Vec3& at, float size, int hot) {
  const Vec3 axes[3] = {{1, 0, 0}, {0, 1, 0}, {0, 0, 1}};
  const uint32_t colors[3] = {kRed, kGreen, kBlue};
  for (int i = 0; i < 3; i++) {
    const uint32_t c = hot == i ? kHot : colors[i];
    const Vec3 tip = at + axes[i] * size, u = axes[(i + 1) % 3] * (size * 0.08f), v = axes[(i + 2) % 3] * (size * 0.08f);
    m_r.line(at, tip, c);
    m_r.line(tip - u, tip + u, c);  // a small cross at the tip: the handle
    m_r.line(tip - v, tip + v, c);
  }
  m_r.flush(false);
}

void ToolDemo::gizmo_rings(const Vec3& at, float size, int hot) {
  m_r.circle(at, {0, 1, 0}, {0, 0, 1}, size, hot == 0 ? kHot : kRed, 48);
  m_r.circle(at, {1, 0, 0}, {0, 0, 1}, size, hot == 1 ? kHot : kGreen, 48);
  m_r.circle(at, {1, 0, 0}, {0, 1, 0}, size, hot == 2 ? kHot : kBlue, 48);
  m_r.flush(false);
}

unsigned ToolDemo::icon(const ShapeParams& shape, const std::string& key, int size) {
  auto& target = m_icons[key];
  if (target && target->width == size) return target->color;
  scene_once();
  KeepTarget keep;
  if (!begin_target(target, size, size)) return 0;
  const int wall = m_wall.material;
  const auto mesh = make_shape(shape, wall);
  Camera cam;
  cam.fov_deg = 35.f;
  cam.yaw = 0.65f;
  cam.pitch = -0.5f;
  cam.frame(mesh->bounds());
  m_materials[wall].base_color = kWallColor;
  m_r.begin(cam, 0, 0, size, size, m_rs, (float)size, (float)size);
  m_r.draw_mesh(*mesh, Mat4::identity(), m_materials, m_textures, m_rs);
  return target->color;
}

unsigned ToolDemo::draw(const std::string& name, double time, int w, int h, std::vector<DemoOverlay>* overlay) {
  if (std::find(names().begin(), names().end(), name) == names().end()) return 0;
  scene_once();
  KeepTarget keep;
  const std::string key = name + "@" + std::to_string(w) + "x" + std::to_string(h);
  m_used[key] = ++m_tick;
  if (!m_targets.count(key) && m_targets.size() >= 32)  // the sizes not drawn for a while
    for (auto it = m_targets.begin(); it != m_targets.end();)
      if (m_used[it->first] + 64 < m_tick) {
        m_used.erase(it->first);
        it = m_targets.erase(it);
      } else {
        ++it;
      }
  auto& target = m_targets[key];
  if (!begin_target(target, w, h)) return 0;
  if (overlay) overlay->clear();
  auto cursor = [&](const Vec2& p) {
    if (overlay) overlay->push_back({DemoOverlay::Kind::Cursor, p, {}, ""});
  };
  auto click = [&](const Vec2& p, float k) {
    if (overlay && k > 0 && k < 1) overlay->push_back({DemoOverlay::Kind::Click, p, {k, 0}, ""});
  };
  auto label = [&](const Vec2& p, const std::string& text) {
    if (overlay) overlay->push_back({DemoOverlay::Kind::Label, p, {}, text});
  };
  auto rect = [&](const Vec2& a, const Vec2& b, const char* style) {
    if (overlay) overlay->push_back({DemoOverlay::Kind::Rect, a, b, style});
  };
  // what the gizmo shows while it is dragged, next to the cursor
  auto readout = [&](const Vec2& at, const char* format, float value) {
    char text[48];
    std::snprintf(text, sizeof(text), format, value);
    label(at + Vec2{14, 10}, text);
  };
  const Vec2 corner{8.f, (float)h - 26.f};
  const Vec2 start{(float)w * 0.78f, (float)h * 0.92f};  // where the cursor comes from
  m_materials[m_wall.material].base_color = kWallColor;

  float t = 0;
  auto period = [&](float p) { t = (float)std::fmod(time, (double)p); };
  Camera cam;
  cam.fov_deg = 45.f;
  cam.zfar = 200.f;
  cam.pos = {7, 5.5f, 9};
  cam.look_at({0, 0.8f, 0});
  if (name == "build" || name == "story") {  // taller
    cam.pos = {8, 7, 11};
    cam.look_at({0, 2.2f, 0});
  } else if (name == "replace" || name == "launch") {  // wider
    cam.pos = {0, 7, 13};
    cam.look_at({0, 1.f, 0});
  } else if (name == "camera") {  // flying around
    period(6);
    const float a = t / 6.f * 2.f * kPi;
    cam.pos = {std::sin(a) * 11.f, 5.5f + std::sin(a * 2) * 1.5f, std::cos(a) * 11.f};
    cam.look_at({0, 0.8f, 0});
  }
  m_r.begin(cam, 0, 0, w, h, m_rs, (float)w, (float)h);
  mesh(m_ground, Mat4::translate({0, -0.2f, 0}));

  if (name == "select") {
    period(4);
    const Vec3 a{-1.6f, 0, 0}, b{2, 0, -1};
    mesh(m_box, Mat4::translate(a), t > 1.1f && t < 3.f ? kSelected : Vec4{});
    mesh(m_pillar, Mat4::translate(b), t >= 3.f ? kSelected : Vec4{});
    const Vec2 pa = screen(a + Vec3{0, 0.8f, 0}), pb = screen(b + Vec3{0, 1.5f, 0});
    cursor(t < 2.4f ? start + (pa - start) * ease(t, 0.2f, 1.f) : pa + (pb - pa) * ease(t, 2.4f, 2.95f));
    click(pa, (t - 1.1f) / 0.4f);
    click(pb, (t - 3.f) / 0.4f);
    label(corner, "guide.demo.select");
  } else if (name == "box_select") {
    period(4);
    const bool selected = t > 2.2f;
    for (const Vec3& p : {Vec3{-2.5f, 0, 0.5f}, Vec3{0, 0, 0}, Vec3{2.5f, 0, -0.5f}})
      mesh(m_box, Mat4::translate(p), selected ? kSelected : Vec4{});
    const Vec2 a = screen({-3.8f, 2.4f, 1.6f}), b = screen({3.8f, -0.2f, -1.6f});
    const Vec2 p = t < 0.8f ? start + (a - start) * ease(t, 0.f, 0.8f) : a + (b - a) * ease(t, 0.8f, 2.2f);
    if (t > 0.8f && t < 2.2f) rect(a, p, "");
    cursor(p);
    label(corner, "guide.demo.box_select");
  } else if (name == "move" || name == "snap") {
    period(4);
    float dx = 3.f * ease(t, 1.f, 2.8f);
    if (name == "snap") dx = std::floor(dx + 0.5f);  // by steps of 1 m
    const Vec3 at{-1.5f + dx, 0, 0}, pivot = at + Vec3{0, 0.8f, 0};
    mesh(m_box, Mat4::translate(at), kSelected);
    gizmo_axes(pivot, 1.8f, t > 0.8f && t < 3.f ? 0 : -1);
    const Vec2 grip = screen(pivot + Vec3{1.4f, 0, 0});
    cursor(t < 0.9f ? start + (grip - start) * ease(t, 0.f, 0.9f) : grip);
    if (t > 1.f && t < 3.2f) readout(grip, "X %+.2f m", dx);
    label(corner, name == "snap" ? "guide.demo.snap" : "guide.demo.move");
  } else if (name == "rotate") {
    period(4);
    const float k = ease(t, 1.f, 2.8f);
    const Vec3 pivot{0, 0.8f, 0};
    mesh(m_box, Mat4::rotate(Quat::axis_angle({0, 1, 0}, radians(45.f) * k)), kSelected);
    gizmo_rings(pivot, 1.6f, t > 0.8f && t < 3.f ? 1 : -1);
    const float a = radians(-30.f + 45.f * k);
    const Vec2 grip = screen(pivot + Vec3{std::sin(a), 0, std::cos(a)} * 1.6f);
    cursor(t < 0.9f ? start + (grip - start) * ease(t, 0.f, 0.9f) : grip);
    if (t > 1.f && t < 3.2f) readout(grip, "Y %+.0f\xc2\xb0", 45.f * k);
    label(corner, "guide.demo.rotate");
  } else if (name == "scale") {
    period(4);
    const float k = 1.f + ease(t, 1.f, 2.8f);
    const Vec3 pivot{0, 0.8f, 0};
    mesh(m_box, Mat4::scale({k, 1, 1}), kSelected);
    gizmo_axes(pivot, 1.f + 0.8f * k, t > 0.8f && t < 3.f ? 0 : -1);
    const Vec2 grip = screen(pivot + Vec3{1.f + 0.8f * k, 0, 0});
    cursor(t < 0.9f ? start + (grip - start) * ease(t, 0.f, 0.9f) : grip);
    if (t > 1.f && t < 3.2f) readout(grip, "X x%.2f", k);
    label(corner, "guide.demo.scale");
  } else if (name == "place") {
    // the piece follows the cursor on the ground; each click leaves a copy
    period(4);
    const Vec3 a{-3, 0, 1}, b{3, 0, -1};
    auto along = [&](float s) { return mix(a, b, ease(s, 0.f, 3.6f)); };
    if (t > 1.3f) mesh(m_box, Mat4::translate(along(1.3f)));
    if (t > 2.7f) mesh(m_box, Mat4::translate(along(2.7f)));
    mesh(m_box, Mat4::translate(along(t)), {0.35f, 0.75f, 1.f, 0.25f}, 0.55f);
    click(screen(along(1.3f) + Vec3{0, 0.8f, 0}), (t - 1.3f) / 0.4f);
    click(screen(along(2.7f) + Vec3{0, 0.8f, 0}), (t - 2.7f) / 0.4f);
    cursor(screen(along(t) + Vec3{0, 0.8f, 0}));
    label(corner, "guide.demo.place");
  } else if (name == "build") {
    // walls rise one after the other, a roof on them, then the palette's materials on the selection
    period(6);
    static const Vec4 colors[3] = {{0.7f, 0.85f, 1.f, 1}, {1.f, 0.75f, 0.55f, 1}, {0.7f, 0.95f, 0.7f, 1}};
    const bool painting = t > 2.4f;
    if (painting) m_materials[m_wall.material].base_color = colors[(int)((t - 2.4f) / 1.2f) % 3];
    const Vec4 tint = painting ? kSelected : Vec4{};
    auto rise = [&](const Shape& s, const Mat4& at, float from) {
      const float k = ease(t, from, from + 0.4f);
      if (k > 0) mesh(s, at * Mat4::scale({1, k, 1}), tint);
    };
    const Mat4 quarter = Mat4::rotate(Quat::axis_angle({0, 1, 0}, radians(90.f)));
    rise(m_wall, Mat4::translate({0, 0, 1.8f}), 0.1f);
    rise(m_back, Mat4::translate({0, 0, -1.8f}), 0.4f);
    rise(m_side, Mat4::translate({-1.8f, 0, 0}) * quarter, 0.7f);
    rise(m_side, Mat4::translate({1.8f, 0, 0}) * quarter, 1.f);
    if (t > 1.5f) mesh(m_roof, Mat4::translate({0, 3.f + 2.f * (1.f - ease(t, 1.5f, 1.9f)), 0}));
    label(corner, painting ? "guide.demo.build_paint" : "guide.demo.build");
  } else if (name == "prefab") {
    // a group selected and saved, then placed again elsewhere
    period(5);
    auto group = [&](const Vec3& at, const Vec4& tint) {
      mesh(m_box, Mat4::translate(at + Vec3{-1.2f, 0, 0}), tint);
      mesh(m_pillar, Mat4::translate(at + Vec3{1.f, 0, -0.4f}), tint);
      mesh(m_pad, Mat4::translate(at + Vec3{0, 0, 1.4f}), tint);
    };
    group({-2, 0, 0}, t > 0.6f && t < 2.f ? kSelected : Vec4{});
    if (t > 2.4f) group(mix({8, 0, -4}, {3.2f, 0, -1}, ease(t, 2.4f, 3.6f)), {});
    label(corner, t < 2.2f ? "guide.demo.prefab" : "guide.demo.prefab_place");
  } else if (name == "camera") {
    mesh(m_box, Mat4::translate({-1.6f, 0, 0}));
    mesh(m_pillar, Mat4::translate({2, 0, -1}));
    mesh(m_wall, Mat4::translate({0, 0, -3}));
    label(corner, "guide.demo.camera");
  } else if (name == "bounce") {
    // falls on the pad, thrown up, back down
    period(3);
    mesh(m_pad, Mat4::identity());
    float y = 0;
    if (t < 0.5f) {
      y = 3.f - 3.f * (t / 0.5f) * (t / 0.5f);
    } else {
      const float u = (t - 0.5f) / 2.3f;
      y = std::max(0.f, 18.f * u * (1.f - u));
    }
    mesh(m_ball, Mat4::translate({0, 0.3f + y, 0}));
    label(corner, "guide.demo.bounce");
  } else if (name == "launch") {
    // thrown from the pad onto the block
    period(3.5f);
    mesh(m_pad, Mat4::translate({-3, 0, 0}));
    mesh(m_box, Mat4::translate({3.5f, 0, 0}) * Mat4::scale({1.4f, 1.f, 1.4f}));
    Vec3 p;
    if (t < 0.8f) {
      p = mix({-5.5f, 0, 0}, {-3, 0.3f, 0}, t / 0.8f);
    } else {
      const float u = clamp01((t - 0.8f) / 1.5f);
      p = mix({-3, 0.3f, 0}, {3.5f, 1.6f, 0}, u) + Vec3{0, 12.f * u * (1 - u), 0};
    }
    mesh(m_ball, Mat4::translate(p));
    label(corner, "guide.demo.launch");
  } else if (name == "replace") {
    // the original in blue between its neighbors; the new level slides and turns into its place
    period(5);
    mesh(m_tower, Mat4::translate({-5, 0, 0}) * Mat4::scale({1.5f, 0.6f, 2}));
    mesh(m_tower, Mat4::translate({5, 0, 0}) * Mat4::scale({1.5f, 0.6f, 2}));
    mesh(m_wall, Mat4::identity(), {0.3f, 0.55f, 1.f, 0.6f}, 0.45f);
    const float k = ease(t, 0.5f, 2.8f);
    mesh(m_wall, Mat4::translate(mix({0, 0, 6}, {0, 0, 0}, k)) * Mat4::rotate(Quat::axis_angle({0, 1, 0}, radians(90.f * (1 - k)))),
         t < 3.2f ? kSelected : Vec4{});
    label(corner, "guide.demo.replace");
  } else if (name == "story") {
    // the story's slider: the roof is there at the start of the game, gone later
    period(5);
    mesh(m_tower, Mat4::identity());
    const bool before = t < 2.5f;
    if (before) mesh(m_cap, Mat4::translate({0, 5, 0}));
    const Vec2 a{20, 14}, b{(float)w - 20, 18};
    rect(a, b, "bar");
    const float x = a.x + (b.x - a.x) * (0.25f + 0.55f * ease(t, 2.1f, 2.5f));
    rect({x - 5, a.y - 5}, {x + 5, b.y + 5}, "knob");
    cursor({x, b.y + 2});
    label(corner, before ? "guide.demo.story_before" : "guide.demo.story_after");
  }
  return target->color;
}

}  // namespace ogle
