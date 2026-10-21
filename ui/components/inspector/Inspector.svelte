<script lang="ts">
  // The inspector, on the right (ui_inspector, ui_node_inspector in panels.cpp, ui_group_section in
  // groups.cpp): the tool going on, the document's levels when nothing is selected, else the selected
  // element: its group, where it is, what it does in the game, its surfaces, its mesh. A field dragged
  // is shown at once and makes one undo step when let go (set_node {live}, then without it).
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import Tip from "../common/Tip.svelte";
  import DocumentLevels from "../panels/DocumentLevels.svelte";
  import BehaviorPanel from "./BehaviorPanel.svelte";
  import DecorBehavior from "./DecorBehavior.svelte";
  import DragNumber from "./DragNumber.svelte";
  import NavMeshSection from "./NavMeshSection.svelte";
  import PreviewSection from "./PreviewSection.svelte";
  import SurfacePanel from "./SurfacePanel.svelte";
  import TopologySection from "./TopologySection.svelte";
  import {
    button,
    check,
    commit,
    followChanges,
    heading,
    live,
    muted,
    query,
    small,
  } from "./edit.svelte";

  type Vec3 = [number, number, number];

  onMount(followChanges);

  const s = $derived(engine.state);
  const open = $derived(s.document.open);
  const count = $derived(s.selection.count);
  const primary = $derived(count ? s.selection.primary : 0);
  const nodeQ = query("node", () => (primary ? { id: primary } : null));
  const infoQ = query("inspector", () => (open ? { id: primary } : null));
  const n = $derived(primary ? nodeQ.value : null);
  const info = $derived(infoQ.value);

  // the scale field keeps the proportions (the C++ m_scale_uniform: kept from one element to the next)
  let uniform = $state(true);

  const kindKey: Record<string, string> = {
    render: "kind.render",
    collision: "kind.collision",
    navmesh: "kind.navmesh",
    actor: "kind.actor",
  };

  const undoOf = {
    position: "undo.move",
    rotation: "undo.rotate",
    scale: "undo.scale",
  };
  type Key = keyof typeof undoOf;
  // where it is: a value of the three dragged (shown at once) or let go, typed (one undo step)
  let dragStart: Vec3 | null = null;
  function setAxis(key: Key, i: number, v: number, drag: boolean) {
    if (!n) return;
    const before: Vec3 = dragStart ?? ([...n[key]] as Vec3);
    if (drag) dragStart = before;
    let out = [...n[key]] as Vec3;
    out[i] = v;
    if (key === "scale") {
      // the value changed gives the ratio of the three
      if (uniform && before[i] > 0)
        out = before.map((x) => x * (v / before[i])) as Vec3;
      out = out.map((x) => Math.max(x, 0.01)) as Vec3;
    }
    n[key] = out;
    if (drag) live("set_node", { id: n.id, [key]: out });
    else {
      dragStart = null;
      commit("set_node", { id: n.id, [key]: out, label: undoOf[key] });
    }
  }
  const axes: {
    key: Key;
    label: string;
    step: number;
    digits: number;
    unit: string;
    min?: number;
    max?: number;
  }[] = [
    {
      key: "position",
      label: "inspector.position",
      step: 0.05,
      digits: 2,
      unit: "",
    },
    {
      key: "rotation",
      label: "inspector.rotation",
      step: 0.5,
      digits: 0,
      unit: "deg",
    },
    {
      key: "scale",
      label: "inspector.scale",
      step: 0.01,
      digits: 2,
      unit: "",
      min: 0.01,
      max: 100,
    },
  ];

  const isRender = $derived(n?.kind === "render");
  const isActor = $derived(n?.kind === "actor");
  const actorScaled = $derived(
    isActor && (n?.scale as Vec3 | undefined)?.some((x) => x !== 1),
  );
</script>

<div class="flex flex-col p-3 text-sm text-neutral-200">
  <h2 class="text-sm font-semibold text-neutral-200">{t("inspector.title")}</h2>

  {#if s.tool === "place"}
    <h3 class={heading}>{t("inspector.placing")}</h3>
    <p class="break-words">{s.placement?.name ?? ""}</p>
    <p class="{muted} my-1">{t("inspector.placing.help")}</p>
    <div>
      <button class={button} onclick={() => call("stop_placement")}
        >{t("inspector.placing.stop")}</button
      >
    </div>
  {/if}
  {#if s.tool === "topology"}
    <TopologySection />
  {/if}

  {#if !n}
    <h3 class={heading}>{t("inspector.level")}</h3>
    {#if !open}
      <p class="text-neutral-400">{t("inspector.no_level")}</p>
    {:else}
      <p>{s.document.level || t("title.untitled")}</p>
      {#if info}<p class="text-neutral-400">
          {t("inspector.counts", info.objects, info.actors)}
        </p>{/if}
      <!-- its levels, the level of the game it replaces, the levels around it, those shown with it -->
      <DocumentLevels />

      <p class="{muted} mt-3">{t("inspector.hint")}</p>
    {/if}
  {:else}
    {#if count > 1}
      <p class="mt-2 text-amber-300">{t("inspector.selected_count", count)}</p>
    {/if}

    <!-- the group of the selection: all of it in one group, else a group to make of it -->
    {#if info?.group}
      {@const g = info.group}
      <h3 class={heading}>
        {t(g.prefab ? "group.prefab_title" : "group.title")}
      </h3>
      <p class="break-words">{t("group.name_count", g.name, g.roots)}</p>
      <div class="mt-1 flex gap-1.5">
        {#if !g.whole}
          <span class="inline-flex"
            ><button
              class={button}
              onclick={() => call("select_group", { id: g.id })}
              >{t("group.select_all")}</button
            ></span
          >
          <Tip text={t("group.select_all.tip")} />
        {/if}
        <span class="inline-flex"
          ><button class={button} onclick={() => call("ungroup", { id: g.id })}
            >{t("group.ungroup")}</button
          ></span
        >
        <Tip text={t("group.ungroup.tip")} />
      </div>
    {:else if info && info.roots >= 2}
      <h3 class={heading}>{t("group.title")}</h3>
      <div>
        <button
          class={button}
          onclick={() =>
            windows.open("new_group", {
              name: t("group.default_name", info.groups + 1),
            })}
        >
          {t("group.make")}
        </button>
      </div>
      <Tip text={t("group.make.tip")} />
    {/if}

    <h3 class={heading}>{t(kindKey[n.kind] ?? "kind.group")}</h3>
    <p class="break-words">{n.name}</p>
    {#if isRender && n.mesh}
      <div class="mt-1">
        <button
          class={small}
          onclick={() => call("select_assembly", { id: n.id })}
          >{t("inspector.select_assembly")}</button
        >
      </div>
      <Tip text={t("menu.edit.select_assembly.tip")} />
    {/if}
    {#if isActor}
      <p>{t("inspector.type", n.actor?.etype || n.extras?.etype || "?")}</p>
      {#each n.actor?.models ?? [] as model}<p class={muted}>
          {t("inspector.model", model)}
        </p>{/each}
    {/if}
    <PreviewSection id={n.id} node={n} />

    <!-- where it is -->
    <div class="mt-2 flex flex-col gap-1">
      {#each axes as a}
        <div class="flex items-center gap-1">
          {#each [0, 1, 2] as i}
            <DragNumber
              value={n[a.key][i]}
              step={a.step}
              min={a.min}
              max={a.max}
              digits={a.digits}
              unit={a.unit}
              oninput={(v) => setAxis(a.key, i, v, true)}
              onchange={(v) => setAxis(a.key, i, v, false)}
            />
          {/each}
          <span class="w-16 shrink-0 truncate text-xs text-neutral-400"
            >{t(a.label)}</span
          >
        </div>
      {/each}
    </div>
    <div class="mt-1 flex flex-wrap items-center gap-x-4 gap-y-1">
      <label class="flex items-center gap-1.5">
        <input type="checkbox" class={check} bind:checked={uniform} />
        {t("inspector.scale.uniform")}
      </label>
      <Tip text={t("inspector.scale.uniform.tip")} />
      {#if isRender}
        <!-- locked: clicks go through it to what is on it (the ground is, from the start) -->
        <label class="flex items-center gap-1.5">
          <input
            type="checkbox"
            class={check}
            checked={n.locked}
            onchange={(e) =>
              commit("set_node", { id: n.id, locked: e.currentTarget.checked })}
          />
          {t("inspector.locked")}
        </label>
        <Tip text={t("inspector.locked.tip")} />
      {/if}
    </div>
    {#if actorScaled}
      <p class="mt-1 text-xs text-amber-300">{t("inspector.scale.actor")}</p>
    {/if}

    {#if n.kind === "navmesh"}
      <NavMeshSection id={n.id} />
    {/if}
    {#if isActor}
      <BehaviorPanel id={n.id} />
    {/if}
    <SurfacePanel node={n} />
    {#if isRender && n.mesh && !n.actor}
      <DecorBehavior id={n.id} />
    {/if}

    <div class="mt-4 flex gap-1.5">
      <button class={button} onclick={() => call("copy_selection")}
        >{t("inspector.copy")}</button
      >
      <button class={button} onclick={() => call("duplicate_selection")}
        >{t("inspector.duplicate")}</button
      >
      <button class={button} onclick={() => call("delete_selection")}
        >{t("inspector.delete")}</button
      >
    </div>
    <button
      class="{button} mt-1.5 w-full"
      onclick={() => windows.open("new_prefab")}
      >{t("inspector.create_prefab")}</button
    >
    {#if isRender && n.mesh}
      <h3 class={heading}>{t("inspector.mesh")}</h3>
      <p class="break-all">{n.mesh.name}</p>
      <p class={muted}>{t("inspector.triangles", n.mesh.triangles)}</p>
      {#if info && info.copies > 1}<p class={muted}>
          {t("inspector.copies", info.copies)}
        </p>{/if}
    {/if}
  {/if}
</div>
