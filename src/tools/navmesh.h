#pragma once

// Navigation meshes: where the creatures of a level walk (the enemies and citizens of Jak II and
// Jak 3 follow them). Generated from the level's walkable surfaces with Recast: the surfaces are
// cut in voxels, kept where they are not too steep, where a creature has room above them and can
// step from one to the next, shrunk by the creature's radius, then outlined and cut in convex
// polygons of at most 4 corners. Then the limits of the game apply (the game's nav-mesh, see
// goal_src/jak2/engine/nav/nav-mesh-h.gc): at most 255 polygons per mesh, each mesh in one piece
// (its route table must reach every polygon), polygons facing up.

#include <cstdint>
#include <string>
#include <vector>

#include "core/math.h"
#include "scene/scene.h"

namespace ogle {

struct NavParams {
  float cell_size = 0.5f;     // m: the voxels' width
  float cell_height = 0.2f;   // m: their height
  float agent_height = 2.0f;  // m: the room a creature needs above the floor
  float agent_radius = 0.6f;  // m: the walls keep this far
  float max_climb = 0.8f;     // m: the highest step
  float max_slope = 45.f;     // degrees
  float min_region = 16.f;    // m²: walkable islands smaller than this are left out
  float merge_region = 60.f;  // m²: smaller regions join their neighbors
  float max_edge = 60.f;      // m: the longest polygon edge along walls
  float max_error = 0.8f;     // m: how far an outline may stray from the walls
  // when not empty, only the pieces of walkable ground next to one of these points are kept (the
  // streets where the creatures go; not the roofs, ledges and closed rooms)
  std::vector<Vec3> keep_from;
};

// A polygon: corners (indices in its mesh's vertices), and for each edge (corner i to i+1) the
// polygon across it in the same mesh, -1 at a border.
struct NavPoly {
  int count = 0;
  int v[4] = {0, 0, 0, 0};
  int adj[4] = {-1, -1, -1, -1};
};

struct NavMeshData {
  Vec3 origin;               // world position the vertices are relative to
  std::vector<Vec3> verts;   // meters, relative to origin
  std::vector<NavPoly> polys;
  float area() const;        // m², the polygons' surface
};

struct NavBuildStats {
  size_t triangles = 0;       // triangles of the level read
  int grid_width = 0, grid_depth = 0;
  size_t recast_polys = 0;    // polygons Recast made
  size_t meshes = 0, polys = 0;
  size_t split = 0;           // pieces over 255 polygons cut in several meshes
  size_t left_out = 0;        // pieces of ground away from the points of keep_from
  double ms = 0;
};

// A surface nothing walks on, left out of the nav meshes: a glow or a light shaft (added to or
// subtracted from what is behind), or water and dark eco as the editor draws them (translucent).
// The levels' own materials are blended with their texture's alpha almost everywhere: that alone
// does not make them see-through.
bool navmesh_ignores(const Material& m);

// The nav meshes of a triangle soup (world space, meters, Y up) inside `region`: one per piece of
// walkable ground, several for a piece over 255 polygons.
bool build_navmeshes(const std::vector<Vec3>& verts, const std::vector<uint32_t>& tris, const AABB& region,
                     const NavParams& params, std::vector<NavMeshData>* out, NavBuildStats* stats,
                     std::string* error);

// The route table of a mesh as the game reads it (nav-mesh route): 2 bits per (from, to) pair, bit
// index (to + from * count) * 2, the edge of `from` to cross towards `to` on the shortest path.
std::vector<uint8_t> navmesh_route_table(const NavMeshData& mesh);

// Checks a mesh against what the game needs; "" when it is fine.
std::string navmesh_problem(const NavMeshData& mesh);

// A nav mesh as a document node (NodeKind::NavMesh): its polygons in extras.og_navmesh, a fan of
// triangles as its mesh (bounds, picking), its origin as its position.
Node navmesh_node(const NavMeshData& mesh, const std::string& name);
// The polygons of such a node, relative to its origin (its transform is not applied).
bool navmesh_of_node(const Node& n, NavMeshData* out);

}  // namespace ogle
