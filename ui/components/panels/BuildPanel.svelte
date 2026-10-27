<script lang="ts">
  // The Build tab (ui_build_panel, ui_level_style in guide_panel.cpp): the shapes (a click places
  // one, again and again; dragged into the view, one), Snap, floors and walls in the style of a
  // level, the texture palette on the pieces selected.
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { viewport } from "../../render/viewport_api";
  import { NodeKind } from "../../render/types";
  import { shapePresets } from "../guide/demos";
  import ToolDemo from "../guide/ToolDemo.svelte";
  import { button, check, heading, muted } from "../common/classes";
  import Dropdown from "../common/Dropdown.svelte";
  import { panelDrag } from "../common/panelDrag";
  import Tip from "../common/Tip.svelte";
  import TextureBrowser from "../windows/TextureBrowser.svelte";

  type Surface = { material: number; texture: string; tile: number };
  type Style = {
    reading?: boolean;
    failed?: boolean;
    floor?: Surface;
    wall?: Surface;
    bumps?: number;
    bump_size?: number;
    valid?: boolean;
  } | null;

  const s = $derived(engine.state);
  // ---- a wall, a floor or a house of the level's own pieces (editor/structures): what it is
  // like, then placed; it grows when it is resized once placed
  let kind = $state<"wall" | "floor" | "house">("house");
  let floors = $state(2);
  let wallFloors = $state(1);
  let windows = $state(false);
  let roof = $state("auto");
  let wear = $state(0.5);
  let houseReading = $state(false);
  const placingHouse = $derived(
    s.tool === "place" && s.placement?.kind === "house",
  );
  // a seed of its own each time: each one a little different; asked again while the level's
  // buildings are read
  async function placeHouse(once: boolean) {
    const level = styleLevel;
    const seed = Math.floor(Math.random() * 1e6);
    const f = kind === "wall" ? wallFloors : floors;
    for (let i = 0; i < 60; i++) {
      const r = await call<{ reading?: boolean } | null>(
        "start_structure_placement",
        { kind, level, floors: f, roof, wear, seed, windows, once },
      );
      houseReading = !!r?.reading;
      if (!houseReading || level !== styleLevel) return;
      await new Promise((ok) => setTimeout(ok, 500));
    }
    houseReading = false;
  }
  // a set of prefabs of the level's structures, saved with the user's
  let setMade = $state("");
  async function makeSet() {
    const level = styleLevel;
    setMade = "";
    houseReading = true;
    for (let i = 0; i < 60; i++) {
      const r = await call<{ reading?: boolean; paths?: string[] } | null>(
        "create_prefab_set",
        { level },
      );
      if (r?.paths) setMade = t("build.structures.set_made", r.paths.length);
      if (!r?.reading) break;
      await new Promise((ok) => setTimeout(ok, 500));
    }
    houseReading = false;
  }
  const snap = $derived(s.gizmo.snap);
  const placing = $derived(
    s.tool === "place" && s.placement?.kind === "shape"
      ? (s.placement?.name as string)
      : "",
  );

  const place = (preset: string, once: boolean) =>
    call("start_shape_placement", { preset, once });
  // the level chosen for the style (C++ m_style_level): the engine's, shared with the terrain brush
  const styleLevel = $derived((s.catalog?.style_level as string) ?? "");

  // ---- in the style of a level: its textures, their size, its light, its ground's bumps
  // (a value: the effect runs again when it changes, not at each state the engine sends)
  const libraryRoot = $derived(s.library_root as string);
  let library = $state<string[]>([]);
  $effect(() => {
    void libraryRoot;
    call<{ levels: { name: string; listed: boolean }[] }>(
      "list_levels",
      { jak2: false },
      true,
    ).then((r) => {
      if (r) library = r.levels.filter((l) => l.listed).map((l) => l.name);
    });
  });
  let style = $state<Style>(null);
  let asked = 0;
  // measured by the engine once the level's decor is read (a few seconds): asked again until then
  async function readStyle() {
    const level = styleLevel;
    const n = ++asked;
    const r = level ? await call<Style>("level_style", { level }, true) : null;
    if (n === asked) style = level ? (r ?? { reading: true }) : null;
  }
  $effect(() => {
    void styleLevel;
    readStyle();
  });
  onMount(() =>
    onEngine<any>(
      "jle:changed",
      (p) => style?.reading && (!p || p.state) && readStyle(),
    ),
  );

  // ---- the palette's pieces: the meshes of the selection (with what is under its elements)
  let rev = $state(0);
  onMount(() => viewport.store.subscribe(() => rev++));
  const pieces = $derived.by(() => {
    void [rev, s.selection.count, s.selection.primary];
    const ids: number[] = [];
    const st = viewport.store;
    for (const r of st.selectionRoots())
      for (const id of st.subtree(r)) {
        const n = st.nodes.get(id);
        if (n?.kind === NodeKind.Render && n.mesh) ids.push(id);
      }
    return ids;
  });
</script>

{#snippet choice(
  label: string,
  items: [string, string | number][],
  value: string | number,
  set: (v: string | number) => void,
)}
  <div class="flex flex-wrap items-center gap-1">
    <span class="w-14 text-neutral-400">{label}</span>
    {#each items as [text, v] (v)}
      <button
        class="rounded border px-1.5 py-0.5 text-xs {value === v
          ? 'border-orange-500 bg-orange-500/15'
          : 'border-[#2a2a2a] bg-[#0b0b0b] hover:border-[#3a3a3a]'}"
        onclick={() => set(v)}>{text}</button
      >
    {/each}
  </div>
{/snippet}

<div class="p-3 text-sm">
  {#if !s.document.open}
    <p class="text-neutral-300">{t("build.no_document")}</p>
  {:else}
    <!-- the shapes: a picture each, a click places it (click again in the view for another, Esc stops) -->
    <h3 class={heading}>{t("build.shapes")}</h3>
    <Tip guide="build" />
    <div class="grid grid-cols-[repeat(auto-fill,minmax(4.25rem,1fr))] gap-1.5">
      {#each shapePresets as p (p.name)}
        {@const name = t(`menu.create.${p.name}`)}
        <button
          class="flex flex-col items-center gap-1 rounded border p-1 {placing ===
          p.name
            ? 'border-orange-500 bg-orange-500/15'
            : 'border-[#2a2a2a] bg-[#0b0b0b] hover:border-[#3a3a3a] hover:bg-[#1a1a1a]'}"
          onclick={() => place(p.name, false)}
          onpointerdown={(e) => panelDrag(e, name, () => place(p.name, true))}
        >
          <ToolDemo shape={p.name} width={52} height={52} />
          <span class="w-full truncate text-center text-[11px] text-neutral-400"
            >{name}</span
          >
        </button>
        <Tip text={t("build.shape.tip", name, ...p.params.size)} />
      {/each}
    </div>
    <label class="mt-2 flex cursor-pointer items-center gap-2 self-start">
      <input
        type="checkbox"
        class={check}
        checked={snap.enabled}
        onchange={(e) =>
          call("set_gizmo", { snap: { enabled: e.currentTarget.checked } })}
      />
      {t("build.snap", snap.translate)}
    </label>
    <Tip text={t("build.snap.tip")} />

    <!-- floors and walls looking like a level's -->
    <h3 class={heading}>{t("build.style")}</h3>
    <Tip guide="style" />
    <Dropdown
      items={library.map((l) => ({ label: l, value: l }))}
      value={styleLevel}
      label={styleLevel || t("build.style.choose")}
      filter={t("build.style.filter")}
      onselect={(l) => call("level_style", { level: l })}
    />
    {#if style}
      <div class="mt-2 flex flex-col items-start gap-1.5">
        {#if style.reading || style.failed}
          <p class={muted}>
            {t(style.failed ? "build.style.failed" : "build.style.reading")}
          </p>
        {:else}
          {#if style.floor && style.floor.material >= 0}
            <p class="text-neutral-300">
              {t(
                "build.style.floor",
                style.floor.texture,
                style.floor.tile,
                style.bumps,
                style.bump_size,
              )}
            </p>
            <button
              class={button}
              onclick={() =>
                call("start_style_placement", {
                  level: styleLevel,
                  floor: true,
                })}>{t("build.style.place_floor")}</button
            >
            <Tip text={t("build.style.place_floor.tip")} />
          {/if}
          {#if style.wall && style.wall.material >= 0}
            <p class="text-neutral-300">
              {t("build.style.wall", style.wall.texture, style.wall.tile)}
            </p>
            <button
              class={button}
              onclick={() =>
                call("start_style_placement", {
                  level: styleLevel,
                  floor: false,
                })}>{t("build.style.place_wall")}</button
            >
            <Tip text={t("build.style.place_wall.tip")} />
          {/if}
          {#if !style.valid}
            <p class={muted}>{t("build.style.none")}</p>
          {:else}
            <button class={button} onclick={() => call("start_terrain_brush")}
              >{t("build.style.terrain")}</button
            >
            <Tip text={t("build.style.terrain.tip")} />
          {/if}
          {#if style.wall && style.wall.material >= 0}
            <!-- a wall, a floor or a house of this level's own pieces, solid, that grows -->
            <h3 class="{heading} mt-3">{t("build.structures")}</h3>
            <Tip guide="structures" />
            <p class={muted}>{t("build.structures.intro", styleLevel)}</p>
            {@render choice(
              t("build.structures.kind"),
              [
                [t("structure.kind.wall"), "wall"],
                [t("structure.kind.floor"), "floor"],
                [t("structure.kind.house"), "house"],
              ],
              kind,
              (v) => (kind = v as "wall" | "floor" | "house"),
            )}
            {#if kind === "house"}
              {@render choice(
                t("build.house.floors"),
                [["1", 1], ["2", 2], ["3", 3], ["4", 4]],
                floors,
                (v) => (floors = v as number),
              )}
              {@render choice(
                t("build.house.roof"),
                [
                  [t("build.house.roof.auto"), "auto"],
                  [t("build.house.roof.gable"), "gable"],
                  [t("build.house.roof.hip"), "hip"],
                  [t("build.house.roof.flat"), "flat"],
                ],
                roof,
                (v) => (roof = v as string),
              )}
            {:else if kind === "wall"}
              {@render choice(
                t("build.structures.height"),
                [
                  ["1", 1],
                  ["2", 2],
                  ["3", 3],
                ],
                wallFloors,
                (v) => (wallFloors = v as number),
              )}
              <label class="flex cursor-pointer items-center gap-2">
                <input type="checkbox" class={check} bind:checked={windows} />
                {t("structure.windows")}
              </label>
            {/if}
            {@render choice(
              t("build.house.wear"),
              [
                [t("build.house.wear.none"), 0],
                [t("build.house.wear.some"), 0.5],
                [t("build.house.wear.lots"), 1],
              ],
              wear,
              (v) => (wear = v as number),
            )}
            <button
              class="{button} {placingHouse ? 'border-orange-500' : ''}"
              disabled={houseReading}
              onclick={() => placeHouse(false)}
              onpointerdown={(e) =>
                panelDrag(e, t(`structure.kind.${kind}`), () => placeHouse(true))}
              >{t(
                houseReading
                  ? "build.house.reading"
                  : `build.structures.place.${kind}`,
              )}</button
            >
            <Tip text={t("build.structures.place.tip")} />
            <button class={button} disabled={houseReading} onclick={makeSet}
              >{t("build.structures.set")}</button
            >
            <Tip text={t("build.structures.set.tip", styleLevel)} />
            {#if setMade}<p class={muted}>{setMade}</p>{/if}
          {/if}
        {/if}
      </div>
    {/if}

    <!-- the palette: the textures Jak 3's levels use again and again, on what is selected -->
    <h3 class={heading}>{t("build.materials")}</h3>
    <Tip guide="palette" />
    <p class="mb-2 {muted}">
      {pieces.length
        ? t("build.materials.count", pieces.length)
        : t("build.materials.none")}
    </p>
    <!-- C++: the rest of the panel, at least 240 px -->
    <TextureBrowser ids={pieces} slot={-1} height={420} />
  {/if}
</div>
