// The Game rendering window (View > Sky, hour and weather...): the game view's settings apart from
// the menus, a half turn of its sky to see the sun, the clouds and the colors of the hour, the hour
// and the weather as the game has them (game_mood.h), and what they do to the level: its light, its
// fog, rain, lightning, the effects at night.

#include <imgui.h>

#include <algorithm>
#include <cmath>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::IsItemHovered(ImGuiHoveredFlags_DelayShort) && text && *text) ImGui::SetTooltip("%s", text);
}

float hash01(uint32_t x) {
  x ^= x >> 16;
  x *= 0x7feb352dU;
  x ^= x >> 15;
  x *= 0x846ca68bU;
  x ^= x >> 16;
  return (float)(x & 0xffffff) / (float)0x1000000;
}

// the shape of a lightning flash over its frames (mood.gc *flash0*...: a few bright blinks)
const float kFlash[] = {1.f, 0.6f, 0.2f, 0.f, 0.f, 0.8f, 1.f, 0.7f, 0.4f, 0.2f, 0.1f, 0.f};

}  // namespace

int App::weather_preset_index() const {
  int count = 0;
  const WeatherPreset* p = weather_presets(&count);
  const MoodSettings& m = game_view.mood;
  for (int i = 0; i < count; i++)
    if (std::fabs(p[i].cloud - m.cloud) < 0.01f && std::fabs(p[i].fog - m.fog) < 0.01f &&
        std::fabs(p[i].rain - m.rain) < 0.01f)
      return i;
  return -1;
}

bool App::set_weather(const std::string& id) {
  int count = 0;
  const WeatherPreset* p = weather_presets(&count);
  for (int i = 0; i < count; i++)
    if (id == p[i].id) {
      game_view.mood.cloud = p[i].cloud;
      game_view.mood.fog = p[i].fog;
      game_view.mood.rain = p[i].rain;
      lightning = p[i].cloud >= 0.99f;
      return true;
    }
  return false;
}

void App::update_weather(float dt) {
  MoodSettings& m = game_view.mood;
  game_view.time += dt;
  if (time_passes) m.hour = std::fmod(m.hour + dt * time_speed / 60.f + 24.f, 24.f);
  // a storm's lightning: a flash every 5 to 10 seconds (gen-lightning-and-thunder!)
  m.flash = 0.f;
  if (!lightning || m.cloud < 0.75f || !game_view_on) {
    m_lightning_wait = 3.f;
    return;
  }
  if (m_lightning_frame >= 0) {
    m_lightning_time += dt;
    const int frame = (int)(m_lightning_time * 30.f);
    if (frame < (int)(sizeof(kFlash) / sizeof(kFlash[0]))) m.flash = kFlash[frame] * 0.8f;
    else m_lightning_frame = -1;
    return;
  }
  m_lightning_wait -= dt;
  if (m_lightning_wait <= 0.f) {
    m_lightning_frame = 0;
    m_lightning_time = 0.f;
    m_lightning_wait = 5.f + 5.f * hash01((uint32_t)(game_view.time * 1000.f));
  }
}

void App::draw_rain() {
  const MoodSettings& m = game_view.mood;
  if (!game_view_on || !game_view.ready() || m.rain <= 0.f || !document_open()) return;
  // streaks falling around the camera (the game's rain particles: launched around it, a few meters
  // long, lit by the mood), as many as the rain is strong
  const GameMood mood = game_mood(m);
  const float light = std::clamp((mood.light_ambient[0] + mood.light_ambient[1] + mood.light_ambient[2]) / 3.f, 0.2f, 1.f);
  const uint32_t color = pack_rgba(0.75f * light + m.flash, 0.8f * light + m.flash, 0.9f * light + m.flash, 0.35f);
  const int count = (int)(2500 * m.rain);
  const float box = 24.f, height = 18.f, speed = 16.f, len = 0.7f;
  const Vec3 c = camera.pos;
  for (int i = 0; i < count; i++) {
    const float x = (hash01(i * 3 + 1) - 0.5f) * 2.f * box, z = (hash01(i * 3 + 2) - 0.5f) * 2.f * box;
    const float fall = std::fmod(game_view.time * speed + hash01(i * 3 + 3) * height, height);
    // the streaks stay put in the world as the camera moves (their cell follows it)
    const Vec3 p{std::floor(c.x / (2 * box)) * 2 * box + x, c.y + height * 0.5f - fall, std::floor(c.z / (2 * box)) * 2 * box + z};
    Vec3 q = p;
    if (q.x - c.x > box) q.x -= 2 * box;
    if (q.x - c.x < -box) q.x += 2 * box;
    if (q.z - c.z > box) q.z -= 2 * box;
    if (q.z - c.z < -box) q.z += 2 * box;
    renderer.line(q, q + Vec3{0.05f, -len, 0.02f}, color);
  }
  renderer.flush(true);
}

void App::fog_effects(std::vector<ParticleSprite>& sprites) const {
  if (!game_view_on || !game_view.ready() || !document_open()) return;
  // the effects in the game's fog of the hour and the weather: lost in it far away
  const GameMood m = game_mood(game_view.mood);
  const float start = m.fog_start / 4096.f, end = std::max(m.fog_end / 4096.f, start + 1.f);
  const float most = (m.fog_max - m.fog_min) / 255.f;
  const Vec3 fog{m.fog_color[0] / 255.f, m.fog_color[1] / 255.f, m.fog_color[2] / 255.f};
  for (auto& s : sprites) {
    const float f = std::clamp((length(s.pos - camera.pos) - start) / (end - start), 0.f, 1.f) * most;
    if (f <= 0.f) continue;
    if (s.additive) {
      s.color.x *= 1.f - f;
      s.color.y *= 1.f - f;
      s.color.z *= 1.f - f;
    } else {
      s.color.x += (fog.x - s.color.x) * f;
      s.color.y += (fog.y - s.color.y) * f;
      s.color.z += (fog.z - s.color.z) * f;
    }
  }
}

void App::ui_game_window() {
  if (!show_game_window) return;
  ImGui::SetNextWindowSize(ImVec2(440, 0), ImGuiCond_FirstUseEver);
  ImGui::SetNextWindowPos(ImVec2(m_vp_x + m_vp_w - 460, m_vp_y + 40), ImGuiCond_FirstUseEver);
  if (!ImGui::Begin(tr_id("game.title").c_str(), &show_game_window, ImGuiWindowFlags_NoCollapse)) {
    ImGui::End();
    return;
  }
  MoodSettings& m = game_view.mood;
  if (!game_view.ready()) {
    ImGui::TextWrapped(tr("menu.view.game_view.unavailable"), game_view_error.c_str());
    ImGui::End();
    return;
  }
  ImGui::Checkbox(tr("game.on"), &game_view_on);
  tooltip(tr("menu.view.game_view.tip"));

  // the sky: a half turn around where the camera looks (the whole turn loses the sense of where
  // things are), the horizon, the four directions
  const float w = ImGui::GetContentRegionAvail().x, h = std::round(w * 0.36f);
  const unsigned tex = game_view.sky_preview((int)w, (int)h, camera.yaw);
  const ImVec2 p0 = ImGui::GetCursorScreenPos();
  if (tex) ImGui::Image((ImTextureID)(uint64_t)tex, ImVec2(w, h), ImVec2(0, 1), ImVec2(1, 0));
  else ImGui::Dummy(ImVec2(w, h));
  {
    ImDrawList* dl = ImGui::GetWindowDrawList();
    const float horizon = p0.y + h * (70.f / 80.f);  // -10..70 degrees, top to bottom
    dl->AddLine(ImVec2(p0.x, horizon), ImVec2(p0.x + w, horizon), IM_COL32(255, 255, 255, 70));
    // the directions: yaw 0 looks to -z (north), yaw grows to the left (west)
    const char* names[] = {"N", "W", "S", "E"};
    for (int k = 0; k < 4; k++) {
      float a = k * 1.5707963f - camera.yaw;
      a = std::remainder(a, 6.2831853f);
      if (std::fabs(a) > 1.5707963f) continue;
      const float x = p0.x + w * (0.5f - a / 3.14159265f);
      dl->AddText(ImVec2(x - 4, horizon + 2), IM_COL32(255, 255, 255, 200), names[k]);
    }
    dl->AddText(ImVec2(p0.x + w * 0.5f - 3, p0.y + 2), IM_COL32(255, 255, 255, 160), "v");
  }
  ImGui::TextDisabled("%s", tr("game.sky_tip"));

  // the hour
  ImGui::SeparatorText(tr("game.hour"));
  ImGui::SetNextItemWidth(-1);
  const int hh = (int)m.hour, mm = (int)((m.hour - hh) * 60.f);
  ImGui::SliderFloat("##hour", &m.hour, 0.f, 24.f, strf("%02d:%02d", hh % 24, mm).c_str());
  tooltip(tr("game.hour_tip"));
  const struct {
    const char* key;
    float hour;
  } hours[] = {{"game.dawn", 6.5f}, {"game.noon", 12.f}, {"game.dusk", 18.5f}, {"game.night", 22.f}};
  for (int i = 0; i < 4; i++) {
    if (i) ImGui::SameLine();
    if (ImGui::Button(tr_id(hours[i].key).c_str())) m.hour = hours[i].hour;
  }
  ImGui::Checkbox(tr("game.time_passes"), &time_passes);
  tooltip(tr("game.time_passes_tip"));
  if (time_passes) {
    ImGui::SameLine();
    ImGui::SetNextItemWidth(-1);
    ImGui::SliderFloat("##speed", &time_speed, 1.f, 240.f, tr("game.speed"), ImGuiSliderFlags_Logarithmic);
  }

  // the weather
  ImGui::SeparatorText(tr("game.weather"));
  int count = 0;
  const WeatherPreset* presets = weather_presets(&count);
  const int current = weather_preset_index();
  ImGui::SetNextItemWidth(-1);
  const std::string label = current >= 0 ? tr(("weather." + std::string(presets[current].id)).c_str()) : tr("weather.custom");
  if (ImGui::BeginCombo("##weather", label.c_str())) {
    for (int i = 0; i < count; i++) {
      if (ImGui::Selectable(tr(("weather." + std::string(presets[i].id)).c_str()), i == current)) set_weather(presets[i].id);
      tooltip(tr(("weather." + std::string(presets[i].id) + ".tip").c_str()));
    }
    ImGui::EndCombo();
  }
  tooltip(tr("game.weather_tip"));
  ImGui::PushItemWidth(-140);
  ImGui::SliderFloat(tr_id("game.clouds").c_str(), &m.cloud, 0.f, 1.f, "%.2f");
  tooltip(tr("game.clouds_tip"));
  ImGui::SliderFloat(tr_id("game.fog").c_str(), &m.fog, 0.f, 1.f, "%.2f");
  tooltip(tr("game.fog_tip"));
  ImGui::SliderFloat(tr_id("game.rain").c_str(), &m.rain, 0.f, 1.f, "%.2f");
  tooltip(tr("game.rain_tip"));
  ImGui::PopItemWidth();
  ImGui::Checkbox(tr("game.lightning"), &lightning);
  tooltip(tr("game.lightning_tip"));
  ImGui::Checkbox(tr("menu.view.game_lights"), &m.level_lights);
  tooltip(tr("menu.view.game_lights.tip"));
  ImGui::Checkbox(tr("menu.view.effects"), &show_effects);
  tooltip(tr("game.effects_tip"));

  // what the game does with it, in words
  const GameMood gm = game_mood(m);
  ImGui::SeparatorText(tr("game.now"));
  const float sun = std::max({gm.light_color[0][0], gm.light_color[0][1], gm.light_color[0][2], 0.f});
  ImGui::TextWrapped(tr("game.now_text"), (gm.sun_dir[1] > 0.f ? tr("game.sun_up") : tr("game.sun_down")), sun,
                     gm.fog_end / 4096.f, m.level_lights && gm.times[5][3] > 0.f ? tr("game.lamps_on") : tr("game.lamps_off"));
  ImGui::End();
}

}  // namespace ogle
