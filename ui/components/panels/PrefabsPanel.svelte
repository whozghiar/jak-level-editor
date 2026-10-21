<script lang="ts" module>
  // kept while the tab is closed (the C++ panel's statics)
  let game = $state(""); // "" Jak 3's, "jak2"
  let level = $state("");
  let category = $state("");
  let search = $state("");
  let gameOpen = $state(true);
  let mineOpen = $state(true);
</script>

<script lang="ts">
  // The Prefabs tab (ui_prefabs_panel, panels.cpp): the game's prefabs (ready-made assemblies of its
  // decor, made from the extracted levels the first time they are shown, level by level), by game,
  // level, category and name; the user's own, made of the selection. A click places copies (then a
  // click in the view), a drag into the view places one; the user's can be deleted.
  import IconDown from "~icons/mdi/chevron-down";
  import IconRight from "~icons/mdi/chevron-right";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { check, input, muted, primary, small } from "../common/classes";
  import Dropdown from "../common/Dropdown.svelte";
  import { panelDrag } from "../common/panelDrag";
  import Tip from "../common/Tip.svelte";
  import Thumb from "./Thumb.svelte";
  import ThumbGrid from "./ThumbGrid.svelte";

  type Prefab = {
    name: string;
    path: string;
    mtime: number;
    category: string;
    description: string;
    game: string;
    level: string;
    made: boolean;
  };

  const s = $derived(engine.state);
  let list = $state<{ folder: string; mine: Prefab[]; game: Prefab[]; busy: boolean; library: boolean; jak2: boolean } | null>(null);
  let asked = 0;
  // (a value: the effect runs again when it changes, not at each state the engine sends)
  const listKey = $derived(JSON.stringify([s.catalog?.epoch, s.catalog?.prefabs_busy, s.library_root]));
  $effect(() => {
    void listKey;
    const n = ++asked;
    call("prefabs", {}, true).then((r) => {
      if (n === asked) list = r ?? null;
    });
  });

  const ofGame = $derived((list?.game ?? []).filter((p) => p.game === (list?.jak2 ? game : "")));
  const levels = $derived([...new Set(ofGame.map((p) => p.level))].sort());
  const chosenLevel = $derived(levels.includes(level) ? level : "");
  const categories = $derived([...new Set(ofGame.filter((p) => !chosenLevel || p.level === chosenLevel).map((p) => p.category))]);
  const chosenCategory = $derived(categories.includes(category) ? category : "");
  const shown = $derived.by(() => {
    const f = search.toLowerCase();
    return ofGame.filter(
      (p) =>
        (!chosenLevel || p.level === chosenLevel) &&
        (!chosenCategory || p.category === chosenCategory) &&
        (!f || p.name.toLowerCase().includes(f) || p.description.toLowerCase().includes(f)),
    );
  });
  const made = $derived((list?.game ?? []).filter((p) => p.made).length);
  const job = $derived(engine.jobs["game_prefabs"]);

  // shown not made yet: made now, with its level's (want_game_prefabs), asked once per level
  const wanted = new Set<string>();
  let pending: [string, string][] = [];
  function want(node: HTMLElement, p: Prefab) {
    const ask = (p: Prefab) => {
      const key = `${p.game}/${p.level}`;
      if (!p.made && !wanted.has(key)) {
        wanted.add(key);
        if (!pending.length) queueMicrotask(() => void call("want_game_prefabs", { levels: pending.splice(0) }, true));
        pending.push([p.game, p.level]);
      }
    };
    ask(p);
    // a cell shown again for another prefab (the grid reuses its cells)
    return { update: ask };
  }

  const pl = $derived(s.tool === "place" ? s.placement : null);
  const place = (p: Prefab, once: boolean) => call("start_prefab_placement", { path: p.path, name: p.name, once });
  const req = (p: Prefab) => (p.made ? { kind: "model" as const, path: p.path, stamp: p.mtime } : null);

  // the user's prefab's menu (right click)
  let menu = $state<{ p: Prefab; x: number; y: number } | null>(null);
  function remove(p: Prefab) {
    menu = null;
    call("delete_prefab", { path: p.path });
  }
</script>

<!-- Esc closes the menu (before the editor's key: select nothing) -->
<svelte:window
  onpointerdown={(e) => menu && !(e.target as HTMLElement).closest?.(".prefab-menu") && (menu = null)}
  onkeydowncapture={(e) => menu && e.key === "Escape" && (e.preventDefault(), (menu = null))}
/>

{#snippet cellBody(p: Prefab)}
  <div
    class="rounded border p-[2px] {pl?.path === p.path
      ? 'border-orange-500 bg-orange-500/25'
      : 'border-white/5 bg-[#0b0b0b] hover:border-white/25'} {p.made ? '' : 'opacity-60'}"
  >
    <Thumb req={req(p)} size={96} />
  </div>
  <p class="truncate text-center text-[11px] leading-4 text-neutral-300">{p.name}</p>
{/snippet}

{#snippet header(open: boolean, key: string, toggle: () => void, guide: string)}
  <button class="flex w-full items-center gap-1 rounded bg-white/5 px-2 py-1 text-left font-semibold text-neutral-200 hover:bg-white/10" onclick={toggle}>
    {#if open}<IconDown />{:else}<IconRight />{/if}
    {t(key)}
  </button>
  <Tip {guide} />
{/snippet}

<div class="flex h-full flex-col gap-2 p-3 text-sm">
  {#if !list?.folder}
    <p class="text-neutral-300">{t("prefabs.no_library")}</p>
  {:else}
    <!-- the game's: ready-made assemblies of its decor, by category, made from the extracted levels -->
    {@render header(gameOpen, "prefabs.game", () => (gameOpen = !gameOpen), "game_prefabs")}
    {#if gameOpen}
      <!-- made level by level when they are shown (each level read once, a few seconds), or all at once -->
      {#if list.busy}
        {@const done = job?.done ?? 0}
        {@const total = job?.total ?? 1}
        <div class="relative h-5 overflow-hidden rounded bg-white/10">
          <div class="h-full bg-orange-600/70" style:width="{(100 * done) / Math.max(total, 1)}%"></div>
          <span class="absolute inset-0 text-center text-xs leading-5">{t("prefabs.game.making", done, total)}</span>
        </div>
      {:else if made < list.game.length && list.library}
        <button class={small} onclick={() => call("game_prefabs", {})}>{t("prefabs.game.make", list.game.length - made)}</button>
        <Tip text={t("prefabs.game.make.tip")} />
      {/if}
      <!-- the game (Jak II's when it was extracted), the level they come from, their category -->
      {#if list.jak2}
        <div class="flex items-center gap-3">
          <span class="text-neutral-400">{t("catalog.game")}</span>
          {#each ["", "jak2"] as g}
            <label class="flex cursor-pointer items-center gap-1.5">
              <input type="radio" class={check} name="prefab-game" checked={game === g} onchange={() => ((game = g), (level = category = ""))} />
              {g ? "Jak II" : "Jak 3"}
            </label>
          {/each}
        </div>
      {/if}
      {#if levels.length}
        <Dropdown
          items={[{ label: t("catalog.all_levels"), value: "" }, ...levels.map((l) => ({ label: l, value: l }))]}
          value={chosenLevel}
          label={chosenLevel || t("catalog.all_levels")}
          filter={t("catalog.filter")}
          onselect={(v) => (level = v)}
        />
      {/if}
      {#if categories.length}
        <Dropdown
          items={[{ label: t("prefabs.game.all"), value: "" }, ...categories.map((c) => ({ label: t(`prefabs.category.${c}`), value: c }))]}
          value={chosenCategory}
          label={chosenCategory ? t(`prefabs.category.${chosenCategory}`) : t("prefabs.game.all")}
          onselect={(v) => (category = v)}
        />
      {/if}
      <!-- and a search by name (the games have a thousand of them) -->
      <input class={input} placeholder={t("prefabs.search")} bind:value={search} />
      <p class={muted}>{t("catalog.count", shown.length, ofGame.length)}</p>
      <ThumbGrid items={shown} size={96} height={mineOpen ? 330 : undefined} reset={[game, chosenLevel, chosenCategory, search]}>
        {#snippet cell(p: Prefab)}
          <button
            class="block w-full"
            use:want={p}
            onclick={() => place(p, false)}
            onpointerdown={(e) => p.made && panelDrag(e, () => void place(p, true))}
          >
            {@render cellBody(p)}
          </button>
          <Tip
            text={[
              p.name,
              `${t(`prefabs.category.${p.category}`)} - ${p.game === "jak2" ? "Jak II, " : ""}${p.level}`,
              p.description,
              t(p.made ? "prefabs.cell_tip" : "prefabs.game.being_made"),
            ]
              .filter(Boolean)
              .join("\n")}
            placement="right"
          />
        {/snippet}
      </ThumbGrid>
    {/if}

    <!-- the user's own -->
    {@render header(mineOpen, "prefabs.mine", () => (mineOpen = !mineOpen), "prefab")}
    {#if mineOpen}
      <span class="block">
        <button class="{primary} w-full py-2" disabled={!s.selection.count} onclick={() => windows.open("new_prefab")}>
          {t("prefabs.create")}
        </button>
      </span>
      <Tip text={t("prefabs.create.tip")} />
      {#if !list.mine.length}<p class={muted}>{t("prefabs.empty")}</p>{/if}
      <ThumbGrid items={list.mine} size={96}>
        {#snippet cell(p: Prefab)}
          <button
            class="block w-full"
            onclick={() => place(p, false)}
            onpointerdown={(e) => panelDrag(e, () => void place(p, true))}
            oncontextmenu={(e) => (e.preventDefault(), (menu = { p, x: e.clientX, y: e.clientY }))}
          >
            {@render cellBody(p)}
          </button>
          <Tip text={`${p.name}\n${t("prefabs.cell_tip")}`} placement="right" />
        {/snippet}
      </ThumbGrid>
    {/if}
  {/if}
</div>

{#if menu}
  {@const p = menu.p}
  <div
    class="prefab-menu fixed z-[95] min-w-44 rounded-md border border-white/10 bg-[#101010] py-1 text-sm shadow-xl shadow-black/60"
    style:left="{menu.x}px"
    style:top="{menu.y}px"
  >
    <p class="px-3 py-1 text-xs text-neutral-500">{p.name}</p>
    <button class="block w-full px-3 py-1 text-left hover:bg-white/10" onclick={() => ((menu = null), place(p, false))}>{t("prefabs.place")}</button>
    <button class="block w-full px-3 py-1 text-left text-red-300 hover:bg-white/10" onclick={() => remove(p)}>{t("prefabs.delete")}</button>
  </div>
{/if}
