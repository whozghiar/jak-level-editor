<script lang="ts">
  // The inspector's Structure (editor/structures): a wall, a floor or a house of a level's own
  // pieces, its size and look. A size changed makes it again with more or fewer of its pieces (a
  // wall twice as long, twice as many bays and pillars), never stretched: one undo step.
  import { t } from "../../lib/i18n";
  import Tip from "../common/Tip.svelte";
  import DragNumber from "./DragNumber.svelte";
  import { button, check, heading, muted, query, send } from "./edit.svelte";

  let { id }: { id: number } = $props();

  type Structure = {
    kind: "wall" | "floor" | "house";
    level: string;
    size: [number, number, number];
    floors: number;
    roof: string;
    wear: number;
    seed: number;
    windows: boolean;
    group: number;
  };
  const q = query<Structure | null>("structure_info", () =>
    id ? { id } : null,
  );
  const st = $derived(q.value);
  let busy = $state(false);

  // made again with what changed
  async function set(args: Record<string, unknown>) {
    busy = true;
    await send("set_structure", { id, ...args });
    busy = false;
  }
  function setSize(i: number, v: number) {
    if (!st || !Number.isFinite(v)) return;
    const size = [...st.size];
    size[i] = v;
    set({ size });
  }
  // [label, axis] of each kind's sizes
  const sizes: Record<string, [string, number][]> = {
    wall: [
      ["structure.length", 0],
      ["structure.height", 1],
      ["structure.thickness", 2],
    ],
    floor: [
      ["structure.width", 0],
      ["structure.depth", 2],
      ["structure.thickness", 1],
    ],
    house: [
      ["structure.width", 0],
      ["structure.depth", 2],
    ],
  };
  const wears: [string, number][] = [
    ["build.house.wear.none", 0],
    ["build.house.wear.some", 0.5],
    ["build.house.wear.lots", 1],
  ];
</script>

{#if st}
  <h3 class={heading}>{t(`structure.kind.${st.kind}`)}</h3>
  <p class={muted}>{t("structure.text", st.level)}</p>
  <Tip guide="structures" />
  <div class="mt-1 flex flex-col gap-1">
    {#each sizes[st.kind] as [label, i] (label)}
      <div class="flex items-center gap-2">
        <span class="w-24 text-neutral-400">{t(label)}</span>
        <DragNumber
          class="flex-1"
          value={st.size[i]}
          step={0.1}
          min={i === 2 && st.kind === "wall" ? 0.1 : 1}
          max={400}
          digits={1}
          unit="m"
          disabled={busy}
          onchange={(v) => setSize(i, v)}
        />
      </div>
    {/each}
    {#if st.kind === "house"}
      <div class="flex items-center gap-2">
        <span class="w-24 text-neutral-400">{t("build.house.floors")}</span>
        {#each [1, 2, 3, 4, 5, 6] as f (f)}
          <button
            class="rounded border px-1.5 py-0.5 text-xs {st.floors === f
              ? 'border-orange-500 bg-orange-500/15'
              : 'border-[#2a2a2a] bg-[#0b0b0b]'}"
            disabled={busy}
            onclick={() => set({ floors: f })}>{f}</button
          >
        {/each}
      </div>
    {/if}
    <div class="flex items-center gap-2">
      <span class="w-24 text-neutral-400">{t("build.house.wear")}</span>
      {#each wears as [label, w] (w)}
        <button
          class="rounded border px-1.5 py-0.5 text-xs {st.wear === w
            ? 'border-orange-500 bg-orange-500/15'
            : 'border-[#2a2a2a] bg-[#0b0b0b]'}"
          disabled={busy}
          onclick={() => set({ wear: w })}>{t(label)}</button
        >
      {/each}
    </div>
    {#if st.kind === "wall"}
      <label class="flex cursor-pointer items-center gap-2">
        <input
          type="checkbox"
          class={check}
          checked={st.windows}
          disabled={busy}
          onchange={(e) => set({ windows: e.currentTarget.checked })}
        />
        {t("structure.windows")}
      </label>
    {/if}
    <span class="inline-flex"
      ><button
        class={button}
        disabled={busy}
        onclick={() => set({ seed: st.seed + 1 })}
        >{t("structure.another")}</button
      ></span
    >
    <Tip text={t("structure.another.tip")} />
    <p class={muted}>{t("structure.grow.tip")}</p>
  </div>
{/if}
