<script lang="ts" module>
  // kept while the tab is closed (the C++ App's m_catalog_tab, m_catalog_jak2, m_catalog_filter...)
  type Tab =
    "objects" | "interactive" | "effects" | "models" | "decor" | "materials";
  let tab = $state<Tab>("objects");
  let jak2 = $state(false);
  let filter = $state("");
  let family = $state(""); // the objects' family, "" every family
  let effectKind = $state(""); // the effects' category
  let modelsLevel = $state("");
  let decorLevel = $state("");
</script>

<script lang="ts">
  // The Catalog tab (ui_catalog_panel, ui_catalog_objects, ui_catalog_models, ui_catalog_decor,
  // ui_catalog_decor_all, ui_catalog_materials in panels.cpp): the objects of the game, the
  // interactive ones, the particle effects, its other models, the decor of its levels, the
  // textures of the games; a click places one (then a click in the view), a drag into the view
  // places one there. Jak II's models, structures and textures once it is extracted.
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import type { ThumbRequest } from "../../render/thumbnails";
  import { check, input, muted } from "../common/classes";
  import Dropdown from "../common/Dropdown.svelte";
  import Jak2Missing from "../common/Jak2Missing.svelte";
  import { panelDrag } from "../common/panelDrag";
  import Tip from "../common/Tip.svelte";
  import TextureBrowser from "../windows/TextureBrowser.svelte";
  import Thumb from "./Thumb.svelte";
  import ThumbGrid from "./ThumbGrid.svelte";

  type Obj = {
    etype: string;
    name: string;
    family: string;
    category: string;
    model: string;
    model_path: string;
    art: string;
    count: number;
    levels: string[];
  };
  type Model = { name: string; path: string; group: string; game: string };
  type Part = {
    level: string;
    background: string;
    stamp: number;
    levels: number;
    part: {
      kind: "prototype" | "piece";
      name: string;
      mesh_node: string;
      triangles: number;
      copies: number;
    };
  };

  const s = $derived(engine.state);
  const game2 = $derived(!!s.jak2_available && jak2);
  const objectTab = $derived(
    tab === "objects" || tab === "interactive" || tab === "effects",
  );
  // Jak II has no objects for Jak 3
  $effect(() => {
    if (game2 && objectTab) tab = "models";
  });

  // the filter asked a moment after the last key
  let query = $state(filter);
  let timer = 0;
  $effect(() => {
    const f = filter;
    clearTimeout(timer);
    timer = window.setTimeout(() => (query = f), 150);
  });

  // the list of the tab shown (another tab's, until the new one comes, is not)
  let got = $state<any>(null);
  const answer = $derived(got?.tab === tab ? got : null);
  let asked = 0;
  // the list again when what it shows changes (the index read, a level's decor read, the library)
  // (a value: the effect runs again when it changes, not at each state the engine sends)
  const catalogKey = $derived(
    JSON.stringify([s.catalog?.epoch, s.library_root]),
  );
  const empty = $derived(!!s.library_empty); // nothing asked (the levels chosen kept)
  $effect(() => {
    if (tab === "materials" || empty) return;
    const args = {
      tab,
      jak2: game2,
      filter: query,
      family: tab === "effects" ? effectKind : family,
      level: tab === "models" ? modelsLevel : decorLevel,
    };
    void catalogKey;
    const n = ++asked;
    call("catalog", args, true).then((r) => {
      if (n !== asked) return;
      got = r ? { ...r, tab: args.tab } : null;
      // a level no longer in the library (another library, the other game): every level (the C++
      // clears m_decor_level, m_models_level)
      if (typeof r?.level === "string" && r.level !== args.level) {
        if (args.tab === "models") modelsLevel = r.level;
        else if (args.tab === "decor") decorLevel = r.level;
      }
    });
  });

  const kinds = $derived<{ tab: Tab; key: string; guide: string }[]>([
    ...(game2
      ? []
      : [
          { tab: "objects" as Tab, key: "catalog.objects", guide: "catalog" },
          {
            tab: "interactive" as Tab,
            key: "catalog.interactive",
            guide: "interactive",
          },
          { tab: "effects" as Tab, key: "catalog.effects", guide: "effects" },
        ]),
    {
      tab: "models",
      key: game2 ? "catalog.models.jak2" : "catalog.models",
      guide: "models",
    },
    { tab: "decor", key: "catalog.decor", guide: "decor" },
    { tab: "materials", key: "catalog.materials", guide: "palette" },
  ]);

  function chooseGame(two: boolean) {
    if (two === jak2) return;
    jak2 = two;
    decorLevel = modelsLevel = "";
  }

  // what is being placed: its cell marked
  const pl = $derived(s.tool === "place" ? s.placement : null);
  const placingObject = (o: Obj) =>
    pl?.kind === "object" && pl.name === o.name && pl.etype === o.etype;
  const placingModel = (m: Model) => pl?.kind === "model" && pl.path === m.path;
  const placingPart = (p: Part) =>
    pl?.kind === "part" &&
    pl.path === p.background &&
    pl.mesh_node === p.part.mesh_node;

  // App::catalog_thumb: its model, its particles, or what it does
  function objectThumb(o: Obj): ThumbRequest {
    const plain: ThumbRequest = o.model_path
      ? { kind: "model", path: o.model_path }
      : { kind: "icon", etype: o.etype, family: o.family };
    return o.family === "effect" && o.art
      ? { kind: "effect", group: o.art, fallback: plain }
      : plain;
  }

  const placeObject = (o: Obj, once: boolean) =>
    call("start_object_placement", { etype: o.etype, name: o.name, once });
  const placeModel = (m: Model, once: boolean) =>
    call("start_model_placement", {
      name: m.name,
      path: m.path,
      game: m.game,
      group: m.group,
      once,
    });
  const placePart = (p: Part, once: boolean) =>
    call("start_part_placement", {
      background: p.background,
      name: p.part.name,
      mesh_node: p.part.mesh_node,
      once,
    });

  function objectTip(o: Obj): string {
    const lines = [o.name, t(o.category), t("inspector.type", o.etype)];
    if (o.family === "effect")
      lines.push(t("catalog.particles", o.art), t("catalog.effect_tip"));
    else
      lines.push(
        o.model ? t("inspector.model", o.model) : t("catalog.no_model"),
      );
    lines.push(
      o.count
        ? t("catalog.found", o.count, o.levels.join(", "))
        : t("catalog.not_found"),
    );
    lines.push(t("catalog.place_tip"));
    return lines.join("\n");
  }
  const modelTip = (m: Model) =>
    [
      m.name,
      t("catalog.from", m.group),
      t("catalog.model_tip"),
      t("catalog.place_tip"),
    ].join("\n");
  function partTip(p: Part, all: boolean): string {
    const lines = [
      p.part.name,
      p.part.kind === "prototype"
        ? t("catalog.prototype", p.part.copies)
        : t("catalog.piece"),
    ];
    if (all) lines.push(t("catalog.from", p.level));
    if (p.levels > 1) lines.push(t("catalog.in_levels", p.levels));
    lines.push(
      t("inspector.triangles", p.part.triangles),
      t("catalog.place_tip"),
    );
    return lines.join("\n");
  }

  // the Materials tab: the pieces selected (not the actors' models)
  let pieces = $state<number[]>([]);
  const piecesKey = $derived(
    JSON.stringify([s.selection.hash, s.history.revision]),
  );
  $effect(() => {
    if (tab !== "materials") return;
    void piecesKey;
    call<number[]>("selection_meshes", {}, true).then(
      (ids) => (pieces = ids ?? []),
    );
  });

  const indexing = $derived(
    engine.jobs[game2 ? "library_index_jak2" : "library_index"],
  );
</script>

{#snippet cellBody(req: ThumbRequest | null, name: string, active: boolean)}
  <div
    class="rounded border p-[2px] {active
      ? 'border-orange-500 bg-orange-500/25'
      : 'border-white/5 bg-[#0b0b0b] hover:border-white/25'}"
  >
    <Thumb {req} />
  </div>
  <p class="truncate text-center text-[11px] leading-4 text-neutral-300">
    {name}
  </p>
{/snippet}

<div class="flex h-full flex-col gap-2 p-3 text-sm">
  {#if s.library_empty}
    <p class="text-neutral-300">{t("catalog.no_library")}</p>
  {:else}
    <!-- the game: Jak 3's, or Jak II's (its models, structures and materials, brought in as decor:
         its objects' code is not in Jak 3), extracted first -->
    <div class="flex items-center gap-3 border-b border-white/5 pb-2">
      <span class="text-neutral-400">{t("catalog.game")}</span>
      {#each [false, true] as two}
        <label class="flex cursor-pointer items-center gap-1.5">
          <input
            type="radio"
            class={check}
            name="catalog-game"
            checked={jak2 === two}
            onchange={() => chooseGame(two)}
          />
          {two ? "Jak II" : "Jak 3"}
        </label>
      {/each}
    </div>
    <Tip guide="jak2" />

    {#if jak2 && !s.jak2_available}
      <Jak2Missing />
    {:else}
      <!-- its kinds -->
      <div class="flex flex-wrap gap-x-3 gap-y-1">
        {#each kinds as k}
          <label class="flex cursor-pointer items-center gap-1.5">
            <input
              type="radio"
              class={check}
              name="catalog-tab"
              checked={tab === k.tab}
              onchange={() => (tab = k.tab)}
            />
            {t(k.key)}
          </label>
          <Tip guide={k.guide} />
        {/each}
      </div>

      {#if tab === "materials"}
        <!-- the textures of the game chosen above, given to the selection (all its materials) -->
        <p class={muted}>
          {pieces.length
            ? t("build.materials.count", pieces.length)
            : t("build.materials.none")}
        </p>
        <div class="flex min-h-0 flex-1 flex-col">
          <TextureBrowser ids={pieces} slot={-1} game={game2 ? "jak2" : ""} />
        </div>
      {:else if objectTab}
        {#if tab === "effects"}<p class={muted}>
            {t("catalog.effects.help")}
          </p>{/if}
        <Dropdown
          items={[
            { label: t("catalog.all_families"), value: "" },
            ...((answer?.families ?? []) as string[]).map((f) => ({
              label: t(f),
              value: f,
            })),
          ]}
          value={answer?.family ?? ""}
          label={answer?.family ? t(answer.family) : t("catalog.all_families")}
          onselect={(v) =>
            tab === "effects" ? (effectKind = v) : (family = v)}
        />
        <input
          class={input}
          placeholder={t("catalog.filter")}
          bind:value={filter}
        />
        {#if answer?.items}
          <p class={muted}>{t("catalog.count", answer.count, answer.total)}</p>
          {#if answer.empty}<p class="text-neutral-400">
              {t("catalog.empty")}
            </p>{/if}
          <ThumbGrid
            items={answer.items as Obj[]}
            reset={[tab, query, answer.family]}
          >
            {#snippet cell(o: Obj)}
              <button
                class="block w-full"
                onclick={() => placeObject(o, false)}
                onpointerdown={(e) =>
                  panelDrag(e, o.name, () => void placeObject(o, true))}
              >
                {@render cellBody(objectThumb(o), o.name, placingObject(o))}
              </button>
              <Tip
                text={objectTip(o)}
                effect={o.family === "effect" && o.art ? o.art : undefined}
                placement="right"
              />
            {/snippet}
          </ThumbGrid>
        {/if}
      {:else if tab === "models"}
        {#if game2}<p class={muted}>{t("catalog.models.jak2.tip")}</p>{/if}
        <!-- the level they come from, or every level -->
        <Dropdown
          items={[
            { label: t("catalog.all_levels"), value: "" },
            ...((answer?.levels ?? []) as string[]).map((l) => ({
              label: l,
              value: l,
            })),
          ]}
          value={answer?.level ?? ""}
          label={answer?.level || t("catalog.all_levels")}
          filter={t("catalog.filter")}
          onselect={(v) => (modelsLevel = v)}
        />
        <input
          class={input}
          placeholder={t("catalog.filter")}
          bind:value={filter}
        />
        {#if answer?.items}
          <p class={muted}>{t("catalog.count", answer.count, answer.total)}</p>
          <ThumbGrid
            items={answer.items as Model[]}
            reset={[tab, game2, query, answer.level]}
          >
            {#snippet cell(m: Model)}
              <button
                class="block w-full"
                onclick={() => placeModel(m, false)}
                onpointerdown={(e) =>
                  panelDrag(e, m.name, () => void placeModel(m, true))}
              >
                {@render cellBody(
                  { kind: "model", path: m.path },
                  m.name,
                  placingModel(m),
                )}
              </button>
              <Tip text={modelTip(m)} placement="right" />
            {/snippet}
          </ThumbGrid>
        {/if}
      {:else}
        <!-- the decor of a level, or of every level -->
        <Dropdown
          items={[
            { label: t("catalog.all_levels"), value: "" },
            ...((answer?.levels ?? []) as string[]).map((l) => ({
              label: l,
              value: l,
            })),
          ]}
          value={decorLevel}
          label={decorLevel || t("catalog.all_levels")}
          filter={t("catalog.filter")}
          onselect={(v) => (decorLevel = v)}
        />
        {#if answer?.indexing}
          <!-- the levels are read once (then from the cache) -->
          {@const done = indexing?.done ?? answer.indexing.done}
          {@const total = indexing?.total ?? answer.indexing.total}
          <p class={muted}>{t("catalog.indexing", done, total)}</p>
          <div class="h-1.5 overflow-hidden rounded bg-white/10">
            <div
              class="h-full bg-orange-500"
              style:width="{total ? (100 * done) / total : 0}%"
            ></div>
          </div>
        {:else if answer?.loading}
          <p class={muted}>{t("catalog.decor.loading", answer.level)}</p>
        {:else if answer?.failed}
          <p class="text-neutral-400">{t("catalog.decor.failed")}</p>
        {:else if answer?.items}
          <input
            class={input}
            placeholder={decorLevel
              ? t("catalog.filter")
              : t("catalog.decor.filter")}
            bind:value={filter}
          />
          <p class={muted}>
            {t("catalog.parts_count", answer.count, answer.total)}
          </p>
          {#if decorLevel && !answer.total}<p class="text-neutral-400">
              {t("catalog.decor.none")}
            </p>{/if}
          <ThumbGrid
            items={answer.items as Part[]}
            reset={[tab, game2, query, decorLevel]}
          >
            {#snippet cell(p: Part)}
              <button
                class="block w-full"
                onclick={() => placePart(p, false)}
                onpointerdown={(e) =>
                  panelDrag(e, p.part.name, () => void placePart(p, true))}
              >
                {@render cellBody(
                  {
                    kind: "part",
                    path: p.background,
                    node: p.part.mesh_node,
                    stamp: p.stamp,
                  },
                  p.part.name,
                  placingPart(p),
                )}
              </button>
              <Tip text={partTip(p, !decorLevel)} placement="right" />
            {/snippet}
          </ThumbGrid>
        {/if}
      {/if}
    {/if}
  {/if}
</div>
