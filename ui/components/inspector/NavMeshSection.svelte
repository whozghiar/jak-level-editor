<script lang="ts">
  // The inspector's section of a nav mesh (ui_navmesh_inspector, panels.cpp): its polygons, its
  // walkable area, whether it suits the game, its area generated again with the parameters of
  // Level > Nav meshes. Mounted by the inspector for a node of kind "navmesh": <NavMeshSection {id} />
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import Tip from "../common/Tip.svelte";

  let { id }: { id: number } = $props();

  type NavData = {
    verts: unknown[];
    polys: unknown[];
    area: number;
    problem: string | null;
  };
  let m = $state<NavData | null>(null);

  const read = async (node: number) =>
    (m =
      (await call<NavData[]>("navmesh_data", { ids: [node] }, true))?.[0] ??
      null);

  $effect(() => void read(id));
  onMount(() => onEngine("jle:changed", (p) => (!p || p.scene) && read(id)));

  const busy = $derived(!!engine.jobs["navmesh"]);
</script>

{#if m}
  <section class="flex flex-col gap-1 text-sm">
    <h3
      class="mb-0.5 border-b border-white/10 pb-1 text-xs font-semibold tracking-wide text-neutral-400 uppercase"
    >
      {t("navmesh.inspector")}
    </h3>
    <p>{t("navmesh.polys", m.polys.length, m.verts.length)}</p>
    <p>{t("navmesh.area", m.area)}</p>
    {#if m.problem}
      <p class="text-orange-400">{t("navmesh.problem", m.problem)}</p>
    {:else}
      <p class="text-green-400">{t("navmesh.ok")}</p>
    {/if}
    <button
      class="self-start rounded border border-[#2a2a2a] bg-[#0b0b0b] px-3 py-1 text-sm hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-50"
      disabled={busy}
      onclick={() => call("generate_navmesh", { node: id })}
    >
      {t("navmesh.regenerate")}
    </button>
    <Tip text={t("navmesh.regenerate.tip")} placement="left" />
  </section>
{/if}
