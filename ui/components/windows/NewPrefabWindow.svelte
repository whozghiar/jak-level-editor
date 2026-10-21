<script lang="ts" module>
  // the window's choices, kept (App::prefab_skip_small, prefab_min_size)
  let skipSmall = $state(true);
  let minSize = $state(0.5);
</script>

<script lang="ts">
  // A prefab made of the selection (the prefab popup of ui_windows, panels.cpp): its name, the small
  // pieces left out (bolts, debris, stains: a prefab is a structure). Then the Prefabs tab shows it.
  import { onMount } from "svelte";
  import { ui } from "../../lib/actions.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { button, check, input, muted, primary } from "../common/classes";
  import Tip from "../common/Tip.svelte";

  let name = $state("");
  let field: HTMLInputElement;
  let info = $state({ name: "", roots: 0, kept: 0, folder: true });

  onMount(async () => {
    const r = await call<typeof info>("prefab_dialog", { min_size: minSize }, true);
    if (r) {
      info = r;
      name = r.name;
    }
    field.focus();
    field.select();
  });
  // what the min size leaves out
  $effect(() => {
    const min = skipSmall ? minSize : 0;
    call<typeof info>("prefab_dialog", { min_size: min }, true).then((r) => r && (info = r));
  });

  const ready = $derived(!!name && info.folder && info.roots > 0);
  async function create() {
    if (!ready) return;
    windows.close("new_prefab");
    await call("create_prefab", { name, min_size: skipSmall ? minSize : 0 });
    ui.leftTab = "prefabs";
  }
</script>

<p class="mb-2">{t("prefab_popup.text", info.roots)}</p>
{#if !info.folder}<p class="mb-2 text-red-400">{t("log.prefab_no_library")}</p>{/if}
<label class="flex items-center gap-2">
  <span class="shrink-0 text-neutral-400">{t("prefab_popup.name")}</span>
  <input bind:this={field} class={input} bind:value={name} onkeydown={(e) => e.key === "Enter" && create()} />
</label>
<div class="mt-3 flex items-center gap-2">
  <label class="flex cursor-pointer items-center gap-2">
    <input type="checkbox" class={check} bind:checked={skipSmall} />
    {t("prefab_popup.skip_small")}
  </label>
  <Tip text={t("prefab_popup.skip_small.tip")} />
  {#if skipSmall}
    <input type="number" class="{input.replace('w-full', '')} w-20 shrink-0" min="0.05" max="10" step="0.05" bind:value={minSize} />
    <span class="text-neutral-400">m</span>
  {/if}
</div>
{#if skipSmall && info.kept < info.roots}
  <p class="mt-1 {muted}">{t("prefab_popup.left_out", info.roots - info.kept)}</p>
{/if}
<div class="mt-4 flex justify-end gap-2">
  <button class={primary} disabled={!ready} onclick={create}>{t("prefab_popup.create")}</button>
  <button class={button} onclick={() => windows.close("new_prefab")}>{t("popup.cancel")}</button>
</div>
