#pragma once

// ogle: the editor's stand-in for the game's texture pool (textures addressed by their place in the
// PS2's VRAM, filled by the game). The background renderers the editor uses bind the level's own
// textures (LevelData::textures) instead. The headers the game's brings, for its includers.

#include <array>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

#include "common/common_types.h"
#include "common/log/log.h"

class TexturePool;
