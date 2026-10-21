<script lang="ts">
  // Level > Find holes in the decor (ui_gaps_window and draw_gaps_overlay, building_tools.cpp):
  // where a player walking the level would see the void. The engine looks (find_gaps, around the
  // selection, else the document's own level) and keeps what it found (gaps_report: the remote
  // control's search shows here too); the spots are drawn in the view while the window is open
  // (render/gaps_overlay.svelte.ts).
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { gaps, refreshGaps, type GapReport } from "../../render/gaps_overlay.svelte";
  import { viewport } from "../../render/viewport_api";
  import Tip from "../common/Tip.svelte";

  const report = $derived(gaps.report);
  let running = $state(false);

  async function run() {
    running = true;
    const r = await call<GapReport>("find_gaps");
    running = false;
    if (r) {
      gaps.report = r;
      gaps.selected = -1;
    }
  }

  // looked at from where the player would see it
  function look(i: number) {
    gaps.selected = i;
    const s = report!.spots[i];
    viewport.setCamera({ pos: s.seen_from, look_at: s.at });
  }

  onMount(() => {
    gaps.shown = true;
    void refreshGaps();
    return () => {
      gaps.shown = false;
      viewport.requestRedraw();
    };
  });

  $effect(() => {
    void [gaps.report, gaps.selected];
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
          gaps.selected
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
