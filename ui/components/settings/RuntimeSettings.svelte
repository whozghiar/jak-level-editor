<script lang="ts">
  // Settings > Game: where Play installs the levels and starts the game (the host's game runtime,
  // runtime_settings). The standalone app's: its own OpenGOAL (the release installed below,
  // ToolingSettings), else a jak-project checkout chosen here (its goalc and gk: its build's, or a
  // folder chosen); which one is used, why one cannot be.
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { heading, muted, small } from "../common/classes";
  import ToolingSettings from "./ToolingSettings.svelte";

  // tooling: the OpenGOAL section shown at once
  let { tooling = false }: { tooling?: boolean } = $props();

  type Candidate = {
    kind: string;
    folder: string;
    available: boolean;
    reason: string;
  };
  type Settings = {
    info: {
      kind: string;
      label: string;
      available: boolean;
      reason: string;
      installed: string[];
      notice: string;
      notice_args: string[];
    };
    settings: {
      folder: string;
      bin: string;
      checkout: Candidate | null;
      own: Candidate;
      used: string | null;
    } | null;
  } | null;

  let r = $state<Settings>();
  const refresh = async () =>
    (r = await call<Settings>("runtime_settings", {}, true));
  onMount(refresh);

  async function choose(
    key: "runtime_folder" | "runtime_bin",
    current: string,
  ) {
    const path = await open({
      directory: true,
      defaultPath: current || undefined,
    });
    if (typeof path === "string") await set(key, path);
  }
  async function set(key: "runtime_folder" | "runtime_bin", value: string) {
    await call("set_prefs", { [key]: value });
    refresh();
  }

  const status = (c: Candidate) =>
    c.available ? t("settings.runtime.ready") : t(c.reason);
  const color = (c: Candidate) =>
    c.available ? "text-green-400" : "text-amber-300";
  const path =
    "min-w-0 flex-1 font-mono text-xs break-all text-neutral-200 select-text";
</script>

<h3 class={heading}>{t("settings.runtime")}</h3>
<p class="mb-2 {muted}">{t("settings.runtime.help")}</p>
{#if r === null}
  <p class="text-xs text-neutral-300">{t("settings.runtime.none")}</p>
{:else if r && !r.settings}
  <!-- the host's own (the OpenGOAL Launcher's game) -->
  <p class={path}>{r.info.label}</p>
  <p class="text-xs {r.info.available ? 'text-green-400' : 'text-amber-300'}">
    {r.info.available ? t("settings.runtime.ready") : t(r.info.reason)}
  </p>
  {#if r.info.notice}<p class="text-xs text-sky-300">
      {t(r.info.notice, ...r.info.notice_args)}
    </p>{/if}
{:else if r?.settings}
  {@const s = r.settings}
  <div class="flex flex-col gap-1.5">
    <p class="text-xs font-semibold text-neutral-300">
      {t("settings.runtime.own")}
      {#if s.used === "opengoal"}<span class="ml-1 text-orange-400"
          >({t("settings.runtime.used")})</span
        >{/if}
    </p>
    {#if s.own.folder}<span class={path}>{s.own.folder}</span>{/if}
    <p class="text-xs {color(s.own)}">{status(s.own)}</p>
    {#if s.used === "jak-project"}<p class={muted}>
        {t("settings.runtime.own.not_used")}
      </p>{/if}

    <p class="mt-2 text-xs font-semibold text-neutral-300">
      {t("settings.runtime.checkout")}
      {#if s.used === "jak-project"}<span class="ml-1 text-orange-400"
          >({t("settings.runtime.used")})</span
        >{/if}
    </p>
    <div class="flex items-center gap-2">
      <span class={path}>{s.folder || t("settings.runtime.checkout.none")}</span
      >
      <button class={small} onclick={() => choose("runtime_folder", s.folder)}
        >{t("settings.runtime.choose")}</button
      >
      {#if s.folder}<button
          class={small}
          onclick={() => set("runtime_folder", "")}
          >{t("settings.runtime.clear")}</button
        >{/if}
    </div>
    {#if s.checkout}
      <p class="text-xs {color(s.checkout)}">{status(s.checkout)}</p>
      <div class="flex items-center gap-2">
        <span class="{muted} shrink-0">{t("settings.runtime.bin")}</span>
        <span class={path}>{s.bin || t("settings.runtime.bin.default")}</span>
        <button
          class={small}
          onclick={() => choose("runtime_bin", s.bin || s.folder)}
          >{t("settings.runtime.choose")}</button
        >
        {#if s.bin}<button class={small} onclick={() => set("runtime_bin", "")}
            >{t("settings.runtime.clear")}</button
          >{/if}
      </div>
    {/if}
    {#if r.info.installed.length}
      <p class="mt-1 {muted}">
        {t("settings.runtime.installed", r.info.installed.join(", "))}
      </p>
    {/if}
  </div>
{/if}
<ToolingSettings focus={tooling} onchange={refresh} />
