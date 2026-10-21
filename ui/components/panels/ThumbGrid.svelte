<script lang="ts" generics="T">
  // A grid of pictures with their names (thumbnail_grid in panels.cpp): the rows out of the view are
  // not made (the catalog has thousands of cells). It takes the room left, else `height`; back to
  // its top when `reset` changes (another list).
  import type { Snippet } from "svelte";

  let {
    items,
    size = 80,
    cell,
    height,
    reset,
  }: { items: T[]; size?: number; cell: Snippet<[T, number]>; height?: number; reset?: unknown } = $props();

  const gap = 6;
  let width = $state(0);
  let shownHeight = $state(0);
  let top = $state(0);
  let box: HTMLDivElement;
  const perRow = $derived(Math.max(1, Math.floor((width + gap) / (size + 6 + gap))));
  // a cell: its picture (size + its frame) and its name
  const rowH = $derived(size + 6 + 18 + gap);
  const rows = $derived(Math.ceil(items.length / perRow));
  const first = $derived(Math.max(0, Math.floor(top / rowH) - 1));
  const last = $derived(Math.min(rows, Math.ceil((top + shownHeight) / rowH) + 1));
  const shown = $derived(items.slice(first * perRow, last * perRow));

  $effect(() => {
    void reset;
    if (box) box.scrollTop = top = 0;
  });
</script>

<div
  bind:this={box}
  class="min-h-0 overflow-y-auto {height ? '' : 'flex-1'}"
  style:height={height ? `${height}px` : undefined}
  bind:clientWidth={width}
  bind:clientHeight={shownHeight}
  onscroll={(e) => (top = e.currentTarget.scrollTop)}
>
  <div class="relative" style:height="{rows * rowH}px">
    <div
      class="absolute inset-x-0 grid"
      style:top="{first * rowH}px"
      style:grid-template-columns="repeat({perRow}, {size + 6}px)"
      style:grid-auto-rows="{rowH - gap}px"
      style:gap="{gap}px"
    >
      {#each shown as item, i (first * perRow + i)}
        <div class="min-w-0">{@render cell(item, first * perRow + i)}</div>
      {/each}
    </div>
  </div>
</div>
