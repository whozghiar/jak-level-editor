// The jle:// protocol's answers (docs/rust-architecture.md §4), little-endian, read in place:
// the matrices and the vertex arrays are views into the answer's bytes, nothing is copied.

import { fetchBytes } from "./engine";
import { aabb, aabbAdd, type AABB } from "./math";
import {
  AlphaMode,
  BlendMode,
  PrimFlag,
  type Material,
  type NodeRecord,
} from "./types";

const magic = (dv: DataView, s: string) =>
  dv.byteLength >= 4 &&
  [0, 1, 2, 3].every((i) => dv.getUint8(i) === s.charCodeAt(i));

export interface SceneAnswer {
  full: boolean;
  nodes: NodeRecord[];
  removed: Uint32Array;
}

// JLES 1: magic, version, full, count, then per node (84 bytes) id, parent, u8 kind, u8 flags,
// u16 reserved, mesh uid, mesh version, f32 world[16]; then removed, the ids removed
export function parseScene(buf: ArrayBuffer): SceneAnswer {
  const dv = new DataView(buf);
  if (!magic(dv, "JLES")) throw new Error("scene: not JLES");
  const version = dv.getUint32(4, true);
  if (version !== 1) throw new Error(`scene: JLES version ${version}`);
  const full = dv.getUint32(8, true) === 1;
  const count = dv.getUint32(12, true);
  const nodes: NodeRecord[] = new Array(count);
  let o = 16;
  for (let i = 0; i < count; i++, o += 84) {
    nodes[i] = {
      id: dv.getUint32(o, true),
      parent: dv.getUint32(o + 4, true),
      kind: dv.getUint8(o + 8),
      flags: dv.getUint8(o + 9),
      extra: dv.getUint16(o + 10, true),
      mesh: dv.getUint32(o + 12, true),
      mesh_version: dv.getUint32(o + 16, true),
      world: new Float32Array(buf, o + 20, 16),
    };
  }
  const removed_count = dv.getUint32(o, true);
  const removed = new Uint32Array(buf, o + 4, removed_count);
  return { full, nodes, removed };
}

export interface MeshPrim {
  vertices: number;
  indices: number;
  material: number;
  flags: number; // PrimFlag
  pos: number; // byte offsets into the answer, -1 when absent
  nrm: number;
  uv: number;
  col: number;
  idx: number;
}

export interface MeshData {
  uid: number;
  version: number; // the node records' mesh version it was fetched for
  buffer: ArrayBuffer;
  prims: MeshPrim[];
  bounds: AABB; // local
  backdrop: boolean; // PrimFlag.Backdrop on its primitives
}

function readPrims(
  dv: DataView,
  count: number,
  headers_first: boolean,
): MeshPrim[] | null {
  const prims: MeshPrim[] = [];
  let o = 12;
  let h = 12;
  if (headers_first) o += count * 16;
  for (let i = 0; i < count; i++) {
    const at = headers_first ? h : o;
    if (at + 16 > dv.byteLength) return null;
    const n = dv.getUint32(at, true),
      m = dv.getUint32(at + 4, true);
    const material = dv.getInt32(at + 8, true),
      flags = dv.getUint32(at + 12, true);
    if (flags & ~7) return null;
    if (headers_first) h += 16;
    else o += 16;
    const p: MeshPrim = {
      vertices: n,
      indices: m,
      material,
      flags,
      pos: o,
      nrm: o + 12 * n,
      uv: -1,
      col: -1,
      idx: 0,
    };
    o += 24 * n;
    if (flags & PrimFlag.Uv) {
      p.uv = o;
      o += 8 * n;
    }
    if (flags & PrimFlag.Colors) {
      p.col = o;
      o += 4 * n;
    }
    p.idx = o;
    o += 4 * m;
    if (o > dv.byteLength) return null;
    prims.push(p);
  }
  return o === dv.byteLength ? prims : null;
}

// JLEM 1: magic, version, prims, then per primitive vertices, indices, i32 material, flags (1 uv,
// 2 colors, 4 backdrop) and its arrays: f32 pos[3n], f32 nrm[3n], f32 uv[2n] (if any), u8 col[4n]
// (if any), u32 idx[m]. All the headers before all the arrays is read too (the spec's sentence
// reads both ways): the layout whose sizes add up to the answer's.
export function parseMesh(
  buf: ArrayBuffer,
  uid: number,
  version: number,
): MeshData {
  const dv = new DataView(buf);
  if (!magic(dv, "JLEM")) throw new Error(`mesh ${uid}: not JLEM`);
  const v = dv.getUint32(4, true);
  if (v !== 1) throw new Error(`mesh ${uid}: JLEM version ${v}`);
  const count = dv.getUint32(8, true);
  const prims = readPrims(dv, count, false) ?? readPrims(dv, count, true);
  if (!prims) throw new Error(`mesh ${uid}: sizes do not add up`);
  const bounds = aabb();
  for (const p of prims) {
    const pos = new Float32Array(buf, p.pos, p.vertices * 3);
    for (let i = 0; i < pos.length; i += 3)
      aabbAdd(bounds, [pos[i], pos[i + 1], pos[i + 2]]);
  }
  const backdrop = prims.some((p) => p.flags & PrimFlag.Backdrop);
  return { uid, version, buffer: buf, prims, bounds, backdrop };
}

export interface TextureData {
  width: number;
  height: number;
  rgba: Uint8Array;
}

// texture/<index>: u32 width, u32 height, RGBA bytes
export function parseTexture(buf: ArrayBuffer): TextureData {
  const dv = new DataView(buf);
  const width = dv.getUint32(0, true),
    height = dv.getUint32(4, true);
  if (buf.byteLength < 8 + width * height * 4)
    throw new Error("texture: too short");
  return { width, height, rgba: new Uint8Array(buf, 8, width * height * 4) };
}

const ALPHA_NAMES: Record<string, number> = {
  opaque: AlphaMode.Opaque,
  mask: AlphaMode.Mask,
  blend: AlphaMode.Blend,
};
const BLEND_NAMES: Record<string, number> = {
  "": BlendMode.Alpha,
  alpha: BlendMode.Alpha,
  add: BlendMode.Add,
  add_alpha: BlendMode.AddAlpha,
  subtract: BlendMode.Subtract,
  half: BlendMode.Half,
};
const enum_of = (
  v: unknown,
  names: Record<string, number>,
  fallback: number,
) =>
  typeof v === "number"
    ? v
    : typeof v === "string"
      ? (names[v.toLowerCase()] ?? fallback)
      : fallback;

export interface MaterialsAnswer {
  materials: Material[];
  // the textures' versions when the engine gives them ({textures: [{version}]}): a texture is
  // fetched again when its version changes
  textureVersions: number[] | null;
}

// materials: JSON, an array of materials or {materials: [...], textures: [{version}...]}. A
// material: {name, texture, color | base_color: [r, g, b, a], alpha: "opaque" | "mask" | "blend"
// (or 0..2), blend: "alpha" | "add" | "add_alpha" | "subtract" | "half" (or 0..4), cutoff,
// double_sided | two_sided, translucent, water}
export function parseMaterials(json: any): MaterialsAnswer {
  const list: any[] = Array.isArray(json) ? json : (json?.materials ?? []);
  const materials = list.map((m: any): Material => ({
    name: String(m?.name ?? ""),
    texture: typeof m?.texture === "number" ? m.texture : -1,
    base_color: (m?.color ?? m?.base_color ?? [1, 1, 1, 1]).slice(
      0,
      4,
    ) as Material["base_color"],
    alpha: enum_of(m?.alpha, ALPHA_NAMES, AlphaMode.Opaque),
    blend: enum_of(m?.blend, BLEND_NAMES, BlendMode.Alpha),
    cutoff: typeof m?.cutoff === "number" ? m.cutoff : 0.5,
    double_sided: Boolean(m?.double_sided ?? m?.two_sided ?? true),
    translucent: Boolean(m?.translucent),
    water: Boolean(m?.water),
  }));
  const tex = Array.isArray(json?.textures) ? json.textures : null;
  return {
    materials,
    textureVersions: tex ? tex.map((t: any) => Number(t?.version ?? 0)) : null,
  };
}

export const fetchScene = async (full: boolean) =>
  parseScene(await fetchBytes(full ? "scene?full=1" : "scene"));
export const fetchMesh = async (uid: number, version: number) =>
  parseMesh(await fetchBytes(`mesh/${uid}`), uid, version);
export const fetchTexture = async (index: number) =>
  parseTexture(await fetchBytes(`texture/${index}`));
export const fetchMaterials = async () =>
  parseMaterials(
    JSON.parse(new TextDecoder().decode(await fetchBytes("materials"))),
  );
