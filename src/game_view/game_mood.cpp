#include "game_view/game_mood.h"

#include <algorithm>
#include <cmath>

namespace ogle {

namespace {

// mood-tables.gc, the clear weather (*no-cloud-mood-color-table*, *no-cloud-clear-mood-fog-table*,
// *no-cloud-mood-channel-group*, *mood-sky-table*, *mood-interp-table*): 8 snapshots of the day
struct Snapshot {
  float lgt[3];  // the sun's color
  float amb[4];  // the ambient light's
  float fog[3];  // 0..255
  float sky[3];  // 0..255
};
const Snapshot kSnapshots[8] = {
    {{1.5092f, 1.2699f, 0.f}, {0.498f, 0.4406f, 0.5777f, 1.f}, {160, 150, 200}, {83, 103, 190}},
    {{1.6168f, 1.4675f, 1.0976f}, {0.4199f, 0.5195f, 0.5976f, 1.f}, {150, 165, 220}, {86, 108, 195}},
    {{1.8412f, 1.685f, 1.4374f}, {0.4199f, 0.5195f, 0.5976f, 1.f}, {128, 180, 243}, {89, 113, 200}},
    {{1.6168f, 1.4675f, 1.0976f}, {0.4199f, 0.5195f, 0.5976f, 1.f}, {150, 165, 220}, {86, 108, 195}},
    {{1.6464f, 1.2601f, 0.f}, {0.3984f, 0.4478f, 0.5976f, 1.f}, {160, 150, 200}, {83, 103, 190}},
    {{0.3312f, 0.149f, 0.5f}, {0.2788f, 0.3593f, 0.498f, 1.f}, {12.7999f, 25.5999f, 80}, {38, 48, 96}},
    {{0.1097f, 0.384f, 0.8232f}, {0.2695f, 0.3143f, 0.4492f, 1.f}, {0, 19.1999f, 51.1999f}, {0, 0, 8}},
    {{0.1f, 0.5749f, 0.4f}, {0.239f, 0.3386f, 0.3984f, 1.f}, {0, 44.7999f, 57.5999f}, {0, 29, 33}},
};
constexpr float kFogStart = 131072.f, kFogEnd = 4096000.f, kFogMax = 255.f, kFogMin = 162.f;

// per hour: the two snapshots mixed, and how far from the first at the hour's start and end
struct Hour {
  int s1, s2;
  float start, end;
};
const Hour kHours[24] = {
    {6, 7, 0.2f, 0.4f},   {6, 7, 0.4f, 0.6f},   {6, 7, 0.6f, 0.8f},   {6, 7, 0.8f, 0.99f},
    {0, 7, 0.99f, 0.8f},  {0, 7, 0.8f, 0.6f},   {0, 7, 0.6f, 0.f},    {0, 1, 0.f, 0.5f},
    {0, 1, 0.5f, 0.75f},  {0, 1, 0.75f, 0.99f}, {2, 1, 0.99f, 0.5f},  {2, 1, 0.5f, 0.f},
    {2, 3, 0.f, 0.5f},    {2, 3, 0.5f, 0.99f},  {4, 3, 0.99f, 0.84f}, {4, 3, 0.84f, 0.66f},
    {4, 3, 0.66f, 0.5f},  {4, 3, 0.5f, 0.f},    {4, 5, 0.f, 0.99f},   {6, 5, 0.99f, 0.75f},
    {6, 5, 0.75f, 0.5f},  {6, 5, 0.5f, 0.25f},  {6, 5, 0.25f, 0.f},   {6, 7, 0.f, 0.2f},
};

// the 4 directions of the sun (*mood-direction-table*), toward it
const float kDirections[4][3] = {
    {0.906f, 0.397f, 0.143f}, {0.5f, 0.814f, 0.296f}, {-0.5f, 0.814f, 0.296f}, {-0.906f, 0.397f, 0.143f}};

// how much of the sun comes from each of its 4 directions, per hour (the moon at night)
const float kChannels[4][24] = {
    {0, 0, 0, 0, 0, 0.2f, 0.4f, 1, 0.5f, 0.25f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0},
    {0.8f, 0.6f, 0.4f, 0.2f, 0, 0, 0, 0, 0.5f, 0.75f, 1, 0.75f, 0.5f, 0.25f, 0, 0, 0, 0, 0, 0.75f, 0.82f, 0.88f,
     0.94f, 1},
    {0.2f, 0.4f, 0.6f, 0.8f, 1, 0.8f, 0.6f, 0, 0, 0, 0, 0.25f, 0.5f, 0.75f, 1, 0.84f, 0.67f, 0.5f, 0, 0, 0, 0, 0, 0},
    {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.16f, 0.33f, 0.5f, 1, 0.25f, 0.18f, 0.12f, 0.06f, 0},
};

float lerp(float a, float b, float t) { return a + (b - a) * t; }

}  // namespace

GameMood game_mood(const MoodSettings& s) {
  GameMood m;
  // clear-mood-context: every palette white, off
  for (auto& t : m.times) t = {1, 1, 1, 0};

  // update-mood-exterior: the two snapshots of the hour, mixed
  const float hour = std::fmod(std::fmod(s.hour, 24.f) + 24.f, 24.f);
  const int h = std::min((int)hour, 23);
  const float frac = hour - (float)h;
  const Hour& e = kHours[h];
  const float t = (1.f - frac) * e.start + frac * e.end;
  const Snapshot& a = kSnapshots[e.s1];
  const Snapshot& b = kSnapshots[e.s2];
  for (int c = 0; c < 4; c++) m.times[0][c] = lerp(a.amb[c], b.amb[c], t);
  for (int k = 1; k <= 4; k++) {
    for (int c = 0; c < 3; c++) m.times[k][c] = lerp(a.lgt[c], b.lgt[c], t);
    m.times[k][3] = 0;  // the sun colors' w
  }
  for (int c = 0; c < 3; c++) {
    m.fog_color[c] = lerp(a.fog[c], b.fog[c], t);
    m.sky_color[c] = lerp(a.sky[c], b.sky[c], t);
  }
  m.fog_start = kFogStart;
  m.fog_end = kFogEnd * s.fog_mult;
  m.fog_max = kFogMax;
  m.fog_min = kFogMin;

  // update-mood-direction: the sun's directions lit at that hour, and the light-group 0 the models
  // take (their light-index is 0 unless said otherwise): the directions lit, 3 at most
  for (int c = 0; c < 3; c++) m.light_ambient[c] = m.times[0][c];
  int lights = 0;
  for (int c = 0; c < 4; c++) {
    const float w = (1.f - frac) * kChannels[c][h] + frac * kChannels[c][(h + 1) % 24];
    if (w == 0.f) continue;
    m.times[c + 1][3] = w;
    if (lights < 3) {
      for (int k = 0; k < 3; k++) {
        m.light_dir[lights][k] = kDirections[c][k];
        m.light_color[lights][k] = m.times[c + 1][k] * w;
      }
      lights++;
    }
  }

  // the level's lights, like the city's (update-mood-ctyindb): its lamps at night, the lights
  // always on, and the flickering ones (between 0.8 and 1.0)
  if (s.level_lights) {
    m.times[5][3] = (hour >= 18.f || hour <= 6.f) ? 1.f : 0.f;
    m.times[6][3] = 1.f;
    m.times[7][3] = 0.9f;
  }

  // update-mood-itimes: rgb times w, to fixed point (64 is 1.0), two palettes per quadword
  for (int k = 0; k < 8; k++) {
    const auto& v = m.times[k];
    int32_t q[4];
    for (int c = 0; c < 4; c++) {
      const float x = c < 3 ? v[c] * v[3] : v[3];
      q[c] = ((int32_t)(x * 4096.f)) >> 6;
    }
    auto& out = m.itimes[k / 2];
    const int o = (k % 2) * 2;
    out[o] = (int32_t)((uint32_t)(q[0] & 0xffff) | ((uint32_t)(q[1] & 0xffff) << 16));
    out[o + 1] = (int32_t)((uint32_t)(q[2] & 0xffff) | ((uint32_t)(q[3] & 0xffff) << 16));
  }
  return m;
}

}  // namespace ogle
