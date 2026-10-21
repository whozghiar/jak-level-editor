// The inspector's preview in the view (preview.cpp update_preview, draw_preview_overlay): the model
// moving where it stands, the document left as it is. The engine gives the node a mesh of its own
// in its bind pose while it plays, each vertex's joints and weights once (start_preview), then the
// joints' matrices each frame (preview_frame {dt}); the vertices are moved here (on the CPU: a
// model's few thousand vertices, the matrices only crossing each frame: that keeps 60 frames a
// second) into the mesh the store read, uploaded again. An actor without a model: its icon, drawn
// where preview_frame puts it.
// Viewport.svelte attaches it (`attachPreview`); the inspector's PreviewSection starts and stops it.

import { call, engineReady, onEngine } from "./engine";
import { callQueue } from "./call_queue";
import type { Mat4 } from "./math";
import type { MeshData } from "./protocol";
import type { MeshEntry } from "./scene_store";
import { store } from "./scene_store";
import { NodeFlag } from "./types";
import { viewport } from "./viewport_api";

/** What plays (the node drawn moving: 0 none), for the inspector */
export const previewState = $state({ node: 0 });

interface Skin {
  prim: number;
  joints: Uint16Array; // 4 a vertex
  weights: Float32Array;
}
interface Playing {
  node: number;
  mesh: number;
  icon: boolean;
  skin: Skin[];
  // the bind pose of the mesh read (its positions and normals), the data they are of
  data: MeshData | null;
  bind: { pos: Float32Array; nrm: Float32Array }[];
  joints: Float32Array | null;
  iconEntry: MeshEntry | null;
  iconWorld: Mat4 | null;
}

let playing: Playing | null = null;
let stopAnimating: (() => void) | null = null;
let last = 0;

function bytes(b64: string): Uint8Array {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

function end() {
  if (playing?.iconEntry) store.releaseMesh(playing.iconEntry.uid);
  playing = null;
  previewState.node = 0;
  stopAnimating?.();
  stopAnimating = null;
  viewport.requestRedraw();
}

const frames = callQueue(
  (_, f) => {
    if (!playing) return;
    if (!f) return end(); // ended (another document, an edit, stopped)
    if (f.icon) {
      const p = playing;
      if (p.iconEntry?.uid !== f.icon.mesh) {
        if (p.iconEntry) store.releaseMesh(p.iconEntry.uid);
        p.iconEntry = store.retainMesh(f.icon.mesh, f.icon.version);
      }
      p.iconWorld = new Float32Array(f.icon.world) as Mat4;
    } else if (f.joints) {
      const b = bytes(f.joints);
      playing.joints = new Float32Array(
        b.buffer,
        b.byteOffset,
        b.byteLength / 4,
      );
      skin();
    }
    viewport.requestRedraw();
  },
  ["preview_frame"],
);

/** Starts what the inspector asks: {id, what: animation | motion | break | effect, choice?, loop?} */
export async function startPreview(
  args: Record<string, unknown>,
): Promise<boolean> {
  end();
  let p: any;
  try {
    p = await call("start_preview", args);
  } catch (e) {
    console.warn("[preview]", e);
  }
  if (!p) return false;
  playing = {
    node: p.node,
    mesh: p.mesh,
    icon: !!p.icon,
    skin: (p.skin ?? []).map((s: any) => {
      const j = bytes(s.joints),
        w = bytes(s.weights);
      return {
        prim: s.prim,
        joints: new Uint16Array(j.buffer, j.byteOffset, j.byteLength / 2),
        weights: new Float32Array(w.buffer, w.byteOffset, w.byteLength / 4),
      };
    }),
    data: null,
    bind: [],
    joints: null,
    iconEntry: p.icon ? store.retainMesh(p.mesh, p.version) : null,
    iconWorld: null,
  };
  previewState.node = p.node;
  last = performance.now();
  stopAnimating = viewport.animate();
  return true;
}

export async function stopPreview() {
  end();
  await call("stop_preview").catch(() => {});
}

// the vertices moved by the joints (skin_mesh): each the weighted sum of where its joints put it
function skin() {
  const p = playing!;
  const entry = store.meshes.get(p.mesh);
  const d = entry?.data;
  if (!entry || !d || !p.joints) return;
  if (p.data !== d) {
    // the mesh as read: its bind pose
    p.data = d;
    p.bind = d.prims.map((m) => ({
      pos: new Float32Array(d.buffer, m.pos, m.vertices * 3).slice(),
      nrm: new Float32Array(d.buffer, m.nrm, m.vertices * 3).slice(),
    }));
  }
  const J = p.joints,
    count = J.length / 16;
  for (const s of p.skin) {
    const m = d.prims[s.prim];
    if (!m) continue;
    const pos = new Float32Array(d.buffer, m.pos, m.vertices * 3);
    const nrm = new Float32Array(d.buffer, m.nrm, m.vertices * 3);
    const bp = p.bind[s.prim].pos,
      bn = p.bind[s.prim].nrm;
    for (let v = 0; v < m.vertices; v++) {
      const [x, y, z] = [bp[v * 3], bp[v * 3 + 1], bp[v * 3 + 2]];
      const [nx, ny, nz] = [bn[v * 3], bn[v * 3 + 1], bn[v * 3 + 2]];
      let px = 0,
        py = 0,
        pz = 0,
        qx = 0,
        qy = 0,
        qz = 0,
        total = 0;
      for (let k = 0; k < 4; k++) {
        const w = s.weights[v * 4 + k],
          j = s.joints[v * 4 + k];
        if (w <= 0 || j >= count) continue;
        const o = j * 16;
        px += (J[o] * x + J[o + 4] * y + J[o + 8] * z + J[o + 12]) * w;
        py += (J[o + 1] * x + J[o + 5] * y + J[o + 9] * z + J[o + 13]) * w;
        pz += (J[o + 2] * x + J[o + 6] * y + J[o + 10] * z + J[o + 14]) * w;
        qx += (J[o] * nx + J[o + 4] * ny + J[o + 8] * nz) * w;
        qy += (J[o + 1] * nx + J[o + 5] * ny + J[o + 9] * nz) * w;
        qz += (J[o + 2] * nx + J[o + 6] * ny + J[o + 10] * nz) * w;
        total += w;
      }
      if (total <= 0) continue;
      pos[v * 3] = px / total;
      pos[v * 3 + 1] = py / total;
      pos[v * 3 + 2] = pz / total;
      const l = Math.hypot(qx, qy, qz) || 1;
      nrm[v * 3] = qx / l;
      nrm[v * 3 + 1] = qy / l;
      nrm[v * 3 + 2] = qz / l;
    }
  }
  // uploaded again at its next draw
  if (entry.gpu) store.freeGpu(entry.gpu);
  entry.gpu = null;
}

export function attachPreview(): () => void {
  const off: (() => void)[] = [];
  off.push(
    viewport.addOverlay((r, rs) => {
      const p = playing;
      if (!p) return;
      // the time goes on: the engine's frame asked
      const now = performance.now();
      frames.send("preview_frame", { dt: (now - last) / 1000 });
      last = now;
      if (p.icon) {
        if (p.iconEntry?.data && p.iconWorld)
          r.drawMesh(p.iconEntry, p.iconWorld, [], store, rs);
        return;
      }
      // a model of the decor over the game view: the game view draws it still, the moving one
      // drawn over it (ponytail: both show; a game view leaving it out would need the engine's chunks
      // to skip it)
      const n = store.nodes.get(p.node);
      const entry = store.meshes.get(p.mesh);
      if (rs.game_view && n && entry?.data && !(n.flags & NodeFlag.UnderActor))
        r.drawMesh(entry, store.world(n), store.materials, store, rs);
    }),
  );
  // another document: nothing plays (once the engine is there: the dev page's mock)
  let attached = true;
  off.push(() => (attached = false));
  void engineReady.then(
    () =>
      attached &&
      off.push(onEngine("jle:changed", (c) => c?.document && end())),
  );
  return () => {
    off.forEach((f) => f());
    end();
  };
}
