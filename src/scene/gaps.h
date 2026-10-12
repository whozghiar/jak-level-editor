#pragma once

// Holes in the decor: where a player walking the level would see the void (nothing drawn behind,
// the screen's background), a visual bug in the game. From points of the ground every few meters
// (where Jak's eyes are), rays go around and downward; a ray that meets nothing is a view into the
// void. The rays around it tell where the hole is: a gap between pieces (something on both sides of
// it, the void right behind), or an open edge (the ground ends: seen directly, or through a passage
// between pieces, then where that ground ends).

#include <vector>

#include "scene/scene.h"

namespace ogle {

struct GapOptions {
  AABB region;               // where the player can go (its ground is found inside)
  float spacing = 0.f;       // meters between the points looked from (0: so that there are ~400)
  float eye = 1.6f;          // meters above the ground
  float reach = 300.f;       // meters: what is farther counts as nothing
  float ocean = -1e30f;      // the ocean's height when the level has one: it hides what is under it
  std::vector<uint32_t> ignore;  // elements not looked at (and what is under them)
};

struct GapSpot {
  Vec3 at;          // about where the hole is (the surfaces around it, or the edge crossed)
  Vec3 seen_from;   // a point of the ground it is seen from (the eye's height)
  Vec3 direction;   // looking that way
  int rays = 0;     // how many rays went through it
  bool edge = false;  // an open edge of the level (no surface around), not a gap between pieces
};

struct GapReport {
  std::vector<GapSpot> spots;  // the most seen first
  size_t viewpoints = 0, rays = 0, escaped = 0;
};

GapReport find_gaps(Scene& scene, const GapOptions& options);

}  // namespace ogle
