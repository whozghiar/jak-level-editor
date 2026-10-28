<script lang="ts">
  // The Build panel's "Where creatures walk": the nav meshes of the whole level in one click, for a
  // creature of a size chosen among three (the Nav meshes window keeps every setting), shown over
  // the view, cleared; and how a moving object is given its route.
  import { onMount } from "svelte";
  import { toggleView } from "../../lib/actions.svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { button, check, muted } from "../common/classes";
  import Tip from "../common/Tip.svelte";

  type NavState = {
    params: Record<string, number>;
    busy: boolean;
    nodes: number[];
    polys: number;
    area: number;
  };
  // the creature's size: its radius, height, highest step, steepest slope
  const sizes = {
    small: {
      agent_radius: 0.3,
      agent_height: 1,
      max_climb: 0.5,
      max_slope: 45,
    },
    medium: {
      agent_radius: 0.6,
      agent_height: 2,
      max_climb: 0.8,
      max_slope: 45,
    },
    large: { agent_radius: 1.5, agent_height: 4, max_climb: 1, max_slope: 35 },
  } as const;
  type Size = keyof typeof sizes;

  let st = $state<NavState>();
  const refresh = async () =>
    (st = (await call<NavState>("navmesh_state", {}, true)) ?? st);
  onMount(() => {
    void refresh();
    const offs = [
      onEngine("jle:changed", refresh),
      onEngine("jle:progress", (j) => j?.id === "navmesh" && refresh()),
    ];
    return () => offs.forEach((off) => off());
  });

  // the size the settings are closest to
  const size = $derived.by((): Size | "" => {
    const r = st?.params.agent_radius;
    if (r === undefined) return "";
    return (Object.keys(sizes) as Size[]).reduce((a, b) =>
      Math.abs(sizes[a].agent_radius - r) < Math.abs(sizes[b].agent_radius - r)
        ? a
        : b,
    );
  });
  const busy = $derived(!!st?.busy || !!engine.jobs["navmesh"]);

  async function make() {
    if (!engine.state.view.navmesh) toggleView("navmesh");
    st =
      (await call<NavState>("generate_navmesh", {
        region: "all",
        keep_selected: false,
      })) ?? st;
  }
  async function clear() {
    if (!st?.nodes.length) return;
    await call("select", { ids: st.nodes, mode: "replace" });
    await call("delete_selection");
  }
</script>

<p class="mt-2 font-semibold text-neutral-300">{t("walk.title")}</p>
<Tip guide="walk" />
<p class={muted}>
  {st?.nodes.length
    ? t("walk.made", st.nodes.length, Math.round(st.area))
    : t("walk.none")}
</p>
<div class="mt-1 flex flex-wrap items-center gap-1">
  <span class="w-14 text-neutral-400">{t("walk.size")}</span>
  {#each Object.keys(sizes) as k (k)}
    <button
      class="rounded border px-1.5 py-0.5 text-xs {size === k
        ? 'border-orange-500 bg-orange-500/15'
        : 'border-[#2a2a2a] bg-[#0b0b0b] hover:border-[#3a3a3a]'}"
      onclick={() =>
        call("set_navmesh", { params: sizes[k as Size] }).then(refresh)}
      >{t(`walk.size.${k}`)}</button
    >
  {/each}
</div>
<Tip text={t("walk.size.tip")} />
<div class="mt-1.5 flex flex-wrap gap-1.5">
  <button class={button} disabled={busy} onclick={make}
    >{t(busy ? "walk.making" : "walk.make")}</button
  >
  <Tip text={t("walk.make.tip")} />
  {#if st?.nodes.length}
    <button class={button} disabled={busy} onclick={clear}
      >{t("walk.clear")}</button
    >
  {/if}
  <button class={button} onclick={() => windows.open("navmesh")}
    >{t("walk.settings")}</button
  >
</div>
<label class="mt-1 flex cursor-pointer items-center gap-2 self-start">
  <input
    type="checkbox"
    class={check}
    checked={!!engine.state.view.navmesh}
    onchange={() => toggleView("navmesh")}
  />
  {t("walk.show")}
</label>
<p class="mt-2 font-semibold text-neutral-300">{t("route.title")}</p>
<Tip guide="route" />
<p class={muted}>{t("route.how")}</p>
