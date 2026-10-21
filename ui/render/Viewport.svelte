<script lang="ts">
  // The 3D view: a WebGL2 canvas filling its container, a 2D canvas over it for the gizmo and the
  // readouts. Everything else is `viewport` (viewport_api.ts).
  import { onMount } from "svelte";
  import { engine } from "../lib/engine.svelte";
  import { gameView } from "./game_view";
  import { effects } from "./particles";
  import { viewport } from "./viewport_api";

  let box: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let overlay: HTMLCanvasElement;

  onMount(() => {
    viewport
      .attach(canvas, overlay)
      .catch((e) => console.error("[viewport]", e));
    const offGame = gameView.attach();
    const offEffects = effects.attach();
    const fit = () => {
      const r = box.getBoundingClientRect();
      viewport.resize(r.width, r.height, window.devicePixelRatio || 1);
    };
    const ro = new ResizeObserver(fit);
    ro.observe(box);
    // another screen (another pixel ratio) without a resize
    const dpr = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    dpr.addEventListener("change", fit);
    return () => {
      ro.disconnect();
      dpr.removeEventListener("change", fit);
      offGame();
      offEffects();
      viewport.detach();
    };
  });

  // View > Game rendering (G), a document open; once the engine answered (not on the dev page)
  $effect(() => {
    if (engine.ready)
      gameView.setOn(
        !!engine.state.view.game_view && engine.state.document.open,
      );
  });

  // View > Effects, and the game view's hour (some effects run only at some hours)
  $effect(() => {
    if (!engine.ready) return;
    effects.hour = engine.state.weather?.mood?.hour ?? 12;
    effects.setOn(!!engine.state.view.effects);
  });

  // the View menu's toggles, the level's ocean (once per change of it), the hovered element's group
  let oceanKey = "";
  $effect(() => {
    if (!engine.ready) return;
    const v = engine.state.view;
    viewport.setSettings({
      show_grid: !!v.grid,
      show_actors: !!v.actors,
      show_ocean: !!v.ocean,
      textures: !!v.textures,
      fog: !!v.fog,
    });
    const ocean = engine.state.ocean ?? null;
    const key = JSON.stringify(ocean);
    if (key !== oceanKey) {
      oceanKey = key;
      viewport.setOcean(ocean);
    }
    viewport.outlineGroup = engine.state.hovered_group?.roots ?? [];
  });
</script>

<div class="viewport" bind:this={box}>
  <canvas bind:this={canvas} tabindex="-1"></canvas>
  <canvas class="overlay" bind:this={overlay}></canvas>
</div>

<style>
  .viewport {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    outline: none;
    display: block;
  }
  .overlay {
    pointer-events: none;
  }
</style>
