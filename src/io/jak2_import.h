#pragma once

// The structures of Jak II (buildings, the palace, pillars, cables: the decor parts of its levels)
// brought into Jak 3 levels. ogle-extract writes a Jak II disc to <library>/jak2/ with the same
// writer as Jak 3 (meters, the same axes, vertex colors, materials and collision layout), so a part
// keeps its geometry as it is; what could break is checked and converted when it comes in:
//
//   names      its textures are renamed "jak2-<name>" unless the document has the same image under
//              that name (the two games share most texture names with slightly different pixels:
//              one name for two images would make the level builder and texture packs mix them)
//   mesh       triangles, indices, texture coordinates and vertex colors where the game has them
//   lighting   its vertex colors (the brightest of its level's light palettes, 0.5 neutral) against
//              the Jak 3 decor of the document: more than a quarter brighter or darker is said, and
//              can be matched (its colors scaled)
//   materials  the alpha and blend modes the export writes for Jak 3 (blended: cut out)
//   textures   8-bit RGBA, power-of-two sizes up to 256 as the game's
//
// A part brought in has no actor, no story, no collision of its own (the level builder makes it
// from its shape) and keeps in its node extras og_import: {"game": "jak2", "level", "part",
// "checks": [{"check": "mesh" | "colors" | "lighting" | "materials" | "textures" | "metadata",
// "ok", and its numbers}]}.

#include <memory>
#include <string>
#include <vector>

#include "io/catalog.h"
#include "scene/scene.h"

namespace ogle {

// Whether a decor part of Jak II is one of its structures, to offer: not the low-detail scenery a
// level draws far away (its "-lowres" prototypes: mountains, the city seen from the palace).
bool jak2_structure(const CatalogPart& part);

// A Jak II mesh and its textures and materials made part of a Jak 3 document; `checks` gets what
// was checked (og_import's "checks").
std::shared_ptr<Mesh> import_jak2_mesh(Scene& doc, const Mesh& mesh, const std::vector<Texture>& textures,
                                       const std::vector<Material>& materials, json* checks);

// The mean brightness of the vertex colors of the document's decor that does not come from Jak II
// (0.5 is neutral), 0 when it has none.
float document_lighting(const Scene& doc);
// The mean brightness of a mesh's vertex colors, 0 when it has none.
float mesh_lighting(const Mesh& mesh);
// The lighting check: the part's brightness against the document's (its ratio, 1 when either has
// none), and whether it is within a quarter.
float lighting_ratio(const Mesh& part, const Scene& doc);
// A copy of the mesh with its vertex colors scaled by `k` (kept within 0..1).
std::shared_ptr<Mesh> scaled_colors(const Mesh& mesh, float k);

}  // namespace ogle
