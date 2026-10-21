<script lang="ts">
  // View > Log (show_log, ui_windows in panels.cpp): the engine's lines, followed while at the bottom.
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";

  let box: HTMLDivElement;
  let follow = true;

  $effect(() => {
    engine.log.length;
    if (follow) box.scrollTop = box.scrollHeight;
  });

  const color = { info: "text-neutral-300", warn: "text-amber-300", error: "text-red-400" };
</script>

<div class="flex h-full flex-col gap-2">
  <button
    class="self-start rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-0.5 text-xs hover:border-[#3a3a3a] hover:bg-[#1a1a1a]"
    onclick={() => navigator.clipboard.writeText(engine.log.map((l) => l.text).join("\n") + "\n")}
  >
    {t("log.copy")}
  </button>
  <div
    bind:this={box}
    class="min-h-0 flex-1 overflow-auto rounded border border-white/5 bg-[#0b0b0b] px-2 py-1 font-mono text-[11px] whitespace-pre select-text"
    onscroll={() => (follow = box.scrollTop + box.clientHeight >= box.scrollHeight - 20)}
  >
    {#each engine.log as l}
      <div class={color[l.level] ?? color.info}>{l.text}</div>
    {/each}
  </div>
</div>
