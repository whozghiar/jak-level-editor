<script lang="ts">
  // The toolbar (ui_toolbar in panels.cpp): the gizmo's modes, the tools, Local, Snap and its steps,
  // copy / paste / delete, a prefab, undo / redo.
  import type { Component } from "svelte";
  import IconAxis from "~icons/mdi/axis-arrow";
  import IconChevronDown from "~icons/mdi/chevron-down";
  import IconCopy from "~icons/mdi/content-copy";
  import IconPaste from "~icons/mdi/content-paste";
  import IconMove from "~icons/mdi/cursor-move";
  import IconDelete from "~icons/mdi/delete";
  import IconMagnet from "~icons/mdi/magnet";
  import IconPrefab from "~icons/mdi/package-variant-plus";
  import IconRedo from "~icons/mdi/redo";
  import IconScale from "~icons/mdi/resize";
  import IconRotate from "~icons/mdi/rotate-3d-variant";
  import IconTerrain from "~icons/mdi/terrain";
  import IconUndo from "~icons/mdi/undo";
  import IconTopology from "~icons/mdi/vector-polyline";
  import { engine, type GizmoMode } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import Tip from "../common/Tip.svelte";

  const s = $derived(engine.state);
  const sel = $derived(s.selection.count > 0);
  const snap = $derived(s.gizmo.snap);

  function gizmo(mode: GizmoMode) {
    call("stop_placement");
    call("set_gizmo", { mode });
  }

  let stepsOpen = $state(false);
  let steps: HTMLElement;
  const setSnap = (k: string, value: number) => call("set_gizmo", { snap: { [k]: value } });

  const btn =
    "flex h-8 items-center gap-1.5 rounded border px-2 text-sm whitespace-nowrap disabled:cursor-not-allowed disabled:opacity-40";
  const idle = "border-[#2a2a2a] bg-[#0b0b0b] text-neutral-200 enabled:hover:border-[#3a3a3a] enabled:hover:bg-[#1a1a1a]";
  const active = "border-orange-500 bg-orange-500/15 text-orange-300";
  const field =
    "w-20 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-0.5 text-right text-sm text-neutral-200 focus:border-orange-500 focus:outline-none";
</script>

<svelte:window onpointerdown={(e) => stepsOpen && !steps.contains(e.target as Node) && (stepsOpen = false)} />

{#snippet button(
  Icon: Component,
  label: string,
  run: () => void,
  opts: { on?: boolean; enabled?: boolean; guide?: string; tip?: string; short?: boolean },
)}
  <span class="inline-flex">
    <button class="{btn} {opts.on ? active : idle}" disabled={opts.enabled === false} aria-label={label} onclick={run}>
      <Icon class="text-base" /><span class={opts.short ? "max-2xl:hidden" : "max-xl:hidden"}>{label}</span>
    </button>
  </span>
  <Tip guide={opts.guide} text={opts.tip} />
{/snippet}

{#snippet toggle(Icon: Component, label: string, on: boolean, run: () => void, guide: string)}
  <span class="inline-flex">
    <button class="{btn} {on ? active : idle}" aria-pressed={on} aria-label={label} onclick={run}>
      <Icon class="text-base" /><span class="max-xl:hidden">{label}</span>
    </button>
  </span>
  <Tip {guide} />
{/snippet}

<div class="relative z-[35] flex h-11 shrink-0 items-center gap-1.5 border-b border-white/5 bg-[#101010] px-2">
  {@render button(IconMove, t("toolbar.move"), () => gizmo("translate"), { on: s.gizmo.mode === "translate", guide: "move" })}
  {@render button(IconRotate, t("toolbar.rotate"), () => gizmo("rotate"), { on: s.gizmo.mode === "rotate", guide: "rotate" })}
  {@render button(IconScale, t("toolbar.scale"), () => gizmo("scale"), { on: s.gizmo.mode === "scale", guide: "scale" })}
  {@render button(IconTopology, t("toolbar.topology"), () => call(s.tool === "topology" ? "stop_topology" : "start_topology"), {
    on: s.tool === "topology",
    enabled: s.tool === "topology" || s.selection.topology_editable,
    guide: "topology",
  })}
  {@render button(IconTerrain, t("toolbar.terrain"), () => call(s.tool === "terrain" ? "stop_terrain_brush" : "start_terrain_brush"), {
    on: s.tool === "terrain",
    enabled: s.document.open,
    guide: "terrain",
  })}

  <div class="mx-1 h-6 border-l border-neutral-800"></div>
  {@render toggle(IconAxis, t("toolbar.local"), s.gizmo.local, () => call("set_gizmo", { local: !s.gizmo.local }), "local")}
  <div bind:this={steps} class="relative flex">
    {@render toggle(IconMagnet, t("toolbar.snap"), snap.enabled, () => call("set_gizmo", { snap: { enabled: !snap.enabled } }), "snap")}
    <span class="inline-flex">
      <button class="{btn} {idle} ml-0.5 px-1" aria-label={t("toolbar.snap.steps")} onclick={() => (stepsOpen = !stepsOpen)}>
        <IconChevronDown />
      </button>
    </span>
    <Tip text={t("toolbar.snap.steps.tip")} />
    {#if stepsOpen}
      <!-- the steps of the snap: the grid of modular building (walls of 4 m on a 0.5 m grid...) -->
      <div class="absolute top-full left-0 mt-1 w-max rounded-md border border-white/10 bg-[#101010] p-3 text-sm shadow-xl shadow-black/60">
        <p class="mb-2 text-xs text-neutral-500">{t("toolbar.snap.steps")}</p>
        {#each [{ k: "translate", key: "toolbar.snap.move", min: 0.05, max: 64, step: 0.05, unit: "m", presets: [0.1, 0.25, 0.5, 1, 2, 4] }, { k: "rotate_deg", key: "toolbar.snap.turn", min: 1, max: 180, step: 1, unit: "deg", presets: [5, 15, 45, 90] }, { k: "scale", key: "toolbar.snap.scale", min: 0.01, max: 10, step: 0.01, unit: "", presets: [] }] as f}
          <div class="mb-1.5 flex items-center gap-1.5">
            <input
              type="number"
              class={field}
              min={f.min}
              max={f.max}
              step={f.step}
              value={Math.round((snap as any)[f.k] * 100) / 100}
              onchange={(e) => {
                const n = +e.currentTarget.value;
                if (n >= f.min && n <= f.max) setSnap(f.k, n);
              }}
            />
            <span class="w-24 text-neutral-300">{f.unit} {t(f.key)}</span>
            {#each f.presets as p}
              <button class="rounded bg-white/5 px-1.5 text-xs hover:bg-orange-500/30" onclick={() => setSnap(f.k, p)}>{p}</button>
            {/each}
          </div>
        {/each}
        <p class="mt-2 max-w-72 text-xs text-neutral-500">{t("toolbar.snap.keys")}</p>
      </div>
    {/if}
  </div>

  <div class="mx-1 h-6 border-l border-neutral-800"></div>
  {@render button(IconCopy, t("toolbar.copy"), () => call("copy_selection"), { enabled: sel, tip: t("toolbar.copy.tip"), short: true })}
  {@render button(IconPaste, t("toolbar.paste"), () => call("paste"), { enabled: !s.clipboard_empty, tip: t("toolbar.paste.tip"), short: true })}
  {@render button(IconDelete, t("toolbar.delete"), () => call("delete_selection"), { enabled: sel, tip: t("toolbar.delete.tip"), short: true })}
  <div class="mx-1 h-6 border-l border-neutral-800"></div>
  {@render button(IconPrefab, t("toolbar.prefab"), () => windows.open("new_prefab"), { enabled: sel, guide: "prefab", short: true })}
  <div class="mx-1 h-6 border-l border-neutral-800"></div>
  {@render button(IconUndo, t("toolbar.undo"), () => call("undo"), { enabled: s.history.can_undo, tip: s.history.undo_label || t("toolbar.undo"), short: true })}
  {@render button(IconRedo, t("toolbar.redo"), () => call("redo"), { enabled: s.history.can_redo, tip: s.history.redo_label || t("toolbar.redo"), short: true })}
</div>
