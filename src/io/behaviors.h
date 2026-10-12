#pragma once

// The behaviors of the game's objects the editor sets, kept in a node's extras "og_behavior":
// when a door opens, when a board vent throws Jak up, when an elevator leaves, whether an object
// is there at all. The export writes them as generated GOAL code (<level>-obs.gc): a type of the
// level deriving the object's own type, whose methods or states hold the choice, since OpenGOAL's
// level builder cannot write the game's scripts (pair res lumps).
//
//   og_behavior.open      a door (com-airlock and its subtypes): "approach" (the default),
//                         "locked", "always", "when" (once `when` holds); "distance": meters
//                         Jak opens it from (0: the game's); "loads": a level (the rooms of a
//                         house, a level of their own) loaded and shown when Jak comes to the
//                         door, kept with the levels wanted already: the door opens once it is
//   og_behavior.active    a bouncer (board vents, jump pads, awnings): when it throws Jak up
//   og_behavior.leave     an elevator: when it leaves once ridden
//   og_behavior.present   any object: whether it is there, checked when the level loads
//
// A condition is {"when": "always" | "never" | "task" | "pressed" | "gone", "arg": ...}: always,
// never, once the task node `arg` of the story is closed, once the actor named `arg` was
// activated (a floor switch pressed: its subtask-complete status), once it is gone (a crate
// broken, a pickup taken: its dead status).

#include <string>
#include <vector>

#include "json.hpp"

namespace ogle {

class GameData;

enum class BehaviorKind { Other, Door, Bouncer, Elevator };
BehaviorKind behavior_kind(const GameData* data, const std::string& etype);

struct Condition {
  enum Kind { Always, Never, Task, Pressed, Gone };
  Kind kind = Always;
  std::string arg;
};
Condition condition_of(const nlohmann::json& j, Condition fallback = {});
nlohmann::json condition_json(const Condition& c);
// The condition as a script of the game: "#t", "(task-closed? \"x\")"...
std::string condition_script(const Condition& c);
// The condition as a GOAL test: "#t", "#f", or "(script-eval '<script>)".
std::string condition_test(const Condition& c);
// "always", "never", "task", "pressed", "gone"
const char* condition_name(Condition::Kind k);

// The GOAL code of an actor of type `etype` with these behaviors, as a type `type` deriving it;
// "" when they need no code (the object as the game has it). `comment` heads the code.
std::string behavior_code(const GameData* data, const std::string& etype, const nlohmann::json& behavior,
                          const std::string& type, const std::string& comment);

// A name for the generated type of an actor: "<level>-<actor name>", lower case letters, digits
// and '-' only.
std::string behavior_type_name(const std::string& level, const std::string& actor);

}  // namespace ogle
