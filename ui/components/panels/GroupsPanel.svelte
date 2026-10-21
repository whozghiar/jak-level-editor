<script lang="ts" module>
  let filter = $state(""); // C++ m_groups_filter
</script>

<script lang="ts">
  // The Groups tab (ui_groups_panel, groups.cpp): the prefabs placed [P] and the groups made [G].
  // Click: select it; double click: frame it; right click: rename it, frame it, ungroup it.
  import { onMount, tick } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { aabb, aabbAddBox } from "../../render/math";
  import { NodeFlag } from "../../render/types";
  import { viewport } from "../../render/viewport_api";
  import { button, input, muted } from "../common/classes";
  import Tip from "../common/Tip.svelte";

  type Group = { id: number; name: string; prefab: boolean; roots: number[] };

  const s = $derived(engine.state);
  let groups = $state<Group[]>([]);
  let queued = false;
  function schedule() {
    if (queued) return;
    queued = true;
    requestAnimationFrame(async () => {
      queued = false;
      groups = (await call<Group[]>("list_groups", {}, true)) ?? [];
    });
  }
  // the selected flags of the scene: a group all selected is shown chosen
  let rev = $state(0);
  onMount(() => {
    schedule();
    const offs = [onEngine<any>("jle:changed", (p) => (!p || p.scene || p.document) && schedule()), viewport.store.subscribe(() => rev++)];
    return () => offs.forEach((off) => off());
  });
  const whole = (g: Group) => {
    void rev;
    return g.roots.length > 0 && g.roots.every((r) => (viewport.store.nodes.get(r)?.flags ?? 0) & NodeFlag.Selected);
  };
  const shown = $derived(groups.filter((g) => !filter || g.name.toLowerCase().includes(filter.toLowerCase())));

  // framed from its elements' boxes (the selection reaches the view a moment later)
  async function frame(g: Group) {
    await call("select_group", { id: g.id });
    const b = aabb();
    for (const r of g.roots) aabbAddBox(b, viewport.store.subtreeBounds(r));
    viewport.frame(b);
  }

  let renaming = $state(0);
  let name = $state("");
  let field = $state<HTMLInputElement>();
  async function rename(g: Group) {
    menu = null;
    renaming = g.id;
    name = g.name;
    await tick();
    field?.select();
  }
  function renamed() {
    if (renaming && name) call("rename_group", { id: renaming, name });
    renaming = 0;
  }

  let menu = $state<{ g: Group; x: number; y: number } | null>(null);
  let menuEl = $state<HTMLDivElement>();
  const item = "block w-full px-3 py-1 text-left whitespace-nowrap hover:bg-white/10";
</script>

<svelte:window onpointerdown={(e) => menu && !menuEl?.contains(e.target as Node) && (menu = null)} />

<div class="flex h-full flex-col gap-2 p-3 text-sm">
  {#if !s.document.open}
    <p class="text-neutral-300">{t("groups.no_document")}</p>
  {:else}
    <p class={muted}>{t("groups.help")}</p>
    <span class="flex">
      <button class="{button} flex-1" disabled={s.selection.count < 1} onclick={() => windows.open("new_group")}>{t("group.make")}</button>
    </span>
    <Tip text={t("group.make.tip")} />
    <input class={input} placeholder={t("groups.filter")} bind:value={filter} />
    {#if !groups.length}
      <p class="mt-1 text-neutral-300">{t("groups.none")}</p>
    {:else}
      <p class={muted}>{t("groups.count", groups.length)}</p>
      <div class="min-h-0 flex-1 overflow-y-auto">
        {#each shown as g (g.id)}
          {#if renaming === g.id}
            <input
              bind:this={field}
              class={input}
              bind:value={name}
              onkeydown={(e) => {
                if (e.key === "Enter") renamed();
                else if (e.key === "Escape") renaming = 0;
              }}
              onblur={() => (renaming = 0)}
            />
          {:else}
            <button
              class="block w-full truncate rounded px-2 py-0.5 text-left {whole(g) ? 'bg-orange-500/20 text-white' : 'text-neutral-200 hover:bg-white/5'}"
              onclick={() => call("select_group", { id: g.id })}
              ondblclick={() => frame(g)}
              oncontextmenu={(e) => {
                e.preventDefault();
                menu = { g, x: e.clientX, y: e.clientY };
              }}
            >
              <span class="text-neutral-500">{g.prefab ? "[P]" : "[G]"}</span>
              {g.name}
              <span class="text-neutral-500">({g.roots.length})</span>
            </button>
            <Tip text={t(g.prefab ? "groups.prefab.tip" : "groups.group.tip")} placement="right" />
          {/if}
        {/each}
      </div>
    {/if}
  {/if}
</div>

{#if menu}
  {@const g = menu.g}
  <div
    bind:this={menuEl}
    role="menu"
    tabindex="-1"
    class="fixed z-[90] rounded-md border border-white/10 bg-[#101010] py-1 text-sm shadow-xl shadow-black/60"
    style:left="{menu.x}px"
    style:top="{menu.y}px"
    onkeydown={(e) => e.key === "Escape" && (menu = null)}
  >
    <button role="menuitem" class={item} onclick={() => rename(g)}>{t("groups.rename")}</button>
    <button role="menuitem" class={item} onclick={() => ((menu = null), frame(g))}>{t("groups.frame")}</button>
    <button role="menuitem" class={item} onclick={() => ((menu = null), call("ungroup", { id: g.id }))}>{t("group.ungroup")}</button>
  </div>
{/if}
