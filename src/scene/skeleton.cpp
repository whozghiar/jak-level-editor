#include "scene/skeleton.h"

#include <algorithm>
#include <cmath>

namespace ogle {

namespace {

// A matrix's translation, rotation and scale
void decompose(const Mat4& m, Vec3* t, Quat* r, Vec3* s) {
  *t = m.col3(3);
  const Vec3 c0 = m.col3(0), c1 = m.col3(1), c2 = m.col3(2);
  *s = {length(c0), length(c1), length(c2)};
  const Vec3 x = s->x > 0 ? c0 * (1 / s->x) : Vec3{1, 0, 0};
  const Vec3 y = s->y > 0 ? c1 * (1 / s->y) : Vec3{0, 1, 0};
  const Vec3 z = s->z > 0 ? c2 * (1 / s->z) : Vec3{0, 0, 1};
  // the rotation matrix's quaternion
  const float tr = x.x + y.y + z.z;
  Quat q;
  if (tr > 0) {
    const float k = std::sqrt(tr + 1.f) * 2;
    q = {(y.z - z.y) / k, (z.x - x.z) / k, (x.y - y.x) / k, 0.25f * k};
  } else if (x.x > y.y && x.x > z.z) {
    const float k = std::sqrt(1.f + x.x - y.y - z.z) * 2;
    q = {0.25f * k, (y.x + x.y) / k, (z.x + x.z) / k, (y.z - z.y) / k};
  } else if (y.y > z.z) {
    const float k = std::sqrt(1.f + y.y - x.x - z.z) * 2;
    q = {(y.x + x.y) / k, 0.25f * k, (z.y + y.z) / k, (z.x - x.z) / k};
  } else {
    const float k = std::sqrt(1.f + z.z - x.x - y.y) * 2;
    q = {(z.x + x.z) / k, (z.y + y.z) / k, 0.25f * k, (x.y - y.x) / k};
  }
  *r = q.normalized();
}

Quat slerp(Quat a, Quat b, float t) {
  float d = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
  if (d < 0) {
    b = {-b.x, -b.y, -b.z, -b.w};
    d = -d;
  }
  if (d > 0.9995f)
    return Quat{a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t, a.w + (b.w - a.w) * t}.normalized();
  const float th = std::acos(d), s = std::sin(th);
  const float ka = std::sin((1 - t) * th) / s, kb = std::sin(t * th) / s;
  return {a.x * ka + b.x * kb, a.y * ka + b.y * kb, a.z * ka + b.z * kb, a.w * ka + b.w * kb};
}

// A channel's value at t (its keys held before the first and after the last)
Vec4 sample(const Skeleton::Channel& c, float t) {
  if (c.values.empty()) return {};
  if (c.times.size() < 2 || t <= c.times.front()) return c.values.front();
  if (t >= c.times.back()) return c.values[std::min(c.values.size(), c.times.size()) - 1];
  const size_t i = std::upper_bound(c.times.begin(), c.times.end(), t) - c.times.begin() - 1;
  if (i + 1 >= c.values.size()) return c.values.back();
  const Vec4 a = c.values[i], b = c.values[i + 1];
  if (c.step) return a;
  const float k = (t - c.times[i]) / std::max(c.times[i + 1] - c.times[i], 1e-6f);
  if (c.path == 1) {
    const Quat q = slerp({a.x, a.y, a.z, a.w}, {b.x, b.y, b.z, b.w}, k);
    return {q.x, q.y, q.z, q.w};
  }
  return {a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k, a.z + (b.z - a.z) * k, a.w + (b.w - a.w) * k};
}

}  // namespace

std::vector<Mat4> skeleton_pose(const Skeleton& s, int animation, float t) {
  const size_t n = s.rest.size();
  std::vector<Mat4> local = s.rest;
  if (animation >= 0 && animation < (int)s.animations.size()) {
    // the animated nodes: their translation, rotation and scale from the channels, the rest from
    // their own transform
    std::vector<Vec3> tr(n), sc(n);
    std::vector<Quat> rot(n);
    std::vector<uint8_t> moved(n, 0);
    for (const auto& c : s.animations[animation].channels) {
      if (c.node < 0 || c.node >= (int)n) continue;
      if (!moved[c.node]) decompose(s.rest[c.node], &tr[c.node], &rot[c.node], &sc[c.node]);
      moved[c.node] = 1;
      const Vec4 v = sample(c, t);
      if (c.path == 0) tr[c.node] = v.xyz();
      else if (c.path == 1) rot[c.node] = Quat{v.x, v.y, v.z, v.w}.normalized();
      else sc[c.node] = v.xyz();
    }
    for (size_t i = 0; i < n; i++)
      if (moved[i]) local[i] = Mat4::translate(tr[i]) * Mat4::rotate(rot[i]) * Mat4::scale(sc[i]);
  }
  // down from the roots
  std::vector<Mat4> world(n);
  std::vector<uint8_t> done(n, 0);
  for (size_t i = 0; i < n; i++) {
    std::vector<int> chain;
    for (int k = (int)i; k >= 0 && !done[k] && chain.size() <= n; k = s.parent[k]) chain.push_back(k);
    for (auto it = chain.rbegin(); it != chain.rend(); ++it) {
      const int k = *it, p = s.parent[k];
      world[k] = p >= 0 ? world[p] * local[k] : local[k];
      done[k] = 1;
    }
  }
  return world;
}

std::vector<Mat4> joint_matrices(const Skeleton& s, const std::vector<Mat4>& worlds) {
  std::vector<Mat4> out(s.joints.size());
  for (size_t j = 0; j < s.joints.size(); j++) {
    const int node = s.joints[j];
    const Mat4 w = node >= 0 && node < (int)worlds.size() ? worlds[node] : Mat4::identity();
    out[j] = j < s.inverse_bind.size() ? w * s.inverse_bind[j] : w;
  }
  return out;
}

bool skin_mesh(const Mesh& bind, const std::vector<Mat4>& joints, Mesh& out) {
  if (bind.prims.size() != out.prims.size()) return false;
  for (size_t pi = 0; pi < bind.prims.size(); pi++) {
    const Primitive& b = bind.prims[pi];
    Primitive& o = out.prims[pi];
    auto j = b.extra_attrs.find("JOINTS_0");
    auto w = b.extra_attrs.find("WEIGHTS_0");
    if (o.pos.size() != b.pos.size()) return false;
    if (j == b.extra_attrs.end() || w == b.extra_attrs.end()) continue;  // a piece that does not move
    o.nrm.resize(o.pos.size());
    for (size_t v = 0; v < b.pos.size(); v++) {
      Vec3 p{0, 0, 0}, nr{0, 0, 0};
      float total = 0;
      const Vec3 n = v < b.nrm.size() ? b.nrm[v] : Vec3{0, 1, 0};
      for (int k = 0; k < 4; k++) {
        const float weight = w->second[v][k];
        const int joint = (int)j->second[v][k];
        if (weight <= 0 || joint < 0 || joint >= (int)joints.size()) continue;
        p = p + joints[joint].point(b.pos[v]) * weight;
        nr = nr + joints[joint].dir(n) * weight;
        total += weight;
      }
      o.pos[v] = total > 0 ? p * (1 / total) : b.pos[v];
      o.nrm[v] = total > 0 ? normalize(nr) : n;
    }
  }
  out.touch();
  return true;
}

}  // namespace ogle
