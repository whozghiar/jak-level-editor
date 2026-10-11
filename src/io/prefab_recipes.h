#pragma once

// The game's prefabs: ready-made assemblies of Jak 3's decor (a slum building with its fans and
// pipes, a bridge of the port, a floor vent of the industrial zone and its grate, a generator...),
// described by recipes (data/jak3/prefabs.json: no data of the game, only names) and made from the
// user's own extracted levels the first time they are needed, into <prefabs>/game/.
//
// A recipe names a level, one decor element of it (the anchor: a prototype, the index-th of its
// instances in the level, or a piece of its shell) and how far around it to take what the game put
// there (`margin` in meters, `grow`: more below and above, [lo, hi]): every element of the level's
// decor whose middle is in that box and which is not much larger than it, but the ground and the
// names `exclude` lists.

#include <string>
#include <vector>

#include "io/asset_library.h"
#include "scene/scene.h"

namespace ogle {

class GameData;

struct PrefabRecipe {
  std::string name;         // shown ("Slum house with fans")
  std::string category;     // building, house, bridge, platform, vent, generator, industrial...
  std::string description;  // what it is, where it comes from
  std::string level;        // the level it is taken from
  std::string anchor;       // a prototype ("x.mb") or a piece of the shell
  int index = 0;            // which instance of the prototype, in the level's order
  float margin = 1.f;       // meters around the anchor
  Vec3 grow_lo{0, 0, 0}, grow_hi{0, 0, 0};  // more of the box on each side (meters)
  // or a box of the level's space instead of an anchor (a whole block of buildings: its facades,
  // roofs and what hangs on them)
  AABB box;
  std::vector<std::string> exclude;          // parts of names left out
  std::string file;         // the prefab's file name ("<slug>.glb")
};

// The recipes of data/jak3/prefabs.json (next to the game's data).
std::vector<PrefabRecipe> load_prefab_recipes(const std::string& path, std::string* error);
std::string prefab_recipes_path();

// The elements of `level` (a document of it: opened without actors) a recipe takes: the anchor
// and what is around it.
std::vector<uint32_t> recipe_members(Scene& level, const PrefabRecipe& r, std::string* error);

// Makes a recipe's prefab at `path` from its level (opened without actors). `elements`: how many
// it holds.
bool save_recipe_prefab(Scene& level, const PrefabRecipe& r, const std::string& path, size_t* elements,
                        std::string* error);

}  // namespace ogle
