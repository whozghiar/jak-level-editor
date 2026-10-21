<script lang="ts" module>
  import { untrack } from "svelte";
  import { save, ui } from "../../lib/actions.svelte";
  import { engine } from "../../lib/engine.svelte";
  import { has, t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import Tip from "../common/Tip.svelte";
  import { guideDemos, guideSections, tutorials, type TourAction } from "../guide/guide";
  import ToolDemo from "../guide/ToolDemo.svelte";

  // the tutorial shown (0: Getting started) and its step, kept while the window is closed
  export const guide = $state({ tutorial: 0, step: 0 });

  /** Shows tutorial `id` ("game_view") at `step` (select_tutorial); false: there is none. */
  export function selectTutorial(id: string, step = 0): boolean {
    const i = tutorials.findIndex((x) => x.id === id);
    if (i < 0) return false;
    guide.tutorial = i;
    guide.step = step;
    return true;
  }

  // the guided tour, the first time the editor starts (App::run): guide_seen in the preferences
  call<{ guide_seen?: boolean }>("get_prefs", {}, true).then((p) => {
    if (!p || p.guide_seen) return;
    call("set_prefs", { guide_seen: true }, true);
    windows.open("guide", { tab: 0 });
  });

  // in a plain browser (npx vite): ?window=guide[&tab=1][&tutorial=move][&step=2] opens it, for screenshots
  if (import.meta.env.DEV) {
    const q = new URLSearchParams(location.search);
    if (q.get("window") === "guide")
      queueMicrotask(() =>
        windows.open("guide", {
          tab: Number(q.get("tab") ?? 0),
          tutorial: q.get("tutorial") ?? undefined,
          step: Number(q.get("step") ?? 0),
        }),
      );
  }
</script>

<script lang="ts">
  // Help, F1 (ui_guide_window in guide_panel.cpp): the tutorials, by name, a step at a time with its
  // animations and a button that shows where it is; every tool with its animation, what it does, a
  // concrete example; the keys. Props: tab (0 the tutorials, 1 every tool, 2 the keys), tutorial and
  // step (select_tutorial).
  let { tab = 0, tutorial, step = 0 }: { tab?: number; tutorial?: string; step?: number } = $props();

  let current = $state(0);
  // each opening chooses its tab (guide_tab_request): windows.open gives new props each time
  const opened = $derived(windows.list.find((w) => w.id === "guide")?.props);
  $effect.pre(() => {
    opened;
    untrack(() => {
      current = tab;
      if (tutorial) selectTutorial(tutorial, step);
    });
  });

  const tabs = ["guide.tab.tour", "guide.tab.tools", "guide.tab.keys"];
  // the keys tab: help.<section>.<n>, in this order
  const keys: [string, number[]][] = [
    ["help.camera", [1, 2, 3, 4]],
    ["help.story", [1, 2, 3]],
    ["help.editing", [1, 2, 10, 3, 4, 5, 11, 12, 6, 7, 8, 9]],
  ];

  const ti = $derived(Math.min(Math.max(guide.tutorial, 0), tutorials.length - 1));
  const tut = $derived(tutorials[ti]);
  const si = $derived(Math.min(Math.max(guide.step, 0), tut.steps.length - 1));
  const s = $derived(tut.steps[si]);
  const k = $derived(tut.prefix + s[0]);
  const demos = $derived(s[1]);
  const action = $derived(s[2]);
  const needsDocument = $derived(action === "replace" || action === "export" || action === "save" || action === "navmeshes");
  const blocked = $derived(needsDocument && !engine.state.document.open);

  function go(i: number, j: number) {
    guide.tutorial = i;
    guide.step = j;
  }

  function run(a: TourAction) {
    if (a === "extract") windows.open("extract");
    else if (a === "levels_tab") ui.leftTab = "levels";
    else if (a === "catalog_tab") ui.leftTab = "catalog";
    else if (a === "prefabs_tab") ui.leftTab = "prefabs";
    else if (a === "build_tab") ui.leftTab = "build";
    else if (a === "replace") {
      call("select_only", { id: 0 });
      ui.scrollToReplace = true;
    } else if (a === "export") windows.open("export");
    else if (a === "game_view") call("set_view", { game_view: !!engine.state.view.game_view_ready });
    else if (a === "save") save(false);
    else if (a === "navmeshes") windows.open("navmesh");
  }

  // its animations: one wide, two side by side, four in a square
  let room = $state(0);
  let roomH = $state(0);
  const gap = 8;
  const size = $derived.by(() => {
    const n = demos.length;
    const perRow = n === 1 ? 1 : 2;
    const rows = Math.ceil(n / perRow);
    let w = (room - gap * (perRow - 1)) / perRow;
    let h = (w * 9) / 16;
    const maxH = roomH / rows - gap;
    if (h > maxH && maxH > 60) {
      h = maxH;
      w = (h * 16) / 9;
    }
    return { w: Math.max(0, Math.floor(w)), h: Math.max(0, Math.floor(h)) };
  });

  const button =
    "rounded border border-[#2a2a2a] bg-[#0b0b0b] px-3 py-1 text-xs hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-40";
  const primary =
    "rounded bg-orange-500 px-3 py-1 text-xs font-semibold text-black hover:bg-orange-400 disabled:pointer-events-none disabled:opacity-40";
</script>

{#snippet heading(text: string)}
  <div class="mt-3 mb-2 flex items-center gap-2 text-xs font-semibold text-neutral-400 first:mt-0">
    <span class="shrink-0">{text}</span>
    <span class="h-px flex-1 bg-white/10"></span>
  </div>
{/snippet}

{#snippet title(key: string)}
  <p class="font-semibold text-orange-400">
    {t(`${key}.title`)}
    {#if has(`${key}.keys`)}<span class="ml-1 font-normal text-neutral-500">{t(`${key}.keys`)}</span>{/if}
  </p>
{/snippet}

<div class="flex h-full min-h-0 flex-col">
  <div class="mb-3 flex shrink-0 gap-4 border-b border-white/10">
    {#each tabs as key, i}
      <button
        class="-mb-px border-b-2 px-1 pb-1.5 text-sm {current === i
          ? 'border-orange-500 text-white'
          : 'border-transparent text-neutral-400 hover:text-neutral-200'}"
        onclick={() => (current = i)}
      >
        {t(key)}
      </button>
    {/each}
  </div>

  {#if current === 0}
    <div class="flex min-h-0 flex-1 gap-3">
      <!-- the tutorials, by name, the steps of the one chosen -->
      <div class="w-[250px] shrink-0 overflow-y-auto rounded border border-white/5 bg-[#0b0b0b] p-1 text-xs">
        {#each tutorials as x, i}
          <button
            class="block w-full rounded px-2 py-1 text-left hover:bg-white/5 {i === ti ? 'bg-white/5 text-orange-400' : 'text-neutral-200'}"
            onclick={() => i !== ti && go(i, 0)}
          >
            {i + 1}. {t(`tuto.${x.id}.title`)}
          </button>
          {#if i === ti}
            {#each x.steps as st, j}
              <button
                class="block w-full rounded py-0.5 pr-2 pl-5 text-left hover:bg-white/5 {j === si
                  ? 'bg-orange-500/20 text-white'
                  : 'text-neutral-400'}"
                onclick={() => go(i, j)}
              >
                {i + 1}.{j + 1}
                {t(`${x.prefix}${st[0]}.title`)}
              </button>
            {/each}
          {/if}
        {/each}
      </div>

      <!-- a step at a time: its text, its animations, then the button that shows it, previous, next -->
      <div class="flex min-w-0 flex-1 flex-col">
        <p class="text-xs text-neutral-500">{t(`tuto.${tut.id}.title`)} - {t("tuto.step", si + 1, tut.steps.length)}</p>
        <p class="mt-2 font-semibold text-orange-400">{t(`${k}.title`)}</p>
        <p class="mt-1 whitespace-pre-line text-neutral-200">{t(`${k}.text`)}</p>
        <div class="relative mt-3 min-h-0 flex-1" bind:clientWidth={room} bind:clientHeight={roomH}>
          {#if size.w > 0}
            {#key `${ti}.${si}`}
              <div class="absolute inset-0 flex flex-wrap content-start" style:gap="{gap}px">
                {#each demos as d}
                  <ToolDemo name={d} width={size.w} height={size.h} class="rounded" />
                {/each}
              </div>
            {/key}
          {/if}
        </div>
        <div class="mt-2 flex shrink-0 items-center gap-2 border-t border-white/10 pt-2">
          {#if action}
            <span>
              <button class={button} disabled={blocked} onclick={() => run(action)}>{t(`${k}.action`)}</button>
            </span>
            {#if blocked}<Tip text={t("tour.needs_level")} placement="top" />{/if}
          {/if}
          <button class="{button} ml-auto w-32" disabled={si === 0} onclick={() => go(ti, si - 1)}>{t("tour.previous")}</button>
          {#if si + 1 < tut.steps.length}
            <button class="{primary} w-32" onclick={() => go(ti, si + 1)}>{t("tour.next")}</button>
          {:else if ti + 1 < tutorials.length}
            <!-- the next tutorial -->
            <button class="{primary} w-32" onclick={() => go(ti + 1, 0)}>{t("tuto.next")}</button>
            <Tip text={t(`tuto.${tutorials[ti + 1].id}.title`)} placement="top" />
          {:else}
            <button class="{primary} w-32" onclick={() => windows.close("guide")}>{t("tour.finish")}</button>
          {/if}
        </div>
      </div>
    </div>
  {:else if current === 1}
    <!-- every tool: its animation, what it does, a concrete example -->
    <div class="min-h-0 flex-1 overflow-y-auto pr-1">
      {#each guideSections as section}
        {@render heading(t(section.key))}
        {#each section.ids as id}
          {@const key = `guide.${id}`}
          <div class="mb-2 flex gap-3">
            {#if guideDemos[id]}
              <ToolDemo name={guideDemos[id]} since={0} class="rounded" />
            {:else}
              <div class="h-[126px] w-[224px] shrink-0"></div>
            {/if}
            <div class="min-w-0 flex-1">
              {@render title(key)}
              <p class="mt-0.5 whitespace-pre-line text-neutral-200">{t(`${key}.text`)}</p>
              {#if has(`${key}.example`)}<p class="mt-1 whitespace-pre-line text-neutral-400 italic">{t(`${key}.example`)}</p>{/if}
            </div>
          </div>
        {/each}
      {/each}
    </div>
  {:else}
    <!-- the keys, and where things are -->
    <div class="min-h-0 flex-1 overflow-y-auto pr-1">
      {#each keys as [section, items]}
        {@render heading(t(section))}
        <ul class="list-disc space-y-1 pl-5 text-neutral-200">
          {#each items as i}<li>{t(`${section}.${i}`)}</li>{/each}
        </ul>
      {/each}
    </div>
  {/if}
</div>
