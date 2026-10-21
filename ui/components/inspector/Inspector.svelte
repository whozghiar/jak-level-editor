<script lang="ts">
  // The inspector, on the right (ui_inspector, panels.cpp): the document's levels, the selection, the
  // tool going on. ui.scrollToReplace (lib/actions.svelte.ts): Level > Replace asks for its section.
  // To do.
  import { t } from "../../lib/i18n";
</script>

<div class="p-3">
  <h2 class="text-sm font-semibold text-neutral-200">{t("inspector.title")}</h2>
  <p class="mt-1 text-xs text-neutral-500">{t("common.todo")}</p>
</div>
