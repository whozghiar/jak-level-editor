#pragma once

// Short animations of the editor's tools, for the guided tour and the tooltips: a small scene of
// the editor's own shapes (nothing of the game), the gizmo and the cursor, scripted over a loop of a
// few seconds and drawn into a texture. What goes over the picture (the cursor, a click, a label, a
// selection box) is given back in its pixels, for the interface to draw.

#include <map>
#include <memory>
#include <string>
#include <vector>

#include "render/offscreen.h"
#include "render/renderer.h"
#include "scene/shapes.h"

namespace ogle {

struct DemoOverlay {
  enum class Kind { Cursor, Click, Rect, Label } kind = Kind::Cursor;
  Vec2 at, to;       // pixels of the picture (Rect: two corners; Click: `to.x` its growth 0..1)
  // Label: a key of the translations (guide.*) or the text itself; Rect: "" a selection box, "bar"
  // a slider's track, "knob" its knob
  std::string text;
};

class ToolDemo {
 public:
  bool init(std::string* error);  // needs the GL context
  void shutdown();
  // The picture of demo `name` at `t` seconds (it loops), w x h pixels: its GL texture (bottom row
  // first: shown with its v flipped), 0 for a demo that does not exist. `overlay` gets what goes over it.
  unsigned draw(const std::string& name, double t, int w, int h, std::vector<DemoOverlay>* overlay);
  // the demos: select, box_select, move, rotate, scale, snap, place, build, prefab, camera, bounce,
  // launch, replace, story
  static const std::vector<std::string>& names();
  // A shape seen from above at three quarters, `size` pixels square (drawn once): the buttons of the
  // Build panel
  unsigned icon(const ShapeParams& shape, const std::string& key, int size);

 private:
  struct Shape {
    std::shared_ptr<Mesh> mesh;
    int material = 0;
  };
  void scene_once();
  void mesh(const Shape& s, const Mat4& world, const Vec4& tint = {0, 0, 0, 0}, float alpha = 1.f);
  void gizmo_axes(const Vec3& at, float size, int hot);
  void gizmo_rings(const Vec3& at, float size, int hot);
  Vec2 screen(const Vec3& world) const;

  Renderer m_r;
  RenderSettings m_rs;
  // by demo and size ("move@320x180"); a size left behind (a window resized) is freed later
  std::map<std::string, std::unique_ptr<OffscreenTarget>> m_targets;
  std::map<std::string, uint64_t> m_used;  // when each was last drawn (m_tick)
  uint64_t m_tick = 0;
  std::vector<Material> m_materials;
  std::vector<Texture> m_textures;
  Shape m_ground, m_box, m_pillar, m_wall, m_back, m_side, m_pad, m_ball, m_roof, m_tower, m_cap;
  std::map<std::string, std::unique_ptr<OffscreenTarget>> m_icons;
  bool begin_target(std::unique_ptr<OffscreenTarget>& target, int w, int h);
  bool m_ready = false;
};

}  // namespace ogle
