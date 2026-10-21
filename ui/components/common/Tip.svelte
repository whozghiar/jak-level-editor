<script lang="ts">
  // A tooltip on the element just before it (wrap a disabled button in a <span>: it gets no mouse
  // events). `guide`: a tip of the guide (guide_tip in guide_panel.cpp): the keys
  // guide.<id>.title / .keys / .text / .example; else `text`.
  import { Tooltip } from "flowbite-svelte";
  import { has, t } from "../../lib/i18n";

  type Placement = "top" | "bottom" | "left" | "right" | "right-start" | "bottom-start";
  let { guide, text, placement = "bottom" }: { guide?: string; text?: string; placement?: Placement } = $props();
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
    {:else}
      <p class="whitespace-pre-line">{text}</p>
    {/if}
  </Tooltip>
{/if}
