#pragma once

// The game's lighting at an hour of the day (Jak 3), as its mood code computes it for a level
// (goal_src/jak3/engine/gfx/mood: update-mood-exterior, update-mood-direction,
// update-mood-itimes, with the clear weather's tables of mood-tables.gc): the weights of the 8
// palettes every vertex of the level mixes, the fog and the sky.
//
//   palette 0       the ambient light
//   palettes 1..4   the sun from 4 directions (dawn to dusk)
//   palettes 5..7   the level's own lights: in the city, 5 its lamps (on at night), 6 lights always
//                   on, 7 lights that flicker (update-mood-ctyindb...)

#include <array>
#include <cstdint>

namespace ogle {

struct GameMood {
  // per palette: its color (r, g, b) and weight (w), as the game's mood-context times
  std::array<std::array<float, 4>, 8> times{};
  // the same, packed as the renderers take them (mood-context itimes: 64 is 1.0)
  std::array<std::array<int32_t, 4>, 4> itimes{};
  float fog_color[3] = {0, 0, 0};  // 0..255
  float fog_start = 0, fog_end = 0;  // game units (4096 per meter)
  float fog_max = 255, fog_min = 0;  // how much of the color is kept, near and far (255: all)
  float sky_color[3] = {0, 0, 0};    // 0..255: the sky behind the level (the editor's clear color)
  // the lights of the models (the mood's light-group 0, as merc lights them): the ambient light,
  // and up to 3 directions of the sun (toward it, in the world) with their colors times their
  // intensity at that hour
  float light_ambient[3] = {0, 0, 0};
  float light_dir[3][3] = {};
  float light_color[3][3] = {};
};

struct MoodSettings {
  float hour = 12.f;        // 0..24
  bool level_lights = true;  // palettes 5..7 driven like the city's (lamps on at night)
  float fog_mult = 1.f;     // the level's (level-info fog-mult): stretches the fog's end
};

GameMood game_mood(const MoodSettings& s);

}  // namespace ogle
