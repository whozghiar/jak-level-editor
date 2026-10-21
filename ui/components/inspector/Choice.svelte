<script lang="ts">
  // A list of choices, each with what it does (in the list, and under it for the one chosen); the
  // options in groups (a title key, or null). The `choice` of ui_decor_behavior (behavior_panel.cpp).
  import { t } from "../../lib/i18n";
  import { field } from "./edit.svelte";

  let {
    prefix,
    value,
    groups,
    onchange,
  }: {
    prefix: string;
    value: string;
    groups: [string | null, string[]][];
    onchange: (v: string) => void;
  } = $props();

  let open = $state(false);
  let box = $state<HTMLElement>();
</script>

<!-- Esc closes the list (before the editor's key: select nothing) -->
<svelte:window
  onpointerdown={(e) =>
    open && !box?.contains(e.target as Node) && (open = false)}
  onkeydowncapture={(e) =>
    open && e.key === "Escape" && (e.preventDefault(), (open = false))}
/>

<div bind:this={box}>
  <button
    type="button"
    class="{field} flex w-full items-center justify-between text-left"
    onclick={() => (open = !open)}
  >
    <span class="truncate">{t(prefix + value)}</span><span
      class="text-neutral-500">▾</span
    >
  </button>
  {#if open}
    <div
      class="mt-1 max-h-96 overflow-y-auto rounded border border-white/10 bg-[#0b0b0b] p-1"
    >
      {#each groups as [group, options]}
        {#if group}<p
            class="mt-1.5 mb-0.5 border-b border-white/10 px-1 text-xs font-semibold text-neutral-400"
          >
            {t(group)}
          </p>{/if}
        {#each options as o}
          <!-- its explanation picks it too -->
          <button
            type="button"
            class="block w-full rounded px-1.5 py-1 text-left hover:bg-white/5 {o ===
            value
              ? 'bg-orange-500/15'
              : ''}"
            onclick={() => {
              open = false;
              if (o !== value) onchange(o);
            }}
          >
            <span
              class="text-sm {o === value
                ? 'text-orange-300'
                : 'text-neutral-200'}">{t(prefix + o)}</span
            >
            <span class="block pl-3 text-xs text-neutral-500"
              >{t(`${prefix}${o}.tip`)}</span
            >
          </button>
        {/each}
      {/each}
    </div>
  {/if}
  <p class="mt-0.5 text-xs text-neutral-500">{t(`${prefix}${value}.tip`)}</p>
</div>
