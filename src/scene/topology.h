#pragma once

// Editing a mesh by its vertices and faces (the topology tool). A mesh keeps a vertex once per face
// or piece that draws it (its texture coordinates, its colors differ): the vertices at the same
// place make one point, which moves as one so that the surface never tears. Faces are the
// triangles of its pieces, (piece << 32 | triangle).

#include <cstdint>
#include <set>
#include <vector>

#include "scene/scene.h"

namespace ogle {

using MeshRef = uint64_t;  // (primitive << 32) | vertex or triangle
inline MeshRef mesh_ref(size_t prim, size_t index) { return ((uint64_t)prim << 32) | (uint32_t)index; }
inline size_t ref_prim(MeshRef r) { return (size_t)(r >> 32); }
inline uint32_t ref_index(MeshRef r) { return (uint32_t)r; }

// The points of a mesh: its vertices at the same place (within 0.1 mm) together
struct MeshPoints {
  std::vector<Vec3> pos;                    // per point
  std::vector<std::vector<MeshRef>> verts;  // per point: its vertices
};
MeshPoints mesh_points(const Mesh& mesh);

// The vertices of the faces `faces`
std::set<MeshRef> face_vertices(const Mesh& mesh, const std::set<MeshRef>& faces);

// Removes the faces `faces` (the vertices stay); returns how many went
size_t delete_faces(Mesh& mesh, const std::set<MeshRef>& faces);

// The faces using one of the vertices `verts`
std::set<MeshRef> faces_of_vertices(const Mesh& mesh, const std::set<MeshRef>& verts);

// Pulls the faces `faces` out by `distance` meters along their mean normal: new faces where they were
// moved, walls along their outer edges joining them to the rest. Returns the new faces (the ones
// moved), to keep working on.
std::set<MeshRef> extrude_faces(Mesh& mesh, const std::set<MeshRef>& faces, float distance);

// Cuts each face of `faces` in four (at the middle of its edges); returns the new faces
std::set<MeshRef> subdivide_faces(Mesh& mesh, const std::set<MeshRef>& faces);

// Turns the faces `faces` the other way (their front becomes their back)
void flip_faces(Mesh& mesh, const std::set<MeshRef>& faces);

// Every attribute of vertex v of p copied (or the middle of a and b) as a new vertex; its index
uint32_t copy_vertex(Primitive& p, uint32_t v);
uint32_t middle_vertex(Primitive& p, uint32_t a, uint32_t b);

}  // namespace ogle
