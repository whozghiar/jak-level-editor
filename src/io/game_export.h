#pragma once

// A document exported as a custom level of OpenGOAL, for Jak and Daxter, Jak II or Jak 3: the
// files its level builder (goalc/build_level) takes, in custom_assets/<game>/levels/<name>/.
//
//   <name>.glb     what the view shows of the decor, in the game's space; the collision of the
//                  game where the levels have it (exact: one node per surface), the rest made by
//                  the builder from the decor (automatic walls)
//   <name>.jsonc   the level: its names, the actors (their data as the builder writes res lumps),
//                  the art groups of their models, the destination game's keys
//   <name>.gd      the DGO: the art groups and the level
//   <name>-nav.json  Jak 3: the nav meshes of the document (nav_data)
//   INSTALL.md     the lines to add to the game's code (game.gp, level-info.gc), and what the
//                  export changed for the destination game
//
// Adapted to the destination game: the collision surfaces as its builder has them (Jak 1's
// fields, converted by the builders of Jak II and 3; the materials and events the destination
// lacks become the nearest ones), the actor types it has (some mapped: the doors of Jak II become
// Jak 3's), what crates and pickups hold (by the name of the pickup type), the data that belongs to
// the story of the source game dropped (task actors, kill masks, scripts). What the destination
// game does not have is left out, and said.

#include <map>
#include <string>
#include <vector>

#include "core/games.h"
#include "scene/scene.h"
#include "tools/navmesh.h"

namespace ogle {

class GameData;

struct LevelExportOptions {
  Game game = Game::Jak2;  // the destination
  std::string name;        // long_name: a symbol of the game ("my-level")
  std::string nickname;    // 3 letters: the DGO's name
  std::string iso_name;    // at most 8 letters and digits, upper case
  uint32_t base_id = 0;    // the actors' ids from it (0: the destination's default)
  std::string folder;      // <folder>/<name>/ is written
  bool actors = true;
  bool game_collision = true;  // the levels' own collision where they have it (else from the decor)
  Vec3 start{0, 0, 0};         // where the player starts (the continue point), meters
  // Jak 3: the document's nav meshes, in the game's space (origin: the center of their corners)
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
  std::vector<ExportNote> notes;
};

// The destination's defaults: base_id above the ids of the game's actors.
uint32_t default_base_id(Game game);
// A nickname (3 letters: "m" and the name's first letters after "my-") and an ISO name (8) made
// from a level name.
std::string default_nickname(const std::string& name);
std::string default_iso_name(const std::string& name);
// "" when the names and the folder suit the builder and the game, else the text key of what is
// wrong ("export.bad.name"...).
std::string check_export_names(const LevelExportOptions& opts);

// Writes the level. `from`: the game data of the document's own game (its untagged content: the
// level opened, and anything placed without a game of its own), `to`: the destination's (either
// may be null: then nothing is mapped nor checked). `by_game`: the game data of every other game an
// element of the document may carry as its own (extras "og_game", on an actor or its level's
// group): a level added from another game, or a catalog object placed from one, keep their own
// game's rules (collision layout, actor types, pickups) instead of the document's.
bool export_level_for_game(const Scene& scene, const GameData* from, const GameData* to,
                           const LevelExportOptions& opts, LevelExportReport* report, std::string* error,
                           const std::map<std::string, const GameData*>* by_game = nullptr);

// --- the parts, for the tests ------------------------------------------------------------------

// A collision surface (the pattern of a triangle, in the source game's layout) as the level
// builder takes it: Jak 1's fields (the builders of Jak II and 3 convert them). `loss`: what the
// destination does not keep of it ("hdwood", "rail", "halfpipe", "flags"), "" when nothing.
struct BuilderSurface {
  int material = 0, event = 0, mode = 0;
  bool noentity = false, nocamera = false, noedge = false, nolineofsight = false;
  bool operator<(const BuilderSurface& o) const;
};
BuilderSurface builder_surface(uint32_t pat, Game source, Game destination, std::string* loss = nullptr);

// A res lump value of an extracted actor as the builder reads it: typed arrays (["float", ...]),
// strings and 'symbols. `type`: the extractor's type of the tag (lump_types), guessed when "".
// Null for data the builder cannot write.
json builder_lump_value(const json& value, const std::string& type);

// The actor type of `etype` in the destination game ("" when it has none).
std::string export_actor_type(const std::string& etype, Game source, Game destination, const GameData* to);

}  // namespace ogle
