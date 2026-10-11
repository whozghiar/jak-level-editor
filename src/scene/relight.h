#pragma once

// The light of the level around elements, given to them. Jak 3's decor carries its light: 8
// palettes per vertex (the ambient light, the sun from 4 directions, the lamps), baked by the game's
// artists for each place. An element placed or moved elsewhere keeps the light of where it came
// from: too dark or too bright, too blue or too warm next to its new neighbors. Here each of its
// vertices takes the palettes of the level's vertices around it that face the same way (the nearer,
// the more); the element gets palettes of its own (TodPalette::local: the game view draws it with the
// new geometry, the export writes them) and its vertex colors (the editor's own view) from them.

#include <cstdint>
#include <vector>

#include "scene/scene.h"

namespace ogle {

// Marks a node lit where it stands: moved, it is lit again where it goes (the level's own pieces
// keep the light the game gave them unless asked).
constexpr const char* kLitHere = "og_lit_here";

// Lights the Render nodes among `ids` and under them (not the actors' models: the game lights them
// itself) like the level around them, and marks them kLitHere. Returns how many got it: a node with
// no lit vertex of the level within 16 m keeps its light, as does a selection too large to light at
// once (`max_vertices`).
size_t light_like_surroundings(Scene& scene, const std::vector<uint32_t>& ids, size_t max_vertices = 400000);

// The nodes among `ids` and under them that are marked kLitHere.
std::vector<uint32_t> lit_here_nodes(const Scene& scene, const std::vector<uint32_t>& ids);

}  // namespace ogle
