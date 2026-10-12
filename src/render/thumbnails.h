#pragma once

// Thumbnails of the panels: prefabs (and models), whole levels, decor parts of a level (the
// mesh of one of its nodes) and particle effects (their particles a moment after they start),
// drawn offscreen (3/4 view)
// and cached as PNG files in <preferences>/thumbs, so each is drawn once. get() returns 0 until
// the thumbnail is ready: the geometry is loaded by the AssetStore's worker threads and drawn by
// update() on the main thread.

#include <string>
#include <unordered_map>

#include "io/asset_store.h"
#include "render/renderer.h"

namespace ogle {

struct ThumbRequest {
  enum class Kind { Model = 0, Level, Part, Effect, Icon } kind = Kind::Model;
  // .glb: a model or prefab, or a level's background; Effect: its particle group; Icon: the type of
  // an object without a model (scene/shapes.h object_icon)
  std::string path;
  std::string node;   // Part: the node of the level whose mesh is drawn; Icon: the object's family
  int64_t stamp = 0;  // file date: a changed file gets a new thumbnail
  std::string key() const;
};

class Thumbnails {
 public:
  explicit Thumbnails(AssetStore& store) : m_store(store) {}
  bool init(std::string* error);  // needs the GL context
  void shutdown();
  unsigned get(const ThumbRequest& req);  // GL texture, 0 while it is not ready
  bool failed(const ThumbRequest& req) const;
  // Main thread, once per frame, before the viewport is drawn (the bound framebuffer is kept).
  void update();
  size_t pending() const;  // thumbnails asked for recently and not ready yet
  static constexpr int kSize = 128;

  // the effects: the game's particles and their textures (scene/particles.h)
  void set_particles(const ParticleData* data) { m_particles = data; }
  void set_sprite_folder(const std::string& folder) { m_renderer.set_sprite_folder(folder); }
  // A particle group running live (a tooltip's): its picture this frame, `dt` seconds after the
  // last; another group starts again
  unsigned live_effect(const std::string& group, float dt);

 private:
  struct Item {
    ThumbRequest req;
    unsigned tex = 0;
    uint64_t last_use = 0;
    bool disk_checked = false;
    bool failed = false;
  };
  bool render(Item& item);
  unsigned upload(const uint8_t* rgba, int w, int h);
  std::string cache_file(const std::string& key) const;

  bool render_effect(Item& item);
  void draw_particles(const ParticleSystem& ps, unsigned fbo);

  AssetStore& m_store;
  Renderer m_renderer;
  const ParticleData* m_particles = nullptr;
  ParticleSystem m_live;
  std::string m_live_group;
  unsigned m_live_fbo = 0, m_live_color = 0, m_live_depth = 0;
  std::unordered_map<std::string, Item> m_items;
  unsigned m_fbo = 0, m_color = 0, m_depth = 0;
  uint64_t m_frame = 0;
};

}  // namespace ogle
