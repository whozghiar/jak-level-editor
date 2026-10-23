<script lang="ts">
  // Jak II chosen in a panel before its disc is extracted: why nothing is listed, and its
  // extraction (into <library>/jak2/, the files Jak II's own editing will read too)
  import { t } from "../../lib/i18n";
  import { windows } from "../../lib/windows.svelte";
  import { primary } from "./classes";
</script>

<p class="text-neutral-300">{t("jak2.missing")}</p>
<button
  class="{primary} py-2"
  onclick={() => windows.open("extract", { game: "jak2" })}
  >{t("jak2.extract")}</button
>
