#pragma once

// What the game does with an element of the decor (a wall, a slab, a vent grate, a shape), kept in
// its node's extras "og_decor" and written by the export for the game (docs/behaviors.md):
//
//   collision  how it collides, through the level builder's own fields (nothing generated):
//              "shape" (the default: by its shape, made by the builder), "none" (Jak goes through it:
//              cables, light shafts) or "invisible" (not drawn, it collides: an invisible wall); its
//              surface (stone, ice, sand, wood, metal, grass...: the sound of the steps, ice slides),
//              what it does to Jak (deadly, burn, endless fall, melt), its mode (ground, wall,
//              obstacle; walls are found from the slope anyway) and its flags (Jak cannot hang from
//              its edges, the camera and the creatures go through it, it does not hide Jak)
//   bounce     Jak touching its top is thrown up `height` meters, on foot and on the board: a bouncer
//              of the game without a model of its own (as the board vents of the industrial zone are),
//              a sphere over the element, generated as the level's code
//   launch     Jak touching its top is thrown along an arc to a point `distance` meters away in the
//              direction `yaw` (degrees, around the element's own facing), `height` meters above it,
//              in `time` seconds: the game's own launch (the Metal Head puffers'), generated
//
// Breaking or deforming an element needs a model of its own that the game draws as an actor (the
// decor the builder makes is drawn as one): not offered for the decor (docs/behaviors.md says how).

#include <string>
#include <vector>

#include "json.hpp"

namespace ogle {

struct DecorCollision {
  std::string kind = "shape";      // shape, none, invisible
  std::string material = "stone";  // the builder's surfaces: stone, ice, quicksand, waterbottom, tar, sand,
                                   // wood, grass, pcmetal, snow, deepsnow, hotcoals, lava, crwood, gravel,
                                   // dirt, metal, straw, tube, swamp, stopproj, rotate, neutral
  std::string event = "none";      // none, deadly, endlessfall, burn, deadlyup, burnup, melt
  std::string mode = "auto";       // auto (ground, walls by their slope), ground, wall, obstacle
  bool noedge = false, nocamera = false, noentity = false, nolineofsight = false;
  bool set = false;                // the node has one (else the export leaves it as it was)
};

struct DecorBounce {
  bool set = false;
  float height = 11.f;  // meters (the game's default 45056 units)
};

struct DecorLaunch {
  bool set = false;
  float distance = 20.f;  // meters, along the ground
  float height = 8.f;     // meters above where it starts
  float yaw = 0.f;        // degrees, around the element's facing (its local +z)
  float time = 1.f;       // seconds in the air
};

struct DecorBehavior {
  DecorCollision collision;
  DecorBounce bounce;
  DecorLaunch launch;
  bool any() const { return collision.set || bounce.set || launch.set; }
};

DecorBehavior decor_behavior_of(const nlohmann::json& extras);  // from a node's extras
nlohmann::json decor_behavior_json(const DecorBehavior& b);      // what og_decor holds, null when nothing

// The names the editor offers, the builder's numbers (Jak 1's layout, which it converts: stone 0)
const std::vector<std::string>& collision_materials();
const std::vector<std::string>& collision_events();
int builder_material(const std::string& name);  // -1 when unknown
int builder_event(const std::string& name);
int builder_mode(const std::string& name);       // -1 for auto

// The glTF extras the level builder reads for an element with that collision
nlohmann::json collision_extras(const DecorCollision& c);

// The GOAL code of a bouncer without a model, over an element, `radius` meters around: `type`
// derives the game's bouncer
std::string bounce_code(const std::string& type, float radius, const std::string& comment);
// The GOAL code of a launch from an element: Jak within `radius` meters of `from` (game units) is
// thrown to `to` (game units, the world) in `time` seconds
std::string launch_code(const std::string& type, float radius, const float to[3], float time, const std::string& comment);

}  // namespace ogle
