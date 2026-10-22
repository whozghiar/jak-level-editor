<script lang="ts" module>
  // kept while the tab is closed (the C++ App's m_levels_filter, m_levels_jak2, m_selected_level)
  let filter = $state("");
  let jak2 = $state(false);
  let selected = $state("");
</script>

<script lang="ts">
  // The Levels tab (ui_levels_panel, panels.cpp): the levels of the library (Jak 3's, or Jak II's
  // once extracted), one loaded or added to the document at its place in the game's world (the
  // document's levels are the inspector's, with nothing selected).
  import { confirmDiscard } from "../../lib/actions.svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import {
    button,
    check,
    input,
    muted,
    primary,
    small,
    wip,
  } from "../common/classes";
  import Tip from "../common/Tip.svelte";
  import Thumb from "./Thumb.svelte";

  type Level = {
    name: string;
    dgo: string;
    listed: boolean;
    background: string;
    mtime: number;
    in_document: "open" | "added" | "companion" | "";
  };

  const s = $derived(engine.state);
  // the game: Jak 3's levels, or Jak II's (its decor brought into Jak 3) once it is extracted
  const game2 = $derived(!!s.jak2_available && jak2);
  const opening = $derived(!!s.levels?.opening);
  const adding = $derived(!!s.levels?.adding);
  const loading = $derived(opening || adding);

  let levels = $state<Level[]>([]);
  let asked = 0;
  // read again when the game, the library or the document's levels change (the marks)
  // (a value: the effect runs again when it changes, not at each state the engine sends)
  const listKey = $derived(
    JSON.stringify([s.library_root, s.library_empty, s.levels?.document]),
  );
  $effect(() => {
    const args = { jak2: game2 };
    void listKey;
    const n = ++asked;
    call<{ levels: Level[] }>("list_levels", args, true).then((r) => {
      if (r && n === asked) levels = r.levels.filter((l) => l.listed);
    });
  });

  const shown = $derived.by(() => {
    const f = filter.toLowerCase();
    return levels.filter(
      (l) => !f || l.name.includes(f) || l.dgo.toLowerCase().includes(f),
    );
  });
  const chosen = $derived(levels.find((l) => l.name === selected));

  function load(l: Level) {
    const args = { name: l.name, jak2: game2 };
    confirmDiscard(() => void call("open_level", args));
  }
</script>

<div class="flex flex-col gap-2 p-3 text-sm">
  {#if s.library_empty}
    <p class="text-neutral-300">{t("levels.no_library")}</p>
    <button class="{primary} py-2.5" onclick={() => windows.open("extract")}
      >{t("levels.extract")}</button
    >
    {#if s.library_root}
      <p class="mt-2 {muted}">{t("levels.folder_empty")}</p>
      <p class="text-xs break-all text-neutral-300">{s.library_root}</p>
    {/if}
  {:else}
    {#if s.jak2_available}
      <div class="flex items-center gap-3">
        <span class="text-neutral-400">{t("levels.of")}</span>
        {#each [false, true] as two}
          <label class="flex cursor-pointer items-center gap-1.5">
            <input
              type="radio"
              class={check}
              name="levels-game"
              checked={jak2 === two}
              onchange={() => (jak2 = two)}
            />
            {two ? "Jak II" : "Jak 3"}
          </label>
        {/each}
      </div>
      <Tip text={t("levels.jak2.tip")} />
    {/if}
    <input class={input} placeholder={t("levels.filter")} bind:value={filter} />
    <p class={muted}>{t("levels.count", shown.length, levels.length)}</p>

    <div
      class="h-[38vh] min-h-40 overflow-y-auto rounded border border-white/5 bg-[#0b0b0b] py-1"
    >
      {#each shown as l (l.name)}
        <button
          class="flex w-full items-center gap-2 px-2 py-0.5 text-left {l.name ===
          selected
            ? 'bg-orange-500/20 text-white'
            : 'text-neutral-200 hover:bg-white/5'}"
          onclick={() => (selected = l.name)}
          ondblclick={() => !loading && load(l)}
        >
          <span class="min-w-0 truncate">{l.name}</span>
          {#if l.in_document === "open"}
            <span class="ml-auto shrink-0 text-xs text-green-400"
              >{t("levels.open_mark")}</span
            >
          {:else if l.in_document}
            <span class="ml-auto shrink-0 text-xs text-sky-400"
              >{t("levels.added_mark")}</span
            >
          {:else}
            <span class="ml-auto shrink-0 truncate text-xs text-neutral-500"
              >{l.dgo}</span
            >
          {/if}
        </button>
      {:else}
        <p class="px-2 {muted}">{t("levels.none_found")}</p>
      {/each}
    </div>

    <!-- the chosen level: a picture and the button that loads it -->
    {#if chosen}
      <div class="flex items-start gap-3">
        <div class="shrink-0 rounded border border-white/5 bg-[#0b0b0b]">
          <Thumb
            req={{
              kind: "level",
              path: chosen.background,
              stamp: chosen.mtime,
            }}
            size={96}
          />
        </div>
        <div class="min-w-0">
          <p class="truncate font-semibold">{chosen.name}</p>
          <p class={muted}>{chosen.dgo}</p>
        </div>
      </div>
    {/if}
    {#if game2}
      <p class={muted}>{t("levels.jak2.note")}</p>
      <p class="flex items-center gap-2 {muted}">
        <span class={wip}>{t("wip")}</span>{t("menu.game.wip_tip", "Jak II")}
      </p>
    {:else}
      <label class="flex cursor-pointer items-center gap-2 self-start">
        <input
          type="checkbox"
          class={check}
          checked={s.load_actors}
          onchange={(e) =>
            call("set_load_actors", { actors: e.currentTarget.checked })}
        />
        {t("levels.with_actors")}
      </label>
      <Tip text={t("levels.with_actors.tip")} />
    {/if}
    <button
      class="{primary} py-2"
      disabled={!chosen || loading}
      onclick={() => chosen && load(chosen)}
    >
      {t(opening ? "levels.loading" : "levels.load")}
    </button>
    <!-- or add it to the document, at its place in the game's world -->
    <span class="flex">
      <button
        class="{button} flex-1"
        disabled={!chosen ||
          loading ||
          !s.document.open ||
          !!chosen.in_document}
        onclick={() =>
          chosen && call("add_level", { name: chosen.name, jak2: game2 })}
      >
        {t(adding ? "levels.adding" : "levels.add")}
      </button>
    </span>
    <Tip
      text={t(
        chosen?.in_document ? "levels.add.in_document" : "levels.add.tip",
      )}
    />
    <button class="{small} self-start" onclick={() => windows.open("extract")}
      >{t("levels.extract_more")}</button
    >
  {/if}
</div>
