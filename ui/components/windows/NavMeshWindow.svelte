<script lang="ts">
  // Level > Nav meshes (ui_navmesh_window, panels.cpp): the creature's parameters, the generation
  // (on the engine's worker: the new meshes replace those of the region, one undo step, selected),
  // what the document has, the nav_data file for the game. The engine keeps the fields
  // (navmesh_state, set_navmesh); the view draws the meshes (render/navmesh_overlay.ts).
  import { save } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { check } from "../common/classes";
  import Tip from "../common/Tip.svelte";

  type Params = Record<string, number>;
  type Stats = {
    meshes: number;
    polys: number;
    split: number;
    left_out: number;
    ms: number;
  };
  type NavState = {
    params: Params;
    keep_selected: boolean;
    first_aid: number;
    busy: boolean;
    generation: number;
    last: { ok: true; stats: Stats } | { ok: false; error: string } | null;
    nodes: number[];
    polys: number;
    area: number;
    bad: number;
    selection_region: boolean;
    selection_points: number;
  };

  // [field, text key, step, min, max, unit, decimals]
  type Field = [string, string, number, number, number, string, number];
  const creature: Field[] = [
    ["agent_radius", "navmesh.radius", 0.05, 0.1, 5, "m", 2],
    ["agent_height", "navmesh.height", 0.05, 0.5, 10, "m", 2],
    ["max_climb", "navmesh.climb", 0.05, 0, 5, "m", 2],
    ["max_slope", "navmesh.slope", 0.5, 5, 85, "deg", 0],
  ];
  const precision: Field[] = [
    ["cell_size", "navmesh.cell", 0.01, 0.1, 4, "m", 2],
    ["max_edge", "navmesh.edge", 0.5, 4, 200, "m", 0],
    ["max_error", "navmesh.error", 0.05, 0.1, 5, "m", 2],
    ["min_region", "navmesh.min_region", 1, 0, 1000, "m2", 0],
  ];

  let st = $state<NavState>();
  const refresh = async () =>
    (st = (await call<NavState>("navmesh_state", {}, true)) ?? st);
  const set = async (args: Record<string, unknown>) =>
    (st = (await call<NavState>("set_navmesh", args)) ?? st);

  function setParam(f: Field, value: number) {
    if (!Number.isFinite(value)) return;
    set({ params: { [f[0]]: Math.min(Math.max(value, f[3]), f[4]) } });
  }

  async function generate(region: "all" | "selection") {
    const s = await call<NavState>("generate_navmesh", {
      region,
      keep_selected: st?.keep_selected ?? false,
    });
    if (s) st = s;
  }

  // what the document has
  async function selectAll() {
    if (st?.nodes.length)
      await call("select", { ids: st.nodes, mode: "replace" });
  }
  async function deleteAll() {
    if (!st?.nodes.length) return;
    await call("select", { ids: st.nodes, mode: "replace" });
    await call("delete_selection");
  }

  async function exportFile() {
    const path = await save({
      filters: [{ name: "nav_data (.json)", extensions: ["json"] }],
    });
    if (path) await call("export_navmeshes", { path });
  }

  onMount(() => {
    void refresh();
    const offs = [
      onEngine("jle:changed", refresh),
      onEngine("jle:progress", (j) => j?.id === "navmesh" && refresh()),
    ];
    return () => offs.forEach((off) => off());
  });

  const busy = $derived(!!st?.busy || !!engine.jobs["navmesh"]);
  const open = $derived(engine.state.document.open);
  const keepBlocked = $derived(!!st?.keep_selected && !st.selection_points);

  const muted = "text-xs text-neutral-500";
  const warn = "text-orange-400";
  const heading =
    "mt-3 mb-1.5 border-b border-white/10 pb-1 text-xs font-semibold tracking-wide text-neutral-400 uppercase";
  const input =
    "w-24 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-0.5 text-right text-sm outline-none focus:border-orange-600";
  const button =
    "rounded border border-[#2a2a2a] bg-[#0b0b0b] px-3 py-1 text-sm hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-50";
</script>

{#snippet field(f: Field)}
  <label class="flex items-center gap-2">
    <span class="flex-1 text-neutral-300">{t(f[1])}</span>
    <input
      class={input}
      type="number"
      step={f[2]}
      min={f[3]}
      max={f[4]}
      value={(st?.params[f[0]] ?? 0).toFixed(f[6])}
      onchange={(e) => setParam(f, e.currentTarget.valueAsNumber)}
    />
    <span class="w-7 text-xs text-neutral-500">{f[5]}</span>
  </label>
  <Tip text={t(`${f[1]}.tip`)} placement="left" />
{/snippet}

{#if st}
  <div class="flex flex-col gap-1.5 text-sm">
    <p class="text-neutral-300">{t("navmesh.help")}</p>

    <h3 class={heading}>{t("navmesh.creature")}</h3>
    {#each creature as f (f[0])}{@render field(f)}{/each}
    <details class="mt-1">
      <summary
        class="cursor-pointer text-neutral-400 select-none hover:text-neutral-200"
        >{t("navmesh.detail")}</summary
      >
      <div class="mt-1.5 flex flex-col gap-1.5 pl-3">
        {#each precision as f (f[0])}{@render field(f)}{/each}
        <button
          class="{button} self-start"
          onclick={() => set({ defaults: true })}
          >{t("navmesh.defaults")}</button
        >
      </div>
    </details>

    <label class="mt-2 flex items-center gap-2">
      <input
        type="checkbox"
        class={check}
        checked={st.keep_selected}
        onchange={(e) => set({ keep_selected: e.currentTarget.checked })}
      />
      {t("navmesh.keep_selected")}
    </label>
    <Tip text={t("navmesh.keep_selected.tip")} placement="left" />
    <button
      class={button}
      disabled={busy || !open || keepBlocked}
      onclick={() => generate("all")}
    >
      {t("navmesh.generate_all")}
    </button>
    <Tip text={t("navmesh.generate_all.tip")} placement="left" />
    <button
      class={button}
      disabled={busy || !open || keepBlocked || !st.selection_region}
      onclick={() => generate("selection")}
    >
      {t("navmesh.generate_selection")}
    </button>
    <Tip text={t("navmesh.generate_selection.tip")} placement="left" />
    {#if busy}
      <p class={muted}>{t("navmesh.working")}</p>
    {:else if st.last?.ok}
      <p class={muted}>
        {t(
          "navmesh.last",
          st.last.stats.meshes,
          st.last.stats.polys,
          st.last.stats.ms / 1000,
        )}
      </p>
      {#if st.last.stats.split}<p class="text-neutral-300">
          {t("navmesh.split", st.last.stats.split)}
        </p>{/if}
      {#if st.last.stats.left_out}<p class="text-neutral-300">
          {t("navmesh.left_out", st.last.stats.left_out)}
        </p>{/if}
    {:else if st.last}
      <p class={warn}>{st.last.error}</p>
    {/if}

    <h3 class={heading}>{t("navmesh.document")}</h3>
    <p>{t("navmesh.count", st.nodes.length, st.polys, st.area)}</p>
    {#if st.bad}<p class={warn}>{t("navmesh.bad", st.bad)}</p>{/if}
    <div class="flex gap-2">
      <button class={button} disabled={!st.nodes.length} onclick={selectAll}
        >{t("navmesh.select_all")}</button
      >
      <button class={button} disabled={!st.nodes.length} onclick={deleteAll}
        >{t("navmesh.delete_all")}</button
      >
    </div>

    <h3 class={heading}>{t("navmesh.export")}</h3>
    <p class="text-neutral-300">{t("navmesh.export.help")}</p>
    <label class="flex items-center gap-2">
      <span class="text-neutral-300">{t("navmesh.first_id")}</span>
      <input
        class={input}
        type="number"
        step="1"
        min="0"
        value={st.first_aid}
        onchange={(e) => {
          const v = e.currentTarget.valueAsNumber;
          if (Number.isInteger(v) && v >= 0) set({ first_aid: v });
        }}
      />
    </label>
    <Tip text={t("navmesh.first_id.tip")} placement="left" />
    <button
      class={button}
      disabled={!st.nodes.length || st.bad > 0}
      onclick={exportFile}
    >
      {t("navmesh.export_button")}
    </button>
  </div>
{/if}
