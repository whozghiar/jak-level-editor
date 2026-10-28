<script lang="ts" module>
  // the window's destination, kept while it is closed
  let output = $state("");
</script>

<script lang="ts">
  // File > Extract game assets: the levels of a game installed with OpenGOAL (the OpenGOAL
  // Launcher's, else the folder of the settings), read by its own extractor and written to
  // <library>/<game>/. The engine runs it (crates/core/src/io/extraction, editor/extract.rs); the
  // window follows it with extract_state, on jle:progress and every 500 ms while it runs.
  import { open } from "@tauri-apps/plugin-dialog";
  import { Progressbar } from "flowbite-svelte";
  import { onMount } from "svelte";
  import { refreshGameTest } from "../../lib/game_test.svelte";
  import { has, t } from "../../lib/i18n";
  import { games } from "../../lib/games";
  import { call, onEngine } from "../../lib/rpc";
  import { check, wip } from "../common/classes";

  // the game asked (File > Extract game assets > Jak II, a panel's Extract Jak II...)
  let { game: asked }: { game?: string } = $props();

  // a text of the engine: its translation (then ": detail"), else as it came
  type Text = { key: string; args: number[]; detail: string; text: string };
  type Extraction = {
    running: boolean;
    detected: {
      game: string;
      folder: string;
      from_host: boolean;
      install: { version: string; root: string } | null;
      error: Text | null;
      serial: string;
      levels: number;
    };
    log: string[];
    step: Text | null;
    done: number;
    total: number;
    current: string;
    error: Text | null;
    finished: boolean;
    seconds: number;
  };
  type Defaults = {
    output: string;
    library: { root: string; levels: number } | null;
  };

  let ex = $state<Extraction>();
  let defaults = $state<Defaults>();
  let logBox = $state<HTMLDivElement>();
  let follow = true;

  const text = (m: Text) =>
    has(m.key)
      ? t(m.key, ...m.args) + (m.detail ? `: ${m.detail}` : "")
      : m.text;
  // 75 -> "1:15"
  function timeText(seconds: number): string {
    const s = Math.floor(Math.max(0, seconds));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  const running = $derived(!!ex?.running);
  // the game whose levels are read: Jak 3, or Jak II for its structures (brought into Jak 3
  // levels: Catalog > Decor > Jak II)
  let game = $state("jak3");
  const detected = $derived(
    ex?.detected.game === game ? ex.detected : undefined,
  );
  const ready = $derived(!!detected?.install && !detected.error);

  function show(s: Extraction | undefined) {
    if (s) ex = s;
  }
  const refresh = async () =>
    show(await call<Extraction>("extract_state", {}, true));
  const detect = async () =>
    show(await call<Extraction>("extract_detect", { game }));
  const start = async () =>
    show(await call<Extraction>("extract_start", { game, output }));
  const cancel = async () => show(await call<Extraction>("extract_stop"));

  function pick(g: string) {
    game = g;
    detect();
  }
  let picked = "";
  $effect(() => {
    if (defaults && asked && asked !== picked) {
      picked = asked;
      pick(asked);
    }
  });

  // the folder of the game installed, as File > Settings keeps it
  async function chooseInstall() {
    const path = await open({
      directory: true,
      defaultPath: detected?.folder || undefined,
    });
    if (typeof path !== "string") return;
    await call("set_game_install", { game, folder: path });
    refreshGameTest();
    detect();
  }

  async function chooseOutput() {
    const path = await open({
      directory: true,
      defaultPath: output || undefined,
    });
    if (typeof path === "string") output = path;
  }

  onMount(() => {
    const timer = setInterval(() => ex?.running && refresh(), 500);
    const off = onEngine("jle:progress", refresh);
    (async () => {
      defaults = await call<Defaults>("extract_defaults");
      if (!defaults) return;
      if (!output) output = defaults.output;
      await refresh();
      if (ex?.running) game = ex.detected.game || game;
      else if (!asked) detect();
    })();
    return () => {
      clearInterval(timer);
      off();
    };
  });

  $effect(() => {
    ex?.log.length;
    if (logBox && follow) logBox.scrollTop = logBox.scrollHeight;
  });

  const good = "text-green-400";
  const bad = "text-red-400";
  const muted = "text-xs text-neutral-500";
  const heading =
    "mb-1.5 border-b border-white/10 pb-1 text-xs font-semibold tracking-wide text-neutral-400 uppercase";
  const input =
    "min-w-0 flex-1 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-1 text-sm outline-none focus:border-orange-600 disabled:opacity-50";
  const button =
    "shrink-0 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-3 py-1 text-sm hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-50";
</script>

{#if defaults}
  <div class="flex flex-col gap-4 text-sm">
    <div>
      <p class="text-neutral-300">{t("extract.intro")}</p>
      {#if defaults.library}
        <p class="mt-1 {muted}">
          {t("extract.already", defaults.library.levels, defaults.library.root)}
        </p>
      {/if}
    </div>

    <!-- 1. the game installed -->
    <section>
      <h3 class={heading}>{t("extract.game")}</h3>
      <div class="mb-2 flex flex-wrap items-center gap-3">
        {#each games as g}
          <label
            class="flex items-center gap-1.5 {g.extractable
              ? 'cursor-pointer'
              : 'text-neutral-500'}"
          >
            <input
              type="radio"
              class={check}
              name="extract-game"
              checked={game === g.id}
              disabled={!g.extractable || running}
              onchange={() => pick(g.id)}
            />
            {g.name}
            {#if !g.extractable}<span class={wip}>{t("wip")}</span>{/if}
          </label>
        {/each}
      </div>
      {#if detected}
        <div class="flex items-center gap-2">
          <span
            class="min-w-0 flex-1 font-mono text-xs break-all text-neutral-300 select-text"
            >{detected.folder || t("settings.games.none")}</span
          >
          {#if !detected.from_host}
            <button class={button} disabled={running} onclick={chooseInstall}
              >{t("settings.games.choose")}</button
            >
          {/if}
        </div>
        {#if ready && detected.install}
          <p class="mt-1.5 {good}">
            {t(
              "extract.game.found",
              detected.install.version || "?",
              detected.levels,
              detected.serial || "?",
            )}
          </p>
        {:else if detected.error}
          <p class="mt-1.5 {bad}">{text(detected.error)}</p>
          {#if !detected.from_host}<p class="mt-1 {muted}">
              {t("extract.game.help")}
            </p>{/if}
        {/if}
        {#if detected.from_host}<p class="mt-1 {muted}">
            {t("settings.games.from_host")}
          </p>{/if}
        {#if game === "jak2"}
          <p class="mt-1 text-neutral-300">{t("extract.jak2")}</p>
          <p class="mt-1 flex items-center gap-2 {muted}">
            <span class={wip}>{t("wip")}</span>{t(
              "menu.game.wip_tip",
              "Jak II",
            )}
          </p>
        {/if}
      {/if}
    </section>

    <!-- 2. where -->
    <section>
      <h3 class={heading}>{t("extract.destination")}</h3>
      <div class="flex gap-2">
        <input
          class={input}
          placeholder={t("extract.destination.hint")}
          bind:value={output}
          disabled={running}
        />
        <button class={button} disabled={running} onclick={chooseOutput}
          >{t("extract.choose")}</button
        >
      </div>
      {#if output}<p class="mt-1.5 {muted}">
          {t("extract.destination.where", output, game)}
        </p>{/if}
    </section>

    <button
      class="self-start rounded border border-orange-600 bg-orange-600 px-6 py-2 font-semibold text-white hover:bg-orange-500 disabled:pointer-events-none disabled:opacity-40"
      disabled={running || !ready || !output}
      onclick={start}
    >
      {t("extract.start")}
    </button>

    <!-- progress and outcome of an extraction -->
    {#if ex && (running || ex.log.length)}
      {#if running}
        <div class="flex items-end gap-3">
          <div class="flex-1">
            <Progressbar
              color="orange"
              size="h-2"
              progress={ex.total ? Math.round((100 * ex.done) / ex.total) : 0}
              labelOutside={ex.total
                ? `${ex.done} / ${ex.total}  ${ex.current}`
                : t("extract.preparing")}
              classes={{
                span: "text-xs text-neutral-300",
                progressCls: "text-xs text-neutral-400",
              }}
            />
          </div>
          <button class={button} onclick={cancel}>{t("extract.cancel")}</button>
        </div>
        <p class={muted}>
          {ex.step ? text(ex.step) : ""} &nbsp;-&nbsp; {timeText(ex.seconds)}
        </p>
      {:else if ex.finished}
        <p class={good}>
          {t(
            "extract.done",
            timeText(ex.seconds),
            ex.done > 0 ? ex.done - 1 : 0,
          )}
        </p>
      {:else if ex.error}
        <p class={bad}>{text(ex.error)}</p>
      {/if}
    {/if}

    {#if ex?.log.length}
      <details
        ontoggle={() => logBox && (logBox.scrollTop = logBox.scrollHeight)}
      >
        <summary
          class="cursor-pointer text-xs text-neutral-400 select-none hover:text-neutral-200"
          >{t("extract.log")}</summary
        >
        <div
          bind:this={logBox}
          class="mt-1.5 h-[150px] overflow-auto rounded border border-white/5 bg-[#0b0b0b] px-2 py-1 font-mono text-[11px] whitespace-pre text-neutral-300 select-text"
          onscroll={() =>
            logBox &&
            (follow =
              logBox.scrollTop + logBox.clientHeight >=
              logBox.scrollHeight - 20)}
        >
          {#each ex.log as l}<div>{l}</div>{/each}
        </div>
        <button
          class="mt-1.5 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-0.5 text-xs hover:border-[#3a3a3a] hover:bg-[#1a1a1a]"
          onclick={() =>
            ex && navigator.clipboard.writeText(ex.log.join("\n") + "\n")}
        >
          {t("log.copy")}
        </button>
      </details>
    {/if}
  </div>
{/if}
