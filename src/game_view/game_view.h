#pragma once

// The level as the game draws it (Jak 3), in the viewport. The document's decor is made into what
// OpenGOAL's level builder makes of a custom level (tfrag trees: the triangles of every piece in
// the game's units, a draw per material, the 8 palettes of each vertex: game_export.h writes the
// same), and drawn by the game's own renderer (OpenGOAL's TFragment and its shaders, vendored in
// extractor/opengoal), with the game's camera, its time of the day (game_mood.h) and its fog.
//
// The level is made in chunks: the pieces sharing a palette (a tree of an extracted level, or the
// new geometry) in a cell of the map. An edit makes the chunks it touched again, at once.
// Actors' models, the levels shown for reference and the editor's overlays are the editor's to
// draw, with the game's camera and depth (Renderer::set_game_depth).
//
// The game's shaders need OpenGL 4.1.

#include <memory>
#include <string>

#include "core/math.h"
#include "game_view/game_mood.h"
#include "render/camera.h"
#include "scene/scene.h"

namespace ogle {

class GameView {
 public:
  GameView();
  ~GameView();
  // the game's shaders are read from `shader_dir`
  bool init(const std::string& shader_dir, std::string* error);
  void shutdown();
  void reset();  // a new document: its chunks and textures are made again
  bool ready() const;

  // The decor's opaque and masked surfaces in the viewport (pixels, origin bottom-left), after
  // clearing it to the sky and its depth the game's way (0: far). The game camera's matrix (world
  // meters -> clip space) goes to `clip` for what the editor draws over it.
  void draw_opaque(const Scene& scene, const Camera& cam, int x, int y, int w, int h, Mat4* clip);
  // then its translucent surfaces (water, glass, glows), after the editor's opaque models
  void draw_translucent();

  // the hour, the weather, the level's lights (the Game rendering window); the fog is always the
  // game's
  MoodSettings mood;
  float time = 0.f;  // seconds: the clouds drift
  // a half turn of the sky around `yaw` (the camera's angle) in a texture of w x h, for the Game
  // rendering window (sky.h); 0 when it cannot be drawn
  unsigned sky_preview(int w, int h, float yaw);
  struct Stats {
    size_t chunks = 0, triangles = 0, rebuilt = 0;
    double ms_build = 0;  // the last chunks made
    std::string error;    // the last failure of the game's renderer
  } stats;
  const GameMood& current_mood() const;

 private:
  struct Impl;
  std::unique_ptr<Impl> m;
};

}  // namespace ogle
