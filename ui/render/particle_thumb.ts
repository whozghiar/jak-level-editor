// The catalog's pictures of the effects (render/thumbnails.cpp render_effect, App::catalog_thumb):
// an effect alone, a moment after it starts (a second and a half of the game), framed from 3/4 a
// little from below the top (most effects rise), on the thumbnails' dark background. One GL
// context of its own draws them one at a time.

import { Camera } from "./camera";
import { aabbRadius, aabbValid, identity, matMul } from "./math";
import {
  loadParticleData,
  ParticleSystem,
  SpriteRenderer,
  type ParticleData,
} from "./particles";

let gl: WebGL2RenderingContext | null = null;
let sprites: SpriteRenderer | null = null;
let queue: Promise<unknown> = Promise.resolve();

/** Draws the effect `group` (an art-name, quoted or not) into `target` at its size, after
 *  `seconds` of the game; false when there is no such effect (or no WebGL2). */
export function drawEffectThumb(target: HTMLCanvasElement, group: string, seconds = 1.5): Promise<boolean> {
  const job = queue.then(() => draw(target, group, seconds));
  queue = job.catch(() => {});
  return job;
}

async function draw(target: HTMLCanvasElement, name: string, seconds: number): Promise<boolean> {
  const data = await loadParticleData();
  const group = data?.group(name);
  if (!data || !group) return false;
  const ps = new ParticleSystem();
  ps.beginFrame();
  ps.emit(1, group, identity());
  for (let i = 0; i < Math.round(seconds * 6); i++) ps.update(data, 1 / 6, [0, 0, 0], -1);
  return render(target, ps, data);
}

/** The effect `group` playing in `target` (the catalog's tooltip: Thumbnails::live_effect), until
 *  the returned function stops it. */
export function playEffect(target: HTMLCanvasElement, group: string): () => void {
  let stop = false;
  void loadParticleData().then(async (data) => {
    const g = data?.group(group);
    if (!data || !g) return;
    const ps = new ParticleSystem();
    let last = performance.now();
    while (!stop) {
      const now = performance.now();
      ps.beginFrame();
      ps.emit(1, g, identity());
      ps.update(data, Math.min((now - last) / 1000, 0.1), [0, 0, 0], -1);
      last = now;
      const job = queue.then(() => render(target, ps, data));
      queue = job.catch(() => {});
      await job.catch(() => {});
      await new Promise((r) => requestAnimationFrame(r));
    }
  });
  return () => (stop = true);
}

/** A svelte action: the effect plays on the canvas while it is there. */
export function playing(canvas: HTMLCanvasElement, group: string) {
  return { destroy: playEffect(canvas, group) };
}

// the particles framed from 3/4 (draw_particles), into `target`
async function render(target: HTMLCanvasElement, ps: ParticleSystem, data: ParticleData): Promise<boolean> {
  const list = ps.sprites();
  let b = ps.bounds();
  if (!aabbValid(b)) b = { lo: [-1, -1, -1], hi: [1, 1, 1] };
  const cam = new Camera();
  cam.fov_deg = 40;
  cam.yaw = 0.65;
  cam.pitch = -0.25;
  cam.frame(b);
  cam.znear = Math.max(0.01, aabbRadius(b) * 0.01);
  if (!gl) {
    gl = document.createElement("canvas").getContext("webgl2", { alpha: false, preserveDrawingBuffer: true });
    if (!gl) return false;
    sprites = new SpriteRenderer(gl);
  }
  await sprites!.ready(list, data.textures);
  const canvas = gl.canvas as HTMLCanvasElement;
  const w = Math.max(target.width, 1),
    h = Math.max(target.height, 1);
  if (canvas.width !== w || canvas.height !== h) Object.assign(canvas, { width: w, height: h });
  gl.viewport(0, 0, w, h);
  gl.clearColor(0.07, 0.08, 0.1, 1);
  gl.clearDepth(1);
  gl.depthMask(true);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  const view = cam.view();
  sprites!.draw(list, data.textures, matMul(cam.proj(w / h), view), view, gl.LEQUAL);
  const ctx = target.getContext("2d");
  ctx?.drawImage(canvas, 0, 0);
  return !!ctx;
}
