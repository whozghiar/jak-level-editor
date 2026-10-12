#pragma once

// Closing a structure (Edit > Close the building, MCP close_building): most buildings of the game
// are facades, made to be seen from the street only, open behind. Placed where the player can walk
// around them, their back shows the void or the insides of their walls. The closure is a volume
// made to fill them: their footprint and their height read on a grid of the ground plan, filled
// inside and behind them as seen from where the player sees them (`seen_from`: what is visible from
// there, a courtyard facing the street, stays open; the extracted game's triangles do not say which
// side of a wall is its front), as deep as a building of their width, walls where nothing closed
// it, a top under theirs. It follows their shape (as high as they are where they are, lower terrace
// by terrace going back), with relief: pilasters and a cornice on its new walls, in their own
// materials. The structure's surfaces stay in front of it.

#include <memory>
#include <string>
#include <vector>

#include "scene/scene.h"

namespace ogle {

struct ClosureOptions {
  float cell = 0.f;    // meters of the grid (0: about 1/48 of the structure's width, 0.5 at least)
  float depth = 0.f;   // meters filled behind a facade alone (0: from its width, 4 to 24)
  bool relief = true;  // setbacks, pilasters, a cornice
  int seed = 1;
  float tile = 2.f;    // meters per repeat of the textures
  int wall_material = -1, top_material = -1;  // -1: the structure's own (its most used walls, tops)
  // where the player sees it from (its front faces there): the camera, an agent's point of the
  // street; without it, from the side its faces mostly look to (or only its inside is filled)
  bool has_view = false;
  Vec3 seen_from{0, 0, 0};
};

struct ClosureResult {
  std::shared_ptr<Mesh> mesh;  // its vertices around `origin`, turned by `yaw`
  Vec3 origin{0, 0, 0};        // the middle of its base
  float yaw = 0.f;             // radians around the vertical: the structure's own axes (its walls)
  size_t cells = 0;            // of the grid filled
  size_t added = 0;            // of them where the structure had nothing (behind a facade, inside)
  float cell = 0.f;
  bool facade = false;         // filled behind it (seen from a side), not only inside
  Vec3 back{0, 0, 0};          // then: the way behind it
  int wall_material = -1, top_material = -1;
  std::string error;
};

ClosureResult close_structure(Scene& scene, const std::vector<uint32_t>& roots, const ClosureOptions& options);

}  // namespace ogle
