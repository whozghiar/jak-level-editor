#include "game_view/sky.h"

#include <glad/glad.h>

#include <cmath>
#include <string>

#include "core/log.h"

namespace ogle {

namespace {

const char* kSkyVs = R"(#version 330 core
out vec2 v_uv;
void main() {
  // one triangle over the whole viewport
  vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  v_uv = p;
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
)";

const char* kSkyFs = R"(#version 330 core
in vec2 v_uv;
uniform int u_mode;          // 0: the camera's view, 1: a half turn around u_yaw
uniform vec3 u_fwd, u_right, u_up;
uniform vec2 u_tan;          // the view's tangents (x, y)
uniform float u_yaw;
uniform vec3 u_sky, u_horizon;  // 0..1
uniform vec3 u_sun_dir, u_sun_col, u_moon_dir, u_moon_col, u_amb;
uniform vec4 u_cloud;        // min, max, darkness, time (seconds)
uniform float u_haze;
uniform float u_flash;       // a lightning flash: the whole sky lit an instant
out vec4 o_color;

float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), f.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), f.x), f.y);
}
float fbm(vec2 p) {
  float v = 0.0, a = 0.5;
  for (int i = 0; i < 5; i++) {
    v += a * noise(p);
    p = p * 2.03 + vec2(1.7, 9.2);
    a *= 0.5;
  }
  return v;
}

void main() {
  vec3 d;
  if (u_mode == 0) {
    vec2 ndc = v_uv * 2.0 - 1.0;
    d = normalize(u_fwd + u_right * (ndc.x * u_tan.x) + u_up * (ndc.y * u_tan.y));
  } else {
    float yaw = u_yaw - (v_uv.x - 0.5) * 3.14159265;
    float pitch = radians(mix(-10.0, 70.0, v_uv.y));
    d = vec3(-sin(yaw) * cos(pitch), sin(pitch), -cos(yaw) * cos(pitch));
  }
  float e = d.y;
  // the mood's sky high up, its fog at the horizon, darker under it
  vec3 col = mix(u_horizon, u_sky, smoothstep(-0.02, 0.45, e));
  if (e < 0.0) col = u_horizon * mix(1.0, 0.8, clamp(-e * 4.0, 0.0, 1.0));
  col = mix(col, u_horizon, u_haze * 0.6 * (1.0 - smoothstep(0.0, 0.5, e)));
  // the clouds: a layer far above, drifting, as much of it as the weather's (min..max of its texture)
  float alpha = 0.0;
  vec3 cloud = vec3(0.0);
  float sun_scale = clamp(8.0 * (0.125 + u_sun_dir.y), 0.0, 1.0);    // sky-tng: sun0-scale
  float moon_scale = clamp(8.0 * (0.125 + u_moon_dir.y), 0.0, 1.0);
  if (e > 0.0) {
    vec2 p = d.xz / (e + 0.08) * 1.4 + vec2(u_cloud.w * 0.012, u_cloud.w * 0.004);
    float n = clamp((fbm(p) - 0.2) / 0.6, 0.0, 1.0);
    alpha = clamp((n - u_cloud.x) / max(u_cloud.y - u_cloud.x, 0.02), 0.0, 1.0) * smoothstep(0.0, 0.16, e);
    // lit as sky-tng lights them: the ambient light, the sun (and the moon) on their side
    float lit = 0.55 + 0.45 * clamp(dot(d, u_sun_dir) * 0.5 + 0.5, 0.0, 1.0);
    cloud = u_amb * 0.9 + u_sun_col * sun_scale * 0.45 * lit + u_moon_col * moon_scale * 0.25;
    cloud *= mix(1.0, 0.45, u_cloud.z) * (0.85 + 0.3 * n);
  }
  // the stars at night, where there are no clouds
  float night = 1.0 - clamp(max(u_sky.r, max(u_sky.g, u_sky.b)) * 4.0, 0.0, 1.0);
  if (e > 0.0 && night > 0.0) {
    vec2 cell = floor(vec2(atan(d.x, d.z) * 300.0, asin(e) * 300.0));
    float star = step(0.9965, hash(cell)) * hash(cell + 3.1);
    col += vec3(star * night * (1.0 - alpha));
  }
  // the sun and the moon, behind the clouds
  float s = max(dot(d, u_sun_dir), 0.0);
  vec3 sun = u_sun_col * (smoothstep(0.9992, 0.9996, s) * 2.0 + pow(s, 80.0) * 0.45 + pow(s, 6.0) * 0.08) * sun_scale;
  float m = max(dot(d, u_moon_dir), 0.0);
  vec3 moon = (vec3(0.85, 0.9, 1.0) * smoothstep(0.9995, 0.9997, m) * 1.2 + u_moon_col * pow(m, 60.0) * 0.3) * moon_scale;
  col += (sun + moon) * (1.0 - alpha * 0.92);
  col = mix(col, cloud, alpha);
  col += vec3(0.75, 0.8, 1.0) * u_flash * (0.4 + 0.6 * alpha);
  o_color = vec4(col, 1.0);
}
)";

unsigned compile(GLenum type, const char* src) {
  const unsigned s = glCreateShader(type);
  glShaderSource(s, 1, &src, nullptr);
  glCompileShader(s);
  int ok = 0;
  glGetShaderiv(s, GL_COMPILE_STATUS, &ok);
  if (!ok) {
    char log[2048];
    glGetShaderInfoLog(s, sizeof(log), nullptr, log);
    LOG_WARN("sky shader: %s", log);
    glDeleteShader(s);
    return 0;
  }
  return s;
}

}  // namespace

Sky::~Sky() = default;

bool Sky::init() {
  if (m_prog) return true;
  if (m_failed) return false;
  const unsigned vs = compile(GL_VERTEX_SHADER, kSkyVs), fs = compile(GL_FRAGMENT_SHADER, kSkyFs);
  if (vs && fs) {
    m_prog = glCreateProgram();
    glAttachShader(m_prog, vs);
    glAttachShader(m_prog, fs);
    glLinkProgram(m_prog);
    int ok = 0;
    glGetProgramiv(m_prog, GL_LINK_STATUS, &ok);
    if (!ok) {
      glDeleteProgram(m_prog);
      m_prog = 0;
    }
  }
  if (vs) glDeleteShader(vs);
  if (fs) glDeleteShader(fs);
  if (!m_prog) {
    m_failed = true;
    return false;
  }
  glGenVertexArrays(1, &m_vao);
  return true;
}

void Sky::shutdown() {
  if (m_prog) glDeleteProgram(m_prog);
  if (m_vao) glDeleteVertexArrays(1, &m_vao);
  if (m_fbo) glDeleteFramebuffers(1, &m_fbo);
  if (m_tex) glDeleteTextures(1, &m_tex);
  m_prog = m_vao = m_fbo = m_tex = 0;
  m_tw = m_th = 0;
}

void Sky::uniforms(const GameMood& m, float time) {
  auto loc = [&](const char* n) { return glGetUniformLocation(m_prog, n); };
  glUniform3f(loc("u_sky"), m.sky_color[0] / 255.f, m.sky_color[1] / 255.f, m.sky_color[2] / 255.f);
  glUniform3f(loc("u_horizon"), m.fog_color[0] / 255.f, m.fog_color[1] / 255.f, m.fog_color[2] / 255.f);
  glUniform3fv(loc("u_sun_dir"), 1, m.sun_dir);
  glUniform3fv(loc("u_moon_dir"), 1, m.moon_dir);
  glUniform3f(loc("u_sun_col"), m.sun_color[0] * 0.5f, m.sun_color[1] * 0.5f, m.sun_color[2] * 0.5f);
  glUniform3f(loc("u_moon_col"), m.moon_color[0] * 0.5f, m.moon_color[1] * 0.5f, m.moon_color[2] * 0.5f);
  // the ambient light of the palettes (a lightning flash brightens it): 1.0 is the game's 128
  glUniform3f(loc("u_amb"), m.times[0][0] * 0.5f, m.times[0][1] * 0.5f, m.times[0][2] * 0.5f);
  glUniform1f(loc("u_flash"), m.flash);
  glUniform4f(loc("u_cloud"), m.cloud_min, m.cloud_max, m.cloud_dark, time);
  glUniform1f(loc("u_haze"), m.haze);
}

void Sky::draw(const GameMood& m, const Camera& cam, float aspect, float time) {
  if (!init()) return;
  glUseProgram(m_prog);
  uniforms(m, time);
  const Vec3 f = cam.forward(), r = cam.right(), u = cam.up();
  const float ty = std::tan(radians(cam.fov_deg) * 0.5f);
  glUniform1i(glGetUniformLocation(m_prog, "u_mode"), 0);
  glUniform3f(glGetUniformLocation(m_prog, "u_fwd"), f.x, f.y, f.z);
  glUniform3f(glGetUniformLocation(m_prog, "u_right"), r.x, r.y, r.z);
  glUniform3f(glGetUniformLocation(m_prog, "u_up"), u.x, u.y, u.z);
  glUniform2f(glGetUniformLocation(m_prog, "u_tan"), ty * aspect, ty);
  glDisable(GL_DEPTH_TEST);
  glDepthMask(GL_FALSE);
  glDisable(GL_BLEND);
  glDisable(GL_CULL_FACE);
  glBindVertexArray(m_vao);
  glDrawArrays(GL_TRIANGLES, 0, 3);
  glBindVertexArray(0);
  glDepthMask(GL_TRUE);
  glUseProgram(0);
}

unsigned Sky::preview(const GameMood& m, int w, int h, float yaw, float time) {
  if (w <= 0 || h <= 0 || !init()) return 0;
  // what was bound (the window, or the offscreen target of a capture) is bound back
  GLint prev_fbo = 0, prev_vp[4];
  glGetIntegerv(GL_FRAMEBUFFER_BINDING, &prev_fbo);
  glGetIntegerv(GL_VIEWPORT, prev_vp);
  const GLboolean scissor = glIsEnabled(GL_SCISSOR_TEST);
  if (!m_tex || m_tw != w || m_th != h) {
    if (!m_tex) glGenTextures(1, &m_tex);
    glBindTexture(GL_TEXTURE_2D, m_tex);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_RGBA, GL_UNSIGNED_BYTE, nullptr);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    if (!m_fbo) glGenFramebuffers(1, &m_fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, m_fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, m_tex, 0);
    m_tw = w;
    m_th = h;
  }
  glBindFramebuffer(GL_FRAMEBUFFER, m_fbo);
  glViewport(0, 0, w, h);
  glDisable(GL_SCISSOR_TEST);
  glUseProgram(m_prog);
  uniforms(m, time);
  glUniform1i(glGetUniformLocation(m_prog, "u_mode"), 1);
  glUniform1f(glGetUniformLocation(m_prog, "u_yaw"), yaw);
  glDisable(GL_DEPTH_TEST);
  glDepthMask(GL_FALSE);
  glDisable(GL_BLEND);
  glBindVertexArray(m_vao);
  glDrawArrays(GL_TRIANGLES, 0, 3);
  glBindVertexArray(0);
  glDepthMask(GL_TRUE);
  glUseProgram(0);
  glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)prev_fbo);
  glViewport(prev_vp[0], prev_vp[1], prev_vp[2], prev_vp[3]);
  if (scissor) glEnable(GL_SCISSOR_TEST);
  return m_tex;
}

}  // namespace ogle
