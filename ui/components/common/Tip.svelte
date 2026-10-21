<script lang="ts">
  // A tooltip on the element just before it (wrap a disabled button in a <span>: it gets no mouse
  // events). `guide`: a tip of the guide (guide_tip in guide_panel.cpp): the keys
  // guide.<id>.title / .keys / .text / .example, and its animation; else `text`, with `effect` (a
  // particle group) playing under it.
  import { Tooltip } from "flowbite-svelte";
  import { has, t } from "../../lib/i18n";
  import { guideDemos } from "../guide/guide";
  import ToolDemo from "../guide/ToolDemo.svelte";
  import { playing } from "../../render/particle_thumb";

  type Placement = "top" | "bottom" | "left" | "right" | "right-start" | "bottom-start";
  let {
    guide,
    text,
    effect,
    placement = "bottom",
  }: { guide?: string; text?: string; effect?: string; placement?: Placement } = $props();
  const k = $derived(`guide.${guide}`);
</script>

{#if guide || text}
  <Tooltip
    type="custom"
    {placement}
    arrow={false}
    class="z-[100] max-w-sm rounded-md border border-white/10 bg-[#0b0b0b] px-3 py-2 text-xs font-normal text-neutral-200 shadow-xl shadow-black/60"
  >
    {#if guide}
      <p class="mb-1 font-semibold text-orange-400">
        {t(`${k}.title`)}
        {#if has(`${k}.keys`)}<span class="ml-1 font-normal text-neutral-500">{t(`${k}.keys`)}</span>{/if}
      </p>
      <p class="whitespace-pre-line">{t(`${k}.text`)}</p>
      {#if has(`${k}.example`)}<p class="mt-1 text-neutral-400 italic">{t(`${k}.example`)}</p>{/if}
      <!-- it starts over each time the tooltip opens -->
      {#if guideDemos[guide]}<ToolDemo name={guideDemos[guide]} width={352} height={198} class="mt-2 rounded" />{/if}
    {:else}
      <p class="whitespace-pre-line">{text}</p>
      <!-- its particles, live (it starts over each time the tooltip opens) -->
      {#if effect}<canvas width="160" height="160" class="mt-2 rounded" use:playing={effect}></canvas>{/if}
    {/if}
  </Tooltip>
{/if}
