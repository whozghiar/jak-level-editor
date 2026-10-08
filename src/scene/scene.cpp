#include "scene/scene.h"

#include <algorithm>
#include <atomic>
#include <set>
#include <unordered_map>

#include "scene/bvh.h"

namespace ogle {

// ------------------------------------------------------------------------------------------------
// kinds
// ------------------------------------------------------------------------------------------------

const char* kind_name(NodeKind k) {
  switch (k) {
    case NodeKind::Group: return "group";
    case NodeKind::Render: return "render";
    case NodeKind::Collision: return "collision";
    case NodeKind::Actor: return "actor";
    case NodeKind::NavMesh: return "navmesh";
    default: return "group";
  }
}

const char* blend_mode_name(BlendMode b) {
  switch (b) {
    case BlendMode::Add: return "add";
    case BlendMode::AddAlpha: return "add_alpha";
    case BlendMode::Subtract: return "subtract";
    case BlendMode::Half: return "half";
    default: return "";
  }
}

BlendMode blend_mode_from_name(const std::string& name) {
  for (BlendMode b : {BlendMode::Add, BlendMode::AddAlpha, BlendMode::Subtract, BlendMode::Half})
    if (name == blend_mode_name(b)) return b;
  return BlendMode::Alpha;
}

NodeKind kind_from_name(const std::string& s, NodeKind fallback) {
  for (int i = 0; i < (int)NodeKind::Count; i++) {
    if (s == kind_name((NodeKind)i)) return (NodeKind)i;
  }
  return fallback;
}

// ------------------------------------------------------------------------------------------------
// meshes
// ------------------------------------------------------------------------------------------------

namespace {
std::atomic<uint64_t> g_next_mesh_uid{1};
}

Mesh::Mesh() : uid(g_next_mesh_uid++) {}
Mesh::Mesh(const Mesh& o)
    : name(o.name), prims(o.prims), extras(o.extras), uid(g_next_mesh_uid++), version(1) {}

void Primitive::compute_normals() {
  nrm.assign(pos.size(), Vec3{0, 0, 0});
  for (size_t t = 0; t + 2 < idx.size(); t += 3) {
    const Vec3& a = pos[idx[t]];
    const Vec3& b = pos[idx[t + 1]];
    const Vec3& c = pos[idx[t + 2]];
    Vec3 n = cross(b - a, c - a);  // area weighted
    nrm[idx[t]] += n;
    nrm[idx[t + 1]] += n;
    nrm[idx[t + 2]] += n;
  }
  for (auto& n : nrm) {
    n = normalize(n);
    if (length2(n) == 0) n = {0, 1, 0};
  }
}

void Mesh::touch() {
  version++;
  m_bounds_valid = false;
}

const AABB& Mesh::bounds() {
  if (!m_bounds_valid) {
    m_bounds = AABB{};
    for (const auto& p : prims)
      for (const auto& v : p.pos) m_bounds.add(v);
    m_bounds_valid = true;
  }
  return m_bounds;
}

const Bvh& Mesh::bvh(size_t prim) {
  if (m_bvh_version != version || m_bvh.size() != prims.size()) {
    m_bvh.assign(prims.size(), nullptr);
    m_bvh_version = version;
  }
  if (!m_bvh[prim]) {
    auto b = std::make_shared<Bvh>();
    b->build(prims[prim].pos, prims[prim].idx);
    m_bvh[prim] = b;
  }
  return *m_bvh[prim];
}

size_t Mesh::tri_count() const {
  size_t n = 0;
  for (const auto& p : prims) n += p.tri_count();
  return n;
}

// ------------------------------------------------------------------------------------------------
// settings
// ------------------------------------------------------------------------------------------------

json LevelSettings::to_json() const {
  return json{{"level", level}, {"source_group", source_group}};
}

LevelSettings LevelSettings::from_json(const json& j) {
  LevelSettings s;
  s.level = j.value("level", std::string());
  s.source_group = j.value("source_group", std::string());
  return s;
}

// ------------------------------------------------------------------------------------------------
// scene
// ------------------------------------------------------------------------------------------------

Node* Scene::find(uint32_t id) {
  int i = index_of(id);
  return i >= 0 ? &nodes[i] : nullptr;
}

const Node* Scene::find(uint32_t id) const {
  int i = index_of(id);
  return i >= 0 ? &nodes[i] : nullptr;
}

int Scene::index_of(uint32_t id) const {
  if (id == 0) return -1;
  // fast path: a cache keyed by id, checked against the vector
  static thread_local std::unordered_map<const Scene*, std::unordered_map<uint32_t, int>> cache;
  auto& map = cache[this];
  auto it = map.find(id);
  if (it != map.end() && it->second < (int)nodes.size() && nodes[it->second].id == id) {
    return it->second;
  }
  map.clear();
  for (int i = 0; i < (int)nodes.size(); i++) map[nodes[i].id] = i;
  it = map.find(id);
  return it == map.end() ? -1 : it->second;
}

uint32_t Scene::add(Node node) {
  if (node.id == 0) node.id = next_id++;
  next_id = std::max(next_id, node.id + 1);
  uint32_t id = node.id;
  nodes.push_back(std::move(node));
  return id;
}

void Scene::remove(const std::vector<uint32_t>& ids) {
  std::set<uint32_t> doomed;
  for (auto id : ids) {
    doomed.insert(id);
    for (auto d : descendants(id)) doomed.insert(d);
  }
  nodes.erase(std::remove_if(nodes.begin(), nodes.end(),
                             [&](const Node& n) { return doomed.count(n.id) != 0; }),
              nodes.end());
}

std::vector<uint32_t> Scene::children(uint32_t id) const {
  std::vector<uint32_t> out;
  for (const auto& n : nodes)
    if (n.parent == id) out.push_back(n.id);
  return out;
}

std::vector<uint32_t> Scene::descendants(uint32_t id) const {
  std::vector<uint32_t> out;
  std::vector<uint32_t> stack{id};
  while (!stack.empty()) {
    uint32_t cur = stack.back();
    stack.pop_back();
    for (const auto& n : nodes) {
      if (n.parent == cur && n.id != id) {
        out.push_back(n.id);
        stack.push_back(n.id);
      }
    }
  }
  return out;
}

bool Scene::is_ancestor(uint32_t ancestor, uint32_t id) const {
  const Node* n = find(id);
  int guard = 0;
  while (n && n->parent != 0 && guard++ < 1000) {
    if (n->parent == ancestor) return true;
    n = find(n->parent);
  }
  return false;
}

bool Scene::effectively_visible(const Node& n) const {
  const Node* cur = &n;
  int guard = 0;
  while (cur && guard++ < 1000) {
    if (!cur->visible || cur->story_hidden) return false;
    if (cur->parent == 0) return true;
    cur = find(cur->parent);
  }
  return true;
}

uint32_t Scene::actor_of(const Node& n) const {
  if (n.kind != NodeKind::Render) return 0;
  const Node* p = n.parent ? find(n.parent) : nullptr;
  int guard = 0;
  while (p && guard++ < 64) {
    if (p->kind == NodeKind::Actor) return p->id;
    p = p->parent ? find(p->parent) : nullptr;
  }
  return 0;
}

Mat4 Scene::world(const Node& n) const {
  Mat4 m = n.local.matrix();
  const Node* p = n.parent ? find(n.parent) : nullptr;
  int guard = 0;
  while (p && guard++ < 1000) {
    m = p->local.matrix() * m;
    p = p->parent ? find(p->parent) : nullptr;
  }
  return m;
}

const SceneCache& Scene::cache() const {
  SceneCache& c = m_cache;
  const size_t n = nodes.size();
  // the node set itself (as opposed to only their transforms, which a drag changes every frame):
  // count and the id at each end are enough to tell, as cheaply as the frame counter itself
  const bool same_nodes = c.count == n && (!n || (c.first_id == nodes.front().id && c.last_id == nodes.back().id));
  if (c.frame == m_frame && same_nodes) return c;
  c.frame = m_frame;
  c.count = n;
  c.first_id = n ? nodes.front().id : 0;
  c.last_id = n ? nodes.back().id : 0;
  c.worlds.resize(n);
  c.visible.assign(n, 0);
  c.actor.assign(n, 0);
  c.bounds.assign(n, AABB{});
  // id -> index (ids are small and dense)
  uint32_t max_id = 0;
  for (const auto& node : nodes) max_id = std::max(max_id, node.id);
  std::vector<int32_t> index(max_id + 1, -1);
  for (size_t i = 0; i < n; i++) index[nodes[i].id] = (int32_t)i;
  std::vector<uint8_t> done(n, 0);
  std::vector<uint32_t> chain;
  for (size_t i = 0; i < n; i++) {
    if (done[i]) continue;
    // the unresolved ancestors, then down from the first resolved one
    chain.clear();
    int32_t cur = (int32_t)i;
    while (cur >= 0 && !done[cur] && chain.size() < 1000) {
      chain.push_back((uint32_t)cur);
      uint32_t parent = nodes[cur].parent;
      cur = parent && parent <= max_id ? index[parent] : -1;
    }
    for (size_t k = chain.size(); k-- > 0;) {
      const uint32_t j = chain[k];
      const Node& node = nodes[j];
      const int32_t p = node.parent && node.parent <= max_id ? index[node.parent] : -1;
      Mat4 local = node.local.matrix();
      if (p >= 0 && done[p]) {
        c.worlds[j] = c.worlds[p] * local;
        c.visible[j] = c.visible[p] && node.visible && !node.story_hidden;
        if (node.kind == NodeKind::Render)
          c.actor[j] = nodes[p].kind == NodeKind::Actor ? nodes[p].id : c.actor[p];
        else if (node.kind != NodeKind::Actor)
          c.actor[j] = c.actor[p];
      } else {
        c.worlds[j] = local;
        c.visible[j] = node.visible && !node.story_hidden;
      }
      if (node.mesh) c.bounds[j] = node.mesh->bounds().transformed(c.worlds[j]);
      done[j] = 1;
    }
  }
  // instances of a prototype (a decor piece, an actor's model) keep the same index in `nodes`
  // while only transforms change (a drag): regrouping by mesh, with its many small allocations,
  // is only needed when the node set itself changed
  if (!same_nodes) {
    c.mesh_groups.clear();
    std::unordered_map<const Mesh*, size_t> group_of;
    group_of.reserve(n / 4 + 1);
    for (size_t i = 0; i < n; i++) {
      const Node& node = nodes[i];
      if (node.kind != NodeKind::Render || !node.mesh) continue;
      auto it = group_of.find(node.mesh.get());
      if (it == group_of.end()) {
        it = group_of.emplace(node.mesh.get(), c.mesh_groups.size()).first;
        c.mesh_groups.push_back({node.mesh.get(), {}});
      }
      c.mesh_groups[it->second].second.push_back((uint32_t)i);
    }
  }
  return c;
}

Mat4 Scene::world(uint32_t id) const {
  const Node* n = find(id);
  return n ? world(*n) : Mat4::identity();
}

void Scene::set_world(Node& n, const Mat4& w) {
  Mat4 parent = n.parent ? world(n.parent) : Mat4::identity();
  n.local = Transform::from_matrix(parent.inverse() * w);
}

AABB Scene::world_bounds(const Node& n) const {
  Mat4 w = world(n);
  if (n.mesh) return n.mesh->bounds().transformed(w);
  AABB b;
  const Vec3 c = w.col3(3);
  if (n.kind == NodeKind::Actor) {
    b.add(c - Vec3{kMarkerRadius, kMarkerRadius, kMarkerRadius});
    b.add(c + Vec3{kMarkerRadius, kMarkerRadius, kMarkerRadius});
  } else {
    b.add(c);
  }
  return b;
}

AABB Scene::bounds() const {
  AABB b;
  for (const auto& n : nodes) {
    if (n.kind == NodeKind::Group || n.kind == NodeKind::Collision) continue;
    b.add(world_bounds(n));
  }
  return b;
}

AABB focus_box(const std::vector<AABB>& boxes) {
  AABB all;
  std::vector<float> lo[3], hi[3];
  for (const auto& b : boxes) {
    if (!b.valid()) continue;
    all.add(b);
    for (int a = 0; a < 3; a++) {
      lo[a].push_back(b.lo[a]);
      hi[a].push_back(b.hi[a]);
    }
  }
  if (lo[0].size() < 20) return all;
  AABB out;
  Vec3 l, h;
  for (int a = 0; a < 3; a++) {
    std::sort(lo[a].begin(), lo[a].end());
    std::sort(hi[a].begin(), hi[a].end());
    l[a] = lo[a][lo[a].size() * 2 / 100];
    h[a] = hi[a][hi[a].size() * 98 / 100];
  }
  out.add(l);
  out.add(h);
  return out.valid() ? out : all;
}

AABB Scene::focus_bounds() const {
  std::vector<AABB> boxes;
  for (const auto& n : nodes)
    if (n.kind != NodeKind::Group && n.kind != NodeKind::Collision) boxes.push_back(world_bounds(n));
  const AABB b = focus_box(boxes);
  return b.valid() ? b : bounds();
}

int Scene::add_material(Material m) {
  materials.push_back(std::move(m));
  return (int)materials.size() - 1;
}

int Scene::add_texture(Texture t) {
  textures.push_back(std::move(t));
  return (int)textures.size() - 1;
}

void Scene::clear() {
  nodes.clear();
  textures.clear();
  materials.clear();
  next_id = 1;
}

namespace {

uint64_t fnv1a(const std::vector<uint8_t>& data) {
  uint64_t h = 1469598103934665603ull;
  for (uint8_t b : data) h = (h ^ b) * 1099511628211ull;
  return h;
}

bool same_material(const Material& a, const Material& b) {
  return a.name == b.name && a.texture == b.texture && a.alpha == b.alpha && a.blend == b.blend &&
         a.cutoff == b.cutoff && a.double_sided == b.double_sided && a.base_color.x == b.base_color.x &&
         a.base_color.y == b.base_color.y && a.base_color.z == b.base_color.z && a.base_color.w == b.base_color.w;
}

}  // namespace

std::vector<uint32_t> Scene::append(Scene&& src) {
  // textures: the same image (name, size, pixels) is shared
  std::unordered_map<uint64_t, std::vector<int>> by_hash;
  for (int i = 0; i < (int)textures.size(); i++) by_hash[fnv1a(textures[i].rgba)].push_back(i);
  std::vector<int> tex_map(src.textures.size(), -1);
  for (size_t i = 0; i < src.textures.size(); i++) {
    Texture& t = src.textures[i];
    const uint64_t h = fnv1a(t.rgba);
    for (int j : by_hash[h])
      if (textures[j].name == t.name && textures[j].width == t.width && textures[j].height == t.height &&
          textures[j].rgba == t.rgba)
        tex_map[i] = j;
    if (tex_map[i] < 0) {
      tex_map[i] = add_texture(std::move(t));
      by_hash[h].push_back(tex_map[i]);
    }
  }
  std::vector<int> mat_map(src.materials.size(), -1);
  for (size_t i = 0; i < src.materials.size(); i++) {
    Material m = std::move(src.materials[i]);
    if (m.texture >= 0 && m.texture < (int)tex_map.size()) m.texture = tex_map[m.texture];
    for (int j = 0; j < (int)materials.size() && mat_map[i] < 0; j++)
      if (same_material(materials[j], m)) mat_map[i] = j;
    if (mat_map[i] < 0) mat_map[i] = add_material(std::move(m));
  }
  // the meshes (shared by the copies of a prototype) take the new material numbers once
  std::set<Mesh*> renumbered;
  for (auto& n : src.nodes) {
    if (!n.mesh || !renumbered.insert(n.mesh.get()).second) continue;
    for (auto& p : n.mesh->prims)
      if (p.material >= 0 && p.material < (int)mat_map.size()) p.material = mat_map[p.material];
  }
  std::unordered_map<uint32_t, uint32_t> ids;
  for (const auto& n : src.nodes) ids[n.id] = next_id++;
  std::vector<uint32_t> roots;
  nodes.reserve(nodes.size() + src.nodes.size());
  for (auto& n : src.nodes) {
    n.id = ids[n.id];
    auto p = n.parent ? ids.find(n.parent) : ids.end();
    n.parent = p != ids.end() ? p->second : 0;
    if (!n.parent) roots.push_back(n.id);
    nodes.push_back(std::move(n));
  }
  src.clear();
  return roots;
}

}  // namespace ogle
