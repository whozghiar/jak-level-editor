<script lang="ts">
  // What the game does with an element of the decor (ui_decor_behavior, behavior_panel.cpp): its
  // collision, what Jak walks on, what touching it does to him, a bounce, a launch. Every selected
  // piece of decor gets what is set here: the same on every one, one undo step for a change, one per
  // drag (when it ends). The engine's calls: decor_behavior, set_decor_behavior (behaviors.rs).
  import { t } from "../../lib/i18n";
  import Tip from "../common/Tip.svelte";
  import Choice from "./Choice.svelte";
  import DragNumber from "./DragNumber.svelte";
  import {
    check,
    commit,
    heading,
    live,
    muted,
    query,
    small,
  } from "./edit.svelte";

  type Collision = {
    set: boolean;
    kind: string;
    material: string;
    event: string;
    mode: string;
    noedge: boolean;
    nocamera: boolean;
    noentity: boolean;
    nolineofsight: boolean;
  };
  type Decor = {
    targets: number[];
    collision: Collision;
    bounce: { set: boolean; height: number };
    launch: {
      set: boolean;
      distance: number;
      height: number;
      yaw: number;
      time: number;
    };
    default: "game" | "see_through" | "small" | "shape";
  };

  let { id }: { id: number } = $props();
  const q = query<Decor>("decor_behavior", () => ({ id }));
  const d = $derived(q.value);

  // the same on every target; `drag`: shown at once, the undo step when the drag ends
  function write(drag = false) {
    if (!d) return;
    const args = {
      ids: d.targets,
      collision: d.collision.set ? d.collision : null,
      bounce: d.bounce.set ? d.bounce : null,
      launch: d.launch.set ? d.launch : null,
    };
    if (drag) live("set_decor_behavior", args);
    else commit("set_decor_behavior", args);
  }

  function setCollision(change: Partial<Collision>) {
    if (!d) return;
    Object.assign(d.collision, change, { set: true });
    write();
  }

  const flags = ["noedge", "nocamera", "noentity", "nolineofsight"] as const;
  const launchFields = [
    { k: "distance", step: 0.2, min: 0, max: 500, digits: 1, unit: "m" },
    { k: "height", step: 0.1, min: -200, max: 200, digits: 1, unit: "m" },
    { k: "yaw", step: 1, min: -180, max: 180, digits: 0, unit: "deg" },
    { k: "time", step: 0.02, min: 0.2, max: 10, digits: 2, unit: "s" },
  ] as const;
  const row = "flex items-center gap-2";
  const label =
    "w-32 shrink-0 text-xs leading-tight break-words text-neutral-400";
</script>

{#if d}
  {@const c = d.collision}
  <!-- its collision: yes or no; by default what the export gives it (the game's own, made from its
       shape, or none: a small piece, a surface seen through) -->
  {@const defaultOn = d.default === "game" || d.default === "shape"}
  {@const on = c.set ? c.kind !== "none" : defaultOn}
  <h3 class={heading}>{t("decor.title")}</h3>
  <Tip guide="ingame" />
  <div class="flex flex-col gap-1.5 text-sm">
    <label class="flex items-center gap-2">
      <input
        type="checkbox"
        class={check}
        checked={on}
        onchange={(e) =>
          setCollision({
            kind: e.currentTarget.checked
              ? c.kind === "invisible"
                ? "invisible"
                : "shape"
              : "none",
          })}
      />
      {t("decor.collision")}
    </label>
    <Tip guide="collision" />
    <p class="{muted} pl-5">
      {t(
        !c.set
          ? `decor.collision.${d.default}`
          : !on
            ? "decor.collision.off"
            : c.kind === "invisible"
              ? "decor.collision.invisible_on"
              : c.kind === "box"
                ? "decor.collision.box_on"
                : "decor.collision.on",
      )}
    </p>
    {#if on && c.kind !== "invisible"}
      <!-- its own shape, or the box around it (a detailed model Jak would fall through) -->
      <p>{t("decor.shape")}</p>
      <Tip text={t("decor.shape.tip")} />
      <Choice
        prefix="decor.shape."
        value={c.set && c.kind === "box" ? "box" : "shape"}
        groups={[[null, ["shape", "box"]]]}
        onchange={(kind) => setCollision({ kind })}
      />
    {/if}
    {#if on}
      <!-- what Jak walks on: the sound of his steps and their dust, and for a few how he moves -->
      <p>{t("decor.material")}</p>
      <Tip guide="surface" />
      <Choice
        prefix="decor.material."
        value={c.material}
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
        onchange={(material) => setCollision({ material })}
      />
      <!-- what touching it does to Jak -->
      <p>{t("decor.event")}</p>
      <Tip guide="event" />
      <Choice
        prefix="decor.event."
        value={c.event}
        groups={[
          [null, ["none"]],
          ["decor.event.group.hurt", ["deadly", "burn", "deadlyup", "burnup"]],
          ["decor.event.group.death", ["endlessfall", "melt"]],
        ]}
        onchange={(event) => setCollision({ event })}
      />
      <details>
        <summary class="cursor-pointer text-neutral-300"
          >{t("decor.more")}</summary
        >
        <div class="mt-1 flex flex-col gap-1.5 pl-3">
          <!-- not drawn in the game, it collides: an invisible wall -->
          <label class="flex items-center gap-2">
            <input
              type="checkbox"
              class={check}
              checked={c.set && c.kind === "invisible"}
              onchange={(e) =>
                setCollision({
                  kind: e.currentTarget.checked ? "invisible" : "shape",
                })}
            />
            {t("decor.invisible")}
          </label>
          <Tip text={t("decor.invisible.tip")} />
          <p>{t("decor.mode")}</p>
          <Choice
            prefix="decor.mode."
            value={c.mode}
            groups={[[null, ["auto", "ground", "wall", "obstacle"]]]}
            onchange={(mode) => setCollision({ mode })}
          />
          {#each flags as key}
            <label class="flex items-center gap-2">
              <input
                type="checkbox"
                class={check}
                checked={c[key]}
                onchange={(e) =>
                  setCollision({ [key]: e.currentTarget.checked })}
              />
              {t(`decor.flag.${key}`)}
            </label>
            <Tip text={t(`decor.flag.${key}.tip`)} />
          {/each}
        </div>
      </details>
    {/if}
    {#if c.set}
      <div>
        <button
          class={small}
          onclick={() => {
            d.collision = { ...c, set: false };
            write();
          }}
        >
          {t("decor.collision.reset")}
        </button>
      </div>
    {/if}

    <!-- what it does to Jak on top of it -->
    <label class="flex items-center gap-2">
      <input
        type="checkbox"
        class={check}
        checked={d.bounce.set}
        onchange={(e) => {
          d.bounce.set = e.currentTarget.checked;
          write();
        }}
      />
      {t("decor.bounce")}
    </label>
    <Tip guide="bounce" />
    {#if d.bounce.set}
      <div class={row}>
        <span class={label}>{t("decor.bounce.height")}</span>
        <DragNumber
          value={d.bounce.height}
          step={0.1}
          min={0.5}
          max={200}
          digits={1}
          unit="m"
          oninput={(h) => {
            d.bounce.height = h;
            write(true);
          }}
          onchange={(h) => {
            d.bounce.height = h;
            write();
          }}
        />
      </div>
    {/if}
    <label class="flex items-center gap-2">
      <input
        type="checkbox"
        class={check}
        checked={d.launch.set}
        onchange={(e) => {
          d.launch.set = e.currentTarget.checked;
          write();
        }}
      />
      {t("decor.launch")}
    </label>
    <Tip guide="launch" />
    {#if d.launch.set}
      <div class="flex flex-col gap-1">
        {#each launchFields as f}
          <div class={row}>
            <span class={label}>{t(`decor.launch.${f.k}`)}</span>
            <DragNumber
              value={d.launch[f.k]}
              step={f.step}
              min={f.min}
              max={f.max}
              digits={f.digits}
              unit={f.unit}
              oninput={(v) => {
                d.launch[f.k] = v;
                write(true);
              }}
              onchange={(v) => {
                d.launch[f.k] = v;
                write();
              }}
            />
          </div>
        {/each}
      </div>
      <Tip text={t("decor.launch.values.tip")} />
    {/if}
  </div>
{/if}
