<script lang="ts">
  // Level > Play in the game (the toolbar's Play): the host's game runtime exports the level,
  // installs it in the game, builds it and starts the game on it (play_level, on the "play" job).
  // The steps, the progress, the tools' last lines, how it ended (play_state); the levels installed
  // in that game, to play again or take out. Why it cannot play; what the runtime says besides (the
  // launcher's game: a newer OpenGOAL to install from its header). A runtime that installs OpenGOAL
  // itself (the standalone app's) without any installed yet: its latest release installed, then
  // the level played, in one click.
  import { onMount } from "svelte";
  import IconCheck from "~icons/mdi/check";
  import IconClose from "~icons/mdi/close";
  import IconLoading from "~icons/mdi/loading";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { heading, muted, primary, small } from "../common/classes";
  import type { ToolingState } from "../settings/ToolingSettings.svelte";

  type PlayState = {
    busy: boolean;
    action: string;
    name: string;
    steps: { key: string; args: string[] }[];
    done: number;
    total: number;
    log: string[];
    outcome: "started" | "handed_over" | null;
    error: string;
    seconds: number;
    generation: number;
  };

  let s = $state<PlayState>();
  let box = $state<HTMLDivElement>();
  const runtime = $derived(
    engine.state.runtime as {
      label: string;
      available: boolean;
      reason: string;
      installed: string[];
      notice: string;
      notice_args: string[];
      tooling?: { active: string; latest: string; update: boolean };
    } | null,
  );
  // no OpenGOAL installed yet: the one to install (tooling_state)
  const needsTooling = $derived(!!runtime?.tooling && !runtime.tooling.active);
  let tooling = $state<ToolingState>();
  let playAfter = $state(false);

  const refresh = async () =>
    (s = (await call<PlayState>("play_state", {}, true)) ?? s);

  onMount(() => {
    refresh();
    // the steps come as jle:progress, the tools' lines are read while it works
    const timer = setInterval(() => s?.busy && refresh(), 400);
    const off = onEngine("jle:progress", (j) => j?.id === "play" && refresh());
    // OpenGOAL installed: the level played
    const offTooling = onEngine("jle:progress", async (j) => {
      if (j?.id !== "tooling" || !j.finished) return;
      tooling = await call<ToolingState>("tooling_state", {}, true);
      if (playAfter && tooling?.active) run("play_level");
      playAfter = false;
    });
    return () => {
      clearInterval(timer);
      off();
      offTooling();
    };
  });

  $effect(() => {
    if (needsTooling && !tooling)
      call<ToolingState>("tooling_state", {}, true).then((r) => (tooling = r));
  });
  const latest = $derived(tooling?.releases[0]);

  async function installTooling() {
    if (!latest) return;
    playAfter = true;
    const r = await call<ToolingState>("tooling_install", {
      version: latest.tag,
    });
    if (r) tooling = r;
    else playAfter = false;
  }

  $effect(() => {
    s?.log.length;
    if (box) box.scrollTop = box.scrollHeight;
  });

  async function run(name: string, args: Record<string, unknown> = {}) {
    const r = await call<PlayState>(name, args);
    if (r) s = r;
  }
</script>

<div class="flex flex-col gap-1.5 text-sm">
  {#if runtime}
    {#if runtime.label}<p class="{muted} break-all">
        {t("play.runtime", runtime.label)}
      </p>{/if}
    {#if needsTooling}
      {#if latest}
        <p class="text-neutral-300">
          {t("play.no_tooling", latest.size / 1048576)}
        </p>
        <button
          class="{primary} self-start"
          disabled={!!engine.jobs.tooling || !engine.state.document.open}
          onclick={installTooling}
          >{t("play.install_tooling", latest.tag)}</button
        >
        {#if engine.jobs.tooling}
          {@const j = engine.jobs.tooling}
          <p class={muted}>{j.label}</p>
          <div class="h-1.5 overflow-hidden rounded bg-white/10">
            <div
              class="h-full bg-green-500 transition-all"
              style="width: {(100 * j.done) / j.total}%"
            ></div>
          </div>
        {/if}
      {:else if tooling}
        <p class="text-amber-300">{t("play.no_release")}</p>
      {/if}
    {:else if !runtime.available && runtime.reason}
      <p class="text-amber-300">{t(runtime.reason)}</p>
    {/if}
    {#if runtime.notice}
      <p class="text-sky-300">{t(runtime.notice, ...runtime.notice_args)}</p>
    {/if}
  {/if}
  {#if s && s.action}
    <ul class="flex flex-col gap-1">
      {#each s.steps as st, i (i)}
        {@const last = i === s.steps.length - 1}
        <li class="flex items-center gap-2">
          {#if last && s.busy}
            <IconLoading class="shrink-0 animate-spin text-orange-400" />
          {:else if last && s.error}
            <IconClose class="shrink-0 text-red-400" />
          {:else}
            <IconCheck class="shrink-0 text-green-400" />
          {/if}
          <span class={last && s.busy ? "text-neutral-100" : "text-neutral-400"}
            >{t(st.key, ...st.args)}</span
          >
        </li>
      {/each}
    </ul>
    {#if s.busy}
      <div class="h-1.5 overflow-hidden rounded bg-white/10">
        {#if s.total > 0}
          <div
            class="h-full bg-orange-500 transition-all"
            style="width: {(100 * s.done) / s.total}%"
          ></div>
        {:else}
          <div class="h-full w-1/3 animate-pulse bg-orange-500/60"></div>
        {/if}
      </div>
    {:else if s.error}
      <p class="break-words whitespace-pre-wrap text-red-400 select-text">
        {t("play.failed", s.error)}
      </p>
    {:else if s.outcome === "handed_over"}
      <p class="text-green-400">{t("play.handed_over")}</p>
    {:else if s.outcome}
      <p class="text-green-400">
        {t(s.action === "remove" ? "play.removed" : "play.started", s.name)}
      </p>
    {/if}
    {#if !s.busy && s.seconds}<p class={muted}>
        {t("play.seconds", s.seconds)}
      </p>{/if}
    {#if s.log.length}
      <div
        bind:this={box}
        class="max-h-48 min-h-16 overflow-auto rounded border border-white/5 bg-[#0b0b0b] px-2 py-1 font-mono text-[11px] whitespace-pre text-neutral-400 select-text"
      >
        {#each s.log as line}<div>{line}</div>{/each}
      </div>
    {/if}
  {/if}

  {#if !needsTooling}
    <button
      class="{primary} mt-1 self-start"
      disabled={!!s?.busy || !engine.state.document.open || !runtime?.available}
      onclick={() => run("play_level")}
    >
      {s?.action ? t("play.again") : t("play.button")}
    </button>
  {/if}

  {#if runtime?.installed?.length}
    <h3 class={heading}>{t("play.levels")}</h3>
    {#each runtime.installed as name (name)}
      <div class="flex items-center gap-2">
        <span class="flex-1 font-mono text-xs text-neutral-200">{name}</span>
        <button
          class={small}
          disabled={!!s?.busy}
          onclick={() => run("runtime_launch", { name })}
          >{t("play.launch")}</button
        >
        <button
          class={small}
          disabled={!!s?.busy}
          onclick={() => run("runtime_remove", { name })}
          >{t("play.remove")}</button
        >
      </div>
    {/each}
    <p class={muted}>{t("play.levels.tip")}</p>
  {/if}
</div>
