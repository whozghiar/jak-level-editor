<script lang="ts" module>
  // the window's choices, kept while it is closed (ExtractState's source and output)
  let source = $state("");
  let output = $state("");
</script>

<script lang="ts">
  // File > Extract game assets (ui_extract_window, extract.cpp): ogle-extract reads the user's disc
  // (an .iso or a disc folder) and writes its levels to <library>/jak3/. The engine runs it
  // (crates/core/src/editor/extract.rs); the window follows it with extract_state, on jle:progress
  // and every 500 ms while it runs.
  import { open } from "@tauri-apps/plugin-dialog";
  import { Progressbar } from "flowbite-svelte";
  import { onMount } from "svelte";
  import { ui } from "../../lib/actions.svelte";
  import { has, t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";

  // a text of the engine: its translation (then ": detail"), else as ogle-extract said it
  type Text = { key: string; args: number[]; detail: string; text: string };
  type Extraction = {
    running: boolean;
    detecting: boolean;
    detected: {
      source: string;
      game: string;
      name: string;
      version: string;
      serial: string;
      error: Text | null;
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
    source: string;
    output: string;
    library: { root: string; levels: number } | null;
    iso_dir: string;
    extractor_found: boolean;
    extractor_dir: string;
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
  const extracting = $derived(running && !ex?.detecting);
  const recognized = $derived(
    !!ex?.detected.game && ex.detected.source === source,
  );
  // Jak 3, or Jak II for its structures (brought into Jak 3 levels: Catalog > Decor > Jak II)
  const jak2 = $derived(recognized && ex?.detected.game === "jak2");
  const matches = $derived(
    recognized && (ex?.detected.game === "jak3" || jak2),
  );

  function show(s: Extraction | undefined) {
    if (!s) return;
    // done: the Levels panel shows what was written (<library>/jak3)
    if (ex?.running && !ex.detecting && !s.running && s.finished)
      ui.leftTab = "levels";
    ex = s;
  }

  const refresh = async () =>
    show(await call<Extraction>("extract_state", {}, true));
  const detect = async () =>
    show(await call<Extraction>("extract_detect", { source }));
  const start = async () =>
    show(await call<Extraction>("extract_start", { source, output }));
  const cancel = async () => show(await call<Extraction>("extract_stop"));

  async function chooseSource(directory: boolean) {
    const d = await call<Defaults>("extract_defaults", {}, true);
    const path = await open({
      directory,
      defaultPath: d?.iso_dir || undefined,
      filters: directory
        ? undefined
        : [{ name: "Disc image (.iso)", extensions: ["iso"] }],
    });
    if (typeof path !== "string") return;
    source = path;
    detect();
  }

  async function chooseOutput() {
    const d = await call<Defaults>("extract_defaults", {}, true);
    const path = await open({
      directory: true,
      defaultPath: d?.output || undefined,
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
      if (!source && !(ex?.running && !ex.detecting)) {
        // the disc chosen last
        source = defaults.source;
        detect();
      }
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

{#if defaults && !defaults.extractor_found}
  <p class={bad}>{t("extract.missing")}</p>
  <p class="mt-1 text-sm text-neutral-400">
    {t("extract.missing.help", defaults.extractor_dir)}
  </p>
{:else if defaults}
  <div class="flex flex-col gap-4 text-sm">
    <div>
      <p class="text-neutral-300">{t("extract.intro")}</p>
      {#if defaults.library}
        <p class="mt-1 {muted}">
          {t("extract.already", defaults.library.levels, defaults.library.root)}
        </p>
      {/if}
    </div>

    <!-- 1. the disc -->
    <section>
      <h3 class={heading}>{t("extract.disc")}</h3>
      <div class="flex gap-2">
        <input
          class={input}
          placeholder={t("extract.disc.hint")}
          bind:value={source}
          onchange={detect}
          disabled={extracting}
        />
        <button
          class={button}
          disabled={extracting}
          onclick={() => chooseSource(false)}>{t("extract.iso_file")}</button
        >
        <button
          class={button}
          disabled={extracting}
          onclick={() => chooseSource(true)}>{t("extract.folder")}</button
        >
      </div>
      <div class="mt-1.5">
        {#if running && ex?.detecting}
          <p class={muted}>{t("extract.detecting")}</p>
        {:else if recognized && ex}
          <p>
            <span class="font-semibold {matches ? good : bad}"
              >{ex.detected.name}</span
            >
            <span class={muted}
              >{ex.detected.version} &nbsp;-&nbsp; {ex.detected.serial}</span
            >
          </p>
          {#if !matches}<p class={bad}>{t("extract.wrong_game")}</p>{/if}
          {#if jak2}<p class="mt-1 text-neutral-300">
              {t("extract.jak2")}
            </p>{/if}
        {:else if ex?.detected.error && ex.detected.source === source}
          <p class={bad}>{text(ex.detected.error)}</p>
        {:else if !source}
          <p class={muted}>{t("extract.disc.choose")}</p>
        {/if}
      </div>
    </section>

    <!-- 2. where -->
    <section>
      <h3 class={heading}>{t("extract.destination")}</h3>
      <div class="flex gap-2">
        <input
          class={input}
          placeholder={t("extract.destination.hint")}
          bind:value={output}
          disabled={extracting}
        />
        <button class={button} disabled={extracting} onclick={chooseOutput}
          >{t("extract.choose")}</button
        >
      </div>
      {#if output}<p class="mt-1.5 {muted}">
          {t("extract.destination.where", output)}
        </p>{/if}
    </section>

    <button
      class="self-start rounded border border-orange-600 bg-orange-600 px-6 py-2 font-semibold text-white hover:bg-orange-500 disabled:pointer-events-none disabled:opacity-40"
      disabled={running || !matches || !output}
      onclick={start}
    >
      {t("extract.start")}
    </button>

    <!-- progress and outcome of an extraction -->
    {#if ex && !ex.detecting && (running || ex.log.length)}
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
