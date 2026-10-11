#include "scene/particles.h"

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>

#include "json.hpp"

namespace ogle {

namespace {

using json = nlohmann::json;

// sp-cpuinfo-flag
uint32_t cpuinfo_flag(const std::string& name) {
  static const std::unordered_map<std::string, int> bits = {
      {"ready-to-launch", 6}, {"distort", 7},  {"aux-list", 8},         {"level0", 10},
      {"level1", 11},         {"glow", 15},    {"use-global-acc", 16},  {"launch-along-z", 17},
      {"left-multiply-quat", 18}, {"right-multiply-quat", 19}, {"set-conerot", 20}};
  if (auto it = bits.find(name); it != bits.end()) return 1u << it->second;
  if (name.rfind("sp-cpuinfo-flag-", 0) == 0) return 1u << std::atoi(name.c_str() + 16);
  return 0;
}

// sp1, sp7, is-3d...: their bit
uint16_t sp_bits(const json& names) {
  uint16_t out = 0;
  if (!names.is_array()) return out;
  for (const auto& n : names) {
    if (!n.is_string()) continue;
    const std::string s = n.get<std::string>();
    if (s == "is-3d") out |= 1;
    else if (s.size() > 2 && s[0] == 's' && s[1] == 'p') out |= (uint16_t)(1u << std::atoi(s.c_str() + 2));
  }
  return out;
}

// an angle of the game (65536 a turn) as radians
float angle(float units) { return units * (2.f * kPi / 65536.f); }

Mat4 rotation(float rx, float ry, float rz, bool cone) {
  const Mat4 x = Mat4::rotate(Quat::axis_angle({1, 0, 0}, angle(rx)));
  const Mat4 y = Mat4::rotate(Quat::axis_angle({0, 1, 0}, angle(ry)));
  const Mat4 z = Mat4::rotate(Quat::axis_angle({0, 0, 1}, angle(rz)));
  // the cone turns the launch away from its axis (z, then x and y), the rotation turns that around
  return cone ? z * x * y : y * x * z;
}

}  // namespace

bool ParticleData::load(const std::string& path, std::string* error) {
  std::ifstream f(std::filesystem::path(std::u8string((const char8_t*)path.c_str())), std::ios::binary);
  if (!f) {
    if (error) *error = "cannot read " + path;
    return false;
  }
  json j = json::parse(f, nullptr, false);
  if (!j.is_object() || !j.contains("groups") || !j.contains("launchers")) {
    if (error) *error = "not the editor's particles: " + path;
    return false;
  }
  std::unordered_map<std::string, int> texture_index;
  for (const auto& [id, specs] : j["launchers"].items()) {
    auto& out = m_launchers[std::atoi(id.c_str())];
    for (const auto& s : specs) {
      if (!s.is_array() || s.size() != 5 || !s[0].is_number() || !s[1].is_string()) continue;
      SpSpec spec;
      spec.field = (uint8_t)s[0].get<int>();
      const std::string kind = s[1].get<std::string>();
      auto num = [&](int i) { return s[i].is_number() ? s[i].get<float>() : 0.f; };
      if (kind == "f" || kind == "fi" || kind == "i") {
        spec.kind = kind == "f" ? SpSpec::Float : kind == "fi" ? SpSpec::FloatIntRand : SpSpec::Int;
        spec.a = num(2);
        spec.b = num(3);
        spec.c = num(4);
      } else if (kind == "copy" && s[3].is_number()) {
        spec.kind = SpSpec::Copy;
        spec.ref = s[3].get<int>();
      } else if (kind == "next" && s[3].is_number()) {
        spec.kind = SpSpec::Next;
        spec.ref = s[3].get<int>();
      } else if (kind == "flags" && s[3].is_array()) {
        spec.kind = SpSpec::Flags;
        for (const auto& n : s[3])
          if (n.is_string()) spec.flags |= cpuinfo_flag(n.get<std::string>());
      } else if (kind == "tex" && s[3].is_string()) {
        spec.kind = SpSpec::Texture;
        const std::string t = s[3].get<std::string>();
        auto [it, fresh] = texture_index.emplace(t, (int)m_textures.size());
        if (fresh) m_textures.push_back(t);
        spec.ref = it->second;
      } else {
        continue;  // symbols (the game's functions): not run
      }
      out.push_back(spec);
    }
  }
  for (const auto& [name, g] : j["groups"].items()) {
    SpGroup group;
    group.name = name;
    group.flags = sp_bits(g.value("flags", json::array()));
    if (g.contains("bounds") && g["bounds"].is_array() && g["bounds"].size() == 4 && g["bounds"][3].is_number())
      group.radius = g["bounds"][3].get<float>();
    for (const auto& p : g.value("parts", json::array())) {
      if (!p.is_array() || p.size() != 9 || !p[0].is_number()) continue;
      SpItem item;
      item.launcher = p[0].get<int>();
      item.flags = sp_bits(p[1]);
      item.period = p[2].is_number() ? p[2].get<int>() : 0;
      item.length = p[3].is_number() ? p[3].get<int>() : 0;
      item.offset = p[4].is_number() ? p[4].get<int>() : 0;
      item.fade_after = p[5].is_number() ? p[5].get<float>() : 0.f;
      item.falloff_to = p[6].is_number() ? p[6].get<float>() : 0.f;
      item.hour_mask = p[8].is_number() ? p[8].get<uint32_t>() : 0;
      group.items.push_back(item);
    }
    m_groups.emplace(name, std::move(group));
  }
  return true;
}

const SpGroup* ParticleData::group(const std::string& name) const {
  auto it = m_groups.find(!name.empty() && name[0] == '\'' ? name.substr(1) : name);
  return it == m_groups.end() ? nullptr : &it->second;
}

const std::vector<SpSpec>* ParticleData::launcher(int id) const {
  auto it = m_launchers.find(id);
  return it == m_launchers.end() ? nullptr : &it->second;
}

// ------------------------------------------------------------------------------------------------

void ParticleSystem::begin_frame() {
  for (auto& [id, e] : m_emitters) e.seen = false;
}

void ParticleSystem::emit(uint64_t id, const SpGroup* group, const Mat4& world) {
  if (!group) return;
  Emitter& e = m_emitters[id];
  if (e.group != group) {
    e = Emitter{};
    e.group = group;
    e.accum.assign(group->items.size(), 0.f);
    e.launched.assign(group->items.size(), false);
  }
  e.world = world;
  e.seen = true;
}

void ParticleSystem::clear() {
  m_emitters.clear();
  m_particles.clear();
  m_time = 0;
}

void ParticleSystem::update(const ParticleData& data, double dt, const Vec3& camera, float hour) {
  for (auto it = m_emitters.begin(); it != m_emitters.end();) {
    if (!it->second.seen) it = m_emitters.erase(it);
    else ++it;
  }
  m_time = std::min(m_time + dt, 10.0 / 60.0);
  while (m_time >= 1.0 / 60.0) {
    m_time -= 1.0 / 60.0;
    step(data, camera, hour);
  }
}

// sp-init-fields!: each field from its spec, the ones not given at 0 (a new particle) or as they
// are (a launcher taking over)
void ParticleSystem::init(Particle& p, const std::vector<SpSpec>& specs, bool relaunch) {
  if (!relaunch) {
    p.f.fill(0.f);
    p.f[sp::Friction] = 1.f;  // none given: it keeps its speed
    p.f[sp::Timer] = -1.f;
  }
  for (const auto& s : specs) {
    float& v = p.f[s.field];
    switch (s.kind) {
      case SpSpec::Float: v = s.b != 0 ? s.a + s.c * random() * s.b : s.a; break;
      case SpSpec::FloatIntRand: v = s.b != 0 ? s.a + s.c * std::floor(random() * (s.b + 1)) : s.a; break;
      case SpSpec::Int: v = s.b != 0 ? s.a + s.c * std::floor(random() * s.b) : s.a; break;
      case SpSpec::Copy:
        if (s.ref >= 0 && s.ref < sp::Count) v = p.f[s.ref];
        break;
      case SpSpec::Next: v = (float)s.ref; break;
      case SpSpec::Flags: p.flags = s.flags; break;
      case SpSpec::Texture: p.texture = s.ref; break;
      default: break;
    }
  }
}

void ParticleSystem::launch(Emitter& e, const std::vector<SpSpec>& specs, float rate, float& accum) {
  // how many: num a launch (a fraction carried to the next)
  float num = 1.f;
  for (const auto& s : specs)
    if (s.field == sp::Num && (s.kind == SpSpec::Float || s.kind == SpSpec::Int || s.kind == SpSpec::FloatIntRand))
      num = s.b != 0 ? s.a + s.c * random() * s.b : s.a;
  accum += num * rate;
  int n = (int)std::floor(accum);
  accum -= (float)n;
  const bool turned = (e.group->flags & (1u << 4)) != 0;  // sp4: the effect turns with its actor
  const Vec3 origin = e.world.point({0, 0, 0}) * 4096.f;
  for (; n > 0 && m_particles.size() < max_particles; n--) {
    Particle& p = m_particles.emplace_back();
    init(p, specs, false);
    const auto& f = p.f;
    // launched along its cone and turned (the launcher's), then turned with the effect
    const Mat4 r = rotation(f[sp::RotateX], f[sp::RotateY], f[sp::RotateZ], false) *
                   rotation(f[sp::ConeRotX], f[sp::ConeRotY], f[sp::ConeRotZ], true);
    Vec3 pos = r.dir(Vec3{f[sp::X], f[sp::Y], f[sp::Z] + f[sp::ConeRotRadius]});
    Vec3 vel = r.dir(Vec3{f[sp::VelX], f[sp::VelY], f[sp::VelZ]});
    Vec3 acc{f[sp::AccelX], f[sp::AccelY], f[sp::AccelZ]};
    if (turned) {
      const Mat4 w = e.world;
      auto turn = [&](const Vec3& v) { return normalize(w.dir(v)) * length(v); };
      pos = turn(pos);
      vel = turn(vel);
      if (!(p.flags & kSpGlobalAcc)) acc = turn(acc);
    }
    p.pos = origin + pos;
    p.vel = vel;
    p.acc = acc;
    p.next_time = f[sp::NextLauncher] != 0 ? (int)f[sp::NextTime] : -1;
  }
}

void ParticleSystem::step(const ParticleData& data, const Vec3& camera, float hour) {
  constexpr int kTicks = 5;  // a frame at 60 a second
  // the effects launch (sparticle-launch-control spawn)
  const uint32_t hour_bit = hour < 0 ? 0u : 1u << std::clamp((int)hour, 0, 23);
  for (auto& [id, e] : m_emitters) {
    if (!e.seen) continue;
    const float dist = length(e.world.point({0, 0, 0}) - camera);
    if (dist > max_distance) continue;
    for (size_t i = 0; i < e.group->items.size(); i++) {
      const SpItem& item = e.group->items[i];
      const auto* specs = data.launcher(item.launcher);
      if (!specs) continue;
      float rate = item.falloff_to > 0 ? 1.f - dist / item.falloff_to : 1.f;
      if (rate <= 0) continue;
      if (item.flags & (1u << 3)) {  // sp3: once
        if (!e.launched[i]) {
          e.launched[i] = true;
          launch(e, *specs, rate, e.accum[i]);
        }
        continue;
      }
      if ((item.hour_mask & hour_bit) || (item.fade_after > 0 && dist > item.fade_after)) continue;
      if (item.period > 0) {
        const int t = ((e.clock - item.offset) % item.period + item.period) % item.period;
        if (t >= item.length) continue;
      }
      launch(e, *specs, rate, e.accum[i]);
    }
    e.clock += kTicks;
  }
  // the particles move (sp-process-block): their velocity per tick, their friction and
  // acceleration per frame
  size_t alive = 0;
  for (size_t i = 0; i < m_particles.size(); i++) {
    Particle& p = m_particles[i];
    auto& f = p.f;
    // freed once its timer ran out the frame before (a particle of one frame is drawn once)
    if (f[sp::Timer] != -1.f) {
      if (f[sp::Timer] <= 0) continue;
      f[sp::Timer] = std::max(0.f, f[sp::Timer] - kTicks);
    }
    p.age += kTicks;
    if (p.next_time >= 0 && p.age >= p.next_time) {
      // another launcher takes over (sp-relaunch-setup-fields): its fields over these
      const auto* next = data.launcher((int)f[sp::NextLauncher]);
      p.next_time = -1;
      if (next) {
        const float friction = f[sp::Friction];
        f[sp::NextLauncher] = 0;
        init(p, *next, true);
        if (f[sp::Friction] == 0) f[sp::Friction] = friction;
        for (const auto& s : *next) {
          if (s.field == sp::VelX || s.field == sp::VelY || s.field == sp::VelZ) p.vel = {f[sp::VelX], f[sp::VelY], f[sp::VelZ]};
          if (s.field == sp::AccelX || s.field == sp::AccelY || s.field == sp::AccelZ)
            p.acc = {f[sp::AccelX], f[sp::AccelY], f[sp::AccelZ]};
        }
        if (f[sp::NextLauncher] != 0) p.next_time = p.age + (int)f[sp::NextTime];
      }
    }
    p.vel += p.acc;
    p.vel *= f[sp::Friction];
    p.pos += p.vel * (float)kTicks;
    f[sp::ScaleX] += f[sp::ScaleVelX] * kTicks;
    f[sp::ScaleY] += f[sp::ScaleVelY] * kTicks;
    f[sp::RotZ] += f[sp::RotVelZ] * kTicks;
    for (int c = 0; c < 4; c++) f[sp::R + c] = std::max(0.f, f[sp::R + c] + f[sp::FadeR + c] * kTicks);
    if (f[sp::A] <= 0 && f[sp::FadeA] <= 0 && p.age > kTicks) continue;  // faded out
    if (alive != i) m_particles[alive] = std::move(p);
    alive++;
  }
  m_particles.resize(alive);
}

void ParticleSystem::sprites(std::vector<ParticleSprite>& out) const {
  out.clear();
  out.reserve(m_particles.size());
  for (const auto& p : m_particles) {
    const auto& f = p.f;
    ParticleSprite s;
    s.pos = p.pos / 4096.f;
    s.sx = f[sp::ScaleX] / 4096.f;
    s.sy = f[sp::ScaleY] / 4096.f;
    s.rot = angle(f[sp::RotZ]);
    s.color = {f[sp::R] / 128.f, f[sp::G] / 128.f, f[sp::B] / 128.f, std::min(f[sp::A], 128.f) / 128.f};
    s.texture = p.texture;
    s.additive = (p.flags & kSpGlow) != 0;
    s.from_left = ((int)f[sp::RotX] & 15) == 4;
    if (s.sx <= 0 || s.sy <= 0 || s.color.w <= 0.002f) continue;
    out.push_back(s);
  }
}

AABB ParticleSystem::bounds() const {
  AABB b;
  for (const auto& p : m_particles) {
    const float r = std::max(p.f[sp::ScaleX], p.f[sp::ScaleY]) / 8192.f;
    b.add(p.pos / 4096.f - Vec3{r, r, r});
    b.add(p.pos / 4096.f + Vec3{r, r, r});
  }
  return b;
}

}  // namespace ogle
