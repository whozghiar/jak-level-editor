//! The game's lighting at an hour of the day and in a weather (Jak 3), as its mood code computes it
//! for a level (goal_src/jak3/engine/gfx/mood: init-weather! mixing the weathers' tables of
//! mood-tables.gc, update-mood-exterior, update-mood-direction, update-mood-itimes): the weights of
//! the 8 palettes every vertex of the level mixes, the fog, the sky, its sun and its clouds.
//!
//!   palette 0       the ambient light
//!   palettes 1..4   the sun from 4 directions (dawn to dusk)
//!   palettes 5..7   the level's own lights: in the city, 5 its lamps (on at night), 6 lights always
//!                   on, 7 lights that flicker (update-mood-ctyindb...)
//!
//! The weather, as the game's *mood-control* has it: `cloud` 0 a clear sky, 0.5 overcast, 1 a
//! storm; `fog` 0 haze, 0.5 clear, 1 rain (smog under a clear sky). The game moves them slowly
//! on its own; here they stay where they are set.
//!
//! The engine computes, the interface draws: it asks `game_mood` for the settings of the Game
//! rendering window (GameWeather, from src/app/game_window.cpp) and draws the sky, the fog, the
//! rain and the level's palettes with what it answers.

// the game's tables, indexed as mood.gc indexes them
#![allow(clippy::needless_range_loop)]

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GameMood {
  /// per palette: its color (r, g, b) and weight (w), as the game's mood-context times
  pub times: [[f32; 4]; 8],
  /// the same, packed as the renderers take them (mood-context itimes: 64 is 1.0)
  pub itimes: [[i32; 4]; 4],
  /// 0..255
  pub fog_color: [f32; 3],
  /// game units (4096 per meter)
  pub fog_start: f32,
  pub fog_end: f32,
  /// how much of the color is kept, near and far (255: all)
  pub fog_max: f32,
  pub fog_min: f32,
  /// 0..255: the sky behind the level, high up
  pub sky_color: [f32; 3],
  /// the lights of the models (the mood's light-group 0, as merc lights them): the ambient light,
  /// and up to 3 directions of the sun (toward it, in the world) with their colors times their
  /// intensity at that hour
  pub light_ambient: [f32; 3],
  pub light_dir: [[f32; 3]; 3],
  pub light_color: [[f32; 3]; 3],
  /// the sky (sky-tng): toward the sun and the moon (unit vectors), their colors (sky-work
  /// sun0-color, moon-color), how much of the sky the clouds cover (the weather's mood-clouds: the
  /// part of their texture shown, min..max; 1, 1: none) and how dark they are (0 white .. 1 a
  /// storm)
  pub sun_dir: [f32; 3],
  pub moon_dir: [f32; 3],
  pub sun_color: [f32; 3],
  pub moon_color: [f32; 3],
  pub cloud_min: f32,
  pub cloud_max: f32,
  pub cloud_dark: f32,
  /// 0..1: the weather's haze toward the horizon (fog 0 haze, 1 rain)
  pub haze: f32,
  /// a lightning flash now (MoodSettings::flash)
  pub flash: f32,
}

impl Default for GameMood {
  fn default() -> Self {
    Self {
      times: [[0.0; 4]; 8],
      itimes: [[0; 4]; 4],
      fog_color: [0.0; 3],
      fog_start: 0.0,
      fog_end: 0.0,
      fog_max: 255.0,
      fog_min: 0.0,
      sky_color: [0.0; 3],
      light_ambient: [0.0; 3],
      light_dir: [[0.0; 3]; 3],
      light_color: [[0.0; 3]; 3],
      sun_dir: [0.0, 1.0, 0.0],
      moon_dir: [0.0, -1.0, 0.0],
      sun_color: [1.0, 1.0, 1.0],
      moon_color: [0.1, 0.4, 0.8],
      cloud_min: 1.0,
      cloud_max: 1.0,
      cloud_dark: 0.0,
      haze: 0.0,
      flash: 0.0,
    }
  }
}

impl GameMood {
  /// How much of the fog's color what is `distance` meters from the camera takes (the effects lost
  /// in the fog far away, App::fog_effects): 0 near, (fog_max - fog_min) / 255 at the fog's end.
  /// An additive effect is dimmed by as much, the others mixed toward fog_color / 255.
  pub fn fog_amount(&self, distance: f32) -> f32 {
    let start = self.fog_start / 4096.0;
    let end = (self.fog_end / 4096.0).max(start + 1.0);
    ((distance - start) / (end - start)).clamp(0.0, 1.0) * (self.fog_max - self.fog_min) / 255.0
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoodSettings {
  /// 0..24
  pub hour: f32,
  /// the weather: 0 clear, 0.5 overcast, 1 a storm (the game's start: 0.25)
  pub cloud: f32,
  /// 0 haze, 0.5 clear, 1 rain or smog
  pub fog: f32,
  /// 0..1: rain falling (a level's setting in the game), drawn by the editor
  pub rain: f32,
  /// 0..1: a lightning flash of a storm, now
  pub flash: f32,
  /// palettes 5..7 driven like the city's (lamps on at night)
  pub level_lights: bool,
  /// the level's (level-info fog-mult): stretches the fog's end
  pub fog_mult: f32,
}

impl Default for MoodSettings {
  fn default() -> Self {
    Self {
      hour: 12.0,
      cloud: 0.25,
      fog: 0.5,
      rain: 0.0,
      flash: 0.0,
      level_lights: true,
      fog_mult: 1.0,
    }
  }
}

/// The weathers the game has, as the Game rendering window offers them: cloud, fog, rain
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct WeatherPreset {
  /// its name's key: weather.<id>
  pub id: &'static str,
  pub cloud: f32,
  pub fog: f32,
  pub rain: f32,
}

const fn preset(id: &'static str, cloud: f32, fog: f32, rain: f32) -> WeatherPreset {
  WeatherPreset {
    id,
    cloud,
    fog,
    rain,
  }
}

const K_PRESETS: [WeatherPreset; 10] = [
  preset("clear", 0.0, 0.5, 0.0),
  preset("few_clouds", 0.25, 0.5, 0.0),
  preset("haze", 0.0, 0.0, 0.0),
  preset("smog", 0.0, 1.0, 0.0),
  preset("overcast", 0.5, 0.5, 0.0),
  preset("overcast_fog", 0.5, 0.0, 0.0),
  preset("rain", 0.5, 1.0, 0.6),
  preset("storm", 1.0, 0.5, 0.0),
  preset("storm_rain", 1.0, 1.0, 1.0),
  preset("storm_fog", 1.0, 0.0, 0.0),
];

pub fn weather_presets() -> &'static [WeatherPreset] {
  &K_PRESETS
}

// ------------------------------------------------------------------------------------------------
// the game's tables
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct FogSnap {
  /// 0..255
  color: [f32; 3],
  start: f32,
  end: f32,
  max: f32,
  min: f32,
}

#[derive(Clone, Copy, Default)]
struct ColorSnap {
  /// the sun's
  lgt: [f32; 3],
  /// the ambient light's
  amb: [f32; 4],
}

const fn f(r: f32, g: f32, b: f32, start: f32, end: f32, max: f32, min: f32) -> FogSnap {
  FogSnap {
    color: [r, g, b],
    start,
    end,
    max,
    min,
  }
}

const fn c(l0: f32, l1: f32, l2: f32, a0: f32, a1: f32, a2: f32) -> ColorSnap {
  ColorSnap {
    lgt: [l0, l1, l2],
    amb: [a0, a1, a2, 1.0],
  }
}

// mood-tables.gc: the fogs of the weathers (*mood-control* fogs: no cloud haze, clear, smog;
// overcast fog, clear, rain; storm fog, clear, rain), 8 snapshots each: color, start, end, max, min
#[rustfmt::skip]
const K_FOGS: [[FogSnap; 8]; 9] = [
  [
    f(160.0, 150.0, 200.0, 131072.0, 1843200.0, 255.0, 128.0),
    f(150.0, 165.0, 220.0, 131072.0, 1843200.0, 255.0, 128.0),
    f(128.0, 180.0, 243.0, 131072.0, 1843200.0, 255.0, 128.0),
    f(150.0, 165.0, 220.0, 262144.0, 1843200.0, 255.0, 128.0),
    f(160.0, 150.0, 200.0, 131072.0, 1843200.0, 255.0, 128.0),
    f(16.0, 32.0, 100.0, 131072.0, 1843200.0, 255.0, 128.0),
    f(0.0, 24.0, 64.0, 131072.0, 1843200.0, 255.0, 128.0),
    f(0.0, 44.7999, 57.5999, 131072.0, 1843200.0, 255.0, 128.0),
  ],
  [
    f(160.0, 150.0, 200.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(150.0, 165.0, 220.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(128.0, 180.0, 243.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(150.0, 165.0, 220.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(160.0, 150.0, 200.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(12.7999, 25.5999, 80.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(0.0, 19.1999, 51.1999, 131072.0, 4096000.0, 255.0, 162.0),
    f(0.0, 44.7999, 57.5999, 131072.0, 4096000.0, 255.0, 162.0),
  ],
  [
    f(108.75, 87.5, 61.5, 65536.0, 1572864.0, 255.0, 96.0),
    f(105.625, 100.3437, 84.5, 65536.0, 1572864.0, 255.0, 96.0),
    f(105.625, 104.9648, 103.6445, 65536.0, 1572864.0, 255.0, 96.0),
    f(105.625, 100.3437, 84.5, 65536.0, 1572864.0, 255.0, 96.0),
    f(108.75, 90.0, 61.5, 65536.0, 1572864.0, 255.0, 96.0),
    f(54.0, 62.0, 108.0, 65536.0, 1572864.0, 255.0, 96.0),
    f(18.5625, 24.75, 37.125, 65536.0, 1572864.0, 255.0, 96.0),
    f(0.0, 38.5, 49.5, 65536.0, 1572864.0, 255.0, 96.0),
  ],
  [
    f(84.5625, 83.875, 80.7124, 0.0, 1024000.0, 196.0, 4.0),
    f(103.7812, 116.2734, 117.2343, 0.0, 1024000.0, 196.0, 4.0),
    f(108.9843, 116.25, 124.2421, 0.0, 1024000.0, 196.0, 4.0),
    f(103.7812, 116.2734, 117.2343, 0.0, 1024000.0, 196.0, 4.0),
    f(84.082, 84.7656, 79.5703, 0.0, 1024000.0, 196.0, 4.0),
    f(34.0, 40.0, 61.0, 196608.0, 1024000.0, 196.0, 4.0),
    f(16.7343, 18.7031, 26.5781, 0.0, 1024000.0, 196.0, 8.0),
    f(18.75, 28.75, 33.75, 0.0, 1024000.0, 196.0, 8.0),
  ],
  [
    f(136.0, 130.0, 160.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(134.0, 143.0, 176.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(125.4, 156.5999, 194.3999, 131072.0, 4096000.0, 255.0, 162.0),
    f(134.0, 143.0, 176.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(136.0, 130.0, 160.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(23.68, 31.3599, 64.0, 131072.0, 4096000.0, 255.0, 162.0),
    f(10.2399, 21.7599, 40.9599, 131072.0, 4096000.0, 255.0, 162.0),
    f(11.5199, 38.3999, 46.0799, 131072.0, 4096000.0, 255.0, 162.0),
  ],
  [
    f(69.4938, 84.6425, 100.0708, 0.0, 1024000.0, 255.0, 16.0),
    f(77.0012, 108.5625, 126.4826, 0.0, 1024000.0, 255.0, 16.0),
    f(82.6313, 108.699, 132.716, 0.0, 1024000.0, 255.0, 16.0),
    f(77.0012, 108.5625, 126.4826, 0.0, 1024000.0, 255.0, 16.0),
    f(68.186, 85.1003, 99.9291, 0.0, 1024000.0, 255.0, 16.0),
    f(33.2822, 41.2822, 67.5448, 0.0, 1024000.0, 255.0, 16.0),
    f(2.0, 18.25, 26.5, 0.0, 1024000.0, 255.0, 16.0),
    f(2.4022, 24.8554, 29.6991, 0.0, 1024000.0, 255.0, 16.0),
  ],
  [
    f(51.4475, 60.0221, 72.8841, 0.0, 1024000.0, 255.0, 16.0),
    f(37.0013, 69.5625, 75.4827, 0.0, 1024000.0, 255.0, 16.0),
    f(46.6313, 70.6991, 76.7161, 0.0, 1024000.0, 255.0, 16.0),
    f(37.0013, 69.5625, 75.4827, 0.0, 1024000.0, 255.0, 16.0),
    f(56.1861, 62.1004, 73.9291, 0.0, 1024000.0, 255.0, 16.0),
    f(29.2823, 29.2823, 40.5449, 0.0, 1024000.0, 255.0, 16.0),
    f(2.0, 10.25, 18.5, 0.0, 1024000.0, 255.0, 16.0),
    f(2.4023, 15.8554, 19.6992, 0.0, 1024000.0, 255.0, 16.0),
  ],
  [
    f(115.0, 115.0, 110.0, 32768.0, 2048000.0, 255.0, 162.0),
    f(122.5, 127.5, 127.5, 32768.0, 2048000.0, 255.0, 162.0),
    f(127.5, 127.5, 127.5, 32768.0, 2048000.0, 255.0, 162.0),
    f(122.5, 127.5, 127.5, 32768.0, 2048000.0, 255.0, 162.0),
    f(110.0, 110.0, 105.0, 32768.0, 2048000.0, 255.0, 162.0),
    f(90.0, 85.0, 90.0, 32768.0, 2048000.0, 255.0, 162.0),
    f(0.0, 12.0, 32.0, 32768.0, 2048000.0, 255.0, 162.0),
    f(0.0, 28.0, 36.0, 32768.0, 2048000.0, 255.0, 162.0),
  ],
  [
    f(51.4475, 60.0221, 72.8841, 0.0, 1024000.0, 255.0, 16.0),
    f(37.0013, 69.5625, 75.4827, 0.0, 1024000.0, 255.0, 16.0),
    f(46.6313, 70.6991, 76.7161, 0.0, 1024000.0, 255.0, 16.0),
    f(37.0013, 69.5625, 75.4827, 0.0, 1024000.0, 255.0, 16.0),
    f(56.1861, 62.1004, 73.9291, 0.0, 1024000.0, 255.0, 16.0),
    f(29.2823, 29.2823, 40.5449, 0.0, 1024000.0, 255.0, 16.0),
    f(2.0, 10.25, 18.5, 0.0, 1024000.0, 255.0, 16.0),
    f(2.4023, 15.8554, 19.6992, 0.0, 1024000.0, 255.0, 16.0),
  ],
];

// the colors of the weathers (no cloud, overcast, storm): the sun's and the ambient light's
#[rustfmt::skip]
const K_COLORS: [[ColorSnap; 8]; 3] = [
  [
    c(1.5092, 1.2699, 0.0, 0.498, 0.4406, 0.5777),
    c(1.6168, 1.4675, 1.0976, 0.4199, 0.5195, 0.5976),
    c(1.8412, 1.685, 1.4374, 0.4199, 0.5195, 0.5976),
    c(1.6168, 1.4675, 1.0976, 0.4199, 0.5195, 0.5976),
    c(1.6464, 1.2601, 0.0, 0.3984, 0.4478, 0.5976),
    c(0.3312, 0.149, 0.5, 0.2788, 0.3593, 0.498),
    c(0.1097, 0.384, 0.8232, 0.2695, 0.3143, 0.4492),
    c(0.1, 0.5749, 0.4, 0.239, 0.3386, 0.3984),
  ],
  [
    c(0.3772, 0.3473, 0.1886, 0.5378, 0.4605, 0.5976),
    c(0.4041, 0.3855, 0.3392, 0.5087, 0.5639, 0.642),
    c(0.4602, 0.4407, 0.4098, 0.5087, 0.5639, 0.642),
    c(0.4041, 0.3855, 0.3392, 0.5087, 0.5639, 0.642),
    c(0.4115, 0.3633, 0.2057, 0.4979, 0.4975, 0.6473),
    c(0.1038, 0.0811, 0.125, 0.3883, 0.414, 0.5527),
    c(0.1166, 0.1508, 0.2057, 0.3593, 0.3592, 0.4941),
    c(0.0843, 0.1437, 0.1218, 0.3186, 0.3784, 0.4382),
  ],
  [
    c(0.1509, 0.1449, 0.1131, 0.3714, 0.3964, 0.4877),
    c(0.1616, 0.1579, 0.1486, 0.3684, 0.4604, 0.5124),
    c(0.1841, 0.1802, 0.174, 0.3684, 0.4604, 0.5124),
    c(0.1616, 0.1579, 0.1486, 0.3684, 0.4604, 0.5124),
    c(0.1646, 0.1549, 0.1234, 0.3648, 0.4145, 0.5142),
    c(0.0457, 0.0412, 0.0499, 0.2951, 0.3406, 0.4329),
    c(0.0644, 0.0713, 0.0823, 0.3593, 0.3592, 0.4941),
    c(0.0456, 0.0574, 0.0531, 0.3186, 0.3784, 0.4382),
  ],
];

// how much of the sun comes from each of its 4 directions per hour, per weather (the moon at night)
#[rustfmt::skip]
const K_CHANNEL_GROUPS: [[[f32; 24]; 4]; 3] = [
  [
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.2, 0.4, 1.0, 0.5, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.8, 0.6, 0.4, 0.2, 0.0, 0.0, 0.0, 0.0, 0.5, 0.75, 1.0, 0.75, 0.5, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.75, 0.82, 0.88, 0.94, 1.0],
    [0.2, 0.4, 0.6, 0.8, 1.0, 0.8, 0.6, 0.0, 0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 0.84, 0.67, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.16, 0.33, 0.5, 1.0, 0.25, 0.18, 0.12, 0.06, 0.0],
  ],
  [
    [0.0; 24],
    [0.5, 0.42, 0.34, 0.25, 0.17, 0.08, 0.0, 1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.42, 0.34, 0.25, 0.17, 0.08, 0.0, 1.0, 0.9, 0.8, 0.7, 0.6],
    [0.5, 0.58, 0.66, 0.75, 0.83, 0.92, 1.0, 0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.58, 0.66, 0.75, 0.83, 0.92, 1.0, 0.0, 0.1, 0.2, 0.3, 0.4],
    [0.0; 24],
  ],
  [
    [0.0; 24],
    [0.5; 24],
    [0.5; 24],
    [0.0; 24],
  ],
];

// *clouds-000* .. *clouds-1000*: the sky's clouds (min, max of their texture shown)
const K_CLOUDS: [[f32; 2]; 9] = [
  [1.0, 1.0],     // 000
  [0.333, 1.0],   // 125
  [0.333, 0.555], // 250
  [0.167, 0.605], // 375
  [0.0, 0.655],   // 500
  [0.0, 0.605],   // 625
  [0.0, 0.555],   // 750
  [0.0, 0.505],   // 875
  [0.0, 0.455],   // 1000
];

// *mood-sky-table*: the sky's color at the 8 snapshots of the day
const K_SKY: [[f32; 3]; 8] = [
  [83.0, 103.0, 190.0],
  [86.0, 108.0, 195.0],
  [89.0, 113.0, 200.0],
  [86.0, 108.0, 195.0],
  [83.0, 103.0, 190.0],
  [38.0, 48.0, 96.0],
  [0.0, 0.0, 8.0],
  [0.0, 29.0, 33.0],
];

// *mood-interp-table*, per hour: the two snapshots mixed, and how far from the first at the hour's
// start and end
struct Hour {
  s1: usize,
  s2: usize,
  start: f32,
  end: f32,
}

const fn hr(s1: usize, s2: usize, start: f32, end: f32) -> Hour {
  Hour { s1, s2, start, end }
}

#[rustfmt::skip]
const K_HOURS: [Hour; 24] = [
  hr(6, 7, 0.2, 0.4),   hr(6, 7, 0.4, 0.6),   hr(6, 7, 0.6, 0.8),   hr(6, 7, 0.8, 0.99),
  hr(0, 7, 0.99, 0.8),  hr(0, 7, 0.8, 0.6),   hr(0, 7, 0.6, 0.0),   hr(0, 1, 0.0, 0.5),
  hr(0, 1, 0.5, 0.75),  hr(0, 1, 0.75, 0.99), hr(2, 1, 0.99, 0.5),  hr(2, 1, 0.5, 0.0),
  hr(2, 3, 0.0, 0.5),   hr(2, 3, 0.5, 0.99),  hr(4, 3, 0.99, 0.84), hr(4, 3, 0.84, 0.66),
  hr(4, 3, 0.66, 0.5),  hr(4, 3, 0.5, 0.0),   hr(4, 5, 0.0, 0.99),  hr(6, 5, 0.99, 0.75),
  hr(6, 5, 0.75, 0.5),  hr(6, 5, 0.5, 0.25),  hr(6, 5, 0.25, 0.0),  hr(6, 7, 0.0, 0.2),
];

// the 4 directions of the sun (*mood-direction-table*), toward it
const K_DIRECTIONS: [[f32; 3]; 4] = [
  [0.906, 0.397, 0.143],
  [0.5, 0.814, 0.296],
  [-0.5, 0.814, 0.296],
  [-0.906, 0.397, 0.143],
];

fn lerp(a: f32, b: f32, t: f32) -> f32 {
  a + (b - a) * t
}

// init-weather!: the weathers' tables mixed for a cloud and a fog (apply-mood-clouds-and-fog,
// apply-mood-colors, apply-mood-channels, adjust-num-clouds). A table whose weight is 0 is never
// read (cloud or fog 1 would read past the last).
#[derive(Default)]
struct Weather {
  fog: [FogSnap; 8],
  color: [ColorSnap; 8],
  channels: [[f32; 24]; 4],
  cloud_min: f32,
  cloud_max: f32,
}

fn mix_weather(cloud: f32, fog: f32) -> Weather {
  let mut w = Weather::default();
  let cloud = cloud.clamp(0.0, 1.0);
  let fog = fog.clamp(0.0, 1.0);
  let (wc, wf) = (2.0 * cloud, 2.0 * fog);
  let (ic, ifg) = (wc as usize, wf as usize);
  let fc = wc - ic as f32;
  let cloud_interp = if ic == 0 {
    if fc < 0.5 { 0.0 } else { 2.0 * (fc - 0.5) }
  } else {
    fc
  };
  let fog_interp = wf - ifg as f32;
  let index = [
    3 * ic + ifg,
    3 * ic + ifg + 1,
    3 * (ic + 1) + ifg,
    3 * (ic + 1) + ifg + 1,
  ];
  let weight = [
    (1.0 - fog_interp) * (1.0 - cloud_interp),
    fog_interp * (1.0 - cloud_interp),
    (1.0 - fog_interp) * cloud_interp,
    fog_interp * cloud_interp,
  ];
  for k in 0..4 {
    if weight[k] == 0.0 || index[k] > 8 {
      continue;
    }
    for s in 0..8 {
      let f = &K_FOGS[index[k]][s];
      let o = &mut w.fog[s];
      for c in 0..3 {
        o.color[c] += f.color[c] * weight[k];
      }
      o.start += f.start * weight[k];
      o.end += f.end * weight[k];
      o.max += f.max * weight[k];
      o.min += f.min * weight[k];
    }
  }
  // the colors: no cloud, overcast, storm
  let color_weight = [1.0 - cloud_interp, cloud_interp];
  for k in 0..2 {
    if color_weight[k] == 0.0 || ic + k > 2 {
      continue;
    }
    for s in 0..8 {
      for c in 0..4 {
        if c < 3 {
          w.color[s].lgt[c] += K_COLORS[ic + k][s].lgt[c] * color_weight[k];
        }
        w.color[s].amb[c] += K_COLORS[ic + k][s].amb[c] * color_weight[k];
      }
    }
  }
  // the sun's directions: the clear weather's up to overcast, then overcast's, then the storm's
  let (ci, channel_interp) = match ic {
    0 => (0, 0.0),
    2 => (2, 0.0),
    _ if fc < 0.5 => (0, 2.0 * fc),
    _ => (1, 2.0 * (fc - 0.5)),
  };
  let channel_weight = [1.0 - channel_interp, channel_interp];
  for k in 0..2 {
    if channel_weight[k] == 0.0 || ci + k > 2 {
      continue;
    }
    for d in 0..4 {
      for h in 0..24 {
        w.channels[d][h] += K_CHANNEL_GROUPS[ci + k][d][h] * channel_weight[k];
      }
    }
  }
  // the clouds: 9 steps of cloud
  let fcl = 8.0 * cloud;
  let i = fcl as usize;
  let t = fcl - i as f32;
  let next = |c: usize| {
    if t != 0.0 && i < 8 {
      K_CLOUDS[i + 1][c] * t
    } else {
      0.0
    }
  };
  w.cloud_min = K_CLOUDS[i][0] * (1.0 - t) + next(0);
  w.cloud_max = K_CLOUDS[i][1] * (1.0 - t) + next(1);
  w
}

fn normalize3(v: &mut [f32; 3]) {
  let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
  if l > 1e-6 {
    for x in v.iter_mut() {
      *x /= l;
    }
  }
}

pub fn game_mood(s: &MoodSettings) -> GameMood {
  let mut m = GameMood::default();
  let w = mix_weather(s.cloud, s.fog);
  // clear-mood-context: every palette white, off
  m.times = [[1.0, 1.0, 1.0, 0.0]; 8];

  // update-mood-exterior: the two snapshots of the hour, mixed
  let hour = ((s.hour % 24.0) + 24.0) % 24.0;
  let h = (hour as usize).min(23);
  let frac = hour - h as f32;
  let e = &K_HOURS[h];
  let t = (1.0 - frac) * e.start + frac * e.end;
  let (a, b) = (&w.color[e.s1], &w.color[e.s2]);
  for c in 0..4 {
    m.times[0][c] = lerp(a.amb[c], b.amb[c], t);
  }
  for k in 1..=4 {
    for c in 0..3 {
      m.times[k][c] = lerp(a.lgt[c], b.lgt[c], t);
    }
    m.times[k][3] = 0.0; // the sun colors' w
  }
  let (fa, fb) = (&w.fog[e.s1], &w.fog[e.s2]);
  for c in 0..3 {
    m.fog_color[c] = lerp(fa.color[c], fb.color[c], t);
    m.sky_color[c] = lerp(K_SKY[e.s1][c], K_SKY[e.s2][c], t);
  }
  m.fog_start = lerp(fa.start, fb.start, t);
  m.fog_end = lerp(fa.end, fb.end, t) * s.fog_mult;
  m.fog_max = lerp(fa.max, fb.max, t);
  m.fog_min = lerp(fa.min, fb.min, t);

  // a lightning flash: the light from above, brighter (the sun's palette of lightning-index 1)
  if s.flash > 0.0 {
    for c in 0..3 {
      m.times[2][c] += s.flash;
    }
  }

  // update-mood-direction: the sun's directions lit at that hour, and the light-group 0 the models
  // take (their light-index is 0 unless said otherwise): the directions lit, 3 at most
  for c in 0..3 {
    m.light_ambient[c] = m.times[0][c];
  }
  let mut lights = 0;
  for c in 0..4 {
    let mut wgt = (1.0 - frac) * w.channels[c][h] + frac * w.channels[c][(h + 1) % 24];
    if c == 1 && s.flash > 0.0 {
      wgt = wgt.max(s.flash);
    }
    if wgt == 0.0 {
      continue;
    }
    m.times[c + 1][3] = wgt;
    if lights < 3 {
      for k in 0..3 {
        m.light_dir[lights][k] = K_DIRECTIONS[c][k];
        m.light_color[lights][k] = m.times[c + 1][k] * wgt;
      }
      lights += 1;
    }
  }

  // the level's lights, like the city's (update-mood-ctyindb): its lamps at night, the lights
  // always on, and the flickering ones (between 0.8 and 1.0)
  if s.level_lights {
    m.times[5][3] = if hour >= 18.0 || hour <= 6.0 {
      1.0
    } else {
      0.0
    };
    m.times[6][3] = 1.0;
    m.times[7][3] = 0.9;
  }

  // update-mood-itimes: rgb times w, to fixed point (64 is 1.0), two palettes per quadword
  for k in 0..8 {
    let v = m.times[k];
    let q: [i32; 4] =
      std::array::from_fn(|c| (((if c < 3 { v[c] * v[3] } else { v[3] }) * 4096.0) as i32) >> 6);
    let out = &mut m.itimes[k / 2];
    let o = (k % 2) * 2;
    out[o] = ((q[0] & 0xffff) as u32 | ((q[1] & 0xffff) as u32) << 16) as i32;
    out[o + 1] = ((q[2] & 0xffff) as u32 | ((q[3] & 0xffff) as u32) << 16) as i32;
  }

  // the sky: the sun rises in the east (+x, where the morning's light comes from) at 6, is highest
  // at noon and sets in the west at 18; the moon goes the same way through the night
  // ponytail: a plain arc from the mood's directions, not sky-tng's orbit of the sun and the moon
  let pi = std::f32::consts::PI;
  let (day, night) = ((hour - 6.0) / 12.0 * pi, (hour - 18.0) / 12.0 * pi);
  m.sun_dir = [day.cos(), day.sin() * 0.92, 0.3];
  normalize3(&mut m.sun_dir);
  m.moon_dir = [night.cos(), night.sin() * 0.92, 0.3];
  normalize3(&mut m.moon_dir);
  // update-colors-for-time: the sun drawn with the clear weather's light of the hour
  for c in 0..3 {
    m.sun_color[c] = lerp(K_COLORS[0][e.s1].lgt[c], K_COLORS[0][e.s2].lgt[c], t);
    m.moon_color[c] = K_COLORS[0][6].lgt[c];
  }
  m.cloud_min = w.cloud_min;
  m.cloud_max = w.cloud_max;
  m.cloud_dark = (s.cloud * 1.25 - 0.25).clamp(0.0, 1.0);
  m.haze = ((s.fog - 0.5).abs() * 2.0).clamp(0.0, 1.0);
  m.flash = s.flash;
  m
}

// ------------------------------------------------------------------------------------------------
// the Game rendering window's hour and weather (src/app/game_window.cpp)
// ------------------------------------------------------------------------------------------------

fn hash01(mut x: u32) -> f32 {
  x ^= x >> 16;
  x = x.wrapping_mul(0x7feb352d);
  x ^= x >> 15;
  x = x.wrapping_mul(0x846ca68b);
  x ^= x >> 16;
  (x & 0xffffff) as f32 / 0x1000000 as f32
}

// the shape of a lightning flash over its frames (mood.gc *flash0*...: a few bright blinks)
const K_FLASH: [f32; 12] = [1.0, 0.6, 0.2, 0.0, 0.0, 0.8, 1.0, 0.7, 0.4, 0.2, 0.1, 0.0];

/// The game view's hour and weather as the editor keeps them (App's): the mood's settings, the
/// clouds' clock, the hour passing by itself, a storm's lightning.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameWeather {
  pub mood: MoodSettings,
  /// seconds: the clouds drift (GameView::time)
  pub time: f32,
  /// the hour goes on by itself
  pub time_passes: bool,
  /// minutes of the game per second
  pub time_speed: f32,
  /// a storm's flashes
  pub lightning: bool,
  #[serde(skip)]
  lightning_wait: f32,
  #[serde(skip)]
  lightning_time: f32,
  #[serde(skip)]
  lightning_frame: Option<usize>,
}

impl Default for GameWeather {
  fn default() -> Self {
    Self {
      mood: MoodSettings::default(),
      time: 0.0,
      time_passes: false,
      time_speed: 20.0,
      lightning: false,
      lightning_wait: 3.0,
      lightning_time: 0.0,
      lightning_frame: None,
    }
  }
}

impl GameWeather {
  /// The weather chosen among weather_presets(), None for one of the user's own.
  pub fn preset_index(&self) -> Option<usize> {
    let m = &self.mood;
    K_PRESETS.iter().position(|p| {
      (p.cloud - m.cloud).abs() < 0.01
        && (p.fog - m.fog).abs() < 0.01
        && (p.rain - m.rain).abs() < 0.01
    })
  }

  /// A weather of weather_presets() by its id (a storm with its lightning); false when there is
  /// none of that id.
  pub fn set_weather(&mut self, id: &str) -> bool {
    let Some(p) = K_PRESETS.iter().find(|p| p.id == id) else {
      return false;
    };
    self.mood.cloud = p.cloud;
    self.mood.fog = p.fog;
    self.mood.rain = p.rain;
    self.lightning = p.cloud >= 0.99;
    true
  }

  /// `dt` seconds later: the hour passing, a storm's lightning (a flash every 5 to 10 seconds,
  /// gen-lightning-and-thunder!) while the game view is on.
  pub fn update(&mut self, dt: f32, game_view_on: bool) {
    let m = &mut self.mood;
    self.time += dt;
    if self.time_passes {
      m.hour = (m.hour + dt * self.time_speed / 60.0 + 24.0) % 24.0;
    }
    m.flash = 0.0;
    if !self.lightning || m.cloud < 0.75 || !game_view_on {
      self.lightning_wait = 3.0;
      return;
    }
    if self.lightning_frame.is_some() {
      self.lightning_time += dt;
      let frame = (self.lightning_time * 30.0) as usize;
      match K_FLASH.get(frame) {
        Some(f) => m.flash = f * 0.8,
        None => self.lightning_frame = None,
      }
      return;
    }
    self.lightning_wait -= dt;
    if self.lightning_wait <= 0.0 {
      self.lightning_frame = Some(0);
      self.lightning_time = 0.0;
      self.lightning_wait = 5.0 + 5.0 * hash01((self.time * 1000.0) as u32);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn weathers() {
    // the weathers: a storm darker than a clear sky, its fog nearer; every mix of cloud and fog read
    let clear = MoodSettings {
      cloud: 0.0,
      ..Default::default()
    };
    let storm = MoodSettings {
      cloud: 1.0,
      fog: 1.0,
      ..Default::default()
    };
    let (mc, ms) = (game_mood(&clear), game_mood(&storm));
    let mut all = true;
    for ci in 0..=8 {
      for fi in 0..=4 {
        let gm = game_mood(&MoodSettings {
          cloud: ci as f32 * 0.125,
          fog: fi as f32 * 0.25,
          ..Default::default()
        });
        all &= gm.fog_end.is_finite() && gm.fog_end > 0.0 && gm.light_ambient[0].is_finite();
      }
    }
    assert!(
      ms.times[1][0] < mc.times[1][0]
        && ms.fog_end < mc.fog_end
        && mc.cloud_min > ms.cloud_min
        && all
        && weather_presets().len() == 10,
      "weather: a storm's sun dimmer and its fog nearer than a clear sky's, every mix of the game's tables read"
    );
  }

  #[test]
  fn the_hour_and_the_presets() {
    // noon: the sun up, the lamps off; night: the sun down, the lamps on
    let noon = game_mood(&MoodSettings::default());
    let night = game_mood(&MoodSettings {
      hour: 22.0,
      ..Default::default()
    });
    assert!(noon.sun_dir[1] > 0.0 && noon.times[5][3] == 0.0);
    assert!(night.sun_dir[1] < 0.0 && night.times[5][3] == 1.0);
    // the packed palettes: 64 is 1.0 (the lamps always on, weight 1, white)
    assert_eq!(noon.itimes[3][0] & 0xffff, 64);
    let mut w = GameWeather::default();
    assert_eq!(w.preset_index(), Some(1)); // the game's start: a few clouds
    assert!(w.set_weather("storm_rain") && w.lightning && w.preset_index() == Some(8));
    assert!(!w.set_weather("snow"));
    // a storm flashes within 10 seconds
    let mut flashed = false;
    for _ in 0..(12 * 30) {
      w.update(1.0 / 30.0, true);
      flashed |= w.mood.flash > 0.0;
    }
    assert!(flashed);
    assert!(noon.fog_amount(0.0) == 0.0 && noon.fog_amount(1e6) > 0.0);
  }
}
