#include "scene/story.h"

#include <unordered_map>

namespace ogle {

namespace {

// What the story decides in a document, collected once per evaluation.
struct Item {
  enum Kind : uint8_t { Actor, Prototype, Layer, LayerDecor };
  size_t index = 0;  // in Scene::nodes
  Kind kind = Actor;
  int level = 0;     // in Items::levels
  uint32_t kill_mask = 0;
  std::vector<const StoryCond*> rules;  // all must hold
};

struct Items {
  std::vector<std::string> levels;  // [0]: the level opened
  std::vector<uint8_t> is_layer;
  std::vector<int> host;            // per level: the level the player is in (levels[0], or an added level)
  std::vector<Item> items;
};

bool ends_with(const std::string& s, const std::string& suffix) {
  return s.size() >= suffix.size() && s.compare(s.size() - suffix.size(), suffix.size(), suffix) == 0;
}

std::string extra_string(const Node& n, const char* key) {
  auto it = n.extras.find(key);
  return it != n.extras.end() && it->is_string() ? it->get<std::string>() : std::string();
}

uint32_t kill_mask_of(const Node& n) {
  auto lump = n.extras.find("lump");
  if (lump == n.extras.end() || !lump->is_object()) return 0;
  auto km = lump->find("kill-mask");
  if (km == lump->end() || !km->is_number()) return 0;
  return (uint32_t)(int64_t)km->get<double>();
}

Items collect(const Scene& scene, const GameData& data) {
  Items out;
  out.levels.push_back(scene.settings.level);
  out.is_layer.push_back(0);
  out.host.push_back(0);
  const size_t n = scene.nodes.size();
  std::unordered_map<uint32_t, size_t> index;
  index.reserve(n);
  for (size_t i = 0; i < n; i++) index[scene.nodes[i].id] = i;
  // a level per group (the same level twice counts once); host: an added level is its own, the
  // companions and layers under its group are in it, the others in the level opened
  std::unordered_map<std::string, int> level_ids;
  auto level_id = [&](const std::string& name, bool layer, int host) {
    auto it = level_ids.find(name);
    if (it != level_ids.end()) return it->second;
    const int id = (int)out.levels.size();
    out.levels.push_back(name);
    out.is_layer.push_back(layer);
    out.host.push_back(host < 0 ? id : host);
    level_ids[name] = id;
    return id;
  };
  auto host_of = [&](const Node& group) {
    for (auto p = group.parent ? index.find(group.parent) : index.end(); p != index.end();) {
      const Node& up = scene.nodes[p->second];
      const std::string added = extra_string(up, "og_level");
      if (!added.empty()) return level_id(added, false, -1);
      p = up.parent ? index.find(up.parent) : index.end();
    }
    return 0;
  };
  // the level of each node: a group of an extra level, else the level opened
  std::vector<int> level_of(n, -1);
  std::vector<size_t> chain;
  for (size_t i = 0; i < n; i++) {
    chain.clear();
    size_t cur = i;
    int found = -1;
    while (true) {
      if (level_of[cur] >= 0) {
        found = level_of[cur];
        break;
      }
      chain.push_back(cur);
      const Node& node = scene.nodes[cur];
      const std::string layer = extra_string(node, "og_layer");
      const std::string companion = extra_string(node, "og_companion");
      const std::string added = extra_string(node, "og_level");
      if (!added.empty()) {
        found = level_id(added, false, -1);
        break;
      }
      if (!layer.empty() || !companion.empty()) {
        found = layer.empty() ? level_id(companion, false, host_of(node)) : level_id(layer, true, host_of(node));
        break;
      }
      auto p = node.parent ? index.find(node.parent) : index.end();
      if (p == index.end() || chain.size() > 1000) {
        found = 0;
        break;
      }
      cur = p->second;
    }
    for (size_t c : chain) level_of[c] = found;
  }

  for (size_t i = 0; i < n; i++) {
    const Node& node = scene.nodes[i];
    const int level = level_of[i];
    const bool in_layer = out.is_layer[level] != 0;
    if (!extra_string(node, "og_layer").empty()) {
      out.items.push_back({i, Item::Layer, level, 0, {}});
      continue;
    }
    auto pit = node.parent ? index.find(node.parent) : index.end();
    const Node* parent = pit != index.end() ? &scene.nodes[pit->second] : nullptr;
    if (in_layer && parent && !extra_string(*parent, "og_layer").empty() && !ends_with(node.name, "-actors")) {
      out.items.push_back({i, Item::LayerDecor, level, 0, {}});
      continue;
    }
    if (node.kind == NodeKind::Actor) {
      Item it{i, Item::Actor, level, kill_mask_of(node), {}};
      if (const StoryCond* rule = data.type_rule(extra_string(node, "etype"))) it.rules.push_back(rule);
      if (it.kill_mask || !it.rules.empty() || in_layer) out.items.push_back(std::move(it));
      continue;
    }
    // a decor prototype: a node under the level's tie or shrub trees
    if (parent && instance_tree(parent->name)) {
      const auto* rules = data.prototype_rules(out.levels[level]);
      if (!rules) continue;
      std::string name = node.name;
      if (ends_with(name, ".mb")) name.resize(name.size() - 3);
      Item it{i, Item::Prototype, level, 0, {}};
      for (const auto& r : *rules)
        for (const auto& proto : r.names)
          if (proto == name) it.rules.push_back(&r.when);
      if (!it.rules.empty()) out.items.push_back(std::move(it));
    }
  }
  return out;
}

// hidden[k]: whether items.items[k] is hidden, each host (items.host) at its moment (moments[host]);
// only the items of host `only` when it is not -1
void evaluate(const Items& items, const GameData& data, const std::vector<const StoryMoment*>& moments,
              std::vector<uint8_t>& hidden, int only = -1) {
  // what each host shows at its moment (the layers it borrows), and each level's task mask there
  std::unordered_map<int, std::map<std::string, bool>> borrowed;
  std::vector<uint32_t> masks(items.levels.size());
  for (size_t l = 0; l < items.levels.size(); l++) {
    const int h = items.host[l];
    if (only >= 0 && h != only) continue;
    const StoryMoment& m = *moments[h];
    if (!borrowed.count(h)) borrowed[h] = data.borrowed(items.levels[h], m);
    masks[l] = data.task_mask(items.levels[l], items.levels[h], m);
  }
  hidden.assign(items.items.size(), 0);
  for (size_t k = 0; k < items.items.size(); k++) {
    const Item& it = items.items[k];
    if (only >= 0 && items.host[it.level] != only) continue;
    const StoryMoment& m = *moments[items.host[it.level]];
    const std::string& level = items.levels[it.level];
    const auto& shown = borrowed.at(items.host[it.level]);
    auto b = items.is_layer[it.level] ? shown.find(level) : shown.end();
    const bool special = b != shown.end() && b->second;
    bool hide = false;
    switch (it.kind) {
      case Item::Layer: hide = b == shown.end(); break;
      case Item::LayerDecor: hide = special; break;  // a 'special level draws no decor
      case Item::Actor: hide = !data.actor_alive(it.kill_mask, masks[it.level], special); break;
      case Item::Prototype: break;
    }
    for (const StoryCond* r : it.rules) hide |= !data.eval(*r, m);
    hidden[k] = hide;
  }
}

}  // namespace

void apply_story(Scene& scene, const GameData* data, const StoryView& view) {
  for (auto& n : scene.nodes) n.story_hidden = false;
  scene.begin_frame();
  if (!data || !data->has_story() || !view.enabled || scene.nodes.empty()) return;
  const Items items = collect(scene, *data);
  // the moment of each host: the level opened at view.step, a level added at its own
  std::vector<StoryMoment> by_host(items.levels.size());
  std::vector<const StoryMoment*> moments(items.levels.size(), nullptr);
  for (size_t l = 0; l < items.levels.size(); l++) {
    const int h = items.host[l];
    if (!moments[h]) {
      by_host[h] = data->moment(h == 0 ? view.step : view.step_of(items.levels[h]));
      moments[h] = &by_host[h];
    }
  }
  for (size_t l = 0; l < items.levels.size(); l++) moments[l] = moments[items.host[l]];
  std::vector<uint8_t> hidden;
  evaluate(items, *data, moments, hidden);
  for (size_t k = 0; k < items.items.size(); k++) scene.nodes[items.items[k].index].story_hidden = hidden[k] != 0;
}

std::vector<int> story_changes(const Scene& scene, const GameData& data, const std::string& level) {
  std::vector<int> out;
  if (!data.has_story() || scene.nodes.empty()) return out;
  const Items items = collect(scene, data);
  if (items.items.empty()) return out;
  int host = level.empty() ? 0 : -1;
  for (size_t l = 1; l < items.levels.size() && host < 0; l++)
    if (items.levels[l] == level && items.host[l] == (int)l) host = (int)l;
  if (host < 0) return out;
  std::vector<uint8_t> prev, cur;
  for (int s = 1; s <= data.step_count(); s++) {
    const StoryMoment m = data.moment(s);
    const std::vector<const StoryMoment*> moments(items.levels.size(), &m);
    evaluate(items, data, moments, cur, host);
    if (s > 1 && cur != prev) out.push_back(s);
    prev.swap(cur);
  }
  return out;
}

}  // namespace ogle
