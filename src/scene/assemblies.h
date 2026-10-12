#pragma once

// The decor of a level as the assemblies its artists built: the elements touching each other, a
// house with its facades, its roof, its windows and what hangs on them, a bridge with its pillars,
// a tower with its balconies. What selects a whole structure in one click and what the game's
// prefabs are made of (io/prefab_recipes.h).

#include <cstdint>
#include <vector>

namespace ogle {

class Scene;

// Every assembly of the scene's decor (the render nodes that are not the ground, an actor's model,
// the far scenery nor a reference level), the ones with the most elements first, each one's
// elements by id. Two elements link when their boxes touch, but:
// - what lies flat (a sidewalk, a roof, a floor plate) links only to what holds it, under it: a
//   house standing on a sidewalk is not linked to the next house by it;
// - what is long and thin (a pipe or a cable from a house to the next, a lamp post) and the
//   ground's details (dirt, pebbles, grass, decals...) do not link;
// they join the assembly whose box holds their middle (the smallest), else stay alone. In a city
// everything touches (the houses of a block side by side, a wall along a canal): an assembly wider
// than `max_size` meters is cut in two where the fewest elements cross (a street, the seam between
// two buildings), again until its parts fit.
std::vector<std::vector<uint32_t>> find_assemblies(Scene& scene, float max_size = 60.f);

// The assembly of `assemblies` holding the element `id` (empty when none does).
std::vector<uint32_t> assembly_of(const std::vector<std::vector<uint32_t>>& assemblies, uint32_t id);

}  // namespace ogle
