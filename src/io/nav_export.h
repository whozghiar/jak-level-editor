#pragma once

// The nav meshes of a level for the game: entity-nav-mesh objects laid out as the game's levels
// have them (entity, nav-mesh, route table, grid-hash of the polygons, polygons, res tags), in the
// relocatable form OpenGOAL's level builder writes into a bsp: a DataBlob json (words, pointers,
// types, symbols, roots "nav-mesh-<n>"), the "nav_data" of a level (goalc/build_level/common/
// DataBlob.h). Distances in game units (4096 per meter).

#include <cstdint>
#include <string>
#include <vector>

#include "scene/scene.h"
#include "tools/navmesh.h"

namespace ogle {

struct NavExportEntry {
  const NavMeshData* mesh = nullptr;
  uint32_t aid = 0;   // the entity's id: unique in the game and not an actor's
  std::string name;   // its res 'name
};

// The grid-hash of a mesh's polygons (nav-mesh poly-hash), made like the game's: a box centered on
// the mesh's origin, about one cell per polygon (cells of at least 4 m), one layer; each cell holds
// the polygons whose box meets it, but those whose pat has bit 0. The same as the game's, byte for
// byte, for every nav mesh of the game's levels.
struct NavGrid {
  int dims[3] = {1, 1, 1};
  float axis_scale[3] = {0, 0, 0};
  float box_min[3] = {0, 0, 0}, box_max[3] = {0, 0, 0};
  int bucket_size = 1;   // bytes: one bit per polygon
  int bucket_count = 1;
  std::vector<uint8_t> buckets;
};
NavGrid navmesh_grid(const NavMeshData& mesh);

// The nav_data blob of these meshes. `nearest_y_threshold` (meters): how far above or below a
// polygon a point may be to be on it.
json navmesh_blob(const std::vector<NavExportEntry>& meshes, float nearest_y_threshold = 10.f, int max_users = 64);
bool write_navmesh_blob(const std::vector<NavExportEntry>& meshes, const std::string& path, std::string* error,
                        float nearest_y_threshold = 10.f, int max_users = 64);

// A blob read back (tests): words, pointers (word -> byte), roots.
struct NavBlobView {
  std::vector<uint32_t> words;
  std::vector<std::pair<int, int>> pointers;
  std::vector<std::pair<int, std::string>> types, symbols;
  std::vector<std::pair<std::string, int>> roots;
  uint32_t u32(int byte) const { return words.at(byte / 4); }
  uint8_t u8(int byte) const { return (uint8_t)(words.at(byte / 4) >> (8 * (byte % 4))); }
  float f32(int byte) const;
  int ptr(int byte) const;  // -1 when that word is no pointer
};
bool read_navmesh_blob(const json& j, NavBlobView* out);
// The meshes of a blob (the game's, from decompiler_out/<game>/entities/<level>-nav.json, or ours).
bool navmeshes_of_blob(const NavBlobView& blob, std::vector<NavMeshData>* out);

}  // namespace ogle
