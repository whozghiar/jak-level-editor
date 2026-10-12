// The inspector's preview of an object's animation or effect: its model drawn moving where it
// stands, the document left as it is. Three kinds: an animation of its model (the game's, from its
// art group), broken (the pieces of its -explode model, or of its own cut up, thrown apart), and
// for the objects the game moves by its code, that movement imitated (an elevator going up and
// down, a platform sliding, a fan turning...).

#include <algorithm>
#include <cmath>
#include <map>
#include <random>

#include "app/app.h"
#include "core/i18n.h"
#include "core/log.h"
#include "imgui.h"
#include "io/gltf_io.h"
#include "io/jak2_import.h"

namespace ogle {

namespace {

void tooltip(const char* text) {
  if (ImGui::BeginItemTooltip()) {
    ImGui::PushTextWrapPos(ImGui::GetFontSize() * 28.f);
    ImGui::TextUnformatted(text);
    ImGui::PopTextWrapPos();
    ImGui::EndTooltip();
  }
}

// "fac-elevator-a-gate_up" of model "fac-elevator-a": "gate_up"
std::string short_name(const std::string& animation, const std::string& model) {
  if (animation.size() > model.size() + 1 && animation.compare(0, model.size() + 1, model + "-") == 0)
    return animation.substr(model.size() + 1);
  return animation;
}

// The preview's kinds besides an animation of the model (>= 0)
constexpr int kBroken = -1;    // its -explode model's pieces thrown apart
constexpr int kMotion = -2;    // the movement its code gives it, imitated
constexpr int kFracture = -3;  // its own model cut in pieces, thrown apart

// The movement the game's code gives an object, as far as its name and family tell
enum Motion { None = 0, Rise, Fall, Slide, Spin, Rock, Press, Lift, Sink, Bob, Flicker, Blow, Swing, Climb };
const char* const kMotionKeys[] = {"",     "rise", "fall", "slide",   "spin", "rock",  "press",
                                   "lift", "sink", "bob",  "flicker", "blow", "swing", "climb"};

Motion object_motion(const std::string& etype, const std::string& family) {
  auto has = [&](const char* s) { return etype.find(s) != std::string::npos; };
  // what their code does, read in the game's (temple-obs, foresta-obs, factoryc-obs, mine-platforms...)
  if (etype == "ladder") return Climb;
  if (has("elec") || has("eyes")) return Flicker;
  if (has("for-pillar") || has("for-tower")) return Rise;
  if (has("torch")) return Slide;
  if (has("ramp")) return Rock;
  if (has("gun")) return Spin;
  if (has("elevator") || has("elev") || has("rising") || has("pop")) return Rise;
  if (has("drop-plat") || has("collapsing") || has("falling")) return Fall;
  if (has("spin") || has("fan") || has("turbine") || has("saw") || has("rings") || has("rot")) return Spin;
  if (has("balance") || has("floating") || has("lava-plat")) return Rock;
  if (has("move") || has("rove") || has("slide") || has("arena-plat") || has("square-plat") || has("air-train") ||
      has("moving"))
    return Slide;
  if (has("switch") || has("button") || has("jump-pad")) return Press;
  if (has("security-wall") || has("eggwall") || has("krimson-wall") || has("field")) return Sink;
  if (has("door") || has("gate") || has("lid")) return Lift;
  if (has("artifact") || has("idol") || has("interceptor") || has("spawner")) return Bob;
  if (family == "platform") return Rise;
  if (family == "button") return Press;
  if (family == "door-like") return Lift;
  return None;
}

// What the picture of an object without a model does (a vent blowing, lightning, a swing pole)
Motion icon_motion(const std::string& etype, const std::string& family) {
  auto has = [&](const char* s) { return etype.find(s) != std::string::npos; };
  if (has("vent")) return Blow;
  if (has("elec-gate") || has("vingate") || has("laser") || has("turbo")) return Flicker;
  if (has("swingpole") || has("swing-pole")) return Swing;
  if (has("whirlpool") || has("spinning")) return Spin;
  if (has("conveyor")) return Slide;
  if (family == "hazard") return Flicker;
  return Bob;
}

// Whether the game breaks it (a crate, a breakable, a bridge that falls apart...)
bool breaks(const std::string& etype, const std::string& family) {
  auto has = [&](const char* s) { return etype.find(s) != std::string::npos; };
  return family == "crate" || family == "breakable" || has("break") || has("cactus") || has("fruit-stand") ||
         has("eco-tank") || has("urn") || has("market") || has("power-box");
}

// Where the motion puts the model at t, around the middle of its base `base`, model height `h`
Mat4 motion_at(Motion m, float t, const Vec3& base, float h) {
  const float two_pi = 2 * kPi;
  Vec3 off{0, 0, 0};
  Quat q;
  switch (m) {
    case Rise:
      off.y = 8.f * (0.5f - 0.5f * std::cos(two_pi * t / 6.f));
      break;
    case Fall: {
      const float c = std::fmod(t, 3.f);
      if (c < 0.6f) off.x = 0.06f * std::sin(c * 60.f);
      else off.y = -std::min(0.5f * 9.8f * (c - 0.6f) * (c - 0.6f), 20.f);
      break;
    }
    case Slide:
      off.x = 4.f * std::sin(two_pi * t / 5.f);
      break;
    case Spin:
      q = Quat::axis_angle({0, 1, 0}, two_pi * t / 3.f);
      break;
    case Rock:
      q = Quat::axis_angle({1, 0, 0}, radians(8.f) * std::sin(two_pi * t / 4.f));
      off.y = 0.2f * std::sin(two_pi * t / 2.f);
      break;
    case Press: {
      const float c = std::fmod(t, 2.f);
      off.y = c > 0.3f && c < 1.f ? -std::min(0.3f, h * 0.3f) : 0.f;
      break;
    }
    case Lift:
    case Sink: {
      // open over 1.5 s, held, closed again: a period of 6 s
      const float c = std::fmod(t, 6.f);
      const float k = c < 1.5f ? c / 1.5f : c < 3.f ? 1.f : c < 4.5f ? 1.f - (c - 3.f) / 1.5f : 0.f;
      const float e = k * k * (3 - 2 * k);
      off.y = (m == Lift ? 1.f : -1.f) * h * 0.95f * e;
      break;
    }
    case Bob:
      off.y = 0.4f * std::sin(two_pi * t / 2.f);
      q = Quat::axis_angle({0, 1, 0}, two_pi * t / 6.f);
      break;
    case Flicker: {
      // lightning: it shivers, flares and dims
      const float k = std::floor(t * 14.f);
      off = {0.06f * std::sin(k * 12.9898f), 0, 0.06f * std::sin(k * 78.233f)};
      return Mat4::translate(base + off) * Mat4::scale({1, 1.f + 0.12f * std::sin(k * 37.719f), 1});
    }
    case Blow: {
      // a gust: it rises a meter and a half, again and again
      const float c = std::fmod(t, 1.2f) / 1.2f;
      off.y = 1.5f * c * c;
      break;
    }
    case Swing:
      // a pole Jak swings around: it turns about its bar
      q = Quat::axis_angle({1, 0, 0}, two_pi * t / 1.5f);
      return Mat4::translate(base + Vec3{0, h * 0.75f, 0}) * Mat4::rotate(q) * Mat4::translate({0, -h * 0.75f, 0});
    case Climb:
      // someone going up the ladder and down again, in front of it (h: the ladder's height)
      off = {0, std::max(h - 1.9f, 0.f) * (0.5f - 0.5f * std::cos(two_pi * t / 6.f)), 0.7f};
      break;
    default:
      break;
  }
  return Mat4::translate(base + off) * Mat4::rotate(q);
}

// `src` as pieces that each move with a joint of a made skeleton: one piece (`cell` 0) or its
// triangles grouped in cells of `cell` meters (their vertices made apart)
void cut_in_pieces(const Mesh& src, float cell, Mesh* bind, Skeleton* s) {
  const AABB b = const_cast<Mesh&>(src).bounds();
  const Vec3 base = b.valid() ? Vec3{b.center().x, b.lo.y, b.center().z} : Vec3{0, 0, 0};
  std::map<std::tuple<int, int, int>, int> piece_of;
  std::vector<Vec3> sum;
  std::vector<float> count;
  for (const auto& p : src.prims) {
    Primitive q;
    q.material = p.material;
    auto& joints = q.extra_attrs["JOINTS_0"];
    auto& weights = q.extra_attrs["WEIGHTS_0"];
    for (size_t t = 0; t + 2 < p.idx.size(); t += 3) {
      int piece = 0;
      if (cell > 0) {
        const Vec3 c = (p.pos[p.idx[t]] + p.pos[p.idx[t + 1]] + p.pos[p.idx[t + 2]]) * (1.f / 3.f);
        const auto key = std::make_tuple((int)std::floor(c.x / cell), (int)std::floor(c.y / cell), (int)std::floor(c.z / cell));
        auto [it, fresh] = piece_of.emplace(key, (int)piece_of.size());
        piece = it->second;
      }
      if ((int)sum.size() <= piece) {
        sum.resize(piece + 1, Vec3{0, 0, 0});
        count.resize(piece + 1, 0);
      }
      for (int k = 0; k < 3; k++) {
        const uint32_t v = p.idx[t + k];
        q.idx.push_back((uint32_t)q.pos.size());
        q.pos.push_back(p.pos[v]);
        q.nrm.push_back(v < p.nrm.size() ? p.nrm[v] : Vec3{0, 1, 0});
        if (p.uv.size() == p.pos.size()) q.uv.push_back(p.uv[v]);
        if (p.col.size() == p.pos.size()) q.col.push_back(p.col[v]);
        joints.push_back({(float)piece, 0, 0, 0});
        weights.push_back({1, 0, 0, 0});
        sum[piece] = sum[piece] + p.pos[v];
        count[piece] += 1;
      }
    }
    bind->prims.push_back(std::move(q));
  }
  // a joint per piece, at its middle (one piece: at the middle of the model's base)
  for (size_t i = 0; i < sum.size(); i++) {
    const Vec3 at = cell > 0 && count[i] > 0 ? sum[i] * (1.f / count[i]) : base;
    s->parent.push_back(-1);
    s->rest.push_back(Mat4::translate(at));
    s->joints.push_back((int)i);
    s->inverse_bind.push_back(Mat4::translate(at * -1.f));
  }
}

}  // namespace

const Skeleton* App::model_skeleton(const std::string& path) {
  auto it = m_skeletons.find(path);
  if (it == m_skeletons.end()) {
    auto s = std::make_shared<Skeleton>();
    std::string err;
    if (!load_skeleton(path, s.get(), &err)) s.reset();
    it = m_skeletons.emplace(path, s).first;
  }
  return it->second.get();
}

void App::stop_preview() {
  if (!m_preview.node) return;
  if (Node* n = scene.find(m_preview.node); n && n->mesh == m_preview.moving) n->mesh = m_preview.original;
  m_preview = AnimPreview{};
}

bool App::start_preview(uint32_t model_node, const std::string& path, int animation, int motion, bool jak2) {
  stop_preview();
  Node* n = scene.find(model_node);
  if (!n || !n->mesh) return false;
  AnimPreview p;
  p.node = model_node;
  p.original = n->mesh;
  p.animation = animation;
  p.motion = motion;
  auto skeleton = std::make_shared<Skeleton>();
  if (animation == kMotion || animation == kFracture) {
    // its own model, as one piece or cut in pieces about a third of its size
    auto bind = std::make_shared<Mesh>();
    const AABB b = n->mesh->bounds();
    const Vec3 size = b.valid() ? b.hi - b.lo : Vec3{1, 1, 1};
    const float cell = animation == kFracture ? std::max({size.x, size.y, size.z, 0.3f}) / 3.f : 0.f;
    cut_in_pieces(*n->mesh, cell, bind.get(), skeleton.get());
    p.bind = bind;
    p.moving = std::make_shared<Mesh>(*bind);
    p.base = b.valid() ? Vec3{b.center().x, b.lo.y, b.center().z} : Vec3{0, 0, 0};
    p.height = size.y;
  } else {
    // the file again with each vertex's joints: the same vertices as the model in the document
    LoadedMesh lm;
    std::string err;
    if (!load_model_asset(path, &lm, &err, true) || !load_skeleton(path, skeleton.get(), &err)) {
      LOG_WARN("%s: %s", path.c_str(), err.c_str());
      return false;
    }
    p.bind = lm.mesh;
    // broken: the model of its pieces in place of its own, its materials the document's (Jak II's
    // textures named apart)
    p.moving = animation >= 0 ? std::make_shared<Mesh>(*n->mesh)
               : jak2         ? import_jak2_mesh(scene, *lm.mesh, lm.textures, lm.materials, nullptr)
                              : merge_mesh(scene, *lm.mesh, lm.textures, lm.materials);
  }
  p.skeleton = skeleton;
  if (!skin_mesh(*p.bind, joint_matrices(*skeleton, skeleton_pose(*skeleton, -1, 0)), *p.moving)) {
    LOG_WARN(tr("log.preview_mismatch"), path.c_str());
    return false;
  }
  if (animation == kBroken || animation == kFracture) {
    // each piece thrown out of the middle and up, turning, then falling
    p.rest = skeleton_pose(*skeleton, -1, 0);
    const AABB b = p.bind->bounds();
    const Vec3 middle = b.valid() ? b.center() : Vec3{0, 0, 0};
    // a crate's pieces fly less far than a wall's
    const float k = b.valid() ? std::clamp(length(b.hi - b.lo) / 8.f, 0.5f, 1.5f) : 1.f;
    std::mt19937 rng(7);
    std::uniform_real_distribution<float> u(-1.f, 1.f);
    for (const Mat4& w : p.rest) {
      Vec3 out = w.col3(3) - middle;
      out.y = 0;
      out = length(out) > 0.01f ? normalize(out) : Vec3{u(rng), 0, u(rng)};
      p.velocity.push_back((out * (2.5f + 1.5f * u(rng)) + Vec3{0, 5.f + 1.5f * u(rng), 0}) * k);
      p.spin.push_back(Vec3{u(rng), u(rng), u(rng)} * 4.f);
    }
  }
  p.revision = history.revision();
  m_preview = std::move(p);
  n->mesh = m_preview.moving;
  return true;
}

bool App::start_icon_preview(uint32_t actor, int motion) {
  stop_preview();
  const Node* n = scene.find(actor);
  if (!n) return false;
  const std::string etype = n->extras.value("etype", std::string());
  std::string family;
  for (const auto& o : catalog)
    if (o.etype == etype) family = o.family;
  AnimPreview p;
  p.node = actor;
  p.icon = true;
  p.motion = motion;
  // climbing: someone on its model (a ladder), as high as it
  p.moving = object_icon(motion == Climb ? "climber" : etype, family);
  AABB b = p.moving->bounds();
  if (motion == Climb)
    for (auto c : scene.children(actor))
      if (Node* cn = scene.find(c); cn && cn->mesh) b = cn->mesh->bounds();
  p.height = b.valid() ? b.hi.y - b.lo.y : 1.f;
  p.revision = history.revision();
  m_preview = std::move(p);
  return true;
}

void App::draw_preview_overlay() {
  if (!m_preview.node || !m_preview.icon || !m_preview.moving) return;
  const Node* n = scene.find(m_preview.node);
  if (!n) return;
  static const std::vector<Material> kNoMaterials;
  static const std::vector<Texture> kNoTextures;
  if (m_preview.motion == Flicker) {
    // lightning: its zigzags made anew 14 times a second
    const int seed = 1 + (int)(m_preview.time * 14.f);
    if (seed != m_preview.seed) {
      std::string family;
      const std::string etype = n->extras.value("etype", std::string());
      for (const auto& o : catalog)
        if (o.etype == etype) family = o.family;
      m_preview.moving = object_icon(etype, family, seed);
      m_preview.seed = seed;
    }
  }
  const Mat4 at = scene.world(*n) * motion_at((Motion)m_preview.motion, m_preview.time, {0, 0, 0}, m_preview.height);
  renderer.draw_mesh(*m_preview.moving, at, kNoMaterials, kNoTextures, rs);
}

void App::update_preview(float dt) {
  if (!m_preview.node) return;
  Node* n = scene.find(m_preview.node);
  if (m_preview.icon) {
    // a picture over the view: only its time goes on
    if (!n || history.revision() != m_preview.revision) m_preview = AnimPreview{};
    else m_preview.time += dt;
    return;
  }
  if (!n || n->mesh != m_preview.moving) {
    // another document, or the node changed: nothing to give back
    m_preview = AnimPreview{};
    return;
  }
  if (history.revision() != m_preview.revision) {
    stop_preview();
    return;
  }
  AnimPreview& p = m_preview;
  const Skeleton& s = *p.skeleton;
  p.time += dt;
  std::vector<Mat4> worlds;
  if (p.animation >= 0) {
    const float d = s.animations[p.animation].duration;
    float t = p.time;
    if (d > 0) t = m_preview_loop ? std::fmod(p.time, d + 0.5f) : std::min(p.time, d);
    worlds = skeleton_pose(s, p.animation, std::min(t, d));
  } else if (p.animation == kMotion) {
    worlds = {motion_at((Motion)p.motion, m_preview_loop ? p.time : std::min(p.time, 6.f), p.base, p.height)};
  } else {
    // 2.5 s of flight, then again (or the pieces left lying)
    const float t = m_preview_loop ? std::fmod(p.time, 2.5f) : std::min(p.time, 2.5f);
    worlds = p.rest;
    for (size_t i = 0; i < worlds.size() && i < p.velocity.size(); i++) {
      const Vec3 at = p.rest[i].col3(3);
      const Vec3 moved = p.velocity[i] * t + Vec3{0, -9.8f * 0.5f * t * t, 0};
      const float turn = length(p.spin[i]) * t;
      const Quat q = turn > 0 ? Quat::axis_angle(p.spin[i], turn) : Quat{};
      worlds[i] = Mat4::translate(at + moved) * Mat4::rotate(q) * Mat4::translate(at * -1.f) * p.rest[i];
    }
  }
  skin_mesh(*p.bind, joint_matrices(s, worlds), *p.moving);
}

json App::PreviewOptions::to_json() const {
  json anims = json::array();
  for (int i : animations) anims.push_back(skeleton->animations[i].name);
  json j = {{"animations", anims}};
  if (motion) j["movement"] = kMotionKeys[motion];
  if (broken || fracture) j["break"] = broken ? "its pieces" : "its model cut up";
  if (icon) j["picture"] = true;
  return j;
}

App::PreviewOptions App::preview_options(const Node& n) {
  PreviewOptions o;
  std::string etype, family;
  const AssetLibrary* lib = &library;
  if (n.kind == NodeKind::Actor) {
    // an actor: its model, its type's family
    etype = n.extras.value("etype", std::string());
    for (const auto& c : catalog)
      if (c.etype == etype) {
        family = c.family;
        break;
      }
    for (auto c : scene.children(n.id))
      if (const Node* cn = scene.find(c); cn && cn->extras.contains("og_actor_visual")) o.model = cn;
    if (!o.model || !o.model->mesh) {
      // nothing drawn of its own (a vent, a swing pole, an electric gate): its picture
      o.icon = true;
      o.motion = icon_motion(etype, family);
      return o;
    }
    o.name = o.model->extras.value("og_actor_visual", std::string());
  } else if (n.kind == NodeKind::Render && n.mesh && !scene.actor_of(n)) {
    // a model placed from the catalog
    o.model = &n;
    o.name = n.extras.value("og_model", std::string());
    auto imp = n.extras.find("og_import");
    if (imp != n.extras.end() && imp->is_object() && imp->value("game", std::string()) == "jak2") {
      o.name = imp->value("part", std::string());
      lib = &library_jak2;
      o.jak2 = true;
    }
    if (o.name.empty()) return o;
  } else {
    return o;
  }
  // the copy of its model with the most animations (the same model is saved with every level that
  // loads it, not always with them)
  o.asset = lib->model_named(o.name, "");
  o.skeleton = o.asset ? model_skeleton(o.asset->path) : nullptr;
  size_t best = 0;
  for (const ModelAsset* copy : lib->models_named(o.name)) {
    const Skeleton* s = model_skeleton(copy->path);
    size_t moving = 0;
    if (s)
      for (const auto& a : s->animations) moving += a.duration > 0;
    if (moving > best) {
      best = moving;
      o.asset = copy;
      o.skeleton = s;
    }
  }
  o.broken = o.asset ? lib->model_named(o.name + "-explode", o.asset->group) : nullptr;
  if (o.skeleton)
    for (int i = 0; i < (int)o.skeleton->animations.size(); i++)
      if (o.skeleton->animations[i].duration > 0) o.animations.push_back(i);
  if (!etype.empty()) {
    o.fracture = !o.broken && breaks(etype, family);
    if (o.animations.empty()) o.motion = object_motion(etype, family);
  }
  return o;
}

void App::ui_preview_inspector(const Node& n) {
  const PreviewOptions o = preview_options(n);
  // a model of the catalog that does nothing: no section (a piece of decor neither)
  if (n.kind != NodeKind::Actor && !o.any()) return;
  ImGui::SeparatorText(tr("preview.title"));
  guide_tip("preview");
  if (!o.any()) {
    ImGui::TextWrapped("%s", tr("preview.none"));
    return;
  }
  const uint32_t moving_node = o.icon ? n.id : o.model->id;
  const bool playing = m_preview.node == moving_node || m_preview.node == n.id;
  bool first = true;
  auto next = [&]() {
    if (!first) ImGui::SameLine();
    first = false;
  };
  if (o.icon) {
    ImGui::TextWrapped("%s", tr("preview.no_model"));
    next();
    if (ImGui::Button(tr_id("preview.effect").c_str())) start_icon_preview(n.id, o.motion);
    tooltip(tr((std::string("preview.motion.") + kMotionKeys[o.motion]).c_str()));
  }
  if (!o.animations.empty() && o.asset) {
    m_preview_choice = std::clamp(m_preview_choice, 0, (int)o.animations.size() - 1);
    const auto& chosen = o.skeleton->animations[o.animations[m_preview_choice]];
    ImGui::SetNextItemWidth(-1);
    if (ImGui::BeginCombo("##preview-anim",
                          strf(tr("preview.animation"), short_name(chosen.name, o.name).c_str(), chosen.duration).c_str())) {
      for (int i = 0; i < (int)o.animations.size(); i++) {
        const auto& a = o.skeleton->animations[o.animations[i]];
        if (ImGui::Selectable(strf(tr("preview.animation"), short_name(a.name, o.name).c_str(), a.duration).c_str(),
                              i == m_preview_choice)) {
          m_preview_choice = i;
          if (playing) start_preview(moving_node, o.asset->path, o.animations[i], 0, o.jak2);
        }
      }
      ImGui::EndCombo();
    }
    next();
    if (ImGui::Button(tr_id("preview.play").c_str()))
      start_preview(moving_node, o.asset->path, o.animations[m_preview_choice], 0, o.jak2);
    tooltip(tr("preview.play.tip"));
  }
  if (o.motion && !o.icon) {
    next();
    if (ImGui::Button(tr_id("preview.motion").c_str())) {
      if (o.motion == Climb) start_icon_preview(n.id, o.motion);  // someone on it
      else start_preview(moving_node, "", kMotion, o.motion);
    }
    tooltip(tr((std::string("preview.motion.") + kMotionKeys[o.motion]).c_str()));
  }
  if (o.broken || o.fracture) {
    next();
    if (ImGui::Button(tr_id("preview.break").c_str()))
      start_preview(moving_node, o.broken ? o.broken->path : "", o.broken ? kBroken : kFracture, 0, o.jak2);
    tooltip(tr("preview.break.tip"));
  }
  next();
  ImGui::BeginDisabled(!playing);
  if (ImGui::Button(tr_id("preview.stop").c_str())) stop_preview();
  ImGui::EndDisabled();
  ImGui::SameLine();
  ImGui::Checkbox(tr_id("preview.loop").c_str(), &m_preview_loop);
}

}  // namespace ogle
