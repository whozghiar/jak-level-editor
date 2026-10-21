// The holes found (draw_gaps_overlay, building_tools.cpp): red a gap between pieces, orange an open
// edge, the bigger the more rays went through; drawn while the Holes window is open, in captures
// too. The engine keeps the last search (gaps_report: the window's, or the remote control's
// find_gaps, which opens the window as the C++ did).

import { call, engineReady, onEngine } from "./engine";
import { packRgba, type Vec3 } from "./math";
import { viewport } from "./viewport_api";

export type GapSpot = {
  at: Vec3;
  seen_from: Vec3;
  direction: Vec3;
  rays: number;
  edge: boolean;
};
export type GapReport = {
  spots: GapSpot[];
  gaps: number;
  edges: number;
  viewpoints: number;
  rays: number;
};

export const gaps = $state({
  report: null as GapReport | null,
  // the spot looked at (m_gap_selected): a line from where the player sees it
  selected: -1,
  // the Holes window is open (show_gaps_window)
  shown: false,
});

let reading: Promise<unknown> = Promise.resolve();

/** The engine's last search read again (a capture waits for it). */
export function refreshGaps(): Promise<unknown> {
  reading = call("gaps_report")
    .then((r) => (gaps.report = r ?? null))
    .catch(() => {})
    .finally(() => viewport.requestRedraw());
  return reading;
}

export function attachGapsOverlay(): () => void {
  const offs = [
    viewport.addOverlay(
      (r) => {
        const spots = gaps.shown ? (gaps.report?.spots ?? []) : [];
        if (!spots.length) return;
        for (let i = 0; i < spots.length && i < 200; i++) {
          const s = spots[i];
          const col = s.edge
            ? packRgba(1, 0.75, 0.2, 0.9)
            : packRgba(1, 0.25, 0.2, 0.95);
          const radius = 0.4 + Math.min(2, s.rays * 0.1);
          r.circle(s.at, [1, 0, 0], [0, 0, 1], radius, col, 20);
          r.circle(s.at, [1, 0, 0], [0, 1, 0], radius, col, 20);
          if (i === gaps.selected) r.line(s.seen_from, s.at, col);
        }
        r.flush(false);
      },
      { capture: true },
    ),
  ];
  const hook = async () => void (await reading);
  viewport.captureHooks.push(hook);
  offs.push(
    () =>
      (viewport.captureHooks = viewport.captureHooks.filter((h) => h !== hook)),
  );
  void engineReady.then(() => {
    offs.push(
      onEngine("jle:changed", (p: any) => {
        if (p?.gaps) void refreshGaps();
      }),
    );
    void refreshGaps();
  });
  return () => offs.forEach((off) => off());
}
