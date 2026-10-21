// The catalog's pictures of the effects (render/thumbnails.cpp render_effect, App::catalog_thumb):
// an effect alone, a moment after it starts (a second and a half of the game), framed from 3/4 a
// little from below the top (most effects rise), on the thumbnails' dark background. One GL
// context of its own draws them one at a time.

import { Camera } from "./camera";
import { aabbRadius, aabbValid, identity, matMul } from "./math";
import { loadParticleData, ParticleSystem, SpriteRenderer } from "./particles";

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
