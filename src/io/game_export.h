#pragma once

// A document exported as a custom level of OpenGOAL for Jak 3: the files its level builder
// (goalc/build_level) takes, laid out in <folder> (the project's export: <library>/export/<project>/)
// as in the jak-project checkout they are copied into:
//
//   README.md                                   how to bring it into the game (the folders to copy,
//                                               the lines to add to game.gp and level-info.gc, how to
//                                               build it and go there) and what the export changed
//   custom_assets/jak3/levels/<name>/           the level:
//   goal_src/jak3/levels/<name>/<name>-obs.gc   the behaviors set in the editor (when there are some)
//
// The level's files:
//
//   <name>.glb     what the view shows of the decor, in the game's space; the collision of the
//                  game where the levels have it (exact: one node per surface), the rest made by
//                  the builder from the decor (automatic walls). Where a piece of a level's decor
//                  was deleted, moved or hidden, the game's collision there is left out (the level
//                  as the game has it, `originals`, tells which pieces are still in place)
//   <name>.jsonc   the level: its names, the actors (their data as the builder writes res lumps),
//                  the art groups of their models
//   <name>.gd      the DGO: the art groups and the level
//   <name>-nav.json  the nav meshes of the document (nav_data)
//   level-info.gc  its definition in the game's code (or the line to change, for a level replacing
//                  one of the game's)
//
// The collision surfaces are written as the builder takes them (Jak 1's fields, which it converts:
// the materials and events it lacks become the nearest ones); the kill masks are dropped (the new
// level has its own story). What the builder cannot write is left out, and said.

#include <map>
#include <memory>
#include <string>
#include <vector>

#include "scene/scene.h"
#include "tools/navmesh.h"

namespace ogle {

class GameData;

// the actors' ids by default: above the ids of the game's own actors (72,851 at most)
constexpr uint32_t kDefaultBaseId = 90000;

struct LevelExportOptions {
  std::string name;      // long_name: a symbol of the game ("my-level")
  std::string nickname;  // 3 letters: the DGO's name
  std::string iso_name;  // at most 8 letters and digits, upper case
  uint32_t base_id = 0;  // the actors' ids from it (0: kDefaultBaseId)
  std::string folder;    // the export's folder: README.md, custom_assets/..., goal_src/...
  bool actors = true;
  bool game_collision = true;  // the levels' own collision where they have it (else from the decor)
  Vec3 start{0, 0, 0};         // where the player starts (the continue point), meters
  // a level of the game the level replaces ("ctyindb"), "" for a level of its own. The game then
  // loads it instead of that level (its definition's nickname names this DGO): with that level's
  // continue points, music and missions; only the document's own level is written (not the
  // levels added next to it), and its DGO has that level's code
  std::string replaces;
  // the levels of the game as extracted, by name: the pieces of their decor that the document
  // deleted, moved or hid lose the game's collision there (game_collision); a level without one
  // keeps all of its collision
  std::map<std::string, std::shared_ptr<const Scene>> originals;
  // the regions of the level replaced (its level.json, region_trees): the game loads and shows the
  // levels around it by them
  json region_trees;
  // the document's nav meshes, in the game's space (origin: the center of their corners)
  std::vector<NavMeshData> navmeshes;
  std::vector<std::string> navmesh_names;
};

// What the export changed or left out: a text key (lang/*.json, "export.note.*") with a count
// and details (names of types, lumps...).
struct ExportNote {
  std::string key;
  size_t count = 0;
  std::string detail;
};

struct LevelExportReport {
  std::string dir;        // the folder written (the export's: README.md at its top)
  std::string level_dir;  // the level's files: <dir>/custom_assets/jak3/levels/<name>
  std::vector<std::string> files;
  size_t meshes = 0, triangles = 0;
  size_t collision_triangles = 0;   // the game's, exact or approximated
  size_t collision_from_decor = 0;  // the meshes whose collision the builder makes
  size_t actors = 0, art_groups = 0, navmeshes = 0;
  size_t behaviors = 0;   // actors whose behavior is the level's code
  // the places of its floors where Jak would meet more collision than the game gathers at once
  // (461 triangles): he can fall through there
  size_t dense_places = 0;
  std::vector<std::pair<size_t, Vec3>> densest;  // the 100 densest of them: triangles, where
  size_t regions = 0;      // of the level replaced (region_trees)
  std::string code_file;  // that code, <name>-obs.gc ("" when none)
  std::vector<ExportNote> notes;
};

// A nickname (3 letters: "m" and the name's first letters after "my-") and an ISO name (8) made
// from a level name.
std::string default_nickname(const std::string& name);
std::string default_iso_name(const std::string& name);
// "" when the names and the folder suit the builder and the game, else the text key of what is
// wrong ("export.bad.name"...). With the game's data: not the name or the nickname of one of its
// levels (its DGO would be written over the game's), and a level to replace that the game has.
std::string check_export_names(const LevelExportOptions& opts, const GameData* data = nullptr);

// Where the player would start in a level, for each continue point of the game's level `level`:
// the height of the ground under it in the document (NaN when there is none within a few meters).
// The document's own level only: not the levels added next to it nor a reference.
struct ContinueCheck {
  std::string name;
  Vec3 position;
  float ground = 0;  // meters, NaN: nothing under it
  bool ok() const { return ground == ground; }
};
std::vector<ContinueCheck> check_continues(const Scene& scene, const GameData* data, const std::string& level);

// Whether the node at index `i` is part of the document's own level: not under a level added to it,
// a companion, a mission layer, nor a reference.
bool own_level_node(const Scene& scene, size_t i);

// The collision an element of the decor has in the game when the editor sets none (the export's
// rule): the game's own (a piece of a level whose collision the document has, where it is still in
// place), none (a surface seen through: water, glass; a piece under 4 m: a pipe, a lamp, a decal)
// or one made from its shape by the level builder.
enum class DefaultCollision { Game, SeeThrough, Small, Shape };
DefaultCollision default_decor_collision(const Scene& scene, size_t index);

// Writes the level. `data`: the game's data (may be null: then the actor types are not checked).
bool export_level_for_game(const Scene& scene, const GameData* data, const LevelExportOptions& opts,
                           LevelExportReport* report, std::string* error);

// --- the parts, for the tests ------------------------------------------------------------------

// A collision surface (the pattern of a triangle, in the game's layout) as the level builder takes
// it: Jak 1's fields (the builder converts them). `loss`: what the builder does not keep of it
// ("hdwood", "rail", "halfpipe", "flags"), "" when nothing.
struct BuilderSurface {
  int material = 0, event = 0, mode = 0;
  bool noentity = false, nocamera = false, noedge = false, nolineofsight = false;
  bool operator<(const BuilderSurface& o) const;
};
BuilderSurface builder_surface(uint32_t pat, std::string* loss = nullptr);

// A res lump value of an extracted actor as the builder reads it: typed arrays (["float", ...]),
// strings and 'symbols. `type`: the extractor's type of the tag (lump_types), guessed when "".
// Null for data the builder cannot write.
json builder_lump_value(const json& value, const std::string& type);

}  // namespace ogle
