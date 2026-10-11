#pragma once

// The particle effects of Jak 3 (steam, sparks, smoke, fire, lights) as the game makes them: its
// definitions (data/jak3/particles.json, read in its code by tools/gen_particles.py) run by its
// rules (sparticle-launcher.gc, sparticle.gc). A group launches the particles of its parts over
// time (continuously, in bursts every period, or once); a particle starts from its launcher's
// fields (a value and a random range each: its position, velocity, size, rotation, color and how
// they change, its life) and moves 60 times a second: velocities are per tick (300 a second, 5 a
// frame), friction and acceleration per frame. At a time of its life it can take the fields of
// another launcher (next-launcher: steam that fades out).
//
// The editor shows them where the effects of the levels are (the actors naming a group by their
// art-name: part-spawner...), and in the catalog.

#include <array>
#include <cstdint>
#include <random>
#include <string>
#include <unordered_map>
#include <vector>

#include "core/math.h"

namespace ogle {

// sp-field-id
namespace sp {
enum Field : uint8_t {
  Texture = 1, Num = 6, X = 10, Y = 11, Z = 12, ScaleX = 13, RotX = 14, RotY = 15, RotZ = 16, ScaleY = 17,
  R = 18, G = 19, B = 20, A = 21, Omega = 24, VelX = 25, VelY = 26, VelZ = 27, ScaleVelX = 28, RotVelX = 29,
  RotVelY = 30, RotVelZ = 31, ScaleVelY = 32, FadeR = 33, FadeG = 34, FadeB = 35, FadeA = 36, AccelX = 37,
  AccelY = 38, AccelZ = 39, Friction = 45, Timer = 46, Flags = 47, NextTime = 50, NextLauncher = 51,
  LaunchRotX = 54, LaunchRotY = 55, LaunchRotZ = 56, ConeRotX = 58, ConeRotY = 59, ConeRotZ = 60,
  RotateX = 62, RotateY = 63, RotateZ = 64, ConeRotRadius = 65, Count = 72
};
}

// an init spec of a launcher: how a field starts
struct SpSpec {
  enum Kind : uint8_t { Float, FloatIntRand, Int, Copy, Next, Flags, Texture, Other } kind = Other;
  uint8_t field = 0;
  float a = 0, b = 0, c = 0;  // the value, its random range, its random multiplier
  int ref = 0;                // Copy: the field copied; Next: the launcher; Texture: its index
  uint32_t flags = 0;         // Flags: sp-cpuinfo-flag bits
};

// sp-cpuinfo-flag bits the editor uses
constexpr uint32_t kSpGlow = 1u << 15;          // drawn by the glow renderer: added to what is behind
constexpr uint32_t kSpGlobalAcc = 1u << 16;     // its acceleration in the world (not turned with the effect)

// a part of a group: a launcher and when it launches (sp-group-item)
struct SpItem {
  int launcher = 0;
  uint16_t flags = 0;  // is-3d sp1..sp15: bit 3 once, at the start
  int period = 0, length = 0, offset = 0;  // ticks: launching length ticks every period (0: always)
  float fade_after = 0, falloff_to = 0;    // meters: not launched beyond, fewer up to
  uint32_t hour_mask = 0;                  // not launched at these hours (bit h)
};

struct SpGroup {
  std::string name;
  uint16_t flags = 0;  // sp-group-flag: bit 4, the effect turned with its actor
  float radius = 0;    // meters: its bounds
  std::vector<SpItem> items;
};

class ParticleData {
 public:
  bool load(const std::string& path, std::string* error);
  bool empty() const { return m_groups.empty(); }
  const SpGroup* group(const std::string& name) const;  // an art-name ("group-...", quoted or not)
  const std::vector<SpSpec>* launcher(int id) const;
  // the textures the launchers draw: "<tpage>/<name>", or "<page>:<index>"
  const std::vector<std::string>& textures() const { return m_textures; }

 private:
  std::unordered_map<std::string, SpGroup> m_groups;
  std::unordered_map<int, std::vector<SpSpec>> m_launchers;
  std::vector<std::string> m_textures;
};

// a particle as drawn: a sprite facing the camera
struct ParticleSprite {
  Vec3 pos;           // meters
  float sx = 0, sy = 0;  // meters
  float rot = 0;      // radians
  Vec4 color;         // 1 is the texture as it is (the game's 128)
  int texture = -1;   // ParticleData::textures()
  bool additive = false;
  bool from_left = false;  // the sprite starts at its position (the game's flag 4: streaks), else centered
};

class ParticleSystem {
 public:
  // the effects to run this frame: each by an id of the caller (an actor), its group and where it
  // is; the effects not given again stop launching and go once their particles are gone
  void begin_frame();
  void emit(uint64_t id, const SpGroup* group, const Mat4& world);
  // the time passed (seconds: 60 steps a second at most 10 a call), the camera (the effects far
  // from it do not launch), the hour (the hour-mask of some parts; < 0: every part, any hour)
  void update(const ParticleData& data, double dt, const Vec3& camera, float hour = 12.f);
  void clear();
  size_t count() const { return m_particles.size(); }
  void sprites(std::vector<ParticleSprite>& out) const;
  // the box of the particles (meters), for a picture of an effect
  AABB bounds() const;

  size_t max_particles = 20000;
  float max_distance = 150.f;  // meters

 private:
  struct Particle {
    std::array<float, sp::Count> f{};  // the fields (sp-field-id)
    Vec3 pos, vel, acc;                // game units, per tick, per frame
    const std::vector<SpSpec>* next = nullptr;
    int next_time = -1;  // ticks of its life the next launcher takes over
    int age = 0;         // ticks
    int texture = -1;
    uint32_t flags = 0;
  };
  struct Emitter {
    const SpGroup* group = nullptr;
    Mat4 world;
    std::vector<float> accum;   // per item: the particles started but for a fraction
    std::vector<bool> launched;  // per item: launched once (bit 3)
    int clock = 0;               // ticks
    bool seen = true;
  };
  void step(const ParticleData& data, const Vec3& camera, float hour);
  void launch(Emitter& e, const std::vector<SpSpec>& specs, float rate, float& accum);
  void init(Particle& p, const std::vector<SpSpec>& specs, bool relaunch);
  float random() { return m_dist(m_rng); }

  std::unordered_map<uint64_t, Emitter> m_emitters;
  std::vector<Particle> m_particles;
  double m_time = 0;  // not stepped yet
  std::mt19937 m_rng{12345};
  std::uniform_real_distribution<float> m_dist{0.f, 1.f};
};

}  // namespace ogle
