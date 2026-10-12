#pragma once

// The shapes made in the editor (Build panel and menu): blocks, floors, walls, ramps, stairs, cylinders, to
// build what the game's decor has not. A shape keeps its kind and size in its node's extras
// (og_shape): the inspector changes them and the mesh is made again, its texture repeating every
// `tile` meters instead of stretching. Its origin is the middle of its base.

#include <functional>
#include <memory>
#include <string>
#include <vector>

#include "scene/scene.h"

namespace ogle {

struct ShapeParams {
  // box, ramp, stairs, cylinder (floors, walls and pillars are boxes); roof (two slopes meeting at
  // a ridge along z), roof-hip (four slopes); doorway and window (a wall with an opening); ground
  // (a floor with bumps)
  std::string kind = "box";
  Vec3 size{4, 2, 4};        // meters: width (x), height (y), depth (z); a cylinder's x and z are its diameters
  float tile = 2.f;          // meters of the shape per repeat of its texture
  int steps = 8;             // stairs
  int sides = 16;            // cylinder
  Vec2 opening{2.f, 3.f};    // doorway, window: the opening's width and height (meters)
  // bent around a vertical axis, in degrees over its length (along x; ramps and stairs along their
  // climb, z): a wall bent 90 makes a quarter of a round tower, stairs 180 turn half a turn
  float bend = 0.f;
  float round = 0.f;         // box: the radius of its upright edges (meters), rounded corners
  // ground: a slab whose top goes up and down by `bumps` meters, its bumps `bump_size` across
  // (a level's ground measured: scene/level_style.h); `seed` chooses them
  float bumps = 0.3f;
  float bump_size = 3.f;
  int seed = 1;
  Vec4 color{0, 0, 0, 0};    // its vertex colors (a level's light on it); w 0: none

  json to_json() const;
  static ShapeParams from_json(const json& j);  // clamped to sensible values
};

// What the Build panel and menu offer: a name ("box", "floor", "wall", "doorway", "window", "pillar", "ramp",
// "stairs", "roof", "roof-hip", "cylinder") and its starting size: walls of 4 m by 4 m, the grid of
// modular building (with the snap's 0.5 m)
struct ShapePreset {
  const char* name;
  ShapeParams params;
};
const std::vector<ShapePreset>& shape_presets();
const ShapePreset* shape_preset(const std::string& name);

// The shape's mesh, every face with `material`, its UVs in meters / tile
std::shared_ptr<Mesh> make_shape(const ShapeParams& p, int material);

// A picture in 3D of an object of the game that has no model (what the game draws of it is in the
// level's decor, or made of effects: lightning, vents, beams): what it does, for its thumbnail. By
// its type, else its family (platform, hazard, water...); vertex colors, no material.
// `seed`: other zigzags of its lightning (a preview draws it again and again: it crackles)
std::shared_ptr<Mesh> object_icon(const std::string& etype, const std::string& family, int seed = 0);

// The node's shape, when it is one
bool node_shape(const Node& n, ShapeParams* out);

// A smooth noise over the plane, -0.5..0.5, one bump per unit, the same for the same seed (the
// ground's bumps, the terrain brush's relief)
float smooth_noise(float x, float z, int seed);

// The material of new shapes: a grey grid texture (one cell per repeat), added to the scene once
int shape_default_material(Scene& scene);

// A shape node's mesh made again from `p` (its extras too), with its material
void reshape(Scene& scene, Node& n, const ShapeParams& p);

// An image file (PNG, JPG, TGA, BMP) as a texture of the scene (the same image once); -1 when
// unreadable
int texture_from_image(Scene& scene, const std::string& path, std::string* error);
// A texture of the scene: the one it has with the same name and image, else this one added
int add_texture_once(Scene& scene, Texture t);

// Gives the Render nodes among `ids` that draw material `material` (-1: every material of theirs) a
// new material made by `edit` from it (an identical one is shared). Their meshes are copied (shared
// again between them): the other nodes drawing the material keep it, and a NodeChange made before
// undoes it. Returns how many nodes changed.
size_t edit_material(Scene& scene, const std::vector<uint32_t>& ids, int material,
                     const std::function<void(Material&)>& edit);

}  // namespace ogle
