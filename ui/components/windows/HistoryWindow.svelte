<script lang="ts">
  // Edit > History (arrange.rs): every step done, the oldest first; a click goes back (or forward)
  // to just after it. The steps undone stay, greyed, until something else is done.
  import { onMount } from "svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { muted } from "../common/classes";

  let steps = $state<string[]>([]);
  let at = $state(0);
  async function read() {
    const r = await call<{ steps: string[]; at: number }>(
      "history_list",
      {},
      true,
    );
    if (r) [steps, at] = [r.steps, r.at];
  }
  onMount(() => {
    read();
    return onEngine<any>("jle:changed", (p) => (!p || p.state) && read());
  });
  const go = async (n: number) => {
    await call("history_goto", { at: n });
    read();
  };
</script>

<div class="flex max-h-[60vh] flex-col overflow-y-auto text-sm">
  <p class="mb-2 {muted}">{t("history.text")}</p>
  <button
    class="rounded px-2 py-1 text-left hover:bg-[#1a1a1a] {at === 0
      ? 'bg-orange-500/15 text-orange-200'
      : ''}"
    onclick={() => go(0)}>{t("history.start")}</button
  >
  {#each steps as label, i (i)}
    <button
      class="rounded px-2 py-1 text-left hover:bg-[#1a1a1a] {i + 1 === at
        ? 'bg-orange-500/15 text-orange-200'
        : i + 1 > at
          ? 'text-neutral-500'
          : ''}"
      onclick={() => go(i + 1)}>{i + 1}. {label}</button
    >
  {/each}
  {#if !steps.length}
    <p class={muted}>{t("history.empty")}</p>
  {/if}
</div>
