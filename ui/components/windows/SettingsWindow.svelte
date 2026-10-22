<script lang="ts">
  // File > Settings: the library folder (another one is used as it is: nothing moved or deleted),
  // the language, where ogle-extract was found (or the host's download), whether an assistant (the
  // MCP server) may control the editor (its remote control: remote_state, set_remote_control).
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import IconFolder from "~icons/mdi/folder-outline";
  import { changeLanguage } from "../../lib/actions.svelte";
  import { language, languages, t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import {
    button,
    check,
    heading,
    input,
    muted,
    primary,
  } from "../common/classes";
  import type { LibrarySetup } from "./WelcomeWindow.svelte";

  type RemoteState = { on: boolean; port: number | null; session: boolean };

  let setup = $state<LibrarySetup>();
  let remote = $state<RemoteState>();
  // the folder left for an empty one: what to do with it
  let left = $state("");
  let fetching = $state(false);

  const refresh = async () =>
    (setup = await call<LibrarySetup>("library_setup", {}, true));

  onMount(() => {
    refresh();
    call<RemoteState>("remote_state", {}, true).then((r) => (remote = r));
    // the host's download of ogle-extract ended
    return onEngine("jle:progress", (p) => {
      if (p?.id !== "extract_fetch") return;
      fetching = !p.finished;
      if (p.finished) refresh();
    });
  });

  async function changeLibrary() {
    if (!setup) return;
    const path = await open({
      directory: true,
      defaultPath: setup.root || setup.default || undefined,
    });
    if (typeof path !== "string") return;
    const before = setup.root;
    const r = await call<LibrarySetup>("choose_library", { path });
    if (!r) return;
    setup = r;
    left = r.root_game || r.root === before ? "" : before;
  }

  async function setRemote(on: boolean) {
    remote = (await call<RemoteState>("set_remote_control", { on })) ?? remote;
  }

  async function fetchExtractor() {
    fetching = true;
    if (!(await call("extract_fetch"))) fetching = false;
  }
</script>

<div class="flex flex-col text-sm">
  <h3 class={heading}>{t("settings.library")}</h3>
  <p class="mb-2 {muted}">{t("settings.library.help")}</p>
  <div
    class="flex items-center gap-2 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-1.5"
  >
    <IconFolder class="shrink-0 text-neutral-400" />
    <span
      class="min-w-0 flex-1 font-mono text-xs break-all text-neutral-200 select-text"
      >{setup?.root}</span
    >
  </div>
  {#if setup}
    <p
      class="mt-1.5 text-xs {setup.root_game
        ? 'text-green-400'
        : 'text-amber-300'}"
    >
      {t(
        setup.root_game ? "settings.library.game" : "settings.library.no_game",
      )}
    </p>
  {/if}
  {#if left}
    <p class="mt-1.5 text-xs text-neutral-300">
      {t("settings.library.empty", left)}
    </p>
  {/if}
  <button class="{button} mt-2 self-start" onclick={changeLibrary}
    >{t("settings.library.change")}</button
  >

  <h3 class={heading}>{t("settings.language")}</h3>
  <select
    class="{input} max-w-48"
    value={language()}
    onchange={(e) => changeLanguage(e.currentTarget.value)}
  >
    {#each languages as l}<option value={l.code}>{l.name}</option>{/each}
  </select>

  <h3 class={heading}>{t("settings.extractor")}</h3>
  {#if setup?.extractor}
    <p class="text-xs break-all text-neutral-300 select-text">
      {t("settings.extractor.found", setup.extractor)}
    </p>
  {:else if setup}
    <p class="text-xs text-red-400">{t("settings.extractor.missing")}</p>
    <ul class="mt-1 list-disc pl-5 text-xs break-all text-neutral-400">
      {#each setup.extractor_dirs as dir}<li>{dir}</li>{/each}
    </ul>
    {#if setup.can_fetch}
      <button
        class="{primary} mt-2 self-start"
        disabled={fetching}
        onclick={fetchExtractor}
      >
        {fetching ? t("extract.fetching") : t("extract.fetch")}
      </button>
    {/if}
  {/if}

  <h3 class={heading}>{t("settings.remote")}</h3>
  <p class="mb-2 {muted}">{t("settings.remote.help")}</p>
  <label class="flex items-center gap-2 text-neutral-200">
    <input
      type="checkbox"
      class={check}
      checked={remote?.on}
      onchange={(e) => setRemote(e.currentTarget.checked)}
    />
    {t("settings.remote.allow")}
  </label>
  {#if remote}
    <p
      class="mt-1.5 text-xs {remote.port
        ? 'text-green-400'
        : 'text-neutral-400'}"
    >
      {remote.port
        ? t("settings.remote.listening", remote.port)
        : t("settings.remote.off")}
    </p>
    {#if remote.session}<p class="mt-1 {muted}">
        {t("settings.remote.session")}
      </p>{/if}
  {/if}
</div>
