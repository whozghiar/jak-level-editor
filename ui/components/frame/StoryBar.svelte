<script lang="ts">
  // The story bar, at the bottom of the viewport (ui_story_bar, panels.cpp): the point of the game the
  // level is shown at. Shown by LevelEditor when engine.state.story.available. Its states: from the
  // start, and from each step that changes the level shown (the steps where nothing changes for it
  // are skipped); each level of the document has its own.
  import IconLeft from "~icons/mdi/chevron-left";
  import IconRight from "~icons/mdi/chevron-right";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { check, range } from "../common/classes";
  import Dropdown from "../common/Dropdown.svelte";
  import Tip from "../common/Tip.svelte";

  type Story = {
    enabled: boolean;
    main: string;
    level: string;
    levels: string[];
    states: { step: number; node: string }[];
    state: number;
  };
  const st = $derived(engine.state.story as Story);
  const count = $derived(st.states?.length ?? 1);
  const state = $derived(st.state ?? 1);
  const shown = $derived(st.states?.[state - 1]);
  const setState = (n: number) => call("set_story", { state: Math.min(Math.max(n, 1), count) });
  const text = $derived(
    !st.enabled ? t("story.all") : count <= 1 ? t("story.no_change") : !shown || shown.step <= 1 ? t("story.start") : t("story.after", shown.node),
  );

  const arrow =
    "flex size-7 shrink-0 items-center justify-center rounded border border-[#2a2a2a] bg-[#0b0b0b] text-lg hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-40";
</script>

<div class="flex h-10 items-center gap-2 rounded-lg border border-white/5 bg-[#101010]/90 px-3 text-sm text-neutral-200 shadow-lg shadow-black/40">
  <label class="flex shrink-0 cursor-pointer items-center gap-2">
    <input type="checkbox" class={check} checked={st.enabled} onchange={(e) => call("set_story", { enabled: e.currentTarget.checked })} />
    {t("story.enabled")}
  </label>
  <Tip text={t("story.enabled.tip")} placement="top" />
  <!-- the level shown: its own point of the story, when the document has several -->
  {#if st.levels?.length}
    <div class="w-36 shrink-0">
      <Dropdown
        items={[{ label: st.main, value: "" }, ...st.levels.map((l) => ({ label: l, value: l }))]}
        value={st.level}
        label={st.level || st.main}
        onselect={(level) => call("set_story", { level })}
        tip={t("story.level.tip")}
      />
    </div>
  {/if}
  <span class="inline-flex">
    <button class={arrow} disabled={!st.enabled || state <= 1} aria-label={t("story.prev.tip")} onclick={() => setState(state - 1)}><IconLeft /></button>
  </span>
  <Tip text={t("story.prev.tip")} placement="top" />
  <span class="flex min-w-20 flex-1 items-center gap-2">
    <input
      type="range"
      class={range}
      min="1"
      max={Math.max(count, 1)}
      step="1"
      value={state}
      disabled={!st.enabled || count <= 1}
      oninput={(e) => +e.currentTarget.value !== state && setState(+e.currentTarget.value)}
    />
    <span class="shrink-0 text-xs text-neutral-400 tabular-nums">{state} / {count}</span>
  </span>
  <Tip text={t(count <= 1 ? "story.slider.none" : "story.slider.tip")} placement="top" />
  <span class="inline-flex">
    <button class={arrow} disabled={!st.enabled || state >= count} aria-label={t("story.next.tip")} onclick={() => setState(state + 1)}><IconRight /></button>
  </span>
  <Tip text={t("story.next.tip")} placement="top" />
  <!-- a width of its own: the arrows stay where they are as the text changes -->
  <span class="w-[min(420px,42%)] shrink-0 truncate text-neutral-300">{text}</span>
  {#if st.enabled && shown && shown.step > 1}<Tip text={shown.node} placement="top" />{/if}
</div>
