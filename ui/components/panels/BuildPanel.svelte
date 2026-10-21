<script lang="ts" module>
  // the level chosen for the style (C++ m_style_level; the engine keeps it too, for the terrain brush)
  let styleLevel = $state("");
</script>

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
  type Style = { reading?: boolean; failed?: boolean; floor?: Surface; wall?: Surface; bumps?: number; bump_size?: number; valid?: boolean } | null;

  const s = $derived(engine.state);
  const snap = $derived(s.gizmo.snap);
  const placing = $derived(s.tool === "place" && s.placement?.kind === "shape" ? (s.placement?.name as string) : "");

  const place = (preset: string, once: boolean) => call("start_shape_placement", { preset, once });

  // ---- in the style of a level: its textures, their size, its light, its ground's bumps
  let library = $state<string[]>([]);
  $effect(() => {
    void s.library_root;
    call<{ levels: { name: string; listed: boolean }[] }>("list_levels", { jak2: false }, true).then((r) => {
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
  onMount(() => onEngine<any>("jle:changed", (p) => style?.reading && (!p || p.state) && readStyle()));

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
          class="flex flex-col items-center gap-1 rounded border p-1 {placing === p.name
            ? 'border-orange-500 bg-orange-500/15'
            : 'border-[#2a2a2a] bg-[#0b0b0b] hover:border-[#3a3a3a] hover:bg-[#1a1a1a]'}"
          onclick={() => place(p.name, false)}
          onpointerdown={(e) => panelDrag(e, () => place(p.name, true))}
        >
          <ToolDemo shape={p.name} width={52} height={52} />
          <span class="w-full truncate text-center text-[11px] text-neutral-400">{name}</span>
        </button>
        <Tip text={t("build.shape.tip", name, ...p.params.size)} />
      {/each}
    </div>
    <label class="mt-2 flex cursor-pointer items-center gap-2 self-start">
      <input type="checkbox" class={check} checked={snap.enabled} onchange={(e) => call("set_gizmo", { snap: { enabled: e.currentTarget.checked } })} />
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
      onselect={(l) => (styleLevel = l)}
    />
    {#if style}
      <div class="mt-2 flex flex-col items-start gap-1.5">
        {#if style.reading || style.failed}
          <p class={muted}>{t(style.failed ? "build.style.failed" : "build.style.reading")}</p>
        {:else}
          {#if style.floor && style.floor.material >= 0}
            <p class="text-neutral-300">{t("build.style.floor", style.floor.texture, style.floor.tile, style.bumps, style.bump_size)}</p>
            <button class={button} onclick={() => call("start_style_placement", { level: styleLevel, floor: true })}>{t("build.style.place_floor")}</button>
            <Tip text={t("build.style.place_floor.tip")} />
          {/if}
          {#if style.wall && style.wall.material >= 0}
            <p class="text-neutral-300">{t("build.style.wall", style.wall.texture, style.wall.tile)}</p>
            <button class={button} onclick={() => call("start_style_placement", { level: styleLevel, floor: false })}>{t("build.style.place_wall")}</button>
            <Tip text={t("build.style.place_wall.tip")} />
          {/if}
          {#if !style.valid}
            <p class={muted}>{t("build.style.none")}</p>
          {:else}
            <button class={button} onclick={() => call("start_terrain_brush")}>{t("build.style.terrain")}</button>
            <Tip text={t("build.style.terrain.tip")} />
          {/if}
        {/if}
      </div>
    {/if}

    <!-- the palette: the textures Jak 3's levels use again and again, on what is selected -->
    <h3 class={heading}>{t("build.materials")}</h3>
    <Tip guide="palette" />
    <p class="mb-2 {muted}">{pieces.length ? t("build.materials.count", pieces.length) : t("build.materials.none")}</p>
    <!-- C++: the rest of the panel, at least 240 px -->
    <TextureBrowser ids={pieces} slot={-1} height={420} />
  {/if}
</div>
