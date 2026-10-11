// The inspector's sections of an actor (panels.cpp shows them): its behavior, set in simple terms
// for the objects whose behavior is the game's code (doors, bouncers, elevators, whether an object
// is there at all: io/behaviors.h), and its parameters, every res lump the code of its type reads
// (game data: params), typed.

#include <algorithm>
#include <cmath>
#include <cstring>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "imgui.h"
#include "imgui_stdlib.h"
#include "io/behaviors.h"
#include "io/decor_behaviors.h"
#include "io/game_export.h"

namespace ogle {

namespace {

constexpr float kUnits = 4096.f;  // game units per meter

void tooltip(const char* text) {
  if (ImGui::IsItemHovered(ImGuiHoveredFlags_DelayShort) && text && *text) ImGui::SetTooltip("%s", text);
}

// the parameters given in meters: distances, heights, sizes
bool in_meters(const std::string& tag) {
  for (const char* w : {"distance", "height", "radius", "dist", "width", "range", "threshold", "depth", "offset"})
    if (tag.find(w) != std::string::npos) return true;
  return false;
}

// a lump value as numbers (a number, or an array of them)
std::vector<double> numbers_of(const json& v) {
  std::vector<double> out;
  if (v.is_number()) out.push_back(v.get<double>());
  else if (v.is_array())
    for (const auto& x : v)
      if (x.is_number()) out.push_back(x.get<double>());
  return out;
}

}  // namespace

// --- a condition: always, never, after a task, once an actor was activated or is gone ----------

bool App::ui_condition(const char* id, json& value, bool with_always) {
  Condition c = condition_of(value);
  bool changed = false;
  static const Condition::Kind kinds[] = {Condition::Always, Condition::Never, Condition::Task, Condition::Pressed,
                                          Condition::Gone};
  auto label = [](Condition::Kind k) { return tr((std::string("behavior.when.") + condition_name(k)).c_str()); };
  ImGui::PushID(id);
  ImGui::SetNextItemWidth(-1);
  if (ImGui::BeginCombo("##when", label(c.kind))) {
    for (auto k : kinds) {
      if (k == Condition::Always && !with_always) continue;
      if (ImGui::Selectable(label(k), k == c.kind) && k != c.kind) {
        c.kind = k;
        c.arg.clear();
        changed = true;
      }
    }
    ImGui::EndCombo();
  }
  if (c.kind == Condition::Task) {
    // a task node of the story: closed once the player did that step
    const GameData* data = game_data().get();
    ImGui::SetNextItemWidth(-1);
    if (ImGui::BeginCombo("##task", c.arg.empty() ? tr("behavior.choose_task") : c.arg.c_str(),
                          ImGuiComboFlags_HeightLarge)) {
      static std::string filter;
      ImGui::SetNextItemWidth(-1);
      if (ImGui::IsWindowAppearing()) ImGui::SetKeyboardFocusHere();
      ImGui::InputTextWithHint("##filter", tr("behavior.filter"), &filter);
      for (int i = 1; data && i < data->step_count(); i++) {
        const std::string& name = data->node(i).name;
        if (!filter.empty() && name.find(filter) == std::string::npos) continue;
        if (ImGui::Selectable(name.c_str(), name == c.arg)) {
          c.arg = name;
          changed = true;
        }
      }
      ImGui::EndCombo();
    }
    tooltip(tr("behavior.task.tip"));
  } else if (c.kind == Condition::Pressed || c.kind == Condition::Gone) {
    // another actor of the document, by its name
    ImGui::SetNextItemWidth(-1);
    if (ImGui::BeginCombo("##actor", c.arg.empty() ? tr("behavior.choose_actor") : c.arg.c_str(),
                          ImGuiComboFlags_HeightLarge)) {
      for (const auto& o : scene.nodes) {
        if (o.kind != NodeKind::Actor || o.name.empty()) continue;
        if (ImGui::Selectable(o.name.c_str(), o.name == c.arg)) {
          c.arg = o.name;
          changed = true;
        }
      }
      ImGui::EndCombo();
    }
    tooltip(tr(c.kind == Condition::Pressed ? "behavior.pressed.tip" : "behavior.gone.tip"));
  }
  ImGui::PopID();
  if (changed) value = condition_json(c);
  return changed;
}

// --- the behavior of an actor --------------------------------------------------------------------

void App::ui_actor_behavior(Node& n) {
  const GameData* data = game_data().get();
  const std::string etype = n.extras.value("etype", std::string());
  if (!data || etype.empty()) return;
  const BehaviorKind kind = behavior_kind(data, etype);
  ImGui::SeparatorText(tr("behavior.title"));
  json b = n.extras.value("og_behavior", json::object());
  if (!b.is_object()) b = json::object();
  const Node before = n;
  bool changed = false;
  auto lump_float = [&](const char* tag, float fallback) {
    auto l = n.extras.find("lump");
    if (l != n.extras.end() && l->is_object() && l->contains(tag)) {
      const auto v = numbers_of((*l)[tag]);
      if (!v.empty()) return (float)v[0];
    }
    if (const auto* ps = data->params(etype))
      for (const auto& p : *ps)
        if (p.tag == tag && p.def.is_number()) return p.def.get<float>();
    return fallback;
  };
  auto set_lump = [&](const char* tag, const json& v, const char* type) {
    if (!n.extras.contains("lump") || !n.extras["lump"].is_object()) n.extras["lump"] = json::object();
    if (!n.extras.contains("lump_types") || !n.extras["lump_types"].is_object()) n.extras["lump_types"] = json::object();
    n.extras["lump"][tag] = v;
    n.extras["lump_types"][tag] = type;
  };

  if (kind == BehaviorKind::Door) {
    static const char* modes[] = {"approach", "locked", "always", "when"};
    std::string open = b.value("open", std::string("approach"));
    ImGui::TextUnformatted(tr("behavior.door.open"));
    ImGui::SetNextItemWidth(-1);
    if (ImGui::BeginCombo("##door", tr(("behavior.door." + open).c_str()))) {
      for (const char* m : modes)
        if (ImGui::Selectable(tr((std::string("behavior.door.") + m).c_str()), open == m) && open != m) {
          b["open"] = m;
          open = m;
          changed = true;
        }
      ImGui::EndCombo();
    }
    tooltip(tr(("behavior.door." + open + ".tip").c_str()));
    if (open == "when") {
      json w = b.value("when", json::object());
      if (ui_condition("door-when", w, false)) {
        b["when"] = w;
        changed = true;
      }
    }
    if (open == "approach" || open == "when") {
      float d = b.value("distance", 0.0f);
      ImGui::SetNextItemWidth(-160);
      ImGui::DragFloat(tr_id("behavior.door.distance").c_str(), &d, 0.1f, 0.f, 200.f, d > 0 ? "%.1f m" : "-");
      edit_begin(before);
      if (ImGui::IsItemEdited()) {
        n.extras["og_behavior"] = b;
        n.extras["og_behavior"]["distance"] = std::max(0.f, d);
      }
      edit_end(tr("undo.behavior"));
      tooltip(tr("behavior.door.distance.tip"));
    }
  } else if (kind == BehaviorKind::Bouncer) {
    ImGui::TextUnformatted(tr("behavior.bouncer.active"));
    json a = b.value("active", json::object());
    if (ui_condition("bouncer", a, true)) {
      b["active"] = a;
      changed = true;
    }
    float h = lump_float("spring-height", 45056.f) / kUnits;
    ImGui::SetNextItemWidth(-160);
    ImGui::DragFloat(tr_id("behavior.bouncer.height").c_str(), &h, 0.1f, 0.f, 200.f, "%.1f m");
    edit_begin(before);
    if (ImGui::IsItemEdited()) set_lump("spring-height", std::max(0.f, h) * kUnits, "float");
    edit_end(tr("undo.behavior"));
    tooltip(tr("behavior.bouncer.height.tip"));
  } else if (kind == BehaviorKind::Elevator) {
    ImGui::TextUnformatted(tr("behavior.elevator.leave"));
    json l = b.value("leave", json::object());
    if (ui_condition("elevator", l, true)) {
      b["leave"] = l;
      changed = true;
    }
    float v = lump_float("elevator-move-rate", 25600.f) / kUnits;
    ImGui::SetNextItemWidth(-160);
    ImGui::DragFloat(tr_id("behavior.elevator.speed").c_str(), &v, 0.05f, 0.1f, 100.f, "%.2f m/s");
    edit_begin(before);
    if (ImGui::IsItemEdited()) set_lump("elevator-move-rate", std::max(0.1f, v) * kUnits, "float");
    edit_end(tr("undo.behavior"));
  }

  if (data->derives_from(etype, "crate")) {
    // what kind of crate, what it holds
    static const char* looks[] = {"wood", "iron", "steel", "metal", "blue", "darkeco"};
    std::string look = "wood";
    if (auto l = n.extras.find("lump"); l != n.extras.end() && l->is_object() && l->contains("crate-type")) {
      const json& v = (*l)["crate-type"];
      if (v.is_string()) look = v.get<std::string>();
      if (!look.empty() && look[0] == '\'') look = look.substr(1);
    }
    ImGui::SetNextItemWidth(-160);
    if (ImGui::BeginCombo(tr_id("behavior.crate.type").c_str(), look.c_str())) {
      for (const char* k : looks)
        if (ImGui::Selectable(k, look == k) && look != k) {
          set_lump("crate-type", std::string("'") + k, "symbol");
          commit_node_edit(before, tr("undo.behavior"));
        }
      ImGui::EndCombo();
    }
  }

  // there or not
  ImGui::TextUnformatted(tr("behavior.present"));
  json p = b.value("present", json::object());
  if (ui_condition("present", p, true)) {
    b["present"] = p;
    changed = true;
  }
  tooltip(tr("behavior.present.tip"));

  if (changed) {
    n.extras["og_behavior"] = b;
    commit_node_edit(before, tr("undo.behavior"));
  }
}

// --- what the game does with an element of the decor (io/decor_behaviors.h) -----------------------

void App::ui_decor_behavior(Node& n) {
  if (n.kind != NodeKind::Render || !n.mesh || scene.actor_of(n)) return;
  ImGui::SeparatorText(tr("decor.title"));
  guide_tip("ingame");
  // every selected piece of decor gets what is set here
  std::vector<uint32_t> targets;
  for (auto id : selection)
    if (const Node* s = scene.find(id); s && s->kind == NodeKind::Render && s->mesh && !scene.actor_of(*s))
      targets.push_back(id);
  if (std::find(targets.begin(), targets.end(), n.id) == targets.end()) targets.push_back(n.id);
  DecorBehavior b = decor_behavior_of(n.extras);
  bool changed = false, dragging = false;
  // a list of choices, each with what it does (in the list, and under it for the one chosen); the
  // options in groups (a title key, or null)
  using Groups = std::vector<std::pair<const char*, std::vector<std::string>>>;
  auto choice = [&](const char* id, const char* prefix, std::string& value, const Groups& groups) {
    bool edited = false;
    const std::string p = prefix;
    ImGui::SetNextItemWidth(-1);
    if (ImGui::BeginCombo(id, tr((p + value).c_str()), ImGuiComboFlags_HeightLargest)) {
      const float wrap = ImGui::GetFontSize() * 21.f;
      for (const auto& [group, options] : groups) {
        if (group) ImGui::SeparatorText(tr(group));
        for (const auto& o : options) {
          ImGui::PushID(o.c_str());
          ImGui::BeginGroup();
          bool picked = ImGui::Selectable(tr((p + o).c_str()), o == value, 0, ImVec2(wrap, 0));
          ImGui::Indent();
          ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + wrap - ImGui::GetStyle().IndentSpacing);
          ImGui::TextDisabled("%s", tr((p + o + ".tip").c_str()));
          ImGui::PopTextWrapPos();
          ImGui::Unindent();
          ImGui::EndGroup();
          // its explanation picks it too
          if (!picked && ImGui::IsItemHovered() && ImGui::IsMouseClicked(ImGuiMouseButton_Left)) {
            picked = true;
            ImGui::CloseCurrentPopup();
          }
          if (picked && o != value) {
            value = o;
            edited = true;
          }
          ImGui::PopID();
        }
      }
      ImGui::EndCombo();
    }
    ImGui::PushTextWrapPos(0.f);
    ImGui::TextDisabled("%s", tr((p + value + ".tip").c_str()));
    ImGui::PopTextWrapPos();
    return edited;
  };
  // its collision: yes or no; by default what the export gives it (the game's own, made from its
  // shape, or none: a small piece, a surface seen through)
  {
    static uint64_t checked_rev = ~0ull;
    static uint32_t checked_id = 0;
    static DefaultCollision by_default = DefaultCollision::Shape;
    if (checked_rev != history.revision() || checked_id != n.id) {
      checked_rev = history.revision();
      checked_id = n.id;
      const int index = scene.index_of(n.id);
      by_default = index >= 0 ? default_decor_collision(scene, (size_t)index) : DefaultCollision::Shape;
    }
    const bool default_on = by_default == DefaultCollision::Game || by_default == DefaultCollision::Shape;
    bool on = b.collision.set ? b.collision.kind != "none" : default_on;
    if (ImGui::Checkbox(tr_id("decor.collision").c_str(), &on)) {
      b.collision.set = true;
      b.collision.kind = on ? (b.collision.kind == "invisible" ? "invisible" : "shape") : "none";
      changed = true;
    }
    guide_tip("collision");
    const char* why = !b.collision.set                         ? by_default == DefaultCollision::Game         ? "decor.collision.game"
                                                                 : by_default == DefaultCollision::SeeThrough ? "decor.collision.see_through"
                                                                 : by_default == DefaultCollision::Small      ? "decor.collision.small"
                                                                                                              : "decor.collision.shape"
                      : !on                                     ? "decor.collision.off"
                      : b.collision.kind == "invisible"         ? "decor.collision.invisible_on"
                                                                : "decor.collision.on";
    ImGui::Indent();
    ImGui::PushTextWrapPos(0.f);
    ImGui::TextDisabled("%s", tr(why));
    ImGui::PopTextWrapPos();
    ImGui::Unindent();
    if (on) {
      // what Jak walks on: the sound of his steps and their dust, and for a few how he moves
      ImGui::TextUnformatted(tr("decor.material"));
      guide_tip("surface");
      std::string material = b.collision.material;
      if (choice("##decor-material", "decor.material.", material,
                 {{"decor.material.group.sound",
                   {"stone", "metal", "wood", "grass", "dirt", "sand", "snow", "deepsnow", "straw", "crwood", "pcmetal",
                    "hotcoals", "lava", "tar", "swamp", "rotate", "neutral"}},
                  {"decor.material.group.move", {"ice", "gravel", "quicksand", "tube", "waterbottom"}},
                  {"decor.material.group.special", {"stopproj"}}})) {
        b.collision.set = true;
        b.collision.material = material;
        changed = true;
      }
      // what touching it does to Jak
      ImGui::TextUnformatted(tr("decor.event"));
      guide_tip("event");
      std::string event = b.collision.event;
      if (choice("##decor-event", "decor.event.", event,
                 {{nullptr, {"none"}},
                  {"decor.event.group.hurt", {"deadly", "burn", "deadlyup", "burnup"}},
                  {"decor.event.group.death", {"endlessfall", "melt"}}})) {
        b.collision.set = true;
        b.collision.event = event;
        changed = true;
      }
      if (ImGui::TreeNode(tr_id("decor.more").c_str())) {
        // not drawn in the game, it collides: an invisible wall
        bool invisible = b.collision.set && b.collision.kind == "invisible";
        if (ImGui::Checkbox(tr_id("decor.invisible").c_str(), &invisible)) {
          b.collision.set = true;
          b.collision.kind = invisible ? "invisible" : "shape";
          changed = true;
        }
        tooltip(tr("decor.invisible.tip"));
        std::string mode = b.collision.mode;
        ImGui::TextUnformatted(tr("decor.mode"));
        if (choice("##decor-mode", "decor.mode.", mode, {{nullptr, {"auto", "ground", "wall", "obstacle"}}})) {
          b.collision.set = true;
          b.collision.mode = mode;
          changed = true;
        }
        for (auto [key, flag] : {std::pair{"noedge", &b.collision.noedge}, {"nocamera", &b.collision.nocamera},
                                 {"noentity", &b.collision.noentity}, {"nolineofsight", &b.collision.nolineofsight}}) {
          if (ImGui::Checkbox(tr_id((std::string("decor.flag.") + key).c_str()).c_str(), flag)) {
            b.collision.set = true;
            changed = true;
          }
          tooltip(tr((std::string("decor.flag.") + key + ".tip").c_str()));
        }
        ImGui::TreePop();
      }
    }
    if (b.collision.set && ImGui::SmallButton(tr_id("decor.collision.reset").c_str())) {
      b.collision = DecorCollision{};
      changed = true;
    }
  }
  // what it does to Jak on top of it
  if (ImGui::Checkbox(tr_id("decor.bounce").c_str(), &b.bounce.set)) changed = true;
  guide_tip("bounce");
  if (b.bounce.set) {
    ImGui::SetNextItemWidth(-160);
    if (ImGui::DragFloat(tr_id("decor.bounce.height").c_str(), &b.bounce.height, 0.1f, 0.5f, 200.f, "%.1f m"))
      dragging = true;
  }
  if (ImGui::Checkbox(tr_id("decor.launch").c_str(), &b.launch.set)) changed = true;
  guide_tip("launch");
  if (b.launch.set) {
    ImGui::PushItemWidth(-160);
    dragging |= ImGui::DragFloat(tr_id("decor.launch.distance").c_str(), &b.launch.distance, 0.2f, 0.f, 500.f, "%.1f m");
    dragging |= ImGui::DragFloat(tr_id("decor.launch.height").c_str(), &b.launch.height, 0.1f, -200.f, 200.f, "%.1f m");
    dragging |= ImGui::DragFloat(tr_id("decor.launch.yaw").c_str(), &b.launch.yaw, 1.f, -180.f, 180.f, "%.0f deg");
    dragging |= ImGui::DragFloat(tr_id("decor.launch.time").c_str(), &b.launch.time, 0.02f, 0.2f, 10.f, "%.2f s");
    ImGui::PopItemWidth();
    tooltip(tr("decor.launch.values.tip"));
  }
  // the same on every target: one undo step for a change, one per drag (when it ends)
  const json value = decor_behavior_json(b);
  auto write = [&]() {
    for (auto id : targets)
      if (Node* t = scene.find(id)) {
        if (value.is_null()) t->extras.erase("og_decor");
        else t->extras["og_decor"] = value;
      }
  };
  if (changed) {
    NodeChange c(scene);
    for (auto id : targets) c.save(id);
    write();
    history.push(c.commit(tr("undo.decor")));
  } else if (dragging) {
    if (!m_decor_edit) {
      m_decor_edit = std::make_unique<NodeChange>(scene);
      for (auto id : targets) m_decor_edit->save(id);
    }
    write();
  }
  if (m_decor_edit && !dragging && !ImGui::IsAnyItemActive()) {
    history.push(m_decor_edit->commit(tr("undo.decor")));
    m_decor_edit.reset();
  }
}

// --- the parameters of an actor: the res lumps its type's code reads -----------------------------

void App::ui_actor_params(Node& n) {
  const GameData* data = game_data().get();
  const std::string etype = n.extras.value("etype", std::string());
  const auto* params = data ? data->params(etype) : nullptr;
  if (!params || params->empty()) return;
  if (!ImGui::CollapsingHeader(strf(tr("params.title"), params->size()).c_str())) return;
  ImGui::TextWrapped("%s", tr("params.help"));
  if (!n.extras.contains("lump") || !n.extras["lump"].is_object()) n.extras["lump"] = json::object();
  if (!n.extras.contains("lump_types") || !n.extras["lump_types"].is_object()) n.extras["lump_types"] = json::object();
  const Node before = n;
  json& lump = n.extras["lump"];
  json& types = n.extras["lump_types"];
  bool committed = false;
  // a change made at once (not dragged): one undo step, after the change
  auto commit = [&](const std::string& tag, const json& v, const char* type) {
    lump[tag] = v;
    types[tag] = type;
    if (!committed) commit_node_edit(before, tr("undo.param"));
    committed = true;
  };
  ImGui::PushItemWidth(-150);
  for (const auto& p : *params) {
    ImGui::PushID(p.tag.c_str());
    const bool set = lump.contains(p.tag);
    const json value = set ? lump[p.tag] : p.def;
    const char* label = p.tag.c_str();
    if (p.kind == "float") {
      const auto v = numbers_of(value);
      const bool meters = in_meters(p.tag);
      float f = (float)(v.empty() ? 0.0 : v[0]) / (meters ? kUnits : 1.f);
      ImGui::DragFloat(label, &f, meters ? 0.1f : 0.01f, 0, 0, meters ? "%.2f m" : "%.3f");
      edit_begin(before);
      if (ImGui::IsItemEdited()) {
        lump[p.tag] = f * (meters ? kUnits : 1.f);
        types[p.tag] = "float";
      }
      edit_end(tr("undo.param"));
    } else if (p.kind == "floats" || p.kind == "ints") {
      // a list of numbers, separated by spaces
      std::string text;
      for (double d : numbers_of(value)) {
        char buf[32];
        std::snprintf(buf, sizeof(buf), "%g", d);
        text += (text.empty() ? "" : " ") + std::string(buf);
      }
      if (ImGui::InputText(label, &text, ImGuiInputTextFlags_EnterReturnsTrue)) {
        json arr = json::array();
        const char* s = text.c_str();
        char* end = nullptr;
        for (double d = std::strtod(s, &end); end != s; s = end, d = std::strtod(s, &end))
          arr.push_back(p.kind == "ints" ? json((int64_t)std::llround(d)) : json(d));
        commit(p.tag, arr, p.kind == "ints" ? "int32" : "float");
      }
      tooltip(tr("params.list.tip"));
    } else if (p.kind == "int" || p.kind.rfind("enum:", 0) == 0) {
      const auto v = numbers_of(value);
      int64_t i = v.empty() ? 0 : (int64_t)std::llround(v[0]);
      const GameData::Enum* en = p.kind.size() > 5 ? data->enumeration(p.kind.substr(5)) : nullptr;
      if (en && en->bitfield) {
        // the bits by their names
        ImGui::TextUnformatted(label);
        for (const auto& [name, bit] : en->values) {
          bool on = (i >> bit) & 1;
          ImGui::SameLine();
          if (ImGui::Checkbox(name.c_str(), &on))
            commit(p.tag, on ? (i | (int64_t(1) << bit)) : (i & ~(int64_t(1) << bit)), "uint32");
        }
      } else if (en) {
        const char* current = "?";
        for (const auto& [name, val] : en->values)
          if (val == i) current = name.c_str();
        if (ImGui::BeginCombo(label, current)) {
          for (const auto& [name, val] : en->values)
            if (ImGui::Selectable(name.c_str(), val == i) && val != i) commit(p.tag, val, "uint32");
          ImGui::EndCombo();
        }
      } else {
        int k = (int)i;
        if (ImGui::InputInt(label, &k, 1, 10, ImGuiInputTextFlags_EnterReturnsTrue)) commit(p.tag, k, "int32");
      }
    } else if (p.kind == "symbol" || p.kind == "string") {
      std::string s = value.is_string() ? value.get<std::string>() : std::string();
      if (p.kind == "symbol" && !s.empty() && s[0] == '\'') s = s.substr(1);
      if (ImGui::InputText(label, &s, ImGuiInputTextFlags_EnterReturnsTrue))
        commit(p.tag, p.kind == "symbol" ? "'" + s : s, p.kind.c_str());
    } else if (p.kind == "actor") {
      // another actor of the level, by its name (the game's own links are its ids)
      std::string current = value.is_string() ? value.get<std::string>() : std::string();
      if (current.empty() && !numbers_of(value).empty()) {
        const double aid = numbers_of(value)[0];
        for (const auto& o : scene.nodes)
          if (o.kind == NodeKind::Actor && o.extras.value("aid", -1.0) == aid) current = o.name;
        if (current.empty()) current = tr("params.actor.missing");
      }
      if (ImGui::BeginCombo(label, current.empty() ? "-" : current.c_str(), ImGuiComboFlags_HeightLarge)) {
        for (const auto& o : scene.nodes)
          if (o.kind == NodeKind::Actor && o.id != n.id && !o.name.empty() &&
              ImGui::Selectable(o.name.c_str(), o.name == current))
            commit(p.tag, o.name, "actor-name");
        ImGui::EndCombo();
      }
    } else if (p.kind == "eco-info") {
      // what it holds: a pickup and how many
      const auto v = numbers_of(value);
      int64_t pick = v.size() > 0 ? (int64_t)v[0] : 0;
      int amount = v.size() > 1 ? (int)v[1] : 1;
      const GameData::Enum* en = data->enumeration("pickup-type");
      const char* current = "?";
      if (en)
        for (const auto& [name, val] : en->values)
          if (val == pick) current = name.c_str();
      ImGui::SetNextItemWidth(ImGui::CalcItemWidth() * 0.65f);
      if (ImGui::BeginCombo("##pickup", current, ImGuiComboFlags_HeightLarge)) {
        if (en)
          for (const auto& [name, val] : en->values)
            if (ImGui::Selectable(name.c_str(), val == pick)) commit(p.tag, json::array({val, amount}), "int32");
        ImGui::EndCombo();
      }
      ImGui::SameLine();
      ImGui::SetNextItemWidth(ImGui::CalcItemWidth() * 0.35f - ImGui::GetStyle().ItemSpacing.x);
      if (ImGui::InputInt("##amount", &amount, 0, 0, ImGuiInputTextFlags_EnterReturnsTrue))
        commit(p.tag, json::array({pick, std::max(0, amount)}), "int32");
      ImGui::SameLine();
      ImGui::TextUnformatted(label);
    } else if (p.kind == "vector") {
      auto v = numbers_of(value);
      v.resize(4, 0.0);
      float f[4] = {(float)v[0], (float)v[1], (float)v[2], (float)v[3]};
      ImGui::DragFloat4(label, f, 1.f);
      edit_begin(before);
      if (ImGui::IsItemEdited()) {
        lump[p.tag] = json::array({f[0], f[1], f[2], f[3]});
        types[p.tag] = "vector";
      }
      edit_end(tr("undo.param"));
    } else {
      // paths, scripts, groups of actors: shown, not edited here
      const char* what = p.kind == "path"     ? "params.path"
                         : p.kind == "script" ? "params.script"
                                              : "params.other";
      ImGui::TextDisabled("%s", label);
      ImGui::SameLine();
      ImGui::TextDisabled(tr(what), value.is_array() ? value.size() : (size_t)0);
      if (p.kind == "script" && value.is_string() && ImGui::IsItemHovered()) ImGui::SetTooltip("%s", value.get<std::string>().c_str());
    }
    // back to the game's value
    if (set && p.kind != "path" && p.kind != "script" && p.kind != "data" && p.kind != "actor-groups") {
      ImGui::SameLine();
      if (ImGui::SmallButton("x")) {
        lump.erase(p.tag);
        types.erase(p.tag);
        if (!committed) commit_node_edit(before, tr("undo.param"));
        committed = true;
      }
      tooltip(tr("params.reset.tip"));
    }
    ImGui::PopID();
  }
  ImGui::PopItemWidth();
}

}  // namespace ogle
