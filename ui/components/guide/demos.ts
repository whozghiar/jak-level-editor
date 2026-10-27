// Short animations of the editor's tools, for the guided tour and the tooltips (render/tool_demo.cpp):
// a small scene of the editor's own shapes (nothing of the game), the gizmo and the cursor, scripted
// over a loop of a few seconds; and the shapes seen from above at three quarters (the Build panel's
// buttons). Drawn on a 2D canvas: faces lit as the viewport lights shapes, sorted far to near, with
// the lines of the shapes' grid texture; then what goes over the picture (the cursor, a click, a
// label, a selection box). ToolDemo.svelte shows them.

import { printf, t } from "../../lib/i18n";

type V2 = [number, number];
type V3 = [number, number, number];
type V4 = [number, number, number, number];
type Q = V4; // x, y, z, w
type M = number[]; // an affine transform, column-major: its x, y, z axes, then its translation

const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a: V3, k: number): V3 => [a[0] * k, a[1] * k, a[2] * k];
const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: V3, b: V3): V3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const normalize = (a: V3): V3 => mul(a, 1 / (Math.hypot(...a) || 1));
const mix = (a: V3, b: V3, k: number) => add(a, mul(sub(b, a), k));
const mix2 = (a: V2, b: V2, k: number): V2 => [
  a[0] + (b[0] - a[0]) * k,
  a[1] + (b[1] - a[1]) * k,
];
const radians = (deg: number) => (deg * Math.PI) / 180;
const clamp01 = (v: number) => (v < 0 ? 0 : v > 1 ? 1 : v);
// 0 before a, 1 after b, eased between
function ease(t: number, a: number, b: number): number {
  const x = clamp01((t - a) / (b - a));
  return x * x * (3 - 2 * x);
}

function axisAngle(axis: V3, rad: number): Q {
  const a = normalize(axis);
  const s = Math.sin(rad * 0.5);
  return [a[0] * s, a[1] * s, a[2] * s, Math.cos(rad * 0.5)];
}
function qmul([x, y, z, w]: Q, b: Q): Q {
  return [
    w * b[0] + x * b[3] + y * b[2] - z * b[1],
    w * b[1] - x * b[2] + y * b[3] + z * b[0],
    w * b[2] + x * b[1] - y * b[0] + z * b[3],
    w * b[3] - x * b[0] - y * b[1] - z * b[2],
  ];
}
function qrotate(q: Q, v: V3): V3 {
  const u: V3 = [q[0], q[1], q[2]];
  const k = mul(cross(u, v), 2);
  return add(add(v, mul(k, q[3])), cross(u, k));
}
const qnormalized = (q: Q): Q => q.map((c) => c / Math.hypot(...q)) as Q;
const NO_TURN: Q = [0, 0, 0, 1];

const IDENTITY: M = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0];
const translate = (v: V3): M => [1, 0, 0, 0, 1, 0, 0, 0, 1, ...v];
const scale = (v: V3): M => [v[0], 0, 0, 0, v[1], 0, 0, 0, v[2], 0, 0, 0];
const rotate = (q: Q): M => [
  ...qrotate(q, [1, 0, 0]),
  ...qrotate(q, [0, 1, 0]),
  ...qrotate(q, [0, 0, 1]),
  0,
  0,
  0,
];
const point = (m: M, p: V3): V3 => [
  m[0] * p[0] + m[3] * p[1] + m[6] * p[2] + m[9],
  m[1] * p[0] + m[4] * p[1] + m[7] * p[2] + m[10],
  m[2] * p[0] + m[5] * p[1] + m[8] * p[2] + m[11],
];
// a * b * c...: the last applied first
const chain = (...ms: M[]): M =>
  ms.reduce((a, b) => {
    const axis = (i: number) =>
      sub(point(a, [b[i], b[i + 1], b[i + 2]]), [a[9], a[10], a[11]]);
    return [
      ...axis(0),
      ...axis(3),
      ...axis(6),
      ...point(a, [b[9], b[10], b[11]]),
    ];
  });

// ---- the shapes (make_shape in scene/shapes.cpp) ----

/** A shape of the editor (ShapeParams in scene/shapes.h): its origin is the middle of its base. */
export interface ShapeParams {
  // box, ramp, stairs, cylinder, roof, roof-hip, doorway, window, ground
  kind: string;
  size: V3; // meters: width (x), height (y), depth (z)
  tile?: number; // meters per repeat of its texture (2)
  steps?: number; // stairs (8)
  sides?: number; // cylinder (16)
  opening?: V2; // doorway, window: the opening's width and height (2, 3)
  bumps?: number; // ground (0.3)
  bump_size?: number; // ground (3)
  seed?: number; // ground (1)
}

/** What the Build panel and menu offer (shape_presets): walls of 4 m by 4 m, the grid of modular building. */
export const shapePresets: { name: string; params: ShapeParams }[] = [
  { name: "box", params: { kind: "box", size: [4, 4, 4] } },
  { name: "floor", params: { kind: "box", size: [8, 0.5, 8] } },
  { name: "wall", params: { kind: "box", size: [8, 4, 0.5] } },
  { name: "doorway", params: { kind: "doorway", size: [4, 4, 0.5] } },
  {
    name: "window",
    params: { kind: "window", size: [4, 4, 0.5], opening: [1.6, 1.4] },
  },
  { name: "pillar", params: { kind: "box", size: [1, 6, 1] } },
  { name: "ramp", params: { kind: "ramp", size: [4, 2, 8] } },
  { name: "stairs", params: { kind: "stairs", size: [4, 3, 6] } },
  { name: "roof", params: { kind: "roof", size: [8, 3, 8] } },
  { name: "roof-hip", params: { kind: "roof-hip", size: [8, 3, 10] } },
  { name: "cylinder", params: { kind: "cylinder", size: [2, 6, 2] } },
  { name: "ground", params: { kind: "ground", size: [16, 0.5, 16] } },
];

// a flat convex face, its corners in order facing `n`; `top`: the ground's top, its texture laid
// from above
interface Face {
  pts: V3[];
  n: V3;
  top?: boolean;
}
interface Mesh {
  faces: Face[];
  lo: V3;
  hi: V3;
  tile: number;
}

// A smooth noise over the plane, -0.5..0.5, one bump per unit, the same for the same seed
function smoothNoise(x: number, z: number, seed: number): number {
  const hash = (x: number, z: number) => {
    let k =
      (Math.imul(x, 73856093) ^
        Math.imul(z, 19349663) ^
        Math.imul(seed, 83492791)) >>>
      0;
    k = Math.imul(k ^ (k >>> 13), 1274126177) >>> 0;
    return ((k ^ (k >>> 16)) & 0xffff) / 65535 - 0.5;
  };
  const x0 = Math.floor(x);
  const z0 = Math.floor(z);
  let fx = x - x0;
  let fz = z - z0;
  fx = fx * fx * (3 - 2 * fx);
  fz = fz * fz * (3 - 2 * fz);
  const a = hash(x0, z0) + (hash(x0 + 1, z0) - hash(x0, z0)) * fx;
  const b = hash(x0, z0 + 1) + (hash(x0 + 1, z0 + 1) - hash(x0, z0 + 1)) * fx;
  return a + (b - a) * fz;
}

// ponytail: no bend nor rounded edges (no preset nor demo has them); add them when a picture shows a shape edited
// prettier-ignore
function makeShape(s: ShapeParams): Mesh {
  const w = s.size[0] / 2;
  const h = s.size[1];
  const d = s.size[2] / 2;
  const faces: Face[] = [];
  const face = (pts: V3[], n: V3, top = false) => {
    n = normalize(n);
    if (dot(cross(sub(pts[1], pts[0]), sub(pts[2], pts[0])), n) < 0) pts.reverse();
    faces.push({ pts, n, top });
  };
  const box = ([x0, y0, z0]: V3, [x1, y1, z1]: V3) => {
    face([[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]], [1, 0, 0]);
    face([[x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]], [-1, 0, 0]);
    face([[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]], [0, 1, 0]);
    face([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]], [0, -1, 0]);
    face([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], [0, 0, 1]);
    face([[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]], [0, 0, -1]);
  };
  if (s.kind === "ramp") {
    // rising towards +z
    face([[-w, 0, -d], [w, 0, -d], [w, 0, d], [-w, 0, d]], [0, -1, 0]);
    face([[-w, 0, d], [w, 0, d], [w, h, d], [-w, h, d]], [0, 0, 1]);
    face([[-w, 0, -d], [w, 0, -d], [w, h, d], [-w, h, d]], [0, 2 * d, -h]);
    face([[w, 0, -d], [w, 0, d], [w, h, d]], [1, 0, 0]);
    face([[-w, 0, -d], [-w, 0, d], [-w, h, d]], [-1, 0, 0]);
  } else if (s.kind === "stairs") {
    // rising towards +z: one block under each step
    const n = Math.max(s.steps ?? 8, 1);
    for (let i = 0; i < n; i++) box([-w, 0, -d + (2 * d * i) / n], [w, (h * (i + 1)) / n, -d + (2 * d * (i + 1)) / n]);
  } else if (s.kind === "roof" || s.kind === "roof-hip") {
    // two slopes meeting at a ridge along z, height h; a hip roof slopes at its ends too (its ridge
    // shorter by its width, a pyramid when it is square)
    const r = s.kind === "roof-hip" ? Math.max(0, d - w) : d;
    face([[-w, 0, -d], [0, h, -r], [0, h, r], [-w, 0, d]], [-h, w, 0]);
    face([[w, 0, -d], [0, h, -r], [0, h, r], [w, 0, d]], [h, w, 0]);
    if (s.kind === "roof-hip") {
      face([[-w, 0, d], [w, 0, d], [0, h, r]], [0, d - r, h]);
      face([[-w, 0, -d], [w, 0, -d], [0, h, -r]], [0, d - r, -h]);
    } else {
      face([[-w, 0, d], [w, 0, d], [0, h, d]], [0, 0, 1]);
      face([[-w, 0, -d], [w, 0, -d], [0, h, -d]], [0, 0, -1]);
    }
    face([[-w, 0, -d], [w, 0, -d], [w, 0, d], [-w, 0, d]], [0, -1, 0]);
  } else if (s.kind === "doorway" || s.kind === "window") {
    // a wall around its opening: a door from the ground, a window in the middle of its height
    const [openW, openH] = s.opening ?? [2, 3];
    const ow = Math.min(openW, s.size[0] - 0.2) / 2;
    const oh = Math.min(openH, h - 0.1);
    const bottom = s.kind === "window" ? (h - oh) / 2 : 0;
    const top = bottom + oh;
    box([-w, 0, -d], [-ow, h, d]);
    box([ow, 0, -d], [w, h, d]);
    box([-ow, top, -d], [ow, h, d]);
    if (bottom > 0) box([-ow, 0, -d], [ow, bottom, d]);
  } else if (s.kind === "cylinder") {
    const n = Math.max(s.sides ?? 16, 3);
    const ring = Array.from({ length: n }, (_, i): V3 => [w * Math.cos((2 * Math.PI * i) / n), 0, d * Math.sin((2 * Math.PI * i) / n)]);
    const up = (v: V3): V3 => [v[0], h, v[2]];
    ring.forEach((a, i) => {
      const b = ring[(i + 1) % n];
      face([a, b, up(b), up(a)], [(a[0] + b[0]) / 2, 0, (a[2] + b[2]) / 2]);
    });
    face(ring.map(up), [0, 1, 0]);
    face([...ring], [0, -1, 0]);
  } else if (s.kind === "ground") {
    // its top on a grid, raised and lowered by a smooth noise; its sides follow it
    const bumps = s.bumps ?? 0.3;
    const size = s.bump_size ?? 3;
    const seed = s.seed ?? 1;
    const step = Math.min(Math.max(size / 3, 0.25), 4);
    const nx = bumps > 0 ? Math.min(Math.max(Math.ceil(s.size[0] / step), 1), 128) : 1;
    const nz = bumps > 0 ? Math.min(Math.max(Math.ceil(s.size[2] / step), 1), 128) : 1;
    const height = (x: number, z: number) => {
      if (bumps <= 0) return h;
      const u = x / size;
      const v = z / size;
      return h + bumps * (smoothNoise(u, v, seed) + 0.35 * smoothNoise(u * 2.3 + 17, v * 2.3 + 5, seed));
    };
    const at = (ix: number, iz: number): V3 => {
      const x = -w + (2 * w * ix) / nx;
      const z = -d + (2 * d * iz) / nz;
      return [x, height(x, z), z];
    };
    for (let iz = 0; iz < nz; iz++)
      for (let ix = 0; ix < nx; ix++) {
        face([at(ix, iz), at(ix, iz + 1), at(ix + 1, iz)], [0, 1, 0], true);
        face([at(ix + 1, iz), at(ix, iz + 1), at(ix + 1, iz + 1)], [0, 1, 0], true);
      }
    // the sides, along its edges, and its bottom
    const side = (a: V3, b: V3, out: V3) => face([a, b, [b[0], height(b[0], b[2]), b[2]], [a[0], height(a[0], a[2]), a[2]]], out);
    for (let ix = 0; ix < nx; ix++) {
      const x0 = -w + (2 * w * ix) / nx;
      const x1 = -w + (2 * w * (ix + 1)) / nx;
      side([x0, 0, -d], [x1, 0, -d], [0, 0, -1]);
      side([x0, 0, d], [x1, 0, d], [0, 0, 1]);
    }
    for (let iz = 0; iz < nz; iz++) {
      const z0 = -d + (2 * d * iz) / nz;
      const z1 = -d + (2 * d * (iz + 1)) / nz;
      side([-w, 0, z0], [-w, 0, z1], [-1, 0, 0]);
      side([w, 0, z0], [w, 0, z1], [1, 0, 0]);
    }
    face([[-w, 0, -d], [w, 0, -d], [w, 0, d], [-w, 0, d]], [0, -1, 0]);
  } else {
    box([-w, 0, -d], [w, h, d]);
  }
  const all = faces.flatMap((f) => f.pts);
  const lo = [0, 1, 2].map((i) => Math.min(...all.map((p) => p[i]))) as V3;
  const hi = [0, 1, 2].map((i) => Math.max(...all.map((p) => p[i]))) as V3;
  return { faces, lo, hi, tile: s.tile ?? 2 };
}

// ---- the camera (render/camera.h) ----

class Camera {
  pos: V3 = [0, 10, 30];
  yaw = 0; // radians, 0 looks down -Z
  pitch = -0.3;
  fov = 45;
  forward(): V3 {
    const c = Math.cos(this.pitch);
    return [
      -Math.sin(this.yaw) * c,
      Math.sin(this.pitch),
      -Math.cos(this.yaw) * c,
    ];
  }
  lookAt(target: V3) {
    const d = normalize(sub(target, this.pos));
    this.pitch = Math.asin(Math.min(Math.max(d[1], -0.999), 0.999));
    this.yaw = Math.atan2(-d[0], -d[2]);
  }
  // Frames a box: keeps the orientation, backs off so the box fits.
  frame(lo: V3, hi: V3) {
    const r = Math.max(Math.hypot(...sub(hi, lo)) * 0.5, 0.5);
    const dist = (r / Math.tan(radians(this.fov) * 0.5)) * 1.1;
    this.pos = sub(mix(lo, hi, 0.5), mul(this.forward(), dist));
  }
}

// ---- the picture ----

const SELECTED: V4 = [1, 0.6, 0.12, 0.35];
const NONE: V4 = [0, 0, 0, 0];
const WALL_COLOR: V3 = [0.92, 0.88, 0.78];
const RED = "rgb(232,72,72)";
const GREEN = "rgb(96,206,90)";
const BLUE = "rgb(80,132,245)";
const HOT = "rgb(255,220,70)";
const ORANGE = "rgb(255,158,38)";
const CLEAR = "rgb(33,38,46)";
const LIGHT = normalize([0.35, 1, 0.25]);
// the editor's grid texture: light grey, a darker line around each cell
const CELL = 168 / 255;
const CELL_LINE = 96 / 255;
const FONT = "12px Inter, 'Noto Sans', sans-serif"; // the interface's (src/fonts.css)

type Overlay =
  | { kind: "cursor"; at: V2 }
  | { kind: "click"; at: V2; k: number } // k: its growth 0..1
  | { kind: "rect"; at: V2; to: V2; style: "" | "bar" | "knob" } // "": a selection box, "bar" a slider's track, "knob" its knob
  | { kind: "label"; at: V2; text: string }; // a key of the translations (guide.*) or the text itself

interface DrawnFace {
  depth: number;
  pts: V2[];
  fill: string;
  line: string;
  grid: number[]; // the texture's lines: x0, y0, x1, y1...
  alpha: number;
}

class Picture {
  faces: DrawnFace[] = [];
  lines: { a: V2; b: V2; color: string }[] = []; // over the meshes: the selection's outline, the gizmo
  overlay: Overlay[] = [];
  wall = WALL_COLOR; // the color of the walls' material (the build demo paints it)
  #f: V3;
  #r: V3;
  #u: V3;
  #tan: number;

  constructor(
    public cam: Camera,
    public w: number,
    public h: number,
  ) {
    this.#f = cam.forward();
    this.#r = normalize(cross(this.#f, [0, 1, 0]));
    this.#u = cross(this.#r, this.#f);
    this.#tan = Math.tan(radians(cam.fov) * 0.5);
  }

  // pixels of the picture, and the distance in front of the camera
  project(p: V3): [number, number, number] {
    const d = sub(p, this.cam.pos);
    const z = dot(d, this.#f);
    const x = dot(d, this.#r) / (z * this.#tan * (this.w / this.h));
    const y = dot(d, this.#u) / (z * this.#tan);
    return [(x * 0.5 + 0.5) * this.w, (1 - (y * 0.5 + 0.5)) * this.h, z];
  }
  screen(p: V3): V2 {
    const [x, y] = this.project(p);
    return [x, y];
  }

  /**
   * A mesh at `world` in `color`, mixed with `tint` (its alpha the amount); `alpha` < 1: see-through,
   * its back faces seen. `under`: drawn before everything (the ground).
   */
  mesh(m: Mesh, world: M, color: V3, tint = NONE, alpha = 1, under = false) {
    const shade = (k: number) => {
      const c = color.map(
        (v, i) => Math.min(v * k, 1) * (1 - tint[3]) + tint[i] * tint[3],
      );
      return `rgb(${c.map((v) => Math.round(v * 255)).join(",")})`;
    };
    for (const f of m.faces) {
      const pts = f.pts.map((p) => point(world, p));
      // its normal from its corners (Newell's), facing the camera: lit on both sides
      const n: V3 = [0, 0, 0];
      pts.forEach((a, i) => {
        const b = pts[(i + 1) % pts.length];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
      });
      if (!Math.hypot(...n)) continue;
      const facing = dot(n, sub(this.cam.pos, pts[0])) > 0;
      if (!facing && alpha >= 1) continue;
      const projected = pts.map((p) => this.project(p));
      if (projected.some((p) => p[2] < 0.1)) continue; // ponytail: dropped, not clipped at the near plane: no demo comes that close
      const l =
        0.45 +
        0.55 * Math.max(dot(normalize(facing ? n : mul(n, -1)), LIGHT), 0);
      this.faces.push({
        depth: under
          ? Infinity
          : projected.reduce((s, p) => s + p[2], 0) / projected.length,
        pts: projected.map(([x, y]) => [x, y]),
        fill: shade(CELL * l),
        line: shade(CELL_LINE * l),
        grid: this.#grid(f, world, m.tile),
        alpha,
      });
    }
    if (tint === SELECTED) this.#outline(m, world);
  }

  // the lines of the grid texture on a face: one cell per `tile` meters, horizontal along its width
  // and upright along its slope (add_face's uvs); the ground's top laid from above
  #grid(f: Face, world: M, tile: number): number[] {
    const right: V3 =
      f.top || Math.abs(f.n[1]) > 0.99
        ? [1, 0, 0]
        : normalize(cross([0, 1, 0], f.n));
    const across: V3 = f.top ? [0, 0, 1] : cross(f.n, right);
    const out: number[] = [];
    for (const axis of [right, across]) {
      const g = f.pts.map((p) => dot(p, axis) / tile);
      // a line on an edge too: the texture's cells are outlined
      for (
        let k = Math.ceil(Math.min(...g) - 1e-5);
        k <= Math.max(...g) + 1e-5;
        k++
      ) {
        const ends: V3[] = [];
        f.pts.forEach((a, i) => {
          const da = g[i] - k;
          const db = g[(i + 1) % g.length] - k;
          if (Math.abs(da) < 1e-5) ends.push(a);
          else if (Math.abs(db) >= 1e-5 && da * db < 0)
            ends.push(mix(a, f.pts[(i + 1) % g.length], da / (da - db)));
        });
        if (ends.length >= 2)
          out.push(
            ...this.screen(point(world, ends[0])),
            ...this.screen(point(world, ends[1])),
          );
      }
    }
    return out;
  }

  // the selection's outline: the mesh's box, the edges of its faces turned to the camera
  #outline(m: Mesh, world: M) {
    const corner = (i: number) =>
      point(world, [
        i & 1 ? m.hi[0] : m.lo[0],
        i & 2 ? m.hi[1] : m.lo[1],
        i & 4 ? m.hi[2] : m.lo[2],
      ]);
    const c = Array.from({ length: 8 }, (_, i) => corner(i));
    const middle = mix(c[0], c[7], 0.5);
    // face (axis, side): toward the camera?
    const seen = (axis: number, side: number) => {
      const on = c.filter((_, i) => ((i >> axis) & 1) === side);
      const center = mul(on.reduce(add), 0.25);
      const [b, e] = [(axis + 1) % 3, (axis + 2) % 3];
      let n = cross(sub(c[1 << b], c[0]), sub(c[1 << e], c[0]));
      if (dot(n, sub(center, middle)) < 0) n = mul(n, -1);
      return dot(n, sub(this.cam.pos, center)) > 0;
    };
    for (let i = 0; i < 8; i++)
      for (let axis = 0; axis < 3; axis++) {
        if (i & (1 << axis)) continue;
        const others = [0, 1, 2].filter((a) => a !== axis);
        if (others.some((a) => seen(a, (i >> a) & 1)))
          this.line(c[i], c[i | (1 << axis)], ORANGE);
      }
  }

  line(a: V3, b: V3, color: string) {
    this.lines.push({ a: this.screen(a), b: this.screen(b), color });
  }

  gizmoAxes(at: V3, size: number, hot: number, turn = NO_TURN) {
    const axes = [
      qrotate(turn, [1, 0, 0]),
      qrotate(turn, [0, 1, 0]),
      qrotate(turn, [0, 0, 1]),
    ];
    const colors = [RED, GREEN, BLUE];
    for (let i = 0; i < 3; i++) {
      const c = hot === i ? HOT : colors[i];
      const tip = add(at, mul(axes[i], size));
      const u = mul(axes[(i + 1) % 3], size * 0.08);
      const v = mul(axes[(i + 2) % 3], size * 0.08);
      this.line(at, tip, c);
      this.line(sub(tip, u), add(tip, u), c); // a small cross at the tip: the handle
      this.line(sub(tip, v), add(tip, v), c);
    }
  }

  gizmoRings(at: V3, size: number, hot: number) {
    const circle = (u: V3, v: V3, color: string) => {
      const p = (i: number) =>
        add(
          at,
          mul(
            add(
              mul(u, Math.cos((i / 48) * 2 * Math.PI)),
              mul(v, Math.sin((i / 48) * 2 * Math.PI)),
            ),
            size,
          ),
        );
      for (let i = 0; i < 48; i++) this.line(p(i), p(i + 1), color);
    };
    circle([0, 1, 0], [0, 0, 1], hot === 0 ? HOT : RED);
    circle([1, 0, 0], [0, 0, 1], hot === 1 ? HOT : GREEN);
    circle([1, 0, 0], [0, 1, 0], hot === 2 ? HOT : BLUE);
  }

  draw(ctx: CanvasRenderingContext2D) {
    ctx.save();
    ctx.globalAlpha = 1;
    ctx.fillStyle = CLEAR;
    ctx.fillRect(0, 0, this.w, this.h);
    ctx.lineJoin = "round";
    // far to near (stable: the ground first, in the order given)
    for (const f of [...this.faces].sort((a, b) => b.depth - a.depth)) {
      ctx.globalAlpha = f.alpha;
      ctx.beginPath();
      f.pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      ctx.closePath();
      ctx.fillStyle = f.fill;
      ctx.fill();
      if (f.alpha >= 1) {
        // its own color over its edges: no seams between faces
        ctx.strokeStyle = f.fill;
        ctx.lineWidth = 0.75;
        ctx.stroke();
      }
      if (f.grid.length) {
        ctx.beginPath();
        for (let i = 0; i < f.grid.length; i += 4) {
          ctx.moveTo(f.grid[i], f.grid[i + 1]);
          ctx.lineTo(f.grid[i + 2], f.grid[i + 3]);
        }
        ctx.strokeStyle = f.line;
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    }
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1.5;
    for (const l of this.lines) {
      ctx.beginPath();
      ctx.moveTo(...l.a);
      ctx.lineTo(...l.b);
      ctx.strokeStyle = l.color;
      ctx.stroke();
    }
    for (const o of this.overlay) this.#drawOverlay(ctx, o);
    ctx.restore();
  }

  #drawOverlay(ctx: CanvasRenderingContext2D, o: Overlay) {
    const round = (a: V2, b: V2, r: number) => {
      ctx.beginPath();
      ctx.roundRect(
        Math.min(a[0], b[0]),
        Math.min(a[1], b[1]),
        Math.abs(b[0] - a[0]),
        Math.abs(b[1] - a[1]),
        r,
      );
    };
    const [x, y] = o.at;
    if (o.kind === "rect") {
      round(o.at, o.to, o.style ? 3 : 0);
      if (o.style === "bar") {
        ctx.fillStyle = "rgb(90,96,108)";
        ctx.fill();
      } else if (o.style === "knob") {
        ctx.fillStyle = "rgb(230,236,245)";
        ctx.fill();
      } else {
        ctx.fillStyle = "rgba(80,160,255,0.16)";
        ctx.fill();
        ctx.strokeStyle = "rgba(80,160,255,0.86)";
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    } else if (o.kind === "click") {
      ctx.beginPath();
      ctx.arc(x, y, 6 + 16 * o.k, 0, 2 * Math.PI);
      ctx.strokeStyle = `rgba(255,255,255,${1 - o.k})`;
      ctx.lineWidth = 2;
      ctx.stroke();
    } else if (o.kind === "cursor") {
      // an arrow: white, outlined
      ctx.beginPath();
      ctx.moveTo(x + 4.5, y + 12);
      ctx.lineTo(x + 8, y + 19);
      ctx.strokeStyle = "black";
      ctx.lineWidth = 4;
      ctx.stroke();
      ctx.strokeStyle = "white";
      ctx.lineWidth = 2;
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(x, y);
      ctx.lineTo(x, y + 17);
      ctx.lineTo(x + 12, y + 12);
      ctx.closePath();
      ctx.fillStyle = "white";
      ctx.fill();
      ctx.strokeStyle = "black";
      ctx.lineWidth = 1.5;
      ctx.stroke();
    } else {
      const text = o.text.startsWith("guide.") ? t(o.text) : o.text;
      ctx.font = FONT;
      ctx.textBaseline = "top";
      // wrapped at its words, kept in the picture
      const wrap = Math.max(60, this.w - x - 12);
      const lines: string[] = [];
      for (const word of text.split(/\s+/)) {
        const last = lines.length ? `${lines[lines.length - 1]} ${word}` : word;
        if (lines.length && ctx.measureText(last).width <= wrap)
          lines[lines.length - 1] = last;
        else lines.push(word);
      }
      const lh = 15;
      const width = Math.min(
        wrap,
        Math.max(...lines.map((l) => ctx.measureText(l).width)),
      );
      const height = lines.length * lh;
      const ty = y + height + 4 > this.h ? this.h - height - 6 : y;
      round([x - 4, ty - 2], [x + width + 4, ty + height + 2], 4);
      ctx.fillStyle = "rgba(0,0,0,0.63)";
      ctx.fill();
      ctx.fillStyle = "white";
      lines.forEach((l, i) => ctx.fillText(l, x, ty + i * lh + 1.5));
    }
  }
}

// the scene: the editor's grid texture, a color per kind of thing (null: the walls' material, which
// the build demo paints)
interface Shape {
  mesh: Mesh;
  color: V3 | null;
}
const shape = (kind: string, size: V3, color: V3 | null): Shape => ({
  mesh: makeShape({ kind, size, tile: 1 }),
  color,
});
const CRATE: V3 = [1, 0.62, 0.3];
const RED_PAD: V3 = [1, 0.38, 0.32];
const GROUND = shape("box", [14, 0.2, 14], [0.8, 0.82, 0.85]);
const BOX = shape("box", [1.6, 1.6, 1.6], CRATE);
const PILLAR = shape("cylinder", [1.2, 3, 1.2], [0.55, 0.72, 1]);
const WALL = shape("doorway", [4, 3, 0.4], null);
const BACK = shape("box", [4, 3, 0.4], null);
const SIDE = shape("window", [3.2, 3, 0.4], null);
const PAD = shape("cylinder", [1.8, 0.3, 1.8], RED_PAD);
const BALL = shape("cylinder", [0.6, 1, 0.6], [1, 0.9, 0.3]);
const ROOF = shape("roof", [4.4, 1.6, 4.4], CRATE);
const TOWER = shape("box", [2, 5, 2], WALL_COLOR);
const CAP = shape("roof-hip", [2.6, 1.4, 2.6], RED_PAD);
// what makes a level playable: flags on poles, a death zone, the walkable ground, a platform
const POLE = shape("cylinder", [0.12, 2, 0.12], [0.45, 0.45, 0.45]);
const START_FLAG = shape("box", [0.8, 0.5, 0.05], [0.2, 0.5, 1]);
const CHECK_FLAG = shape("box", [0.8, 0.5, 0.05], [0.25, 0.9, 0.35]);
const SPAWN_FLAG = shape("box", [0.8, 0.5, 0.05], [1, 0.45, 0.12]);
const ZONE = shape("box", [16, 0.3, 6], [1, 0.22, 0.15]);
const STEP = shape("box", [2.2, 0.5, 2.2], [0.75, 0.78, 0.82]);
const NAV = shape("box", [1, 0.03, 1], [0.15, 0.9, 0.8]);
const PLATFORM = shape("box", [2.2, 0.35, 2.2], [0.85, 0.7, 0.4]);
const ROUTE_POLE = shape("cylinder", [0.15, 0.8, 0.15], [1, 0.85, 0.15]);
const BAY = shape("box", [2, 3, 0.4], null);
const PIER = shape("box", [0.45, 3.2, 0.8], [0.55, 0.5, 0.45]);

/** The demos: drawDemo's names. */
export const demoNames = [
  "select",
  "box_select",
  "move",
  "rotate",
  "scale",
  "snap",
  "place",
  "build",
  "prefab",
  "camera",
  "bounce",
  "launch",
  "replace",
  "story",
  "local",
  "free_rotate",
  "navmesh",
  "route",
  "spawn",
  "grow",
  "creature",
];

/**
 * Draws demo `name` at `time` seconds (it loops) on `ctx`, w x h pixels (its transform already set
 * for the device's pixels). False for a demo that does not exist (nothing drawn).
 */
export function drawDemo(
  ctx: CanvasRenderingContext2D,
  name: string,
  time: number,
  w: number,
  h: number,
): boolean {
  if (!demoNames.includes(name)) return false;
  const cursor = (at: V2) => pic.overlay.push({ kind: "cursor", at });
  const click = (at: V2, k: number) =>
    k > 0 && k < 1 && pic.overlay.push({ kind: "click", at, k });
  const label = (at: V2, text: string) =>
    pic.overlay.push({ kind: "label", at, text });
  const rect = (at: V2, to: V2, style: "" | "bar" | "knob") =>
    pic.overlay.push({ kind: "rect", at, to, style });
  // what the gizmo shows while it is dragged, next to the cursor
  const readout = (at: V2, format: string, value: number) =>
    label([at[0] + 14, at[1] + 10], printf(format, [value]));
  const corner: V2 = [8, h - 26];
  const start: V2 = [w * 0.78, h * 0.92]; // where the cursor comes from

  let t = 0;
  const period = (p: number) => (t = time % p);
  const cam = new Camera();
  cam.fov = 45;
  cam.pos = [7, 5.5, 9];
  cam.lookAt([0, 0.8, 0]);
  if (name === "build" || name === "story") {
    // taller
    cam.pos = [8, 7, 11];
    cam.lookAt([0, 2.2, 0]);
  } else if (name === "replace" || name === "launch") {
    // wider
    cam.pos = [0, 7, 13];
    cam.lookAt([0, 1, 0]);
  } else if (name === "camera") {
    // flying around
    period(6);
    const a = (t / 6) * 2 * Math.PI;
    cam.pos = [Math.sin(a) * 11, 5.5 + Math.sin(a * 2) * 1.5, Math.cos(a) * 11];
    cam.lookAt([0, 0.8, 0]);
  }
  const pic = new Picture(cam, w, h);
  const mesh = (s: Shape, world: M, tint = NONE, alpha = 1) =>
    pic.mesh(s.mesh, world, s.color ?? pic.wall, tint, alpha);
  const screen = (p: V3) => pic.screen(p);
  pic.mesh(GROUND.mesh, translate([0, -0.2, 0]), GROUND.color!, NONE, 1, true);

  if (name === "select") {
    period(4);
    const a: V3 = [-1.6, 0, 0];
    const b: V3 = [2, 0, -1];
    mesh(BOX, translate(a), t > 1.1 && t < 3 ? SELECTED : NONE);
    mesh(PILLAR, translate(b), t >= 3 ? SELECTED : NONE);
    const pa = screen(add(a, [0, 0.8, 0]));
    const pb = screen(add(b, [0, 1.5, 0]));
    cursor(
      t < 2.4
        ? mix2(start, pa, ease(t, 0.2, 1))
        : mix2(pa, pb, ease(t, 2.4, 2.95)),
    );
    click(pa, (t - 1.1) / 0.4);
    click(pb, (t - 3) / 0.4);
    label(corner, "guide.demo.select");
  } else if (name === "box_select") {
    period(4);
    const selected = t > 2.2;
    for (const x of [-2.5, 0, 2.5])
      mesh(BOX, translate([x, 0, -x / 5]), selected ? SELECTED : NONE); // (-2.5, 0, 0.5), (0, 0, 0), (2.5, 0, -0.5)
    const a = screen([-3.8, 2.4, 1.6]);
    const b = screen([3.8, -0.2, -1.6]);
    const p =
      t < 0.8 ? mix2(start, a, ease(t, 0, 0.8)) : mix2(a, b, ease(t, 0.8, 2.2));
    if (t > 0.8 && t < 2.2) rect(a, p, "");
    cursor(p);
    label(corner, "guide.demo.box_select");
  } else if (name === "move" || name === "snap") {
    period(4);
    let dx = 3 * ease(t, 1, 2.8);
    if (name === "snap") dx = Math.floor(dx + 0.5); // by steps of 1 m
    const at: V3 = [-1.5 + dx, 0, 0];
    const pivot = add(at, [0, 0.8, 0]);
    mesh(BOX, translate(at), SELECTED);
    pic.gizmoAxes(pivot, 1.8, t > 0.8 && t < 3 ? 0 : -1);
    const grip = screen(add(pivot, [1.4, 0, 0]));
    cursor(t < 0.9 ? mix2(start, grip, ease(t, 0, 0.9)) : grip);
    if (t > 1 && t < 3.2) readout(grip, "X %+.2f m", dx);
    label(corner, name === "snap" ? "guide.demo.snap" : "guide.demo.move");
  } else if (name === "rotate") {
    period(4);
    const k = ease(t, 1, 2.8);
    const pivot: V3 = [0, 0.8, 0];
    mesh(BOX, rotate(axisAngle([0, 1, 0], radians(45) * k)), SELECTED);
    pic.gizmoRings(pivot, 1.6, t > 0.8 && t < 3 ? 1 : -1);
    const a = radians(-30 + 45 * k);
    const grip = screen(add(pivot, mul([Math.sin(a), 0, Math.cos(a)], 1.6)));
    cursor(t < 0.9 ? mix2(start, grip, ease(t, 0, 0.9)) : grip);
    if (t > 1 && t < 3.2) readout(grip, "Y %+.0f°", 45 * k);
    label(corner, "guide.demo.rotate");
  } else if (name === "scale") {
    period(4);
    const k = 1 + ease(t, 1, 2.8);
    const pivot: V3 = [0, 0.8, 0];
    mesh(BOX, scale([k, 1, 1]), SELECTED);
    pic.gizmoAxes(pivot, 1 + 0.8 * k, t > 0.8 && t < 3 ? 0 : -1);
    const grip = screen(add(pivot, [1 + 0.8 * k, 0, 0]));
    cursor(t < 0.9 ? mix2(start, grip, ease(t, 0, 0.9)) : grip);
    if (t > 1 && t < 3.2) readout(grip, "X x%.2f", k);
    label(corner, "guide.demo.scale");
  } else if (name === "local") {
    // a catwalk turned: in the world's axes the arrow goes across it, in its own along it
    period(8);
    const local = t >= 4;
    const s = local ? t - 4 : t;
    const turn = axisAngle([0, 1, 0], radians(35));
    const d = 2.2 * ease(s, 1, 2.8);
    const dir: V3 = local ? qrotate(turn, [1, 0, 0]) : [1, 0, 0];
    const at = add([-1.2, 0, 0.6], mul(dir, d));
    const pivot = add(at, [0, 0.8, 0]);
    mesh(WALL, chain(translate(at), rotate(turn)), SELECTED);
    pic.gizmoAxes(
      pivot,
      1.8,
      s > 0.8 && s < 3 ? 0 : -1,
      local ? turn : NO_TURN,
    );
    const grip = screen(add(pivot, mul(dir, 1.4)));
    cursor(s < 0.9 ? mix2(start, grip, ease(s, 0, 0.9)) : grip);
    label(corner, local ? "guide.demo.local" : "guide.demo.world");
  } else if (name === "free_rotate") {
    // the right button held on the selection: the mouse turns it every way
    period(6);
    const k = ease(t, 0.9, 5);
    const yaw = Math.sin(k * 2 * Math.PI) * 1.1;
    const pitch = Math.sin(k * 4 * Math.PI) * 0.5;
    const pivot: V3 = [0, 0.8, 0];
    const r = qnormalized(
      qmul(axisAngle([0, 1, 0], yaw), axisAngle([1, 0, -1], pitch)),
    );
    mesh(
      BOX,
      chain(translate(pivot), rotate(r), translate(mul(pivot, -1))),
      SELECTED,
    );
    const on = screen(add(pivot, [0.2, 0.3, 0.3]));
    const swirl: V2 = [
      Math.sin(k * 2 * Math.PI) * 40,
      Math.sin(k * 4 * Math.PI) * 18,
    ];
    cursor(
      t < 0.9
        ? mix2(start, on, ease(t, 0, 0.9))
        : [on[0] + swirl[0], on[1] + swirl[1]],
    );
    label(corner, "guide.demo.free_rotate");
  } else if (name === "place") {
    // the piece follows the cursor on the ground; each click leaves a copy
    period(4);
    const along = (s: number) => mix([-3, 0, 1], [3, 0, -1], ease(s, 0, 3.6));
    if (t > 1.3) mesh(BOX, translate(along(1.3)));
    if (t > 2.7) mesh(BOX, translate(along(2.7)));
    mesh(BOX, translate(along(t)), [0.35, 0.75, 1, 0.25], 0.55);
    click(screen(add(along(1.3), [0, 0.8, 0])), (t - 1.3) / 0.4);
    click(screen(add(along(2.7), [0, 0.8, 0])), (t - 2.7) / 0.4);
    cursor(screen(add(along(t), [0, 0.8, 0])));
    label(corner, "guide.demo.place");
  } else if (name === "build") {
    // walls rise one after the other, a roof on them, then the palette's materials on the selection
    period(6);
    const colors: V3[] = [
      [0.7, 0.85, 1],
      [1, 0.75, 0.55],
      [0.7, 0.95, 0.7],
    ];
    const painting = t > 2.4;
    if (painting) pic.wall = colors[Math.floor((t - 2.4) / 1.2) % 3];
    const tint = painting ? SELECTED : NONE;
    const rise = (s: Shape, at: M, from: number) => {
      const k = ease(t, from, from + 0.4);
      if (k > 0) mesh(s, chain(at, scale([1, k, 1])), tint);
    };
    const quarter = rotate(axisAngle([0, 1, 0], radians(90)));
    rise(WALL, translate([0, 0, 1.8]), 0.1);
    rise(BACK, translate([0, 0, -1.8]), 0.4);
    rise(SIDE, chain(translate([-1.8, 0, 0]), quarter), 0.7);
    rise(SIDE, chain(translate([1.8, 0, 0]), quarter), 1);
    if (t > 1.5) mesh(ROOF, translate([0, 3 + 2 * (1 - ease(t, 1.5, 1.9)), 0]));
    label(corner, painting ? "guide.demo.build_paint" : "guide.demo.build");
  } else if (name === "prefab") {
    // a group selected and saved, then placed again elsewhere
    period(5);
    const group = (at: V3, tint: V4) => {
      mesh(BOX, translate(add(at, [-1.2, 0, 0])), tint);
      mesh(PILLAR, translate(add(at, [1, 0, -0.4])), tint);
      mesh(PAD, translate(add(at, [0, 0, 1.4])), tint);
    };
    group([-2, 0, 0], t > 0.6 && t < 2 ? SELECTED : NONE);
    if (t > 2.4) group(mix([8, 0, -4], [3.2, 0, -1], ease(t, 2.4, 3.6)), NONE);
    label(corner, t < 2.2 ? "guide.demo.prefab" : "guide.demo.prefab_place");
  } else if (name === "camera") {
    mesh(BOX, translate([-1.6, 0, 0]));
    mesh(PILLAR, translate([2, 0, -1]));
    mesh(WALL, translate([0, 0, -3]));
    label(corner, "guide.demo.camera");
  } else if (name === "bounce") {
    // falls on the pad, thrown up, back down
    period(3);
    mesh(PAD, IDENTITY);
    let y: number;
    if (t < 0.5) {
      y = 3 - 3 * (t / 0.5) * (t / 0.5);
    } else {
      const u = (t - 0.5) / 2.3;
      y = Math.max(0, 18 * u * (1 - u));
    }
    mesh(BALL, translate([0, 0.3 + y, 0]));
    label(corner, "guide.demo.bounce");
  } else if (name === "launch") {
    // thrown from the pad onto the block
    period(3.5);
    mesh(PAD, translate([-3, 0, 0]));
    mesh(BOX, chain(translate([3.5, 0, 0]), scale([1.4, 1, 1.4])));
    let p: V3;
    if (t < 0.8) {
      p = mix([-5.5, 0, 0], [-3, 0.3, 0], t / 0.8);
    } else {
      const u = clamp01((t - 0.8) / 1.5);
      p = add(mix([-3, 0.3, 0], [3.5, 1.6, 0], u), [0, 12 * u * (1 - u), 0]);
    }
    mesh(BALL, translate(p));
    label(corner, "guide.demo.launch");
  } else if (name === "replace") {
    // the original in blue between its neighbors; the new level slides and turns into its place
    period(5);
    mesh(TOWER, chain(translate([-5, 0, 0]), scale([1.5, 0.6, 2])));
    mesh(TOWER, chain(translate([5, 0, 0]), scale([1.5, 0.6, 2])));
    mesh(WALL, IDENTITY, [0.3, 0.55, 1, 0.6], 0.45);
    const k = ease(t, 0.5, 2.8);
    const turn = rotate(axisAngle([0, 1, 0], radians(90 * (1 - k))));
    mesh(
      WALL,
      chain(translate(mix([0, 0, 6], [0, 0, 0], k)), turn),
      t < 3.2 ? SELECTED : NONE,
    );
    label(corner, "guide.demo.replace");
  } else if (name === "story") {
    // the story's slider: the roof is there at the start of the game, gone later
    period(5);
    mesh(TOWER, IDENTITY);
    const before = t < 2.5;
    if (before) mesh(CAP, translate([0, 5, 0]));
    const a: V2 = [20, 14];
    const b: V2 = [w - 20, 18];
    rect(a, b, "bar");
    const x = a[0] + (b[0] - a[0]) * (0.25 + 0.55 * ease(t, 2.1, 2.5));
    rect([x - 5, a[1] - 5], [x + 5, b[1] + 5], "knob");
    cursor([x, b[1] + 2]);
    label(
      corner,
      before ? "guide.demo.story_before" : "guide.demo.story_after",
    );
  } else if (name === "navmesh" || name === "creature") {
    // the walkable ground appears around two crates, a creature walks on it between them; or
    // (creature) on that ground, a click puts the orange flag the creature appears at
    const flagged = name === "creature";
    period(flagged ? 6 : 5);
    mesh(BOX, translate([-1.5, 0, -0.5]));
    mesh(BOX, translate([1.8, 0, 1]));
    const k = flagged ? 1 : ease(t, 0.6, 1.6);
    if (k > 0) {
      // the cells of the nav mesh: not under the crates nor too close to them
      const near = (x: number, z: number, cx: number, cz: number) =>
        Math.abs(x - cx) < 1.6 && Math.abs(z - cz) < 1.6;
      for (let x = -5; x <= 5; x += 1.25)
        for (let z = -4; z <= 4; z += 1.25) {
          if (near(x, z, -1.5, -0.5) || near(x, z, 1.8, 1)) continue;
          mesh(
            NAV,
            chain(translate([x, 0.01, z]), scale([1.1, 1, 1.1])),
            NONE,
            0.35 * k,
          );
        }
    }
    const path: V3[] = [
      [-4, 0, 3],
      [-0.2, 0, 1.6],
      [0.2, 0, -2.6],
      [4, 0, -3],
    ];
    const born = flagged ? 2.6 : 1.8;
    if (flagged) {
      // the cursor to the ground, a click: the flag
      const at = screen(path[0]);
      cursor(t < 1.2 ? mix2(start, at, ease(t, 0.2, 1.2)) : at);
      click(at, (t - 1.3) / 0.4);
      if (t > 1.4) {
        mesh(POLE, translate(add(path[0], [-0.6, 0, -0.4])));
        mesh(SPAWN_FLAG, translate(add(path[0], [-0.15, 1.45, -0.4])));
      }
    }
    if (t > born) {
      const u = ease(t, born, born + 2.8);
      const seg = Math.min(2, Math.floor(u * 3));
      mesh(BALL, translate(mix(path[seg], path[seg + 1], u * 3 - seg)));
    }
    label(
      corner,
      flagged
        ? t < born
          ? "guide.demo.creature_place"
          : "guide.demo.creature_walk"
        : k < 1
          ? "guide.demo.navmesh"
          : "guide.demo.navmesh_walk",
    );
  } else if (name === "route") {
    // three clicks give a platform its route; then it goes along it
    period(7);
    const points: V3[] = [
      [-3.5, 0.6, 1],
      [0, 0.6, 2.5],
      [2.5, 2.2, -0.5],
      [3.5, 3.2, -3],
    ];
    const clicks = [1.2, 2, 2.8];
    const shown = 1 + clicks.filter((c) => t > c).length;
    for (let i = 0; i + 1 < shown; i++)
      pic.lines.push({
        a: screen(points[i]),
        b: screen(points[i + 1]),
        color: "#ffd933",
      });
    for (let i = 1; i < shown; i++) mesh(ROUTE_POLE, translate(points[i]));
    let at = points[0];
    if (t > 3.4) {
      const u = ease(t, 3.4, 6.4) * 3;
      const seg = Math.min(2, Math.floor(u));
      at = mix(points[seg], points[seg + 1], u - seg);
    }
    mesh(PLATFORM, translate(add(at, [0, -0.35, 0])), t < 3.2 ? SELECTED : NONE);
    clicks.forEach((c, i) => click(screen(points[i + 1]), (t - c) / 0.4));
    const aim = clicks.findIndex((c) => t < c);
    cursor(
      aim < 0
        ? screen(points[3])
        : mix2(
            aim === 0 ? start : screen(points[aim]),
            screen(points[aim + 1]),
            ease(t, aim === 0 ? 0.2 : clicks[aim - 1] + 0.2, clicks[aim]),
          ),
    );
    label(corner, t < 3.2 ? "guide.demo.route" : "guide.demo.route_go");
  } else if (name === "spawn") {
    // Jak starts at the blue flag, passes the green one, falls into the red zone: back at the green
    period(6);
    for (const p of [
      [-4, 0, 0],
      [0, 0.5, 0],
      [4, 1, 0],
    ] as V3[])
      mesh(STEP, translate(p));
    mesh(ZONE, translate([0, -3, 0]), NONE, 0.45);
    const flag = (at: V3, f: Shape) => {
      mesh(POLE, translate(at));
      mesh(f, translate(add(at, [0.45, 1.45, 0])));
    };
    flag([-4.6, 0.5, -0.6], START_FLAG);
    flag([-0.6, 1, -0.6], CHECK_FLAG);
    let p: V3;
    if (t < 1.2) p = [-4, 0.5, 0];
    else if (t < 2.2) {
      const u = t - 1.2;
      p = add(mix([-4, 0.5, 0], [0, 1, 0], u), [0, 3 * u * (1 - u), 0]);
    } else if (t < 3.6) {
      // a jump too short: he falls
      const u = (t - 2.2) / 1.4;
      p = add(mix([0, 1, 0], [2.6, -3, 0], u), [0, 3 * u * (1 - u), 0]);
    } else p = [0, 1, 0];
    if (!(t > 3.5 && t < 3.9)) mesh(BALL, translate(p));
    label(
      corner,
      t < 1.2
        ? "guide.demo.spawn_start"
        : t < 3.6
          ? "guide.demo.spawn_fall"
          : "guide.demo.spawn_back",
    );
  } else if (name === "grow") {
    // a wall stretched by its end: more bays and pillars, not a wider texture
    period(5);
    const len = 4 + 6 * ease(t, 1, 3.4);
    const bays = Math.max(2, Math.round(len / 2));
    const b = len / bays;
    for (let i = 0; i < bays; i++)
      mesh(
        BAY,
        chain(translate([-len / 2 + b * (i + 0.5), 0, 0]), scale([b / 2, 1, 1])),
        SELECTED,
      );
    for (let i = 0; i <= bays; i++)
      mesh(PIER, translate([-len / 2 + b * i, 0, 0]), SELECTED);
    const tip: V3 = [len / 2, 1.5, 0];
    pic.gizmoAxes(tip, 0.8, t > 0.8 && t < 3.6 ? 0 : -1);
    const grip = screen(add(tip, [0.6, 0, 0]));
    cursor(t < 0.9 ? mix2(start, grip, ease(t, 0, 0.9)) : grip);
    if (t > 1 && t < 3.6) readout(grip, "%.1f m", len);
    label(corner, "guide.demo.grow");
  }
  pic.draw(ctx);
  return true;
}

/**
 * Draws a shape seen from above at three quarters, w x h pixels (the Build panel's buttons,
 * ToolDemo::icon): a preset's name (shapePresets) or its params. False for a preset that does not exist.
 */
export function drawIcon(
  ctx: CanvasRenderingContext2D,
  shape: string | ShapeParams,
  w: number,
  h: number,
): boolean {
  const params =
    typeof shape === "string"
      ? shapePresets.find((p) => p.name === shape)?.params
      : shape;
  if (!params) return false;
  const m = makeShape(params);
  const cam = new Camera();
  cam.fov = 35;
  cam.yaw = 0.65;
  cam.pitch = -0.5;
  cam.frame(m.lo, m.hi);
  const pic = new Picture(cam, w, h);
  pic.mesh(m, IDENTITY, WALL_COLOR);
  pic.draw(ctx);
  return true;
}
