<script lang="ts" module>
  /** Follows a drag from a pointerdown: `move` gets how far the pointer went since the last move. */
  export function dragPointer(e: PointerEvent, move: (dx: number, dy: number) => void): void {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    let x = e.clientX;
    let y = e.clientY;
    const onmove = (m: PointerEvent) => {
      move(m.clientX - x, m.clientY - y);
      x = m.clientX;
      y = m.clientY;
    };
    el.addEventListener("pointermove", onmove);
    el.addEventListener("lostpointercapture", () => el.removeEventListener("pointermove", onmove), { once: true });
  }
</script>

<script lang="ts">
  // A floating window (an ImGui window of the C++ editor): its title, its cross, dragged by its title
  // bar, resized by its corner, kept inside the editor (its parent: the windows' layer).
  import { onMount, type Snippet } from "svelte";
  import IconClose from "~icons/mdi/close";
  import { t } from "../../lib/i18n";

  let {
    title,
    width = 420,
    height,
    modal = false,
    anchor = "center",
    z = 0,
    onclose,
    onfocus,
    children,
  }: {
    title: string;
    width?: number;
    height?: number;
    modal?: boolean;
    anchor?: "center" | "top-right";
    z?: number;
    onclose?: () => void;
    onfocus?: () => void;
    children: Snippet;
  } = $props();

  let el: HTMLDivElement;
  let x = $state(0);
  let y = $state(0);
  // not seen before it is placed (still focusable: its fields take the keys when it opens)
  let placed = $state(false);

  function clamp() {
    const p = el.offsetParent as HTMLElement | null;
    if (!p) return;
    x = Math.max(0, Math.min(x, p.clientWidth - el.offsetWidth));
    y = Math.max(0, Math.min(y, p.clientHeight - el.offsetHeight));
  }

  onMount(() => {
    const p = el.offsetParent as HTMLElement;
    if (anchor === "top-right") {
      // over the viewport's top right, left of the inspector
      x = p.clientWidth - el.offsetWidth - 344;
      y = 20;
    } else {
      x = (p.clientWidth - el.offsetWidth) / 2;
      y = Math.max(0, (p.clientHeight - el.offsetHeight) / 3);
    }
    clamp();
    placed = true;
    const ro = new ResizeObserver(clamp);
    ro.observe(p);
    ro.observe(el);
    return () => ro.disconnect();
  });

  function ondragstart(e: PointerEvent) {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    dragPointer(e, (dx, dy) => {
      x += dx;
      y += dy;
      clamp();
    });
  }
</script>

<svelte:window onkeydown={(e) => modal && e.key === "Escape" && (e.preventDefault(), onclose?.())} />

{#if modal}
  <div class="pointer-events-auto absolute inset-0 bg-black/50" style:z-index={z}></div>
{/if}
<div
  bind:this={el}
  role="dialog"
  aria-label={title}
  tabindex="-1"
  class="pointer-events-auto absolute flex max-h-full max-w-full min-h-24 min-w-56 resize flex-col overflow-hidden rounded-lg border border-white/5 bg-[#141414] text-sm text-neutral-200 shadow-2xl shadow-black/70"
  style:left="{x}px"
  style:top="{y}px"
  style:width="{width}px"
  style:height={height ? `${height}px` : undefined}
  style:z-index={z}
  style:opacity={placed ? undefined : 0}
  onpointerdown={onfocus}
>
  <!-- svelte-ignore a11y_no_static_element_interactions (dragged with the mouse) -->
  <header
    class="flex h-8 shrink-0 cursor-move items-center gap-2 border-b border-white/5 bg-[#101010] pl-3 select-none"
    onpointerdown={ondragstart}
  >
    <span class="h-3.5 w-0.75 rounded-r bg-orange-500"></span>
    <p class="min-w-0 flex-1 truncate font-semibold">{title}</p>
    <button
      class="flex h-full w-10 items-center justify-center text-neutral-400 hover:bg-red-600 hover:text-white"
      aria-label={t("window.close")}
      onclick={onclose}
    >
      <IconClose />
    </button>
  </header>
  <div class="min-h-0 flex-1 overflow-auto p-3">
    {@render children()}
  </div>
</div>
