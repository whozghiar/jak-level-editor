// The level as the game draws it (Jak 3), in the viewport (src/game_view/game_view.{h,cpp}): the
// engine makes the document's decor into what OpenGOAL's level builder makes of a custom level
// (crates/core/src/game/view.rs: the triangles of every piece in the game's units, a draw per
// material, the 8 palettes of each vertex), in chunks; they are drawn here by the game's own
// TFragment shader (tfrag3.vert/.frag, ported), with the game's camera, its time of the day (the
// call `game_mood`, sky.ts) and its fog. Actors' models, the levels shown for reference and the
// editor's overlays are the editor's to draw over it, with the game camera's matrix and its depth
// (Renderer.setGameDepth).
//
// The game's depth is reversed (1 near, 0 far, GEQUAL) in OpenGL's usual -1..1 clip space: the
// game's renderer uses no glClipControl, so its matrix gives WebGL2 the same depth it gives OpenGL.

import { engine } from "../lib/engine.svelte";
import type { Camera } from "./camera";
import { call, engineReady, fetchBytes, onEngine } from "./engine";
import {
  clamp,
  cross,
  dot,
  length2,
  normalize,
  packRgba,
  radians,
  sub,
  type Mat4,
  type Vec3,
} from "./math";
import type { Renderer } from "./renderer";
import { drawSky, program, type GameMood, type Program } from "./sky";
import type { RenderSettings, WaterLook } from "./types";
import { viewport } from "./viewport_api";

export type { GameMood };

const K_UNITS = 4096; // game units per meter

// ---------------------------------------------------------------------------------------------
// the engine's answers (docs/architecture.md §14, "Game view"), little-endian, read in place

const magic = (dv: DataView, s: string) =>
  dv.byteLength >= 12 &&
  [0, 1, 2, 3].every((i) => dv.getUint8(i) === s.charCodeAt(i)) &&
  dv.getUint32(4, true) === 1;

const box = (dv: DataView, o: number): [Vec3, Vec3] => {
  const f = (i: number) => dv.getFloat32(o + i * 4, true);
  return [
    [f(0), f(1), f(2)],
    [f(3), f(4), f(5)],
  ];
};

export interface ChunkInfo {
  id: number;
  version: number;
  palette: number;
  triangles: number;
  lo: Vec3; // its box, meters
  hi: Vec3;
}

// gameview (JLGL 1): magic, version, count, then per chunk (40 bytes) id, version, palette,
// triangles, f32 lo[3], hi[3]
export function parseChunkList(buf: ArrayBuffer): ChunkInfo[] {
  const dv = new DataView(buf);
  if (!magic(dv, "JLGL")) throw new Error("gameview: not JLGL 1");
  const out: ChunkInfo[] = [];
  for (let i = 0, o = 12; i < dv.getUint32(8, true); i++, o += 40) {
    const [lo, hi] = box(dv, o + 16);
    out.push({
      id: dv.getUint32(o, true),
      version: dv.getUint32(o + 4, true),
      palette: dv.getUint32(o + 8, true),
      triangles: dv.getUint32(o + 12, true),
      lo,
      hi,
    });
  }
  return out;
}

export const DrawFlag = {
  Translucent: 1, // the translucent tree: after the editor's opaque models
  DepthWrite: 2,
  AlphaTest: 4, // what is under aref / 127 is not drawn
  OwnTexture: 8, // a texture of the editor's: its alpha halved (the game's are the PS2's)
} as const;

// the PS2's blends (DrawMode::AlphaBlend)
export const AlphaBlend = {
  Disabled: 0,
  SrcDstSrcDst: 1, // alpha
  Src0SrcDst: 2, // add alpha
  Src0FixDst: 3, // add
  ZeroSrcSrcDst: 4, // subtract
  SrcDstFixDst: 5, // half
} as const;

export interface ChunkDraw {
  first: number; // into the chunk's indices
  count: number;
  texture: number; // the document's, -1 white
  flags: number; // DrawFlag
  aref: number;
  blend: number; // AlphaBlend
}

export interface ChunkData extends ChunkInfo {
  draws: ChunkDraw[];
  vertices: Uint8Array; // 24 bytes each: f32 x, y, z (game units), f32 s, t, u16 color, u16 0
  indices: Uint32Array;
}

// gameview/chunk/<id> (JLGC 1): magic, version, id, chunk version, palette, vertices, indices,
// draws, f32 lo[3], hi[3]; the draws (16 bytes), the vertices (24 bytes), the indices (u32)
export function parseChunk(buf: ArrayBuffer): ChunkData {
  const dv = new DataView(buf);
  if (!magic(dv, "JLGC")) throw new Error("gameview chunk: not JLGC 1");
  const u = (o: number) => dv.getUint32(o, true);
  const [nv, ni, nd] = [u(20), u(24), u(28)];
  const [lo, hi] = box(dv, 32);
  const draws: ChunkDraw[] = [];
  for (let i = 0, o = 56; i < nd; i++, o += 16)
    draws.push({
      first: u(o),
      count: u(o + 4),
      texture: dv.getInt32(o + 8, true),
      flags: dv.getUint8(o + 12),
      aref: dv.getUint8(o + 13),
      blend: dv.getUint8(o + 14),
    });
  const vo = 56 + nd * 16,
    io = vo + nv * 24;
  if (buf.byteLength < io + ni * 4) throw new Error("gameview chunk: short");
  return {
    id: u(8),
    version: u(12),
    palette: u(16),
    triangles: Math.floor(ni / 3),
    lo,
    hi,
    draws,
    vertices: new Uint8Array(buf, vo, nv * 24),
    indices: new Uint32Array(buf, io, ni),
  };
}

// gameview/palette/<key> (JLGP 1): magic, version, colors, then 32 bytes per color (its 8
// palettes' RGBA, 128 is 1.0)
export function parsePalette(buf: ArrayBuffer): {
  colors: number;
  rgba: Uint8Array;
} {
  const dv = new DataView(buf);
  if (!magic(dv, "JLGP")) throw new Error("gameview palette: not JLGP 1");
  const colors = dv.getUint32(8, true);
  return { colors, rgba: new Uint8Array(buf, 12, colors * 32) };
}

// ---------------------------------------------------------------------------------------------
// the game's camera

// make_new_cam_mat (background_common.cpp): the matrix the game's background shaders take
function makeNewCamMat(
  cam_T_w: number[][],
  persp: number[][],
  fog_constant: number,
  hvdf_z: number,
): number[][] {
  // the perspective x-scaling, mapped to a 256-pixel buffer: on PC, divided by the pixel width,
  // the fog constant of the game's perspective divide included
  const pc_pxx = (fog_constant * persp[0][0]) / 256;
  // y: a later SCISSOR scaling in the shader expects this ratio
  const pc_pyy = (-fog_constant * persp[1][1]) / 128;
  // the depth: once to write in the depth buffer, scaled for PC, once for the perspective divide
  let depth_scale = (fog_constant * persp[2][2]) / 8388608;
  const game_pzw = persp[2][3];
  const game_depth_offset = persp[3][2];
  // shifting the depth buffer to line up with OpenGL is adding transformed.w * (hvdf_z / 8388608
  // - 1) to the depth; w is depth * pzw
  depth_scale += (hvdf_z / 8388608 - 1) * game_pzw;
  const scale = [pc_pxx, pc_pyy, depth_scale];
  const r = [0, 1, 2, 3].map(() => [0, 0, 0, 0]);
  for (let row = 0; row < 3; row++) {
    for (let col = 0; col < 3; col++)
      r[row][col] = cam_T_w[row][col] * scale[col];
    r[row][3] = cam_T_w[row][2] * game_pzw;
  }
  r[3][2] = (fog_constant * game_depth_offset) / 8388608;
  return r;
}

export interface GameCamera {
  pc_camera: Float32Array; // tfrag3.vert's
  hvdf_offset: [number, number, number, number];
  cam_trans: [number, number, number, number];
  fog: [number, number, number]; // fog constant, fog_min, fog_max
  clip: Mat4; // world meters -> clip space, the game's depth: what the editor draws over it
}

// The game's camera for the editor's (update-math-camera, update-camera, and the packet of
// background.gc the background renderers take), and the matrix its shaders make of it (world
// meters -> clip space: tfrag3.vert, make_new_cam_mat): the depth is the game's, 1 near, 0 far.
export function gameCamera(
  cam: Camera,
  aspect: number,
  mood: GameMood,
): GameCamera {
  const d = 1024,
    f = 40960000; // near (0.25 m), far (10 km)
  // the PC's view: the 4:3 cropping, the wider screens see more on the sides
  const yr = Math.tan(radians(cam.fov_deg) * 0.5);
  const xr = yr * aspect;
  // the game's fov (at 4:3, in its angle units: 65536 a turn) corrects the fog and the depth
  const fov = ((2 * Math.atan(yr / 0.75)) / (2 * Math.PI)) * 65536;
  const fcf = 0.00008583069 * Math.min(11650.845, fov);
  const fog_start = mood.fog_start * fcf,
    fog_end = Math.max(mood.fog_end * fcf, fog_start + 1);
  const pfog0 = (d * (mood.fog_min - mood.fog_max)) / (fog_end - fog_start);
  const pfog1 =
    (fog_end * mood.fog_max - fog_start * mood.fog_min) / (fog_end - fog_start);
  const k = (-0.5 * (16760631 - 100)) / (d * (f - d));
  const P = [0, 1, 2, 3].map(() => [0, 0, 0, 0]);
  P[0][0] = fcf * -(256 / (xr * d));
  P[1][1] = fcf * -(208 / (yr * d));
  P[2][2] = fcf * (f + d) * k;
  P[2][3] = (fcf / d) * pfog0;
  P[3][2] = -2 * k * f * d * fcf;
  // its frame (forward-down->inv-matrix): rvec to the left of the screen, uvec up, fvec forward
  const F = cam.forward();
  const R = normalize(cross(F, [0, -1, 0]));
  const U = normalize(cross(F, R));
  const T: Vec3 = [
    cam.pos[0] * K_UNITS,
    cam.pos[1] * K_UNITS,
    cam.pos[2] * K_UNITS,
  ];
  const rot = [0, 1, 2].map((r) => [R[r], U[r], F[r], 0]);
  rot.push([-dot(T, R), -dot(T, U), -dot(T, F), 1]);
  const hvdf_z = 0.5 * (16760631 + 100);
  const n = makeNewCamMat(rot, P, pfog0, hvdf_z);
  // tfrag3.vert: clip = -(n3 without w) - sum(n_i * v_i), v = position - trans (game units),
  // then y *= SCISSOR_ADJUST * HEIGHT_SCALE (Jak 3: 512 / 416 * 0.5)
  const ky = (512 / 416) * 0.5;
  const clip = new Float32Array(16);
  for (let row = 0; row < 4; row++) {
    const s = row === 1 ? ky : 1;
    let t = 0;
    for (let i = 0; i < 3; i++) {
      clip[i * 4 + row] = -K_UNITS * n[i][row] * s;
      t += n[i][row] * T[i];
    }
    clip[12 + row] = ((row === 3 ? 0 : -n[3][row]) + t) * s;
  }
  return {
    pc_camera: new Float32Array(n.flat()),
    hvdf_offset: [2048, 2048, hvdf_z, pfog1],
    cam_trans: [T[0], T[1], T[2], 1],
    fog: [pfog0, mood.fog_min, mood.fog_max],
    clip,
  };
}

// The palettes' weights the renderers take (update-mood-itimes: rgb times w, to fixed point, 64 is
// 1.0; interp_time_of_day reads the low byte of each): per palette, per channel
export function todWeights(mood: GameMood): Uint32Array {
  const out = new Uint32Array(32);
  for (let k = 0; k < 8; k++) {
    const v = mood.times[k];
    for (let c = 0; c < 4; c++) {
      const x = c < 3 ? v[c] * v[3] : v[3];
      out[k * 4 + c] = (Math.trunc(x * 4096) >> 6) & 0xff;
    }
  }
  return out;
}

// game_view_settings (app.cpp): the editor's draws over the game view, its models lit and fogged
// as the game does at that hour
function gameViewSettings(rs: RenderSettings, m: GameMood) {
  rs.game_view = true;
  rs.show_grid = false;
  rs.far_ocean = false;
  rs.game_light = true;
  const v3 = (v: number[]): Vec3 => [v[0], v[1], v[2]];
  rs.light_ambient = v3(m.light_ambient);
  rs.light_dir = m.light_dir.map(v3);
  rs.light_color = m.light_color.map(v3);
  rs.game_fog = [
    m.fog_color[0] / 255,
    m.fog_color[1] / 255,
    m.fog_color[2] / 255,
    (m.fog_max - m.fog_min) / 255,
  ];
  rs.game_fog_start = m.fog_start / 4096;
  rs.game_fog_end = m.fog_end / 4096;
}

// ocean_look (actions.cpp): the game's hour on the ocean, the sky it reflects, the sun (or the
// moon) glinting, as bright as its light
export function oceanLook(look: WaterLook, m: GameMood): WaterLook {
  let best = -1,
    strongest = 0;
  for (let i = 0; i < 3; i++) {
    const s = m.light_color[i][0] + m.light_color[i][1] + m.light_color[i][2];
    if (s > strongest) {
      strongest = s;
      best = i;
    }
  }
  const ambient =
    (m.light_ambient[0] + m.light_ambient[1] + m.light_ambient[2]) / 3;
  const k = clamp(ambient + strongest / 6, 0.15, 1.1);
  const v3 = (f: (c: number) => number) =>
    [f(0), f(1), f(2)] as [number, number, number];
  return {
    ...look,
    sky: v3((c) => m.sky_color[c] / 255),
    color: v3((c) => look.color[c] * k),
    deep: v3((c) => look.deep[c] * k),
    sun_dir: best >= 0 ? v3((c) => m.light_dir[best][c]) : look.sun_dir,
    sun_color: v3((c) =>
      best >= 0 ? Math.min(1, m.light_color[best][c] * 0.6) : 0,
    ),
  };
}

// ---------------------------------------------------------------------------------------------
// the game's TFragment shader (tfrag3.vert, tfrag3.frag), in GLSL ES 3.00. The time of the day is
// mixed here (interp_time_of_day: the vertex's 8 palettes times the mood's weights), not in a
// texture of mixed colors made on the CPU each frame.

const TFRAG_VS = `#version 300 es
precision highp float;
precision highp int;
layout(location = 0) in vec3 position_in;
layout(location = 1) in vec2 tex_coord_in;
layout(location = 2) in uint time_of_day_index;
uniform vec4 hvdf_offset;
uniform vec4 cam_trans;
uniform mat4 pc_camera;
uniform float fog_min;
uniform float fog_max;
// the palettes: the 8 RGBA of a color side by side, 128 colors a row; sampled in the vertex
// shader on purpose
uniform highp usampler2D tex_T10;
uniform uvec4 itimes[8];  // the palettes' weights (64 is 1.0)
out vec4 fragment_color;
out vec2 tex_coord;
out float fogginess;

void main() {
  // the camera transform
  vec3 vert = position_in - cam_trans.xyz;
  vec4 transformed = -pc_camera[3];
  transformed.w = 0.0;
  transformed -= pc_camera[0] * vert.x;
  transformed -= pc_camera[1] * vert.y;
  transformed -= pc_camera[2] * vert.z;

  // do fog!
  fogginess = 255.0 - clamp(-transformed.w + hvdf_offset.w, fog_min, fog_max);

  // scissoring area adjust
  transformed.y *= (512.0 / 416.0) * 0.5;
  gl_Position = transformed;

  // time of day lookup: the 8 palettes mixed (weights times 2^6), the alpha saturated to 128
  int c = int(time_of_day_index);
  ivec2 at = ivec2((c % 128) * 8, c / 128);
  uvec4 sum = uvec4(0u);
  for (int k = 0; k < 8; k++) sum += texelFetch(tex_T10, at + ivec2(k, 0), 0) * itimes[k];
  fragment_color = vec4(min(sum >> 6u, uvec4(255u, 255u, 255u, 128u))) / 255.0;
  // color adjustment
  fragment_color *= 2.0;
  fragment_color.a *= 2.0;

  tex_coord = tex_coord_in;
}
`;

const TFRAG_FS = `#version 300 es
precision highp float;
out vec4 color;
in vec4 fragment_color;
in vec2 tex_coord;
in float fogginess;
uniform sampler2D tex_T0;
uniform float tex_alpha;  // 0.5: a texture of the editor's, its alpha the PS2's (128 opaque)
uniform float alpha_min;
uniform float alpha_max;
uniform vec4 fog_color;

void main() {
  vec4 T0 = texture(tex_T0, tex_coord);
  T0.a *= tex_alpha;
  color = fragment_color * T0;
  if (color.a < alpha_min || color.a > alpha_max) {
    discard;
  }
  color.rgb = mix(color.rgb, fog_color.rgb, clamp(fogginess * fog_color.a, 0.0, 1.0));
}
`;

const PALETTE_ROW = 128; // colors per row of a palette texture

interface GpuChunk {
  info: ChunkInfo;
  center: Vec3;
  draws: ChunkDraw[];
  vao: WebGLVertexArrayObject;
  vbo: WebGLBuffer;
  ebo: WebGLBuffer;
}

class GameView {
  // drawn: View > Game rendering on, a document open (Viewport.svelte follows engine.state)
  on = false;
  mood: GameMood | null = null;
  // the game view's GL, made for the viewport's context
  private gl: WebGL2RenderingContext | null = null;
  private prog: Program | null = null;
  private white: WebGLTexture | null = null; // alpha 128: the PS2's opaque
  private failed = false;
  private chunks = new Map<number, GpuChunk>(); // by id
  private palettes = new Map<number, WebGLTexture>(); // by key
  private cam: GameCamera | null = null;
  private busy: Promise<void> | null = null;
  private again = false;
  private stop_animation: (() => void) | null = null;
  // a drag's pieces: left out of the chunks (the call game_view_filter), drawn by the editor's
  // renderer where they go until the chunks come back with them (the C++ made the moved chunks
  // again at once)
  private dragged = new Set<number>();
  private drag_key = "";
  // a capture (viewport.capture): the game view at its hour, or none (on false)
  private shot: { on: boolean; mood: GameMood | null } | null = null;

  // its passes in the viewport's frame and the engine's changes; returns what stops them
  attach(): () => void {
    const offs = [
      viewport.addPass("opaque", (r, rs) => this.drawOpaque(r, rs), {
        capture: true,
      }),
      viewport.addPass("translucent", (r) => this.drawTranslucent(r), {
        capture: true,
      }),
      viewport.addPass("overlay", (r, rs) => this.drawRain(r, rs), {
        capture: true,
      }),
    ];
    viewport.waterLook = (look, rs) =>
      rs.game_view && this.moodShown()
        ? oceanLook(look, this.moodShown()!)
        : look;
    const hook = (hour: number | undefined, only: number[] | null) =>
      hour === undefined ? Promise.resolve() : this.prepareCapture(hour, only);
    viewport.captureHooks.push(hook);
    offs.push(viewport.store.subscribe(() => this.followDrag()));
    let live = true;
    void engineReady.then(() => {
      if (!live) return;
      offs.push(
        onEngine("jle:changed", (p: any) => {
          // another document: the old one's chunks not drawn meanwhile (their textures are gone)
          if (p?.document) this.dropChunks();
          if (!this.on) return;
          if (!p || p.gameview || p.scene || p.document || p.materials)
            void this.refresh();
          if (!p || p.gameview || p.state || p.document)
            void this.refreshMood();
        }),
      );
    });
    return () => {
      live = false;
      offs.forEach((off) => off());
      viewport.waterLook = null;
      viewport.captureHooks = viewport.captureHooks.filter((h) => h !== hook);
      this.setOn(false);
    };
  }

  setOn(on: boolean) {
    if (on === this.on) return;
    this.on = on;
    // the clouds drift: the view draws every frame (the C++ editor's did anyway)
    this.stop_animation?.();
    this.stop_animation = on ? viewport.animate() : null;
    if (on) {
      void this.refreshMood();
      void this.refresh();
    }
    viewport.requestRedraw();
  }

  async refreshMood() {
    const a = await call("game_mood").catch((e) =>
      console.warn("[game view] game_mood:", e),
    );
    const m = a?.times ? a : a?.mood; // the mood, or {mood, ...}
    if (m?.times) {
      this.mood = m as GameMood;
      viewport.requestRedraw();
    }
  }

  // A drag started or ended (the store's preview): its pieces left out of the chunks while it
  // lasts; once it ended, drawn by the editor until the chunks have them where they went.
  private followDrag() {
    const p = viewport.store.preview;
    const key = p.size ? `${p.size}:${p.keys().next().value}` : "";
    if (key === this.drag_key) return;
    this.drag_key = key;
    if (!this.on && !this.dragged.size) return;
    if (key) {
      this.dragged = new Set(p.keys());
      void call("game_view_filter", { hide: [...this.dragged] }).catch(
        () => {},
      );
    } else {
      const done = this.dragged;
      void call("game_view_filter", {})
        .then(() => this.refresh())
        .catch(() => {})
        .finally(() => {
          if (this.dragged === done) this.dragged = new Set();
          viewport.requestRedraw();
        });
    }
  }

  // The game view's part of a capture at that hour (-1: the editor's view) of some elements alone
  // (only: null all): its mood and its chunks read first; returns what puts it back.
  private async prepareCapture(
    hour: number,
    only: number[] | null,
  ): Promise<() => void> {
    if (hour < 0) {
      this.shot = { on: false, mood: null };
      return () => (this.shot = null);
    }
    const r = viewport.renderer;
    const m = await call("game_mood", { hour }).catch(() => null);
    const mood = (m?.times ? m : m?.mood) as GameMood | undefined;
    if (!r || !mood?.times || !this.ready(r.gl)) {
      this.shot = { on: false, mood: null };
      return () => (this.shot = null);
    }
    if (only) await call("game_view_filter", { only }).catch(() => {});
    await this.refresh();
    this.shot = { on: true, mood };
    return () => {
      this.shot = null;
      if (!only) return;
      // what the drag left out, else everything again
      const back = this.dragged.size ? { hide: [...this.dragged] } : {};
      void call("game_view_filter", back)
        .then(() => (this.on ? this.refresh() : undefined))
        .catch(() => {});
    };
  }

  // the mood drawn: a capture's while it is taken
  private moodShown(): GameMood | null {
    return viewport.capturing && this.shot ? this.shot.mood : this.mood;
  }

  // The chunks again: those not had (by id and version) and their palettes read, those gone
  // freed. One read at a time; what changes meanwhile is read right after (the promise ends
  // once it is).
  refresh(): Promise<void> {
    if (this.busy) {
      this.again = true;
      return this.busy;
    }
    this.busy = this.read().finally(() => (this.busy = null));
    return this.busy;
  }

  private async read() {
    try {
      do {
        this.again = false;
        const gl = this.gl;
        if (!gl) return; // read when first drawn
        const list = parseChunkList(await fetchBytes("gameview"));
        const keys = new Set(list.map((c) => c.palette));
        await Promise.all([
          ...list
            .filter((c) => this.chunks.get(c.id)?.info.version !== c.version)
            .map((c) => this.readChunk(gl, c.id)),
          ...[...keys]
            .filter((k) => !this.palettes.has(k))
            .map((k) => this.readPalette(gl, k)),
        ]);
        if (gl !== this.gl) return;
        const ids = new Set(list.map((c) => c.id));
        for (const [id, c] of this.chunks)
          if (!ids.has(id)) {
            this.freeChunk(c);
            this.chunks.delete(id);
          }
        for (const [k, t] of this.palettes)
          if (!keys.has(k)) {
            gl.deleteTexture(t);
            this.palettes.delete(k);
          }
        viewport.requestRedraw();
      } while (this.again);
    } catch (e) {
      console.warn("[game view]", e);
    }
  }

  private async readChunk(gl: WebGL2RenderingContext, id: number) {
    let d: ChunkData;
    try {
      d = parseChunk(await fetchBytes(`gameview/chunk/${id}`));
    } catch (e) {
      return console.warn(`[game view] chunk ${id}:`, e); // made again meanwhile: read next time
    }
    if (gl !== this.gl) return;
    const old = this.chunks.get(id);
    if (old) this.freeChunk(old);
    gl.bindVertexArray(null);
    const vao = gl.createVertexArray()!,
      vbo = gl.createBuffer()!,
      ebo = gl.createBuffer()!;
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.bufferData(gl.ARRAY_BUFFER, d.vertices, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 24, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 24, 12);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribIPointer(2, 1, gl.UNSIGNED_SHORT, 24, 20);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ebo);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, d.indices, gl.STATIC_DRAW);
    gl.bindVertexArray(null);
    this.chunks.set(id, {
      info: d,
      center: [
        (d.lo[0] + d.hi[0]) / 2,
        (d.lo[1] + d.hi[1]) / 2,
        (d.lo[2] + d.hi[2]) / 2,
      ],
      draws: d.draws,
      vao,
      vbo,
      ebo,
    });
  }

  private async readPalette(gl: WebGL2RenderingContext, key: number) {
    let p: { colors: number; rgba: Uint8Array };
    try {
      p = parsePalette(await fetchBytes(`gameview/palette/${key}`));
    } catch (e) {
      return console.warn(`[game view] palette ${key}:`, e);
    }
    if (gl !== this.gl) return;
    const rows = Math.max(1, Math.ceil(p.colors / PALETTE_ROW));
    const data = new Uint8Array(PALETTE_ROW * 32 * rows);
    data.set(p.rgba);
    const tex = gl.createTexture()!;
    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA8UI,
      PALETTE_ROW * 8,
      rows,
      0,
      gl.RGBA_INTEGER,
      gl.UNSIGNED_BYTE,
      data,
    );
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.activeTexture(gl.TEXTURE0);
    this.palettes.get(key) && gl.deleteTexture(this.palettes.get(key)!);
    this.palettes.set(key, tex);
  }

  private dropChunks() {
    const gl = this.gl;
    if (!gl) return;
    for (const c of this.chunks.values()) this.freeChunk(c);
    for (const t of this.palettes.values()) gl.deleteTexture(t);
    this.chunks.clear();
    this.palettes.clear();
    viewport.requestRedraw();
  }

  private freeChunk(c: GpuChunk) {
    const gl = this.gl!;
    gl.deleteVertexArray(c.vao);
    gl.deleteBuffer(c.vbo);
    gl.deleteBuffer(c.ebo);
  }

  // the GL of the viewport's context (another context: everything read again)
  private ready(gl: WebGL2RenderingContext): boolean {
    if (gl === this.gl) return !!this.prog;
    this.gl = gl;
    this.chunks.clear();
    this.palettes.clear();
    this.prog = null;
    if (this.failed) return false;
    try {
      this.prog = program(gl, TFRAG_VS, TFRAG_FS);
    } catch (e) {
      this.failed = true;
      console.error("[game view] the game's shader:", e);
      return false;
    }
    this.white = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.white);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA8,
      1,
      1,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      new Uint8Array([255, 255, 255, 128]),
    );
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    void this.refresh();
    return true;
  }

  private active(r: Renderer): boolean {
    const on = viewport.capturing && this.shot ? this.shot.on : this.on;
    return on && !!this.moodShown() && this.ready(r.gl);
  }

  // The decor's opaque and masked surfaces, after clearing the view to the sky and its depth the
  // game's way (0: far); what the editor draws next goes over it with the game's camera and depth.
  private drawOpaque(r: Renderer, rs: RenderSettings) {
    r.editorDrawn = this.dragged;
    if (!this.active(r)) return;
    const gl = r.gl,
      m = this.moodShown()!;
    const vp = gl.getParameter(gl.VIEWPORT) as Int32Array;
    const aspect = vp[2] / Math.max(vp[3], 1);
    this.cam = gameCamera(viewport.camera, aspect, m);
    gameViewSettings(rs, m);
    // the sky behind, the depth the game's way (0 far)
    gl.clearColor(
      m.sky_color[0] / 255,
      m.sky_color[1] / 255,
      m.sky_color[2] / 255,
      1,
    );
    gl.clearDepth(0);
    gl.depthMask(true);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.clearDepth(1);
    // its sun, its clouds, the stars at night
    drawSky(gl, m, viewport.camera, aspect, rs.time);
    this.render(gl, r, [...this.chunks.values()], false);
    r.setGameDepth(this.cam.clip);
  }

  // then its translucent surfaces (water, glass, glows), after the editor's opaque models: the
  // farthest first
  private drawTranslucent(r: Renderer) {
    if (!this.active(r) || !this.cam) return;
    const eye = viewport.camera.pos;
    const order = [...this.chunks.values()]
      .filter((c) => c.draws.some((d) => d.flags & DrawFlag.Translucent))
      .sort(
        (a, b) => length2(sub(b.center, eye)) - length2(sub(a.center, eye)),
      );
    this.render(r.gl, r, order, true);
  }

  // The rain (draw_rain, game_window.cpp): streaks falling around the camera (the game's rain
  // particles: launched around it, a few meters long, lit by the mood), as many as it is strong
  private drawRain(r: Renderer, rs: RenderSettings) {
    const m = this.moodShown();
    const rain = engine.state.weather?.mood?.rain ?? 0;
    if (!this.active(r) || !m || rain <= 0) return;
    const a = m.light_ambient;
    const light = clamp((a[0] + a[1] + a[2]) / 3, 0.2, 1);
    const color = packRgba(
      0.75 * light + m.flash,
      0.8 * light + m.flash,
      0.9 * light + m.flash,
      0.35,
    );
    const count = Math.trunc(2500 * rain);
    const box = 24,
      height = 18,
      speed = 16,
      len = 0.7;
    const c = viewport.camera.pos;
    // the streaks stay put in the world as the camera moves (their cell follows it)
    const cx = Math.floor(c[0] / (2 * box)) * 2 * box,
      cz = Math.floor(c[2] / (2 * box)) * 2 * box;
    for (let i = 0; i < count; i++) {
      const x = (hash01(i * 3 + 1) - 0.5) * 2 * box,
        z = (hash01(i * 3 + 2) - 0.5) * 2 * box;
      const fall = (rs.time * speed + hash01(i * 3 + 3) * height) % height;
      const q: Vec3 = [cx + x, c[1] + height * 0.5 - fall, cz + z];
      if (q[0] - c[0] > box) q[0] -= 2 * box;
      if (q[0] - c[0] < -box) q[0] += 2 * box;
      if (q[2] - c[2] > box) q[2] -= 2 * box;
      if (q[2] - c[2] < -box) q[2] += 2 * box;
      r.line(q, [q[0] + 0.05, q[1] - len, q[2] + 0.02], color);
    }
    r.flush(true);
  }

  private render(
    gl: WebGL2RenderingContext,
    r: Renderer,
    order: GpuChunk[],
    translucent: boolean,
  ) {
    const p = this.prog!,
      u = p.u,
      c = this.cam!,
      m = this.moodShown()!;
    // first_tfrag_draw_setup
    gl.useProgram(p.p);
    gl.uniform1i(u.tex_T0, 0);
    gl.uniform1i(u.tex_T10, 2);
    gl.uniformMatrix4fv(u.pc_camera, false, c.pc_camera);
    gl.uniform4fv(u.hvdf_offset, c.hvdf_offset);
    gl.uniform4fv(u.cam_trans, c.cam_trans);
    gl.uniform1f(u.fog_min, c.fog[1]);
    gl.uniform1f(u.fog_max, c.fog[2]);
    // the fog's color as the renderer state has it (bytes), its intensity 1
    const fc = (i: number) => Math.trunc(m.fog_color[i]) / 255;
    gl.uniform4f(u.fog_color, fc(0), fc(1), fc(2), 1 / 255);
    gl.uniform4uiv(u.itimes, todWeights(m));
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.GEQUAL);
    gl.disable(gl.CULL_FACE);
    const want = translucent ? DrawFlag.Translucent : 0;
    for (const ch of order) {
      const pal = this.palettes.get(ch.info.palette);
      if (!pal) continue;
      gl.activeTexture(gl.TEXTURE2);
      gl.bindTexture(gl.TEXTURE_2D, pal);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindVertexArray(ch.vao);
      for (const d of ch.draws) {
        if ((d.flags & DrawFlag.Translucent) !== want) continue;
        gl.bindTexture(
          gl.TEXTURE_2D,
          d.texture >= 0 ? r.glTexture(viewport.store, d.texture) : this.white,
        );
        this.drawMode(gl, d);
        gl.uniform1f(u.tex_alpha, d.flags & DrawFlag.OwnTexture ? 0.5 : 1);
        gl.drawElements(gl.TRIANGLES, d.count, gl.UNSIGNED_INT, d.first * 4);
        r.stats.draws++;
        r.stats.triangles += d.count / 3;
      }
    }
    // the state the editor's renderer expects back
    gl.bindVertexArray(null);
    gl.disable(gl.BLEND);
    gl.blendEquation(gl.FUNC_ADD);
    gl.depthMask(true);
    gl.activeTexture(gl.TEXTURE0);
  }

  // setup_opengl_from_draw_mode, setup_tfrag_shader (background_common.cpp): the depth test is
  // always GEQUAL, the blend and the alpha test the draw's
  private drawMode(gl: WebGL2RenderingContext, d: ChunkDraw) {
    const u = this.prog!.u;
    gl.enable(gl.BLEND);
    switch (d.blend) {
      case AlphaBlend.SrcDstSrcDst:
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFuncSeparate(
          gl.SRC_ALPHA,
          gl.ONE_MINUS_SRC_ALPHA,
          gl.ONE,
          gl.ZERO,
        );
        break;
      case AlphaBlend.Src0SrcDst:
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE, gl.ONE, gl.ZERO);
        break;
      case AlphaBlend.Src0FixDst:
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFuncSeparate(gl.ONE, gl.ONE, gl.ONE, gl.ZERO);
        break;
      case AlphaBlend.SrcDstFixDst:
        // Cv = (Cs - Cd) * FIX + Cd: Cs * FIX * 0.5 + Cd * FIX * 0.5
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFuncSeparate(
          gl.CONSTANT_COLOR,
          gl.CONSTANT_COLOR,
          gl.ONE,
          gl.ZERO,
        );
        gl.blendColor(0.5, 0.5, 0.5, 0.5);
        break;
      case AlphaBlend.ZeroSrcSrcDst:
        gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE, gl.ONE, gl.ZERO);
        gl.blendEquation(gl.FUNC_REVERSE_SUBTRACT);
        break;
      default:
        gl.disable(gl.BLEND);
    }
    gl.depthMask((d.flags & DrawFlag.DepthWrite) !== 0);
    gl.uniform1f(u.alpha_min, d.flags & DrawFlag.AlphaTest ? d.aref / 127 : 0);
    gl.uniform1f(u.alpha_max, 10);
  }
}

// a number of 0..1 for an integer (game_window.cpp hash01)
function hash01(x: number): number {
  x >>>= 0;
  x = (x ^ (x >>> 16)) >>> 0;
  x = Math.imul(x, 0x7feb352d) >>> 0;
  x = (x ^ (x >>> 15)) >>> 0;
  x = Math.imul(x, 0x846ca68b) >>> 0;
  x = (x ^ (x >>> 16)) >>> 0;
  return (x & 0xffffff) / 0x1000000;
}

export const gameView = new GameView();
