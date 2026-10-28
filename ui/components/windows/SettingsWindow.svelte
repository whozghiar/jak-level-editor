<script lang="ts">
  // File > Settings: the library folder (another one is used as it is: nothing moved or deleted),
  // the language, the games installed whose levels are extracted and tried (game_installs; the
  // host's, the OpenGOAL Launcher's, cannot be changed), whether an assistant (the MCP server) may
  // control the editor (its remote control: remote_state, set_remote_control).
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import IconFolder from "~icons/mdi/folder-outline";
  import { changeLanguage } from "../../lib/actions.svelte";
  import { refreshGameTest } from "../../lib/game_test.svelte";
  import { games } from "../../lib/games";
  import { language, languages, t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { button, check, heading, input, muted } from "../common/classes";
  import type { LibrarySetup } from "./WelcomeWindow.svelte";

  type RemoteState = { on: boolean; port: number | null; session: boolean };
  type GameInstall = {
    game: string;
    folder: string;
    from_host: boolean;
    install: { version: string } | null;
    error: string; // why levels cannot be tried in it
    extract_error: string; // why its levels cannot be extracted ("" when they can)
  };

  let setup = $state<LibrarySetup>();
  let remote = $state<RemoteState>();
  let installs = $state<GameInstall[]>([]);
  // the folder left for an empty one: what to do with it
  let left = $state("");

  const refresh = async () =>
    (setup = await call<LibrarySetup>("library_setup", {}, true));

  onMount(() => {
    refresh();
    call<RemoteState>("remote_state", {}, true).then((r) => (remote = r));
    call<GameInstall[]>("game_installs", {}, true).then(
      (r) => (installs = r ?? []),
    );
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

  async function setInstall(game: string, folder: string) {
    installs =
      (await call<GameInstall[]>("set_game_install", { game, folder })) ??
      installs;
    refreshGameTest();
  }

  async function chooseInstall(g: GameInstall) {
    const path = await open({
      directory: true,
      defaultPath: g.folder || undefined,
    });
    if (typeof path === "string") setInstall(g.game, path);
  }

  async function setRemote(on: boolean) {
    remote = (await call<RemoteState>("set_remote_control", { on })) ?? remote;
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

  <h3 class={heading}>{t("settings.games")}</h3>
  <p class="mb-2 {muted}">{t("settings.games.help")}</p>
  {#each installs as g (g.game)}
    {@const game = games.find((x) => x.id === g.game)}
    <div class="mb-2 flex flex-col">
      <div class="flex items-center gap-2">
        <span class="w-28 shrink-0 text-neutral-200"
          >{game?.name ?? g.game}</span
        >
        <span
          class="min-w-0 flex-1 font-mono text-xs break-all text-neutral-300 select-text"
          >{g.folder || t("settings.games.none")}</span
        >
        {#if !g.from_host}
          <button class={button} onclick={() => chooseInstall(g)}
            >{t("settings.games.choose")}</button
          >
          {#if g.folder}<button
              class={button}
              onclick={() => setInstall(g.game, "")}
              >{t("settings.games.forget")}</button
            >{/if}
        {/if}
      </div>
      <p
        class="mt-0.5 text-xs {g.install
          ? 'text-green-400'
          : g.folder
            ? 'text-amber-300'
            : 'text-neutral-400'}"
      >
        {g.install
          ? t("settings.games.ready", g.install.version || "?")
          : g.folder && !g.extract_error
            ? `${t("settings.games.extract_only")} ${t(g.error)}`
            : g.folder
              ? t(g.extract_error)
              : ""}
        {#if g.from_host}{t("settings.games.from_host")}{/if}
        {#if !game?.ready}{t("settings.games.wip")}{/if}
      </p>
    </div>
  {/each}

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
    {#if remote.session && !remote.on}<p class="mt-1 {muted}">
        {t("settings.remote.session")}
      </p>{/if}
  {/if}
</div>
