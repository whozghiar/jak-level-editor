<script lang="ts">
  // Edit > Repeat the selection (arrange.rs): copies in a row, in a circle or scattered around,
  // made again in the view as the numbers change; Keep makes them one step to undo, closing the
  // window takes them away.
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { button, input, muted, primary } from "../common/classes";
  import Tip from "../common/Tip.svelte";

  let mode = $state("row");
  let count = $state(4);
  let axis = $state("x");
  let spacing = $state(0);
  let radius = $state(10);
  let vary = $state(0.2);
  let seed = $state(1);
  let copies = $state(0);
  const sel = $derived(engine.state.selection.count > 0);

  // the copies made again for these numbers (one call at a time, the last numbers win)
  let busy = false;
  let again = false;
  async function show() {
    if (busy) return void (again = true);
    busy = true;
    const r = await call<{ copies: number }>("repeat_preview", {
      mode,
      count,
      axis,
      spacing,
      radius,
      vary,
      seed,
    });
    copies = r?.copies ?? 0;
    busy = false;
    if (again) {
      again = false;
      show();
    }
  }
  $effect(() => {
    void [mode, count, axis, spacing, radius, vary, seed];
    show();
  });
  onMount(() => () => void call("repeat_finish", { keep: false }));

  async function keep() {
    await call("repeat_finish", { keep: true });
    windows.close("repeat");
  }
  const modes = ["row", "circle", "scatter"] as const;
</script>

{#snippet number(
  label: string,
  value: number,
  step: number,
  set: (v: number) => void,
)}
  <label class="flex items-center justify-between gap-2">
    <span class="text-neutral-300">{label}</span>
    <input
      type="number"
      class="{input} w-24"
      {step}
      min="0"
      {value}
      onchange={(e) => {
        const v = Number(e.currentTarget.value);
        if (Number.isFinite(v)) set(v);
      }}
    />
  </label>
{/snippet}

<div class="flex flex-col gap-2 text-sm">
  {#if !sel && !copies}
    <p class={muted}>{t("repeat.none")}</p>
  {/if}
  <div class="flex gap-1">
    {#each modes as m (m)}
      <button
        class="flex-1 rounded border px-2 py-1 {mode === m
          ? 'border-orange-500 bg-orange-500/15'
          : 'border-[#2a2a2a] bg-[#0b0b0b] hover:border-[#3a3a3a]'}"
        onclick={() => (mode = m)}>{t(`repeat.mode.${m}`)}</button
      >
    {/each}
  </div>
  <Tip text={t(`repeat.mode.${mode}.tip`)} />
  {@render number(
    t("repeat.count"),
    count,
    1,
    (v) => (count = Math.max(1, Math.round(v))),
  )}
  {#if mode === "row"}
    <div class="flex items-center justify-between gap-2">
      <span class="text-neutral-300">{t("repeat.axis")}</span>
      <div class="flex gap-1">
        {#each ["x", "z"] as a (a)}
          <button
            class="rounded border px-2 py-0.5 {axis === a
              ? 'border-orange-500 bg-orange-500/15'
              : 'border-[#2a2a2a] bg-[#0b0b0b]'}"
            onclick={() => (axis = a)}>{t(`repeat.axis.${a}`)}</button
          >
        {/each}
      </div>
    </div>
    {@render number(t("repeat.spacing"), spacing, 0.5, (v) => (spacing = v))}
  {:else}
    {@render number(
      t("repeat.radius"),
      radius,
      1,
      (v) => (radius = Math.max(0.5, v)),
    )}
  {/if}
  {#if mode === "scatter"}
    {@render number(
      t("repeat.vary"),
      vary,
      0.05,
      (v) => (vary = Math.min(0.5, v)),
    )}
    <button class={button} onclick={() => seed++}>{t("repeat.again")}</button>
  {/if}
  <p class={muted}>{t("repeat.text", copies)}</p>
  <div class="mt-2 flex justify-end gap-2">
    <button class={primary} disabled={!copies} onclick={keep}
      >{t("repeat.keep")}</button
    >
    <button class={button} onclick={() => windows.close("repeat")}
      >{t("popup.cancel")}</button
    >
  </div>
</div>
