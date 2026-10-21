<script lang="ts">
  // The project's name: its folder in <library>/projects (the save popup of ui_windows, panels.cpp).
  // props: { name } (save's ask_name). The path it would take and whether a project is there already
  // come from the engine (project_save_path), the folder's rules being its own.
  import { onMount } from "svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { button, input, muted, primary } from "../common/classes";

  let { name: initial = "" }: { name?: string } = $props();
  // svelte-ignore state_referenced_locally (the name proposed when it opens)
  let name = $state(initial);
  let field: HTMLInputElement;
  let target = $state({ folder: "", path: "", exists: false });

  onMount(() => {
    field.focus();
    field.select();
  });

  $effect(() => {
    const asked = name;
    call<typeof target>("project_save_path", { name: asked }, true).then(
      (r) => {
        if (r && asked === name) target = r;
      },
    );
  });

  async function saveAs() {
    if (!target.folder) return;
    windows.close("save_as");
    // a failure is logged by the engine (log.save_failed) and shown
    await call("save_project_as", { name });
  }
</script>

<p class="mb-3 text-neutral-300">{t("save.text")}</p>
<label class="flex items-center gap-2">
  <span class="shrink-0 text-neutral-400">{t("save.name")}</span>
  <input
    bind:this={field}
    class={input}
    bind:value={name}
    onkeydown={(e) => e.key === "Enter" && saveAs()}
  />
</label>
{#if target.folder}
  <p class="mt-2 break-all {muted}">{target.path}</p>
  {#if target.exists}<p class="mt-1 text-xs text-amber-300">
      {t("save.exists")}
    </p>{/if}
{/if}
<div class="mt-4 flex justify-end gap-2">
  <button class={primary} disabled={!target.folder} onclick={saveAs}
    >{t("save.button")}</button
  >
  <button class={button} onclick={() => windows.close("save_as")}
    >{t("popup.cancel")}</button
  >
</div>
