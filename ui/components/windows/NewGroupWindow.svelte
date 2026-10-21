<script lang="ts">
  // A name for a group of the selection (ui_new_group_window, groups.cpp): "Group <n + 1>" at first.
  // Opened by Edit > Group the selection (Ctrl+Shift+G), the Groups panel and the inspector.
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { viewport } from "../../render/viewport_api";
  import { button, input, primary } from "../common/classes";

  let name = $state("");
  let field: HTMLInputElement;
  const roots = viewport.store.selectionRoots().length;

  onMount(async () => {
    const groups = (await call<unknown[]>("list_groups", {}, true)) ?? [];
    name = t("group.default_name", groups.length + 1);
    field.focus();
    field.select();
  });

  function create() {
    if (!name || !engine.state.selection.count) return;
    call("group_selection", { name });
    windows.close("new_group");
  }
</script>

<p class="mb-3">{t("group.new_text", roots)}</p>
<input bind:this={field} class={input} bind:value={name} onkeydown={(e) => e.key === "Enter" && create()} />
<div class="mt-4 flex justify-end gap-2">
  <button class={primary} disabled={!name || !engine.state.selection.count} onclick={create}>{t("group.create")}</button>
  <button class={button} onclick={() => windows.close("new_group")}>{t("popup.cancel")}</button>
</div>
