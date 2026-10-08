#include "io/behaviors.h"

#include <cctype>
#include <sstream>

#include "io/game_data.h"

namespace ogle {

using json = nlohmann::json;

namespace {

// the names a script holds between quotes: task nodes and actor names
std::string clean_name(const std::string& s) {
  std::string out;
  for (char c : s)
    if (std::isalnum((unsigned char)c) || c == '-' || c == '_' || c == '.') out += c;
  return out;
}

// what the game's own doors test before opening (airlock.gc): no cutscene, Jak not grabbed
const char* kDoorFree = "(not (or (scene-player?) (focus-test? *target* grabbed)))";

}  // namespace

// The condition tested in GOAL: #t, #f, or the game's script evaluator on its script (a list:
// script-eval takes nothing else).
std::string condition_test(const Condition& c) {
  if (c.kind == Condition::Always) return "#t";
  if (c.kind == Condition::Never) return "#f";
  return "(script-eval '" + condition_script(c) + ")";
}

BehaviorKind behavior_kind(const GameData* data, const std::string& etype) {
  if (!data) return BehaviorKind::Other;
  if (data->derives_from(etype, "com-airlock")) return BehaviorKind::Door;
  if (data->derives_from(etype, "bouncer")) return BehaviorKind::Bouncer;
  if (data->derives_from(etype, "elevator")) return BehaviorKind::Elevator;
  return BehaviorKind::Other;
}

const char* condition_name(Condition::Kind k) {
  switch (k) {
    case Condition::Always: return "always";
    case Condition::Never: return "never";
    case Condition::Task: return "task";
    case Condition::Pressed: return "pressed";
    case Condition::Gone: return "gone";
  }
  return "always";
}

Condition condition_of(const json& j, Condition fallback) {
  if (!j.is_object()) return fallback;
  const std::string when = j.value("when", std::string());
  Condition c = fallback;
  for (auto k : {Condition::Always, Condition::Never, Condition::Task, Condition::Pressed, Condition::Gone})
    if (when == condition_name(k)) c.kind = k;
  c.arg = j.value("arg", std::string());
  // a condition on nothing named: as the fallback
  if ((c.kind == Condition::Task || c.kind == Condition::Pressed || c.kind == Condition::Gone) &&
      clean_name(c.arg).empty())
    return fallback;
  return c;
}

json condition_json(const Condition& c) {
  json j = {{"when", condition_name(c.kind)}};
  if (!c.arg.empty()) j["arg"] = c.arg;
  return j;
}

std::string condition_script(const Condition& c) {
  const std::string a = clean_name(c.arg);
  switch (c.kind) {
    case Condition::Always: return "#t";
    case Condition::Never: return "#f";
    case Condition::Task: return "(task-closed? \"" + a + "\")";
    case Condition::Pressed: return "(entity-status? \"" + a + "\" subtask-complete)";
    case Condition::Gone: return "(entity-status? \"" + a + "\" dead)";
  }
  return "#t";
}

std::string behavior_type_name(const std::string& level, const std::string& actor) {
  std::string out;
  for (char c : level + "-" + actor) {
    const char l = (char)std::tolower((unsigned char)c);
    if (std::isalnum((unsigned char)l)) out += l;
    else if (!out.empty() && out.back() != '-') out += '-';
  }
  while (!out.empty() && out.back() == '-') out.pop_back();
  return out;
}

std::string behavior_code(const GameData* data, const std::string& etype, const json& b, const std::string& type,
                          const std::string& comment) {
  if (!b.is_object()) return {};
  const BehaviorKind kind = behavior_kind(data, etype);
  std::ostringstream body;

  // there or not, when the level loads
  const Condition present = condition_of(b.value("present", json()));
  if (present.kind != Condition::Always && present.kind != Condition::Never) {
    body << "(defmethod init-from-entity! ((this " << type << ") (arg0 entity-actor))\n"
         << "  (if " << condition_test(present) << "\n"
         << "      (call-parent-method this arg0)\n"
         << "      (deactivate this)\n"
         << "      )\n"
         << "  (none)\n"
         << "  )\n\n";
  }

  if (kind == BehaviorKind::Door) {
    const std::string open = b.value("open", std::string("approach"));
    const Condition when = condition_of(b.value("when", json()));
    const double distance = b.value("distance", 0.0);
    const bool locked = open == "locked" || (open == "when" && when.kind == Condition::Never);
    // without its on-notice script (the builder cannot write it), a door of the game never opens:
    // every door that opens says that the levels behind it are there
    if (!locked) {
      body << "(defmethod destination-loaded? ((this " << type << ") (arg0 symbol))\n"
           << "  #t\n"
           << "  )\n\n";
      std::string test;
      if (open == "always") test = "(not #f)";
      else if (open == "when" && when.kind != Condition::Always)
        test = std::string("(and ") + kDoorFree + " " + condition_script(when) + ")";
      const double d = open == "always" ? 1000.0 : distance;
      if (!test.empty() || d > 0) {
        body << "(defmethod init-airlock! ((this " << type << "))\n"
             << "  (call-parent-method this)\n";
        if (!test.empty()) body << "  (set! (-> this open-test) '" << test << ")\n";
        if (d > 0) {
          char buf[64];
          std::snprintf(buf, sizeof(buf), "%.2f", d);
          char active[64];
          std::snprintf(active, sizeof(active), "%.2f", open == "always" ? d : d + 35.0);
          body << "  (set! (-> this open-distance 0) (meters " << buf << "))\n"
               << "  (set! (-> this open-distance 1) (meters " << buf << "))\n"
               << "  (set! (-> this active-distance 0) (meters " << active << "))\n"
               << "  (set! (-> this active-distance 1) (meters " << active << "))\n";
        }
        body << "  this\n"
             << "  )\n\n";
      }
    }
  }

  if (kind == BehaviorKind::Bouncer) {
    const Condition active = condition_of(b.value("active", json()));
    if (active.kind != Condition::Always) {
      body << "(defstate idle (" << type << ")\n"
           << "  :virtual #t\n"
           << "  :event (behavior ((proc process) (argc int) (message symbol) (block event-message-block))\n"
           << "    (when " << condition_test(active) << "\n"
           << "      (call-parent-state-handler event :type (function process int symbol event-message-block object)\n"
           << "                                 proc argc message block)\n"
           << "      )\n"
           << "    )\n"
           << "  )\n\n";
    }
  }

  if (kind == BehaviorKind::Elevator) {
    const Condition leave = condition_of(b.value("leave", json()));
    if (leave.kind != Condition::Always) {
      // its activate-test: a script (a list), "#f" written as one
      const std::string test = leave.kind == Condition::Never ? "(not #t)" : condition_script(leave);
      body << "(defstate waiting (" << type << ")\n"
           << "  :virtual #t\n"
           << "  :enter (behavior ()\n"
           << "    (set! (-> self activate-test) '" << test << ")\n"
           << "    (call-parent-state-handler enter)\n"
           << "    )\n"
           << "  )\n\n";
    }
  }

  const std::string methods = body.str();
  if (methods.empty()) return {};
  std::ostringstream out;
  if (!comment.empty()) out << ";; " << comment << "\n";
  out << "(deftype " << type << " (" << etype << ") ())\n\n" << methods;
  return out.str();
}

}  // namespace ogle
