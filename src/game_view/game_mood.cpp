#include "game_view/game_mood.h"

#include <algorithm>
#include <cmath>

namespace ogle {

namespace {

struct FogSnap {
  float color[3];  // 0..255
  float start, end, max, min;
};
struct ColorSnap {
  float lgt[3];  // the sun's
  float amb[4];  // the ambient light's
};

// mood-tables.gc: the fogs of the weathers (*mood-control* fogs: no cloud haze, clear, smog;
// overcast fog, clear, rain; storm fog, clear, rain), 8 snapshots each: color, start, end, max, min
const FogSnap kFogs[9][8] = {
    {
        {{160.f, 150.f, 200.f}, 131072.f, 1843200.f, 255.f, 128.f},
        {{150.f, 165.f, 220.f}, 131072.f, 1843200.f, 255.f, 128.f},
        {{128.f, 180.f, 243.f}, 131072.f, 1843200.f, 255.f, 128.f},
        {{150.f, 165.f, 220.f}, 262144.f, 1843200.f, 255.f, 128.f},
        {{160.f, 150.f, 200.f}, 131072.f, 1843200.f, 255.f, 128.f},
        {{16.f, 32.f, 100.f}, 131072.f, 1843200.f, 255.f, 128.f},
        {{0.f, 24.f, 64.f}, 131072.f, 1843200.f, 255.f, 128.f},
        {{0.f, 44.7999f, 57.5999f}, 131072.f, 1843200.f, 255.f, 128.f},
    },
    {
        {{160.f, 150.f, 200.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{150.f, 165.f, 220.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{128.f, 180.f, 243.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{150.f, 165.f, 220.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{160.f, 150.f, 200.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{12.7999f, 25.5999f, 80.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{0.f, 19.1999f, 51.1999f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{0.f, 44.7999f, 57.5999f}, 131072.f, 4096000.f, 255.f, 162.f},
    },
    {
        {{108.75f, 87.5f, 61.5f}, 65536.f, 1572864.f, 255.f, 96.f},
        {{105.625f, 100.3437f, 84.5f}, 65536.f, 1572864.f, 255.f, 96.f},
        {{105.625f, 104.9648f, 103.6445f}, 65536.f, 1572864.f, 255.f, 96.f},
        {{105.625f, 100.3437f, 84.5f}, 65536.f, 1572864.f, 255.f, 96.f},
        {{108.75f, 90.f, 61.5f}, 65536.f, 1572864.f, 255.f, 96.f},
        {{54.f, 62.f, 108.f}, 65536.f, 1572864.f, 255.f, 96.f},
        {{18.5625f, 24.75f, 37.125f}, 65536.f, 1572864.f, 255.f, 96.f},
        {{0.f, 38.5f, 49.5f}, 65536.f, 1572864.f, 255.f, 96.f},
    },
    {
        {{84.5625f, 83.875f, 80.7124f}, 0.f, 1024000.f, 196.f, 4.f},
        {{103.7812f, 116.2734f, 117.2343f}, 0.f, 1024000.f, 196.f, 4.f},
        {{108.9843f, 116.25f, 124.2421f}, 0.f, 1024000.f, 196.f, 4.f},
        {{103.7812f, 116.2734f, 117.2343f}, 0.f, 1024000.f, 196.f, 4.f},
        {{84.082f, 84.7656f, 79.5703f}, 0.f, 1024000.f, 196.f, 4.f},
        {{34.f, 40.f, 61.f}, 196608.f, 1024000.f, 196.f, 4.f},
        {{16.7343f, 18.7031f, 26.5781f}, 0.f, 1024000.f, 196.f, 8.f},
        {{18.75f, 28.75f, 33.75f}, 0.f, 1024000.f, 196.f, 8.f},
    },
    {
        {{136.f, 130.f, 160.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{134.f, 143.f, 176.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{125.4f, 156.5999f, 194.3999f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{134.f, 143.f, 176.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{136.f, 130.f, 160.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{23.68f, 31.3599f, 64.f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{10.2399f, 21.7599f, 40.9599f}, 131072.f, 4096000.f, 255.f, 162.f},
        {{11.5199f, 38.3999f, 46.0799f}, 131072.f, 4096000.f, 255.f, 162.f},
    },
    {
        {{69.4938f, 84.6425f, 100.0708f}, 0.f, 1024000.f, 255.f, 16.f},
        {{77.0012f, 108.5625f, 126.4826f}, 0.f, 1024000.f, 255.f, 16.f},
        {{82.6313f, 108.699f, 132.716f}, 0.f, 1024000.f, 255.f, 16.f},
        {{77.0012f, 108.5625f, 126.4826f}, 0.f, 1024000.f, 255.f, 16.f},
        {{68.186f, 85.1003f, 99.9291f}, 0.f, 1024000.f, 255.f, 16.f},
        {{33.2822f, 41.2822f, 67.5448f}, 0.f, 1024000.f, 255.f, 16.f},
        {{2.f, 18.25f, 26.5f}, 0.f, 1024000.f, 255.f, 16.f},
        {{2.4022f, 24.8554f, 29.6991f}, 0.f, 1024000.f, 255.f, 16.f},
    },
    {
        {{51.4475f, 60.0221f, 72.8841f}, 0.f, 1024000.f, 255.f, 16.f},
        {{37.0013f, 69.5625f, 75.4827f}, 0.f, 1024000.f, 255.f, 16.f},
        {{46.6313f, 70.6991f, 76.7161f}, 0.f, 1024000.f, 255.f, 16.f},
        {{37.0013f, 69.5625f, 75.4827f}, 0.f, 1024000.f, 255.f, 16.f},
        {{56.1861f, 62.1004f, 73.9291f}, 0.f, 1024000.f, 255.f, 16.f},
        {{29.2823f, 29.2823f, 40.5449f}, 0.f, 1024000.f, 255.f, 16.f},
        {{2.f, 10.25f, 18.5f}, 0.f, 1024000.f, 255.f, 16.f},
        {{2.4023f, 15.8554f, 19.6992f}, 0.f, 1024000.f, 255.f, 16.f},
    },
    {
        {{115.f, 115.f, 110.f}, 32768.f, 2048000.f, 255.f, 162.f},
        {{122.5f, 127.5f, 127.5f}, 32768.f, 2048000.f, 255.f, 162.f},
        {{127.5f, 127.5f, 127.5f}, 32768.f, 2048000.f, 255.f, 162.f},
        {{122.5f, 127.5f, 127.5f}, 32768.f, 2048000.f, 255.f, 162.f},
        {{110.f, 110.f, 105.f}, 32768.f, 2048000.f, 255.f, 162.f},
        {{90.f, 85.f, 90.f}, 32768.f, 2048000.f, 255.f, 162.f},
        {{0.f, 12.f, 32.f}, 32768.f, 2048000.f, 255.f, 162.f},
        {{0.f, 28.f, 36.f}, 32768.f, 2048000.f, 255.f, 162.f},
    },
    {
        {{51.4475f, 60.0221f, 72.8841f}, 0.f, 1024000.f, 255.f, 16.f},
        {{37.0013f, 69.5625f, 75.4827f}, 0.f, 1024000.f, 255.f, 16.f},
        {{46.6313f, 70.6991f, 76.7161f}, 0.f, 1024000.f, 255.f, 16.f},
        {{37.0013f, 69.5625f, 75.4827f}, 0.f, 1024000.f, 255.f, 16.f},
        {{56.1861f, 62.1004f, 73.9291f}, 0.f, 1024000.f, 255.f, 16.f},
        {{29.2823f, 29.2823f, 40.5449f}, 0.f, 1024000.f, 255.f, 16.f},
        {{2.f, 10.25f, 18.5f}, 0.f, 1024000.f, 255.f, 16.f},
        {{2.4023f, 15.8554f, 19.6992f}, 0.f, 1024000.f, 255.f, 16.f},
    },
};
// the colors of the weathers (no cloud, overcast, storm): the sun's and the ambient light's
const ColorSnap kColors[3][8] = {
    {
        {{1.5092f, 1.2699f, 0.f}, {0.498f, 0.4406f, 0.5777f, 1.f}},
        {{1.6168f, 1.4675f, 1.0976f}, {0.4199f, 0.5195f, 0.5976f, 1.f}},
        {{1.8412f, 1.685f, 1.4374f}, {0.4199f, 0.5195f, 0.5976f, 1.f}},
        {{1.6168f, 1.4675f, 1.0976f}, {0.4199f, 0.5195f, 0.5976f, 1.f}},
        {{1.6464f, 1.2601f, 0.f}, {0.3984f, 0.4478f, 0.5976f, 1.f}},
        {{0.3312f, 0.149f, 0.5f}, {0.2788f, 0.3593f, 0.498f, 1.f}},
        {{0.1097f, 0.384f, 0.8232f}, {0.2695f, 0.3143f, 0.4492f, 1.f}},
        {{0.1f, 0.5749f, 0.4f}, {0.239f, 0.3386f, 0.3984f, 1.f}},
    },
    {
        {{0.3772f, 0.3473f, 0.1886f}, {0.5378f, 0.4605f, 0.5976f, 1.f}},
        {{0.4041f, 0.3855f, 0.3392f}, {0.5087f, 0.5639f, 0.642f, 1.f}},
        {{0.4602f, 0.4407f, 0.4098f}, {0.5087f, 0.5639f, 0.642f, 1.f}},
        {{0.4041f, 0.3855f, 0.3392f}, {0.5087f, 0.5639f, 0.642f, 1.f}},
        {{0.4115f, 0.3633f, 0.2057f}, {0.4979f, 0.4975f, 0.6473f, 1.f}},
        {{0.1038f, 0.0811f, 0.125f}, {0.3883f, 0.414f, 0.5527f, 1.f}},
        {{0.1166f, 0.1508f, 0.2057f}, {0.3593f, 0.3592f, 0.4941f, 1.f}},
        {{0.0843f, 0.1437f, 0.1218f}, {0.3186f, 0.3784f, 0.4382f, 1.f}},
    },
    {
        {{0.1509f, 0.1449f, 0.1131f}, {0.3714f, 0.3964f, 0.4877f, 1.f}},
        {{0.1616f, 0.1579f, 0.1486f}, {0.3684f, 0.4604f, 0.5124f, 1.f}},
        {{0.1841f, 0.1802f, 0.174f}, {0.3684f, 0.4604f, 0.5124f, 1.f}},
        {{0.1616f, 0.1579f, 0.1486f}, {0.3684f, 0.4604f, 0.5124f, 1.f}},
        {{0.1646f, 0.1549f, 0.1234f}, {0.3648f, 0.4145f, 0.5142f, 1.f}},
        {{0.0457f, 0.0412f, 0.0499f}, {0.2951f, 0.3406f, 0.4329f, 1.f}},
        {{0.0644f, 0.0713f, 0.0823f}, {0.3593f, 0.3592f, 0.4941f, 1.f}},
        {{0.0456f, 0.0574f, 0.0531f}, {0.3186f, 0.3784f, 0.4382f, 1.f}},
    },
};
// how much of the sun comes from each of its 4 directions per hour, per weather (the moon at night)
const float kChannelGroups[3][4][24] = {
    {
        {0.f, 0.f, 0.f, 0.f, 0.f, 0.2f, 0.4f, 1.f, 0.5f, 0.25f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f},
        {0.8f, 0.6f, 0.4f, 0.2f, 0.f, 0.f, 0.f, 0.f, 0.5f, 0.75f, 1.f, 0.75f, 0.5f, 0.25f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.75f, 0.82f, 0.88f, 0.94f, 1.f},
        {0.2f, 0.4f, 0.6f, 0.8f, 1.f, 0.8f, 0.6f, 0.f, 0.f, 0.f, 0.f, 0.25f, 0.5f, 0.75f, 1.f, 0.84f, 0.67f, 0.5f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f},
        {0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.16f, 0.33f, 0.5f, 1.f, 0.25f, 0.18f, 0.12f, 0.06f, 0.f},
    },
    {
        {0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f},
        {0.5f, 0.42f, 0.34f, 0.25f, 0.17f, 0.08f, 0.f, 1.f, 0.9f, 0.8f, 0.7f, 0.6f, 0.5f, 0.42f, 0.34f, 0.25f, 0.17f, 0.08f, 0.f, 1.f, 0.9f, 0.8f, 0.7f, 0.6f},
        {0.5f, 0.58f, 0.66f, 0.75f, 0.83f, 0.92f, 1.f, 0.f, 0.1f, 0.2f, 0.3f, 0.4f, 0.5f, 0.58f, 0.66f, 0.75f, 0.83f, 0.92f, 1.f, 0.f, 0.1f, 0.2f, 0.3f, 0.4f},
        {0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f},
    },
    {
        {0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f},
        {0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f},
        {0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f, 0.5f},
        {0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f, 0.f},
    },
};
// *clouds-000* .. *clouds-1000*: the sky's clouds (min, max of their texture shown)
const float kClouds[9][2] = {
    {1.f, 1.f},  // 000
    {0.333f, 1.f},  // 125
    {0.333f, 0.555f},  // 250
    {0.167f, 0.605f},  // 375
    {0.f, 0.655f},  // 500
    {0.f, 0.605f},  // 625
    {0.f, 0.555f},  // 750
    {0.f, 0.505f},  // 875
    {0.f, 0.455f},  // 1000
};

// *mood-sky-table*: the sky's color at the 8 snapshots of the day
const float kSky[8][3] = {{83, 103, 190}, {86, 108, 195}, {89, 113, 200}, {86, 108, 195},
                          {83, 103, 190}, {38, 48, 96},   {0, 0, 8},      {0, 29, 33}};

// *mood-interp-table*, per hour: the two snapshots mixed, and how far from the first at the hour's
// start and end
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

float lerp(float a, float b, float t) { return a + (b - a) * t; }

// init-weather!: the weathers' tables mixed for a cloud and a fog (apply-mood-clouds-and-fog,
// apply-mood-colors, apply-mood-channels, adjust-num-clouds). A table whose weight is 0 is never
// read (cloud or fog 1 would read past the last).
struct Weather {
  FogSnap fog[8] = {};
  ColorSnap color[8] = {};
  float channels[4][24] = {};
  float cloud_min = 0, cloud_max = 0;
};

Weather mix_weather(float cloud, float fog) {
  Weather w;
  cloud = std::clamp(cloud, 0.f, 1.f);
  fog = std::clamp(fog, 0.f, 1.f);
  const float wc = 2.f * cloud, wf = 2.f * fog;
  const int ic = (int)wc, ifg = (int)wf;
  const float fc = wc - (float)ic;
  const float cloud_interp = ic == 0 ? (fc < 0.5f ? 0.f : 2.f * (fc - 0.5f)) : fc;
  const float fog_interp = wf - (float)ifg;
  const int index[4] = {3 * ic + ifg, 3 * ic + ifg + 1, 3 * (ic + 1) + ifg, 3 * (ic + 1) + ifg + 1};
  const float weight[4] = {(1.f - fog_interp) * (1.f - cloud_interp), fog_interp * (1.f - cloud_interp),
                           (1.f - fog_interp) * cloud_interp, fog_interp * cloud_interp};
  for (int k = 0; k < 4; k++) {
    if (weight[k] == 0.f || index[k] > 8) continue;
    for (int s = 0; s < 8; s++) {
      const FogSnap& f = kFogs[index[k]][s];
      FogSnap& o = w.fog[s];
      for (int c = 0; c < 3; c++) o.color[c] += f.color[c] * weight[k];
      o.start += f.start * weight[k];
      o.end += f.end * weight[k];
      o.max += f.max * weight[k];
      o.min += f.min * weight[k];
    }
  }
  // the colors: no cloud, overcast, storm
  const float color_weight[2] = {1.f - cloud_interp, cloud_interp};
  for (int k = 0; k < 2; k++) {
    if (color_weight[k] == 0.f || ic + k > 2) continue;
    for (int s = 0; s < 8; s++)
      for (int c = 0; c < 4; c++) {
        if (c < 3) w.color[s].lgt[c] += kColors[ic + k][s].lgt[c] * color_weight[k];
        w.color[s].amb[c] += kColors[ic + k][s].amb[c] * color_weight[k];
      }
  }
  // the sun's directions: the clear weather's up to overcast, then overcast's, then the storm's
  int ci = 0;
  float channel_interp = 0.f;
  if (ic == 0) ci = 0;
  else if (ic == 2) ci = 2;
  else if (fc < 0.5f) channel_interp = 2.f * fc;
  else {
    channel_interp = 2.f * (fc - 0.5f);
    ci = 1;
  }
  const float channel_weight[2] = {1.f - channel_interp, channel_interp};
  for (int k = 0; k < 2; k++) {
    if (channel_weight[k] == 0.f || ci + k > 2) continue;
    for (int d = 0; d < 4; d++)
      for (int h = 0; h < 24; h++) w.channels[d][h] += kChannelGroups[ci + k][d][h] * channel_weight[k];
  }
  // the clouds: 9 steps of cloud
  const float fcl = 8.f * cloud;
  const int i = (int)fcl;
  const float t = fcl - (float)i;
  w.cloud_min = kClouds[i][0] * (1.f - t) + (t != 0.f && i < 8 ? kClouds[i + 1][0] * t : 0.f);
  w.cloud_max = kClouds[i][1] * (1.f - t) + (t != 0.f && i < 8 ? kClouds[i + 1][1] * t : 0.f);
  return w;
}

void normalize3(float v[3]) {
  const float l = std::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
  if (l > 1e-6f)
    for (int k = 0; k < 3; k++) v[k] /= l;
}

const WeatherPreset kPresets[] = {
    {"clear", 0.f, 0.5f, 0.f},       {"few_clouds", 0.25f, 0.5f, 0.f}, {"haze", 0.f, 0.f, 0.f},
    {"smog", 0.f, 1.f, 0.f},         {"overcast", 0.5f, 0.5f, 0.f},     {"overcast_fog", 0.5f, 0.f, 0.f},
    {"rain", 0.5f, 1.f, 0.6f},       {"storm", 1.f, 0.5f, 0.f},         {"storm_rain", 1.f, 1.f, 1.f},
    {"storm_fog", 1.f, 0.f, 0.f},
};

}  // namespace

const WeatherPreset* weather_presets(int* count) {
  *count = (int)(sizeof(kPresets) / sizeof(kPresets[0]));
  return kPresets;
}

GameMood game_mood(const MoodSettings& s) {
  GameMood m;
  const Weather w = mix_weather(s.cloud, s.fog);
  // clear-mood-context: every palette white, off
  for (auto& t : m.times) t = {1, 1, 1, 0};

  // update-mood-exterior: the two snapshots of the hour, mixed
  const float hour = std::fmod(std::fmod(s.hour, 24.f) + 24.f, 24.f);
  const int h = std::min((int)hour, 23);
  const float frac = hour - (float)h;
  const Hour& e = kHours[h];
  const float t = (1.f - frac) * e.start + frac * e.end;
  const ColorSnap& a = w.color[e.s1];
  const ColorSnap& b = w.color[e.s2];
  for (int c = 0; c < 4; c++) m.times[0][c] = lerp(a.amb[c], b.amb[c], t);
  for (int k = 1; k <= 4; k++) {
    for (int c = 0; c < 3; c++) m.times[k][c] = lerp(a.lgt[c], b.lgt[c], t);
    m.times[k][3] = 0;  // the sun colors' w
  }
  const FogSnap& fa = w.fog[e.s1];
  const FogSnap& fb = w.fog[e.s2];
  for (int c = 0; c < 3; c++) {
    m.fog_color[c] = lerp(fa.color[c], fb.color[c], t);
    m.sky_color[c] = lerp(kSky[e.s1][c], kSky[e.s2][c], t);
  }
  m.fog_start = lerp(fa.start, fb.start, t);
  m.fog_end = lerp(fa.end, fb.end, t) * s.fog_mult;
  m.fog_max = lerp(fa.max, fb.max, t);
  m.fog_min = lerp(fa.min, fb.min, t);

  // a lightning flash: the light from above, brighter (the sun's palette of lightning-index 1)
  if (s.flash > 0.f)
    for (int c = 0; c < 3; c++) m.times[2][c] += s.flash;

  // update-mood-direction: the sun's directions lit at that hour, and the light-group 0 the models
  // take (their light-index is 0 unless said otherwise): the directions lit, 3 at most
  for (int c = 0; c < 3; c++) m.light_ambient[c] = m.times[0][c];
  int lights = 0;
  for (int c = 0; c < 4; c++) {
    float wgt = (1.f - frac) * w.channels[c][h] + frac * w.channels[c][(h + 1) % 24];
    if (c == 1 && s.flash > 0.f) wgt = std::max(wgt, s.flash);
    if (wgt == 0.f) continue;
    m.times[c + 1][3] = wgt;
    if (lights < 3) {
      for (int k = 0; k < 3; k++) {
        m.light_dir[lights][k] = kDirections[c][k];
        m.light_color[lights][k] = m.times[c + 1][k] * wgt;
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

  // the sky: the sun rises in the east (+x, where the morning's light comes from) at 6, is highest
  // at noon and sets in the west at 18; the moon goes the same way through the night
  // ponytail: a plain arc from the mood's directions, not sky-tng's orbit of the sun and the moon
  const float day = (hour - 6.f) / 12.f * 3.14159265f, night = (hour - 18.f) / 12.f * 3.14159265f;
  m.sun_dir[0] = std::cos(day);
  m.sun_dir[1] = std::sin(day) * 0.92f;
  m.sun_dir[2] = 0.3f;
  normalize3(m.sun_dir);
  m.moon_dir[0] = std::cos(night);
  m.moon_dir[1] = std::sin(night) * 0.92f;
  m.moon_dir[2] = 0.3f;
  normalize3(m.moon_dir);
  // update-colors-for-time: the sun drawn with the clear weather's light of the hour
  for (int c = 0; c < 3; c++) {
    m.sun_color[c] = lerp(kColors[0][e.s1].lgt[c], kColors[0][e.s2].lgt[c], t);
    m.moon_color[c] = kColors[0][6].lgt[c];
  }
  m.cloud_min = w.cloud_min;
  m.cloud_max = w.cloud_max;
  m.cloud_dark = std::clamp(s.cloud * 1.25f - 0.25f, 0.f, 1.f);
  m.haze = std::clamp(std::fabs(s.fog - 0.5f) * 2.f, 0.f, 1.f);
  m.flash = s.flash;
  return m;
}

}  // namespace ogle
