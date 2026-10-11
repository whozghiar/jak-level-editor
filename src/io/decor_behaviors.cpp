#include "io/decor_behaviors.h"

#include <algorithm>
#include <cstdio>
#include <sstream>

namespace ogle {

using json = nlohmann::json;

namespace {

std::string meters_of(float m) {
  char buf[48];
  std::snprintf(buf, sizeof(buf), "%.1f", m * 4096.f);
  return buf;
}

}  // namespace

const std::vector<std::string>& collision_materials() {
  // Jak 1's surfaces, in their order: the level builder takes them and converts them to Jak 3's
  static const std::vector<std::string> v = {"stone",  "ice",      "quicksand", "waterbottom", "tar",   "sand",
                                             "wood",   "grass",    "pcmetal",   "snow",        "deepsnow", "hotcoals",
                                             "lava",   "crwood",   "gravel",    "dirt",        "metal", "straw",
                                             "tube",   "swamp",    "stopproj",  "rotate",      "neutral"};
  return v;
}

const std::vector<std::string>& collision_events() {
  static const std::vector<std::string> v = {"none", "deadly", "endlessfall", "burn", "deadlyup", "burnup", "melt"};
  return v;
}

int builder_material(const std::string& name) {
  const auto& v = collision_materials();
  auto it = std::find(v.begin(), v.end(), name);
  return it == v.end() ? -1 : (int)(it - v.begin());
}

int builder_event(const std::string& name) {
  const auto& v = collision_events();
  auto it = std::find(v.begin(), v.end(), name);
  return it == v.end() ? -1 : (int)(it - v.begin());
}

int builder_mode(const std::string& name) {
  if (name == "ground") return 0;
  if (name == "wall") return 1;
  if (name == "obstacle") return 2;
  return -1;
}

DecorBehavior decor_behavior_of(const json& extras) {
  DecorBehavior b;
  auto it = extras.find("og_decor");
  if (it == extras.end() || !it->is_object()) return b;
  const json& d = *it;
  if (auto c = d.find("collision"); c != d.end() && c->is_object()) {
    DecorCollision& k = b.collision;
    k.set = true;
    k.kind = c->value("kind", k.kind);
    if (k.kind != "none" && k.kind != "invisible") k.kind = "shape";
    k.material = c->value("material", k.material);
    if (builder_material(k.material) < 0) k.material = "stone";
    k.event = c->value("event", k.event);
    if (builder_event(k.event) < 0) k.event = "none";
    k.mode = c->value("mode", k.mode);
    if (builder_mode(k.mode) < 0) k.mode = "auto";
    k.noedge = c->value("noedge", false);
    k.nocamera = c->value("nocamera", false);
    k.noentity = c->value("noentity", false);
    k.nolineofsight = c->value("nolineofsight", false);
  }
  if (auto c = d.find("bounce"); c != d.end() && c->is_object()) {
    b.bounce.set = true;
    b.bounce.height = std::clamp(c->value("height", b.bounce.height), 0.5f, 200.f);
  }
  if (auto c = d.find("launch"); c != d.end() && c->is_object()) {
    DecorLaunch& l = b.launch;
    l.set = true;
    l.distance = std::clamp(c->value("distance", l.distance), 0.f, 500.f);
    l.height = std::clamp(c->value("height", l.height), -200.f, 200.f);
    l.yaw = c->value("yaw", l.yaw);
    l.time = std::clamp(c->value("time", l.time), 0.2f, 10.f);
  }
  return b;
}

json decor_behavior_json(const DecorBehavior& b) {
  if (!b.any()) return json();
  json j = json::object();
  if (b.collision.set) {
    const DecorCollision& c = b.collision;
    j["collision"] = {{"kind", c.kind}, {"material", c.material}, {"event", c.event}, {"mode", c.mode}};
    for (auto [key, on] : {std::pair{"noedge", c.noedge}, {"nocamera", c.nocamera}, {"noentity", c.noentity},
                           {"nolineofsight", c.nolineofsight}})
      if (on) j["collision"][key] = true;
  }
  if (b.bounce.set) j["bounce"] = {{"height", b.bounce.height}};
  if (b.launch.set)
    j["launch"] = {{"distance", b.launch.distance}, {"height", b.launch.height}, {"yaw", b.launch.yaw}, {"time", b.launch.time}};
  return j;
}

json collision_extras(const DecorCollision& c) {
  if (c.kind == "none") return json{{"set_collision", 1}, {"ignore", 1}};
  json j = {{"set_collision", 1},
            {"collide_material", std::max(0, builder_material(c.material))},
            {"collide_event", std::max(0, builder_event(c.event))}};
  if (builder_mode(c.mode) >= 0) j["collide_mode"] = builder_mode(c.mode);
  if (c.kind == "invisible") j["set_invisible"] = 1;
  if (c.noedge) j["noedge"] = 1;
  if (c.nocamera) j["nocamera"] = 1;
  if (c.noentity) j["noentity"] = 1;
  if (c.nolineofsight) j["nolineofsight"] = 1;
  return j;
}

std::string bounce_code(const std::string& type, float radius, const std::string& comment) {
  // the game's bouncer without a model (as the industrial zone's board vents, ctyinda-obs.gc): a
  // sphere over the element that Jak overlaps. On the board and in a dive the bouncer's own events
  // throw him; on foot, touching it from above while not going up does
  std::ostringstream o;
  if (!comment.empty()) o << ";; " << comment << "\n";
  o << "(deftype " << type << " (bouncer) ())\n\n"
    << "(defmethod init-skel! ((this " << type << "))\n"
    << "  0\n"
    << "  (none)\n"
    << "  )\n\n"
    << "(defmethod init-collision! ((this " << type << "))\n"
    << "  (let ((s5-0 (new 'process 'collide-shape this (collide-list-enum hit-by-player))))\n"
    << "    (let ((v1-2 (new 'process 'collide-shape-prim-sphere s5-0 (the-as uint 9))))\n"
    << "      (set! (-> v1-2 prim-core collide-as) (collide-spec crate))\n"
    << "      (set! (-> v1-2 prim-core collide-with) (collide-spec jak player-list))\n"
    << "      (set! (-> v1-2 prim-core action) (collide-action))\n"
    << "      (set-vector! (-> v1-2 local-sphere) 0.0 2048.0 0.0 " << meters_of(radius) << ")\n"
    << "      (set! (-> s5-0 total-prims) (the-as uint 1))\n"
    << "      (set! (-> s5-0 root-prim) v1-2)\n"
    << "      )\n"
    << "    (set! (-> s5-0 nav-radius) (* 0.75 (-> s5-0 root-prim local-sphere w)))\n"
    << "    (let ((v1-5 (-> s5-0 root-prim)))\n"
    << "      (set! (-> s5-0 backup-collide-as) (-> v1-5 prim-core collide-as))\n"
    << "      (set! (-> s5-0 backup-collide-with) (-> v1-5 prim-core collide-with))\n"
    << "      )\n"
    << "    (set! (-> this root) s5-0)\n"
    << "    )\n"
    << "  0\n"
    << "  (none)\n"
    << "  )\n\n"
    << "(defstate idle (" << type << ")\n"
    << "  :virtual #t\n"
    << "  :event (behavior ((proc process) (argc int) (message symbol) (block event-message-block))\n"
    << "    (case message\n"
    << "      (('touch)\n"
    << "       (when (and (= proc *target*)\n"
    << "                  (< (- (-> self root trans y) 1228.8) (-> *target* control trans y))\n"
    << "                  (>= 0.0 (vector-dot (-> *target* control transv) (-> *target* control dynam gravity-normal)))\n"
    << "                  (send-event proc 'jump (-> self spring-height) (-> self spring-height) (-> self mods))\n"
    << "                  )\n"
    << "         (play-sound self)\n"
    << "         (go-virtual fire)\n"
    << "         )\n"
    << "       )\n"
    << "      (else\n"
    << "        (call-parent-state-handler event :type (function process int symbol event-message-block object)\n"
    << "                                   proc argc message block)\n"
    << "        )\n"
    << "      )\n"
    << "    )\n"
    << "  )\n\n";
  return o.str();
}

std::string launch_code(const std::string& type, float radius, const float to[3], float time, const std::string& comment) {
  // the game's launch (target-launch-dir, sent the way mhcity-obs2.gc's puffers send it: the way to
  // the landing point, the time, the gravity -327680.0 as its bits), when Jak comes on the element
  char landing[160], seconds[32];
  std::snprintf(landing, sizeof(landing), "%.1f %.1f %.1f", to[0], to[1], to[2]);
  std::snprintf(seconds, sizeof(seconds), "%d", (int)(time * 300.f + 0.5f));
  std::ostringstream o;
  if (!comment.empty()) o << ";; " << comment << "\n";
  o << "(deftype " << type << " (process-drawable)\n"
    << "  ((landing  vector  :inline))\n"
    << "  (:state-methods\n"
    << "    idle\n"
    << "    )\n"
    << "  )\n\n"
    << "(defmethod init-from-entity! ((this " << type << ") (arg0 entity-actor))\n"
    << "  (set! (-> this root) (new 'process 'trsqv))\n"
    << "  (process-drawable-from-entity! this arg0)\n"
    << "  (set-vector! (-> this landing) " << landing << " 1.0)\n"
    << "  (go (method-of-object this idle))\n"
    << "  )\n\n"
    << "(defstate idle (" << type << ")\n"
    << "  :virtual #t\n"
    << "  :trans (behavior ()\n"
    << "    (when (and *target*\n"
    << "               (< (vector-vector-xz-distance (-> self root trans) (target-pos 0)) " << meters_of(radius) << ")\n"
    << "               (< (fabs (- (-> (target-pos 0) y) (-> self root trans y))) 6144.0)\n"
    << "               )\n"
    << "      (send-event *target* 'launch-dir\n"
    << "                  (vector-! (new 'stack-no-clear 'vector) (-> self landing) (target-pos 0))\n"
    << "                  " << seconds << "\n"
    << "                  -929038336\n"
    << "                  )\n"
    << "      )\n"
    << "    )\n"
    << "  :code (behavior ()\n"
    << "    (until #f\n"
    << "      (suspend)\n"
    << "      )\n"
    << "    #f\n"
    << "    )\n"
    << "  )\n\n";
  return o.str();
}

}  // namespace ogle
