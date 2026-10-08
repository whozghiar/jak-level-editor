#pragma once

// A document exported as a custom level of OpenGOAL for Jak 3: the files its level builder
// (goalc/build_level) takes, in custom_assets/jak3/levels/<name>/.
//
//   <name>.glb     what the view shows of the decor, in the game's space; the collision of the
//                  game where the levels have it (exact: one node per surface), the rest made by
//                  the builder from the decor (automatic walls)
//   <name>.jsonc   the level: its names, the actors (their data as the builder writes res lumps),
//                  the art groups of their models
//   <name>.gd      the DGO: the art groups and the level
//   <name>-nav.json  the nav meshes of the document (nav_data)
//   INSTALL.md     the lines to add to the game's code (game.gp, level-info.gc), and what the
//                  export changed
//
// The collision surfaces are written as the builder takes them (Jak 1's fields, which it converts:
// the materials and events it lacks become the nearest ones); the kill masks are dropped (the new
// level has its own story). What the builder cannot write is left out, and said.

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
  std::string folder;    // <folder>/<name>/ is written
  bool actors = true;
  bool game_collision = true;  // the levels' own collision where they have it (else from the decor)
  Vec3 start{0, 0, 0};         // where the player starts (the continue point), meters
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
  std::string dir;  // the folder written
  std::vector<std::string> files;
  size_t meshes = 0, triangles = 0;
  size_t collision_triangles = 0;   // the game's, exact or approximated
  size_t collision_from_decor = 0;  // the meshes whose collision the builder makes
  size_t actors = 0, art_groups = 0, navmeshes = 0;
  size_t behaviors = 0;   // actors whose behavior is the level's code
  std::string code_file;  // that code, <name>-obs.gc ("" when none)
  std::vector<ExportNote> notes;
};

// A nickname (3 letters: "m" and the name's first letters after "my-") and an ISO name (8) made
// from a level name.
std::string default_nickname(const std::string& name);
std::string default_iso_name(const std::string& name);
// "" when the names and the folder suit the builder and the game, else the text key of what is
// wrong ("export.bad.name"...).
std::string check_export_names(const LevelExportOptions& opts);

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
