// A pretend engine for `npx vite` in a browser (no Tauri): a small scene encoded in the jle://
// protocol's exact formats (JLES, JLEM, texture, materials), and the calls the viewport makes
// (pick, set_camera_state, transform_nodes) plus `select` for the dev page.

import type { transport as Transport } from "../engine";
import {
  aabb,
  aabbAdd,
  aabbTransformed,
  axisAngle,
  identity,
  inverse,
  matMul,
  rotate,
  scale,
  translate,
  type AABB,
  type Mat4,
  type Vec3,
} from "../math";
import { parseMesh, parseScene } from "../protocol";
import { NodeFlag, NodeKind } from "../types";

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
    throw new Error(`404 ${path}`);
  };

  t.listen = (event, cb) => {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event)!.add(cb);
    return () => listeners.get(event)?.delete(cb);
  };

  t.call = async (name: string, args: any = {}) => {
    switch (name) {
      case "pick": {
        let best: {
          node: number;
          point: Vec3;
          normal: Vec3;
          distance: number;
        } | null = null;
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
            args.origin,
            args.dir,
          );
          if (hit && (!best || hit.t < best.distance)) {
            const p: Vec3 = [0, 1, 2].map(
              (a) => args.origin[a] + args.dir[a] * hit.t,
            ) as Vec3;
            // an actor's model: the actor
            const node = n.flags & NodeFlag.UnderActor ? n.parent : n.id;
            best = { node, point: p, normal: hit.normal, distance: hit.t };
          }
        }
        return best;
      }
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
        const ids = new Set<number>(args.ids ?? []);
        for (const n of nodes) {
          const sel = ids.has(n.id);
          if (sel !== !!(n.flags & NodeFlag.Selected)) {
            n.flags ^= NodeFlag.Selected;
            dirty.add(n.id);
          }
        }
        emit("jle:changed", { selection: true });
        return null;
      }
      default:
        return null; // set_camera_state, frontend_reply...
    }
  };
  selfcheck();
  console.info("[viewport] dev mock engine");
}
