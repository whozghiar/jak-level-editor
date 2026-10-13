#pragma once

// A terrain drawn with the brush (Build panel > Terrain brush): a grid of square cells, the ground's
// height at each of their corners, which cells are ground and which are walls (a column of the
// cell, its height above the ground), kept in its node's extras (og_terrain_grid) and its mesh made
// again from them at each stroke. Each cell keeps the look it was painted with (a level's style,
// scene/level_style.h): what is flat draws its floor material, the slopes steeper than 45 degrees,
// the walls and the edges its wall material.

#include <memory>
#include <vector>

#include "scene/scene.h"

namespace ogle {

struct TerrainGrid {
  enum Cell : uint8_t { None = 0, Ground = 1, Wall = 2 };
  float cell = 1.f;           // meters across a cell
  int x0 = 0, z0 = 0;         // the first cell, counted from the node's origin (cell i covers x in [i, i + 1] * cell)
  int w = 0, d = 0;           // cells along x and z
  std::vector<float> height;  // (w + 1) * (d + 1) corners: the ground's height (meters)
  std::vector<uint8_t> cells; // w * d
  std::vector<float> wall;    // w * d: a wall cell's height above its ground (meters)
  struct Look {
    int floor_material = -1, wall_material = -1;  // in the scene; -1: the other one, or none
    float floor_tile = 2.f, wall_tile = 2.f;       // meters per repeat of their textures
    Vec4 color{0, 0, 0, 0};                        // vertex colors (a level's light); w 0: none
    bool operator==(const Look& o) const {
      return floor_material == o.floor_material && wall_material == o.wall_material && floor_tile == o.floor_tile &&
             wall_tile == o.wall_tile && color.x == o.color.x && color.y == o.color.y && color.z == o.color.z &&
             color.w == o.color.w;
    }
  };
  std::vector<Look> looks{Look{}};  // the looks painted with (64 at most)
  std::vector<uint8_t> look;        // w * d: a cell's, in `looks`

  bool empty() const;
  uint8_t at(int i, int k) const;          // a cell, None outside
  float corner(int i, int k) const;        // a corner's height, 0 outside
  bool corner_on_ground(int i, int k) const;  // one of the 4 cells around it is ground or wall
  void grow(int i0, int k0, int i1, int k1);  // holds the cells [i0, i1] x [k0, k1]
  int add_look(const Look& l);                // its index in `looks` (added when new)
  json to_json() const;
  static bool from_json(const json& j, TerrainGrid* out);
};

enum class BrushMode { Paint, Erase, Raise, Dig, Smooth, Flatten, Wall, Unwall };

// One application of the square brush, in the terrain's node space
struct BrushStroke {
  BrushMode mode = BrushMode::Paint;
  Vec3 center;
  float size = 4.f;    // meters: the square's side
  // raise, dig: meters at its middle; smooth, flatten: 0..1 of the way; wall: its height above the ground
  float amount = 0.f;
  float height = 0.f;  // paint, wall, flatten: the ground's height (where the stroke started)
  // paint: the relief of the new ground (a level's: its bumps up and down, their size)
  float bumps = 0.f, bump_size = 3.f;
  int seed = 1;
  int look = 0;  // paint, wall: the look given to the cells (in the grid's `looks`)
};

// Applies the brush: the cells whose middle is in the square, the corners in it (for raise, dig,
// smooth and flatten the most in its middle, fading to its edges; raise and dig lay ground first
// where there is none under its middle). Paint over ground gives it the stroke's look. Returns
// whether the terrain changed.
bool apply_brush(TerrainGrid& g, const BrushStroke& s);

// The cells of [i0, i1] x [k0, k1] that a change from `before` to `after` changed: laid, raised or
// dug, walled (drawn from `after`), or taken away (`removed`: drawn from `before`)
struct TerrainChange {
  int i, k;
  bool removed;
};
std::vector<TerrainChange> terrain_changes(const TerrainGrid& before, const TerrainGrid& after, int i0, int k0, int i1, int k1);

// Hills over a rectangle around `center` (x, z: `size` meters), made by a fractal noise: `hills`
// meters high at most, `roughness` 0..1 (smooth slopes to broken ground); its new ground painted with `look`
void generate_terrain(TerrainGrid& g, Vec3 center, Vec2 size, float hills, float roughness, int seed, int look = 0);

// The magnet brush: the top of the decor nearest to `at` (world space) beside it, not under it: of the
// surfaces facing up of the visible pieces (not props: 4 m2 of them at least; `skip` left out: the
// terrain painted), within `reach`
// meters across and `window` meters up or down, the one whose edge is nearest; `edge` gets the point
// of that edge nearest to `at`, at the decor's height there. False when there is none (or only
// decor under `at`).
bool nearest_decor_edge(const Scene& scene, Vec3 at, float reach, float window, uint32_t skip, Vec3* edge,
                        uint32_t* node = nullptr);
// The middle of a square of half side `half` moved from `at` so that its side towards `edge` goes
// `overlap` meters past it (along the axis of that side), its height the edge's
Vec3 magnet_center(Vec3 at, Vec3 edge, float half, float overlap);

// The terrain's height at (x, z) (bilinear over its cell); false where there is no ground
bool terrain_height(const TerrainGrid& g, float x, float z, float* y);

// Its mesh: the floor, the slopes and the walls, the sides of its edges down below its lowest point
std::shared_ptr<Mesh> make_terrain_mesh(const TerrainGrid& g);

// A terrain node's grid, when it is one
bool node_terrain(const Node& n, TerrainGrid* out);

}  // namespace ogle
