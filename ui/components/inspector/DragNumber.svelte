<script lang="ts">
  // A number of the inspector as ImGui's DragFloat: dragged left and right (Shift faster, Alt finer),
  // shown at once (`oninput`) and applied when let go (`onchange`: one undo step); a click types it
  // (Enter or leaving the field applies it, Esc gives up).
  import { tick } from "svelte";

  let {
    value,
    step = 0.1,
    min,
    max,
    digits = 2,
    unit = "",
    format,
    oninput,
    onchange,
    class: cls = "",
  }: {
    value: number;
    step?: number; // per pixel
    min?: number;
    max?: number;
    digits?: number;
    unit?: string;
    format?: (v: number) => string;
    oninput?: (v: number) => void;
    onchange?: (v: number) => void;
    class?: string;
  } = $props();

  let editing = $state(false);
  let dragged = $state<number | null>(null);
  let text = $state("");
  let input = $state<HTMLInputElement>();
  let start: { x: number; v: number; moved: boolean } | null = null;
  let justDragged = false;

  const clamp = (v: number) =>
    min !== undefined && v < min ? min : max !== undefined && v > max ? max : v;
  const round = (v: number) => +v.toFixed(digits);
  const shown = $derived.by(() => {
    const v = dragged ?? value ?? 0;
    return format ? format(v) : `${v.toFixed(digits)}${unit ? ` ${unit}` : ""}`;
  });

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    start = { x: e.clientX, v: value ?? 0, moved: false };
  }

  function move(e: PointerEvent) {
    if (!start) return;
    const dx = e.clientX - start.x;
    if (!start.moved && Math.abs(dx) < 3) return;
    start.moved = true;
    const k = e.shiftKey ? 10 : e.altKey ? 0.1 : 1;
    const v = clamp(round(start.v + dx * step * k));
    if (v === dragged) return;
    dragged = v;
    oninput?.(v);
  }

  function up() {
    if (!start) return;
    justDragged = start.moved;
    start = null;
    if (justDragged && dragged !== null) onchange?.(dragged);
    dragged = null;
  }

  async function edit() {
    if (justDragged) return void (justDragged = false);
    text = String(round(value ?? 0));
    editing = true;
    await tick();
    input?.focus();
    input?.select();
  }

  function apply() {
    if (!editing) return;
    editing = false;
    const v = parseFloat(text.replace(",", "."));
    if (Number.isFinite(v) && clamp(v) !== value) onchange?.(clamp(v));
  }
</script>

{#if editing}
  <input
    bind:this={input}
    bind:value={text}
    class="w-full min-w-0 rounded border border-orange-500 bg-[#0b0b0b] px-1 py-0.5 text-right text-sm text-neutral-100 outline-none {cls}"
    onkeydown={(e) => {
      if (e.key === "Enter") apply();
      else if (e.key === "Escape") editing = false;
      e.stopPropagation();
    }}
    onblur={apply}
  />
{:else}
  <button
    type="button"
    class="w-full min-w-0 cursor-ew-resize truncate rounded border border-[#2a2a2a] bg-[#0b0b0b] px-1 py-0.5 text-right text-sm text-neutral-200 tabular-nums hover:border-[#3a3a3a] {cls}"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onclick={edit}
  >
    {shown}
  </button>
{/if}
