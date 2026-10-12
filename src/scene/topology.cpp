#include "scene/topology.h"

#include <array>
#include <cmath>
#include <map>
#include <tuple>

namespace ogle {

MeshPoints mesh_points(const Mesh& mesh) {
  MeshPoints out;
  std::map<std::tuple<long long, long long, long long>, uint32_t> at;  // a position to 0.1 mm -> its point
  auto key = [](const Vec3& v) {
    return std::make_tuple(std::llround(v.x * 10000.0), std::llround(v.y * 10000.0), std::llround(v.z * 10000.0));
  };
  for (size_t pi = 0; pi < mesh.prims.size(); pi++) {
    const Primitive& p = mesh.prims[pi];
    for (size_t v = 0; v < p.pos.size(); v++) {
      auto [it, fresh] = at.emplace(key(p.pos[v]), (uint32_t)out.pos.size());
      if (fresh) {
        out.pos.push_back(p.pos[v]);
        out.verts.emplace_back();
      }
      out.verts[it->second].push_back(mesh_ref(pi, v));
    }
  }
  return out;
}

std::set<MeshRef> face_vertices(const Mesh& mesh, const std::set<MeshRef>& faces) {
  std::set<MeshRef> out;
  for (MeshRef f : faces) {
    const size_t pi = ref_prim(f), t = ref_index(f);
    if (pi >= mesh.prims.size() || t * 3 + 2 >= mesh.prims[pi].idx.size()) continue;
    for (int k = 0; k < 3; k++) out.insert(mesh_ref(pi, mesh.prims[pi].idx[t * 3 + k]));
  }
  return out;
}

std::set<MeshRef> faces_of_vertices(const Mesh& mesh, const std::set<MeshRef>& verts) {
  std::set<MeshRef> out;
  for (size_t pi = 0; pi < mesh.prims.size(); pi++) {
    const auto& idx = mesh.prims[pi].idx;
    for (size_t t = 0; t + 2 < idx.size(); t += 3)
      for (int k = 0; k < 3; k++)
        if (verts.count(mesh_ref(pi, idx[t + k]))) {
          out.insert(mesh_ref(pi, t / 3));
          break;
        }
  }
  return out;
}

size_t delete_faces(Mesh& mesh, const std::set<MeshRef>& faces) {
  size_t gone = 0;
  for (size_t pi = 0; pi < mesh.prims.size(); pi++) {
    Primitive& p = mesh.prims[pi];
    std::vector<uint32_t> kept;
    std::vector<uint32_t> pats;
    for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
      if (faces.count(mesh_ref(pi, t / 3))) {
        gone++;
        continue;
      }
      kept.insert(kept.end(), {p.idx[t], p.idx[t + 1], p.idx[t + 2]});
      if (t / 3 < p.tri_pat.size()) pats.push_back(p.tri_pat[t / 3]);
    }
    p.idx = std::move(kept);
    if (!p.tri_pat.empty()) p.tri_pat = std::move(pats);
  }
  mesh.touch();
  return gone;
}

uint32_t copy_vertex(Primitive& p, uint32_t v) {
  const uint32_t n = (uint32_t)p.pos.size();
  p.pos.push_back(p.pos[v]);
  if (p.nrm.size() == n) p.nrm.push_back(p.nrm[v]);
  if (p.uv.size() == n) p.uv.push_back(p.uv[v]);
  if (p.col.size() == n) p.col.push_back(p.col[v]);
  for (auto& [name, a] : p.extra_attrs)
    if (a.size() == n) a.push_back(a[v]);
  return n;
}

uint32_t middle_vertex(Primitive& p, uint32_t a, uint32_t b) {
  const uint32_t n = (uint32_t)p.pos.size();
  p.pos.push_back((p.pos[a] + p.pos[b]) * 0.5f);
  if (p.nrm.size() == n) p.nrm.push_back(normalize(p.nrm[a] + p.nrm[b]));
  if (p.uv.size() == n) p.uv.push_back((p.uv[a] + p.uv[b]) * 0.5f);
  if (p.col.size() == n) p.col.push_back((p.col[a] + p.col[b]) * 0.5f);
  for (auto& [name, x] : p.extra_attrs)
    if (x.size() == n) x.push_back(x[a]);  // palettes, joints: the first one's (they are indices or bytes)
  return n;
}

std::set<MeshRef> extrude_faces(Mesh& mesh, const std::set<MeshRef>& faces, float distance) {
  std::set<MeshRef> out;
  // their mean normal
  Vec3 normal{0, 0, 0};
  for (MeshRef f : faces) {
    const size_t pi = ref_prim(f), t = ref_index(f);
    if (pi >= mesh.prims.size() || t * 3 + 2 >= mesh.prims[pi].idx.size()) continue;
    const Primitive& p = mesh.prims[pi];
    const Vec3 a = p.pos[p.idx[t * 3]], b = p.pos[p.idx[t * 3 + 1]], c = p.pos[p.idx[t * 3 + 2]];
    normal = normal + cross(b - a, c - a);
  }
  if (length(normal) < 1e-9f) return out;
  const Vec3 shift = normalize(normal) * distance;
  for (size_t pi = 0; pi < mesh.prims.size(); pi++) {
    Primitive& p = mesh.prims[pi];
    std::vector<size_t> tris;
    for (size_t t = 0; t * 3 + 2 < p.idx.size(); t++)
      if (faces.count(mesh_ref(pi, t))) tris.push_back(t);
    if (tris.empty()) continue;
    // the edges of the faces by their points: the outer ones belong to one face of them
    auto pkey = [&](uint32_t v) {
      const Vec3& q = p.pos[v];
      return std::make_tuple(std::lround(q.x * 10000.f), std::lround(q.y * 10000.f), std::lround(q.z * 10000.f));
    };
    using P = std::tuple<long, long, long>;
    std::map<std::pair<P, P>, int> edge_count;
    for (size_t t : tris)
      for (int k = 0; k < 3; k++) {
        P a = pkey(p.idx[t * 3 + k]), b = pkey(p.idx[t * 3 + (k + 1) % 3]);
        edge_count[a < b ? std::make_pair(a, b) : std::make_pair(b, a)]++;
      }
    // the faces moved: their vertices copied once, and moved
    std::map<uint32_t, uint32_t> moved;
    auto moved_of = [&](uint32_t v) {
      auto it = moved.find(v);
      if (it != moved.end()) return it->second;
      const uint32_t c = copy_vertex(p, v);
      p.pos[c] = p.pos[c] + shift;
      return moved[v] = c;
    };
    const size_t first_new = p.idx.size() / 3;
    std::vector<std::array<uint32_t, 2>> walls;  // outer edges, in the faces' order
    for (size_t t : tris) {
      uint32_t old[3] = {p.idx[t * 3], p.idx[t * 3 + 1], p.idx[t * 3 + 2]};
      for (int k = 0; k < 3; k++) {
        P a = pkey(old[k]), b = pkey(old[(k + 1) % 3]);
        if (edge_count[a < b ? std::make_pair(a, b) : std::make_pair(b, a)] == 1) walls.push_back({old[k], old[(k + 1) % 3]});
      }
      for (int k = 0; k < 3; k++) p.idx[t * 3 + k] = moved_of(old[k]);
      out.insert(mesh_ref(pi, t));
    }
    // a wall along each outer edge: from the edge where it was to where it is (its own vertices:
    // its texture and normal are its own)
    for (const auto& e : walls) {
      const uint32_t a = copy_vertex(p, e[0]), b = copy_vertex(p, e[1]);
      const uint32_t c = copy_vertex(p, moved_of(e[1])), d = copy_vertex(p, moved_of(e[0]));
      const Vec3 n = normalize(cross(p.pos[b] - p.pos[a], shift));
      for (uint32_t v : {a, b, c, d})
        if (v < p.nrm.size()) p.nrm[v] = n;
      p.idx.insert(p.idx.end(), {a, b, c, a, c, d});
      if (!p.tri_pat.empty()) p.tri_pat.insert(p.tri_pat.end(), 2, p.tri_pat[0]);
    }
    (void)first_new;
  }
  mesh.touch();
  return out;
}

std::set<MeshRef> subdivide_faces(Mesh& mesh, const std::set<MeshRef>& faces) {
  std::set<MeshRef> out;
  for (size_t pi = 0; pi < mesh.prims.size(); pi++) {
    Primitive& p = mesh.prims[pi];
    const size_t count = p.idx.size() / 3;
    std::map<std::pair<uint32_t, uint32_t>, uint32_t> middles;  // an edge's middle, made once
    auto middle = [&](uint32_t a, uint32_t b) {
      const auto k = a < b ? std::make_pair(a, b) : std::make_pair(b, a);
      auto it = middles.find(k);
      if (it != middles.end()) return it->second;
      return middles[k] = middle_vertex(p, a, b);
    };
    for (size_t t = 0; t < count; t++) {
      if (!faces.count(mesh_ref(pi, t))) continue;
      const uint32_t a = p.idx[t * 3], b = p.idx[t * 3 + 1], c = p.idx[t * 3 + 2];
      const uint32_t ab = middle(a, b), bc = middle(b, c), ca = middle(c, a);
      p.idx[t * 3 + 1] = ab;
      p.idx[t * 3 + 2] = ca;
      out.insert(mesh_ref(pi, t));
      for (const auto& tri : {std::array<uint32_t, 3>{ab, b, bc}, {ca, bc, c}, {ab, bc, ca}}) {
        out.insert(mesh_ref(pi, p.idx.size() / 3));
        p.idx.insert(p.idx.end(), tri.begin(), tri.end());
        if (t < p.tri_pat.size()) p.tri_pat.push_back(p.tri_pat[t]);
      }
    }
  }
  mesh.touch();
  return out;
}

void flip_faces(Mesh& mesh, const std::set<MeshRef>& faces) {
  for (MeshRef f : faces) {
    const size_t pi = ref_prim(f), t = ref_index(f);
    if (pi >= mesh.prims.size() || t * 3 + 2 >= mesh.prims[pi].idx.size()) continue;
    Primitive& p = mesh.prims[pi];
    std::swap(p.idx[t * 3 + 1], p.idx[t * 3 + 2]);
    for (int k = 0; k < 3; k++) {
      const uint32_t v = p.idx[t * 3 + k];
      if (v < p.nrm.size()) p.nrm[v] = p.nrm[v] * -1.f;
    }
  }
  mesh.touch();
}

}  // namespace ogle
