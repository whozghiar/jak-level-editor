<script lang="ts" module>
  // the animation chosen, kept from one element to the next (App::m_preview_choice)
  let choice = $state(0);
</script>

<script lang="ts">
  // The inspector's preview of an actor's or a model's animation or effect (ui_preview_inspector,
  // preview.cpp): its animations, the movement its code gives it, broken, its picture's effect;
  // played in the view (ui/render/preview.svelte.ts), the document left as it is. Mounted by the
  // inspector for an actor or a model: <PreviewSection id={n.id} />
  import { onMount } from "svelte";
  import { t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import {
    previewState,
    startPreview,
    stopPreview,
  } from "../../render/preview.svelte";
  import { button, check, heading, input } from "../common/classes";
  import Tip from "../common/Tip.svelte";

  let { id }: { id: number; node?: unknown } = $props();

  type Options = {
    section: boolean;
    any: boolean;
    icon: boolean;
    motion: string;
    break: boolean;
    animations: { index: number; name: string; label: string }[];
    playing: boolean;
    loop: boolean;
  };
  let o = $state<Options | null>(null);

  const read = async (node: number) =>
    (o = (await call<Options>("preview_options", { id: node }, true)) ?? null);
  $effect(() => {
    void previewState.node;
    void read(id);
  });
  onMount(() => onEngine("jle:changed", (p) => (!p || p.scene) && read(id)));

  const playing = $derived(!!o?.playing && previewState.node !== 0);
  $effect(() => {
    if (o && o.animations.length)
      choice = Math.min(Math.max(choice, 0), o.animations.length - 1);
  });
  const play = (what: string) =>
    startPreview({ id, what, choice, loop: o?.loop ?? true });
</script>

{#if o?.section}
  <section class="flex flex-col gap-1.5 text-sm">
    <h3 class={heading}>{t("preview.title")}</h3>
    <Tip guide="preview" placement="left" />
    {#if !o.any}
      <p class="text-xs text-neutral-400">{t("preview.none")}</p>
    {:else}
      {#if o.icon}
        <p class="text-xs text-neutral-400">{t("preview.no_model")}</p>
      {/if}
      {#if o.animations.length}
        <select
          class={input}
          value={choice}
          onchange={(e) => {
            choice = +(e.currentTarget as HTMLSelectElement).value;
            if (playing) play("animation");
          }}
        >
          {#each o.animations as a, i (a.index)}
            <option value={i}>{a.label}</option>
          {/each}
        </select>
      {/if}
      <div class="flex flex-wrap items-center gap-2">
        {#if o.icon}
          <button class={button} onclick={() => play("effect")}
            >{t("preview.effect")}</button
          >
          <Tip text={t(`preview.motion.${o.motion}`)} placement="left" />
        {/if}
        {#if o.animations.length}
          <button class={button} onclick={() => play("animation")}
            >{t("preview.play")}</button
          >
          <Tip text={t("preview.play.tip")} placement="left" />
        {/if}
        {#if o.motion && !o.icon}
          <button class={button} onclick={() => play("motion")}
            >{t("preview.motion")}</button
          >
          <Tip text={t(`preview.motion.${o.motion}`)} placement="left" />
        {/if}
        {#if o.break}
          <button class={button} onclick={() => play("break")}
            >{t("preview.break")}</button
          >
          <Tip text={t("preview.break.tip")} placement="left" />
        {/if}
        <button class={button} disabled={!playing} onclick={() => stopPreview()}
          >{t("preview.stop")}</button
        >
        <label class="flex items-center gap-1.5">
          <input
            type="checkbox"
            class={check}
            checked={o.loop}
            onchange={async (e) => {
              const loop = (e.currentTarget as HTMLInputElement).checked;
              o =
                (await call<Options>("preview_options", { id, loop }, true)) ??
                o;
            }}
          />
          {t("preview.loop")}
        </label>
      </div>
    {/if}
  </section>
{/if}
