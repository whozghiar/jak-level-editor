<script lang="ts" module>
  // the spot looked at, kept while the window is closed (m_gap_selected)
  let selected = $state(-1);
</script>

<script lang="ts">
  // Level > Find holes in the decor (ui_gaps_window and draw_gaps_overlay, building_tools.cpp):
  // where a player walking the level would see the void. The engine looks (find_gaps, around the
  // selection, else the document's own level) and keeps what it found (gaps_report: the remote
  // control's search shows here too); the spots are drawn in the view while the window is open.
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { packRgba, type Vec3 } from "../../render/math";
  import { viewport } from "../../render/viewport_api";
  import Tip from "../common/Tip.svelte";

  type Spot = {
    at: Vec3;
    seen_from: Vec3;
    direction: Vec3;
    rays: number;
    edge: boolean;
  };
  type Report = {
    spots: Spot[];
    gaps: number;
    edges: number;
    viewpoints: number;
    rays: number;
  };

  let report = $state<Report | null>(null);
  let running = $state(false);

  const refresh = async () =>
    (report = (await call<Report | null>("gaps_report", {}, true)) ?? null);

  async function run() {
    running = true;
    const r = await call<Report>("find_gaps");
    running = false;
    if (r) {
      report = r;
      selected = -1;
    }
  }

  // looked at from where the player would see it
  function look(i: number) {
    selected = i;
    const s = report!.spots[i];
    viewport.setCamera({ pos: s.seen_from, look_at: s.at });
  }

  onMount(() => {
    void refresh();
    const offs = [
      onEngine("jle:changed", (p) => p?.gaps && refresh()),
      // red: a gap between pieces, orange: an open edge; the bigger, the more rays went through
      viewport.addOverlay(
        (r) => {
          const spots = report?.spots ?? [];
          for (let i = 0; i < spots.length && i < 200; i++) {
            const s = spots[i];
            const col = s.edge
              ? packRgba(1, 0.75, 0.2, 0.9)
              : packRgba(1, 0.25, 0.2, 0.95);
            const radius = 0.4 + Math.min(2, s.rays * 0.1);
            r.circle(s.at, [1, 0, 0], [0, 0, 1], radius, col, 20);
            r.circle(s.at, [1, 0, 0], [0, 1, 0], radius, col, 20);
            if (i === selected) r.line(s.seen_from, s.at, col);
          }
          r.flush(false);
        },
        { capture: true },
      ),
    ];
    return () => offs.forEach((off) => off());
  });

  $effect(() => {
    void [report, selected];
    viewport.requestRedraw();
  });
</script>

<div class="flex h-full flex-col gap-2 text-sm">
  <p class="text-neutral-500">{t("gaps.help")}</p>
  <button
    class="rounded border border-[#2a2a2a] bg-[#0b0b0b] px-3 py-1 hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-50"
    disabled={!engine.state.document.open || running}
    onclick={run}
  >
    {t(engine.state.selection.count ? "gaps.run_selection" : "gaps.run_level")}
  </button>
  {#if report}
    <p>
      {t(
        "gaps.result",
        report.gaps,
        report.edges,
        report.viewpoints,
        report.rays,
      )}
    </p>
    <div
      class="max-h-80 min-h-24 flex-1 overflow-auto rounded border border-white/5 bg-[#0b0b0b] py-1"
    >
      {#each report.spots as s, i (i)}
        <button
          class="block w-full px-2 py-0.5 text-left hover:bg-white/5 {i ===
          selected
            ? 'bg-orange-600/25 text-orange-200'
            : s.edge
              ? 'text-orange-300'
              : 'text-red-300'}"
          onclick={() => look(i)}
        >
          {t(
            s.edge ? "gaps.edge" : "gaps.gap",
            s.at[0],
            s.at[1],
            s.at[2],
            s.rays,
          )}
        </button>
      {/each}
    </div>
    <Tip text={t("gaps.look_tip")} placement="left" />
  {/if}
</div>
