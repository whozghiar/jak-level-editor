#pragma once

// The editor's document: a level loaded from the library (and the levels added to it, each under
// its group: see scene/story.h), made of nodes.
//
//   Render    : visible geometry (terrain, decor instances, actor models)
//   Collision : the level's collision surfaces, kept with the level but not shown
//   Actor     : an entity of the level (etype, res-lump), with its model as a Render child
//   Group     : an empty node that organizes the others (a decor prototype and its instances)
//
// A project is saved as a .glb: the same data, with the editor's node extras (og_kind...).

#include <array>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "core/math.h"
#include "json.hpp"

namespace ogle {

using json = nlohmann::json;

// NavMesh: where creatures walk (tools/navmesh.h), drawn over the level, not part of its decor
enum class NodeKind { Group = 0, Render, Collision, Actor, NavMesh, Count };
const char* kind_name(NodeKind k);  // "group", "render", "collision", "actor", "navmesh"
NodeKind kind_from_name(const std::string& s, NodeKind fallback);

struct Texture {
  std::string name;
  int width = 0;
  int height = 0;
  std::vector<uint8_t> rgba;  // width * height * 4
  uint64_t version = 1;       // bumped on change, the renderer re-uploads
};

enum class AlphaMode { Opaque = 0, Mask, Blend };
// How a Blend material mixes with what is behind it. glTF only has Alpha; the others are the PS2's
// additive and subtractive blends (glows, light beams), kept in the material's extras.og_blend.
enum class BlendMode { Alpha = 0, Add, AddAlpha, Subtract, Half };
const char* blend_mode_name(BlendMode b);  // "" for Alpha, else "add", "add_alpha"...
BlendMode blend_mode_from_name(const std::string& name);

struct Material {
  std::string name;
  int texture = -1;  // index into Scene::textures
  Vec4 base_color{1, 1, 1, 1};
  AlphaMode alpha = AlphaMode::Opaque;
  BlendMode blend = BlendMode::Alpha;
  float cutoff = 0.5f;
  bool double_sided = true;
  // drawn by the game without writing its depth (extras.og_translucent: the extractor's): see
  // through, like glass and water. The game blends most of a level's other surfaces too, but
  // writes their depth: they are walls (the alpha test makes their holes).
  bool translucent = false;
  // a water surface of the game (a water actor's model): its texture flows, as the game animates it
  // (set when its model is drawn, not saved)
  bool water = false;
};

// Whether the game draws a material in its translucent pass, behind which the rest shows (glass,
// water, glows, light shafts): additive and subtractive blends, the levels' surfaces drawn so
// (Material::translucent), and the editor's own blended materials. What the export gives the level
// builder's translucent tree, and the game view draws last.
bool translucent_in_game(const Material& m);

struct Bvh;
class Scene;

struct Primitive {
  std::vector<Vec3> pos;
  std::vector<Vec3> nrm;   // same size as pos
  std::vector<Vec2> uv;    // empty or same size as pos
  std::vector<Vec4> col;   // empty or same size as pos (vertex colors, 0..1)
  std::vector<uint32_t> idx;  // triangle list
  int material = -1;          // index into Scene::materials
  std::vector<uint32_t> tri_pat;  // collision only: the surface of each triangle
  // extra vertex attributes kept for round trips, e.g. the 8 time-of-day palettes _SUNRISE...
  std::map<std::string, std::vector<Vec4>> extra_attrs;

  size_t tri_count() const { return idx.size() / 3; }
  void compute_normals();  // smooth normals from faces
};

struct Mesh {
  Mesh();
  Mesh(const Mesh& other);             // copies get a new uid
  Mesh& operator=(const Mesh&) = delete;

  std::string name;
  std::vector<Primitive> prims;
  json extras = json::object();

  uint64_t uid;          // unique for the life of the program (render cache key)
  uint64_t version = 1;  // bumped by touch()

  // derived data, rebuilt lazily after touch()
  const AABB& bounds();
  const Bvh& bvh(size_t prim);
  void touch();
  size_t tri_count() const;

 private:
  AABB m_bounds;
  bool m_bounds_valid = false;
  std::vector<std::shared_ptr<Bvh>> m_bvh;
  uint64_t m_bvh_version = 0;
};

// The game's lighting of a level's geometry (Jak 3): the game mixes 8 palettes per color (the
// ambient light, 4 directions of the sun, the lamps of each area...) with weights it sets every
// frame from the time of the day and the level's mood. A table of colors, 8 RGBA bytes each...
struct TodPalette {
  std::vector<uint8_t> rgba;  // color c's palette k at c * 32 + k * 4
  // made for one element, lit like the level around it (scene/relight.h), not a tree of the level's
  bool local = false;
  size_t size() const { return rgba.size() / 32; }
};

// ...and the color of each vertex of a node's mesh in it. The instances of a decor prototype share
// its mesh, not their lighting: it is the node's.
struct TodColors {
  std::shared_ptr<const TodPalette> palette;
  std::vector<std::vector<uint16_t>> index;  // per primitive of the mesh, per vertex (< palette's size)
  // whether it still matches the mesh (its vertices edited, the node is lit like new geometry)
  bool fits(const Mesh& mesh) const;
};

// How much of a lit color each palette gives on average (the colors of the extracted levels are
// the brightest of their 8): new geometry, which has one color, gets its palettes this way. From
// the nodes of the scene that have their lighting, else a district of the city's (ctyindb).
std::array<float, 8> tod_ratios(const Scene& scene, const std::vector<uint32_t>& nodes);
// a color as the editor shows it (the vertex's times its material's: 1 is the texture as it is)
// as 8 palettes
void tod_from_color(const Vec4& shown, const std::array<float, 8>& ratios, uint8_t out[32]);

struct Node {
  uint32_t id = 0;
  uint32_t parent = 0;  // 0 = root
  std::string name;
  NodeKind kind = NodeKind::Group;
  Transform local;
  std::shared_ptr<Mesh> mesh;    // Render, Collision
  json extras = json::object();  // actor data (etype, aid, lump...), glTF extras
  std::shared_ptr<const TodColors> tod;  // its lighting in the game, when known (Render)
  bool visible = true;
  bool locked = false;           // not selectable (the terrain)
  bool story_hidden = false;     // not shown at the point of the story viewed (not saved)
};

struct LevelSettings {
  std::string level;         // the library level the document comes from ("desert")
  std::string source_group;  // its folder in the library: its models are drawn for its actors
  // a level of the game this one is made to replace in the game ("ctyindb"), "" for a level of its
  // own: the game then loads it at that level's place, with that level's continue points
  std::string replaces;

  json to_json() const;
  static LevelSettings from_json(const json& j);
};

// World matrices, effective visibility and owning actor of every node, aligned with Scene::nodes.
// Computed once per Scene::begin_frame() instead of walking the parents of each node again for
// every query (a whole city is 150 000 nodes).
struct SceneCache {
  std::vector<Mat4> worlds;
  std::vector<uint8_t> visible;   // the node and all its parents are visible (and not hidden by the story)
  std::vector<uint32_t> actor;    // Render nodes under an actor (its model): the actor, else 0
  // under a group marked og_reference: a level shown to compare with, not part of the document's
  // level (not selected, not exported)
  std::vector<uint8_t> reference;
  std::vector<AABB> bounds;       // world box of the node's mesh (Render, Collision), else empty
  // render nodes grouped by mesh (the instances of a prototype): mesh, node indices
  std::vector<std::pair<Mesh*, std::vector<uint32_t>>> mesh_groups;
  std::vector<const Mesh*> meshes;  // each node's when they were grouped
  // the tree by index: each node's parent (-1 for a root), and its children (kids[kid_start[i] ..
  // kid_start[i + 1]]), so that walking a subtree does not scan the whole scene
  std::vector<int32_t> parent;
  std::vector<uint32_t> kid_start, kids;
  uint64_t parents_hash = 0;
  uint64_t version = 0;  // counts the times the cache was made: what was computed from it is still good while it holds
  uint64_t frame = ~0ull;
  size_t count = 0;
  uint32_t first_id = 0, last_id = 0;
};

class Scene {
 public:
  std::vector<Node> nodes;  // parents before children is not required
  std::vector<Texture> textures;
  std::vector<Material> materials;
  LevelSettings settings;
  uint32_t next_id = 1;

  Node* find(uint32_t id);
  const Node* find(uint32_t id) const;
  int index_of(uint32_t id) const;
  uint32_t add(Node node);  // assigns an id when node.id == 0
  // removes the nodes and all their descendants
  void remove(const std::vector<uint32_t>& ids);
  std::vector<uint32_t> children(uint32_t id) const;
  std::vector<uint32_t> descendants(uint32_t id) const;
  // The descendants of each of `roots` (not the roots themselves), in one pass over the nodes
  // however many roots: a node under two of them goes to the nearest.
  std::vector<std::vector<uint32_t>> descendants_of(const std::vector<uint32_t>& roots) const;
  // The indices (in `nodes`) of the subtree of the node at `index`, itself first, from the cache's
  // tree: as the nodes were when the cache was made.
  void subtree(size_t index, std::vector<uint32_t>& out) const;
  bool is_ancestor(uint32_t ancestor, uint32_t id) const;
  bool effectively_visible(const Node& n) const;
  // the actor a render node belongs to (its model drawn under it), or 0
  uint32_t actor_of(const Node& n) const;

  Mat4 world(const Node& n) const;
  Mat4 world(uint32_t id) const;
  // the cache of the current frame (recomputed after begin_frame() or when nodes were added or
  // removed); queries made after a change in the same frame see the old matrices
  void begin_frame() { m_frame++; }
  const SceneCache& cache() const;
  // sets the node's local transform so that its world matrix becomes `w`
  void set_world(Node& n, const Mat4& w);

  AABB world_bounds(const Node& n) const;  // mesh bounds, or the marker of an actor
  AABB bounds() const;                      // whole scene (collision left out)
  // where most of the scene is: a stray piece far away does not shrink the view (framing)
  AABB focus_bounds() const;

  int add_material(Material m);
  int add_texture(Texture t);
  void clear();
  // Moves the nodes, materials and textures of `src` into this scene (a level loaded apart): the
  // nodes get new ids, the textures and materials this scene already has are shared. Returns the
  // new ids of src's roots.
  std::vector<uint32_t> append(Scene&& src);

 private:
  uint64_t m_frame = 0;
  mutable SceneCache m_cache;
};

// Where most of these boxes are: the 2nd to 98th percentile on each axis, so that a stray piece
// far away does not count; their union when they are few.
AABB focus_box(const std::vector<AABB>& boxes);

// Whether a piece of a level's shell is ground, which the editor locks so that clicks go to what
// stands on it: 60 m across or more, most of its surface facing up or down. A facade, a wall or a
// door of a level's shell as large is not (they stay selectable).
bool ground_piece(Mesh& mesh);
// A piece of ground (ground_piece) the game made one with walls standing on it (a street and the
// facade along it): what stands more than 3 m above its ground, mostly upright (walls, and the roofs
// on them), moved to a piece of its own, <name>-walls, selectable; a hill is left whole. Its light
// (the game's palettes) goes with each part. Returns the new node's index, -1 when there is nothing
// to split. The nodes may move in memory.
int split_ground_piece(Scene& scene, size_t index);
// A piece whose texture is of natural ground (sand, grass, rock, dirt, mud, snow, a beach, a mountain,
// a dune, a cliff, lava, "ground"): ground even over other ground (a ledge, a terrace); the level's
// pieces are named after their texture
bool natural_ground(const std::string& name);
// ... and of a structure (a roof, a bridge, a pillar, the palace, a tower's side, a monitor, a beam,
// a pipe, a brace): never ground, however large and flat
bool structure_texture(const std::string& name);
// The height of a piece's floor: of its flat surfaces (by area) the lowest fifth's, in the world
// (`world` its node's matrix); `area` gets their area
float piece_floor(const Mesh& mesh, const Mat4& world, float* area = nullptr);

// Whether a node of an extracted level is one of its trees of instanced decor, <level>-tie-<n> or
// <level>-shrub-<n>: its children are the prototypes of its decor, their children the instances.
// A prototype's own name may hold "-tie-" or "-shrub-" too (des-stone-shrub-01.mb).
bool instance_tree(const std::string& name);

// Marker size of actors without a model (meters)
constexpr float kMarkerRadius = 0.6f;

}  // namespace ogle
