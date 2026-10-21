// The renderer's shaders (renderer.cpp), in GLSL ES 3.00. WebGL2 has no texture buffers: the
// instances are in a float texture 4096 texels wide, 5 texels each (world matrix columns, tint).

export const INSTANCE_TEXELS_PER_ROW = 4096;

// Meshes are drawn instanced: every node of a mesh is one instance (a decor prototype and its
// copies: one draw per primitive). A draw reads its instances from u_instance_base on.
export const MESH_VS = `#version 300 es
precision highp float;
precision highp int;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_nrm;
layout(location = 2) in vec2 a_uv;
layout(location = 3) in vec4 a_col;
uniform mat4 u_viewproj;
uniform highp sampler2D u_instances;
uniform int u_instance_base;
uniform vec2 u_uv_scroll;
out vec3 v_nrm;
out vec2 v_uv;
out vec4 v_col;
out vec4 v_tint;
out vec3 v_world;
vec4 instance(int t) { return texelFetch(u_instances, ivec2(t % ${INSTANCE_TEXELS_PER_ROW}, t / ${INSTANCE_TEXELS_PER_ROW}), 0); }
void main() {
  int base = (u_instance_base + gl_InstanceID) * 5;
  mat4 model = mat4(instance(base), instance(base + 1), instance(base + 2), instance(base + 3));
  vec4 a_tint = instance(base + 4);
  // the normals of a stretched element: each axis divided by its scale squared (rotation * 1/scale)
  mat3 nm = mat3(model);
  for (int i = 0; i < 3; i++) nm[i] /= max(dot(nm[i], nm[i]), 1e-8);
  v_nrm = nm * a_nrm;
  v_uv = a_uv + u_uv_scroll;
  v_col = a_col;
  v_tint = a_tint;
  vec4 world = model * vec4(a_pos, 1.0);
  v_world = world.xyz;
  gl_Position = u_viewproj * world;
}
`;

export const MESH_FS = `#version 300 es
precision highp float;
precision highp int;
in vec3 v_nrm;
in vec2 v_uv;
in vec4 v_col;
in vec4 v_tint;
in vec3 v_world;
uniform sampler2D u_tex;
uniform int u_has_tex;
uniform vec4 u_base;
uniform int u_alpha_mode;
uniform float u_cutoff;
uniform int u_use_vcol;
uniform float u_vcol_scale;
uniform int u_shade;
uniform int u_fog;
uniform vec3 u_fog_color;
uniform float u_fog_start;
uniform float u_fog_end;
uniform vec3 u_cam_pos;
// the game view: the models lit as merc lights them, fogged as the game fogs
uniform int u_game_light;
uniform vec3 u_light_ambient;
uniform vec3 u_light_dir[3];
uniform vec3 u_light_color[3];
uniform vec4 u_game_fog;
uniform vec2 u_game_fog_range;
uniform vec3 u_cam_fwd;
out vec4 o_color;
// lit colors above 0.8 rolled off towards 1 instead of cut: a bright texture under a bright light
// (the PS2 doubles vertex colors) keeps its detail instead of turning white
vec3 shoulder(vec3 x) {
  vec3 over = max(x - 0.8, 0.0);
  return min(x, vec3(0.8)) + 0.2 * (1.0 - exp(-over / 0.2));
}
void main() {
  vec4 c = u_base;
  if (u_has_tex == 1) c *= texture(u_tex, v_uv);
  if (u_game_light == 1) {
    vec3 n = normalize(v_nrm);
    if (!gl_FrontFacing) n = -n;
    vec3 light = u_light_ambient;
    for (int i = 0; i < 3; i++) light += max(dot(n, u_light_dir[i]), 0.0) * u_light_color[i];
    vec3 vcol = u_use_vcol == 1 ? v_col.rgb * u_vcol_scale : vec3(1.0);
    c.rgb = min(c.rgb * vcol * light, vec3(1.0));
  } else if (u_use_vcol == 1) {
    c.rgb = shoulder(c.rgb * clamp(v_col.rgb * u_vcol_scale, 0.0, 4.0));
  }
  // alpha test, like the PS2: masked and blended materials (most of a level's) drop the texels
  // under their reference and write depth for the others
  if (u_alpha_mode != 0 && c.a < u_cutoff) discard;
  if (u_shade == 1 && u_game_light == 0) {
    vec3 n = normalize(v_nrm);
    if (!gl_FrontFacing) n = -n;
    float l = 0.45 + 0.55 * max(dot(n, normalize(vec3(0.35, 1.0, 0.25))), 0.0);
    c.rgb *= l;
  }
  if (u_fog == 1) {
    float d = length(v_world - u_cam_pos);
    float f = clamp((d - u_fog_start) / max(u_fog_end - u_fog_start, 1.0), 0.0, 1.0);
    c.rgb = mix(c.rgb, u_fog_color, f);
  }
  if (u_game_light == 1) {
    float d = dot(v_world - u_cam_pos, u_cam_fwd);
    float f = clamp((d - u_game_fog_range.x) / max(u_game_fog_range.y - u_game_fog_range.x, 1.0), 0.0, 1.0);
    c.rgb = mix(c.rgb, u_game_fog.rgb, f * u_game_fog.a);
  }
  c.rgb = mix(c.rgb, v_tint.rgb, v_tint.a);
  o_color = c;
}
`;

// the ocean: a flat mesh, its surface moving (drawWater)
export const WATER_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
uniform mat4 u_viewproj;
uniform mat4 u_world;
out vec3 v_world;
void main() {
  vec4 w = u_world * vec4(a_pos, 1.0);
  v_world = w.xyz;
  gl_Position = u_viewproj * w;
}
`;

export const WATER_FS = `#version 300 es
precision highp float;
in vec3 v_world;
uniform float u_time;
uniform vec3 u_color;
uniform vec3 u_deep;
uniform vec3 u_sky;
uniform vec3 u_sun_dir;
uniform vec3 u_sun_color;
uniform vec3 u_cam_pos;
uniform vec3 u_cam_fwd;
uniform float u_alpha;
uniform vec4 u_fog;  // the game's fog (game view): its color, how much at most (0: none)
uniform vec2 u_fog_range;
out vec4 o_color;
// the swell: waves of a few lengths and directions, each as fast as deep water waves of its length
// go; their slopes bend the normal, the small ones fading away with the distance (no shimmer)
vec3 waves(vec2 p, float t, float dist, out float height) {
  const vec2 dirs[6] = vec2[](vec2(0.98, 0.2), vec2(0.71, 0.71), vec2(-0.29, 0.96), vec2(0.91, -0.41),
                              vec2(-0.85, 0.53), vec2(0.2, -0.98));
  const float lens[6] = float[](31.0, 17.0, 9.7, 5.3, 2.9, 1.6);
  vec2 slope = vec2(0.0);
  height = 0.0;
  for (int i = 0; i < 6; i++) {
    float k = 6.2831853 / lens[i];
    float w = sqrt(9.81 * k);
    float a = 0.05 / k * clamp(1.0 - dist / (lens[i] * 90.0), 0.0, 1.0);
    float ph = k * dot(dirs[i], p) - w * t + float(i) * 1.7;
    height += a * sin(ph);
    slope += a * k * cos(ph) * dirs[i];
  }
  return normalize(vec3(-slope.x, 1.0, -slope.y));
}
void main() {
  float dist = length(u_cam_pos - v_world);
  float h;
  vec3 n = waves(v_world.xz, u_time, dist, h);
  vec3 v = normalize(u_cam_pos - v_world);
  float fresnel = 0.02 + 0.98 * pow(1.0 - max(dot(n, v), 0.0), 5.0);
  vec3 r = reflect(-v, n);
  vec3 sky = mix(u_sky * 1.15 + 0.05, u_sky, clamp(r.y, 0.0, 1.0));
  // the water: deeper looking far away, lighter on the crests
  vec3 body = mix(u_color, u_deep, clamp(dist / 700.0, 0.0, 1.0) * 0.6);
  body *= 0.92 + 0.9 * clamp(h, -0.15, 0.3);
  vec3 c = mix(body, sky, clamp(fresnel * 0.85, 0.0, 1.0));
  c += u_sun_color * pow(max(dot(r, normalize(u_sun_dir)), 0.0), 160.0) * 1.4;
  float alpha = mix(u_alpha, 1.0, fresnel);
  if (u_fog.a > 0.0) {
    float d = dot(v_world - u_cam_pos, u_cam_fwd);
    float f = clamp((d - u_fog_range.x) / max(u_fog_range.y - u_fog_range.x, 1.0), 0.0, 1.0);
    c = mix(c, u_fog.rgb, f * u_fog.a);
  }
  o_color = vec4(c, alpha);
}
`;

export const FLAT_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec4 a_col;
uniform mat4 u_mvp;
out vec4 v_col;
void main() {
  v_col = a_col;
  gl_Position = u_mvp * vec4(a_pos, 1.0);
}
`;

export const FLAT_FS = `#version 300 es
precision highp float;
in vec4 v_col;
uniform float u_alpha;
uniform vec4 u_override;
out vec4 o_color;
void main() {
  vec4 c = v_col;
  c.a *= u_alpha;
  c = mix(c, vec4(u_override.rgb, c.a), u_override.a);
  o_color = c;
}
`;

// the outlines: GL ES has no polygon mode nor line offset, the edges are drawn as lines pulled a
// little toward the camera (u_depth_bias, in clip space: the C++ glPolygonOffset(-1, -1))
export const SOLID_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
uniform mat4 u_mvp;
uniform float u_depth_bias;
void main() {
  gl_Position = u_mvp * vec4(a_pos, 1.0);
  gl_Position.z -= u_depth_bias * gl_Position.w;
}
`;

export const SOLID_FS = `#version 300 es
precision highp float;
uniform vec4 u_color;
out vec4 o_color;
void main() { o_color = u_color; }
`;
