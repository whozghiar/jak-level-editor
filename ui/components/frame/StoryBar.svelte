<script lang="ts">
  // The story bar, at the bottom of the viewport (ui_story_bar, panels.cpp): the point of the game the
  // level is shown at. Shown by LevelEditor when engine.state.story.available. To do.
  import { t } from "../../lib/i18n";
</script>

<div class="flex h-9 items-center gap-3 rounded-lg border border-white/5 bg-[#101010]/90 px-3 text-xs text-neutral-400">
  {t("story.enabled")}
</div>
