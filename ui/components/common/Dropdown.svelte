<script lang="ts" generics="T">
  // An ImGui combo: what is chosen on a button, the list under it (over it near the bottom of the
  // editor), a filter field on top when `filter` (its placeholder), a tooltip per item (`tip`).
  // The combo's own tooltip: `tip` (a text) or `guide` (a tip of the guide), on its button only.
  import { tick } from "svelte";
  import IconDown from "~icons/mdi/chevron-down";
  import { input } from "./classes";
  import Tip from "./Tip.svelte";

  type Item = { label: string; value: T; tip?: string };
  let {
    items,
    value,
    label,
    onselect,
    filter,
    tip,
    guide,
    class: cls = "",
  }: {
    items: Item[];
    value: T;
    label: string;
    onselect: (v: T) => void;
    filter?: string;
    tip?: string;
    guide?: string;
    class?: string;
  } = $props();

  let open = $state(false);
  let text = $state("");
  let btn: HTMLButtonElement;
  let menu = $state<HTMLDivElement>();
  let field = $state<HTMLInputElement>();
  let at = $state({ left: 0, width: 0, top: 0, bottom: 0, up: false, max: 320 });

  function toggle() {
    if (open) return void (open = false);
    const r = btn.getBoundingClientRect();
    const below = innerHeight - r.bottom;
    const up = below < 240 && r.top > below;
    at = { left: r.left, width: r.width, top: r.bottom + 2, bottom: innerHeight - r.top + 2, up, max: Math.min(360, (up ? r.top : below) - 10) };
    text = "";
    open = true;
    tick().then(() => field?.focus());
  }

  function choose(v: T) {
    open = false;
    onselect(v);
  }

  const shown = $derived(text ? items.filter((i) => i.label.toLowerCase().includes(text.toLowerCase())) : items);
</script>

<svelte:window
  onpointerdown={(e) => open && !menu?.contains(e.target as Node) && !btn.contains(e.target as Node) && (open = false)}
  onblur={() => (open = false)}
  onkeydowncapture={(e) => open && e.key === "Escape" && (e.preventDefault(), (open = false))}
/>

<button
  bind:this={btn}
  class="flex w-full min-w-0 items-center gap-1 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-1 text-left text-sm text-neutral-200 hover:border-[#3a3a3a] disabled:pointer-events-none disabled:opacity-40 {open
    ? 'border-orange-600'
    : ''} {cls}"
  onclick={toggle}
>
  <span class="min-w-0 flex-1 truncate">{label}</span>
  <IconDown class="shrink-0 text-neutral-500" />
</button>
<!-- not over its open list (ImGui's: the combo is not hovered then) -->
{#if !open}<Tip text={tip} {guide} />{/if}

{#if open}
  <div
    bind:this={menu}
    role="listbox"
    tabindex="-1"
    class="fixed z-[90] flex flex-col overflow-hidden rounded-md border border-white/10 bg-[#101010] text-sm shadow-xl shadow-black/60"
    style:left="{at.left}px"
    style:min-width="{at.width}px"
    style:top={at.up ? undefined : `${at.top}px`}
    style:bottom={at.up ? `${at.bottom}px` : undefined}
    style:max-height="{at.max}px"
  >
    {#if filter}
      <input bind:this={field} class="{input} rounded-none border-0 border-b border-white/10" placeholder={filter} bind:value={text} />
    {/if}
    <div class="min-h-0 overflow-y-auto py-1">
      {#each shown as item}
        <button
          role="option"
          aria-selected={item.value === value}
          class="block w-full px-3 py-1 text-left whitespace-nowrap hover:bg-white/10 {item.value === value ? 'text-orange-300' : 'text-neutral-200'}"
          onclick={() => choose(item.value)}
        >
          {item.label}
        </button>
        {#if item.tip}<Tip text={item.tip} placement="right" />{/if}
      {/each}
    </div>
  </div>
{/if}
