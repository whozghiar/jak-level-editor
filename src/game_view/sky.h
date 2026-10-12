#pragma once

// The sky of the game view: its colors at the hour and in the weather (game_mood.h: the mood's sky
// high up, its fog at the horizon), the sun, the moon and the stars, the clouds drifting, lit like
// the game lights them (sky-tng: the ambient light and the sun on them, darker in a storm), the
// flash of the lightning. Drawn behind the level, or as a half turn around the view for the Game
// rendering window (a whole turn loses the sense of where things are).

#include "game_view/game_mood.h"
#include "render/camera.h"

namespace ogle {

class Sky {
 public:
  ~Sky();
  // the sky the camera sees, over the viewport bound (nothing of the depth changes)
  void draw(const GameMood& m, const Camera& cam, float aspect, float time);
  // a half turn of the sky centered on `yaw` (the camera's angle), from 10 degrees under the horizon
  // to 70 above, in a texture of w x h; 0 when it cannot be drawn
  unsigned preview(const GameMood& m, int w, int h, float yaw, float time);
  void shutdown();

 private:
  bool init();
  void uniforms(const GameMood& m, float time);
  unsigned m_prog = 0, m_vao = 0, m_fbo = 0, m_tex = 0;
  int m_tw = 0, m_th = 0;
  bool m_failed = false;
};

}  // namespace ogle
