#pragma once

// The game's lighting at an hour of the day and in a weather (Jak 3), as its mood code computes it
// for a level (goal_src/jak3/engine/gfx/mood: init-weather! mixing the weathers' tables of
// mood-tables.gc, update-mood-exterior, update-mood-direction, update-mood-itimes): the weights of
// the 8 palettes every vertex of the level mixes, the fog, the sky, its sun and its clouds.
//
//   palette 0       the ambient light
//   palettes 1..4   the sun from 4 directions (dawn to dusk)
//   palettes 5..7   the level's own lights: in the city, 5 its lamps (on at night), 6 lights always
//                   on, 7 lights that flicker (update-mood-ctyindb...)
//
// The weather, as the game's *mood-control* has it: `cloud` 0 a clear sky, 0.5 overcast, 1 a
// storm; `fog` 0 haze, 0.5 clear, 1 rain (smog under a clear sky). The game moves them slowly
// on its own; here they stay where they are set.

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
  float sky_color[3] = {0, 0, 0};    // 0..255: the sky behind the level, high up
  // the lights of the models (the mood's light-group 0, as merc lights them): the ambient light,
  // and up to 3 directions of the sun (toward it, in the world) with their colors times their
  // intensity at that hour
  float light_ambient[3] = {0, 0, 0};
  float light_dir[3][3] = {};
  float light_color[3][3] = {};
  // the sky (sky-tng): toward the sun and the moon (unit vectors), their colors (sky-work
  // sun0-color, moon-color), how much of the sky the clouds cover (the weather's mood-clouds: the
  // part of their texture shown, min..max; 1, 1: none) and how dark they are (0 white .. 1 a storm)
  float sun_dir[3] = {0, 1, 0}, moon_dir[3] = {0, -1, 0};
  float sun_color[3] = {1, 1, 1}, moon_color[3] = {0.1f, 0.4f, 0.8f};
  float cloud_min = 1, cloud_max = 1;
  float cloud_dark = 0;
  float haze = 0;   // 0..1: the weather's haze toward the horizon (fog 0 haze, 1 rain)
  float flash = 0;  // a lightning flash now (MoodSettings::flash)
};

struct MoodSettings {
  float hour = 12.f;          // 0..24
  float cloud = 0.25f;        // the weather: 0 clear, 0.5 overcast, 1 a storm (the game's start: 0.25)
  float fog = 0.5f;           // 0 haze, 0.5 clear, 1 rain or smog
  float rain = 0.f;           // 0..1: rain falling (a level's setting in the game), drawn by the editor
  float flash = 0.f;          // 0..1: a lightning flash of a storm, now
  bool level_lights = true;   // palettes 5..7 driven like the city's (lamps on at night)
  float fog_mult = 1.f;       // the level's (level-info fog-mult): stretches the fog's end
};

// The weathers the game has, as the Game rendering window offers them: cloud, fog, rain
struct WeatherPreset {
  const char* id;  // its name's key: weather.<id>
  float cloud, fog, rain;
};
const WeatherPreset* weather_presets(int* count);

GameMood game_mood(const MoodSettings& s);

}  // namespace ogle
