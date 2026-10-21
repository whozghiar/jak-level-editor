<script lang="ts" module>
  // its choices, shared by every browser (the C++ App's m_tex_game, m_tex_level, m_tex_filter): ""
  // Jak 3 / "jak2"; "" every level, "*" the document's own textures
  let texGame = $state("");
  let texLevel = $state("");
  let texFilter = $state("");

  // the pictures of the textures, read once (a few at a time: the palette reads a level's file
  // for all its textures)
  const images = new Map<string, Promise<string | null>>();
  let running = 0;
  const waiting: (() => void)[] = [];
  async function slot<T>(work: () => Promise<T>): Promise<T> {
    if (running >= 6) await new Promise<void>((r) => waiting.push(r));
    running++;
    try {
      return await work();
    } finally {
      running--;
      waiting.shift()?.();
    }
  }
  function image(path: string, key: string): Promise<string | null> {
    let p = images.get(key);
    if (!p) {
      p = slot(async () => {
        try {
          const t = parseTexture(await fetchBytes(path));
          const c = document.createElement("canvas");
          [c.width, c.height] = [t.width, t.height];
          const ctx = c.getContext("2d")!;
          const img = ctx.createImageData(t.width, t.height);
          img.data.set(t.rgba);
          ctx.putImageData(img, 0, 0);
          return c.toDataURL();
        } catch {
          images.delete(key); // asked again later
          return null;
        }
      });
      images.set(key, p);
    }
    return p;
  }
</script>

<script lang="ts">
  // The textures of the games (ui_texture_browser, surface_panel.cpp): the game (Jak 3, Jak II when
  // it was extracted too, or the caller's `game`), a level (the textures its decor draws), every
  // level or the document's own, a part of a name; a click gives the texture to `ids` (their
  // material `slot` in the first one's mesh, -1: all of them). The Textures window, the Build
  // panel's and the catalog's Materials show it.
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { fetchBytes } from "../../render/engine";
  import { parseTexture } from "../../render/protocol";
  import { check, input, muted } from "../common/classes";
  import Dropdown from "../common/Dropdown.svelte";
  import Tip from "../common/Tip.svelte";
  import ThumbGrid from "../panels/ThumbGrid.svelte";

  type Tex = { name: string; game?: string; background?: string; levels?: number; uses?: number; index?: number; width?: number; height?: number };

  let { ids, slot: material = -1, height, game }: { ids: number[]; slot?: number; height?: number; game?: string } = $props();

  const s = $derived(engine.state);
  // the game chosen here, or by the caller
  const chosenGame = $derived(game ?? (s.jak2_available ? texGame : ""));

  let answer = $state<any>(null);
  let asked = 0;
  // (a value: the effect runs again when it changes, not at each state the engine sends)
  const listKey = $derived(JSON.stringify([s.catalog?.epoch, texLevel === "*" ? s.history.undo_label : 0]));
  $effect(() => {
    const args = { game: chosenGame, level: texLevel, filter: texFilter.toLowerCase() };
    // the document's textures change with it; the index once read
    void listKey;
    const n = ++asked;
    call("texture_list", args, true).then((r) => {
      if (n === asked) answer = r ?? null;
    });
  });

  // the texture of the material chosen, marked
  let current = $state("");
  const historyKey = $derived(JSON.stringify([s.history.undo_label, s.history.can_undo]));
  $effect(() => {
    void historyKey;
    if (material < 0 || !ids.length) return void (current = "");
    call("materials", { ids }, true).then((m) => (current = m?.slots?.[material]?.texture?.name ?? ""));
  });

  const levelLabel = $derived(texLevel === "" ? t("textures.all_levels") : texLevel === "*" ? t("textures.document") : texLevel);
  const nameOf = (x: Tex) => (x.index !== undefined || !x.game ? x.name : `${x.game}-${x.name}`);
  const q = encodeURIComponent;
  const picture = (x: Tex) =>
    x.index !== undefined
      ? image(`texture/${x.index}`, `doc:${x.index}:${x.name}:${x.width}x${x.height}`)
      : image(`palette?game=${q(x.game ?? "")}&background=${q(x.background ?? "")}&name=${q(x.name)}`, `${x.game}:${x.name}`);

  function tip(x: Tex): string {
    if (x.index !== undefined) return `${x.name}\n${x.width}x${x.height}`;
    return `${nameOf(x)}\n${texLevel ? t("textures.used_level", x.uses) : t("textures.used", x.levels, x.uses)}`;
  }

  // the texture into the document (read now if its image is not yet), then on the pieces
  async function give(x: Tex) {
    const texture = x.index ?? (await call<{ texture: number }>("palette_texture", { name: x.name, game: x.game, background: x.background }))?.texture;
    if (texture === undefined || texture < 0 || !ids.length) return;
    call("set_material", { ids, slot: material, texture });
  }

  const indexing = $derived(engine.jobs["library_index"]);
</script>

<div class="flex min-h-0 flex-1 flex-col gap-2" style:height={height ? `${height}px` : undefined}>
  {#if game === undefined && s.jak2_available}
    <div class="flex items-center gap-3">
      <span class="text-neutral-400">{t("textures.game")}</span>
      {#each ["", "jak2"] as g}
        <label class="flex cursor-pointer items-center gap-1.5">
          <input
            type="radio"
            class={check}
            checked={texGame === g}
            onchange={() => {
              texGame = g;
              texLevel = "";
            }}
          />
          {g ? "Jak II" : "Jak 3"}
        </label>
      {/each}
    </div>
  {/if}
  <div class="flex gap-2">
    <div class="w-1/2 min-w-0">
      <Dropdown
        items={[
          { label: t("textures.all_levels"), value: "" },
          { label: t("textures.document"), value: "*" },
          ...((answer?.levels ?? []) as { level: string; count: number }[]).map((l) => ({ label: `${l.level} (${l.count})`, value: l.level })),
        ]}
        value={texLevel}
        label={levelLabel}
        filter={t("textures.filter")}
        onselect={(v) => (texLevel = v)}
      />
      <Tip text={t("textures.level.tip")} />
    </div>
    <input class={input} placeholder={t("textures.filter")} bind:value={texFilter} />
  </div>
  {#if answer?.indexing}
    <p class={muted}>{t("catalog.indexing", indexing?.done ?? answer.indexing.done, indexing?.total ?? answer.indexing.total)}</p>
  {:else if answer?.items}
    <p class={muted}>{t("textures.count", answer.count, answer.total)}</p>
    {#if !answer.count}<p class={muted}>{t("textures.none")}</p>{/if}
    <ThumbGrid items={answer.items as Tex[]} size={60} reset={[chosenGame, texLevel, texFilter]}>
      {#snippet cell(x: Tex)}
        <button
          class="block w-full rounded border p-[2px] {current && current === nameOf(x)
            ? 'border-teal-400 bg-teal-500/30'
            : 'border-white/5 bg-[#0b0b0b] hover:border-white/25'}"
          onclick={() => give(x)}
        >
          {#await picture(x)}
            <div class="flex size-[60px] items-center justify-center text-neutral-500">...</div>
          {:then url}
            {#if url}
              <img src={url} alt="" draggable="false" class="size-[60px] object-cover" />
            {:else}
              <div class="flex size-[60px] items-center justify-center text-neutral-500">?</div>
            {/if}
          {/await}
        </button>
        <Tip text={tip(x)} placement="right" />
        <p class="truncate text-center text-[11px] leading-4 text-neutral-400">{nameOf(x)}</p>
      {/snippet}
    </ThumbGrid>
  {/if}
</div>
