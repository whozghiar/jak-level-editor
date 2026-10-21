<script lang="ts">
  // The inspector's section of the topology tool (ui_topology_inspector, topology_tool.cpp): the
  // element edited, its points and faces, what is picked, points or faces, the edits of the faces.
  // Mounted by the inspector while the tool is "topology": <TopologySection />
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { button, check, heading, muted } from "../common/classes";
  import Tip from "../common/Tip.svelte";
  import DragNumber from "./DragNumber.svelte";

  const s = $derived(
    engine.state.topology as {
      node: number;
      name: string;
      faces: boolean;
      points: number;
      triangles: number;
      picked: number;
      editable_faces: number;
      extrude: number;
    } | null,
  );
  let extrude = $state(1);
  $effect(() => {
    if (s) extrude = s.extrude;
  });
  const none = $derived(!s?.editable_faces);
</script>

{#if s}
  <section class="flex flex-col gap-1.5 text-sm">
    <h3 class={heading}>{t("topology.title")}</h3>
    <Tip guide="topology" placement="left" />
    <p class="break-words">{s.name}</p>
    <p class={muted}>{t("topology.counts", s.points, s.triangles)}</p>
    <div class="flex gap-4">
      <label class="flex items-center gap-1.5">
        <input
          type="radio"
          class={check}
          checked={!s.faces}
          onchange={() => call("topology_select", { faces: false })}
        />
        {t("topology.points")}
      </label>
      <Tip text={t("topology.points.tip")} placement="left" />
      <label class="flex items-center gap-1.5">
        <input
          type="radio"
          class={check}
          checked={s.faces}
          onchange={() => call("topology_select", { faces: true })}
        />
        {t("topology.faces")}
      </label>
      <Tip text={t("topology.faces.tip")} placement="left" />
    </div>
    <p class={muted}>
      {t(
        "topology.picked",
        s.picked,
        t(s.faces ? "topology.faces_word" : "topology.points_word"),
      )}
    </p>
    <div class="flex gap-2">
      <button
        class={button}
        onclick={() => call("topology_select", { all: true })}
        >{t("topology.all")}</button
      >
      <button
        class={button}
        onclick={() => call("topology_select", { all: false })}
        >{t("topology.none")}</button
      >
    </div>
    <div class="flex items-center gap-2">
      <button
        class={button}
        disabled={none}
        onclick={() => call("topology_extrude", { distance: extrude })}
      >
        {t("topology.extrude")}
      </button>
      <Tip text={t("topology.extrude.tip")} placement="left" />
      <DragNumber
        class="w-24"
        value={extrude}
        step={0.05}
        min={-100}
        max={100}
        unit="m"
        oninput={(v) => (extrude = v)}
        onchange={(v) => {
          extrude = v;
          call("topology_select", { extrude: v });
        }}
      />
    </div>
    <div class="flex flex-wrap gap-2">
      <button
        class={button}
        disabled={none}
        onclick={() => call("topology_subdivide")}
        >{t("topology.subdivide")}</button
      >
      <Tip text={t("topology.subdivide.tip")} placement="left" />
      <button
        class={button}
        disabled={none}
        onclick={() => call("topology_flip")}>{t("topology.flip")}</button
      >
      <Tip text={t("topology.flip.tip")} placement="left" />
      <button
        class={button}
        disabled={none}
        onclick={() => call("topology_delete")}>{t("topology.delete")}</button
      >
      <Tip text={t("topology.delete.tip")} placement="left" />
    </div>
    <button class="{button} w-full" onclick={() => call("stop_topology")}
      >{t("topology.done")}</button
    >
    <p class={muted}>{t("topology.keys")}</p>
  </section>
{/if}
