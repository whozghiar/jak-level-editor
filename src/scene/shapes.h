#pragma once

// The shapes made in the editor (Create menu): blocks, floors, walls, ramps, stairs, cylinders, to
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
  std::string kind = "box";  // box, ramp, stairs, cylinder (floors and walls are boxes)
  Vec3 size{4, 2, 4};        // meters: width (x), height (y), depth (z); a cylinder's x and z are its diameters
  float tile = 2.f;          // meters of the shape per repeat of its texture
  int steps = 8;             // stairs
  int sides = 16;            // cylinder

  json to_json() const;
  static ShapeParams from_json(const json& j);  // clamped to sensible values
};

// What the Create menu offers: a name ("box", "floor", "wall", "ramp", "stairs", "cylinder") and
// its starting size
struct ShapePreset {
  const char* name;
  ShapeParams params;
};
const std::vector<ShapePreset>& shape_presets();
const ShapePreset* shape_preset(const std::string& name);

// The shape's mesh, every face with `material`, its UVs in meters / tile
std::shared_ptr<Mesh> make_shape(const ShapeParams& p, int material);

// The node's shape, when it is one
bool node_shape(const Node& n, ShapeParams* out);

// The material of new shapes: a grey grid texture (one cell per repeat), added to the scene once
int shape_default_material(Scene& scene);

// A shape node's mesh made again from `p` (its extras too), with its material
void reshape(Scene& scene, Node& n, const ShapeParams& p);

// An image file (PNG, JPG, TGA, BMP) as a texture of the scene (the same image once); -1 when
// unreadable
int texture_from_image(Scene& scene, const std::string& path, std::string* error);

// Gives the Render nodes among `ids` that draw material `material` (-1: every material of theirs) a
// new material made by `edit` from it (an identical one is shared). Their meshes are copied (shared
// again between them): the other nodes drawing the material keep it, and a NodeChange made before
// undoes it. Returns how many nodes changed.
size_t edit_material(Scene& scene, const std::vector<uint32_t>& ids, int material,
                     const std::function<void(Material&)>& edit);

}  // namespace ogle
