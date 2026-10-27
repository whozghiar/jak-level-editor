// The route of the object selected over the view (editor/arrange.rs): where a platform or an
// elevator goes, its points joined in yellow, a small cross at each, the first one green. Read from
// the engine (actor_route) when the selection or the document changes.

import { engine } from "../lib/engine.svelte";
import { call } from "./engine";
import { add, packRgba, type Vec3 } from "./math";
import type { Renderer } from "./renderer";
import { viewport } from "./viewport_api";

const lift: Vec3 = [0, 0.15, 0];
let key = "";
let points: Vec3[] = [];

async function read(id: number, k: string) {
  const r: { points: Vec3[] } | null = await call("actor_route", { id }).catch(
    () => null,
  );
  if (k !== key) return;
  points = r?.points ?? [];
  viewport.requestRedraw();
}

function draw(r: Renderer) {
  const s = engine.state;
  const id = s.selection.count ? s.selection.primary : 0;
  const k = `${id}:${s.history.revision}`;
  if (k !== key) {
    key = k;
    points = [];
    if (id) void read(id, k);
  }
  if (points.length < 2) return;
  const line = packRgba(1, 0.85, 0.15, 1);
  const first = packRgba(0.3, 0.95, 0.4, 1);
  for (let i = 0; i < points.length; i++) {
    const p = add(points[i], lift);
    if (i + 1 < points.length) r.line(p, add(points[i + 1], lift), line);
    const c = i === 0 ? first : line;
    for (const d of [
      [0.5, 0, 0],
      [0, 0, 0.5],
      [0, 0.5, 0],
    ] as Vec3[]) {
      r.line(add(p, d), add(p, [-d[0], -d[1], -d[2]]), c);
    }
  }
  r.flush(false);
}

/** Its pass in the viewport's frame; returns what removes it. */
export function attachRouteOverlay(): () => void {
  const off = viewport.addOverlay((r) => draw(r), { capture: true });
  return () => {
    off();
    key = "";
    points = [];
  };
}
