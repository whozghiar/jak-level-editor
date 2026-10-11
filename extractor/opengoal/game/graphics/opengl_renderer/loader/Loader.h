#pragma once

// ogle: the editor's loader. The game's reads the .fr3 files of the levels on a thread and uploads
// them over several frames; the editor builds its levels itself (src/game_view) and gives them
// here by name, uploaded.

#include <map>
#include <string>

#include "game/graphics/opengl_renderer/loader/common.h"

class Loader {
 public:
  LevelData* get_tfrag3_level(const std::string& level_name) {
    auto it = levels.find(level_name);
    return it == levels.end() ? nullptr : it->second;
  }
  std::map<std::string, LevelData*> levels;
};
