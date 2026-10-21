<script lang="ts">
  // Unsaved changes before loading or quitting (confirm_discard, ui_windows in panels.cpp).
  import { save, ui } from "../../lib/actions.svelte";
  import { t } from "../../lib/i18n";
  import { windows } from "../../lib/windows.svelte";

  function close(): (() => void) | null {
    const action = ui.pendingDiscard;
    ui.pendingDiscard = null;
    windows.close("discard");
    return action;
  }

  async function saveFirst() {
    const action = close();
    // saved at once when the project has a file; else the Save window opens and the action waits
    // for the user to ask again
    const r = await save(false);
    if (r?.saved) action?.();
  }

  const button = "rounded border px-4 py-1.5 text-sm font-semibold";
</script>

<p class="mb-4">{t("discard.text")}</p>
<div class="flex justify-end gap-2">
  <button class="{button} border-orange-600 bg-orange-600 text-white hover:bg-orange-500" onclick={saveFirst}>
    {t("discard.save")}
  </button>
  <button class="{button} border-[#2a2a2a] bg-[#0b0b0b] hover:border-[#3a3a3a] hover:bg-[#1a1a1a]" onclick={() => close()?.()}>
    {t("discard.discard")}
  </button>
  <button class="{button} border-[#2a2a2a] bg-[#0b0b0b] hover:border-[#3a3a3a] hover:bg-[#1a1a1a]" onclick={close}>
    {t("popup.cancel")}
  </button>
</div>
