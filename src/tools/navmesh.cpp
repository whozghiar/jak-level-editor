#include "tools/navmesh.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <memory>
#include <queue>

#include "Recast.h"

namespace ogle {

namespace {

constexpr int kMaxPolys = 255;  // the game's poly-count is a byte, 255 marks "no polygon"

// Twice the signed area of a polygon seen from above (x, z). The game wants its polygons the way
// round where (v1 - v0) x (v2 - v0) points up, which is a negative value here.
float signed_area2(const std::vector<Vec3>& verts, const NavPoly& p) {
  float s = 0;
  for (int i = 0; i < p.count; i++) {
    const Vec3& a = verts[p.v[i]];
    const Vec3& b = verts[p.v[(i + 1) % p.count]];
    s += a.x * b.z - b.x * a.z;
  }
  return s;
}

// The game's own test (nav-mesh initialize-mesh!): the normal of the first three corners.
float first_normal_y(const std::vector<Vec3>& verts, const NavPoly& p) {
  const Vec3 a = verts[p.v[1]] - verts[p.v[0]];
  const Vec3 c = verts[p.v[2]] - verts[p.v[0]];
  return a.z * c.x - a.x * c.z;
}

void reverse_poly(NavPoly& p) {
  const int n = p.count;
  NavPoly r = p;
  for (int k = 0; k < n; k++) {
    r.v[k] = p.v[n - 1 - k];
    // edge k of the reversed polygon is edge n-2-k of the original, the other way
    r.adj[k] = p.adj[(n - 2 - k + n) % n];
  }
  p = r;
}

void rotate_poly(NavPoly& p, int s) {
  NavPoly r = p;
  for (int k = 0; k < p.count; k++) {
    r.v[k] = p.v[(k + s) % p.count];
    r.adj[k] = p.adj[(k + s) % p.count];
  }
  p = r;
}

Vec3 centroid(const std::vector<Vec3>& verts, const NavPoly& p) {
  Vec3 c{0, 0, 0};
  for (int i = 0; i < p.count; i++) c += verts[p.v[i]];
  return c / (float)p.count;
}

// How far `p` is from the polygon, across (x, z): 0 inside.
float distance_xz(const std::vector<Vec3>& verts, const NavPoly& q, const Vec3& p) {
  bool inside_cw = true, inside_ccw = true;
  float best = 1e30f;
  for (int k = 0; k < q.count; k++) {
    const Vec3& a = verts[q.v[k]];
    const Vec3& b = verts[q.v[(k + 1) % q.count]];
    const float ex = b.x - a.x, ez = b.z - a.z, px = p.x - a.x, pz = p.z - a.z;
    const float cross = ex * pz - ez * px;
    inside_cw = inside_cw && cross <= 0;
    inside_ccw = inside_ccw && cross >= 0;
    const float len2 = ex * ex + ez * ez;
    const float t = len2 > 0 ? std::clamp((px * ex + pz * ez) / len2, 0.f, 1.f) : 0.f;
    best = std::min(best, std::hypot(px - t * ex, pz - t * ez));
  }
  return inside_cw || inside_ccw ? 0.f : best;
}

// The connected groups of `ids` (polygons of `polys`), through the adjacencies inside `ids`.
std::vector<std::vector<int>> components(const std::vector<NavPoly>& polys, const std::vector<int>& ids) {
  std::vector<int> group(polys.size(), -2);
  for (int id : ids) group[id] = -1;
  std::vector<std::vector<int>> out;
  for (int start : ids) {
    if (group[start] != -1) continue;
    const int g = (int)out.size();
    out.emplace_back();
    std::vector<int> stack{start};
    group[start] = g;
    while (!stack.empty()) {
      const int i = stack.back();
      stack.pop_back();
      out[g].push_back(i);
      for (int k = 0; k < polys[i].count; k++) {
        const int j = polys[i].adj[k];
        if (j >= 0 && group[j] == -1) {
          group[j] = g;
          stack.push_back(j);
        }
      }
    }
  }
  return out;
}

struct RecastScratch {
  rcHeightfield* hf = nullptr;
  rcCompactHeightfield* chf = nullptr;
  rcContourSet* cset = nullptr;
  rcPolyMesh* pmesh = nullptr;
  ~RecastScratch() {
    rcFreeHeightField(hf);
    rcFreeCompactHeightfield(chf);
    rcFreeContourSet(cset);
    rcFreePolyMesh(pmesh);
  }
};

}  // namespace

float NavMeshData::area() const {
  float a = 0;
  for (const auto& p : polys)
    for (int i = 1; i + 1 < p.count; i++)
      a += 0.5f * length(cross(verts[p.v[i]] - verts[p.v[0]], verts[p.v[i + 1]] - verts[p.v[0]]));
  return a;
}

bool build_navmeshes(const std::vector<Vec3>& verts, const std::vector<uint32_t>& tris, const AABB& region,
                     const NavParams& params, std::vector<NavMeshData>* out, NavBuildStats* stats,
                     std::string* error) {
  const auto t0 = std::chrono::steady_clock::now();
  NavBuildStats st;
  out->clear();
  if (!region.valid()) {
    if (error) *error = "empty region";
    return false;
  }
  // the triangles that reach the region (with room for the creature around it)
  const AABB reach = region.expanded(params.agent_radius + params.cell_size);
  std::vector<float> fv;
  std::vector<int> ft;
  std::vector<int> remap(verts.size(), -1);
  AABB used;
  for (size_t t = 0; t + 2 < tris.size(); t += 3) {
    AABB tb;
    for (int k = 0; k < 3; k++) tb.add(verts[tris[t + k]]);
    if (tb.hi.x < reach.lo.x || tb.lo.x > reach.hi.x || tb.hi.z < reach.lo.z || tb.lo.z > reach.hi.z ||
        tb.hi.y < reach.lo.y || tb.lo.y > reach.hi.y)
      continue;
    for (int k = 0; k < 3; k++) {
      const uint32_t v = tris[t + k];
      if (remap[v] < 0) {
        remap[v] = (int)(fv.size() / 3);
        fv.insert(fv.end(), {verts[v].x, verts[v].y, verts[v].z});
      }
      ft.push_back(remap[v]);
    }
    used.add(tb);
  }
  st.triangles = ft.size() / 3;
  if (ft.empty()) {
    if (error) *error = "no surface in the region";
    return false;
  }
  // the grid: the region where there is something, a little wider than the creature needs
  float bmin[3] = {std::max(reach.lo.x, used.lo.x), std::max(reach.lo.y, used.lo.y) - 1.f,
                   std::max(reach.lo.z, used.lo.z)};
  float bmax[3] = {std::min(reach.hi.x, used.hi.x), std::min(reach.hi.y, used.hi.y) + params.agent_height,
                   std::min(reach.hi.z, used.hi.z)};
  float cs = std::max(0.05f, params.cell_size);
  const float ch = std::max(0.05f, params.cell_height);
  int w = 0, d = 0;
  rcCalcGridSize(bmin, bmax, cs, &w, &d);
  // a very large region: coarser voxels (Recast's spans would not fit in memory)
  while ((double)w * d > 16e6) {
    cs *= 1.25f;
    rcCalcGridSize(bmin, bmax, cs, &w, &d);
  }
  st.grid_width = w;
  st.grid_depth = d;

  rcConfig cfg{};
  cfg.cs = cs;
  cfg.ch = ch;
  cfg.width = w;
  cfg.height = d;
  cfg.walkableSlopeAngle = params.max_slope;
  cfg.walkableHeight = (int)std::ceil(params.agent_height / ch);
  cfg.walkableClimb = (int)std::floor(params.max_climb / ch);
  cfg.walkableRadius = (int)std::ceil(params.agent_radius / cs);
  cfg.maxEdgeLen = (int)(params.max_edge / cs);
  cfg.maxSimplificationError = params.max_error;
  cfg.minRegionArea = (int)std::ceil(params.min_region / (cs * cs));
  cfg.mergeRegionArea = (int)std::ceil(params.merge_region / (cs * cs));
  cfg.maxVertsPerPoly = 4;
  rcVcopy(cfg.bmin, bmin);
  rcVcopy(cfg.bmax, bmax);

  rcContext ctx(false);
  RecastScratch r;
  const int nverts = (int)(fv.size() / 3), ntris = (int)(ft.size() / 3);
  r.hf = rcAllocHeightfield();
  if (!r.hf || !rcCreateHeightfield(&ctx, *r.hf, w, d, cfg.bmin, cfg.bmax, cfg.cs, cfg.ch)) {
    if (error) *error = "not enough memory for the voxels";
    return false;
  }
  // walkable: not steeper than the slope, whichever way the triangle is wound (the game's meshes do
  // not always face up)
  std::vector<unsigned char> areas(ntris, 0);
  const float min_up = std::cos(params.max_slope * kPi / 180.f);
  for (int i = 0; i < ntris; i++) {
    const float* a = &fv[ft[i * 3] * 3];
    const float* b = &fv[ft[i * 3 + 1] * 3];
    const float* c = &fv[ft[i * 3 + 2] * 3];
    const Vec3 n = cross(Vec3{b[0] - a[0], b[1] - a[1], b[2] - a[2]}, Vec3{c[0] - a[0], c[1] - a[1], c[2] - a[2]});
    const float len = length(n);
    if (len > 0 && std::fabs(n.y) / len >= min_up) areas[i] = RC_WALKABLE_AREA;
  }
  if (!rcRasterizeTriangles(&ctx, fv.data(), nverts, ft.data(), areas.data(), ntris, *r.hf, cfg.walkableClimb)) {
    if (error) *error = "the surfaces could not be cut in voxels";
    return false;
  }
  // steps, ledges, and floors without head room are not walkable
  rcFilterLowHangingWalkableObstacles(&ctx, cfg.walkableClimb, *r.hf);
  rcFilterLedgeSpans(&ctx, cfg.walkableHeight, cfg.walkableClimb, *r.hf);
  rcFilterWalkableLowHeightSpans(&ctx, cfg.walkableHeight, *r.hf);
  r.chf = rcAllocCompactHeightfield();
  if (!r.chf || !rcBuildCompactHeightfield(&ctx, cfg.walkableHeight, cfg.walkableClimb, *r.hf, *r.chf) ||
      !rcErodeWalkableArea(&ctx, cfg.walkableRadius, *r.chf) || !rcBuildDistanceField(&ctx, *r.chf) ||
      !rcBuildRegions(&ctx, *r.chf, 0, cfg.minRegionArea, cfg.mergeRegionArea)) {
    if (error) *error = "the walkable regions could not be built";
    return false;
  }
  rcFreeHeightField(r.hf);
  r.hf = nullptr;
  r.cset = rcAllocContourSet();
  if (!r.cset || !rcBuildContours(&ctx, *r.chf, cfg.maxSimplificationError, cfg.maxEdgeLen, *r.cset)) {
    if (error) *error = "the outlines of the walkable regions could not be built";
    return false;
  }
  r.pmesh = rcAllocPolyMesh();
  if (!r.pmesh || !rcBuildPolyMesh(&ctx, *r.cset, cfg.maxVertsPerPoly, *r.pmesh)) {
    if (error) *error = "the polygons could not be built";
    return false;
  }
  const rcPolyMesh& pm = *r.pmesh;
  st.recast_polys = (size_t)pm.npolys;

  // Recast's polygons, in meters
  std::vector<Vec3> all_verts(pm.nverts);
  for (int i = 0; i < pm.nverts; i++) {
    const unsigned short* v = &pm.verts[i * 3];
    all_verts[i] = {pm.bmin[0] + v[0] * pm.cs, pm.bmin[1] + v[1] * pm.ch, pm.bmin[2] + v[2] * pm.cs};
  }
  std::vector<NavPoly> all(pm.npolys);
  std::vector<bool> dead(pm.npolys, false);
  for (int i = 0; i < pm.npolys; i++) {
    const unsigned short* p = &pm.polys[i * 2 * pm.nvp];
    NavPoly& q = all[i];
    for (int j = 0; j < pm.nvp && p[j] != RC_MESH_NULL_IDX; j++) {
      q.v[q.count] = p[j];
      const unsigned short nb = p[pm.nvp + j];
      q.adj[q.count] = (nb == RC_MESH_NULL_IDX || (nb & 0x8000)) ? -1 : nb;
      q.count++;
    }
    if (q.count < 3 || std::fabs(signed_area2(all_verts, q)) < 1e-4f) dead[i] = true;
  }
  for (int i = 0; i < pm.npolys; i++) {
    NavPoly& q = all[i];
    if (dead[i]) continue;
    for (int k = 0; k < q.count; k++)
      if (q.adj[k] >= 0 && dead[q.adj[k]]) q.adj[k] = -1;
    // facing up as the game counts it, and not starting on three corners in a line
    if (signed_area2(all_verts, q) > 0) reverse_poly(q);
    for (int s = 0; s < q.count && std::fabs(first_normal_y(all_verts, q)) < 1e-6f; s++) rotate_poly(q, 1);
  }

  // the pieces: connected groups, cut in halves (along their longest side) until each has at most
  // 255 polygons
  std::vector<int> alive;
  for (int i = 0; i < pm.npolys; i++)
    if (!dead[i]) alive.push_back(i);
  std::vector<std::vector<int>> todo = components(all, alive), pieces;
  if (!params.keep_from.empty()) {
    // the pieces next to a point: the polygon nearest to it across, at its height (a point on an
    // element standing on the ground is over a hole of the nav mesh, the ground around is near)
    constexpr float kReach = 12.f, kHeight = 3.f;
    std::vector<int> piece_of(all.size(), -1);
    for (size_t g = 0; g < todo.size(); g++)
      for (int id : todo[g]) piece_of[id] = (int)g;
    std::vector<bool> reached(todo.size(), false);
    for (const Vec3& p : params.keep_from) {
      int nearest = -1;
      float nearest_d = kReach;
      for (int id : alive) {
        const NavPoly& q = all[id];
        float lo = 1e30f, hi = -1e30f;
        for (int k = 0; k < q.count; k++) lo = std::min(lo, all_verts[q.v[k]].y), hi = std::max(hi, all_verts[q.v[k]].y);
        if (p.y < lo - kHeight || p.y > hi + kHeight) continue;
        const float d = distance_xz(all_verts, q, p);
        if (d < nearest_d) nearest = id, nearest_d = d;
      }
      if (nearest >= 0) reached[piece_of[nearest]] = true;
    }
    std::vector<std::vector<int>> kept;
    for (size_t g = 0; g < todo.size(); g++) {
      if (reached[g]) kept.push_back(std::move(todo[g]));
      else st.left_out++;
    }
    todo = std::move(kept);
    if (todo.empty()) {
      if (error) *error = "no walkable ground next to the points given";
      return false;
    }
  }
  while (!todo.empty()) {
    std::vector<int> ids = std::move(todo.back());
    todo.pop_back();
    if ((int)ids.size() <= kMaxPolys) {
      pieces.push_back(std::move(ids));
      continue;
    }
    st.split++;
    AABB cb;
    for (int id : ids) cb.add(centroid(all_verts, all[id]));
    const bool along_x = cb.size().x >= cb.size().z;
    std::sort(ids.begin(), ids.end(), [&](int a, int b) {
      const Vec3 ca = centroid(all_verts, all[a]), cb2 = centroid(all_verts, all[b]);
      return along_x ? ca.x < cb2.x : ca.z < cb2.z;
    });
    const size_t half = ids.size() / 2;
    std::vector<int> lo(ids.begin(), ids.begin() + half), hi(ids.begin() + half, ids.end());
    for (auto* side : {&lo, &hi})
      for (auto& g : components(all, *side)) todo.push_back(std::move(g));
  }
  // the cuts leave slivers behind: a small piece joins the smallest neighbor it touches when both
  // fit in one mesh (touching, they stay in one piece)
  if (st.split) {
    constexpr size_t kSliver = 64;
    std::vector<int> piece_of(all.size(), -1);
    for (size_t g = 0; g < pieces.size(); g++)
      for (int id : pieces[g]) piece_of[id] = (int)g;
    for (bool merged = true; merged;) {
      merged = false;
      for (size_t g = 0; g < pieces.size(); g++) {
        if (pieces[g].empty() || pieces[g].size() > kSliver) continue;
        int best = -1;
        for (int id : pieces[g])
          for (int k = 0; k < all[id].count; k++) {
            const int nb = all[id].adj[k];
            const int h = nb >= 0 ? piece_of[nb] : -1;
            if (h < 0 || h == (int)g || pieces[h].size() + pieces[g].size() > (size_t)kMaxPolys) continue;
            if (best < 0 || pieces[h].size() < pieces[best].size()) best = h;
          }
        if (best < 0) continue;
        for (int id : pieces[g]) piece_of[id] = best;
        pieces[best].insert(pieces[best].end(), pieces[g].begin(), pieces[g].end());
        pieces[g].clear();
        merged = true;
      }
    }
    pieces.erase(std::remove_if(pieces.begin(), pieces.end(), [](const auto& p) { return p.empty(); }), pieces.end());
  }
  // largest first, like the game's levels list their meshes
  std::sort(pieces.begin(), pieces.end(), [](const auto& a, const auto& b) { return a.size() > b.size(); });

  for (const auto& ids : pieces) {
    NavMeshData m;
    std::vector<int> local_poly(all.size(), -1), local_vert(all_verts.size(), -1);
    for (int id : ids) local_poly[id] = (int)m.polys.size(), m.polys.push_back(all[id]);
    AABB vb;
    for (auto& p : m.polys) {
      for (int k = 0; k < p.count; k++) {
        int& lv = local_vert[p.v[k]];
        if (lv < 0) {
          lv = (int)m.verts.size();
          m.verts.push_back(all_verts[p.v[k]]);
          vb.add(all_verts[p.v[k]]);
        }
        p.v[k] = lv;
        p.adj[k] = p.adj[k] >= 0 ? local_poly[p.adj[k]] : -1;
      }
    }
    // the origin is the center of its corners, as in the game's meshes
    m.origin = vb.center();
    for (auto& v : m.verts) v = v - m.origin;
    st.polys += m.polys.size();
    out->push_back(std::move(m));
  }
  st.meshes = out->size();
  st.ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
  if (stats) *stats = st;
  if (out->empty()) {
    if (error) *error = "no walkable ground (the creature does not fit anywhere: try a smaller radius or height)";
    return false;
  }
  return true;
}

std::vector<uint8_t> navmesh_route_table(const NavMeshData& mesh) {
  const int n = (int)mesh.polys.size();
  std::vector<uint8_t> table(((size_t)n * n * 2 + 7) / 8, 0);
  std::vector<Vec3> center(n);
  for (int i = 0; i < n; i++) center[i] = centroid(mesh.verts, mesh.polys[i]);
  // the length of a step from a polygon to its neighbor: through the middle of their edge
  auto step = [&](int i, int k) {
    const NavPoly& p = mesh.polys[i];
    const Vec3 mid = (mesh.verts[p.v[k]] + mesh.verts[p.v[(k + 1) % p.count]]) * 0.5f;
    return length(mid - center[i]) + length(center[p.adj[k]] - mid);
  };
  std::vector<float> dist(n);
  for (int t = 0; t < n; t++) {
    // shortest distances to t (the graph is the same both ways)
    std::fill(dist.begin(), dist.end(), std::numeric_limits<float>::infinity());
    using Item = std::pair<float, int>;
    std::priority_queue<Item, std::vector<Item>, std::greater<Item>> open;
    dist[t] = 0;
    open.push({0.f, t});
    while (!open.empty()) {
      auto [dd, i] = open.top();
      open.pop();
      if (dd > dist[i]) continue;
      const NavPoly& p = mesh.polys[i];
      for (int k = 0; k < p.count; k++) {
        const int j = p.adj[k];
        if (j < 0) continue;
        const float nd = dd + step(i, k);
        if (nd < dist[j]) {
          dist[j] = nd;
          open.push({nd, j});
        }
      }
    }
    for (int s = 0; s < n; s++) {
      if (s == t) continue;
      const NavPoly& p = mesh.polys[s];
      int best = 0;
      float best_d = std::numeric_limits<float>::infinity();
      for (int k = 0; k < p.count; k++) {
        if (p.adj[k] < 0) continue;
        const float dk = step(s, k) + dist[p.adj[k]];
        if (dk < best_d) {
          best_d = dk;
          best = k;
        }
      }
      const size_t bit = ((size_t)t + (size_t)s * n) * 2;
      table[bit >> 3] |= (uint8_t)(best << (bit & 7));
    }
  }
  return table;
}

std::string navmesh_problem(const NavMeshData& mesh) {
  const int n = (int)mesh.polys.size();
  if (n == 0) return "no polygon";
  if (n > kMaxPolys) return "more than 255 polygons";
  for (int i = 0; i < n; i++) {
    const NavPoly& p = mesh.polys[i];
    if (p.count < 3 || p.count > 4) return "a polygon without 3 or 4 corners";
    if (first_normal_y(mesh.verts, p) <= 0) return "a polygon facing down or without area";
    for (int k = 0; k < p.count; k++) {
      // convex: every turn the same way
      const Vec3 a = mesh.verts[p.v[k]], b = mesh.verts[p.v[(k + 1) % p.count]], c = mesh.verts[p.v[(k + 2) % p.count]];
      const float turn = (b.x - a.x) * (c.z - b.z) - (b.z - a.z) * (c.x - b.x);
      const float scale = std::hypot(b.x - a.x, b.z - a.z) * std::hypot(c.x - b.x, c.z - b.z);
      if (turn > 1e-4f * scale + 1e-6f) return "a polygon that is not convex";
      const int j = p.adj[k];
      if (j < 0) continue;
      if (j >= n) return "an adjacency outside the mesh";
      const NavPoly& q = mesh.polys[j];
      bool back = false;
      for (int e = 0; e < q.count; e++)
        back |= q.adj[e] == i && q.v[e] == p.v[(k + 1) % p.count] && q.v[(e + 1) % q.count] == p.v[k];
      if (!back) return "two polygons that do not share their edge both ways";
    }
  }
  std::vector<int> ids(n);
  for (int i = 0; i < n; i++) ids[i] = i;
  if (components(mesh.polys, ids).size() != 1) return "polygons that cannot reach each other";
  return {};
}

Node navmesh_node(const NavMeshData& mesh, const std::string& name) {
  Node n;
  n.name = name;
  n.kind = NodeKind::NavMesh;
  n.local.t = mesh.origin;
  auto m = std::make_shared<Mesh>();
  m->name = name;
  Primitive prim;
  prim.pos = mesh.verts;
  prim.nrm.assign(mesh.verts.size(), Vec3{0, 1, 0});
  json verts = json::array(), polys = json::array(), adj = json::array();
  for (const auto& v : mesh.verts) verts.push_back({v.x, v.y, v.z});
  for (const auto& p : mesh.polys) {
    json pv = json::array(), pa = json::array();
    for (int k = 0; k < p.count; k++) {
      pv.push_back(p.v[k]);
      pa.push_back(p.adj[k]);
    }
    polys.push_back(pv);
    adj.push_back(pa);
    for (int k = 1; k + 1 < p.count; k++)
      prim.idx.insert(prim.idx.end(), {(uint32_t)p.v[0], (uint32_t)p.v[k], (uint32_t)p.v[k + 1]});
  }
  m->prims.push_back(std::move(prim));
  n.mesh = m;
  n.extras["og_navmesh"] = {{"verts", verts}, {"polys", polys}, {"adj", adj}};
  return n;
}

bool navmesh_ignores(const Material& m) {
  return m.blend != BlendMode::Alpha ||
         (m.alpha == AlphaMode::Blend && (m.translucent || m.base_color.w < 0.95f));
}

bool navmesh_of_node(const Node& n, NavMeshData* out) {
  auto it = n.extras.find("og_navmesh");
  if (n.kind != NodeKind::NavMesh || it == n.extras.end() || !it->is_object()) return false;
  const json& j = *it;
  NavMeshData m;
  m.origin = {0, 0, 0};
  try {
    for (const auto& v : j.at("verts")) m.verts.push_back({v.at(0).get<float>(), v.at(1).get<float>(), v.at(2).get<float>()});
    const json& polys = j.at("polys");
    const json& adj = j.at("adj");
    if (polys.size() != adj.size()) return false;
    for (size_t i = 0; i < polys.size(); i++) {
      NavPoly p;
      p.count = (int)std::min<size_t>(4, polys[i].size());
      for (int k = 0; k < p.count; k++) {
        p.v[k] = polys[i][k].get<int>();
        p.adj[k] = k < (int)adj[i].size() ? adj[i][k].get<int>() : -1;
        if (p.v[k] < 0 || p.v[k] >= (int)m.verts.size()) return false;
      }
      m.polys.push_back(p);
    }
  } catch (...) {
    return false;
  }
  *out = std::move(m);
  return true;
}

}  // namespace ogle
