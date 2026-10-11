#pragma once

// The catalog of what can be put in a level:
//
//   objects  the objects of the game that make up a place: its props (a flag, a windmill, a
//            sprinkler, a parked hellcat), its doors and airlocks, its water, dark eco and lava,
//            the particle effects of its levels (one per particle group: lights, neon signs,
//            steam); and apart, the interactive objects (crates, platforms, elevators, bouncers,
//            buttons, breakables, hazards...). Each with the model its code draws (one entry per
//            model for the types that choose theirs by name) and the data of an actor of its type
//            found in the levels. Never the living (enemies, characters, animals: creatures), the
//            collectibles (orbs, gems, eco, ammo...) nor the technical types (shots, vehicles,
//            controllers, the logic of the levels): tools/object_categories.json
//   models   every other model of the game but the characters: parts of objects, debris, props,
//            placed as decor
//   parts    the decor of a level, to reuse in another: the prototypes of its decor (a pillar, a
//            lamp post) and the pieces of its shell (a wall, a bridge)

#include <set>
#include <string>
#include <vector>

#include "io/asset_library.h"
#include "io/game_data.h"
#include "scene/scene.h"

namespace ogle {

struct CatalogObject {
  std::string etype;
  std::string name;        // shown: the type, or the model of a variant (look, art-name)
  // prop (an object of the game that only stands there), door, water, effect: the Objects of the
  // catalog; else a family of the objects that do something (crate, platform, movement, button,
  // breakable, hazard...: GameData::object_family), the Interactive objects
  std::string family;
  bool interactive = false;
  std::string model;       // the model drawn for it, "" for an object the game draws without one
  std::string model_path;  // its file
  json lump;               // the data of a new one: an actor of the game's, without what binds it to its level
  json lump_types;         // the type of each tag of the data of its type's actors (export for the game)
  int count = 0;           // actors of this type in the extracted levels
  std::vector<std::string> levels;  // the first levels they are in
};

class LibraryIndex;

// Every object of the game, by family then name. With the index of the library (once ready), the
// art-names of every actor of the levels: all the variants and particle effects, and their counts.
std::vector<CatalogObject> build_catalog(const AssetLibrary& library, const GameData* data,
                                         const LibraryIndex* index = nullptr);

// The model the game draws for an actor, as far as the editor can tell: the model of its look for
// a water actor, the skeleton its art-name names, the first model its type loads, else a model
// named like its type. From the folder `group` first; null when none.
const ModelAsset* resolve_actor_model(const AssetLibrary& library, const GameData* data, const std::string& etype,
                                      const json& lump, const std::string& group);

struct CatalogModel {
  std::string name;   // without "-lodN"
  std::string path;   // its lowest lod
  std::string group;  // the level folder it comes from ("common" for the shared ones)
  std::string game;   // "" Jak 3's, "jak2" Jak II's (placed as decor: io/jak2_import.h)
};

// The models of the library that are not the model of an object of `objects` nor a character's,
// by name; `creatures`: more types of living things whose models are left out (Jak II's, which has
// no game data: data/jak2/creatures.json).
std::vector<CatalogModel> list_catalog_models(const AssetLibrary& library, const GameData* data,
                                              const std::vector<CatalogObject>& objects,
                                              const std::set<std::string>& creatures = {});

// The data of an actor, without what ties it to its level: its name, story (kill mask),
// visibility volume, paths and the actors it refers to.
json portable_lump(const json& lump);

// A new actor of a catalog object (Actor node, its model added by the caller).
Node make_catalog_actor(const CatalogObject& object, const std::string& name);

struct CatalogPart {
  enum class Kind { Prototype, Piece };
  Kind kind = Kind::Prototype;
  std::string name;        // the prototype ("ruin-top-tower.mb") or the piece ("ruins-canal-top-2")
  std::string mesh_node;   // the node whose mesh it is: the first instance of a prototype, the piece
  size_t triangles = 0;
  int copies = 0;          // instances of a prototype in its level
};

// The decor parts of a level as the library has it (a level's background file, not prepared for
// editing): its prototypes and the pieces of its shell that are not terrain.
std::vector<CatalogPart> list_parts(const Scene& level);

// The mesh of a part, moved so that the bottom center of its geometry is its origin (a part sits
// on the point it is placed at). Null when the level has no such part.
std::shared_ptr<Mesh> part_mesh(const Scene& level, const CatalogPart& part);

}  // namespace ogle
