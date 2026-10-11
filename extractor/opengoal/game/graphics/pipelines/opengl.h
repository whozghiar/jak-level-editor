#pragma once

// ogle: the editor's stand-in for the game's graphics pipeline header (game/graphics/pipelines/
// opengl.h, which needs the game's display and input): OpenGL, and the global settings the
// vendored renderers read.

#include "third-party/glad/include/glad/glad.h"

namespace Gfx {
struct GlobalSettings {
  int lod_tfrag = 0;  // the highest detail
  int lod_tie = 0;
  bool hack_no_tex = false;
};
extern GlobalSettings g_global_settings;
}  // namespace Gfx
