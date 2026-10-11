#pragma once

// OpenGL 3.3 renderer for the editor viewport. Draws the scene's render meshes (texture x vertex
// color, like the game's background), markers for the actors without a model, the outline of the
// selection and an immediate-mode batch of lines and triangles for tools (grid, gizmo...).

#include <set>
#include <string>
#include <unordered_map>
#include <vector>

#include "render/camera.h"
#include "scene/particles.h"
#include "scene/scene.h"

namespace ogle {

struct RenderSettings {
  bool wireframe = false;
  bool show_grid = true;
  bool show_actors = true;
  bool show_ocean = true;
  bool textures = true;
  bool vertex_colors = true;
  float vertex_color_scale = 2.f;  // PS2 vertex colors: 0x80 is neutral, stored as 128/255
  bool shade_uncolored = true;     // simple lighting for meshes without vertex colors
  bool frustum_culling = true;
  float clear_color[3] = {0.16f, 0.18f, 0.21f};
  // the far ocean: the sea to the horizon at that height, drawn behind everything (the game draws
  // it with the sky)
  bool far_ocean = false;
  float far_ocean_y = 0;
  float far_ocean_color[3] = {0.02f, 0.24f, 0.29f};
  // distance fog: what the low-detail backdrop meshes (mesh extras og_backdrop: the distant
  // mountains beyond a level's playable area, "-tfrag-lowres") fade into, by how far the camera
  // is from them. They are made to be seen through the game's fog, which the editor has none of
  // otherwise, and look wrong (a harsh, dithered pattern) without it; nothing else fogs.
  bool fog = true;
  float fog_color[3] = {0.60f, 0.68f, 0.72f};
  float fog_start = 50.f;   // meters
  float fog_end = 400.f;
  // the game view draws the decor (game_view/game_view.h): the editor draws the rest over it
  bool game_view = false;
  // the models lit as the game lights them (merc: the mood's ambient light and up to 3 lights, by
  // their normals) and fogged like the game's, in the game view
  bool game_light = false;
  float light_ambient[3] = {0, 0, 0};
  float light_dir[3][3] = {};
  float light_color[3][3] = {};
  float game_fog[4] = {0, 0, 0, 0};  // its color (0..1) and how much of it at most
  float game_fog_start = 32.f, game_fog_end = 1000.f;  // meters
};

struct DrawStats {
  size_t draws = 0;
  size_t triangles = 0;
  size_t nodes_culled = 0;
  // CPU time of draw_scene's steps (ms), for profiling
  double ms_cull = 0, ms_instances = 0, ms_draw = 0, ms_overlays = 0;
};

class Renderer {
 public:
  bool init(std::string* error);
  void shutdown();
  void reset();  // forget all GPU resources (new document)

  // viewport in framebuffer pixels, origin bottom-left (OpenGL); project() answers in the
  // viewport's logical size (UI coordinates), which differs from pixels on high-DPI screens
  void begin(const Camera& cam, int x, int y, int w, int h, const RenderSettings& rs,
             float logical_w = 0, float logical_h = 0);
  // what begin() sets without drawing: the camera and the viewport project() answers for
  void set_view(const Camera& cam, int x, int y, int w, int h, float logical_w = 0, float logical_h = 0);
  void draw_scene(Scene& scene, const RenderSettings& rs, const std::set<uint32_t>& selection,
                  uint32_t hovered);
  // One mesh with explicit materials and textures (bank thumbnails, placement preview). `tint`
  // is mixed into the color by its alpha; `alpha` < 1 draws it see-through.
  void draw_mesh(Mesh& mesh, const Mat4& world, const std::vector<Material>& materials,
                 const std::vector<Texture>& textures, const RenderSettings& rs,
                 const Vec4& tint = {0, 0, 0, 0}, float alpha = 1.f);

  // immediate mode, world space, flushed by flush()
  void line(const Vec3& a, const Vec3& b, uint32_t color);
  void tri(const Vec3& a, const Vec3& b, const Vec3& c, uint32_t color);
  void box_lines(const Mat4& m, const AABB& local, uint32_t color);
  void circle(const Vec3& center, const Vec3& axis_u, const Vec3& axis_v, float radius,
              uint32_t color, int segments = 48);
  void flush(bool depth_test, bool depth_offset = false);

  // The particles of the effects (scene/particles.h): sprites facing the camera, drawn with the
  // game's textures of them (`textures`: ParticleData::textures(), files of `set_sprite_folder`:
  // <library>/common/sprites/<page>/<texture>.png, index.json for the textures named by number),
  // after the scene, without writing the depth
  void set_sprite_folder(const std::string& folder);
  void draw_particles(const std::vector<ParticleSprite>& sprites, const std::vector<std::string>& textures);

  // What is drawn next goes over the game view (game_view/game_view.h): with the game camera's
  // matrix (world -> clip space) and its depth (reversed: 1 near, 0 far). nullptr: the editor's.
  void set_game_depth(const Mat4* clip);

  // world -> viewport pixels (origin top-left of the viewport). False when behind the camera.
  bool project(const Vec3& world, Vec2* out) const;
  const Mat4& view_proj() const { return m_viewproj; }

  // GL texture of a scene texture (uploaded on first use or after a change). The cache is by
  // index: a renderer draws the textures of one document (reset() before another).
  unsigned gl_texture(const Scene& scene, int texture_index);
  unsigned gl_texture(const std::vector<Texture>& textures, int texture_index);

  DrawStats stats;

 private:
  struct GpuPrim {  // a range of its mesh's buffers
    size_t first_index = 0;
    int count = 0;
    int base_vertex = 0;
    bool has_colors = false;
    int material = -1;
  };
  struct GpuMesh {
    unsigned vao = 0, vbo = 0, ebo = 0;
    uint64_t version = 0;
    std::vector<GpuPrim> prims;
    int last_used = 0;
  };
  struct GlTexture {
    unsigned id = 0;
    uint64_t version = 0;
  };
  struct LineVertex {
    Vec3 pos;
    uint32_t color;
  };

  struct Instance {  // per-instance vertex attributes of the mesh shader
    float m[16];      // world matrix, column-major
    float tint[4];    // selection / hover tint, mixed by its alpha
  };
  static Instance make_instance(const Mat4& world, const Vec4& tint);
  void upload_instances();
  void bind_instances(size_t first);
  void set_material(const Material* mat, const std::vector<Texture>& textures, bool has_colors,
                    const RenderSettings& rs, float alpha = 1.f);
  // 0: opaque and masked, 1: alpha blended, 2: additive and subtractive (BlendMode)
  static int material_pass(const Material* mat);
  static void set_blend(BlendMode b);

  GpuMesh& gpu_mesh(Mesh& mesh);
  void free_mesh(GpuMesh& m);
  void grid(const Camera& cam);
  void markers(Scene& scene, const RenderSettings& rs, const std::set<uint32_t>& selection,
               uint32_t hovered, const std::vector<Mat4>& worlds);
  bool visible(const AABB& box) const;
  // a depth test of the editor's (GL_LESS, GL_LEQUAL) over the game view's reversed depth
  unsigned depth_func(unsigned func) const;
  void polygon_offset(float factor, float units) const;

  unsigned m_mesh_prog = 0, m_flat_prog = 0, m_solid_prog = 0, m_sprite_prog = 0;
  unsigned m_sprite_vao = 0, m_sprite_vbo = 0;
  std::string m_sprite_folder;
  std::unordered_map<std::string, unsigned> m_sprite_textures;  // by name, 0: missing (white)
  std::unordered_map<std::string, std::string> m_sprite_index;  // "<page>:<index>" -> "<page>/<name>"
  unsigned sprite_texture(const std::string& name);
  struct {
    int flat_alpha = -1;
    int flat_mvp = -1;
    int flat_override = -1;
    int mesh_alpha_mode = -1;
    int mesh_instances = -1;
    int mesh_instance_base = -1;
    int mesh_base = -1;
    int mesh_cutoff = -1;
    int mesh_has_tex = -1;
    int mesh_shade = -1;
    int mesh_tex = -1;
    int mesh_use_vcol = -1;
    int mesh_vcol_scale = -1;
    int mesh_viewproj = -1;
    int mesh_fog = -1;
    int mesh_fog_color = -1;
    int mesh_fog_start = -1;
    int mesh_fog_end = -1;
    int mesh_cam_pos = -1;
    int mesh_game_light = -1;
    int mesh_light_ambient = -1;
    int mesh_light_dir = -1;
    int mesh_light_color = -1;
    int mesh_game_fog = -1;
    int mesh_game_fog_range = -1;
    int mesh_cam_fwd = -1;
    int solid_color = -1;
    int solid_mvp = -1;
  } m_u;
  unsigned m_imm_vao = 0, m_imm_vbo = 0;
  unsigned m_inst_vbo = 0, m_inst_tex = 0;
  struct MaterialState {  // the mesh program's material uniforms, to skip redundant updates
    int has_tex;
    float base[4];
    int alpha_mode;
    float cutoff;
    int use_vcol;
    int shade;
  };
  MaterialState m_material{};
  bool m_material_valid = false;
  unsigned m_bound_texture = ~0u;
  unsigned m_bound_vao = ~0u;
  std::vector<Instance> m_instances;
  std::vector<LineVertex> m_lines, m_tris;
  std::unordered_map<uint64_t, GpuMesh> m_meshes;
  std::vector<GlTexture> m_textures;
  unsigned m_white = 0;
  Mat4 m_view, m_proj, m_viewproj;
  Mat4 m_draw_vp;       // what the draws use: m_viewproj, or the game camera's over the game view
  bool m_game = false;  // over the game view (set_game_depth)
  Vec4 m_planes[6];
  Vec3 m_cam_pos;
  Vec3 m_cam_fwd;
  int m_vp[4] = {0, 0, 1, 1};
  float m_logical[2] = {1, 1};
  int m_frame = 0;
};

}  // namespace ogle
