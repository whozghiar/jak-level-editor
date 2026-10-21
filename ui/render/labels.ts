// Words over the view (panels.cpp): View > Labels, the names of the actors near the camera and of
// the selection (ui_labels); in the empty view, where to start (ui_viewport_hint).

import { engine } from "../lib/engine.svelte";
import { t } from "../lib/i18n";
import { call, onEngine } from "./engine";
import {
  aabbCenter,
  aabbContains,
  aabbExpanded,
  aabbValid,
  add,
  col3,
  length,
  sub,
  type Vec2,
} from "./math";
import { store } from "./scene_store";
import { NodeFlag, NodeKind } from "./types";
import { viewport } from "./viewport_api";

export const FONT = "13px sans-serif";
export const LINE_HEIGHT = 16;
export const rgba = (r: number, g: number, b: number, a = 255) =>
  `rgba(${r},${g},${b},${a / 255})`;

/** Lines of text in a box, its top-left at `at` (ImGui's AddRectFilled and AddText). */
export function textBox(
  ctx: CanvasRenderingContext2D,
  text: string,
  at: Vec2,
  pad: Vec2,
  radius: number,
  bg: string,
  fg: string,
) {
  const lines = text.split("\n");
  ctx.font = FONT;
  ctx.textBaseline = "top";
  const w = Math.max(...lines.map((l) => ctx.measureText(l).width));
  ctx.fillStyle = bg;
  ctx.beginPath();
  ctx.roundRect(
    at[0] - pad[0],
    at[1] - pad[1],
    w + pad[0] * 2,
    lines.length * LINE_HEIGHT + pad[1] * 2,
    radius,
  );
  ctx.fill();
  ctx.fillStyle = fg;
  lines.forEach((l, i) => ctx.fillText(l, at[0], at[1] + i * LINE_HEIGHT + 1));
}

// the names (an actor's with its etype), asked again when the document changes
let names = new Map<number, string>();
let stale = true;
let busy = false;

function labels(ctx: CanvasRenderingContext2D) {
  if (!engine.state.view.labels) return;
  if (stale && !busy) {
    stale = false;
    busy = true;
    call("labels")
      .then((list: { id: number; name: string; etype?: string | null }[]) => {
        names = new Map(
          (list ?? []).map((l) => [
            l.id,
            "etype" in l ? `${l.name}  (${l.etype ?? ""})` : l.name,
          ]),
        );
      })
      .catch(() => {})
      .finally(() => {
        busy = false;
        viewport.requestRedraw();
      });
  }
  const cam = viewport.camera.pos,
    [w, h] = viewport.size();
  const few_selected = store.selection.length <= 12;
  let drawn = 0;
  for (const n of store.list) {
    if (drawn > 300) break;
    const sel = few_selected && (n.flags & NodeFlag.Selected) !== 0;
    if (!sel && (n.kind !== NodeKind.Actor || !viewport.settings.show_actors)) continue;
    if (!(n.flags & NodeFlag.Visible)) continue;
    const text = names.get(n.id);
    if (text === undefined) continue;
    let p = col3(store.world(n), 3);
    const b = store.bounds(n);
    // meshes in world space: origin at 0
    if (aabbValid(b) && !aabbContains(aabbExpanded(b, 1), p)) p = aabbCenter(b);
    if (!sel && length(sub(p, cam)) > 60) continue;
    const s = viewport.project(add(p, [0, 0.9, 0]));
    if (!s || s[0] < 0 || s[1] < 0 || s[0] > w || s[1] > h) continue;
    textBox(
      ctx,
      text,
      s,
      [3, 1],
      3,
      rgba(10, 12, 14, 150),
      sel ? rgba(255, 180, 80) : rgba(230, 232, 236, 230),
    );
    drawn++;
  }
}

// a word in the empty viewport: where to start
function hint(ctx: CanvasRenderingContext2D) {
  const s = engine.state;
  if (!engine.ready || s.document.open || s.tool === "place") return;
  const [w, h] = viewport.size();
  ctx.font = FONT;
  ctx.textBaseline = "top";
  ctx.fillStyle = rgba(200, 210, 220, 170);
  let y = h * 0.44;
  for (const l of [
    t(s.library_empty ? "viewport.hint.extract" : "viewport.hint.load"),
    t("viewport.hint.camera"),
  ]) {
    ctx.fillText(l, (w - ctx.measureText(l).width) * 0.5, y);
    y += LINE_HEIGHT + 8;
  }
}

/** Draws them over the view; returns what stops it. */
export function attachLabels(): () => void {
  const offs = [
    onEngine("jle:changed", (p) => {
      if (!p || p.scene || p.selection || p.document) stale = true;
    }),
    viewport.addOverlay2d((ctx) => {
      labels(ctx);
      hint(ctx);
    }),
  ];
  return () => offs.forEach((f) => f());
}
