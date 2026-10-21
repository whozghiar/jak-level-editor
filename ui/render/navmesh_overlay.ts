// The nav meshes over the view (app.cpp draw_navmesh_overlay), View > Nav meshes: the polygons of
// each nav mesh node (the engine's navmesh_data, read once per mesh), a color per mesh,
// see-through, its borders (where creatures stop) brighter, orange when selected or hovered.

import { engine } from "../lib/engine.svelte";
import { call } from "./engine";
import { add, packRgba, point, type Vec3 } from "./math";
import type { Renderer } from "./renderer";
import { store } from "./scene_store";
import { NodeFlag, NodeKind } from "./types";
import { viewport } from "./viewport_api";

type NavData = {
  id: number;
  mesh: number;
  mesh_version: number;
  verts: Vec3[];
  polys: number[][];
  adj: number[][];
};

const kColors: Vec3[] = [
  [0.15, 0.85, 0.75],
  [0.95, 0.75, 0.2],
  [0.55, 0.6, 1],
  [0.95, 0.4, 0.55],
  [0.5, 0.9, 0.3],
  [0.9, 0.55, 0.95],
];
const lift: Vec3 = [0, 0.06, 0];

// the polygons by mesh (uid:version, as the scene's records have it), and the meshes asked for
const cache = new Map<string, NavData>();
const asked = new Set<string>();

async function read(ids: number[]) {
  const list: NavData[] | null = await call("navmesh_data", { ids }).catch(
    () => null,
  );
  for (const d of list ?? []) cache.set(`${d.mesh}:${d.mesh_version}`, d);
  if (list?.length) viewport.requestRedraw();
}

function draw(r: Renderer) {
  if (!engine.state.view.navmesh) return;
  const used = new Set<string>();
  const missing: number[] = [];
  let index = 0;
  for (const n of store.list) {
    if (n.kind !== NodeKind.NavMesh || !n.mesh || !(n.flags & NodeFlag.Visible))
      continue;
    const key = `${n.mesh}:${n.mesh_version}`;
    used.add(key);
    const m = cache.get(key);
    if (!m) {
      if (!asked.has(key)) missing.push(n.id);
      asked.add(key);
      continue;
    }
    const w = store.world(n);
    const sel = !!(n.flags & NodeFlag.Selected) || viewport.hovered === n.id;
    const [cr, cg, cb] = kColors[index++ % 6];
    const fill = packRgba(cr, cg, cb, sel ? 0.5 : 0.3);
    const inner = packRgba(cr, cg, cb, 0.55);
    const border = sel
      ? packRgba(1, 0.6, 0.15, 1)
      : packRgba(cr * 0.6 + 0.4, cg * 0.6 + 0.4, cb * 0.6 + 0.4, 1);
    m.polys.forEach((p, i) => {
      const v = p.map((k) => point(w, m.verts[k]));
      for (let k = 1; k + 1 < v.length; k++) r.tri(v[0], v[k], v[k + 1], fill);
      for (let k = 0; k < v.length; k++) {
        // an inner edge is drawn once, from the polygon with the smaller index
        const j = m.adj[i][k];
        if (j >= 0 && j < i) continue;
        r.line(
          add(v[k], lift),
          add(v[(k + 1) % v.length], lift),
          j < 0 ? border : inner,
        );
      }
    });
  }
  // forget the meshes gone
  for (const k of [...cache.keys(), ...asked]) {
    if (used.has(k)) continue;
    cache.delete(k);
    asked.delete(k);
  }
  if (missing.length) void read(missing);
  r.flush(true, true);
}

/** Its pass in the viewport's frame (in captures too, as the C++); returns what removes it. */
export function attachNavmeshOverlay(): () => void {
  const off = viewport.addOverlay((r) => draw(r), { capture: true });
  return () => {
    off();
    cache.clear();
    asked.clear();
  };
}
