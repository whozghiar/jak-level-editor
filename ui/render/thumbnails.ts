// The panels' pictures (render/thumbnails.cpp): prefabs and models, decor parts of a level, whole
// levels (from above), the icons of the objects without a model, shapes, particle effects. Each is drawn once, offscreen in
// a WebGL2 context of its own, from the engine's package of it (preview/<kind>: its mesh with its
// own materials and textures), in the C++ framing (3/4 view), then kept as a PNG by the engine
// (thumb_store, served again as thumb/<name>). Effects: particle_thumb.ts. A cell asks with
// `thumbs.get(req)` while it is shown and lets go with `thumbs.release(req)`: what scrolled away
// before it was drawn is not drawn.

import { Camera } from "./camera";
import { call, fetchBytes } from "./engine";
import { aabbRadius, aabbValid, identity, type AABB, type Mat4 } from "./math";
import { drawEffectThumb } from "./particle_thumb";
import { parseMaterials, parseMesh, parseTexture } from "./protocol";
import { Renderer } from "./renderer";
import type { SceneStore, TextureEntry } from "./scene_store";
import { defaultRenderSettings } from "./types";

export type ThumbRequest =
  | { kind: "model"; path: string; stamp?: number } // a model or a prefab (.glb)
  | { kind: "part"; path: string; node: string; stamp?: number } // a level's background, the node of the part
  | { kind: "level"; path: string; stamp?: number } // a level's background: all of it
  | { kind: "icon"; etype: string; family: string } // an object of the game without a model
  | { kind: "effect"; group: string; fallback?: ThumbRequest } // a particle group (else its fallback)
  | { kind: "shape"; preset: string };

export const THUMB_SIZE = 128;

// ThumbRequest::key (a changed file, a new stamp: a new picture)
export function thumbKey(r: ThumbRequest): string {
  switch (r.kind) {
    case "model":
      return `m:${r.path}@${r.stamp ?? 0}`;
    case "part":
      return `p:${r.path}#${r.node}@${r.stamp ?? 0}`;
    case "level":
      return `b:${r.path}@${r.stamp ?? 0}`;
    case "icon":
      return `i1:${r.etype}#${r.family}`;
    case "effect":
      return `e2:${r.group}@0`;
    case "shape":
      return `s:${r.preset}`;
  }
}

// the cache file's name: FNV-1a 64 of the key
function fileName(key: string): string {
  let h = 0xcbf29ce484222325n;
  for (const b of new TextEncoder().encode(key))
    h = ((h ^ BigInt(b)) * 0x100000001b3n) & 0xffffffffffffffffn;
  return h.toString(16).padStart(16, "0");
}

const q = (s: string) => encodeURIComponent(s);
function packagePath(r: ThumbRequest): string {
  switch (r.kind) {
    case "model":
      return `preview/model?path=${q(r.path)}`;
    case "part":
      return `preview/part?path=${q(r.path)}&node=${q(r.node)}`;
    case "level":
      return `preview/level?path=${q(r.path)}`;
    case "icon":
      return `preview/icon?etype=${q(r.etype)}&family=${q(r.family)}`;
    default:
      return `preview/shape?preset=${q((r as { preset: string }).preset)}`;
  }
}

// one context for every picture
let ctx: { canvas: HTMLCanvasElement; renderer: Renderer } | null = null;
function context() {
  if (!ctx) {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = THUMB_SIZE;
    const gl = canvas.getContext("webgl2", {
      antialias: true,
      alpha: false,
      preserveDrawingBuffer: true,
    });
    if (!gl) throw new Error("WebGL2 is not available");
    ctx = { canvas, renderer: new Renderer(gl) };
  }
  return ctx;
}

// JLEP 1 (editor/thumbs.rs): magic, version, json bytes, JSON {materials, textures, bounds, draws:
// [{mesh, world}]}, meshes (each its bytes, JLEM), textures (each u32 width, u32 height, RGBA)
function parsePackage(buf: ArrayBuffer) {
  const dv = new DataView(buf);
  if (dv.getUint32(0, true) !== 0x50454c4a) throw new Error("not JLEP");
  let o = 12;
  const jlen = dv.getUint32(8, true);
  const json = JSON.parse(
    new TextDecoder().decode(new Uint8Array(buf, o, jlen)),
  );
  o += jlen;
  const n = dv.getUint32(o, true);
  o += 4;
  const meshes = [];
  for (let i = 0; i < n; i++) {
    const len = dv.getUint32(o, true);
    meshes.push(parseMesh(buf.slice(o + 4, o + 4 + len), i, 0));
    o += 4 + len;
  }
  const count = dv.getUint32(o, true);
  o += 4;
  const textures = [];
  for (let i = 0; i < count; i++) {
    const w = dv.getUint32(o, true),
      h = dv.getUint32(o + 4, true);
    const bytes = 8 + w * h * 4;
    textures.push(parseTexture(buf.slice(o, o + bytes)));
    o += bytes;
  }
  const draws = (json.draws ?? [{ mesh: 0, world: identity() }]) as {
    mesh: number;
    world: Mat4;
  }[];
  return {
    meshes,
    draws,
    materials: parseMaterials(json).materials,
    textures,
    bounds: json.bounds as AABB,
  };
}

// the picture drawn now, as a PNG data URL (null: nothing to draw)
async function draw(r: ThumbRequest): Promise<string | null> {
  if (r.kind === "effect") {
    const c = document.createElement("canvas");
    c.width = c.height = THUMB_SIZE;
    if (await drawEffectThumb(c, r.group)) return c.toDataURL("image/png");
    return r.fallback ? draw(r.fallback) : null;
  }
  let p;
  try {
    p = parsePackage(await fetchBytes(packagePath(r)));
  } catch {
    return null;
  }
  const bounds = p.bounds;
  if (!aabbValid(bounds)) return null;
  const { canvas, renderer } = context();
  const cam = new Camera();
  cam.fov_deg = 35;
  cam.yaw = 0.65;
  cam.pitch = r.kind === "level" ? -0.95 : -0.38;
  cam.frame(bounds);
  cam.znear = Math.max(0.01, aabbRadius(bounds) * 0.01);
  const rs = {
    ...defaultRenderSettings(),
    show_grid: false,
    frustum_culling: false,
    clear_color: [0.19, 0.21, 0.24] as [number, number, number],
  };
  // its own textures, as the store's entries
  const textures: TextureEntry[] = p.textures.map((data) => ({
    version: 0,
    loaded: 0,
    data,
    loading: false,
    gpu: null,
  }));
  const store = {
    texture: (i: number) => textures[i] ?? null,
  } as unknown as SceneStore;
  const srcs = p.meshes.map((data) => ({ data, gpu: null as unknown }));
  renderer.begin(cam, THUMB_SIZE, THUMB_SIZE, rs);
  for (const d of p.draws)
    if (srcs[d.mesh])
      renderer.drawMesh(srcs[d.mesh], d.world, p.materials, store, rs);
  const out = document.createElement("canvas");
  out.width = out.height = THUMB_SIZE;
  out.getContext("2d")!.drawImage(canvas, 0, 0);
  for (const src of srcs) renderer.free(src.gpu);
  for (const t of textures) renderer.free(t.gpu);
  return out.toDataURL("image/png");
}

interface Item {
  req: ThumbRequest;
  name: string;
  wanted: number;
  url: Promise<string | null>;
  resolve: (u: string | null) => void;
  queued: boolean;
}

class Thumbnails {
  private items = new Map<string, Item>();
  private queue: Item[] = []; // the most recently asked drawn first
  private running = 0;

  /** The picture's URL once there (null: none can be drawn); kept until `release`. */
  get(req: ThumbRequest): Promise<string | null> {
    const key = thumbKey(req);
    let it = this.items.get(key);
    if (it) {
      it.wanted++;
      if (!it.queued && it.wanted === 1) void this.find(it);
      return it.url;
    }
    let resolve!: (u: string | null) => void;
    const url = new Promise<string | null>((r) => (resolve = r));
    it = { req, name: fileName(key), wanted: 1, url, resolve, queued: false };
    this.items.set(key, it);
    void this.find(it);
    return url;
  }

  /** The cell asking for it left the screen. */
  release(req: ThumbRequest) {
    const it = this.items.get(thumbKey(req));
    if (it) it.wanted = Math.max(0, it.wanted - 1);
  }

  // drawn before (the engine's cache), else drawn now
  private async find(it: Item) {
    try {
      const png = await fetchBytes(`thumb/${it.name}`);
      it.resolve(URL.createObjectURL(new Blob([png], { type: "image/png" })));
      it.queued = true; // done
      return;
    } catch {
      // not drawn yet
    }
    if (!it.queued) {
      it.queued = true;
      this.queue.push(it);
      this.pump();
    }
  }

  private pump() {
    // three at a time (the C++ draws three per frame), each in a frame of its own
    while (this.running < 3 && this.queue.length) {
      const it = this.queue.pop()!;
      if (!it.wanted) {
        // scrolled away: asked again when shown again
        it.queued = false;
        this.items.delete(thumbKey(it.req));
        continue;
      }
      this.running++;
      requestAnimationFrame(async () => {
        let url: string | null = null;
        try {
          url = await draw(it.req);
        } catch (e) {
          console.warn("[thumbnails]", e);
        }
        it.resolve(url);
        if (url)
          call("thumb_store", {
            name: it.name,
            png: url.slice(url.indexOf(",") + 1),
          }).catch(() => {});
        this.running--;
        this.pump();
      });
    }
  }
}

export const thumbs = new Thumbnails();
