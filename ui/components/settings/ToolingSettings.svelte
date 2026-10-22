<script lang="ts" module>
  export type ToolingRelease = {
    tag: string;
    date: string;
    size: number;
    asset: string;
    installed: boolean;
  };
  export type ToolingState = {
    dir: string;
    active: string;
    status: { active: string; latest: string; update: boolean };
    releases: ToolingRelease[];
    checked: boolean;
    checking: boolean;
    error: string;
    installing: string;
  } | null;
</script>

<script lang="ts">
  // Settings > OpenGOAL: the releases of whozghiar/jak-project the runtime installs itself (the
  // standalone app's, tooling_state; null for the launcher's, whose header and Versions page do
  // it), newest first; Install (the job "tooling"), Use, Remove; GitHub asked again.
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { heading, muted, small } from "../common/classes";

  // focus: shown at once (the title bar's update icon opens the settings there); onchange: a version
  // installed, used or removed (the game's settings above show it)
  let { focus = false, onchange }: { focus?: boolean; onchange?: () => void } =
    $props();
  let s = $state<ToolingState>();
  let head = $state<HTMLElement>();
  $effect(() => {
    // once the settings above it are there too
    const h = focus && head;
    if (h) setTimeout(() => h.scrollIntoView({ block: "start" }), 400);
  });
  const refresh = async (check = false) =>
    (s = await call<ToolingState>("tooling_state", { check }, true));

  onMount(() => {
    refresh();
    // GitHub's answer, the installation's end
    const timer = setInterval(
      () => (s?.checking || s?.installing) && refresh(),
      1000,
    );
    const off = onEngine("jle:progress", (p) => {
      if (p?.id !== "tooling") return;
      refresh();
      if (p.finished) onchange?.();
    });
    return () => {
      clearInterval(timer);
      off();
    };
  });

  async function act(name: string, version: string) {
    const r = await call<ToolingState>(name, { version });
    if (!r) return;
    s = r;
    onchange?.();
  }

  const job = $derived(engine.jobs.tooling);
  const badge = "rounded px-1.5 text-[10px] font-semibold uppercase";
</script>

{#if s}
  <h3 bind:this={head} class={heading}>{t("tooling.title")}</h3>
  <p class="mb-2 {muted} break-all">{t("tooling.help", s.dir)}</p>
  <div class="flex flex-wrap items-center gap-2">
    <button class={small} disabled={s.checking} onclick={() => refresh(true)}
      >{s.checking ? t("tooling.checking") : t("tooling.check")}</button
    >
    {#if s.error}<span class="text-xs text-amber-300"
        >{t("tooling.error", s.error)}</span
      >{/if}
  </div>
  {#if !s.releases.length}
    <p class="mt-1.5 {muted}">{t("tooling.no_release")}</p>
  {/if}
  <ul class="mt-1.5 flex flex-col gap-1">
    {#each s.releases as r, i (r.tag)}
      {@const installing = s.installing === r.tag}
      <li
        class="flex flex-wrap items-center gap-2 rounded border border-white/5 bg-[#0b0b0b] px-2 py-1"
      >
        <span class="font-mono text-xs text-neutral-200 select-text"
          >{r.tag}</span
        >
        {#if i === 0 && r.date}<span
            class="{badge} bg-green-800/40 text-green-300"
            >{t("tooling.latest")}</span
          >{/if}
        {#if r.tag === s.active}<span
            class="{badge} bg-orange-600/30 text-orange-300"
            >{t("tooling.active")}</span
          >{:else if r.installed}<span
            class="{badge} bg-white/10 text-neutral-300"
            >{t("tooling.installed")}</span
          >{/if}
        <span class="ml-auto {muted}"
          >{r.date.slice(0, 10)}{r.size
            ? ` · ${t("tooling.size", r.size / 1048576)}`
            : ""}</span
        >
        {#if installing}
          <span class="h-1.5 w-24 overflow-hidden rounded bg-white/10">
            <span
              class="block h-full bg-green-500"
              style:width="{job ? (100 * job.done) / job.total : 0}%"
            ></span>
          </span>
        {:else if !r.installed}
          <button
            class={small}
            disabled={!!s.installing}
            onclick={() => act("tooling_install", r.tag)}
            >{t("tooling.install")}</button
          >
        {:else}
          {#if r.tag !== s.active}<button
              class={small}
              onclick={() => act("tooling_use", r.tag)}
              >{t("tooling.use")}</button
            >{/if}
          <button
            class={small}
            disabled={!!engine.jobs.play}
            onclick={() => act("tooling_remove", r.tag)}
            >{t("tooling.remove")}</button
          >
        {/if}
      </li>
    {/each}
  </ul>
  {#if job}<p class="mt-1 {muted}">{job.label}</p>{/if}
{/if}
