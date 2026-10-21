<script lang="ts">
  // The inspector's sections of a surface (surface_panel.cpp): the size of a shape made in the editor,
  // the checks of a structure brought from Jak II, and the materials of any piece: an image file, or
  // a texture of the games (the Textures window). A change gives the pieces selected a new material:
  // the other pieces drawing the same material keep it. Their color is the game's (not edited: the
  // game's materials are drawn twice as bright, their vertex colors at half). The engine's calls:
  // shape, set_shape, materials, material_image, import_match_lighting (surfaces.rs).
  import { open } from "@tauri-apps/plugin-dialog";
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import Tip from "../common/Tip.svelte";
  import DragNumber from "./DragNumber.svelte";
  import {
    button,
    commit,
    field,
    heading,
    inspector,
    live,
    muted,
    query,
  } from "./edit.svelte";

  type Vec3 = [number, number, number];
  type Shape = {
    params: Record<string, any> & { kind: string; size: Vec3; tile: number };
    scale: Vec3;
  };
  type Materials = {
    targets: number[];
    slots: {
      material: number;
      texture: {
        index: number;
        name: string;
        width: number;
        height: number;
      } | null;
    }[];
    more: number;
  };

  let { node }: { node: any } = $props();
  const id = $derived(node.id as number);
  const render = $derived(node.kind === "render");
  const hasMesh = $derived(render && !!node.mesh);
  const shapeQ = query<Shape>("shape", () => (render ? { id } : null));
  const matQ = query<Materials>("materials", () => (hasMesh ? { id } : null));
  const shape = $derived(shapeQ.value);
  const p = $derived(shape?.params);

  // a value of the shape: shown at once while dragged, made again when let go (one undo step)
  function setShape(k: string, v: unknown, drag = false) {
    if (!p) return;
    p[k] = v;
    if (drag) live("set_shape", { ids: [id], params: { [k]: v } });
    else commit("set_shape", { ids: [id], params: { [k]: v } });
  }
  const sizeAt = (
    key: "size" | "opening",
    i: number,
    v: number,
    drag: boolean,
  ) => {
    const s = [...p![key]];
    s[i] = v;
    setShape(key, s, drag);
  };
  // a slider let go: one undo step (no `change` when let go at the value it had: committed on the
  // release then, else the inspector would stay "dragging")
  const release = (k: string, e: Event) =>
    inspector.dragging &&
    setShape(k, +(e.currentTarget as HTMLInputElement).value);
  const stretched = $derived(!!shape && shape.scale.some((s) => s !== 1));

  // a structure of Jak II brought into this Jak 3 level: what was checked and converted
  const imported = $derived(
    hasMesh &&
      node.extras?.og_import &&
      typeof node.extras.og_import === "object"
      ? node.extras.og_import
      : null,
  );
  function checkText(c: any): string | null {
    const n = (k: string, d = 0) => (typeof c[k] === "number" ? c[k] : d);
    switch (c.check) {
      case "mesh":
        return t(
          "import.check.mesh",
          n("triangles"),
          n("bad_indices"),
          n("no_uv"),
        );
      case "colors":
        return t("import.check.colors", n("without"), n("outside"));
      case "lighting": // lit where it stands: moot
        return node.extras?.og_lit_here
          ? null
          : t("import.check.lighting", n("ratio", 1));
      case "materials":
        return t("import.check.materials", n("count"), n("blended"));
      case "textures":
        return t("import.check.textures", n("count"), n("renamed"), n("odd"));
      case "metadata":
        return t("import.check.metadata");
    }
    return null;
  }

  async function image(slot: number) {
    const path = await open({
      filters: [
        {
          name: "Images (.png, .jpg, .tga, .bmp)",
          extensions: ["png", "jpg", "jpeg", "tga", "bmp"],
        },
      ],
      defaultPath: engine.state.projects_root || undefined,
    });
    if (typeof path === "string")
      commit("material_image", {
        ids: matQ.value?.targets ?? [id],
        slot,
        path,
      });
  }

  // the Textures window open for this slot of these pieces
  function texturesOpen(slot: number, targets: number[]): boolean {
    const w = windows.list.find((w) => w.id === "textures");
    return (
      !!w &&
      w.props.slot === slot &&
      JSON.stringify(w.props.ids) === JSON.stringify(targets)
    );
  }

  const row = "flex items-center gap-1.5";
  const label =
    "w-24 shrink-0 text-xs leading-tight break-words text-neutral-400";
</script>

{#if p}
  <h3 class={heading}>{t("inspector.shape")}</h3>
  <div class="flex flex-col gap-1 text-sm">
    <div class={row}>
      <span class={label}>{t("inspector.shape.size")}</span>
      {#each [0, 1, 2] as i}
        <DragNumber
          value={p.size[i]}
          step={0.05}
          min={0.05}
          max={2000}
          unit="m"
          oninput={(v) => sizeAt("size", i, v, true)}
          onchange={(v) => sizeAt("size", i, v, false)}
        />
      {/each}
    </div>
    <Tip text={t("inspector.shape.size.tip")} />
    <div class={row}>
      <span class={label}>{t("inspector.shape.tile")}</span>
      <DragNumber
        value={p.tile}
        step={0.05}
        min={0.1}
        max={100}
        unit="m"
        oninput={(v) => setShape("tile", v, true)}
        onchange={(v) => setShape("tile", v)}
      />
    </div>
    <Tip text={t("inspector.shape.tile.tip")} />
    {#each [{ kind: "stairs", k: "steps", key: "inspector.shape.steps", min: 1, max: 64 }, { kind: "cylinder", k: "sides", key: "inspector.shape.sides", min: 3, max: 64 }].filter((f) => f.kind === p.kind) as f}
      <div class={row}>
        <span class={label}>{t(f.key)}</span>
        <input
          type="range"
          class="flex-1 accent-orange-500"
          min={f.min}
          max={f.max}
          value={p[f.k]}
          oninput={(e) => setShape(f.k, +e.currentTarget.value, true)}
          onchange={(e) => release(f.k, e)}
          onpointerup={(e) => release(f.k, e)}
        />
        <span class="w-8 text-right tabular-nums">{p[f.k]}</span>
      </div>
    {/each}
    {#if p.kind === "doorway" || p.kind === "window"}
      <div class={row}>
        <span class={label}>{t("inspector.shape.opening")}</span>
        {#each [0, 1] as i}
          <DragNumber
            value={p.opening?.[i] ?? 0}
            step={0.05}
            min={0.1}
            max={2000}
            unit="m"
            oninput={(v) => sizeAt("opening", i, v, true)}
            onchange={(v) => sizeAt("opening", i, v, false)}
          />
        {/each}
      </div>
      <Tip text={t("inspector.shape.opening.tip")} />
    {/if}
    {#if p.kind === "ground"}
      <div class={row}>
        <span class={label}>{t("inspector.shape.bumps")}</span>
        <DragNumber
          value={p.bumps ?? 0}
          step={0.01}
          min={0}
          max={20}
          unit="m"
          oninput={(v) => setShape("bumps", v, true)}
          onchange={(v) => setShape("bumps", v)}
        />
      </div>
      <Tip text={t("inspector.shape.bumps.tip")} />
      <div class={row}>
        <span class={label}>{t("inspector.shape.bump_size")}</span>
        <DragNumber
          value={p.bump_size ?? 1}
          step={0.05}
          min={0.25}
          max={200}
          digits={1}
          unit="m"
          oninput={(v) => setShape("bump_size", v, true)}
          onchange={(v) => setShape("bump_size", v)}
        />
      </div>
      <Tip text={t("inspector.shape.bump_size.tip")} />
      <div class={row}>
        <span class={label}>{t("inspector.shape.seed")}</span>
        <input
          type="number"
          step="1"
          class="{field} w-full text-right"
          value={p.seed ?? 0}
          onchange={(e) => setShape("seed", Math.round(+e.currentTarget.value))}
        />
      </div>
      <Tip text={t("inspector.shape.seed.tip")} />
    {/if}
    {#if p.kind === "stairs"}
      {@const steps = Math.max(p.steps ?? 1, 1)}
      <p class={muted}>
        {t("inspector.shape.step", p.size[1] / steps, p.size[2] / steps)}
      </p>
    {/if}
    {#if p.kind === "box"}
      <div class={row}>
        <span class={label}>{t("inspector.shape.round")}</span>
        <DragNumber
          value={p.round ?? 0}
          step={0.02}
          min={0}
          max={Math.min(p.size[0], p.size[2]) / 2}
          unit="m"
          oninput={(v) => setShape("round", v, true)}
          onchange={(v) => setShape("round", v)}
        />
      </div>
      <Tip text={t("inspector.shape.round.tip")} />
    {/if}
    {#if p.kind !== "cylinder"}
      <div class={row}>
        <span class={label}>{t("inspector.shape.bend")}</span>
        <input
          type="range"
          class="flex-1 accent-orange-500"
          min={-360}
          max={360}
          step={1}
          value={p.bend ?? 0}
          oninput={(e) => setShape("bend", +e.currentTarget.value, true)}
          onchange={(e) => release("bend", e)}
          onpointerup={(e) => release("bend", e)}
        />
        <span class="w-14 text-right tabular-nums"
          >{(p.bend ?? 0).toFixed(0)} deg</span
        >
      </div>
      <Tip
        text={t(
          p.kind === "stairs" || p.kind === "ramp"
            ? "inspector.shape.bend.climb_tip"
            : "inspector.shape.bend.tip",
        )}
      />
    {/if}
    <!-- stretched with the scale tool: its texture too; the scale made its size keeps the texture's -->
    {#if stretched}
      <button
        class="{button} w-full"
        onclick={() =>
          commit("set_shape", { ids: [id], params: {}, apply_scale: true })}
      >
        {t("inspector.shape.apply_scale")}
      </button>
      <Tip text={t("inspector.shape.apply_scale.tip")} />
    {/if}
  </div>
{/if}

{#if imported}
  <h3 class={heading}>{t("import.title", imported.level ?? "")}</h3>
  <Tip text={t("import.tip")} />
  <div class="flex flex-col gap-1 text-sm">
    {#each Array.isArray(imported.checks) ? imported.checks : [] as c}
      {@const text = c && typeof c === "object" ? checkText(c) : null}
      {#if text}
        {@const good = c.ok ?? true}
        <p class="flex gap-2">
          <span
            class="shrink-0 font-semibold {good
              ? 'text-green-400'
              : 'text-orange-400'}">{good ? "ok" : "!"}</span
          >
          <span class="text-neutral-300">{text}</span>
        </p>
        {#if c.check === "lighting" && !good}
          <!-- its vertex colors scaled to the brightness of the document's Jak 3 decor -->
          <div>
            <button
              class={button}
              onclick={() => commit("import_match_lighting", { id })}
              >{t("import.match_lighting")}</button
            >
          </div>
          <Tip text={t("import.match_lighting.tip")} />
        {/if}
      {/if}
    {/each}
  </div>
{/if}

{#if hasMesh && matQ.value}
  {@const m = matQ.value}
  <h3 class={heading}>{t("inspector.material")}</h3>
  <!-- its light: the game's palettes of where it stands (done when it is placed or moved, or asked) -->
  <button class="{button} w-full" onclick={() => call("relight_selection")}
    >{t("inspector.relight")}</button
  >
  <Tip guide="relight" />
  <div class="mt-1.5 flex flex-col divide-y divide-white/10 text-sm">
    {#each m.slots as s, slot}
      {@const on = texturesOpen(slot, m.targets)}
      <div class="flex flex-col gap-1 py-1.5">
        {#if s.texture}
          <p class="break-all text-neutral-200">
            {s.texture.name} ({s.texture.width}x{s.texture.height})
          </p>
        {:else}
          <p class={muted}>{t("inspector.material.no_texture")}</p>
        {/if}
        <div class="flex gap-1.5">
          <span class="inline-flex"
            ><button class={button} onclick={() => image(slot)}
              >{t("inspector.material.image")}</button
            ></span
          >
          <Tip text={t("inspector.material.image.tip")} />
          <!-- a texture of the games: the window chooses it (a game, a level, a name) -->
          <span class="inline-flex">
            <button
              class="{button} {on
                ? 'border-orange-500 bg-orange-500/15 text-orange-300'
                : ''}"
              onclick={() => windows.open("textures", { ids: m.targets, slot })}
            >
              {t("inspector.material.textures")}
            </button>
          </span>
          <Tip text={t("inspector.material.textures.tip")} />
        </div>
      </div>
    {/each}
  </div>
  {#if m.more > 0}<p class={muted}>
      {t("inspector.material.more", m.more)}
    </p>{/if}
{/if}
