// The particle effects of Jak 3 (steam, sparks, smoke, fire, lights) in the view (src/scene/particles
// .{h,cpp}, crates/core/src/scene/particles.rs: the same rules, the Rust one is the reference its
// tests prove). The game's definitions (data/jak3/particles.json) run by its rules
// (sparticle-launcher.gc, sparticle.gc): a group launches the particles of its parts over time
// (continuously, in bursts every period, or once); a particle starts from its launcher's fields (a
// value and a random range each: its position, velocity, size, rotation, color and how they change,
// its life) and moves 60 times a second: velocities are per tick (300 a second, 5 a frame),
// friction and acceleration per frame. At a time of its life it can take the fields of another
// launcher (next-launcher: steam that fades out).
//
// The view runs the effects of the document (the actors naming a group by their art-name: the
// call `effect_emitters`) and draws them as sprites facing the camera with the game's textures,
// after the scene and the ocean, without writing the depth (Renderer::draw_particles), lost in the
// game's fog far away when the game view is on (App::fog_effects).

import { call, engineReady, fetchBytes, onEngine } from "./engine";
import {
  aabb,
  aabbAdd,
  axisAngle,
  dir,
  length,
  matMul,
  normalize,
  point,
  rotate,
  type AABB,
  type Mat4,
  type Vec3,
  type Vec4,
} from "./math";
import type { Renderer } from "./renderer";
import type { RenderSettings } from "./types";
import { viewport } from "./viewport_api";

// the files the engine serves (asset?path=, docs/rust-architecture.md §4)
const PARTICLES_PATH = "data/jak3/particles.json";
const SPRITES_PATH = "library/jak3/common/sprites"; // <page>/<texture>.png, index.json
const asset = (path: string) =>
  fetchBytes(`asset?path=${encodeURIComponent(path)}`);

// sp-field-id (the ones the editor runs)
const SP = {
  Num: 6,
  X: 10,
  Y: 11,
  Z: 12,
  ScaleX: 13,
  RotX: 14,
  RotZ: 16,
  ScaleY: 17,
  R: 18,
  A: 21,
  VelX: 25,
  VelY: 26,
  VelZ: 27,
  ScaleVelX: 28,
  RotVelZ: 31,
  ScaleVelY: 32,
  FadeR: 33,
  FadeA: 36,
  AccelX: 37,
  AccelY: 38,
  AccelZ: 39,
  Friction: 45,
  Timer: 46,
  NextTime: 50,
  NextLauncher: 51,
  ConeRotX: 58,
  ConeRotY: 59,
  ConeRotZ: 60,
  RotateX: 62,
  RotateY: 63,
  RotateZ: 64,
  ConeRotRadius: 65,
  Count: 72,
} as const;

const Kind = {
  Float: 0,
  FloatIntRand: 1,
  Int: 2,
  Copy: 3,
  Next: 4,
  Flags: 5,
  Texture: 6,
} as const;

// an init spec of a launcher: how a field starts
export interface SpSpec {
  kind: number;
  field: number;
  a: number; // the value, its random range, its random multiplier
  b: number;
  c: number;
  ref: number; // Copy: the field copied; Next: the launcher; Texture: its index
  flags: number; // Flags: sp-cpuinfo-flag bits
}

// sp-cpuinfo-flag bits the editor uses
const kSpGlow = 1 << 15; // drawn by the glow renderer: added to what is behind
const kSpGlobalAcc = 1 << 16; // its acceleration in the world (not turned with the effect)

// a part of a group: a launcher and when it launches (sp-group-item)
export interface SpItem {
  launcher: number;
  flags: number; // is-3d sp1..sp15: bit 3 once, at the start
  period: number; // ticks: launching length ticks every period (0: always)
  length: number;
  offset: number;
  fade_after: number; // meters: not launched beyond, fewer up to
  falloff_to: number;
  hour_mask: number; // not launched at these hours (bit h)
}

export interface SpGroup {
  name: string;
  flags: number; // sp-group-flag: bit 4, the effect turned with its actor
  radius: number; // meters: its bounds
  items: SpItem[];
}

const CPUINFO_BITS: Record<string, number> = {
  "ready-to-launch": 6,
  distort: 7,
  "aux-list": 8,
  level0: 10,
  level1: 11,
  glow: 15,
  "use-global-acc": 16,
  "launch-along-z": 17,
  "left-multiply-quat": 18,
  "right-multiply-quat": 19,
  "set-conerot": 20,
};
const bit = (n: number) => (n >= 0 && n < 32 ? (1 << n) >>> 0 : 0);
// sp-cpuinfo-flag
function cpuinfoFlag(name: string): number {
  if (name in CPUINFO_BITS) return bit(CPUINFO_BITS[name]);
  if (name.startsWith("sp-cpuinfo-flag-"))
    return bit(parseInt(name.slice(16)) || 0);
  return 0;
}
// sp1, sp7, is-3d...: their bit
function spBits(names: unknown): number {
  let out = 0;
  if (!Array.isArray(names)) return out;
  for (const s of names) {
    if (typeof s !== "string") continue;
    if (s === "is-3d") out |= 1;
    else if (s.length > 2 && s.startsWith("sp"))
      out |= bit(parseInt(s.slice(2)) || 0) & 0xffff;
  }
  return out;
}
const num = (v: unknown) => (typeof v === "number" ? v : 0);

export class ParticleData {
  private groups = new Map<string, SpGroup>();
  private launchers = new Map<number, SpSpec[]>();
  // the textures the launchers draw: "<tpage>/<name>", or "<page>:<index>"
  readonly textures: string[] = [];

  // data/jak3/particles.json's contents; null when it is not that
  static fromJson(j: any): ParticleData | null {
    if (!j || typeof j !== "object" || !j.groups || !j.launchers) return null;
    const out = new ParticleData();
    const textureIndex = new Map<string, number>();
    for (const [id, specs] of Object.entries<any>(j.launchers)) {
      const list: SpSpec[] = [];
      out.launchers.set(parseInt(id) || 0, list);
      for (const s of Array.isArray(specs) ? specs : []) {
        if (
          !Array.isArray(s) ||
          s.length !== 5 ||
          typeof s[0] !== "number" ||
          typeof s[1] !== "string"
        )
          continue;
        const field = Math.trunc(s[0]);
        if (field < 0 || field >= SP.Count) continue;
        const spec: SpSpec = {
          kind: -1,
          field,
          a: 0,
          b: 0,
          c: 0,
          ref: 0,
          flags: 0,
        };
        const kind = s[1];
        if (kind === "f" || kind === "fi" || kind === "i") {
          spec.kind =
            kind === "f"
              ? Kind.Float
              : kind === "fi"
                ? Kind.FloatIntRand
                : Kind.Int;
          spec.a = num(s[2]);
          spec.b = num(s[3]);
          spec.c = num(s[4]);
        } else if (
          (kind === "copy" || kind === "next") &&
          typeof s[3] === "number"
        ) {
          spec.kind = kind === "copy" ? Kind.Copy : Kind.Next;
          spec.ref = Math.trunc(s[3]);
        } else if (kind === "flags" && Array.isArray(s[3])) {
          spec.kind = Kind.Flags;
          for (const n of s[3])
            if (typeof n === "string")
              spec.flags = (spec.flags | cpuinfoFlag(n)) >>> 0;
        } else if (kind === "tex" && typeof s[3] === "string") {
          spec.kind = Kind.Texture;
          let t = textureIndex.get(s[3]);
          if (t === undefined) {
            t = out.textures.length;
            textureIndex.set(s[3], t);
            out.textures.push(s[3]);
          }
          spec.ref = t;
        } else {
          continue; // symbols (the game's functions): not run
        }
        list.push(spec);
      }
    }
    for (const [name, g] of Object.entries<any>(j.groups)) {
      const group: SpGroup = {
        name,
        flags: spBits(g?.flags),
        radius: 0,
        items: [],
      };
      if (Array.isArray(g?.bounds) && g.bounds.length === 4)
        group.radius = num(g.bounds[3]);
      for (const p of Array.isArray(g?.parts) ? g.parts : []) {
        if (!Array.isArray(p) || p.length !== 9 || typeof p[0] !== "number")
          continue;
        group.items.push({
          launcher: Math.trunc(p[0]),
          flags: spBits(p[1]),
          period: Math.trunc(num(p[2])),
          length: Math.trunc(num(p[3])),
          offset: Math.trunc(num(p[4])),
          fade_after: num(p[5]),
          falloff_to: num(p[6]),
          hour_mask: num(p[8]) >>> 0,
        });
      }
      out.groups.set(name, group);
    }
    return out;
  }

  get empty(): boolean {
    return this.groups.size === 0;
  }
  // an art-name ("group-...", quoted or not)
  group(name: string): SpGroup | null {
    return this.groups.get(name.startsWith("'") ? name.slice(1) : name) ?? null;
  }
  launcher(id: number): SpSpec[] | null {
    return this.launchers.get(id) ?? null;
  }
}

let data_promise: Promise<ParticleData | null> | null = null;
// the game's effects, read once from the engine (null when it has none)
export function loadParticleData(): Promise<ParticleData | null> {
  data_promise ??= engineReady
    .then(() => asset(PARTICLES_PATH))
    .then((b) => ParticleData.fromJson(JSON.parse(new TextDecoder().decode(b))))
    .catch((e) => {
      console.warn("[effects]", PARTICLES_PATH, e);
      return null;
    });
  return data_promise;
}

// ---------------------------------------------------------------------------------------------
// the simulation

// a particle as drawn: a sprite facing the camera
export interface ParticleSprite {
  pos: Vec3; // meters
  sx: number; // meters
  sy: number;
  rot: number; // radians
  color: Vec4; // 1 is the texture as it is (the game's 128)
  texture: number; // ParticleData.textures, -1 none
  additive: boolean;
  from_left: boolean; // the sprite starts at its position (the game's flag 4: streaks), else centered
}

interface Particle {
  f: Float32Array; // the fields (sp-field-id)
  pos: Vec3; // game units, per tick, per frame
  vel: Vec3;
  acc: Vec3;
  next_time: number; // ticks of its life the next launcher takes over
  age: number; // ticks
  texture: number;
  flags: number;
}

interface Emitter {
  group: SpGroup;
  world: Mat4;
  accum: number[]; // per item: the particles started but for a fraction
  launched: boolean[]; // per item: launched once (bit 3)
  clock: number; // ticks
  seen: boolean;
}

// an angle of the game (65536 a turn) as radians
const angle = (units: number) => units * ((2 * Math.PI) / 65536);

function rotation(rx: number, ry: number, rz: number, cone: boolean): Mat4 {
  const x = rotate(axisAngle([1, 0, 0], angle(rx)));
  const y = rotate(axisAngle([0, 1, 0], angle(ry)));
  const z = rotate(axisAngle([0, 0, 1], angle(rz)));
  // the cone turns the launch away from its axis (z, then x and y), the rotation turns that around
  return cone ? matMul(matMul(z, x), y) : matMul(matMul(y, x), z);
}

const kTicks = 5; // a frame at 60 a second

export class ParticleSystem {
  maxParticles = 20000;
  maxDistance = 150; // meters
  private emitters = new Map<number, Emitter>();
  private particles: Particle[] = [];
  private time = 0; // not stepped yet
  private seed = 12345;

  // 0..1 (mulberry32: the same picture of an effect each time)
  private random(): number {
    let t = (this.seed = (this.seed + 0x6d2b79f5) | 0);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  }

  // The effects to run this frame: each by an id of the caller (an actor), its group and where it
  // is; the effects not given again stop launching and go once their particles are gone
  beginFrame() {
    for (const e of this.emitters.values()) e.seen = false;
  }
  emit(id: number, group: SpGroup, world: Mat4) {
    let e = this.emitters.get(id);
    if (!e || e.group !== group) {
      e = {
        group,
        world,
        accum: group.items.map(() => 0),
        launched: group.items.map(() => false),
        clock: 0,
        seen: true,
      };
      this.emitters.set(id, e);
    }
    e.world = world;
    e.seen = true;
  }
  // the time passed (seconds: 60 steps a second, at most 10 a call), the camera (the effects far
  // from it do not launch), the hour (the hour-mask of some parts; < 0: every part, any hour)
  update(data: ParticleData, dt: number, camera: Vec3, hour = 12) {
    for (const [id, e] of this.emitters) if (!e.seen) this.emitters.delete(id);
    this.time = Math.min(this.time + dt, 10 / 60);
    while (this.time >= 1 / 60) {
      this.time -= 1 / 60;
      this.step(data, camera, hour);
    }
  }
  clear() {
    this.emitters.clear();
    this.particles = [];
    this.time = 0;
  }
  get count(): number {
    return this.particles.length;
  }

  // sp-init-fields!: each field from its spec, the ones not given at 0 (a new particle) or as they
  // are (a launcher taking over)
  private init(p: Particle, specs: SpSpec[], relaunch: boolean) {
    const f = p.f;
    if (!relaunch) {
      f.fill(0);
      f[SP.Friction] = 1; // none given: it keeps its speed
      f[SP.Timer] = -1;
    }
    for (const s of specs) {
      switch (s.kind) {
        case Kind.Float:
          f[s.field] = s.b !== 0 ? s.a + s.c * this.random() * s.b : s.a;
          break;
        case Kind.FloatIntRand:
          f[s.field] =
            s.b !== 0 ? s.a + s.c * Math.floor(this.random() * (s.b + 1)) : s.a;
          break;
        case Kind.Int:
          f[s.field] =
            s.b !== 0 ? s.a + s.c * Math.floor(this.random() * s.b) : s.a;
          break;
        case Kind.Copy:
          if (s.ref >= 0 && s.ref < SP.Count) f[s.field] = f[s.ref];
          break;
        case Kind.Next:
          f[s.field] = s.ref;
          break;
        case Kind.Flags:
          p.flags = s.flags;
          break;
        case Kind.Texture:
          p.texture = s.ref;
          break;
      }
    }
  }

  private launch(e: Emitter, specs: SpSpec[], rate: number, i: number) {
    // how many: num a launch (a fraction carried to the next)
    let count = 1;
    for (const s of specs)
      if (
        s.field === SP.Num &&
        (s.kind === Kind.Float ||
          s.kind === Kind.Int ||
          s.kind === Kind.FloatIntRand)
      )
        count = s.b !== 0 ? s.a + s.c * this.random() * s.b : s.a;
    e.accum[i] += count * rate;
    let n = Math.floor(e.accum[i]);
    e.accum[i] -= n;
    const turned = (e.group.flags & (1 << 4)) !== 0; // sp4: the effect turns with its actor
    const w = e.world;
    const origin: Vec3 = [w[12] * 4096, w[13] * 4096, w[14] * 4096];
    const turn = (v: Vec3): Vec3 => {
      const d = normalize(dir(w, v)),
        l = length(v);
      return [d[0] * l, d[1] * l, d[2] * l];
    };
    for (; n > 0 && this.particles.length < this.maxParticles; n--) {
      const p: Particle = {
        f: new Float32Array(SP.Count),
        pos: [0, 0, 0],
        vel: [0, 0, 0],
        acc: [0, 0, 0],
        next_time: -1,
        age: 0,
        texture: -1,
        flags: 0,
      };
      this.init(p, specs, false);
      const f = p.f;
      // launched along its cone and turned (the launcher's), then turned with the effect
      const r = matMul(
        rotation(f[SP.RotateX], f[SP.RotateY], f[SP.RotateZ], false),
        rotation(f[SP.ConeRotX], f[SP.ConeRotY], f[SP.ConeRotZ], true),
      );
      let pos = dir(r, [f[SP.X], f[SP.Y], f[SP.Z] + f[SP.ConeRotRadius]]);
      let vel = dir(r, [f[SP.VelX], f[SP.VelY], f[SP.VelZ]]);
      let acc: Vec3 = [f[SP.AccelX], f[SP.AccelY], f[SP.AccelZ]];
      if (turned) {
        pos = turn(pos);
        vel = turn(vel);
        if (!(p.flags & kSpGlobalAcc)) acc = turn(acc);
      }
      p.pos = [origin[0] + pos[0], origin[1] + pos[1], origin[2] + pos[2]];
      p.vel = vel;
      p.acc = acc;
      p.next_time = f[SP.NextLauncher] !== 0 ? Math.trunc(f[SP.NextTime]) : -1;
      this.particles.push(p);
    }
  }

  private step(data: ParticleData, camera: Vec3, hour: number) {
    // the effects launch (sparticle-launch-control spawn)
    const hour_bit =
      hour < 0 ? 0 : bit(Math.min(Math.max(Math.trunc(hour), 0), 23));
    for (const e of this.emitters.values()) {
      if (!e.seen) continue;
      const o = point(e.world, [0, 0, 0]);
      const dist = length([
        o[0] - camera[0],
        o[1] - camera[1],
        o[2] - camera[2],
      ]);
      if (dist > this.maxDistance) continue;
      e.group.items.forEach((item, i) => {
        const specs = data.launcher(item.launcher);
        if (!specs) return;
        const rate = item.falloff_to > 0 ? 1 - dist / item.falloff_to : 1;
        if (rate <= 0) return;
        if (item.flags & (1 << 3)) {
          // sp3: once
          if (!e.launched[i]) {
            e.launched[i] = true;
            this.launch(e, specs, rate, i);
          }
          return;
        }
        if (
          item.hour_mask & hour_bit ||
          (item.fade_after > 0 && dist > item.fade_after)
        )
          return;
        if (item.period > 0) {
          const t =
            (((e.clock - item.offset) % item.period) + item.period) %
            item.period;
          if (t >= item.length) return;
        }
        this.launch(e, specs, rate, i);
      });
      e.clock += kTicks;
    }
    // the particles move (sp-process-block): their velocity per tick, their friction and
    // acceleration per frame
    let alive = 0;
    const list = this.particles;
    for (let i = 0; i < list.length; i++) {
      const p = list[i],
        f = p.f;
      // freed once its timer ran out the frame before (a particle of one frame is drawn once)
      if (f[SP.Timer] !== -1) {
        if (f[SP.Timer] <= 0) continue;
        f[SP.Timer] = Math.max(0, f[SP.Timer] - kTicks);
      }
      p.age += kTicks;
      if (p.next_time >= 0 && p.age >= p.next_time) {
        // another launcher takes over (sp-relaunch-setup-fields): its fields over these
        const next = data.launcher(Math.trunc(f[SP.NextLauncher]));
        p.next_time = -1;
        if (next) {
          const friction = f[SP.Friction];
          f[SP.NextLauncher] = 0;
          this.init(p, next, true);
          if (f[SP.Friction] === 0) f[SP.Friction] = friction;
          for (const s of next) {
            if (
              s.field === SP.VelX ||
              s.field === SP.VelY ||
              s.field === SP.VelZ
            )
              p.vel = [f[SP.VelX], f[SP.VelY], f[SP.VelZ]];
            if (
              s.field === SP.AccelX ||
              s.field === SP.AccelY ||
              s.field === SP.AccelZ
            )
              p.acc = [f[SP.AccelX], f[SP.AccelY], f[SP.AccelZ]];
          }
          if (f[SP.NextLauncher] !== 0)
            p.next_time = p.age + Math.trunc(f[SP.NextTime]);
        }
      }
      const fr = f[SP.Friction];
      for (let k = 0; k < 3; k++) {
        p.vel[k] = (p.vel[k] + p.acc[k]) * fr;
        p.pos[k] += p.vel[k] * kTicks;
      }
      f[SP.ScaleX] += f[SP.ScaleVelX] * kTicks;
      f[SP.ScaleY] += f[SP.ScaleVelY] * kTicks;
      f[SP.RotZ] += f[SP.RotVelZ] * kTicks;
      for (let c = 0; c < 4; c++)
        f[SP.R + c] = Math.max(0, f[SP.R + c] + f[SP.FadeR + c] * kTicks);
      if (f[SP.A] <= 0 && f[SP.FadeA] <= 0 && p.age > kTicks) continue; // faded out
      list[alive++] = p;
    }
    list.length = alive;
  }

  sprites(): ParticleSprite[] {
    const out: ParticleSprite[] = [];
    for (const p of this.particles) {
      const f = p.f;
      const s: ParticleSprite = {
        pos: [p.pos[0] / 4096, p.pos[1] / 4096, p.pos[2] / 4096],
        sx: f[SP.ScaleX] / 4096,
        sy: f[SP.ScaleY] / 4096,
        rot: angle(f[SP.RotZ]),
        color: [
          f[SP.R] / 128,
          f[SP.R + 1] / 128,
          f[SP.R + 2] / 128,
          Math.min(f[SP.A], 128) / 128,
        ],
        texture: p.texture,
        additive: (p.flags & kSpGlow) !== 0,
        from_left: (Math.trunc(f[SP.RotX]) & 15) === 4,
      };
      if (s.sx <= 0 || s.sy <= 0 || s.color[3] <= 0.002) continue;
      out.push(s);
    }
    return out;
  }

  // the box of the particles (meters), for a picture of an effect
  bounds(): AABB {
    const b = aabb();
    for (const p of this.particles) {
      const r = Math.max(p.f[SP.ScaleX], p.f[SP.ScaleY]) / 8192;
      const c: Vec3 = [p.pos[0] / 4096, p.pos[1] / 4096, p.pos[2] / 4096];
      aabbAdd(b, [c[0] - r, c[1] - r, c[2] - r]);
      aabbAdd(b, [c[0] + r, c[1] + r, c[2] + r]);
    }
    return b;
  }
}

// ---------------------------------------------------------------------------------------------
// drawing (Renderer::draw_particles)

// the sprites' pictures, read once (shared by every GL context): a texture's name -> its file
// (index.json names the textures given by number, "<page>:<index>")
const images = new Map<string, Promise<ImageBitmap | null>>();
let sprite_index: Promise<Record<string, string>> | null = null;
function spriteImage(name: string): Promise<ImageBitmap | null> {
  let img = images.get(name);
  if (!img) {
    sprite_index ??= asset(`${SPRITES_PATH}/index.json`)
      .then((b) => JSON.parse(new TextDecoder().decode(b)))
      .catch(() => ({}));
    img = sprite_index
      .then((index) => asset(`${SPRITES_PATH}/${index[name] ?? name}.png`))
      .then((b) =>
        createImageBitmap(new Blob([b]), {
          premultiplyAlpha: "none",
          colorSpaceConversion: "none",
        }),
      )
      .catch(() => null);
    images.set(name, img);
  }
  return img;
}

// the particles: textured sprites, their color the game's (1 is the texture as it is: the PS2's
// 128), the alpha of their texture the PS2's (128 is opaque)
const SPRITE_VS = `#version 300 es
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec4 a_col;
uniform mat4 u_viewproj;
out vec2 v_uv;
out vec4 v_col;
void main() {
  v_uv = a_uv;
  v_col = a_col;
  gl_Position = u_viewproj * vec4(a_pos, 1.0);
}
`;
const SPRITE_FS = `#version 300 es
precision mediump float;
in vec2 v_uv;
in vec4 v_col;
uniform sampler2D u_tex;
out vec4 o_color;
void main() {
  vec4 t = texture(u_tex, v_uv);
  o_color = vec4(t.rgb * v_col.rgb, min(1.0, t.a * 2.0) * v_col.a);
  if (o_color.a < 0.004) discard;
}
`;

const FLOATS = 9; // a vertex: pos[3], uv[2], col[4]
const CORNERS = [0, 1, 2, 0, 2, 3];
const UV = [
  [0, 1],
  [1, 1],
  [1, 0],
  [0, 0],
];

// sprites drawn in a GL context (the view's, the thumbnails')
export class SpriteRenderer {
  private prog: WebGLProgram;
  private u_viewproj: WebGLUniformLocation | null;
  private u_tex: WebGLUniformLocation | null;
  private vao: WebGLVertexArrayObject;
  private vbo: WebGLBuffer;
  private verts = new Float32Array(6 * FLOATS * 1024);
  private white: WebGLTexture;
  // by texture name: its GL texture once its picture is read (null: being read)
  private textures = new Map<string, WebGLTexture | null>();
  onTexture: () => void = () => {}; // a picture was read: draw again

  constructor(readonly gl: WebGL2RenderingContext) {
    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
        throw new Error(gl.getShaderInfoLog(s) ?? "shader");
      return s;
    };
    this.prog = gl.createProgram()!;
    gl.attachShader(this.prog, compile(gl.VERTEX_SHADER, SPRITE_VS));
    gl.attachShader(this.prog, compile(gl.FRAGMENT_SHADER, SPRITE_FS));
    gl.linkProgram(this.prog);
    if (!gl.getProgramParameter(this.prog, gl.LINK_STATUS))
      throw new Error(gl.getProgramInfoLog(this.prog) ?? "program");
    this.u_viewproj = gl.getUniformLocation(this.prog, "u_viewproj");
    this.u_tex = gl.getUniformLocation(this.prog, "u_tex");
    this.vao = gl.createVertexArray()!;
    this.vbo = gl.createBuffer()!;
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, FLOATS * 4, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 2, gl.FLOAT, false, FLOATS * 4, 12);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 4, gl.FLOAT, false, FLOATS * 4, 20);
    gl.bindVertexArray(null);
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
      new Uint8Array([255, 255, 255, 255]),
    );
  }

  // a texture of ParticleData.textures: null while its picture is read, white when there is none
  texture(name: string | undefined): WebGLTexture | null {
    if (name === undefined) return this.white;
    if (this.textures.has(name)) return this.textures.get(name)!;
    this.textures.set(name, null);
    void spriteImage(name).then((img) => {
      const gl = this.gl;
      if (!img) return void this.textures.set(name, this.white);
      const t = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, img);
      gl.generateMipmap(gl.TEXTURE_2D);
      gl.texParameteri(
        gl.TEXTURE_2D,
        gl.TEXTURE_MIN_FILTER,
        gl.LINEAR_MIPMAP_LINEAR,
      );
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      this.textures.set(name, t);
      this.onTexture();
    });
    return null;
  }

  // every texture of these sprites read (a picture drawn once)
  async ready(sprites: ParticleSprite[], textures: string[]) {
    const names = new Set(
      sprites.map((s) => textures[s.texture]).filter((n) => n !== undefined),
    );
    for (const n of names) this.texture(n);
    await Promise.all([...names].map(spriteImage)); // uploaded first (texture()'s then)
  }

  // after the scene, without writing the depth: `viewproj` the draws' matrix, `view` the camera's
  // (its right and up face the sprites to it), `depthFunc` the test (reversed over the game view)
  draw(
    sprites: ParticleSprite[],
    textures: string[],
    viewproj: Mat4,
    view: Mat4,
    depthFunc: number,
  ) {
    if (!sprites.length) return 0;
    const gl = this.gl;
    // the camera's right and up (rows of the view's rotation): the sprites face it
    const right: Vec3 = [view[0], view[4], view[8]],
      up: Vec3 = [view[1], view[5], view[9]];
    // by blend, then texture: one draw each
    const order = sprites
      .map((s, i) => i)
      .sort((a, b) => {
        const sa = sprites[a],
          sb = sprites[b];
        if (sa.additive !== sb.additive) return sa.additive ? 1 : -1;
        return sa.texture - sb.texture;
      });
    if (this.verts.length < sprites.length * 6 * FLOATS)
      this.verts = new Float32Array(sprites.length * 6 * FLOATS * 2);
    const v = this.verts;
    const runs: {
      first: number;
      count: number;
      texture: number;
      additive: boolean;
    }[] = [];
    const ax = [0, 0, 0],
      ay = [0, 0, 0],
      xs = [0, 0, 0, 0],
      ys = [-0.5, -0.5, 0.5, 0.5];
    let o = 0;
    for (const i of order) {
      const s = sprites[i];
      const c = Math.cos(s.rot),
        sn = Math.sin(s.rot);
      for (let k = 0; k < 3; k++) {
        ax[k] = (right[k] * c + up[k] * sn) * s.sx;
        ay[k] = (up[k] * c - right[k] * sn) * s.sy;
      }
      xs[0] = xs[3] = s.from_left ? 0 : -0.5;
      xs[1] = xs[2] = xs[0] + 1;
      const last = runs[runs.length - 1];
      if (!last || last.texture !== s.texture || last.additive !== s.additive)
        runs.push({
          first: o / FLOATS,
          count: 0,
          texture: s.texture,
          additive: s.additive,
        });
      for (const k of CORNERS) {
        for (let a = 0; a < 3; a++)
          v[o + a] = s.pos[a] + ax[a] * xs[k] + ay[a] * ys[k];
        v[o + 3] = UV[k][0];
        v[o + 4] = UV[k][1];
        v.set(s.color, o + 5);
        o += FLOATS;
      }
      runs[runs.length - 1].count += 6;
    }
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.bufferData(gl.ARRAY_BUFFER, v.subarray(0, o), gl.STREAM_DRAW);
    gl.useProgram(this.prog);
    gl.uniformMatrix4fv(this.u_viewproj, false, viewproj);
    gl.uniform1i(this.u_tex, 0);
    gl.activeTexture(gl.TEXTURE0);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(depthFunc);
    gl.depthMask(false);
    gl.disable(gl.CULL_FACE);
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.FUNC_ADD);
    let draws = 0;
    for (const r of runs) {
      const tex = this.texture(textures[r.texture]);
      if (!tex) continue; // its picture is being read
      gl.blendFunc(gl.SRC_ALPHA, r.additive ? gl.ONE : gl.ONE_MINUS_SRC_ALPHA);
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.drawArrays(gl.TRIANGLES, r.first, r.count);
      draws++;
    }
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.disable(gl.BLEND);
    gl.depthMask(true);
    gl.bindVertexArray(null);
    return draws;
  }
}

// the effects lost in the game's fog of the hour and the weather, far away (App::fog_effects):
// rs as the game view set it (its fog's color and how much of it at most, where it starts and ends)
function fogEffects(
  sprites: ParticleSprite[],
  rs: RenderSettings,
  camera: Vec3,
) {
  if (!rs.game_view) return;
  const start = rs.game_fog_start,
    end = Math.max(rs.game_fog_end, start + 1);
  const most = rs.game_fog[3];
  for (const s of sprites) {
    const d = length([
      s.pos[0] - camera[0],
      s.pos[1] - camera[1],
      s.pos[2] - camera[2],
    ]);
    const f = Math.min(Math.max((d - start) / (end - start), 0), 1) * most;
    if (f <= 0) continue;
    for (let k = 0; k < 3; k++)
      s.color[k] = s.additive
        ? s.color[k] * (1 - f)
        : s.color[k] + (rs.game_fog[k] - s.color[k]) * f;
  }
}

// ---------------------------------------------------------------------------------------------
// the effects of the document in the view (App::update_effects, draw_effects)

interface EffectEmitter {
  id: number;
  group: string;
  world: number[]; // 16 floats, column-major
}

class Effects {
  // View > Effects (engine.state.view.effects), and the game view's hour (the hour-mask)
  on = true;
  hour = 12;
  readonly system = new ParticleSystem();
  private data: ParticleData | null = null;
  private emitters: { id: number; group: SpGroup; world: Mat4 }[] = [];
  private sprites: SpriteRenderer | null = null;
  private last_time = -1;
  private busy = false;
  private again = false;
  private stop_animation: (() => void) | null = null;

  // its pass in the viewport's frame and the engine's changes; returns what stops them
  attach(): () => void {
    const offs = [
      viewport.addPass("overlay", (r, rs) => this.draw(r, rs), {
        capture: true,
      }),
    ];
    let live = true;
    void engineReady.then(() => {
      if (!live) return;
      offs.push(
        onEngine("jle:changed", (p: any) => {
          if (!p || p.scene || p.document) void this.refresh();
        }),
      );
      void this.refresh();
    });
    return () => {
      live = false;
      offs.forEach((off) => off());
      this.animate(false);
      this.system.clear();
      this.sprites = null;
    };
  }

  setOn(on: boolean) {
    if (on === this.on) return;
    this.on = on;
    if (on) void this.refresh();
    else {
      this.system.clear();
      this.animate(false);
    }
    viewport.requestRedraw();
  }

  // the effects of the document again (the data read the first time); one call at a time
  async refresh() {
    if (!this.on) return;
    if (this.busy) return void (this.again = true);
    this.busy = true;
    try {
      do {
        this.again = false;
        this.data ??= await loadParticleData();
        const data = this.data;
        if (!data) return;
        const list = ((await call("effect_emitters")) ?? []) as EffectEmitter[];
        this.emitters = [];
        for (const e of list) {
          const group = data.group(e.group);
          if (group)
            this.emitters.push({
              id: e.id,
              group,
              world: new Float32Array(e.world),
            });
        }
      } while (this.again);
    } catch (e) {
      console.warn("[effects]", e);
    } finally {
      this.busy = false;
    }
    viewport.requestRedraw();
  }

  // the view draws every frame while effects run (draw() decides)
  private animate(on: boolean) {
    if (on === !!this.stop_animation) return;
    this.stop_animation?.();
    this.stop_animation = on ? viewport.animate() : null;
  }

  private draw(r: Renderer, rs: RenderSettings) {
    // the time since the frame before (none in a capture: the same frame drawn again)
    const dt = this.last_time < 0 ? 0 : Math.max(rs.time - this.last_time, 0);
    this.last_time = rs.time;
    if (!this.on || !this.data) return;
    const camera = viewport.camera.pos;
    if (dt > 0) {
      this.system.beginFrame();
      // where the actor is drawn (a drag's preview first)
      for (const e of this.emitters) {
        const n = viewport.store.nodes.get(e.id);
        this.system.emit(e.id, e.group, n ? viewport.store.world(n) : e.world);
      }
      this.system.update(this.data, dt, camera, this.hour);
    }
    const sprites = this.system.sprites();
    fogEffects(sprites, rs, camera);
    if (!this.sprites || this.sprites.gl !== r.gl) {
      this.sprites = new SpriteRenderer(r.gl);
      this.sprites.onTexture = () => viewport.requestRedraw();
    }
    r.stats.draws += this.sprites.draw(
      sprites,
      this.data.textures,
      r.drawViewProj,
      r.viewMatrix,
      r.depthFunc(r.gl.LEQUAL),
    );
    this.animate(this.emitters.length > 0 || this.system.count > 0);
  }
}

export const effects = new Effects();
