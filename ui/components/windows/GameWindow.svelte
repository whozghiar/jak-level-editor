<script lang="ts">
  // The Game rendering window (View > Sky, hour and weather; ui_game_window, game_window.cpp): the game
  // view's settings apart from the menus, a half turn of its sky to see the sun, the clouds and the
  // colors of the hour, the hour and the weather as the game has them (crates/core/src/game/mood.rs),
  // and what they do to the level: its light, its fog, rain, lightning, the effects at night.
  import { onMount } from "svelte";
  import { engine } from "../../lib/engine.svelte";
  import { printf, t } from "../../lib/i18n";
  import { call, onEngine } from "../../lib/rpc";
  import { skyPreview, type GameMood } from "../../render/sky";
  import { viewport } from "../../render/viewport_api";
  import { button, check, heading, muted, range } from "../common/classes";
  import Dropdown from "../common/Dropdown.svelte";
  import Tip from "../common/Tip.svelte";

  type Preset = { id: string; cloud: number; fog: number; rain: number };

  const s = $derived(engine.state);
  const w = $derived(s.weather ?? { mood: {}, time_passes: false, time_speed: 20, lightning: false, preset: null });
  const m = $derived(w.mood ?? {});
  const hour = $derived<number>(m.hour ?? 12);

  const setWeather = (args: Record<string, unknown>) => call("set_weather", args);
  const setMood = (k: string, v: number | boolean) => setWeather({ mood: { [k]: v } });

  let presets = $state<Preset[]>([]);
  let mood = $state<GameMood>();
  let queued = false;
  const readMood = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(async () => {
      queued = false;
      mood = (await call<GameMood>("game_mood", {}, true)) ?? mood;
    });
  };

  // the sky: a half turn around where the camera looks (the whole turn loses the sense of where
  // things are), drawn each frame (the clouds drift, the camera turns)
  let canvas = $state<HTMLCanvasElement>();
  let width = $state(0);
  let yaw = $state(0);
  const height = $derived(Math.round(width * 0.36));
  onMount(() => {
    call<Preset[]>("weather_presets", {}, true).then((p) => (presets = p ?? []));
    readMood();
    const off = onEngine<any>("jle:changed", (p) => (!p || p.gameview || p.state || p.document) && readMood());
    let frame = requestAnimationFrame(function draw(now) {
      yaw = viewport.camera.yaw;
      if (canvas && mood && height > 0) {
        const dpr = window.devicePixelRatio || 1;
        const [cw, ch] = [Math.round(width * dpr), Math.round(height * dpr)];
        if (canvas.width !== cw || canvas.height !== ch) [canvas.width, canvas.height] = [cw, ch];
        skyPreview(canvas, mood, yaw, now / 1000);
      }
      frame = requestAnimationFrame(draw);
    });
    return () => {
      off();
      cancelAnimationFrame(frame);
    };
  });

  // the directions over it: yaw 0 looks to -z (north), yaw grows to the left (west)
  const directions = $derived(
    ["N", "W", "S", "E"]
      .map((name, k) => {
        let a = k * (Math.PI / 2) - yaw;
        a -= 2 * Math.PI * Math.round(a / (2 * Math.PI)); // std::remainder
        return { name, a };
      })
      .filter((d) => Math.abs(d.a) <= Math.PI / 2),
  );
  // -10..70 degrees, top to bottom
  const horizon = 70 / 80;

  const hours = [
    { key: "game.dawn", hour: 6.5 },
    { key: "game.noon", hour: 12 },
    { key: "game.dusk", hour: 18.5 },
    { key: "game.night", hour: 22 },
  ];
  const clock = $derived(printf("%02d:%02d", [Math.floor(hour) % 24, Math.floor((hour - Math.floor(hour)) * 60)]));
  // the speed's slider, logarithmic from 1 to 240 minutes per second
  const toSpeed = (x: number) => Math.exp(x * Math.log(240));
  const fromSpeed = (v: number) => Math.log(Math.max(v, 1)) / Math.log(240);

  const current = $derived(presets.findIndex((p) => p.id === w.preset));
  const sliders = [
    { k: "cloud", key: "game.clouds", tip: "game.clouds_tip" },
    { k: "fog", key: "game.fog", tip: "game.fog_tip" },
    { k: "rain", key: "game.rain", tip: "game.rain_tip" },
  ];
</script>

{#if !s.view.game_view_ready}
  <p class="text-neutral-300">{t("menu.view.game_view.unavailable", s.view.game_view_error)}</p>
{:else}
  {#snippet checkbox(label: string, on: boolean, run: (on: boolean) => void, tip: string)}
    <label class="flex cursor-pointer items-center gap-2 self-start">
      <input type="checkbox" class={check} checked={on} onchange={(e) => run(e.currentTarget.checked)} />
      {label}
    </label>
    <Tip text={tip} />
  {/snippet}

  <div class="flex flex-col gap-2">
    {@render checkbox(t("game.on"), !!s.view.game_view, (game_view) => call("set_view", { game_view }), t("menu.view.game_view.tip"))}

    <!-- the sky, the horizon, the four directions -->
    <div class="relative overflow-hidden rounded bg-black" bind:clientWidth={width} style:height="{height}px">
      <canvas bind:this={canvas} class="absolute inset-0 size-full"></canvas>
      <div class="absolute inset-x-0 h-px bg-white/30" style:top="{horizon * 100}%"></div>
      {#each directions as d (d.name)}
        <span class="absolute text-xs text-white/80" style:left="calc({(0.5 - d.a / Math.PI) * 100}% - 4px)" style:top="calc({horizon * 100}% + 2px)">
          {d.name}
        </span>
      {/each}
      <span class="absolute top-0.5 left-1/2 -translate-x-1/2 text-xs text-white/60">v</span>
    </div>
    <p class={muted}>{t("game.sky_tip")}</p>

    <!-- the hour -->
    <h3 class={heading}>{t("game.hour")}</h3>
    <div class="flex items-center gap-2">
      <input type="range" class={range} min="0" max="24" step="0.01" value={hour} oninput={(e) => setMood("hour", +e.currentTarget.value)} />
      <span class="w-11 shrink-0 text-right tabular-nums">{clock}</span>
    </div>
    <Tip text={t("game.hour_tip")} />
    <div class="flex gap-1">
      {#each hours as h}
        <button class={button} onclick={() => setMood("hour", h.hour)}>{t(h.key)}</button>
      {/each}
    </div>
    <div class="flex items-center gap-3">
      {@render checkbox(t("game.time_passes"), !!w.time_passes, (time_passes) => setWeather({ time_passes }), t("game.time_passes_tip"))}
      {#if w.time_passes}
        <input
          type="range"
          class={range}
          min="0"
          max="1"
          step="0.001"
          value={fromSpeed(w.time_speed)}
          oninput={(e) => setWeather({ time_speed: toSpeed(+e.currentTarget.value) })}
        />
        <span class="shrink-0 text-xs text-neutral-400">{t("game.speed", w.time_speed)}</span>
      {/if}
    </div>

    <!-- the weather -->
    <h3 class={heading}>{t("game.weather")}</h3>
    <Dropdown
      items={presets.map((p) => ({ label: t(`weather.${p.id}`), value: p.id, tip: t(`weather.${p.id}.tip`) }))}
      value={w.preset ?? ""}
      label={current >= 0 ? t(`weather.${presets[current].id}`) : t("weather.custom")}
      onselect={(preset) => setWeather({ preset })}
      tip={t("game.weather_tip")}
    />
    {#each sliders as sl}
      <div class="flex items-center gap-2">
        <input type="range" class={range} min="0" max="1" step="0.01" value={m[sl.k] ?? 0} oninput={(e) => setMood(sl.k, +e.currentTarget.value)} />
        <span class="w-10 shrink-0 text-right tabular-nums">{(m[sl.k] ?? 0).toFixed(2)}</span>
        <span class="w-28 shrink-0 text-neutral-400">{t(sl.key)}</span>
      </div>
      <Tip text={t(sl.tip)} />
    {/each}
    {@render checkbox(t("game.lightning"), !!w.lightning, (lightning) => setWeather({ lightning }), t("game.lightning_tip"))}
    {@render checkbox(t("menu.view.game_lights"), !!m.level_lights, (on) => setMood("level_lights", on), t("menu.view.game_lights.tip"))}
    {@render checkbox(t("menu.view.effects"), !!s.view.effects, (effects) => call("set_view", { effects }), t("game.effects_tip"))}

    <!-- what the game does with it, in words -->
    {#if mood}
      <h3 class={heading}>{t("game.now")}</h3>
      <p class="text-neutral-300">
        {t(
          "game.now_text",
          t(mood.sun_dir[1] > 0 ? "game.sun_up" : "game.sun_down"),
          Math.max(...mood.light_color[0], 0),
          mood.fog_end / 4096,
          t(m.level_lights && mood.times[5][3] > 0 ? "game.lamps_on" : "game.lamps_off"),
        )}
      </p>
    {/if}
  </div>
{/if}
