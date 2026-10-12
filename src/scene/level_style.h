#pragma once

// The look of a level's ground and walls, measured on its decor, to build in its style: the texture
// its floors draw most (and its walls), how many meters one repeat of it covers there, the light
// on it, and how uneven its ground is (the slums' worn cobbles, the desert's dunes, the industrial
// zone's flat slabs). The Build panel makes a floor (a "ground" shape, its bumps as the level's) and
// walls with them.

#include <string>

#include "scene/scene.h"

namespace ogle {

struct LevelStyle {
  struct Surface {
    int material = -1;    // in the level's scene
    std::string texture;  // its name
    float tile = 2.f;     // meters of the surface per repeat of its texture
    // the vertex colors on it (0.5: the texture as it is), not darker than 0.3 on its brightest
    // channel (a cave's floor keeps its hue, not its darkness)
    Vec4 color{0.5f, 0.5f, 0.5f, 1.f};
    double area = 0;      // square meters of it
  };
  Surface floor, wall;
  float bumps = 0.f;       // meters up and down of its ground
  float bump_size = 2.f;   // meters across a bump
  bool valid() const { return floor.material >= 0 || wall.material >= 0; }
};

// Measured on the decor of a level (its scene as the library has it); the ground under the water
// of its decor (a canal's bed) is not its floor
LevelStyle measure_style(Scene& level);

}  // namespace ogle
