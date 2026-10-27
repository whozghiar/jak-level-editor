<script lang="ts" module>
  // kept while the window is closed: the surface given, the sizes of a floor and a box
  const surface = $state({
    material: "stone",
    event: "none",
    mode: "ground",
    noedge: false,
    nocamera: false,
    noentity: false,
    nolineofsight: false,
  });
  const sizes = $state({ floor: [4, 0.5, 4], box: [2, 2, 2] });
</script>

<script lang="ts">
  // The collision tool's window, open while the tool is (editor/collision_tool.rs, as OpenRAC's
  // collision dock): the colors of the surfaces and their legend, the faces picked, the surface
  // they are given, deleted, moved (shown where they go while it is set), a floor or a box added
  // where the last click landed, the shape of the pieces selected made collision, the places where
  // Jak may fall through. Closed, the tool ends (lib/windows.svelte.ts).
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { collisionUi } from "../../render/collision_tool.svelte";
  import { button, check, heading, muted, small } from "../common/classes";
  import Tip from "../common/Tip.svelte";
  import Choice from "../inspector/Choice.svelte";
  import DragNumber from "../inspector/DragNumber.svelte";

  type Legend = { surface: string; faces: number; color: number[] };
  const c = $derived(engine.state.collision as Record<string, any> | null);
  let legend = $state<Legend[]>([]);
  // the legend comes with the triangles (collision_overlay): asked again when they change
  $effect(() => {
    void c?.key;
    void call("collision_overlay", {}).then((r: any) => {
      if (r?.legend) legend = r.legend;
    });
  });
  // the names of a surface as the inspector's In the game has them
  const prefix = $derived(
    c?.color_by === "effect"
      ? "decor.event."
      : c?.color_by === "kind"
        ? "decor.mode."
        : "decor.material.",
  );
  const swatch = (col: number[]) =>
    `rgb(${col.map((v) => Math.round(v * 255)).join(",")})`;
  const flags = ["noedge", "nocamera", "noentity", "nolineofsight"] as const;
  const picked = $derived((c?.picked ?? 0) as number);
  const moving = $derived(collisionUi.move.some((v) => v !== 0));

  function move() {
    void call("collision_move", { delta: [...collisionUi.move] }).then(() => {
      collisionUi.move = [0, 0, 0];
    });
  }
  const axes = ["x", "y", "z"] as const;
  const row = "flex items-center gap-2";
</script>

{#if c}
  <div class="flex flex-col gap-2 p-3 text-sm">
    <p class={muted}>{t("collision.help")}</p>

    <!-- the colors: by material, by what it does to Jak, by kind -->
    <h3 class={heading}>{t("collision.colors")}</h3>
    <div class="flex gap-3">
      {#each ["material", "effect", "kind"] as by}
        <label class="flex items-center gap-1">
          <input
            type="radio"
            name="collision-by"
            class={check}
            checked={c.color_by === by}
            onchange={() => call("collision_color_by", { by })}
          />
          {t(`collision.by.${by}`)}
        </label>
      {/each}
    </div>
    <div class="flex max-h-44 flex-col overflow-y-auto">
      {#each legend as l (l.surface)}
        <button
          class="flex items-center gap-2 rounded px-1 py-0.5 text-left hover:bg-white/5"
          onclick={(e) =>
            call("collision_select", { surface: l.surface, add: e.shiftKey })}
        >
          <span
            class="inline-block h-3 w-3 shrink-0 rounded-sm"
            style:background={swatch(l.color)}
          ></span>
          <span class="flex-1 truncate">{t(prefix + l.surface)}</span>
          <span class="text-neutral-500">{l.faces}</span>
        </button>
      {/each}
    </div>
    <Tip text={t("collision.legend.tip")} />

    <!-- the faces picked -->
    <h3 class={heading}>{t("collision.picked", picked)}</h3>
    {#if picked}
      <p class={muted}>
        {Object.entries(c.surfaces ?? {})
          .map(([k, n]) => `${t(prefix + k)} ${n}`)
          .join(", ")}
      </p>
    {/if}
    <div class="flex flex-wrap gap-1">
      <button
        class={small}
        disabled={!picked}
        onclick={() => call("collision_select", { none: true })}
        >{t("collision.none")}</button
      >
      <button
        class={small}
        disabled={!c.pieces}
        onclick={(e) =>
          call("collision_select", { pieces: true, add: e.shiftKey })}
        >{t("collision.under_pieces", c.pieces ?? 0)}</button
      >
    </div>

    <!-- the surface they are given (and a floor, a box, pieces made collision) -->
    <h3 class={heading}>{t("collision.surface")}</h3>
    <p>{t("decor.material")}</p>
    <Choice
      prefix="decor.material."
      value={surface.material}
      groups={[
        [
          "decor.material.group.sound",
          [
            "stone",
            "metal",
            "wood",
            "grass",
            "dirt",
            "sand",
            "snow",
            "deepsnow",
            "straw",
            "crwood",
            "pcmetal",
            "hotcoals",
            "lava",
            "tar",
            "swamp",
            "rotate",
            "neutral",
          ],
        ],
        [
          "decor.material.group.move",
          ["ice", "gravel", "quicksand", "tube", "waterbottom"],
        ],
        ["decor.material.group.special", ["stopproj"]],
      ]}
      onchange={(v) => (surface.material = v)}
    />
    <p>{t("decor.event")}</p>
    <Choice
      prefix="decor.event."
      value={surface.event}
      groups={[
        [null, ["none"]],
        ["decor.event.group.hurt", ["deadly", "burn", "deadlyup", "burnup"]],
        ["decor.event.group.death", ["endlessfall", "melt"]],
      ]}
      onchange={(v) => (surface.event = v)}
    />
    <p>{t("decor.mode")}</p>
    <Choice
      prefix="decor.mode."
      value={surface.mode}
      groups={[[null, ["ground", "wall", "obstacle"]]]}
      onchange={(v) => (surface.mode = v)}
    />
    <details>
      <summary class="cursor-pointer text-neutral-300"
        >{t("decor.more")}</summary
      >
      <div class="mt-1 flex flex-col gap-1 pl-3">
        {#each flags as key}
          <label class="flex items-center gap-2">
            <input type="checkbox" class={check} bind:checked={surface[key]} />
            {t(`decor.flag.${key}`)}
          </label>
        {/each}
      </div>
    </details>
    <div class="flex flex-wrap gap-1">
      <button
        class={button}
        disabled={!picked}
        onclick={() => call("collision_paint", { ...surface })}
        >{t("collision.paint")}</button
      >
      <button
        class={button}
        disabled={!picked}
        onclick={() => call("collision_delete")}>{t("collision.delete")}</button
      >
    </div>

    <!-- moved: shown where they go while it is set -->
    <h3 class={heading}>{t("collision.move")}</h3>
    <div class={row}>
      {#each axes as a, k}
        <DragNumber
          value={collisionUi.move[k]}
          step={0.05}
          min={-100}
          max={100}
          digits={2}
          unit={a}
          oninput={(v) => (collisionUi.move[k] = v)}
          onchange={(v) => (collisionUi.move[k] = v)}
        />
      {/each}
    </div>
    <div class="flex flex-wrap gap-1">
      <button class={button} disabled={!picked || !moving} onclick={move}
        >{t("collision.move.apply")}</button
      >
      <button
        class={small}
        disabled={!moving}
        onclick={() => (collisionUi.move = [0, 0, 0])}
        >{t("collision.move.reset")}</button
      >
    </div>

    <!-- added where the last click landed (the yellow cross), of the surface chosen -->
    <h3 class={heading}>{t("collision.add")}</h3>
    {#if !c.last_point}<p class={muted}>{t("collision.no_point")}</p>{/if}
    {#each ["floor", "box"] as const as shape}
      <div class={row}>
        {#each [0, 1, 2] as k}
          {#if shape === "box" || k !== 1}
            <DragNumber
              value={sizes[shape][k]}
              step={0.05}
              min={0.1}
              max={500}
              digits={1}
              unit={["w", "h", "d"][k]}
              onchange={(v) => (sizes[shape][k] = v)}
              oninput={(v) => (sizes[shape][k] = v)}
            />
          {/if}
        {/each}
        <button
          class={small}
          disabled={!c.last_point}
          onclick={() =>
            call("collision_add", { shape, size: sizes[shape], ...surface })}
          >{t(`collision.add.${shape}`)}</button
        >
      </div>
    {/each}
    <Tip text={t("collision.add.tip")} />

    <!-- the shape of the pieces selected (before the tool), faces to change one by one -->
    <button
      class={button}
      disabled={!c.pieces}
      onclick={() => call("collision_from_pieces", { ...surface })}
      >{t("collision.from_pieces", c.pieces ?? 0)}</button
    >
    <Tip text={t("collision.from_pieces.tip")} />

    <!-- where Jak meets more than the game gathers at once -->
    <button class={button} onclick={() => call("collision_check")}
      >{t("collision.check")}</button
    >
    {#if c.checked}
      <p class={muted}>
        {c.hot?.length
          ? t("collision.check.found", c.hot.length)
          : t("collision.check.none")}
      </p>
    {/if}
    <Tip text={t("collision.check.tip")} />
  </div>
{/if}
