// The sky of the game view (src/game_view/sky.{h,cpp}): its colors at the hour and in the weather
// (the mood's sky high up, its fog at the horizon), the sun, the moon and the stars, the clouds
// drifting, lit like the game lights them (sky-tng: the ambient light and the sun on them, darker
// in a storm), the flash of the lightning. Drawn behind the level, or as a half turn around the
// view for the Game rendering window (a whole turn loses the sense of where things are).

import type { Camera } from "./camera";
import { radians } from "./math";

// call("game_mood"): the game's lighting at an hour and in a weather (crates/core/src/game/mood.rs
// GameMood, src/game_view/game_mood.h)
export interface GameMood {
  // per palette: its color (r, g, b) and weight (w), as the game's mood-context times
  times: number[][];
  fog_color: number[]; // 0..255
  fog_start: number; // game units (4096 per meter)
  fog_end: number;
  fog_max: number; // how much of the color is kept, near and far (255: all)
  fog_min: number;
  sky_color: number[]; // 0..255: the sky behind the level, high up
  // the lights of the models: the ambient light, up to 3 directions of the sun (toward it) with
  // their colors times their intensity at that hour
  light_ambient: number[];
  light_dir: number[][];
  light_color: number[][];
  // the sky: toward the sun and the moon, their colors, how much of the sky the clouds cover
  // (min..max of their texture shown; 1, 1: none) and how dark they are
  sun_dir: number[];
  moon_dir: number[];
  sun_color: number[];
  moon_color: number[];
  cloud_min: number;
  cloud_max: number;
  cloud_dark: number;
  haze: number; // 0..1: the haze toward the horizon
  flash: number; // a lightning flash now
}

const SKY_VS = `#version 300 es
out vec2 v_uv;
void main() {
  // one triangle over the whole viewport
  vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  v_uv = p;
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
`;

const SKY_FS = `#version 300 es
precision highp float;
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
`;

export interface Program {
  p: WebGLProgram;
  u: Record<string, WebGLUniformLocation | null>;
}

// a program and its uniforms' locations (looked up once)
export function program(
  gl: WebGL2RenderingContext,
  vs: string,
  fs: string,
): Program {
  const compile = (type: number, src: string) => {
    const s = gl.createShader(type)!;
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
      throw new Error(gl.getShaderInfoLog(s) ?? "shader");
    return s;
  };
  const p = gl.createProgram()!;
  const v = compile(gl.VERTEX_SHADER, vs),
    f = compile(gl.FRAGMENT_SHADER, fs);
  gl.attachShader(p, v);
  gl.attachShader(p, f);
  gl.linkProgram(p);
  gl.deleteShader(v);
  gl.deleteShader(f);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS))
    throw new Error(gl.getProgramInfoLog(p) ?? "program");
  const u: Program["u"] = {};
  const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS) as number;
  for (let i = 0; i < n; i++) {
    const name = gl.getActiveUniform(p, i)!.name.replace(/\[0\]$/, "");
    u[name] = gl.getUniformLocation(p, name);
  }
  return { p, u };
}

interface Sky {
  prog: Program;
  vao: WebGLVertexArrayObject; // none of its attributes enabled
}
const skies = new WeakMap<WebGL2RenderingContext, Sky | null>();

function sky(gl: WebGL2RenderingContext): Sky | null {
  if (skies.has(gl)) return skies.get(gl)!;
  let s: Sky | null = null;
  try {
    s = { prog: program(gl, SKY_VS, SKY_FS), vao: gl.createVertexArray()! };
  } catch (e) {
    console.warn("[game view] sky shader:", e);
  }
  skies.set(gl, s);
  return s;
}

function uniforms(
  gl: WebGL2RenderingContext,
  u: Program["u"],
  m: GameMood,
  time: number,
) {
  const v3 = (n: string, v: number[], k = 1) =>
    gl.uniform3f(u[n], v[0] * k, v[1] * k, v[2] * k);
  v3("u_sky", m.sky_color, 1 / 255);
  v3("u_horizon", m.fog_color, 1 / 255);
  v3("u_sun_dir", m.sun_dir);
  v3("u_moon_dir", m.moon_dir);
  v3("u_sun_col", m.sun_color, 0.5);
  v3("u_moon_col", m.moon_color, 0.5);
  // the ambient light of the palettes (a lightning flash brightens it): 1.0 is the game's 128
  v3("u_amb", m.times[0], 0.5);
  gl.uniform1f(u.u_flash, m.flash);
  gl.uniform4f(u.u_cloud, m.cloud_min, m.cloud_max, m.cloud_dark, time);
  gl.uniform1f(u.u_haze, m.haze);
}

function fullscreen(gl: WebGL2RenderingContext, s: Sky) {
  gl.disable(gl.DEPTH_TEST);
  gl.depthMask(false);
  gl.disable(gl.BLEND);
  gl.disable(gl.CULL_FACE);
  gl.bindVertexArray(s.vao);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  gl.bindVertexArray(null);
  gl.depthMask(true);
  gl.enable(gl.DEPTH_TEST);
}

// the sky the camera sees, over the viewport bound (nothing of the depth changes)
export function drawSky(
  gl: WebGL2RenderingContext,
  m: GameMood,
  cam: Camera,
  aspect: number,
  time: number,
) {
  const s = sky(gl);
  if (!s) return;
  const u = s.prog.u;
  gl.useProgram(s.prog.p);
  uniforms(gl, u, m, time);
  const f = cam.forward(),
    r = cam.right(),
    up = cam.up();
  const ty = Math.tan(radians(cam.fov_deg) * 0.5);
  gl.uniform1i(u.u_mode, 0);
  gl.uniform3f(u.u_fwd, f[0], f[1], f[2]);
  gl.uniform3f(u.u_right, r[0], r[1], r[2]);
  gl.uniform3f(u.u_up, up[0], up[1], up[2]);
  gl.uniform2f(u.u_tan, ty * aspect, ty);
  fullscreen(gl, s);
}

// GameView::sky_preview, for the Game rendering window: a half turn of the sky centered on `yaw`
// (the camera's angle), from 10 degrees under the horizon (the bottom) to 70 above (the top),
// over the whole canvas. The canvas gets a WebGL2 context of its own: draw the horizon line and
// the directions over it with HTML. False when it cannot be drawn.
export function skyPreview(
  canvas: HTMLCanvasElement,
  m: GameMood,
  yaw: number,
  time: number,
): boolean {
  const gl = canvas.getContext("webgl2", { alpha: false, antialias: false });
  const s = gl && sky(gl);
  if (!gl || !s || canvas.width <= 0 || canvas.height <= 0) return false;
  gl.viewport(0, 0, canvas.width, canvas.height);
  gl.useProgram(s.prog.p);
  uniforms(gl, s.prog.u, m, time);
  gl.uniform1i(s.prog.u.u_mode, 1);
  gl.uniform1f(s.prog.u.u_yaw, yaw);
  fullscreen(gl, s);
  return true;
}
