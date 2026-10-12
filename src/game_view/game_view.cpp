#include "game_view/game_view.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstring>
#include <map>
#include <tuple>
#include <unordered_map>

#include "core/log.h"
#include "io/decor_behaviors.h"
#include "game/graphics/opengl_renderer/BucketRenderer.h"
#include "game/graphics/opengl_renderer/background/TFragment.h"
#include "game/graphics/opengl_renderer/background/background_common.h"
#include "game_view/sky.h"

// the matrix the game's background shaders take (background_common.cpp)
std::array<math::Vector4f, 4> make_new_cam_mat(const math::Vector4f cam_T_w[4],
                                               const math::Vector4f persp[4],
                                               float fog_constant,
                                               float hvdf_z);

namespace ogle {

namespace {

constexpr float kUnits = 4096.f;     // game units per meter
constexpr float kCell = 128.f;       // meters: the side of a chunk's cell
constexpr size_t kMaxColors = 8192;  // per tree (TFragment's TIME_OF_DAY_COLOR_COUNT)

using Clock = std::chrono::steady_clock;

uint64_t fnv(uint64_t h, const void* data, size_t n) {
  const uint8_t* p = (const uint8_t*)data;
  for (size_t i = 0; i < n; i++) h = (h ^ p[i]) * 1099511628211ull;
  return h;
}

// The game's camera for the editor's (update-math-camera, update-camera, and the packet of
// background.gc the background renderers take), and the matrix its shaders make of it (world
// meters -> clip space: tfrag3.vert, make_new_cam_mat): the depth is the game's, 1 near, 0 far.
GoalBackgroundCameraData game_camera(const Camera& cam, float aspect, const GameMood& mood, Mat4* clip) {
  GoalBackgroundCameraData c;
  std::memset(&c, 0, sizeof(c));
  const float d = 1024.f, f = 40960000.f;  // near (0.25 m), far (10 km)
  // the PC's view: the 4:3 cropping, the wider screens see more on the sides
  const float yr = std::tan(radians(cam.fov_deg) * 0.5f);
  const float xr = yr * aspect;
  // the game's fov (at 4:3, in its angle units: 65536 a turn) corrects the fog and the depth
  const float fov = 2.f * std::atan(yr / 0.75f) / (2.f * kPi) * 65536.f;
  const float fcf = 0.00008583069f * std::min(11650.845f, fov);
  const float fog_start = mood.fog_start * fcf, fog_end = std::max(mood.fog_end * fcf, fog_start + 1.f);
  const float pfog0 = d * (mood.fog_min - mood.fog_max) / (fog_end - fog_start);
  const float pfog1 = (fog_end * mood.fog_max - fog_start * mood.fog_min) / (fog_end - fog_start);
  const float k = -0.5f * (16760631.f - 100.f) / (d * (f - d));
  math::Vector4f P[4];
  for (auto& r : P) r.set_zero();
  P[0][0] = fcf * -(256.f / (xr * d));
  P[1][1] = fcf * -(208.f / (yr * d));
  P[2][2] = fcf * (f + d) * k;
  P[2][3] = (fcf / d) * pfog0;
  P[3][2] = -2.f * k * f * d * fcf;
  // its frame (forward-down->inv-matrix): rvec to the left of the screen, uvec up, fvec forward
  const Vec3 F = cam.forward();
  const Vec3 R = normalize(cross(F, Vec3{0, -1, 0}));
  const Vec3 U = normalize(cross(F, R));
  const Vec3 T = cam.pos * kUnits;
  for (int r = 0; r < 3; r++) c.rot[r] = math::Vector4f(R[r], U[r], F[r], 0.f);
  c.rot[3] = math::Vector4f(-dot(T, R), -dot(T, U), -dot(T, F), 1.f);
  for (int r = 0; r < 4; r++) {
    c.perspective[r] = P[r];
    for (int col = 0; col < 4; col++) {
      float s = 0;
      for (int i = 0; i < 4; i++) s += c.rot[r][i] * P[i][col];
      c.camera[r][col] = s;
    }
  }
  c.hvdf_off = math::Vector4f(2048.f, 2048.f, 0.5f * (16760631.f + 100.f), pfog1);
  c.fog = math::Vector4f(pfog0, mood.fog_min, mood.fog_max, 0.f);
  c.trans = math::Vector4f(T.x, T.y, T.z, 1.f);
  for (int q = 0; q < 4; q++)
    for (int w = 0; w < 4; w++) c.itimes[q][w] = mood.itimes[q][w];

  if (clip) {
    // tfrag3.vert: clip = -(n3 without w) - sum(n_i * v_i), v = position - trans (game units),
    // then y *= SCISSOR_ADJUST * HEIGHT_SCALE (Jak 3: 512 / 416 * 0.5)
    const auto n = make_new_cam_mat(c.rot, c.perspective, c.fog.x(), c.hvdf_off.z());
    const float ky = 512.f / 416.f * 0.5f;
    Mat4 M(0);
    for (int row = 0; row < 4; row++) {
      const float s = row == 1 ? ky : 1.f;
      float t = 0;
      for (int i = 0; i < 3; i++) {
        M.at(row, i) = -kUnits * n[i][row] * s;
        t += n[i][row] * T[i];
      }
      M.at(row, 3) = ((row == 3 ? 0.f : -n[3][row]) + t) * s;
    }
    *clip = M;
  }
  return c;
}

// A material as the level builder draws it (gltf_util.cpp: make_default_draw_mode,
// setup_alpha_from_material, setup_draw_mode_from_sampler) from what the export writes of it
// (gltf_io.cpp: a game material's alpha reference doubled; blended surfaces in the translucent
// tree, with their PS2 blend, og_blend)
DrawMode draw_mode(const Material* m, bool* translucent) {
  DrawMode mode;
  mode.set_depth_write_enable(true);
  mode.set_depth_test(GsTest::ZTest::GEQUAL);
  mode.set_alpha_blend(DrawMode::AlphaBlend::DISABLED);
  mode.set_aref(0);
  mode.set_alpha_fail(GsTest::AlphaFail::KEEP);
  mode.set_clamp_s_enable(false);
  mode.set_clamp_t_enable(false);
  mode.disable_filt();
  mode.enable_tcc();
  mode.disable_at();
  mode.enable_zt();
  mode.disable_ab();
  mode.disable_decal();
  mode.enable_fog();
  *translucent = false;
  if (!m) return mode;
  if (m->texture >= 0) mode.enable_filt();
  const float cutoff = m->base_color.w > 1.5f ? std::min(1.f, m->cutoff * 2.f) : m->cutoff;
  auto alpha_test = [&] {
    mode.enable_at();
    mode.set_alpha_test(DrawMode::AlphaTest::GEQUAL);
    mode.set_alpha_fail(GsTest::AlphaFail::KEEP);
    mode.set_aref((u8)std::clamp(cutoff * 127.f, 0.f, 255.f));
  };
  if (m->alpha == AlphaMode::Mask) {
    alpha_test();
  } else if (m->alpha == AlphaMode::Blend && !translucent_in_game(*m)) {
    alpha_test();  // a wall the game blends and writes the depth of: the export's (masked)
  } else if (m->alpha == AlphaMode::Blend) {
    alpha_test();
    mode.enable_ab();
    mode.set_depth_write_enable(false);
    switch (m->blend) {
      case BlendMode::Add: mode.set_alpha_blend(DrawMode::AlphaBlend::SRC_0_FIX_DST); break;
      case BlendMode::AddAlpha: mode.set_alpha_blend(DrawMode::AlphaBlend::SRC_0_SRC_DST); break;
      case BlendMode::Subtract: mode.set_alpha_blend(DrawMode::AlphaBlend::ZERO_SRC_SRC_DST); break;
      case BlendMode::Half: mode.set_alpha_blend(DrawMode::AlphaBlend::SRC_DST_FIX_DST); break;
      default: mode.set_alpha_blend(DrawMode::AlphaBlend::SRC_DST_SRC_DST); break;
    }
    *translucent = true;
  }
  return mode;
}

// a palette of 8 RGBA colors per entry as the renderer takes it (the colors 4 by 4, each of the
// 8 palettes of the 4 in turn)
tfrag3::PackedTimeOfDay pack_palette(const uint8_t* rgba, size_t colors) {
  tfrag3::PackedTimeOfDay out;
  out.color_count = (u32)((colors + 3) & ~(size_t)3);
  out.data.resize((size_t)out.color_count * 32);
  for (size_t c = 0; c < colors; c++)
    for (int k = 0; k < 8; k++)
      for (int ch = 0; ch < 4; ch++) out.read((int)c, k, ch) = rgba[c * 32 + k * 4 + ch];
  return out;
}

}  // namespace

struct GameView::Impl {
  std::shared_ptr<Loader> loader = std::make_shared<Loader>();
  std::unique_ptr<SharedRenderState> state;
  std::vector<GLuint> anim_slots;  // none: the document's textures are all its own
  bool ok = false;

  using Key = std::tuple<const TodPalette*, int, int>;  // the palette, the cell (x, z)
  struct Chunk {
    std::string name;
    uint64_t signature = 0;
    LevelData data;
    GLuint vbo = 0;
    std::unique_ptr<TFragment> tfrag;
    Vec3 center;
    size_t triangles = 0;
  };
  std::map<Key, Chunk> chunks;
  int next_chunk = 0;
  uint64_t next_load_id = 1;

  // the document's textures, the game's way (a texture of the editor's own: its alpha halved),
  // and a white one last (the materials without a texture)
  std::vector<GLuint> tex;
  std::vector<uint64_t> tex_version;
  std::vector<GLuint> tex_ids;  // what the chunks bind: tex, then the white one
  GLuint white = 0;

  uint64_t seen_cache = ~0ull;
  uint64_t seen_materials = 0;
  const Scene* seen_scene = nullptr;
  GameMood mood;
  GoalBackgroundCameraData cam{};
  Sky sky;

  void destroy(Chunk& c) {
    loader->levels.erase(c.name);
    c.tfrag.reset();
    if (c.vbo) glDeleteBuffers(1, &c.vbo);
    c.vbo = 0;
  }

  void clear() {
    for (auto& [k, c] : chunks) destroy(c);
    chunks.clear();
    if (!tex.empty()) glDeleteTextures((GLsizei)tex.size(), tex.data());
    tex.clear();
    tex_version.clear();
    tex_ids.clear();
    seen_cache = ~0ull;
    seen_materials = 0;
  }

  void upload_textures(const Scene& scene) {
    // which textures are the game's (their alpha the PS2's: 128 is opaque): a game material's,
    // drawn twice as bright (base_color 2, as the extractor writes them)
    std::vector<uint8_t> own(scene.textures.size(), 0);
    for (const auto& mat : scene.materials)
      if (mat.texture >= 0 && mat.texture < (int)own.size() && mat.base_color.w <= 1.5f) own[mat.texture] = 1;
    if (tex.size() < scene.textures.size()) {
      const size_t old = tex.size();
      tex.resize(scene.textures.size(), 0);
      tex_version.resize(scene.textures.size(), 0);
      glGenTextures((GLsizei)(tex.size() - old), tex.data() + old);
    }
    for (size_t i = 0; i < scene.textures.size(); i++) {
      const Texture& t = scene.textures[i];
      if (tex_version[i] == t.version || t.width <= 0 || t.height <= 0) continue;
      tex_version[i] = t.version;
      std::vector<uint8_t> rgba = t.rgba;
      if (own[i])
        for (size_t p = 3; p < rgba.size(); p += 4) rgba[p] = (uint8_t)((rgba[p] + 1) / 2);
      glBindTexture(GL_TEXTURE_2D, tex[i]);
      glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, t.width, t.height, 0, GL_RGBA, GL_UNSIGNED_BYTE, rgba.data());
      glGenerateMipmap(GL_TEXTURE_2D);
    }
    if (!white) {
      glGenTextures(1, &white);
      glBindTexture(GL_TEXTURE_2D, white);
      const uint8_t px[4] = {255, 255, 255, 128};
      glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, 1, 1, 0, GL_RGBA, GL_UNSIGNED_BYTE, px);
      glGenerateMipmap(GL_TEXTURE_2D);
    }
    tex_ids = tex;
    tex_ids.push_back(white);
  }

  // the chunk's trees, as the level builder makes them (Tfrag.cpp, gltf_mesh_extract.cpp): its
  // nodes' triangles in the game's space, one draw per material, a palette of 8 colors per vertex
  // color; the opaque and masked surfaces in a normal tree, the blended ones in a translucent one
  void build(Chunk& c, const Key& key, const std::vector<uint32_t>& nodes, const Scene& scene,
             const std::array<float, 8>& ratios) {
    const TodPalette* palette = std::get<0>(key);
    // a level's palette larger than a tree of the renderer takes (the decor instances of a whole
    // desert): the colors the chunk uses, as the new geometry's
    const bool compact = palette && palette->size() > kMaxColors;
    std::vector<tfrag3::PreloadedVertex> verts;
    std::map<int, std::vector<u32>> by_material[2];  // [translucent]
    std::vector<uint8_t> synth;                     // the palette of the new geometry
    std::unordered_map<std::string, u16> synth_index;
    int shift = 0;  // the bits of the colors dropped when they are too many for a palette
    const SceneCache& cache = scene.cache();
    AABB box;
    auto make = [&]() {
      verts.clear();
      for (auto& m : by_material) m.clear();
      synth.clear();
      synth_index.clear();
      for (uint32_t i : nodes) {
        const Node& n = scene.nodes[i];
        const Mat4& w = cache.worlds[i];
        const bool lit = palette && !compact && n.tod && n.tod->palette.get() == palette && n.tod->fits(*n.mesh);
        const bool own = !lit && n.tod && n.tod->fits(*n.mesh) && (n.tod->palette->local || compact);
        for (size_t pi = 0; pi < n.mesh->prims.size(); pi++) {
          const Primitive& p = n.mesh->prims[pi];
          if (p.idx.empty()) continue;
          const Material* mat =
              p.material >= 0 && p.material < (int)scene.materials.size() ? &scene.materials[p.material] : nullptr;
          bool trans = false;
          draw_mode(mat, &trans);
          const u32 base = (u32)verts.size();
          for (size_t v = 0; v < p.pos.size(); v++) {
            tfrag3::PreloadedVertex pv;
            const Vec3 q = w.point(p.pos[v]);
            box.add(q);
            pv.x = q.x * kUnits;
            pv.y = q.y * kUnits;
            pv.z = q.z * kUnits;
            if (v < p.uv.size()) {
              pv.s = p.uv[v].x;
              pv.t = p.uv[v].y;
            }
            if (lit) {
              pv.color_index = n.tod->index[pi][v];
            } else {
              // new geometry: its own palettes (lit like the level around it), else lit like the
              // level's from the color shown (game_export's)
              uint8_t col[32];
              if (own) {
                std::memcpy(col, &n.tod->palette->rgba[(size_t)n.tod->index[pi][v] * 32], 32);
              } else {
                Vec4 shown = v < p.col.size() ? p.col[v] : Vec4{1, 1, 1, 1};
                if (mat)
                  shown = {shown.x * mat->base_color.x, shown.y * mat->base_color.y, shown.z * mat->base_color.z, shown.w};
                tod_from_color(shown, ratios, col);
              }
              if (shift)
                for (auto& b : col) b = (uint8_t)((b >> shift) << shift);
              auto [it, fresh] = synth_index.emplace(std::string((const char*)col, 32), (u16)synth_index.size());
              if (fresh) synth.insert(synth.end(), col, col + 32);
              pv.color_index = it->second;
            }
            verts.push_back(pv);
          }
          auto& list = by_material[trans ? 1 : 0][p.material];
          for (uint32_t ix : p.idx) list.push_back(base + ix);
        }
      }
    };
    make();
    while ((!palette || compact) && synth_index.size() > kMaxColors && shift < 6) {
      shift++;
      make();
    }

    auto level = std::make_unique<tfrag3::Level>();
    level->level_name = c.name;
    const uint8_t* colors = palette && !compact ? palette->rgba.data() : synth.data();
    const size_t color_count = palette && !compact ? palette->size() : synth.size() / 32;
    c.triangles = 0;
    for (int t = 0; t < 2; t++) {
      if (by_material[t].empty()) continue;
      auto& tree = level->tfrag_trees[0].emplace_back();
      tree.kind = t ? tfrag3::TFragmentTreeKind::TRANS : tfrag3::TFragmentTreeKind::NORMAL;
      tree.use_strips = false;
      tree.colors = pack_palette(colors, color_count);
      for (const auto& [mi, indices] : by_material[t]) {
        const Material* mat = mi >= 0 && mi < (int)scene.materials.size() ? &scene.materials[mi] : nullptr;
        bool trans = false;
        auto& draw = tree.draws.emplace_back();
        draw.mode = draw_mode(mat, &trans);
        draw.tree_tex_id = mat && mat->texture >= 0 && mat->texture < (int)scene.textures.size()
                               ? mat->texture
                               : (int)scene.textures.size();  // the white texture
        draw.num_triangles = (u32)(indices.size() / 3);
        draw.unpacked.idx_of_first_idx_in_full_buffer = (u32)tree.unpacked.indices.size();
        auto& grp = draw.vis_groups.emplace_back();
        grp.num_inds = (u32)indices.size();
        grp.num_tris = draw.num_triangles;
        grp.vis_idx_in_pc_bvh = UINT16_MAX;  // always drawn (the builder writes no BVH either)
        tree.unpacked.indices.insert(tree.unpacked.indices.end(), indices.begin(), indices.end());
        c.triangles += draw.num_triangles;
      }
    }

    destroy(c);
    glGenBuffers(1, &c.vbo);
    glBindBuffer(GL_ARRAY_BUFFER, c.vbo);
    glBufferData(GL_ARRAY_BUFFER, verts.size() * sizeof(tfrag3::PreloadedVertex), verts.data(), GL_STATIC_DRAW);
    glBindBuffer(GL_ARRAY_BUFFER, 0);
    c.data = LevelData();
    c.data.tfrag_vertex_data[0].assign(level->tfrag_trees[0].size(), c.vbo);
    c.data.level = std::move(level);
    c.data.textures = tex_ids;
    c.data.load_id = next_load_id++;
    loader->levels[c.name] = &c.data;
    c.tfrag = std::make_unique<TFragment>(c.name, 0,
                                          std::vector<tfrag3::TFragmentTreeKind>{tfrag3::TFragmentTreeKind::NORMAL,
                                                                                tfrag3::TFragmentTreeKind::TRANS},
                                          false, 0, &anim_slots);
    c.center = box.valid() ? box.center() : Vec3{};
  }

  // the chunks the scene's decor falls in, made again where it changed
  void update(const Scene& scene, GameView::Stats& stats) {
    const SceneCache& cache = scene.cache();
    uint64_t mh = 1469598103934665603ull;
    for (const auto& mat : scene.materials) {
      mh = fnv(mh, &mat.texture, sizeof(mat.texture));
      mh = fnv(mh, &mat.base_color, sizeof(mat.base_color));
      mh = fnv(mh, &mat.alpha, sizeof(mat.alpha));
      mh = fnv(mh, &mat.blend, sizeof(mat.blend));
      mh = fnv(mh, &mat.cutoff, sizeof(mat.cutoff));
    }
    for (const auto& t : scene.textures) mh = fnv(mh, &t.version, sizeof(t.version));
    mh = fnv(mh, &scene.textures, sizeof(size_t));
    if (&scene != seen_scene) clear();
    if (cache.version == seen_cache && mh == seen_materials && &scene == seen_scene) return;
    seen_scene = &scene;
    seen_cache = cache.version;
    seen_materials = mh;
    const auto t0 = Clock::now();
    upload_textures(scene);

    // ponytail: every node is hashed again after any change (a few ms for a city); a revision
    // per node in the scene cache would hash only the changed ones
    std::map<Key, std::vector<uint32_t>> members;
    const size_t count = std::min(scene.nodes.size(), cache.visible.size());
    for (size_t i = 0; i < count; i++) {
      const Node& n = scene.nodes[i];
      if (n.kind != NodeKind::Render || !n.mesh || !cache.visible[i] || cache.actor[i]) continue;
      if (i < cache.reference.size() && cache.reference[i]) continue;
      // an invisible wall: the game does not draw it (the editor's own view does, to edit it)
      if (n.extras.contains("og_decor") && decor_behavior_of(n.extras).collision.kind == "invisible") continue;
      const TodPalette* pal = n.tod && !n.tod->palette->local && n.tod->fits(*n.mesh) ? n.tod->palette.get() : nullptr;
      const Vec3 at = i < cache.bounds.size() && cache.bounds[i].valid() ? cache.bounds[i].center()
                                                                          : cache.worlds[i].point({0, 0, 0});
      members[{pal, (int)std::floor(at.x / kCell), (int)std::floor(at.z / kCell)}].push_back((uint32_t)i);
    }
    for (auto it = chunks.begin(); it != chunks.end();) {
      if (!members.count(it->first)) {
        destroy(it->second);
        it = chunks.erase(it);
      } else {
        ++it;
      }
    }
    std::array<float, 8> ratios{};
    bool have_ratios = false;
    stats.rebuilt = 0;
    for (const auto& [key, nodes] : members) {
      uint64_t sig = mh;
      for (uint32_t i : nodes) {
        const Node& n = scene.nodes[i];
        sig = fnv(sig, &n.id, sizeof(n.id));
        sig = fnv(sig, &n.mesh->uid, sizeof(n.mesh->uid));
        sig = fnv(sig, &n.mesh->version, sizeof(n.mesh->version));
        sig = fnv(sig, cache.worlds[i].m, sizeof(cache.worlds[i].m));
        const void* tod = n.tod.get();
        sig = fnv(sig, &tod, sizeof(tod));
      }
      Chunk& c = chunks[key];
      if (c.name.empty()) c.name = "ogle-chunk-" + std::to_string(next_chunk++);
      if (c.tfrag && c.signature == sig) continue;
      if (!std::get<0>(key) && !have_ratios) {
        std::vector<uint32_t> ids;
        ids.reserve(count);
        for (size_t i = 0; i < count; i++) ids.push_back(scene.nodes[i].id);
        ratios = tod_ratios(scene, ids);
        have_ratios = true;
      }
      try {
        build(c, key, nodes, scene, ratios);
        c.signature = sig;
        stats.rebuilt++;
      } catch (const std::exception& e) {
        destroy(c);
        c.signature = 0;
        stats.error = e.what();
        LOG_WARN("game view: %s", e.what());
      }
    }
    stats.chunks = chunks.size();
    stats.triangles = 0;
    for (const auto& [k, c] : chunks) stats.triangles += c.triangles;
    if (stats.rebuilt) stats.ms_build = std::chrono::duration<double, std::milli>(Clock::now() - t0).count();
  }

  void render(tfrag3::TFragmentTreeKind kind, const std::vector<Chunk*>& order, GameView::Stats& stats) {
    TfragRenderSettings settings;
    settings.camera = cam;
    settings.tree_idx = 0;
    ScopedProfilerNode prof;
    const std::vector<tfrag3::TFragmentTreeKind> kinds{kind};
    for (Chunk* c : order) {
      if (!c->tfrag) continue;
      try {
        if (c->tfrag->setup_for_level({tfrag3::TFragmentTreeKind::NORMAL, tfrag3::TFragmentTreeKind::TRANS}, c->name,
                                      state.get()))
          c->tfrag->render_matching_trees(0, kinds, settings, state.get(), prof);
      } catch (const std::exception& e) {
        stats.error = e.what();
        LOG_WARN("game view: %s", e.what());
        destroy(*c);
      }
    }
    // the state the editor's renderer expects back
    glDisable(GL_PRIMITIVE_RESTART);
    glDisable(GL_BLEND);
    glBlendEquation(GL_FUNC_ADD);
    glDepthMask(GL_TRUE);
    glBindVertexArray(0);
    glActiveTexture(GL_TEXTURE0);
    glUseProgram(0);
  }
};

GameView::GameView() : m(std::make_unique<Impl>()) {}
GameView::~GameView() = default;

bool GameView::init(const std::string& shader_dir, std::string* error) {
  GLint major = 0, minor = 0;
  glGetIntegerv(GL_MAJOR_VERSION, &major);
  glGetIntegerv(GL_MINOR_VERSION, &minor);
  if (major * 10 + minor < 41) {
    if (error) *error = "OpenGL 4.1 or more is needed (" + std::to_string(major) + "." + std::to_string(minor) + ")";
    return false;
  }
  Shader::folder = shader_dir;
  try {
    m->state = std::make_unique<SharedRenderState>(nullptr, m->loader, GameVersion::Jak3);
    m->state->shaders[ShaderId::TFRAG3];  // compiled now: a missing or broken shader says so here
  } catch (const std::exception& e) {
    if (error) *error = e.what();
    m->state.reset();
    return false;
  }
  m->ok = true;
  return true;
}

void GameView::shutdown() {
  m->clear();
  m->sky.shutdown();
  if (m->white) glDeleteTextures(1, &m->white);
  m->white = 0;
  m->state.reset();
  m->ok = false;
}

void GameView::reset() {
  m->clear();
  m->seen_scene = nullptr;
}

bool GameView::ready() const { return m->ok; }

const GameMood& GameView::current_mood() const { return m->mood; }

void GameView::draw_opaque(const Scene& scene, const Camera& cam, int x, int y, int w, int h, Mat4* clip) {
  if (!m->ok) return;
  m->update(scene, stats);
  m->mood = game_mood(mood);
  m->cam = game_camera(cam, (float)w / (float)std::max(h, 1), m->mood, clip);
  m->state->fog_color = math::Vector<u8, 4>((u8)m->mood.fog_color[0], (u8)m->mood.fog_color[1],
                                            (u8)m->mood.fog_color[2], 128);
  m->state->fog_intensity = 1.f;
  // the sky behind, the depth the game's way (0 far)
  glViewport(x, y, w, h);
  glEnable(GL_SCISSOR_TEST);
  glScissor(x, y, w, h);
  glClearColor(m->mood.sky_color[0] / 255.f, m->mood.sky_color[1] / 255.f, m->mood.sky_color[2] / 255.f, 1.f);
  glClearDepth(0.0);
  glDepthMask(GL_TRUE);
  glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
  glClearDepth(1.0);
  glDisable(GL_SCISSOR_TEST);
  // its sun, its clouds, the stars at night
  m->sky.draw(m->mood, cam, (float)w / (float)std::max(h, 1), time);
  glDisable(GL_CULL_FACE);
  std::vector<Impl::Chunk*> order;
  for (auto& [k, c] : m->chunks) order.push_back(&c);
  m->render(tfrag3::TFragmentTreeKind::NORMAL, order, stats);
}

unsigned GameView::sky_preview(int w, int h, float yaw) {
  if (!m->ok) return 0;
  return m->sky.preview(game_mood(mood), w, h, yaw, time);
}

void GameView::draw_translucent() {
  if (!m->ok) return;
  // the farthest first
  std::vector<Impl::Chunk*> order;
  for (auto& [k, c] : m->chunks) order.push_back(&c);
  const Vec3 eye{m->cam.trans.x() / kUnits, m->cam.trans.y() / kUnits, m->cam.trans.z() / kUnits};
  std::sort(order.begin(), order.end(),
            [&](const Impl::Chunk* a, const Impl::Chunk* b) { return length2(a->center - eye) > length2(b->center - eye); });
  m->render(tfrag3::TFragmentTreeKind::TRANS, order, stats);
}

}  // namespace ogle
