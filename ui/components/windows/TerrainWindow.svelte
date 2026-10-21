<script lang="ts" module>
  // the pictures of the levels' textures (the protocol's palette?...: u32 width, height, RGBA), read
  // once
  import { fetchBytes } from "../../render/engine";

  const pictures = new Map<string, Promise<ImageBitmap | null>>();
  function picture(
    background: string,
    name: string,
  ): Promise<ImageBitmap | null> {
    const key = `${background}|${name}`;
    let p = pictures.get(key);
    if (!p) {
      p = fetchBytes(
        `palette?background=${encodeURIComponent(background)}&name=${encodeURIComponent(name)}`,
      )
        .then((buf) => {
          const dv = new DataView(buf);
          const w = dv.getUint32(0, true),
            h = dv.getUint32(4, true);
          return w && h
            ? createImageBitmap(
                new ImageData(new Uint8ClampedArray(buf, 8, w * h * 4), w, h),
              )
            : null;
        })
        .catch(() => null);
      pictures.set(key, p);
    }
    return p;
  }
  type Pic = { background: string; name: string } | null | undefined;
  // a texture's picture in a canvas
  function thumb(canvas: HTMLCanvasElement, t: Pic) {
    let shown: Pic = undefined;
    const draw = async (t: Pic) => {
      shown = t;
      const ctx = canvas.getContext("2d")!;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      const img = t ? await picture(t.background, t.name) : null;
      if (img && shown === t)
        ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
    };
    void draw(t);
    return { update: (t: Pic) => void draw(t) };
  }
</script>

<script lang="ts">
  // The terrain brush's settings, open while the tool is (ui_terrain_window, terrain_tool.cpp): the
  // terrain painted, the level style painted with and its textures, the left button's modes, the
  // free or magnet brush, the size and strength, hills generated where the brush is. Closed, the
  // brush ends (lib/windows.svelte.ts).
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import {
    button,
    check,
    heading,
    input,
    muted,
    range,
    small,
  } from "../common/classes";
  import Tip from "../common/Tip.svelte";

  type Texture = { name: string; background: string; uses: number };
  const b = $derived(engine.state.brush as Record<string, any>);
  const set = (v: Record<string, unknown>) => call("set_brush", v, true);

  // the window's modes (raising and digging: the right button)
  const modes = ["paint", "wall", "erase", "smooth", "flatten"];

  // what the lists are read again for (primitives: not every state the engine sends, which
  // re-read them at each step of a stroke and closed the texture chooser)
  const libraryRoot = $derived(engine.state.library_root as string);
  const styleLevel = $derived((b.style_level as string) ?? "");
  const texturesReady = $derived(!!b.textures_ready);

  // the level styles: the listed levels of the library
  let levels = $state<string[]>([]);
  let filter = $state("");
  $effect(() => {
    void libraryRoot;
    call<{ levels: { name: string; listed: boolean }[] }>(
      "list_levels",
      {},
      true,
    ).then(
      (r) =>
        (levels = (r?.levels ?? []).filter((l) => l.listed).map((l) => l.name)),
    );
  });
  const shown = $derived(
    levels.filter((l) => !filter || l.includes(filter.toLowerCase())),
  );

  // the textures of the style's level (the most used first), once the library's index is ready
  let textures = $state<Texture[]>([]);
  $effect(() => {
    const level = styleLevel;
    void texturesReady;
    if (!level) return void (textures = []);
    call<{ textures: Texture[] }>("level_textures", { level }, true).then(
      (r) => (textures = r?.textures ?? []),
    );
  });
  const named = (name: string | null | undefined) =>
    name ? textures.find((x) => x.name === name) : undefined;

  // the texture chooser open: which ("floor" | "wall"), its filter
  let choosing = $state<"" | "floor" | "wall">("");
  let texFilter = $state("");
  const choices = $derived(
    textures.filter(
      (x) => !texFilter || x.name.includes(texFilter.toLowerCase()),
    ),
  );

  const style = $derived(
    b.style as {
      status: string;
      floor?: string | null;
      wall?: string | null;
    } | null,
  );
  const log = (v: number) => Math.log(v);
</script>

{#snippet slider(
  key: string,
  value: number,
  lo: number,
  hi: number,
  unit: string,
  digits: number,
  logarithmic: boolean,
  field: string,
  tip?: string,
)}
  <label class="flex items-center gap-2 text-xs">
    <input
      type="range"
      class={range}
      min={logarithmic ? log(lo) : lo}
      max={logarithmic ? log(hi) : hi}
      step={logarithmic ? 0.01 : (hi - lo) / 200}
      value={logarithmic ? log(value) : value}
      oninput={(e) => {
        const v = +(e.currentTarget as HTMLInputElement).value;
        set({ [field]: logarithmic ? Math.exp(v) : v });
      }}
    />
    <span class="w-32 shrink-0 text-neutral-300"
      >{value.toFixed(digits)}{unit} {t(key)}</span
    >
  </label>
  {#if tip}<Tip text={t(tip)} placement="left" />{/if}
{/snippet}

{#snippet texture(
  key: string,
  field: "floor" | "wall",
  chosen: string,
  styleTexture: string | null | undefined,
)}
  {@const current = named(chosen || styleTexture)}
  <button
    class="flex items-center gap-2 text-left"
    onclick={() => (
      (choosing = choosing === field ? "" : field),
      (texFilter = "")
    )}
  >
    <canvas
      class="size-10 shrink-0 rounded border border-[#2a2a2a] bg-black"
      width="40"
      height="40"
      use:thumb={current}
    ></canvas>
    <span class="flex flex-col">
      <span class="text-xs text-neutral-200">{t(key)}</span>
      <span class={muted}>{chosen || t("terrain.texture.style")}</span>
    </span>
  </button>
  <Tip text={t("terrain.texture.tip")} placement="left" />
  {#if choosing === field}
    <div
      class="flex flex-col gap-1 rounded border border-[#2a2a2a] bg-[#111] p-2"
    >
      <input
        class={input}
        placeholder={t("textures.filter")}
        bind:value={texFilter}
      />
      <button
        class="{small} self-start"
        class:border-orange-600={!chosen}
        onclick={() => (set({ [`${field}_texture`]: "" }), (choosing = ""))}
      >
        {t("terrain.texture.style")}
      </button>
      {#if !textures.length}<p class={muted}>
          {t("terrain.texture.none")}
        </p>{/if}
      <div class="grid max-h-80 grid-cols-3 gap-1.5 overflow-y-auto">
        {#each choices as x (x.name)}
          <button
            aria-label={x.name}
            class="rounded border p-0.5 {x.name === (chosen || styleTexture)
              ? 'border-orange-500'
              : 'border-transparent hover:border-[#3a3a3a]'}"
            onclick={() => (
              set({ [`${field}_texture`]: x.name }),
              (choosing = "")
            )}
          >
            <canvas class="block size-16" width="64" height="64" use:thumb={x}
            ></canvas>
          </button>
          <Tip
            text={`${x.name}\n${t("textures.used_level", x.uses)}`}
            placement="left"
          />
        {/each}
      </div>
    </div>
  {/if}
{/snippet}

<div class="flex flex-col gap-1.5 text-sm">
  <!-- how to use it: the mouse's buttons and keys -->
  <div class="flex items-start gap-1.5">
    <span class="cursor-help text-neutral-500">(?)</span>
    <Tip text={t("terrain.help")} placement="left" />
    <p class={muted}>
      {b.painting ? t("terrain.painting", b.painting) : t("terrain.new")}
    </p>
  </div>
  {#if b.node}
    <button class="{small} self-start" onclick={() => call("terrain_start_new")}
      >{t("terrain.start_new")}</button
    >
    <Tip text={t("terrain.start_new.tip")} placement="left" />
  {/if}

  <!-- the level style painted with: the ground takes its floor texture, the walls and slopes its wall texture -->
  <h3 class={heading}>{t("terrain.style.title")}</h3>
  <input
    class={input}
    placeholder={t("build.style.filter")}
    bind:value={filter}
  />
  <select
    class={input}
    value={b.style_level ?? ""}
    onchange={(e) =>
      set({ style_level: (e.currentTarget as HTMLSelectElement).value })}
  >
    {#if !filter}<option value="">{t("terrain.style.grid")}</option>{/if}
    {#if b.style_level && !shown.includes(b.style_level)}<option
        value={b.style_level}>{b.style_level}</option
      >{/if}
    {#each shown as l (l)}<option value={l}>{l}</option>{/each}
  </select>
  <Tip text={t("terrain.style.tip")} placement="left" />
  {#if b.style_level}
    {@render texture(
      "terrain.texture.floor",
      "floor",
      b.floor_texture,
      style?.floor,
    )}
    {@render texture(
      "terrain.texture.wall",
      "wall",
      b.wall_texture,
      style?.wall ?? style?.floor,
    )}
    {#if style?.status === "ready"}
      <p class={muted}>
        {t(
          "terrain.style.textures",
          style.floor ?? "-",
          style.wall ?? style.floor ?? "-",
        )}
      </p>
    {:else}
      <p class={muted}>
        {t(
          style?.status === "failed"
            ? "build.style.failed"
            : "build.style.reading",
        )}
      </p>
    {/if}
  {/if}
  {#if b.node}
    <button
      class="{small} self-start"
      onclick={() => call("terrain_apply_style")}
      >{t("terrain.style.apply")}</button
    >
    <Tip text={t("terrain.style.apply.tip")} placement="left" />
  {/if}

  <!-- the modes, two per line; the right button raises and digs in all of them -->
  <h3 class={heading}>{t("terrain.left_button")}</h3>
  <div class="grid grid-cols-2 gap-x-2 gap-y-1">
    {#each modes as m (m)}
      <label class="flex items-center gap-1.5">
        <input
          type="radio"
          class={check}
          checked={b.mode === m}
          onchange={() => set({ mode: m })}
        />
        {t(`terrain.mode.${m}`)}
      </label>
      <Tip text={t(`terrain.mode.${m}.tip`)} placement="left" />
    {/each}
  </div>
  <div class="grid grid-cols-2 gap-x-2">
    <label class="flex items-center gap-1.5">
      <input
        type="radio"
        class={check}
        checked={!b.magnet}
        onchange={() => set({ magnet: false })}
      />
      {t("terrain.free")}
    </label>
    <Tip text={t("terrain.free.tip")} placement="left" />
    <label class="flex items-center gap-1.5">
      <input
        type="radio"
        class={check}
        checked={!!b.magnet}
        onchange={() => set({ magnet: true })}
      />
      {t("terrain.magnet")}
    </label>
    <Tip text={t("terrain.magnet.tip")} placement="left" />
  </div>
  <p class={muted}>{t("terrain.right_button")}</p>
  {@render slider(
    "terrain.size",
    b.size ?? 4,
    0.5,
    64,
    " m",
    1,
    true,
    "size",
    "terrain.size.tip",
  )}
  {#if b.mode === "smooth" || b.mode === "flatten"}
    {@render slider(
      "terrain.strength",
      b.strength ?? 3,
      0.1,
      20,
      "",
      1,
      true,
      "strength",
      "terrain.strength.tip",
    )}
  {/if}
  {#if b.mode === "wall"}
    {@render slider(
      "terrain.wall_height",
      b.wall_height ?? 4,
      0.5,
      30,
      " m",
      1,
      false,
      "wall_height",
    )}
  {/if}
  {#if b.mode === "paint"}
    <label class="flex items-center gap-1.5 text-xs">
      <input
        type="checkbox"
        class={check}
        checked={!!b.relief}
        onchange={(e) =>
          set({ relief: (e.currentTarget as HTMLInputElement).checked })}
      />
      {t("terrain.relief")}
    </label>
    <Tip text={t("terrain.relief.tip")} placement="left" />
  {/if}
  {#if !b.node}
    {@render slider(
      "terrain.cell",
      b.cell ?? 1,
      0.5,
      4,
      " m",
      1,
      false,
      "cell",
      "terrain.cell.tip",
    )}
  {/if}

  <!-- hills made at once (a fractal noise) where the brush is -->
  <details class="mt-1">
    <summary class="cursor-pointer text-xs font-semibold text-neutral-300"
      >{t("terrain.generate")}</summary
    >
    <div class="mt-1.5 flex flex-col gap-1.5">
      <div class="flex items-center gap-2 text-xs">
        {#each [0, 1] as k (k)}
          <input
            type="number"
            class={input}
            min="8"
            max="512"
            step="1"
            value={b.gen_size?.[k] ?? 64}
            onchange={(e) => {
              const g = [...(b.gen_size ?? [64, 64])];
              g[k] = +(e.currentTarget as HTMLInputElement).value;
              set({ gen_size: g });
            }}
          />
        {/each}
        <span class="shrink-0 text-neutral-300"
          >m {t("terrain.generate.size")}</span
        >
      </div>
      {@render slider(
        "terrain.generate.hills",
        b.hills ?? 12,
        0,
        80,
        " m",
        1,
        false,
        "hills",
      )}
      {@render slider(
        "terrain.generate.roughness",
        b.roughness ?? 0.5,
        0,
        1,
        "",
        2,
        false,
        "roughness",
      )}
      <label class="flex items-center gap-2 text-xs">
        <input
          type="number"
          class={input}
          step="1"
          value={b.seed ?? 1}
          onchange={(e) =>
            set({
              seed: Math.round(+(e.currentTarget as HTMLInputElement).value),
            })}
        />
        <span class="shrink-0 text-neutral-300"
          >{t("terrain.generate.seed")}</span
        >
      </label>
      <button class="{button} w-full" onclick={() => call("terrain_generate")}
        >{t("terrain.generate.go")}</button
      >
      <Tip text={t("terrain.generate.tip")} placement="left" />
    </div>
  </details>
</div>
