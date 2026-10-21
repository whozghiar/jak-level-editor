<script lang="ts">
  // The 3D view: a WebGL2 canvas filling its container, a 2D canvas over it for the gizmo and the
  // readouts. Everything else is `viewport` (viewport_api.ts).
  import { onMount } from "svelte";
  import { viewport } from "./viewport_api";

  let box: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let overlay: HTMLCanvasElement;

  onMount(() => {
    viewport
      .attach(canvas, overlay)
      .catch((e) => console.error("[viewport]", e));
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
      viewport.detach();
    };
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
