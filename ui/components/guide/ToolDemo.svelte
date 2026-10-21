<script lang="ts">
  // A tool's animation (render/tool_demo: demos.ts) on a canvas, or a shape's picture.
  //   name     a demo of demoNames ("move", "place"...): animated, looping, drawn only while on
  //            screen; an unknown name leaves the canvas empty
  //   shape    instead: a shape preset's name ("box", "roof-hip"... of shapePresets) or its
  //            ShapeParams, seen from above at three quarters, drawn once (the Build panel's
  //            buttons: <ToolDemo shape="stairs" width={52} height={52} />)
  //   width, height   CSS pixels (224 x 126 by default, the Every tool tab's)
  //   since    when its loop starts (a performance.now() time): 0 keeps every demo of a page in
  //            step; by default, each time it is shown (a tooltip's starts over when it opens)
  //   class    the canvas' classes
  import { drawDemo, drawIcon, type ShapeParams } from "./demos";

  let {
    name,
    shape,
    width = 224,
    height = 126,
    since,
    class: cls = "",
  }: { name?: string; shape?: string | ShapeParams; width?: number; height?: number; since?: number; class?: string } = $props();

  let canvas: HTMLCanvasElement;

  $effect(() => {
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    if (shape) {
      drawIcon(ctx, shape, width, height);
      return;
    }
    if (!name) return;
    const demo = name;
    const w = width;
    const h = height;
    let start = since ?? performance.now();
    let frame = 0;
    let shown = false;
    const tick = (now: number) => {
      drawDemo(ctx, demo, Math.max(0, now - start) / 1000, w, h);
      frame = requestAnimationFrame(tick);
    };
    // each animation is a picture made every frame: only while it can be seen
    const io = new IntersectionObserver(([e]) => {
      if (e.isIntersecting === shown) return;
      shown = e.isIntersecting;
      cancelAnimationFrame(frame);
      if (!shown) return;
      if (since === undefined) start = performance.now();
      frame = requestAnimationFrame(tick);
    });
    io.observe(canvas);
    return () => {
      io.disconnect();
      cancelAnimationFrame(frame);
    };
  });
</script>

<canvas bind:this={canvas} class="block shrink-0 {cls}" style:width="{width}px" style:height="{height}px"></canvas>
