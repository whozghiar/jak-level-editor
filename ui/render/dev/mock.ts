// A pretend engine for `npx vite` in a browser (no Tauri): a small scene encoded in the jle://
// protocol's exact formats (JLES, JLEM, texture, materials, the game view's JLGL, JLGC, JLGP), and
// the calls the viewport makes (pick, set_camera_state, transform_nodes, game_mood) plus `select`
// and `set_view` for the dev page. `?game=<hour>` starts in the game view at that hour. Two
// effects run (`effect_emitters`: ctyindb's steam, a city light; the particles from the dev
// server's data/, the sprites a white disc); `?fx=1` places the camera on them.

import { Camera } from "../camera";
import type { transport as Transport } from "../engine";
import {
  AlphaBlend,
  DrawFlag,
  gameCamera,
  gameView,
  parseChunk,
  parseChunkList,
  parsePalette,
  type ChunkDraw,
  type GameMood,
} from "../game_view";
import {
  aabb,
  aabbAdd,
  aabbTransformed,
  axisAngle,
  identity,
  inverse,
  matMul,
  point,
  rotate,
  scale,
  translate,
  type AABB,
  type Mat4,
  type Vec3,
} from "../math";
import { parseMaterials, parseMesh, parseScene } from "../protocol";
import {
  AlphaMode,
  BlendMode,
  NodeFlag,
  NodeKind,
  type Material,
} from "../types";

interface Prim {
  pos: number[];
  nrm: number[];
  uv?: number[];
  col?: number[]; // bytes
  idx: number[];
  material: number;
  backdrop?: boolean;
}
interface MockNode {
  id: number;
  parent: number;
  kind: number;
  flags: number;
  extra: number;
  mesh: number;
  version: number;
  world: Mat4;
}

// JLEM 1, each primitive's header followed by its arrays (headers_first: all the headers first)
export function encodeMesh(prims: Prim[], headers_first = false): ArrayBuffer {
  let size = 12;
  for (const p of prims)
    size +=
      16 +
      (p.pos.length / 3) * (24 + (p.uv ? 8 : 0) + (p.col ? 4 : 0)) +
      p.idx.length * 4;
  const buf = new ArrayBuffer(size),
    dv = new DataView(buf);
  [..."JLEM"].forEach((c, i) => dv.setUint8(i, c.charCodeAt(0)));
  dv.setUint32(4, 1, true);
  dv.setUint32(8, prims.length, true);
  let o = headers_first ? 12 + prims.length * 16 : 12;
  const f32 = (a: number[]) =>
    a.forEach((v) => (dv.setFloat32(o, v, true), (o += 4)));
  prims.forEach((p, i) => {
    const h = headers_first ? 12 + i * 16 : o;
    dv.setUint32(h, p.pos.length / 3, true);
    dv.setUint32(h + 4, p.idx.length, true);
    dv.setInt32(h + 8, p.material, true);
    dv.setUint32(
      h + 12,
      (p.uv ? 1 : 0) | (p.col ? 2 : 0) | (p.backdrop ? 4 : 0),
      true,
    );
    if (!headers_first) o += 16;
    f32(p.pos);
    f32(p.nrm);
    if (p.uv) f32(p.uv);
    if (p.col) p.col.forEach((v) => dv.setUint8(o++, v));
    p.idx.forEach((v) => (dv.setUint32(o, v, true), (o += 4)));
  });
  return buf;
}

// the parser reads what the encoders write, both mesh layouts (run when the mock starts)
function selfcheck() {
  const prims = [
    box(2),
    {
      ...quad(1, 2, 2),
      col: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    },
  ];
  for (const headers_first of [false, true]) {
    const m = parseMesh(encodeMesh(prims, headers_first), 7, 3);
    console.assert(
      m.prims.length === 2 &&
        m.prims[1].indices === 6 &&
        m.prims[1].material === 1,
      "JLEM prims",
      headers_first,
    );
    console.assert(
      new Uint8Array(m.buffer, m.prims[1].col, 16)[15] === 16,
      "JLEM colors",
      headers_first,
    );
    console.assert(
      m.bounds.lo[0] === -1 && m.bounds.hi[1] === 2,
      "JLEM bounds",
      headers_first,
    );
  }
  const node: MockNode = {
    id: 5,
    parent: 2,
    kind: 1,
    flags: 33,
    extra: 7,
    mesh: 9,
    version: 4,
    world: translate([1, 2, 3]),
  };
  const sc = parseScene(encodeScene(false, [node], [11, 12]));
  console.assert(
    !sc.full &&
      sc.nodes[0].id === 5 &&
      sc.nodes[0].flags === 33 &&
      sc.nodes[0].extra === 7,
    "JLES record",
  );
  console.assert(
    sc.nodes[0].world[13] === 2 && sc.removed.join() === "11,12",
    "JLES world, removed",
  );
}

// JLES 1
export function encodeScene(
  full: boolean,
  nodes: MockNode[],
  removed: number[],
): ArrayBuffer {
  const buf = new ArrayBuffer(16 + nodes.length * 84 + 4 + removed.length * 4),
    dv = new DataView(buf);
  [..."JLES"].forEach((c, i) => dv.setUint8(i, c.charCodeAt(0)));
  dv.setUint32(4, 1, true);
  dv.setUint32(8, full ? 1 : 0, true);
  dv.setUint32(12, nodes.length, true);
  let o = 16;
  for (const n of nodes) {
    dv.setUint32(o, n.id, true);
    dv.setUint32(o + 4, n.parent, true);
    dv.setUint8(o + 8, n.kind);
    dv.setUint8(o + 9, n.flags);
    dv.setUint16(o + 10, n.extra, true);
    dv.setUint32(o + 12, n.mesh, true);
    dv.setUint32(o + 16, n.version, true);
    n.world.forEach((v, i) => dv.setFloat32(o + 20 + i * 4, v, true));
    o += 84;
  }
  dv.setUint32(o, removed.length, true);
  removed.forEach((id, i) => dv.setUint32(o + 4 + i * 4, id, true));
  return buf;
}

function texture(
  w: number,
  h: number,
  px: (x: number, y: number) => [number, number, number, number],
): ArrayBuffer {
  const buf = new ArrayBuffer(8 + w * h * 4),
    dv = new DataView(buf),
    b = new Uint8Array(buf);
  dv.setUint32(0, w, true);
  dv.setUint32(4, h, true);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) b.set(px(x, y), 8 + (y * w + x) * 4);
  return buf;
}

// ---------------------------------------------------------------------------------------------
// the scene

function grid(
  size: number,
  cells: number,
  y: (x: number, z: number) => number,
  col: (x: number, z: number) => number[],
): Prim {
  const p: Prim = { pos: [], nrm: [], col: [], uv: [], idx: [], material: 0 };
  for (let j = 0; j <= cells; j++)
    for (let i = 0; i <= cells; i++) {
      const x = (i / cells - 0.5) * size,
        z = (j / cells - 0.5) * size;
      p.pos.push(x, y(x, z), z);
      p.nrm.push(0, 1, 0);
      p.uv!.push(i / 4, j / 4);
      p.col!.push(...col(x, z));
    }
  for (let j = 0; j < cells; j++)
    for (let i = 0; i < cells; i++) {
      const a = j * (cells + 1) + i;
      p.idx.push(a, a + cells + 1, a + 1, a + 1, a + cells + 1, a + cells + 2);
    }
  return p;
}

function box(material: number, colors?: number[]): Prim {
  const p: Prim = { pos: [], nrm: [], idx: [], material, uv: [] };
  if (colors) p.col = [];
  const faces: [Vec3, Vec3, Vec3][] = [
    [
      [1, 0, 0],
      [0, 1, 0],
      [0, 0, 1],
    ],
    [
      [-1, 0, 0],
      [0, 1, 0],
      [0, 0, -1],
    ],
    [
      [0, 1, 0],
      [0, 0, 1],
      [1, 0, 0],
    ],
    [
      [0, -1, 0],
      [0, 0, -1],
      [1, 0, 0],
    ],
    [
      [0, 0, 1],
      [1, 0, 0],
      [0, 1, 0],
    ],
    [
      [0, 0, -1],
      [-1, 0, 0],
      [0, 1, 0],
    ],
  ];
  faces.forEach(([n, u, v], f) => {
    const base = p.pos.length / 3;
    for (const [a, b] of [
      [-1, -1],
      [1, -1],
      [1, 1],
      [-1, 1],
    ]) {
      p.pos.push(
        n[0] + u[0] * a + v[0] * b,
        n[1] + u[1] * a + v[1] * b,
        n[2] + u[2] * a + v[2] * b,
      );
      p.nrm.push(...n);
      p.uv!.push((a + 1) / 2, (b + 1) / 2);
      if (colors)
        p.col!.push(
          ...colors.slice(
            (f % (colors.length / 4)) * 4,
            (f % (colors.length / 4)) * 4 + 4,
          ),
        );
    }
    // counter-clockwise seen from outside: u x v = n
    p.idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
  });
  return p;
}

function quad(material: number, w: number, h: number): Prim {
  return {
    pos: [-w / 2, 0, 0, w / 2, 0, 0, w / 2, h, 0, -w / 2, h, 0],
    nrm: [0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1],
    uv: [0, 1, 1, 1, 1, 0, 0, 0],
    idx: [0, 1, 2, 0, 2, 3],
    material,
  };
}

function ring(radius: number, height: number, segments: number): Prim {
  // the far mountains: a jagged wall around the level, colored, fogged by distance
  const p: Prim = {
    pos: [],
    nrm: [],
    col: [],
    idx: [],
    material: 0,
    backdrop: true,
  };
  for (let i = 0; i <= segments; i++) {
    const a = (i / segments) * Math.PI * 2;
    const x = Math.sin(a) * radius,
      z = Math.cos(a) * radius;
    const top = height * (0.6 + 0.4 * Math.abs(Math.sin(i * 1.7)));
    p.pos.push(x, -5, z, x * 1.05, top, z * 1.05);
    p.nrm.push(-Math.sin(a), 0, -Math.cos(a), -Math.sin(a), 0, -Math.cos(a));
    p.col!.push(70, 80, 60, 128, 110, 105, 100, 128);
    if (i < segments) {
      const b = i * 2;
      p.idx.push(b, b + 2, b + 1, b + 1, b + 2, b + 3);
    }
  }
  return p;
}

const MATERIALS = [
  { name: "ground", texture: -1, color: [1, 1, 1, 1], alpha: "opaque" },
  { name: "checker", texture: 0, color: [1, 1, 1, 1], alpha: "opaque" },
  {
    name: "concrete",
    texture: -1,
    color: [0.72, 0.74, 0.78, 1],
    alpha: "opaque",
  },
  {
    name: "glass",
    texture: -1,
    color: [0.6, 0.8, 1, 0.35],
    alpha: "blend",
    blend: "alpha",
    cutoff: 0,
  },
  {
    name: "glow",
    texture: -1,
    color: [1, 0.55, 0.2, 1],
    alpha: "blend",
    blend: "add",
    cutoff: 0,
  },
  {
    name: "water",
    texture: 1,
    color: [1, 1, 1, 0.75],
    alpha: "blend",
    blend: "alpha",
    cutoff: 0,
    water: true,
  },
  {
    name: "fence",
    texture: 2,
    color: [1, 1, 1, 1],
    alpha: "mask",
    cutoff: 0.5,
  },
];

const TEXTURES = [
  texture(64, 64, (x, y) =>
    ((x >> 3) + (y >> 3)) % 2 ? [230, 200, 60, 255] : [60, 70, 160, 255],
  ),
  texture(32, 32, (x, y) => {
    const s = Math.sin((x + y) * 0.4) * 0.5 + 0.5;
    return [30 + s * 40, 110 + s * 60, 150 + s * 60, 255];
  }),
  texture(32, 32, (x, y) =>
    x % 8 < 2 || y % 8 < 2 ? [150, 150, 140, 255] : [0, 0, 0, 0],
  ),
];

function scene() {
  const hue = (x: number, z: number) => {
    // PS2 vertex colors: 128 is neutral (the shader doubles them)
    const t = Math.sin(x * 0.05) * 0.5 + 0.5,
      u = Math.cos(z * 0.04) * 0.5 + 0.5;
    return [
      Math.round(60 + 40 * t),
      Math.round(90 + 30 * u),
      Math.round(50 + 20 * t),
      128,
    ];
  };
  const meshes = new Map<number, ArrayBuffer>([
    [
      1,
      encodeMesh([
        grid(
          200,
          40,
          (x, z) => Math.sin(x * 0.08) * Math.cos(z * 0.06) * 1.5 - 0.2,
          hue,
        ),
      ]),
    ],
    [2, encodeMesh([box(2)])],
    [3, encodeMesh([quad(1, 4, 4)])],
    [
      4,
      encodeMesh([
        box(0, [128, 40, 40, 128, 40, 128, 40, 128, 40, 40, 128, 128]),
        {
          ...quad(3, 3, 3),
          pos: quad(3, 3, 3).pos.map((v, i) => (i % 3 === 2 ? 1.6 : v)),
        },
      ]),
    ],
    [
      5,
      encodeMesh([
        {
          ...grid(
            24,
            4,
            () => 0,
            () => [128, 128, 128, 128],
          ),
          material: 5,
          col: undefined,
        },
      ]),
    ],
    [6, encodeMesh([ring(260, 60, 48)])],
    [7, encodeMesh([quad(4, 2, 5)])],
    [8, encodeMesh([quad(6, 6, 2.5)])],
  ]);
  const V = NodeFlag.Visible;
  const n = (
    id: number,
    parent: number,
    kind: number,
    flags: number,
    mesh: number,
    world: Mat4,
    extra = 0,
  ): MockNode => ({
    id,
    parent,
    kind,
    flags,
    extra,
    mesh,
    version: 1,
    world,
  });
  const at = (x: number, y: number, z: number, yaw = 0, s: Vec3 = [1, 1, 1]) =>
    matMul(
      matMul(translate([x, y, z]), rotate(axisAngle([0, 1, 0], yaw))),
      scale(s),
    );
  const R = NodeKind.Render;
  const nodes: MockNode[] = [
    n(1, 0, NodeKind.Group, V, 0, identity()),
    n(2, 1, R, V, 1, identity()),
    n(3, 1, R, V | NodeFlag.Selected, 2, at(0, 1, 0)),
    n(4, 1, R, V, 2, at(6, 1, -4, 0.5)),
    n(5, 1, R, V, 2, at(-6, 2, -6, 0.2, [1, 2, 1])),
    n(6, 1, R, V, 2, at(12, 0.5, 4, 0, [3, 0.5, 2])),
    n(7, 1, R, V, 2, at(-14, 1, 8, 0.9)),
    n(8, 1, R, V, 3, at(0, 0, -12)),
    n(9, 1, R, V, 4, at(-8, 1, 6, 0.4)),
    n(10, 1, R, V | NodeFlag.Water, 5, at(25, 0.1, -20)),
    n(11, 1, R, V, 6, identity()),
    n(12, 1, NodeKind.Actor, V, 0, at(4, 1, 8, 1.2), 0x5a3c),
    n(13, 1, NodeKind.Actor, V, 0, at(-3, 0, 12)),
    n(
      14,
      13,
      R,
      V | NodeFlag.UnderActor,
      2,
      at(-3, 0.5, 12, 0, [0.5, 0.5, 0.5]),
    ),
    n(15, 0, NodeKind.Group, V | NodeFlag.Reference, 0, identity()),
    n(16, 15, R, V | NodeFlag.Reference, 4, at(-8, 1.3, 6.6, 0.4)),
    n(17, 1, R, V, 7, at(8, 0, -10)),
    n(18, 1, R, V, 8, at(-4, 0, -8)),
    n(19, 1, R, 0, 2, at(0, 6, 0)), // hidden
  ];
  return { meshes, nodes };
}

// ---------------------------------------------------------------------------------------------
// the game view (crates/core/src/game/view.rs): the decor's chunks in their formats, the mood

// game_mood (crates/core/src/game/mood.rs) at noon and at 22:00 in the game's start weather
// (cloud 0.25, fog 0.5)
const NOON: GameMood = {
  times: [
    [0.4199, 0.5195, 0.5976, 1],
    [1.8412, 1.685, 1.4374, 0],
    [1.8412, 1.685, 1.4374, 0.5],
    [1.8412, 1.685, 1.4374, 0.5],
    [1.8412, 1.685, 1.4374, 0],
    [1, 1, 1, 0],
    [1, 1, 1, 1],
    [1, 1, 1, 0.9],
  ],
  fog_color: [128, 180, 243],
  fog_start: 131072,
  fog_end: 4096000,
  fog_max: 255,
  fog_min: 162,
  sky_color: [89, 113, 200],
  light_ambient: [0.4199, 0.5195, 0.5976],
  light_dir: [
    [0.5, 0.814, 0.296],
    [-0.5, 0.814, 0.296],
    [0, 0, 0],
  ],
  light_color: [
    [0.9206, 0.8425, 0.7187],
    [0.9206, 0.8425, 0.7187],
    [0, 0, 0],
  ],
  sun_dir: [0, 0.95073, 0.31002],
  moon_dir: [0, -0.95073, 0.31002],
  sun_color: [1.8412, 1.685, 1.4374],
  moon_color: [0.1097, 0.384, 0.8232],
  cloud_min: 0.333,
  cloud_max: 0.555,
  cloud_dark: 0.0625,
  haze: 0,
  flash: 0,
};
const NIGHT: GameMood = {
  ...NOON,
  times: [
    [0.271825, 0.32555, 0.4614, 1],
    [0.165075, 0.32525, 0.7424, 0],
    [0.165075, 0.32525, 0.7424, 0.94],
    [0.165075, 0.32525, 0.7424, 0],
    [0.165075, 0.32525, 0.7424, 0.06],
    [1, 1, 1, 1],
    [1, 1, 1, 1],
    [1, 1, 1, 0.9],
  ],
  fog_color: [3.2, 20.8, 58.4],
  sky_color: [9.5, 12, 30],
  light_ambient: [0.271825, 0.32555, 0.4614],
  light_dir: [
    [0.5, 0.814, 0.296],
    [-0.906, 0.397, 0.143],
    [0, 0, 0],
  ],
  light_color: [
    [0.15517, 0.305735, 0.697856],
    [0.0099, 0.019515, 0.044544],
    [0, 0, 0],
  ],
  sun_dir: [-0.50642, -0.80698, 0.30385],
  moon_dir: [0.50642, 0.80698, 0.30385],
  sun_color: [0.165075, 0.32525, 0.7424],
};

interface MockChunk {
  id: number;
  version: number;
  palette: number;
  box: AABB;
  draws: ChunkDraw[];
  verts: number[]; // x, y, z (game units), s, t, color per vertex
  idx: number[];
  rgba: number[]; // its palette
}

// view.rs draw_mode: how the level builder draws a material
function drawMode(
  m: Material | null,
): Omit<ChunkDraw, "first" | "count" | "texture"> {
  let flags: number = DrawFlag.DepthWrite,
    aref = 0,
    blend: number = AlphaBlend.Disabled;
  if (!m) return { flags, aref, blend };
  const cutoff = m.base_color[3] > 1.5 ? Math.min(1, m.cutoff * 2) : m.cutoff;
  if (m.alpha !== AlphaMode.Opaque) {
    flags |= DrawFlag.AlphaTest;
    aref = Math.min(255, Math.max(0, Math.trunc(cutoff * 127)));
  }
  // translucent_in_game
  if (
    m.alpha === AlphaMode.Blend &&
    (m.blend !== BlendMode.Alpha || m.translucent || m.base_color[3] <= 1.5)
  ) {
    flags = (flags & ~DrawFlag.DepthWrite) | DrawFlag.Translucent;
    blend = [
      AlphaBlend.SrcDstSrcDst,
      AlphaBlend.Src0FixDst,
      AlphaBlend.Src0SrcDst,
      AlphaBlend.ZeroSrcSrcDst,
      AlphaBlend.SrcDstFixDst,
    ][m.blend];
  }
  // the editor's materials: their textures' alpha halved
  if (m.texture >= 0 && m.base_color[3] <= 1.5) flags |= DrawFlag.OwnTexture;
  return { flags, aref, blend };
}

// The decor's two chunks: the ground and the far mountains lit by a level's palettes (the ambient
// light, the sun, a lamp in the middle that the night lights), the rest new geometry lit from its
// color (tod_from_color, with a district of the city's ratios)
function gameChunks(
  nodes: MockNode[],
  meshes: Map<number, ArrayBuffer>,
): MockChunk[] {
  const materials = parseMaterials({ materials: MATERIALS }).materials;
  const ratios = [0.986, 0.117, 0.17, 0.164, 0.135, 0.007, 0.043, 0.037];
  const level = new Set([2, 11]);
  const byte = (v: number) => Math.min(255, Math.max(0, Math.round(v)));
  return [true, false].map((lit, i): MockChunk => {
    const c: MockChunk = {
      id: i + 1,
      version: i + 1,
      palette: 100 + i,
      box: aabb(),
      draws: [],
      verts: [],
      idx: [],
      rgba: [],
    };
    const colors = new Map<string, number>();
    const by_mat = new Map<number, number[]>(); // material * 2 + translucent
    for (const n of nodes) {
      if (
        n.kind !== NodeKind.Render ||
        !(n.flags & NodeFlag.Visible) ||
        n.flags & (NodeFlag.UnderActor | NodeFlag.Reference) ||
        level.has(n.id) !== lit
      )
        continue;
      const buf = meshes.get(n.mesh)!;
      for (const p of parseMesh(buf, n.mesh, 1).prims) {
        const pos = new Float32Array(buf, p.pos, p.vertices * 3);
        const uv =
          p.uv >= 0 ? new Float32Array(buf, p.uv, p.vertices * 2) : null;
        const col =
          p.col >= 0 ? new Uint8Array(buf, p.col, p.vertices * 4) : null;
        const mat = materials[p.material] ?? null;
        const base = c.verts.length / 6;
        for (let v = 0; v < p.vertices; v++) {
          const q = point(n.world, [
            pos[v * 3],
            pos[v * 3 + 1],
            pos[v * 3 + 2],
          ]);
          aabbAdd(c.box, q);
          const rgb = col
            ? [col[v * 4], col[v * 4 + 1], col[v * 4 + 2]]
            : [255, 255, 255];
          const lamp = Math.max(0, 1 - Math.hypot(q[0], q[2]) / 30);
          const pal: number[] = [];
          for (let k = 0; k < 8; k++)
            for (let ch = 0; ch < 4; ch++)
              pal.push(
                ch === 3
                  ? 128
                  : lit // the ambient light: the vertex's color; the sun some of it; the lamp
                    ? byte(
                        k === 0
                          ? rgb[ch]
                          : k <= 4
                            ? rgb[ch] * 0.45
                            : k === 5
                              ? [230, 150, 60][ch] * lamp
                              : 0,
                      )
                    : byte(
                        (rgb[ch] / 255) *
                          (mat ? mat.base_color[ch] : 1) *
                          ratios[k] *
                          127.5,
                      ),
              );
          const key = pal.join();
          if (!colors.has(key)) {
            colors.set(key, colors.size);
            c.rgba.push(...pal);
          }
          c.verts.push(
            q[0] * 4096,
            q[1] * 4096,
            q[2] * 4096,
            uv ? uv[v * 2] : 0,
            uv ? uv[v * 2 + 1] : 0,
            colors.get(key)!,
          );
        }
        const k =
          p.material * 2 + (drawMode(mat).flags & DrawFlag.Translucent ? 1 : 0);
        const list = by_mat.get(k) ?? [];
        by_mat.set(k, list);
        for (const ix of new Uint32Array(buf, p.idx, p.indices))
          list.push(ix + base);
      }
    }
    // the normal tree's draws, then the translucent tree's
    const keys = [...by_mat.keys()].sort((a, b) => (a & 1) - (b & 1) || a - b);
    for (const k of keys) {
      const mat = materials[k >> 1] ?? null;
      const idx = by_mat.get(k)!;
      c.draws.push({
        first: c.idx.length,
        count: idx.length,
        texture: mat ? mat.texture : -1,
        ...drawMode(mat),
      });
      c.idx.push(...idx);
    }
    return c;
  });
}

function encodeChunkList(chunks: MockChunk[]): ArrayBuffer {
  const dv = new DataView(new ArrayBuffer(12 + chunks.length * 40));
  [..."JLGL"].forEach((c, i) => dv.setUint8(i, c.charCodeAt(0)));
  dv.setUint32(4, 1, true);
  dv.setUint32(8, chunks.length, true);
  chunks.forEach((c, i) => {
    const o = 12 + i * 40;
    [c.id, c.version, c.palette, c.idx.length / 3].forEach((v, k) =>
      dv.setUint32(o + k * 4, v, true),
    );
    [...c.box.lo, ...c.box.hi].forEach((v, k) =>
      dv.setFloat32(o + 16 + k * 4, v, true),
    );
  });
  return dv.buffer as ArrayBuffer;
}

function encodeChunk(c: MockChunk): ArrayBuffer {
  const nv = c.verts.length / 6;
  const dv = new DataView(
    new ArrayBuffer(56 + c.draws.length * 16 + nv * 24 + c.idx.length * 4),
  );
  [..."JLGC"].forEach((ch, i) => dv.setUint8(i, ch.charCodeAt(0)));
  [1, c.id, c.version, c.palette, nv, c.idx.length, c.draws.length].forEach(
    (v, k) => dv.setUint32(4 + k * 4, v, true),
  );
  [...c.box.lo, ...c.box.hi].forEach((v, k) =>
    dv.setFloat32(32 + k * 4, v, true),
  );
  let o = 56;
  for (const d of c.draws) {
    dv.setUint32(o, d.first, true);
    dv.setUint32(o + 4, d.count, true);
    dv.setInt32(o + 8, d.texture, true);
    dv.setUint8(o + 12, d.flags);
    dv.setUint8(o + 13, d.aref);
    dv.setUint8(o + 14, d.blend);
    o += 16;
  }
  for (let v = 0; v < nv; v++, o += 24) {
    for (let k = 0; k < 5; k++)
      dv.setFloat32(o + k * 4, c.verts[v * 6 + k], true);
    dv.setUint16(o + 20, c.verts[v * 6 + 5], true);
  }
  c.idx.forEach((ix, i) => dv.setUint32(o + i * 4, ix, true));
  return dv.buffer as ArrayBuffer;
}

function encodePalette(rgba: number[]): ArrayBuffer {
  const b = new Uint8Array(12 + rgba.length);
  b.set([..."JLGP"].map((c) => c.charCodeAt(0)));
  const dv = new DataView(b.buffer);
  dv.setUint32(4, 1, true);
  dv.setUint32(8, rgba.length / 32, true);
  b.set(rgba, 12);
  return b.buffer;
}

// the game view reads what the encoders write; the game camera places things where the editor's
// does (selftest.cpp's game view)
function gameSelfcheck(chunks: MockChunk[]) {
  const list = parseChunkList(encodeChunkList(chunks));
  console.assert(
    list.length === 2 &&
      list[1].palette === 101 &&
      list[0].triangles === chunks[0].idx.length / 3,
    "JLGL",
  );
  const c = parseChunk(encodeChunk(chunks[1]));
  const v = new DataView(c.vertices.buffer, c.vertices.byteOffset);
  console.assert(
    c.draws.length === chunks[1].draws.length &&
      c.indices.length === chunks[1].idx.length &&
      v.getFloat32(0, true) === Math.fround(chunks[1].verts[0]) &&
      v.getUint16(20, true) === chunks[1].verts[5],
    "JLGC",
  );
  const p = parsePalette(encodePalette(chunks[0].rgba));
  console.assert(
    p.colors * 32 === chunks[0].rgba.length &&
      p.rgba[33] === chunks[0].rgba[33],
    "JLGP",
  );
  const cam = new Camera();
  cam.pos = [0, 8, 25];
  cam.lookAt([0, 0, 0]);
  const clip = gameCamera(cam, 16 / 9, NOON).clip,
    editor = matMul(cam.proj(16 / 9), cam.view());
  const project = (m: Mat4, q: Vec3) => {
    const x = [0, 1, 2, 3].map(
      (r) => m[r] * q[0] + m[4 + r] * q[1] + m[8 + r] * q[2] + m[12 + r],
    );
    return [x[0] / x[3], x[1] / x[3], x[2] / x[3], x[3]];
  };
  for (const q of [
    [5, 1, -3],
    [-8, 4, 2],
    [0, 0, 0],
  ] as Vec3[]) {
    const a = project(clip, q),
      b = project(editor, q);
    console.assert(
      a[3] > 0 &&
        Math.abs(a[0] - b[0]) < 1e-3 &&
        Math.abs(a[1] - b[1]) < 1e-3 &&
        a[2] > -1 &&
        a[2] < 1,
      "game camera",
      q,
      a,
      b,
    );
  }
}

// ---------------------------------------------------------------------------------------------

function slab(box: AABB, o: Vec3, d: Vec3): { t: number; normal: Vec3 } | null {
  let t0 = 0,
    t1 = Infinity;
  let normal: Vec3 = [0, 1, 0];
  for (let a = 0; a < 3; a++) {
    if (Math.abs(d[a]) < 1e-12) {
      if (o[a] < box.lo[a] || o[a] > box.hi[a]) return null;
      continue;
    }
    let ta = (box.lo[a] - o[a]) / d[a],
      tb = (box.hi[a] - o[a]) / d[a];
    let sign = -1;
    if (ta > tb) {
      [ta, tb] = [tb, ta];
      sign = 1;
    }
    if (ta > t0) {
      t0 = ta;
      normal = [0, 0, 0];
      normal[a] = sign;
    }
    t1 = Math.min(t1, tb);
    if (t0 > t1) return null;
  }
  return { t: t0, normal };
}

export function install(t: typeof Transport) {
  const { meshes, nodes } = scene();
  const game = gameChunks(nodes, meshes);
  const game_param = new URLSearchParams(location.search).get("game");
  let game_hour = game_param ? Number(game_param) || 0 : 12;
  const by_id = new Map(nodes.map((n) => [n.id, n]));
  const bounds = new Map<number, AABB>();
  for (const [uid, buf] of meshes) {
    const b = aabb();
    const dv = new DataView(buf);
    let o = 12;
    for (let p = 0, count = dv.getUint32(8, true); p < count; p++) {
      const nv = dv.getUint32(o, true),
        ni = dv.getUint32(o + 4, true),
        flags = dv.getUint32(o + 12, true);
      o += 16;
      for (let i = 0; i < nv; i++)
        aabbAdd(b, [
          dv.getFloat32(o + i * 12, true),
          dv.getFloat32(o + i * 12 + 4, true),
          dv.getFloat32(o + i * 12 + 8, true),
        ]);
      o +=
        nv * 24 + (flags & 1 ? nv * 8 : 0) + (flags & 2 ? nv * 4 : 0) + ni * 4;
    }
    bounds.set(uid, b);
  }
  let dirty = new Set<number>();
  const listeners = new Map<string, Set<(p: any) => void>>();
  const emit = (event: string, payload: any) =>
    setTimeout(() => listeners.get(event)?.forEach((cb) => cb(payload)), 0);
  const subtree = (id: number): MockNode[] => [
    by_id.get(id)!,
    ...nodes.filter((n) => n.parent === id).flatMap((n) => subtree(n.id)),
  ];

  t.fetch = async (path: string) => {
    await new Promise((r) => setTimeout(r, 5));
    if (path.startsWith("asset?path="))
      return asset(decodeURIComponent(path.slice(11)));
    if (path.startsWith("scene")) {
      const full = path.includes("full=1");
      const out = full ? nodes : nodes.filter((n) => dirty.has(n.id));
      dirty = new Set();
      return encodeScene(full, out, []);
    }
    if (path.startsWith("mesh/")) {
      const m = meshes.get(Number(path.slice(5)));
      if (!m) throw new Error(`404 ${path}`);
      return m.slice(0);
    }
    if (path.startsWith("texture/")) {
      const tx = TEXTURES[Number(path.slice(8))];
      if (!tx) throw new Error(`404 ${path}`);
      return tx.slice(0);
    }
    if (path === "materials")
      return new TextEncoder().encode(JSON.stringify({ materials: MATERIALS }))
        .buffer as ArrayBuffer;
    // ponytail: the chunks are made once, an edit on the dev page does not make them again
    if (path === "gameview") return encodeChunkList(game);
    const chunk = game.find((c) => path === `gameview/chunk/${c.id}`);
    if (chunk) return encodeChunk(chunk);
    const pal = game.find((c) => path === `gameview/palette/${c.palette}`);
    if (pal) return encodePalette(pal.rgba);
    throw new Error(`404 ${path}`);
  };

  t.listen = (event, cb) => {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event)!.add(cb);
    return () => listeners.get(event)?.delete(cb);
  };

  // what a ray meets, the nearest first (pick, pick_all)
  const pickAll = (origin: Vec3, dir: Vec3) => {
    const out: {
      node: number;
      selectable: number;
      point: Vec3;
      normal: Vec3;
      distance: number;
    }[] = [];
    for (const n of nodes) {
      // ponytail: boxes, not triangles (the far mountains' box holds everything: left out)
      if (
        n.kind !== NodeKind.Render ||
        !(n.flags & NodeFlag.Visible) ||
        n.flags & NodeFlag.Reference ||
        n.mesh === 6
      )
        continue;
      const hit = slab(
        aabbTransformed(bounds.get(n.mesh)!, n.world),
        origin,
        dir,
      );
      if (!hit) continue;
      const p: Vec3 = [0, 1, 2].map((a) => origin[a] + dir[a] * hit.t) as Vec3;
      // an actor's model: the actor
      const node = n.flags & NodeFlag.UnderActor ? n.parent : n.id;
      out.push({
        node,
        selectable: node,
        point: p,
        normal: hit.normal,
        distance: hit.t,
      });
    }
    return out.sort((a, b) => a.distance - b.distance);
  };
  const selectedIds = () =>
    new Set(nodes.filter((n) => n.flags & NodeFlag.Selected).map((n) => n.id));
  const setSelection = (ids: Set<number>) => {
    for (const n of nodes) {
      if (ids.has(n.id) !== !!(n.flags & NodeFlag.Selected)) {
        n.flags ^= NodeFlag.Selected;
        dirty.add(n.id);
      }
    }
    emit("jle:changed", { selection: true });
  };
  // the elements whose middle a box of the view holds (box_nodes)
  const boxNodes = (a: any): number[] => {
    const m = a.view_proj as number[];
    const [x0, x1] = [Math.min(a.a[0], a.b[0]), Math.max(a.a[0], a.b[0])];
    const [y0, y1] = [Math.min(a.a[1], a.b[1]), Math.max(a.a[1], a.b[1])];
    return nodes
      .filter((n) => {
        if (
          (n.kind !== NodeKind.Render && n.kind !== NodeKind.Actor) ||
          !(n.flags & NodeFlag.Visible) ||
          n.flags & (NodeFlag.Reference | NodeFlag.UnderActor) ||
          n.mesh === 6
        )
          return false;
        const b = n.mesh ? aabbTransformed(bounds.get(n.mesh)!, n.world) : null;
        const c = b
          ? b.lo.map((v, i) => (v + b.hi[i]) / 2)
          : point(n.world, [0, 0, 0]);
        const w = m[3] * c[0] + m[7] * c[1] + m[11] * c[2] + m[15];
        if (w <= 1e-5) return false;
        const x =
          (((m[0] * c[0] + m[4] * c[1] + m[8] * c[2] + m[12]) / w) * 0.5 +
            0.5) *
          a.width;
        const y =
          (1 -
            (((m[1] * c[0] + m[5] * c[1] + m[9] * c[2] + m[13]) / w) * 0.5 +
              0.5)) *
          a.height;
        return x >= x0 && x <= x1 && y >= y0 && y <= y1;
      })
      .map((n) => n.id);
  };

  t.call = async (name: string, args: any = {}) => {
    switch (name) {
      case "pick":
        return pickAll(args.origin, args.dir)[0] ?? null;
      case "pick_all":
        return pickAll(args.origin, args.dir);
      case "transform_nodes": {
        (args.ids as number[]).forEach((id, i) => {
          const root = by_id.get(id);
          if (!root) return;
          const delta = matMul(
            new Float32Array(args.worlds[i]),
            inverse(root.world),
          );
          for (const n of subtree(id)) {
            n.world =
              n === root
                ? new Float32Array(args.worlds[i])
                : matMul(delta, n.world);
            dirty.add(n.id);
          }
        });
        emit("jle:changed", { scene: true });
        return null;
      }
      case "select": {
        const ids: number[] = args.ids ?? [];
        const sel = selectedIds();
        if (args.mode === "add") ids.forEach((id) => sel.add(id));
        else if (args.mode === "remove") ids.forEach((id) => sel.delete(id));
        else if (args.mode === "toggle")
          ids.forEach((id) => (sel.has(id) ? sel.delete(id) : sel.add(id)));
        setSelection(args.mode ? sel : new Set(ids));
        return null;
      }
      case "select_only":
        setSelection(new Set(args.id ? [args.id] : []));
        return null;
      case "toggle_select": {
        const sel = selectedIds();
        if (!sel.delete(args.id)) sel.add(args.id);
        setSelection(sel);
        return null;
      }
      // ponytail: the whole group is what stands beside it (the same parent)
      case "select_whole": {
        const parent = by_id.get(args.id)?.parent ?? 0;
        setSelection(
          new Set(
            nodes
              .filter(
                (n) =>
                  n.parent === parent &&
                  (n.kind === NodeKind.Render || n.kind === NodeKind.Actor),
              )
              .map((n) => n.id),
          ),
        );
        return true;
      }
      case "box_nodes":
        return { ids: boxNodes(args) };
      case "select_box": {
        const ids = boxNodes(args);
        await t.call("select", {
          ids,
          mode: args.mode === "replace" ? undefined : args.mode,
        });
        return { ids };
      }
      case "labels":
        return nodes
          .filter(
            (n) => n.kind === NodeKind.Actor || n.flags & NodeFlag.Selected,
          )
          .map((n) =>
            n.kind === NodeKind.Actor
              ? { id: n.id, name: `actor-${n.id}`, etype: "crate" }
              : { id: n.id, name: `piece-${n.id}` },
          );
      case "node":
        return by_id.has(args.id)
          ? { id: args.id, name: `piece-${args.id}`, group: null }
          : null;
      case "effect_emitters":
        return EFFECTS;
      case "game_mood":
        return game_hour >= 18 || game_hour < 6 ? NIGHT : NOON;
      case "set_view":
        if (typeof args.hour === "number") game_hour = args.hour;
        if (typeof args.game_view === "boolean") gameView.setOn(args.game_view);
        emit("jle:changed", { gameview: true, state: true });
        return null;
      default:
        return null; // set_camera_state, frontend_reply...
    }
  };
  selfcheck();
  gameSelfcheck(game);
  // in the page's console: gameView, call("set_view", {hour: 22}) (the mood of 22:00)
  Object.assign(window, { gameView, call: t.call });
  if (game_param) setTimeout(() => gameView.setOn(true), 0);
  // once the level is framed
  if (new URLSearchParams(location.search).has("fx"))
    setTimeout(
      () =>
        emit("jle:request", {
          kind: "camera",
          pos: [2, 3, 13],
          look_at: [0, 2.5, 4],
        }),
      1500,
    );
  console.info("[viewport] dev mock engine");
}

// ---------------------------------------------------------------------------------------------
// the effects

// ctyindb's fast steam (its vent's z turned up) and a light of the city, beside the selected box
const up = rotate(axisAngle([1, 0, 0], -Math.PI / 2));
const EFFECTS = [
  {
    id: 900,
    group: "group-ctyindb-fast-steam",
    world: [...matMul(translate([2, 0, 4]), up)],
  },
  {
    id: 901,
    group: "group-citylights-newslums",
    world: [...translate([-2, 4, 4])],
  },
];

// a file of asset?path=: the game's tables from the dev server (it serves the repository), the
// sprites a soft white disc (their alpha the PS2's: 128 is opaque)
async function asset(file: string): Promise<ArrayBuffer> {
  if (file.startsWith("data/")) {
    const r = await fetch(`/${file}`);
    if (!r.ok) throw new Error(`404 ${file}`);
    return r.arrayBuffer();
  }
  if (file.endsWith("index.json"))
    return new TextEncoder().encode("{}").buffer as ArrayBuffer;
  if (!file.endsWith(".png")) throw new Error(`404 ${file}`);
  const c = new OffscreenCanvas(32, 32),
    ctx = c.getContext("2d")!;
  const g = ctx.createRadialGradient(16, 16, 0, 16, 16, 16);
  g.addColorStop(0, "rgba(255, 255, 255, 0.5)");
  g.addColorStop(1, "rgba(255, 255, 255, 0)");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, 32, 32);
  return (await c.convertToBlob()).arrayBuffer();
}
