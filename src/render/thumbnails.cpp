#include "render/thumbnails.h"

#include <algorithm>
#include <cstring>
#include <filesystem>

#include "core/log.h"
#include "core/prefs.h"
#include "glad/glad.h"
#include "scene/shapes.h"
#include "stb_image.h"
#include "stb_image_write.h"

namespace ogle {

namespace {

uint64_t fnv(const std::string& s) {
  uint64_t h = 1469598103934665603ull;
  for (unsigned char c : s) h = (h ^ c) * 1099511628211ull;
  return h;
}

}  // namespace

std::string ThumbRequest::key() const {
  if (kind == Kind::Part) return "p:" + path + "#" + node + "@" + std::to_string(stamp);
  if (kind == Kind::Effect) return "e2:" + path + "@" + std::to_string(stamp);
  if (kind == Kind::Icon) return "i1:" + path + "#" + node;
  return (kind == Kind::Model ? "m:" : "b:") + path + "@" + std::to_string(stamp);
}

bool Thumbnails::init(std::string* error) {
  if (!m_renderer.init(error)) return false;
  glGenFramebuffers(1, &m_fbo);
  glBindFramebuffer(GL_FRAMEBUFFER, m_fbo);
  glGenTextures(1, &m_color);
  glBindTexture(GL_TEXTURE_2D, m_color);
  glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, kSize, kSize, 0, GL_RGBA, GL_UNSIGNED_BYTE, nullptr);
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
  glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, m_color, 0);
  glGenRenderbuffers(1, &m_depth);
  glBindRenderbuffer(GL_RENDERBUFFER, m_depth);
  glRenderbufferStorage(GL_RENDERBUFFER, GL_DEPTH24_STENCIL8, kSize, kSize);
  glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_DEPTH_STENCIL_ATTACHMENT, GL_RENDERBUFFER, m_depth);
  bool ok = glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
  // the live effect of a tooltip, as large
  glGenFramebuffers(1, &m_live_fbo);
  glBindFramebuffer(GL_FRAMEBUFFER, m_live_fbo);
  glGenTextures(1, &m_live_color);
  glBindTexture(GL_TEXTURE_2D, m_live_color);
  glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, kSize, kSize, 0, GL_RGBA, GL_UNSIGNED_BYTE, nullptr);
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
  glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, m_live_color, 0);
  glGenRenderbuffers(1, &m_live_depth);
  glBindRenderbuffer(GL_RENDERBUFFER, m_live_depth);
  glRenderbufferStorage(GL_RENDERBUFFER, GL_DEPTH24_STENCIL8, kSize, kSize);
  glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_DEPTH_STENCIL_ATTACHMENT, GL_RENDERBUFFER, m_live_depth);
  ok &= glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
  glBindFramebuffer(GL_FRAMEBUFFER, 0);
  if (!ok && error) *error = "incomplete thumbnail framebuffer";
  std::error_code ec;
  std::filesystem::create_directories(std::filesystem::path(std::u8string((const char8_t*)(Prefs::dir() + "thumbs").c_str())), ec);
  return ok;
}

void Thumbnails::shutdown() {
  for (auto& [k, it] : m_items)
    if (it.tex) glDeleteTextures(1, &it.tex);
  m_items.clear();
  m_renderer.shutdown();
  if (m_fbo) glDeleteFramebuffers(1, &m_fbo);
  if (m_color) glDeleteTextures(1, &m_color);
  if (m_depth) glDeleteRenderbuffers(1, &m_depth);
  m_fbo = m_color = m_depth = 0;
  if (m_live_fbo) glDeleteFramebuffers(1, &m_live_fbo);
  if (m_live_color) glDeleteTextures(1, &m_live_color);
  if (m_live_depth) glDeleteRenderbuffers(1, &m_live_depth);
  m_live_fbo = m_live_color = m_live_depth = 0;
}

// The particles of an effect, framed (the 3/4 view of the thumbnails, a little from below the
// top: most effects rise), into a target of kSize
void Thumbnails::draw_particles(const ParticleSystem& ps, unsigned fbo) {
  AABB bounds = ps.bounds();
  if (!bounds.valid()) bounds = AABB{{-1, -1, -1}, {1, 1, 1}};
  Camera cam;
  cam.fov_deg = 40.f;
  cam.yaw = 0.65f;
  cam.pitch = -0.25f;
  cam.frame(bounds);
  cam.znear = std::max(0.01f, bounds.radius() * 0.01f);
  RenderSettings rs;
  rs.show_grid = false;
  rs.clear_color[0] = 0.07f;
  rs.clear_color[1] = 0.08f;
  rs.clear_color[2] = 0.1f;
  glBindFramebuffer(GL_FRAMEBUFFER, fbo);
  m_renderer.begin(cam, 0, 0, kSize, kSize, rs);
  std::vector<ParticleSprite> sprites;
  ps.sprites(sprites);
  m_renderer.draw_particles(sprites, m_particles->textures());
}

bool Thumbnails::render_effect(Item& item) {
  const SpGroup* group = m_particles ? m_particles->group(item.req.path) : nullptr;
  if (!group) {
    item.failed = true;
    return true;
  }
  // a moment after it starts: a second and a half of the game
  ParticleSystem ps;
  ps.begin_frame();
  ps.emit(1, group, Mat4::identity());
  for (int i = 0; i < 9; i++) ps.update(*m_particles, 1.0 / 6.0, {0, 0, 0}, -1.f);
  GLint previous = 0;
  glGetIntegerv(GL_FRAMEBUFFER_BINDING, &previous);
  draw_particles(ps, m_fbo);
  glFinish();
  std::vector<uint8_t> px((size_t)kSize * kSize * 4), flipped(px.size());
  glReadPixels(0, 0, kSize, kSize, GL_RGBA, GL_UNSIGNED_BYTE, px.data());
  glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous);
  for (int y = 0; y < kSize; y++)
    std::memcpy(&flipped[(size_t)y * kSize * 4], &px[(size_t)(kSize - 1 - y) * kSize * 4], (size_t)kSize * 4);
  for (size_t i = 3; i < flipped.size(); i += 4) flipped[i] = 255;
  stbi_write_png(cache_file(item.req.key()).c_str(), kSize, kSize, 4, flipped.data(), kSize * 4);
  item.tex = upload(flipped.data(), kSize, kSize);
  return true;
}

unsigned Thumbnails::live_effect(const std::string& name, float dt) {
  const SpGroup* group = m_particles ? m_particles->group(name) : nullptr;
  if (!group || !m_live_fbo) return 0;
  if (name != m_live_group) {
    m_live.clear();
    m_live_group = name;
  }
  m_live.begin_frame();
  m_live.emit(1, group, Mat4::identity());
  m_live.update(*m_particles, dt, {0, 0, 0}, -1.f);
  GLint previous = 0, viewport[4];
  glGetIntegerv(GL_FRAMEBUFFER_BINDING, &previous);
  glGetIntegerv(GL_VIEWPORT, viewport);
  draw_particles(m_live, m_live_fbo);
  glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous);
  glViewport(viewport[0], viewport[1], viewport[2], viewport[3]);
  return m_live_color;
}

std::string Thumbnails::cache_file(const std::string& key) const {
  char name[32];
  std::snprintf(name, sizeof(name), "%016llx.png", (unsigned long long)fnv(key));
  return Prefs::dir() + "thumbs/" + name;
}

unsigned Thumbnails::get(const ThumbRequest& req) {
  std::string key = req.key();
  auto it = m_items.find(key);
  if (it == m_items.end()) {
    Item item;
    item.req = req;
    it = m_items.emplace(key, std::move(item)).first;
  }
  it->second.last_use = m_frame;
  return it->second.tex;
}

bool Thumbnails::failed(const ThumbRequest& req) const {
  auto it = m_items.find(req.key());
  return it != m_items.end() && it->second.failed;
}

size_t Thumbnails::pending() const {
  size_t n = 0;
  for (auto& [k, it] : m_items)
    if (!it.tex && !it.failed && it.last_use + 2 >= m_frame) n++;
  return n;
}

unsigned Thumbnails::upload(const uint8_t* rgba, int w, int h) {
  unsigned tex = 0;
  glGenTextures(1, &tex);
  glBindTexture(GL_TEXTURE_2D, tex);
  glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
  glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_RGBA, GL_UNSIGNED_BYTE, rgba);
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
  return tex;
}

// true when the item is finished (drawn or failed), false while its data is loading
bool Thumbnails::render(Item& item) {
  const ThumbRequest& r = item.req;
  if (r.kind == ThumbRequest::Kind::Effect) return render_effect(item);
  // what to draw: meshes with their world matrices, and the arrays their materials index
  std::vector<std::pair<Mesh*, Mat4>> draws;
  const std::vector<Material>* materials = nullptr;
  const std::vector<Texture>* textures = nullptr;
  std::shared_ptr<LoadedMesh> model;
  std::shared_ptr<Scene> level;
  std::shared_ptr<Mesh> icon;
  static const std::vector<Material> kNoMaterials;
  static const std::vector<Texture> kNoTextures;
  if (r.kind == ThumbRequest::Kind::Icon) {
    icon = object_icon(r.path, r.node);
    draws.push_back({icon.get(), Mat4::identity()});
    materials = &kNoMaterials;
    textures = &kNoTextures;
  } else if (r.kind == ThumbRequest::Kind::Model) {
    model = m_store.model(r.path);
    if (!model) {
      item.failed = m_store.failed_model(r.path);
      return item.failed;
    }
    draws.push_back({model->mesh.get(), Mat4::identity()});
    materials = &model->materials;
    textures = &model->textures;
  } else {
    level = m_store.level(r.path);
    if (!level) {
      item.failed = m_store.failed_level(r.path);
      return item.failed;
    }
    materials = &level->materials;
    textures = &level->textures;
    if (r.kind == ThumbRequest::Kind::Part) {
      for (const auto& n : level->nodes)
        if (n.name == r.node && n.mesh) {
          draws.push_back({n.mesh.get(), Mat4::identity()});
          break;
        }
      if (draws.empty()) {
        item.failed = true;
        return true;
      }
    } else {
      for (const auto& n : level->nodes)
        if (n.kind == NodeKind::Render && n.mesh) draws.push_back({n.mesh.get(), level->world(n)});
    }
  }
  // a level is framed on where most of it is (a stray piece far away would make it tiny)
  AABB bounds = level && r.kind == ThumbRequest::Kind::Level ? level->focus_bounds() : AABB{};
  if (!bounds.valid())
    for (auto& [m, w] : draws) bounds.add(m->bounds().transformed(w));
  if (!bounds.valid()) {
    item.failed = true;
    return true;
  }
  // the renderer caches textures by index: a new thumbnail, new arrays
  m_renderer.reset();

  Camera cam;
  cam.fov_deg = 35.f;
  cam.yaw = 0.65f;
  cam.pitch = r.kind == ThumbRequest::Kind::Level ? -0.95f : -0.38f;
  cam.frame(bounds);
  cam.znear = std::max(0.01f, bounds.radius() * 0.01f);
  RenderSettings rs;
  rs.show_grid = false;
  rs.clear_color[0] = 0.19f;
  rs.clear_color[1] = 0.21f;
  rs.clear_color[2] = 0.24f;
  GLint previous = 0;
  glGetIntegerv(GL_FRAMEBUFFER_BINDING, &previous);
  glBindFramebuffer(GL_FRAMEBUFFER, m_fbo);
  m_renderer.begin(cam, 0, 0, kSize, kSize, rs);
  for (auto& [m, w] : draws) m_renderer.draw_mesh(*m, w, *materials, *textures, rs);
  glFinish();
  std::vector<uint8_t> px((size_t)kSize * kSize * 4), flipped(px.size());
  glReadPixels(0, 0, kSize, kSize, GL_RGBA, GL_UNSIGNED_BYTE, px.data());
  glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)previous);
  for (int y = 0; y < kSize; y++)
    std::memcpy(&flipped[(size_t)y * kSize * 4], &px[(size_t)(kSize - 1 - y) * kSize * 4], (size_t)kSize * 4);
  for (size_t i = 3; i < flipped.size(); i += 4) flipped[i] = 255;
  stbi_write_png(cache_file(r.key()).c_str(), kSize, kSize, 4, flipped.data(), kSize * 4);
  item.tex = upload(flipped.data(), kSize, kSize);
  m_renderer.reset();  // its meshes and textures are not drawn again
  return true;
}

void Thumbnails::update() {
  m_frame++;
  int disk_budget = 48;   // cached PNGs read per frame
  int draw_budget = 3;    // thumbnails drawn per frame
  for (auto& [key, item] : m_items) {
    if (item.tex || item.failed || item.last_use + 2 < m_frame) continue;  // not on screen
    if (!item.disk_checked) {
      if (disk_budget <= 0) continue;
      disk_budget--;
      item.disk_checked = true;
      int w, h, comp;
      unsigned char* data = stbi_load(cache_file(key).c_str(), &w, &h, &comp, 4);
      if (data) {
        item.tex = upload(data, w, h);
        stbi_image_free(data);
        continue;
      }
    }
    if (draw_budget > 0) {
      if (render(item) && item.tex) draw_budget--;
    } else {
      // keep its data queued in the store
      if (item.req.kind == ThumbRequest::Kind::Model) m_store.model(item.req.path);
      else if (item.req.kind == ThumbRequest::Kind::Level || item.req.kind == ThumbRequest::Kind::Part)
        m_store.level(item.req.path);
    }
  }
  // forget thumbnails that left the screen long ago
  if (m_items.size() > 2500) {
    for (auto it = m_items.begin(); it != m_items.end();) {
      if (it->second.last_use + 600 < m_frame) {
        if (it->second.tex) glDeleteTextures(1, &it->second.tex);
        it = m_items.erase(it);
      } else {
        ++it;
      }
    }
  }
}

}  // namespace ogle
