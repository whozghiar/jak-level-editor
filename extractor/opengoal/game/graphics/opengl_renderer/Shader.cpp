#include "Shader.h"

#include <fstream>
#include <regex>
#include <sstream>

#include "common/log/log.h"
#include "common/util/Assert.h"
#include "common/util/FileUtil.h"

#include "game/graphics/pipelines/opengl.h"

std::string Shader::folder;

namespace {
// ogle: the text of a shader file of Shader::folder
std::string read_shader(const std::string& file) {
  std::ifstream f(Shader::folder + "/" + file, std::ios::binary);
  std::stringstream ss;
  ss << f.rdbuf();
  return ss.str();
}
}  // namespace

Shader::Shader(const std::string& shader_name, GameVersion version) : m_name(shader_name) {
  const std::string height_scale = version == GameVersion::Jak1 ? "1.0" : "0.5";
  const std::string scissor_height = version == GameVersion::Jak1 ? "448.0" : "416.0";
  const std::string scissor_adjust = "512.0 / " + scissor_height;

  // read the shader source
  auto vert_src = read_shader(shader_name + ".vert");
  auto frag_src = read_shader(shader_name + ".frag");

  vert_src = std::regex_replace(vert_src, std::regex("HEIGHT_SCALE"), height_scale);
  vert_src = std::regex_replace(vert_src, std::regex("SCISSOR_HEIGHT"), scissor_height);
  frag_src = std::regex_replace(frag_src, std::regex("SCISSOR_HEIGHT"), scissor_height);
  vert_src = std::regex_replace(vert_src, std::regex("SCISSOR_ADJUST"), "(" + scissor_adjust + ")");

  m_vert_shader = glCreateShader(GL_VERTEX_SHADER);
  const char* src = vert_src.c_str();
  glShaderSource(m_vert_shader, 1, &src, nullptr);
  glCompileShader(m_vert_shader);

  constexpr int len = 1024;
  int compile_ok;
  char err[len];

  glGetShaderiv(m_vert_shader, GL_COMPILE_STATUS, &compile_ok);
  if (!compile_ok) {
    glGetShaderInfoLog(m_vert_shader, len, nullptr, err);
    lg::error("Failed to compile vertex shader {}:\n{}", shader_name.c_str(), err);
    m_is_okay = false;
    return;
  }

  m_frag_shader = glCreateShader(GL_FRAGMENT_SHADER);
  src = frag_src.c_str();
  glShaderSource(m_frag_shader, 1, &src, nullptr);
  glCompileShader(m_frag_shader);

  glGetShaderiv(m_frag_shader, GL_COMPILE_STATUS, &compile_ok);
  if (!compile_ok) {
    glGetShaderInfoLog(m_frag_shader, len, nullptr, err);
    lg::error("Failed to compile fragment shader {}:\n{}", shader_name.c_str(), err);
    m_is_okay = false;
    return;
  }

  m_program = glCreateProgram();
  glAttachShader(m_program, m_vert_shader);
  glAttachShader(m_program, m_frag_shader);
  glLinkProgram(m_program);

  glGetProgramiv(m_program, GL_LINK_STATUS, &compile_ok);
  if (!compile_ok) {
    glGetProgramInfoLog(m_program, len, nullptr, err);
    lg::error("Failed to link shader {}:\n{}", shader_name.c_str(), err);
    m_is_okay = false;
    return;
  }

  // uniform samplers must be named matching the texture unit
  glUseProgram(m_program);
  for (int i = 1; i < 30; ++i) {
    std::string uniformName = "tex_T" + std::to_string(i);
    GLint texLoc = glGetUniformLocation(m_program, uniformName.c_str());
    if (texLoc != -1) {
      glUniform1i(texLoc, i);
    }
  }
  // assuming that the bones uniform block is always using binding point 1
  GLint bonesLoc = glGetUniformBlockIndex(m_program, "ub_bones");
  if (bonesLoc != -1) {
    glUniformBlockBinding(m_program, bonesLoc, 1);
  }

  glDeleteShader(m_vert_shader);
  glDeleteShader(m_frag_shader);
  m_is_okay = true;
}

void Shader::activate() const {
  ASSERT(m_is_okay);
  glUseProgram(m_program);
}

ShaderLibrary::ShaderLibrary(GameVersion version) : m_version(version) {
  // ogle: the names, compiled by at()
  struct Named {
    ShaderId id;
    const char* name;
  };
  const Named names[] = {
      {ShaderId::SOLID_COLOR, "solid_color"},
      {ShaderId::DIRECT_BASIC, "direct_basic"},
      {ShaderId::DIRECT_BASIC_TEXTURED, "direct_basic_textured"},
      {ShaderId::DIRECT_BASIC_TEXTURED_MULTI_UNIT, "direct_basic_textured_multi_unit"},
      {ShaderId::DEBUG_RED, "debug_red"},
      {ShaderId::SPRITE, "sprite_3d"},
      {ShaderId::SKY, "sky"},
      {ShaderId::SKY_BLEND, "sky_blend"},
      {ShaderId::TFRAG3, "tfrag3"},
      {ShaderId::TFRAG3_NO_TEX, "tfrag3_no_tex"},
      {ShaderId::SPRITE3, "sprite3_3d"},
      {ShaderId::DIRECT2, "direct2"},
      {ShaderId::EYE, "eye"},
      {ShaderId::GENERIC, "generic"},
      {ShaderId::OCEAN_TEXTURE, "ocean_texture"},
      {ShaderId::OCEAN_TEXTURE_MIPMAP, "ocean_texture_mipmap"},
      {ShaderId::OCEAN_COMMON, "ocean_common"},
      {ShaderId::SHRUB, "shrub"},
      {ShaderId::SHADOW, "shadow"},
      {ShaderId::COLLISION, "collision"},
      {ShaderId::MERC2, "merc2"},
      {ShaderId::SPRITE_DISTORT, "sprite_distort"},
      {ShaderId::SPRITE_DISTORT_INSTANCED, "sprite_distort_instanced"},
      {ShaderId::POST_PROCESSING, "post_processing"},
      {ShaderId::DEPTH_CUE, "depth_cue"},
      {ShaderId::EMERC, "emerc"},
      {ShaderId::GLOW_PROBE, "glow_probe"},
      {ShaderId::GLOW_PROBE_READ, "glow_probe_read"},
      {ShaderId::GLOW_PROBE_READ_DEBUG, "glow_probe_read_debug"},
      {ShaderId::GLOW_PROBE_DOWNSAMPLE, "glow_probe_downsample"},
      {ShaderId::GLOW_DRAW, "glow_draw"},
      {ShaderId::ETIE_BASE, "etie_base"},
      {ShaderId::ETIE, "etie"},
      {ShaderId::SHADOW2, "shadow2"},
      {ShaderId::TEX_ANIM, "tex_anim"},
      {ShaderId::GLOW_DEPTH_COPY, "glow_depth_copy"},
      {ShaderId::GLOW_PROBE_ON_GRID, "glow_probe_on_grid"},
      {ShaderId::HFRAG, "hfrag"},
      {ShaderId::HFRAG_MONTAGE, "hfrag_montage"},
      {ShaderId::PLAIN_TEXTURE, "plain_texture"},
      {ShaderId::TIE_WIND, "tie_wind"},
      {ShaderId::SIMPLE_TEXTURE, "simple_texture"},
      {ShaderId::SLOW_TIME, "slow_time"},
      {ShaderId::OCEAN_ENVMAP, "ocean_envmap"},
      {ShaderId::OCEAN_ENVMAP_HAZE, "ocean_envmap_haze"},
  };
  for (const auto& n : names) {
    m_names[(int)n.id] = n.name;
  }
}

Shader& ShaderLibrary::at(ShaderId id) {
  const int i = (int)id;
  if (!m_tried[i]) {
    m_tried[i] = true;
    m_shaders[i] = {m_names[i], m_version};
    ASSERT_MSG(m_shaders[i].okay(), "error compiling shader");
  }
  return m_shaders[i];
}

#if 0  // ogle: the game's list, compiled at start
ShaderLibrary::ShaderLibrary(GameVersion version) {
  at(ShaderId::SOLID_COLOR) = {"solid_color", version};
  at(ShaderId::DIRECT_BASIC) = {"direct_basic", version};
  at(ShaderId::DIRECT_BASIC_TEXTURED) = {"direct_basic_textured", version};
  at(ShaderId::DIRECT_BASIC_TEXTURED_MULTI_UNIT) = {"direct_basic_textured_multi_unit", version};
  at(ShaderId::DEBUG_RED) = {"debug_red", version};
  at(ShaderId::SPRITE) = {"sprite_3d", version};
  at(ShaderId::SKY) = {"sky", version};
  at(ShaderId::SKY_BLEND) = {"sky_blend", version};
  at(ShaderId::TFRAG3) = {"tfrag3", version};
  at(ShaderId::TFRAG3_NO_TEX) = {"tfrag3_no_tex", version};
  at(ShaderId::SPRITE3) = {"sprite3_3d", version};
  at(ShaderId::DIRECT2) = {"direct2", version};
  at(ShaderId::EYE) = {"eye", version};
  at(ShaderId::GENERIC) = {"generic", version};
  at(ShaderId::OCEAN_TEXTURE) = {"ocean_texture", version};
  at(ShaderId::OCEAN_TEXTURE_MIPMAP) = {"ocean_texture_mipmap", version};
  at(ShaderId::OCEAN_COMMON) = {"ocean_common", version};
  at(ShaderId::SHRUB) = {"shrub", version};
  at(ShaderId::SHADOW) = {"shadow", version};
  at(ShaderId::COLLISION) = {"collision", version};
  at(ShaderId::MERC2) = {"merc2", version};
  at(ShaderId::SPRITE_DISTORT) = {"sprite_distort", version};
  at(ShaderId::SPRITE_DISTORT_INSTANCED) = {"sprite_distort_instanced", version};
  at(ShaderId::POST_PROCESSING) = {"post_processing", version};
  at(ShaderId::DEPTH_CUE) = {"depth_cue", version};
  at(ShaderId::EMERC) = {"emerc", version};
  at(ShaderId::GLOW_PROBE) = {"glow_probe", version};
  at(ShaderId::GLOW_PROBE_READ) = {"glow_probe_read", version};
  at(ShaderId::GLOW_PROBE_READ_DEBUG) = {"glow_probe_read_debug", version};
  at(ShaderId::GLOW_PROBE_DOWNSAMPLE) = {"glow_probe_downsample", version};
  at(ShaderId::GLOW_DRAW) = {"glow_draw", version};
  at(ShaderId::ETIE_BASE) = {"etie_base", version};
  at(ShaderId::ETIE) = {"etie", version};
  at(ShaderId::SHADOW2) = {"shadow2", version};
  at(ShaderId::TEX_ANIM) = {"tex_anim", version};
  at(ShaderId::GLOW_DEPTH_COPY) = {"glow_depth_copy", version};
  at(ShaderId::GLOW_PROBE_ON_GRID) = {"glow_probe_on_grid", version};
  at(ShaderId::HFRAG) = {"hfrag", version};
  at(ShaderId::HFRAG_MONTAGE) = {"hfrag_montage", version};
  at(ShaderId::PLAIN_TEXTURE) = {"plain_texture", version};
  at(ShaderId::TIE_WIND) = {"tie_wind", version};
  at(ShaderId::SIMPLE_TEXTURE) = {"simple_texture", version};
  at(ShaderId::SLOW_TIME) = {"slow_time", version};
  at(ShaderId::OCEAN_ENVMAP) = {"ocean_envmap", version};
  at(ShaderId::OCEAN_ENVMAP_HAZE) = {"ocean_envmap_haze", version};

  for (auto& shader : m_shaders) {
    ASSERT_MSG(shader.okay(), "error compiling shader");
  }
}
#endif
