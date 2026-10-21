<script lang="ts">
  // The document's levels (the inspector's with nothing selected in ui_inspector, panels.cpp; the
  // Levels panel shows them too): the one opened, the ones added (shown or not, framed, selected
  // whole to move them, removed), the level of the game it replaces (ui_replace_section), the
  // levels around it in the game to add, the levels shown with it, the mission layers.
  // Level > Replace scrolls to its section (ui.scrollToReplace).
  import { onMount, tick } from "svelte";
  import { ui } from "../../lib/actions.svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine, toast } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import type { AABB, Vec3 } from "../../render/math";
  import { viewport } from "../../render/viewport_api";
  import { button, check, heading, muted, small } from "../common/classes";
  import Dropdown from "../common/Dropdown.svelte";
  import Tip from "../common/Tip.svelte";

  type DocLevel = { name: string; role: "main" | "added" | "companion" | "layer"; group: number; host: number; reference: boolean };
  type Continue = { name: string; position: Vec3; ground: number | null };
  type Replace = { source: string | null; levels: string[]; continues: Continue[] } | null;

  const s = $derived(engine.state);
  const levels = $derived<DocLevel[]>(s.levels?.document ?? []);
  const main = $derived(s.document.level);
  const added = $derived(levels.filter((l) => l.role === "added" && !l.reference));
  const companions = $derived(levels.filter((l) => l.role === "companion"));
  const layers = $derived(levels.filter((l) => l.role === "layer"));
  const neighbors = $derived<string[]>(s.levels?.neighbors ?? []);
  const busy = $derived(!!s.levels?.adding || !!s.levels?.opening);
  const adding = $derived<string>(s.levels?.adding ?? "");
  const current = $derived(s.document.replaces);
  const original = $derived(levels.find((l) => l.reference && l.name === current));

  // the groups' own visibility (the checkboxes), what is shown of them (the layers), the Replace
  // section's answer: read again when the document changes
  let nodes = $state<Record<number, { visible: boolean; shown: boolean }>>({});
  let replace = $state<Replace>(null);
  let queued = false;
  async function refresh() {
    queued = false;
    const ids = levels.filter((l) => l.group).map((l) => l.group);
    const [infos, r] = await Promise.all([Promise.all(ids.map((id) => call<any>("node", { id }, true))), call<Replace>("replace_info", {}, true)]);
    nodes = Object.fromEntries(infos.filter(Boolean).map((n) => [n.id, { visible: n.visible, shown: n.shown }]));
    replace = r ?? null;
  }
  function schedule() {
    if (!queued) (queued = true), requestAnimationFrame(refresh);
  }
  $effect(() => {
    void [JSON.stringify(levels), current];
    schedule();
  });
  onMount(() => onEngine<any>("jle:changed", (p) => (!p || p.scene || p.document || p.state) && schedule()));

  const setShown = (group: number, visible: boolean) => call("set_node", { id: group, visible });

  async function frame(group: number) {
    const b = await call<AABB | null>("level_bounds", { group });
    if (b) viewport.frame(b);
  }

  async function junction(l: DocLevel) {
    const j = await call<AABB | null>("document_junction", { a: 0, b: l.group });
    if (!j) return toast(t("log.no_junction", main, l.name), "warn");
    viewport.setCamera({ pitch: -0.6 });
    viewport.frame(j);
  }

  // Level > Replace: its section scrolled to the top (both copies of it, then the request is done)
  let section = $state<HTMLElement>();
  $effect(() => {
    if (!ui.scrollToReplace || !section) return;
    const el = section;
    tick().then(() => el.scrollIntoView({ block: "start" }));
    setTimeout(() => (ui.scrollToReplace = false));
  });

  const choices = $derived([
    { label: t("replace.none_chosen"), value: "" },
    // the level the document comes from first, then the game's levels the Levels panel lists
    ...(replace?.source ? [{ label: t("replace.source", replace.source), value: replace.source }] : []),
    ...(replace?.levels ?? []).map((l) => ({ label: l, value: l })),
  ]);
  const add = (name: string, reference = false) => call("add_level", { name, reference });
</script>

<!-- its levels -->
<h3 class={heading}>{t(levels.some((l) => l.role === "added") ? "inspector.added_levels" : "inspector.levels_title")}</h3>
<div class="flex flex-wrap items-center gap-1">
  <button class={small} onclick={() => frame(0)}>{main}</button>
  <Tip text={t("inspector.added_levels.main.tip")} />
  <button class={small} onclick={() => call("select_level", { group: 0 })}>{t("inspector.added_levels.select")}</button>
  <Tip text={t("inspector.added_levels.select.tip")} />
</div>
{#each added as l (l.group)}
  <div class="mt-1 flex flex-wrap items-center gap-1">
    <input type="checkbox" class={check} checked={nodes[l.group]?.visible ?? true} onchange={(e) => setShown(l.group, e.currentTarget.checked)} />
    <Tip text={t("inspector.added_levels.shown.tip")} />
    <button class={small} onclick={() => frame(l.group)}>{l.name}</button>
    <Tip text={t("inspector.added_levels.frame.tip")} />
    <button class={small} onclick={() => junction(l)}>{t("inspector.added_levels.junction")}</button>
    <Tip text={t("inspector.added_levels.junction.tip")} />
    <button class={small} onclick={() => call("select_level", { group: l.group })}>{t("inspector.added_levels.select")}</button>
    <Tip text={t("inspector.added_levels.select.tip")} />
    <button class={small} onclick={() => call("remove_level", { group: l.group })}>{t("inspector.added_levels.remove")}</button>
  </div>
{/each}

<!-- replacing a level of the game with the document's: which one, the original shown to compare
     with, the document's level placed and turned where it goes, where the game starts the player -->
{#if replace}
  <h3 class="{heading} scroll-mt-2" bind:this={section}>{t("replace.title")}</h3>
  <Dropdown
    items={choices}
    value={current}
    label={current || t("replace.none_chosen")}
    onselect={(level) => call("set_replaces", { level })}
    guide="replace"
  />
  {#if current}
    <div class="mt-2 flex flex-col gap-1.5">
      {#if !original}
        <span class="flex">
          <button class="{button} flex-1" disabled={busy} onclick={() => add(current, true)}>{t("replace.show")}</button>
        </span>
        <Tip text={t("replace.show.tip")} />
      {:else}
        <div class="flex items-center gap-2">
          <label class="flex cursor-pointer items-center gap-2 {busy ? 'pointer-events-none opacity-40' : ''}">
            <input type="checkbox" class={check} checked={nodes[original.group]?.visible ?? true} onchange={(e) => setShown(original.group, e.currentTarget.checked)} />
            {t("replace.shown")}
          </label>
          <Tip text={t("replace.shown.tip")} />
          <button class={small} disabled={busy} onclick={() => call("remove_level", { group: original.group })}>{t("replace.remove")}</button>
        </div>
      {/if}
      {#if adding && s.levels?.adding_reference}<p class={muted}>{t("inspector.neighbors.adding", adding)}</p>{/if}
      <!-- the document's own level, where the original is -->
      <span class="flex">
        <button class="{button} flex-1" disabled={!original} onclick={() => call("align_to_replaced")}>{t("replace.align")}</button>
      </span>
      <Tip text={t(original ? "replace.align.tip" : "replace.align.disabled", current)} />
      <div class="flex flex-wrap gap-1">
        <button class={small} onclick={() => call("turn_own_level", { degrees: 90 })}>{t("replace.turn_left")}</button>
        <Tip text={t("replace.turn.tip")} />
        <button class={small} onclick={() => call("turn_own_level", { degrees: -90 })}>{t("replace.turn_right")}</button>
        <Tip text={t("replace.turn.tip")} />
        <button class={small} onclick={() => call("select_level", { group: 0 })}>{t("inspector.added_levels.select")}</button>
        <Tip text={t("replace.select.tip")} />
      </div>
      <!-- where the game starts the player there: on the ground of the new level? -->
      {#if replace.continues.length}
        <p class={muted}>{t("replace.continues", current)}</p>
        <Tip text={t("replace.continues.tip")} />
        {#each replace.continues as c (c.name)}
          <div class="flex items-center gap-2">
            <button
              class={small}
              onclick={() => viewport.setCamera({ pos: [c.position[0], c.position[1] + 6, c.position[2] + 9], look_at: c.position })}
            >
              {t("replace.go")}
            </button>
            <span class="truncate text-xs {c.ground !== null ? 'text-green-400' : 'text-orange-400'}">{c.name}</span>
            <Tip text={c.ground !== null ? t("replace.continue.ok", c.position[1] - c.ground) : t("replace.continue.none")} />
          </div>
        {/each}
      {/if}
      <span class="flex">
        <button class="{button} flex-1" onclick={() => windows.open("export")}>{t("replace.export", current)}</button>
      </span>
      <Tip text={t("replace.export.tip")} />
    </div>
  {/if}
{/if}

<!-- the levels around it in the game, to add -->
{#if neighbors.length}
  <h3 class={heading}>{t("inspector.neighbors")}</h3>
  <div class="flex flex-wrap gap-1">
    {#each neighbors as name (name)}
      <span class="inline-flex"><button class={small} disabled={busy} onclick={() => add(name)}>{name}</button></span>
      <Tip text={t("inspector.neighbors.tip")} />
    {/each}
  </div>
  {#if adding}<p class="mt-1 {muted}">{t("inspector.neighbors.adding", adding)}</p>{/if}
{/if}

<!-- the levels shown with it, the layers of the missions -->
{#if companions.length}
  <h3 class={heading}>{t("inspector.companions")}</h3>
  {#each companions as c (c.group)}
    <label class="flex cursor-pointer items-center gap-2 self-start">
      <input type="checkbox" class={check} checked={nodes[c.group]?.visible ?? true} onchange={(e) => setShown(c.group, e.currentTarget.checked)} />
      {c.name}
    </label>
    <Tip text={t("inspector.companions.tip")} />
  {/each}
{/if}
{#if layers.length}
  <p class="mt-3 {muted}">{t("inspector.layers", layers.length, layers.filter((l) => nodes[l.group]?.shown).length)}</p>
  <Tip text={t("inspector.layers.tip")} />
{/if}
