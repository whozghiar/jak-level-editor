<script lang="ts" module>
  // the options being edited, kept while the window is closed, for the level they are of and the
  // one it replaces (export_options and m_export_level of the C++, cleared by Replace)
  let kept: { level: string; options: any } | null = null;
</script>

<script lang="ts">
  // Level > Export for the game (ui_export_window, panels.cpp): the level as a custom level of
  // OpenGOAL for Jak 3, the files its level builder takes. The engine writes it on a worker
  // (export_for_game, then export_state when the job ends); its report says what it wrote, changed
  // or left out, and README.md at the top of the folder how to play it in the game.
  import { open } from "@tauri-apps/plugin-dialog";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { playLevel } from "../../lib/actions.svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine, toast } from "../../lib/rpc";
  import type { Vec3 } from "../../render/math";
  import { viewport } from "../../render/viewport_api";
  import { check } from "../common/classes";
  import Tip from "../common/Tip.svelte";

  type Options = {
    name: string;
    nickname: string;
    iso_name: string;
    base_id: number;
    folder: string;
    actors: boolean;
    game_collision: boolean;
    start: Vec3;
    replaces: string;
  };
  type Report = {
    dir: string;
    meshes: number;
    triangles: number;
    collision_triangles: number;
    actors: number;
    art_groups: number;
    navmeshes: number;
    regions: number;
    dense_places: number;
    densest: [number, Vec3][];
    notes: { key: string; count: number; detail: string }[];
  };
  type ExportState = {
    busy: boolean;
    generation: number;
    report: Report | null;
    error: string;
  };
  type Continue = { name: string; position: Vec3; ground: number | null };

  let o = $state<Options>();
  let level = "";
  let replaceable = $state<string[]>([]);
  let bad = $state<string | null>(null);
  let continues = $state<Continue[]>([]);
  // the last export of this window (reset when it opens, as the C++)
  let report = $state<Report | null>(null);
  let error = $state("");
  let busy = $state(false);

  function show(s: ExportState) {
    busy = s.busy;
    if (s.busy) return;
    report = s.report;
    error = s.error;
  }

  async function exportNow() {
    if (!o) return;
    report = null;
    error = "";
    const s = await call<ExportState>("export_for_game", {
      options: $state.snapshot(o),
    });
    if (s) show(s);
  }

  async function resetStart() {
    const p = await call<Vec3>("export_start_point");
    if (p && o) o.start = p;
  }

  async function browse() {
    const path = await open({
      directory: true,
      defaultPath: o?.folder || undefined,
    });
    if (typeof path === "string" && o) o.folder = path;
  }

  const openIt = (path: string) =>
    openPath(path).catch((e) => toast(String(e), "error"));

  // a dense place of the floors, looked at from above
  function go(at: Vec3) {
    viewport.setCamera({ pos: [at[0], at[1] + 6, at[2] + 9], look_at: at });
  }

  onMount(() => {
    (async () => {
      level = `${engine.state.document.level ?? ""}|${engine.state.document.replaces ?? ""}`;
      // an export going on (started before the window opened)
      const s = await call<ExportState>("export_state", {}, true);
      busy = !!s?.busy;
      const d = await call<{ options: Options; replaceable: string[] }>(
        "export_defaults",
      );
      if (!d) return;
      replaceable = d.replaceable;
      o = kept?.level === level ? kept.options : d.options;
    })();
    const off = onEngine("jle:progress", async (j) => {
      if (j?.id !== "export" || !j.finished) return;
      const s = await call<ExportState>("export_state", {}, true);
      if (s) show(s);
    });
    return off;
  });

  // what is wrong with the names, checked by the engine at each change (the game's levels)
  $effect(() => {
    if (!o) return;
    const options = $state.snapshot(o);
    kept = { level, options };
    void call<string | null>("export_check", { options }, true).then(
      (k) => (bad = k ?? null),
    );
  });

  // in place of a level: where the game starts the player there
  $effect(() => {
    const replaces = o?.replaces;
    continues = [];
    if (replaces)
      void call<Continue[]>("check_continues", { level: replaces }, true).then(
        (c) => (continues = c ?? []),
      );
  });

  const muted = "text-xs text-neutral-500";
  const warn = "text-orange-400";
  const heading =
    "mt-3 mb-1.5 border-b border-white/10 pb-1 text-xs font-semibold tracking-wide text-neutral-400 uppercase";
  const input =
    "min-w-0 flex-1 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-0.5 text-sm outline-none focus:border-orange-600";
  const button =
    "shrink-0 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-3 py-1 text-sm hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-50";
  const label = "w-36 shrink-0 text-neutral-300";
</script>

{#snippet text(
  key: string,
  value: string,
  max: number,
  set: (v: string) => void,
)}
  <label class="flex items-center gap-2">
    <span class={label}>{t(key)}</span>
    <input
      class={input}
      maxlength={max}
      {value}
      oninput={(e) => set(e.currentTarget.value)}
    />
  </label>
  <Tip text={t(`${key}.tip`)} placement="left" />
{/snippet}

{#if o}
  <div class="flex flex-col gap-1.5 text-sm">
    <p class="text-neutral-300">{t("export.help")}</p>

    <h3 class={heading}>{t("export.names")}</h3>
    {@render text("export.name", o.name, 10, (v) => (o!.name = v))}
    {@render text("export.nickname", o.nickname, 3, (v) => (o!.nickname = v))}
    {@render text("export.iso_name", o.iso_name, 8, (v) => (o!.iso_name = v))}
    <label class="flex items-center gap-2">
      <span class={label}>{t("export.base_id")}</span>
      <input
        class={input}
        type="number"
        min="0"
        step="1"
        value={o.base_id}
        onchange={(e) => {
          const v = e.currentTarget.valueAsNumber;
          if (Number.isInteger(v) && v >= 0) o!.base_id = v;
        }}
      />
    </label>
    <Tip text={t("export.base_id.tip")} placement="left" />
    <!-- in place of a level of the game, or a level of its own -->
    <label class="flex items-center gap-2">
      <span class={label}>{t("export.replaces")}</span>
      <select class={input} bind:value={o.replaces}>
        <option value="">{t("export.replaces.none")}</option>
        {#each replaceable.includes(o.replaces) || !o.replaces ? replaceable : [...replaceable, o.replaces] as l (l)}
          <option value={l}>{l}</option>
        {/each}
      </select>
    </label>
    <Tip text={t("export.replaces.tip")} placement="left" />

    <h3 class={heading}>{t("export.content")}</h3>
    <label class="flex items-center gap-2">
      <input type="checkbox" class={check} bind:checked={o.actors} />
      {t("export.actors")}
    </label>
    <Tip text={t("export.actors.tip")} placement="left" />
    <div class="flex gap-4">
      <label class="flex items-center gap-2">
        <input
          type="radio"
          class={check}
          checked={o.game_collision}
          onchange={() => (o!.game_collision = true)}
        />
        {t("export.collision.game")}
      </label>
      <Tip text={t("export.collision.game.tip")} placement="left" />
      <label class="flex items-center gap-2">
        <input
          type="radio"
          class={check}
          checked={!o.game_collision}
          onchange={() => (o!.game_collision = false)}
        />
        {t("export.collision.decor")}
      </label>
      <Tip text={t("export.collision.decor.tip")} placement="left" />
    </div>
    {#if !o.replaces}
      <div class="flex items-center gap-2">
        <span class={label}>{t("export.start")}</span>
        {#each [0, 1, 2] as k (k)}
          <input
            class="{input} w-0 text-right"
            type="number"
            step="0.1"
            value={o.start[k].toFixed(1)}
            onchange={(e) => {
              const v = e.currentTarget.valueAsNumber;
              if (Number.isFinite(v)) o!.start[k] = v;
            }}
          />
        {/each}
        <button class={button} onclick={resetStart}
          >{t("export.start.reset")}</button
        >
        <Tip text={t("export.start.reset.tip")} placement="left" />
      </div>
    {:else}
      <!-- the game starts the player at the continue points of the level replaced -->
      <p class="text-neutral-300">{t("export.replaces.start", o.replaces)}</p>
      {#if continues.length}
        <p class={muted}>{t("replace.continues", o.replaces)}</p>
        <Tip text={t("replace.continues.tip")} placement="left" />
        <div class="flex flex-wrap gap-1">
          {#each continues as c (c.name)}
            <button
              class="rounded border border-white/10 px-2 py-0.5 text-xs hover:bg-white/5 {c.ground !==
              null
                ? 'text-green-400'
                : warn}"
              onclick={() => go(c.position)}
            >
              {c.name}
            </button>
            <Tip
              text={c.ground !== null
                ? t("replace.continue.ok", c.position[1] - c.ground)
                : t("replace.continue.none")}
            />
          {/each}
        </div>
      {/if}
    {/if}

    <h3 class={heading}>{t("export.folder")}</h3>
    <div class="flex items-center gap-2">
      <span class={label}>{t("export.folder_path")}</span>
      <input class={input} bind:value={o.folder} />
      <button class={button} onclick={browse}>{t("export.browse")}</button>
    </div>
    <Tip text={t("export.folder_path.tip")} placement="left" />
    <p class={muted}>{t("export.folder.hint")}</p>

    {#if bad}<p class="mt-2 {warn}">{t(bad)}</p>{/if}
    <button
      class="mt-2 rounded border border-orange-600 bg-orange-600 px-6 py-2 font-semibold text-white hover:bg-orange-500 disabled:pointer-events-none disabled:opacity-40"
      disabled={!!bad || !engine.state.document.open || busy}
      onclick={exportNow}
    >
      {busy ? `${t("export.title")}...` : t("export.button")}
    </button>
    {#if error}<p class={warn}>{error}</p>{/if}

    {#if report}
      <h3 class={heading}>{t("export.done")}</h3>
      <p class="break-all">{t("export.written", report.dir)}</p>
      <p>
        {t(
          "export.counts",
          report.meshes,
          report.triangles,
          report.collision_triangles,
          report.actors,
          report.art_groups,
        )}
      </p>
      {#if report.navmeshes}<p>
          {t("export.counts.navmeshes", report.navmeshes)}
        </p>{/if}
      {#if report.regions}<p>
          {t("export.counts.regions", report.regions)}
        </p>{/if}
      <ul class="ml-4 list-disc text-neutral-300">
        {#each report.notes as note, i (i)}
          <li>{t(note.key, note.count, note.detail)}</li>
        {/each}
      </ul>
      {#if report.densest.length}
        <div class="flex flex-wrap gap-1">
          {#each report.densest.slice(0, 8) as [n, at], i (i)}
            <button
              class="rounded border border-white/10 px-2 py-0.5 text-xs {warn} hover:bg-white/5"
              onclick={() => go(at)}
            >
              {at.map((v) => v.toFixed(0)).join(", ")}: {n}
            </button>
          {/each}
        </div>
        <Tip text={t("export.dense_go")} placement="left" />
      {/if}
      <p class="mt-1 text-neutral-300">{t("export.install")}</p>
      <!-- with the host's game runtime the editor does it (Play); without, by hand -->
      {#if !engine.state.runtime?.available}<p class={muted}>
          {t("export.no_runtime")}
        </p>{/if}
      <div class="flex gap-2">
        <button class={button} onclick={() => openIt(report!.dir)}
          >{t("export.open_folder")}</button
        >
        <button
          class={button}
          onclick={() => openIt(`${report!.dir}/README.md`)}
          >{t("export.open_readme")}</button
        >
        {#if engine.state.runtime?.available}
          <button class={button} disabled={busy} onclick={playLevel}
            >{t("menu.level.play")}</button
          >
        {/if}
      </div>
    {/if}
  </div>
{/if}
