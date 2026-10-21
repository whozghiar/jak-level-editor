// WebGL2 renderer of the viewport (src/render/renderer.{h,cpp}). Draws the scene's render meshes
// (texture x vertex color, like the game's background), markers for the actors without a model,
// the outline of the selection and an immediate-mode batch of lines and triangles for tools (grid,
// gizmo...).

import type { Camera } from "./camera";
import {
  aabb,
  aabbAdd,
  aabbValid,
  col3,
  col32,
  dir,
  identity,
  kPi,
  length2,
  matMul,
  mul,
  add,
  normalize,
  packRgba,
  point,
  sub,
  type AABB,
  type Mat4,
  type Vec2,
  type Vec3,
  type Vec4,
} from "./math";
import type { MeshData, MeshPrim } from "./protocol";
import type { MeshEntry, SceneStore } from "./scene_store";
import {
  FLAT_FS,
  FLAT_VS,
  INSTANCE_TEXELS_PER_ROW,
  MESH_FS,
  MESH_VS,
  SOLID_FS,
  SOLID_VS,
  WATER_FS,
  WATER_VS,
} from "./shaders";
import {
  AlphaMode,
  BlendMode,
  K_MARKER_RADIUS,
  NodeFlag,
  NodeKind,
  type Material,
  type NodeRecord,
  type RenderSettings,
  type WaterLook,
} from "./types";

export interface DrawStats {
  draws: number;
  triangles: number;
  nodes_culled: number;
  // CPU time of drawScene's steps (ms), for profiling
  ms_cull: number;
  ms_instances: number;
  ms_draw: number;
  ms_overlays: number;
}
const no_stats = (): DrawStats => ({
  draws: 0,
  triangles: 0,
  nodes_culled: 0,
  ms_cull: 0,
  ms_instances: 0,
  ms_draw: 0,
  ms_overlays: 0,
});

interface Program {
  p: WebGLProgram;
  u: Record<string, WebGLUniformLocation | null>;
}

interface GpuPrim {
  src: MeshPrim;
  vao: WebGLVertexArrayObject;
  line_vao: WebGLVertexArrayObject | null; // its edges (wireframe, outlines), made when first asked
  first: number; // bytes into the mesh's index buffer
  line_first: number;
  has_colors: boolean;
  material: number;
}
interface GpuMesh {
  kind: "mesh";
  vbo: WebGLBuffer; // the whole JLEM answer: the primitives' arrays where they are in it
  ebo: WebGLBuffer;
  line_ebo: WebGLBuffer | null;
  prims: GpuPrim[];
  data: MeshData;
}
interface GpuTexture {
  kind: "texture";
  tex: WebGLTexture;
}

// a mesh to draw: what the store holds, or a mesh read apart (a preview)
export type MeshSource = MeshEntry | { data: MeshData | null; gpu: unknown };

function hashColor(extra: number, alpha = 1): number {
  // renderer.cpp hash_color(etype): its FNV-1a hash's bytes; the engine sends its low 16 bits
  // (JLES reserved), 0 for none: the hash of "" then, as the C++ for an actor without etype
  const h = extra ? extra : 2166136261;
  const b = extra
    ? (Math.imul(extra, 2654435761) >>> 24) & 0xff
    : (h >>> 16) & 0xff;
  return packRgba(
    0.45 + ((h & 0xff) / 255) * 0.5,
    0.45 + (((h >>> 8) & 0xff) / 255) * 0.5,
    0.45 + (b / 255) * 0.5,
    alpha,
  );
}

const TINT_NONE: Vec4 = [0, 0, 0, 0];
const TINT_REFERENCE: Vec4 = [0.3, 0.55, 1, 0.55];
const TINT_SELECTED: Vec4 = [1, 0.6, 0.12, 0.18];
const TINT_HOVERED: Vec4 = [1, 1, 0.8, 0.12];

const kSelected = 0xff1e9bff; // orange (ABGR packed as RGBA little endian)
const kHovered = 0xffffe08a;

const VERTEX_BYTES = 16; // immediate mode: f32 pos[3], u8 color[4]

export class Renderer {
  readonly gl: WebGL2RenderingContext;
  stats: DrawStats = no_stats();
  // What drawScene outlines (wire over the surfaces): the selection with what is under it, and
  // what the cursor points at: the element under it, its group, what a box would select
  outlineSelection: number[] = [];
  outlineHover: number[] = [];
  outlineGroup: number[] = [];
  // the decor the editor draws in the game view (a drag's pieces, left out of its chunks)
  editorDrawn: ReadonlySet<number> = new Set();
  // a capture of some elements alone (capture_view's `only`): the others are not drawn
  only: ReadonlySet<number> | null = null;

  private mesh_prog!: Program;
  private flat_prog!: Program;
  private solid_prog!: Program;
  private water_prog!: Program;
  private white!: WebGLTexture;
  // a texture not read yet: see-through, the alpha-tested and blended surfaces (leaves, lamps,
  // fences) left out meanwhile instead of drawn as white squares (the C++ had them all at once)
  private unread!: WebGLTexture;
  private inst_tex!: WebGLTexture;
  private inst_rows = 0;
  private inst = new Float32Array(INSTANCE_TEXELS_PER_ROW * 4 * 4);
  private inst_count = 0;
  private imm_vao!: WebGLVertexArrayObject;
  private imm_vbo!: WebGLBuffer;
  private lines: number[] = []; // x, y, z, color per vertex
  private tris: number[] = [];
  private imm_buf = new ArrayBuffer(1 << 16);
  private material_key = "";
  private bound_texture: WebGLTexture | null = null;
  private view: Mat4 = identity();
  private proj: Mat4 = identity();
  private viewproj: Mat4 = identity();
  private draw_vp: Mat4 = identity(); // what the draws use: viewproj, or the game camera's
  private game = false; // over the game view (setGameDepth)
  private planes: Vec4[] = [];
  private cam_pos: Vec3 = [0, 0, 0];
  private cam_fwd: Vec3 = [0, 0, -1];
  private vp: [number, number] = [1, 1];
  private logical: [number, number] = [1, 1];
  private frame = 0;

  constructor(gl: WebGL2RenderingContext) {
    this.gl = gl;
    this.mesh_prog = this.link(MESH_VS, MESH_FS);
    this.flat_prog = this.link(FLAT_VS, FLAT_FS);
    this.solid_prog = this.link(SOLID_VS, SOLID_FS);
    this.water_prog = this.link(WATER_VS, WATER_FS);
    this.inst_tex = gl.createTexture()!;
    this.imm_vao = gl.createVertexArray()!;
    this.imm_vbo = gl.createBuffer()!;
    gl.bindVertexArray(this.imm_vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.imm_vbo);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, VERTEX_BYTES, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 4, gl.UNSIGNED_BYTE, true, VERTEX_BYTES, 12);
    gl.bindVertexArray(null);
    // what a primitive without uvs or colors reads (the attributes left off): uv 0, white
    gl.vertexAttrib2f(2, 0, 0);
    gl.vertexAttrib4f(3, 1, 1, 1, 1);
    const pixel = (rgba: number[]) => {
      const t = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA8,
        1,
        1,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        new Uint8Array(rgba),
      );
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      return t;
    };
    this.white = pixel([255, 255, 255, 255]);
    this.unread = pixel([255, 255, 255, 0]);
  }

  private link(vs: string, fs: string): Program {
    const gl = this.gl;
    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
        throw new Error(gl.getShaderInfoLog(s) ?? "shader");
      return s;
    };
    const p = gl.createProgram()!;
    const v = compile(gl.VERTEX_SHADER, vs),
      f = compile(gl.FRAGMENT_SHADER, fs);
    gl.attachShader(p, v);
    gl.attachShader(p, f);
    gl.linkProgram(p);
    gl.deleteShader(v);
    gl.deleteShader(f);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS))
      throw new Error(gl.getProgramInfoLog(p) ?? "program");
    // uniform locations, looked up once (a lookup is a string search in the driver)
    const u: Program["u"] = {};
    const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS) as number;
    for (let i = 0; i < n; i++) {
      const name = gl.getActiveUniform(p, i)!.name.replace(/\[0\]$/, "");
      u[name] = gl.getUniformLocation(p, name);
    }
    return { p, u };
  }

  // frees what was made of a mesh or a texture (SceneStore.freeGpu)
  free(gpu: unknown) {
    const gl = this.gl;
    const g = gpu as GpuMesh | GpuTexture | null;
    if (!g) return;
    if (g.kind === "texture") {
      gl.deleteTexture(g.tex);
      if (this.bound_texture === g.tex) this.bound_texture = null;
      return;
    }
    gl.deleteBuffer(g.vbo);
    gl.deleteBuffer(g.ebo);
    if (g.line_ebo) gl.deleteBuffer(g.line_ebo);
    for (const p of g.prims) {
      gl.deleteVertexArray(p.vao);
      if (p.line_vao) gl.deleteVertexArray(p.line_vao);
    }
  }

  // ---------------------------------------------------------------------------------------------
  // GPU resources

  private primVao(
    gm: GpuMesh,
    p: MeshPrim,
    ebo: WebGLBuffer,
  ): WebGLVertexArrayObject {
    const gl = this.gl;
    const vao = gl.createVertexArray()!;
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, gm.vbo);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, p.pos);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 3, gl.FLOAT, false, 0, p.nrm);
    if (p.uv >= 0) {
      gl.enableVertexAttribArray(2);
      gl.vertexAttribPointer(2, 2, gl.FLOAT, false, 0, p.uv);
    }
    if (p.col >= 0) {
      gl.enableVertexAttribArray(3);
      gl.vertexAttribPointer(3, 4, gl.UNSIGNED_BYTE, true, 0, p.col);
    }
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ebo);
    gl.bindVertexArray(null);
    return vao;
  }

  // the mesh's buffers, made when first drawn: its answer as it is for the vertices (a view per
  // primitive), the indices together
  private gpuMesh(src: MeshSource): GpuMesh | null {
    if (!src.data) return null;
    if (src.gpu) return src.gpu as GpuMesh;
    const gl = this.gl,
      d = src.data;
    gl.bindVertexArray(null);
    const gm: GpuMesh = {
      kind: "mesh",
      vbo: gl.createBuffer()!,
      ebo: gl.createBuffer()!,
      line_ebo: null,
      prims: [],
      data: d,
    };
    gl.bindBuffer(gl.ARRAY_BUFFER, gm.vbo);
    gl.bufferData(gl.ARRAY_BUFFER, new Uint8Array(d.buffer), gl.STATIC_DRAW);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, gm.ebo);
    gl.bufferData(
      gl.ELEMENT_ARRAY_BUFFER,
      d.prims.reduce((s, p) => s + p.indices * 4, 0),
      gl.STATIC_DRAW,
    );
    let first = 0;
    for (const p of d.prims) {
      gl.bufferSubData(
        gl.ELEMENT_ARRAY_BUFFER,
        first,
        new Uint32Array(d.buffer, p.idx, p.indices),
      );
      gm.prims.push({
        src: p,
        vao: this.primVao(gm, p, gm.ebo),
        line_vao: null,
        first,
        line_first: 0,
        has_colors: p.col >= 0,
        material: p.material,
      });
      first += p.indices * 4;
    }
    src.gpu = gm;
    return gm;
  }

  // the edges of the mesh's triangles (GL ES has no polygon mode): two indices per edge
  private lineVaos(gm: GpuMesh) {
    if (gm.line_ebo) return;
    const gl = this.gl;
    gl.bindVertexArray(null);
    gm.line_ebo = gl.createBuffer()!;
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, gm.line_ebo);
    gl.bufferData(
      gl.ELEMENT_ARRAY_BUFFER,
      gm.prims.reduce((s, p) => s + p.src.indices * 8, 0),
      gl.STATIC_DRAW,
    );
    let first = 0;
    for (const p of gm.prims) {
      const idx = new Uint32Array(gm.data.buffer, p.src.idx, p.src.indices);
      const out = new Uint32Array(idx.length * 2);
      for (let t = 0; t + 2 < idx.length; t += 3) {
        const o = t * 2;
        out[o] = out[o + 5] = idx[t];
        out[o + 1] = out[o + 2] = idx[t + 1];
        out[o + 3] = out[o + 4] = idx[t + 2];
      }
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, gm.line_ebo);
      gl.bufferSubData(gl.ELEMENT_ARRAY_BUFFER, first, out);
      p.line_first = first;
      p.line_vao = this.primVao(gm, p.src, gm.line_ebo);
      first += out.byteLength;
    }
  }

  // GL texture of a texture of the document (read and uploaded on first use or after a change)
  glTexture(store: SceneStore, index: number): WebGLTexture {
    const t = store.texture(index);
    if (!t?.data) {
      const e = store.textures[index];
      return e && e.loaded !== e.version ? this.unread : this.white;
    }
    if (t.gpu) return (t.gpu as GpuTexture).tex;
    const gl = this.gl;
    const tex = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, tex);
    this.bound_texture = tex;
    const { width, height, rgba } = t.data;
    if (width > 0 && height > 0) {
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA8,
        width,
        height,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        rgba,
      );
      gl.generateMipmap(gl.TEXTURE_2D);
    }
    gl.texParameteri(
      gl.TEXTURE_2D,
      gl.TEXTURE_MIN_FILTER,
      gl.LINEAR_MIPMAP_LINEAR,
    );
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
    t.gpu = { kind: "texture", tex } satisfies GpuTexture;
    return tex;
  }

  // ---------------------------------------------------------------------------------------------
  // instances: 5 texels each (world matrix columns, tint) in a float texture

  private pushInstance(world: Mat4, tint: Vec4) {
    const need = (this.inst_count + 1) * 20;
    if (need > this.inst.length) {
      const bigger = new Float32Array(Math.max(this.inst.length * 2, need));
      bigger.set(this.inst);
      this.inst = bigger;
    }
    const o = this.inst_count * 20;
    this.inst.set(world, o);
    this.inst[o + 16] = tint[0];
    this.inst[o + 17] = tint[1];
    this.inst[o + 18] = tint[2];
    this.inst[o + 19] = tint[3];
    this.inst_count++;
  }

  private uploadInstances() {
    const gl = this.gl,
      W = INSTANCE_TEXELS_PER_ROW;
    const rows = Math.max(1, Math.ceil((this.inst_count * 5) / W));
    if (this.inst.length < rows * W * 4) {
      const bigger = new Float32Array(rows * W * 4);
      bigger.set(this.inst);
      this.inst = bigger;
    }
    // the mesh shader reads them from texture unit 1
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.inst_tex);
    if (rows > this.inst_rows) {
      this.inst_rows = Math.max(rows, this.inst_rows * 2, 1);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA32F,
        W,
        this.inst_rows,
        0,
        gl.RGBA,
        gl.FLOAT,
        null,
      );
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    }
    gl.texSubImage2D(
      gl.TEXTURE_2D,
      0,
      0,
      0,
      W,
      rows,
      gl.RGBA,
      gl.FLOAT,
      this.inst,
      0,
    );
    gl.activeTexture(gl.TEXTURE0);
    this.bound_texture = null;
  }

  // ---------------------------------------------------------------------------------------------
  // materials

  // 0: opaque and masked, 1: alpha blended, 2: additive and subtractive (BlendMode)
  static materialPass(mat: Material | null): number {
    if (!mat || mat.alpha !== AlphaMode.Blend) return 0;
    return mat.blend === BlendMode.Alpha ? 1 : 2;
  }

  private setBlend(b: number) {
    const gl = this.gl;
    switch (b) {
      case BlendMode.Add:
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFunc(gl.ONE, gl.ONE);
        break;
      case BlendMode.AddAlpha:
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
        break;
      case BlendMode.Subtract:
        gl.blendEquation(gl.FUNC_REVERSE_SUBTRACT);
        gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
        break;
      case BlendMode.Half:
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendColor(0, 0, 0, 0.5);
        gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE_MINUS_CONSTANT_ALPHA);
        break;
      default:
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    }
  }

  private setMaterial(
    mat: Material | null,
    store: SceneStore,
    has_colors: boolean,
    rs: RenderSettings,
    alpha = 1,
    water = false,
  ) {
    // the state of the previous draw is kept: many primitives in a row share a material
    const gl = this.gl,
      u = this.mesh_prog.u;
    const has_tex = rs.textures && !!mat && mat.texture >= 0;
    const tex = has_tex ? this.glTexture(store, mat!.texture) : this.white;
    if (tex !== this.bound_texture) {
      gl.bindTexture(gl.TEXTURE_2D, tex);
      this.bound_texture = tex;
    }
    const base = mat ? mat.base_color : [0.8, 0.8, 0.8, 1];
    const vcol = rs.vertex_colors && has_colors;
    // a water surface's texture flows (the game animates the textures of its water)
    const flows = (water || !!mat?.water) && has_tex;
    const sx = flows ? (rs.time * 0.031) % 1 : 0,
      sy = flows ? (rs.time * 0.017) % 1 : 0;
    const st = [
      has_tex ? 1 : 0,
      base[0],
      base[1],
      base[2],
      base[3] * alpha,
      mat ? mat.alpha : 0,
      mat ? mat.cutoff : 0.5,
      vcol ? 1 : 0,
      !vcol && rs.shade_uncolored ? 1 : 0,
      sx,
      sy,
    ];
    const key = st.join(",");
    if (key === this.material_key) return;
    gl.uniform1i(u.u_has_tex, st[0]);
    gl.uniform4f(u.u_base, st[1], st[2], st[3], st[4]);
    gl.uniform1i(u.u_alpha_mode, st[5]);
    gl.uniform1f(u.u_cutoff, st[6]);
    gl.uniform1i(u.u_use_vcol, st[7]);
    gl.uniform1i(u.u_shade, st[8]);
    gl.uniform2f(u.u_uv_scroll, st[9], st[10]);
    this.material_key = key;
  }

  // ---------------------------------------------------------------------------------------------
  // the frame

  // the camera and the viewport project() answers for (pixels w x h, logical: CSS pixels)
  setView(cam: Camera, w: number, h: number, logical_w = 0, logical_h = 0) {
    this.vp = [Math.max(w, 1), Math.max(h, 1)];
    this.logical = [
      logical_w > 0 ? logical_w : this.vp[0],
      logical_h > 0 ? logical_h : this.vp[1],
    ];
    this.view = cam.view();
    this.proj = cam.proj(this.vp[0] / this.vp[1]);
    this.viewproj = matMul(this.proj, this.view);
    if (!this.game) this.draw_vp = this.viewproj;
    this.cam_pos = [...cam.pos];
    this.cam_fwd = cam.forward();
  }

  begin(
    cam: Camera,
    w: number,
    h: number,
    rs: RenderSettings,
    logical_w = 0,
    logical_h = 0,
  ) {
    const gl = this.gl;
    this.frame++;
    this.stats = no_stats();
    this.setView(cam, w, h, logical_w, logical_h);
    gl.viewport(0, 0, this.vp[0], this.vp[1]);
    gl.clearColor(rs.clear_color[0], rs.clear_color[1], rs.clear_color[2], 1);
    gl.clearDepth(1);
    gl.depthMask(true);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    // frustum planes (Gribb-Hartmann), in world space
    const m = this.viewproj;
    const row = (r: number): Vec4 => [m[r], m[4 + r], m[8 + r], m[12 + r]];
    const r0 = row(0),
      r1 = row(1),
      r2 = row(2),
      r3 = row(3);
    const plus = (a: Vec4, b: Vec4, s: number): Vec4 => [
      a[0] + b[0] * s,
      a[1] + b[1] * s,
      a[2] + b[2] * s,
      a[3] + b[3] * s,
    ];
    this.planes = [
      plus(r3, r0, 1),
      plus(r3, r0, -1),
      plus(r3, r1, 1),
      plus(r3, r1, -1),
      plus(r3, r2, 1),
      plus(r3, r2, -1),
    ];
    if (rs.far_ocean && rs.show_ocean) {
      // a plane around the camera reaching the far clip, not depth tested: the scene covers it
      const r = cam.zfar * 0.7,
        y = rs.far_ocean_y;
      const c: Vec3 = [cam.pos[0], y, cam.pos[2]];
      const col = packRgba(
        rs.far_ocean_color[0],
        rs.far_ocean_color[1],
        rs.far_ocean_color[2],
        1,
      );
      const a = add(c, [-r, 0, -r]),
        b = add(c, [r, 0, -r]),
        d = add(c, [r, 0, r]),
        e = add(c, [-r, 0, r]);
      this.tri(a, b, d, col);
      this.tri(a, d, e, col);
      this.flush(false);
    }
    if (rs.show_grid) this.grid(cam);
  }

  visible(b: AABB): boolean {
    if (!aabbValid(b)) return false;
    for (const p of this.planes) {
      const x = p[0] >= 0 ? b.hi[0] : b.lo[0],
        y = p[1] >= 0 ? b.hi[1] : b.lo[1],
        z = p[2] >= 0 ? b.hi[2] : b.lo[2];
      if (p[0] * x + p[1] * y + p[2] * z + p[3] < 0) return false;
    }
    return true;
  }

  // world -> viewport CSS pixels (origin top-left). null when behind the camera.
  project(world: Vec3): Vec2 | null {
    const m = this.viewproj;
    const x = m[0] * world[0] + m[4] * world[1] + m[8] * world[2] + m[12];
    const y = m[1] * world[0] + m[5] * world[1] + m[9] * world[2] + m[13];
    const w = m[3] * world[0] + m[7] * world[1] + m[11] * world[2] + m[15];
    if (w <= 1e-5) return null;
    return [
      ((x / w) * 0.5 + 0.5) * this.logical[0],
      (1 - ((y / w) * 0.5 + 0.5)) * this.logical[1],
    ];
  }
  get viewProj(): Mat4 {
    return this.viewproj;
  }
  get viewMatrix(): Mat4 {
    return this.view;
  }
  // what the draws use: the editor camera's matrix, or the game camera's over the game view
  get drawViewProj(): Mat4 {
    return this.draw_vp;
  }

  // What is drawn next goes over the game view: with the game camera's matrix (world -> clip
  // space) and its depth (reversed: 1 near, 0 far). null: the editor's.
  setGameDepth(clip: Mat4 | null) {
    this.game = clip !== null;
    this.draw_vp = clip ?? this.viewproj;
  }
  // a depth test of the editor's (LESS, LEQUAL) over the game view's reversed depth
  depthFunc(f: number): number {
    const gl = this.gl;
    if (!this.game) return f;
    return f === gl.LESS ? gl.GREATER : f === gl.LEQUAL ? gl.GEQUAL : f;
  }
  private polygonOffset(factor: number, units: number) {
    // toward the camera: a smaller depth, or a larger one over the game view
    if (this.game) this.gl.polygonOffset(-factor, -units);
    else this.gl.polygonOffset(factor, units);
  }

  private grid(cam: Camera) {
    const h = Math.max(Math.abs(cam.pos[1]), 1);
    let step = Math.pow(10, Math.floor(Math.log10(h)));
    step = Math.max(step, 1);
    const n = 60;
    const cx = Math.floor(cam.pos[0] / step) * step;
    const cz = Math.floor(cam.pos[2] / step) * step;
    const minor = packRgba(1, 1, 1, 0.06),
      major = packRgba(1, 1, 1, 0.16);
    for (let i = -n; i <= n; i++) {
      const x = cx + i * step,
        z = cz + i * step;
      const major_x = Math.abs(x) % (step * 10) < 0.5 * step;
      const major_z = Math.abs(z) % (step * 10) < 0.5 * step;
      const cx_col =
        Math.abs(x) < 0.5 * step
          ? packRgba(0.3, 0.35, 0.9, 0.9)
          : major_x
            ? major
            : minor;
      const cz_col =
        Math.abs(z) < 0.5 * step
          ? packRgba(0.9, 0.3, 0.3, 0.9)
          : major_z
            ? major
            : minor;
      this.line([x, 0, cz - n * step], [x, 0, cz + n * step], cx_col);
      this.line([cx - n * step, 0, z], [cx + n * step, 0, z], cz_col);
    }
    this.flush(true);
  }

  // ---------------------------------------------------------------------------------------------
  // immediate mode, world space, flushed by flush(). Colors: col32(r, g, b, a) / packRgba.

  line(a: Vec3, b: Vec3, color: number) {
    this.lines.push(a[0], a[1], a[2], color, b[0], b[1], b[2], color);
  }
  tri(a: Vec3, b: Vec3, c: Vec3, color: number) {
    this.tris.push(
      a[0],
      a[1],
      a[2],
      color,
      b[0],
      b[1],
      b[2],
      color,
      c[0],
      c[1],
      c[2],
      color,
    );
  }
  boxLines(m: Mat4, b: AABB, color: number) {
    const c: Vec3[] = [];
    for (let i = 0; i < 8; i++)
      c.push(
        point(m, [
          i & 1 ? b.hi[0] : b.lo[0],
          i & 2 ? b.hi[1] : b.lo[1],
          i & 4 ? b.hi[2] : b.lo[2],
        ]),
      );
    const e = [
      [0, 1],
      [2, 3],
      [4, 5],
      [6, 7],
      [0, 2],
      [1, 3],
      [4, 6],
      [5, 7],
      [0, 4],
      [1, 5],
      [2, 6],
      [3, 7],
    ];
    for (const [i, j] of e) this.line(c[i], c[j], color);
  }
  circle(
    center: Vec3,
    u: Vec3,
    v: Vec3,
    r: number,
    color: number,
    segments = 48,
  ) {
    let prev = add(center, mul(u, r));
    for (let i = 1; i <= segments; i++) {
      const a = (i / segments) * 2 * kPi;
      const p = add(
        center,
        mul(add(mul(u, Math.cos(a)), mul(v, Math.sin(a))), r),
      );
      this.line(prev, p, color);
      prev = p;
    }
  }

  private uploadBatch(list: number[]): number {
    const bytes = (list.length / 4) * VERTEX_BYTES;
    if (bytes > this.imm_buf.byteLength)
      this.imm_buf = new ArrayBuffer(
        Math.max(bytes, this.imm_buf.byteLength * 2),
      );
    const f = new Float32Array(this.imm_buf),
      u = new Uint32Array(this.imm_buf);
    for (let i = 0; i < list.length; i += 4) {
      f[i] = list[i];
      f[i + 1] = list[i + 1];
      f[i + 2] = list[i + 2];
      u[i + 3] = list[i + 3];
    }
    this.gl.bufferData(
      this.gl.ARRAY_BUFFER,
      new Uint8Array(this.imm_buf, 0, bytes),
      this.gl.STREAM_DRAW,
    );
    return list.length / 4;
  }

  flush(depth_test: boolean, depth_offset = false) {
    if (!this.lines.length && !this.tris.length) return;
    const gl = this.gl,
      u = this.flat_prog.u;
    gl.useProgram(this.flat_prog.p);
    gl.uniformMatrix4fv(u.u_mvp, false, this.draw_vp);
    gl.uniform1f(u.u_alpha, 1);
    gl.uniform4f(u.u_override, 0, 0, 0, 0);
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.FUNC_ADD);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    if (depth_test) gl.enable(gl.DEPTH_TEST);
    else gl.disable(gl.DEPTH_TEST);
    gl.depthFunc(this.depthFunc(gl.LEQUAL));
    gl.depthMask(false);
    if (depth_offset) {
      gl.enable(gl.POLYGON_OFFSET_FILL);
      this.polygonOffset(-1, -2);
    }
    gl.bindVertexArray(this.imm_vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.imm_vbo);
    if (this.tris.length)
      gl.drawArrays(gl.TRIANGLES, 0, this.uploadBatch(this.tris));
    if (this.lines.length)
      gl.drawArrays(gl.LINES, 0, this.uploadBatch(this.lines));
    gl.bindVertexArray(null);
    gl.disable(gl.POLYGON_OFFSET_FILL);
    gl.depthMask(true);
    gl.enable(gl.DEPTH_TEST);
    this.lines.length = 0;
    this.tris.length = 0;
  }

  // ---------------------------------------------------------------------------------------------
  // the scene

  private meshUniforms(rs: RenderSettings) {
    const gl = this.gl,
      u = this.mesh_prog.u;
    gl.useProgram(this.mesh_prog.p);
    gl.activeTexture(gl.TEXTURE0);
    gl.uniform1i(u.u_tex, 0);
    gl.uniform1i(u.u_instances, 1);
    this.material_key = "";
    gl.uniform1f(u.u_vcol_scale, rs.vertex_color_scale);
    gl.uniformMatrix4fv(u.u_viewproj, false, this.draw_vp);
    gl.uniform3fv(u.u_fog_color, rs.fog_color);
    gl.uniform1f(u.u_fog_start, rs.fog_start);
    gl.uniform1f(u.u_fog_end, rs.fog_end);
    gl.uniform3fv(u.u_cam_pos, this.cam_pos);
  }

  drawScene(store: SceneStore, rs: RenderSettings, hovered: number) {
    const gl = this.gl;
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(this.depthFunc(gl.LEQUAL));
    gl.disable(gl.CULL_FACE);

    // render meshes, grouped by mesh: the nodes of a mesh (a decor prototype and its instances,
    // copies of a model) are drawn by one instanced call per primitive
    interface Bucket {
      entry: MeshEntry;
      reference: boolean;
      water: boolean;
      first: number;
      nodes: NodeRecord[];
    }
    let t0 = performance.now();
    const by_key = new Map<number, Bucket>();
    const buckets: Bucket[] = [];
    for (const n of store.list) {
      if (
        n.kind !== NodeKind.Render ||
        !n.mesh ||
        !(n.flags & NodeFlag.Visible) ||
        (this.only && !this.only.has(n.id))
      )
        continue;
      const actor_model = (n.flags & NodeFlag.UnderActor) !== 0;
      const reference = (n.flags & NodeFlag.Reference) !== 0;
      if (!rs.show_actors && actor_model) continue;
      // the decor is the game view's (the actors' models and the levels shown for reference are not)
      if (
        rs.game_view &&
        !actor_model &&
        !reference &&
        !this.editorDrawn.has(n.id)
      )
        continue;
      const entry = store.meshes.get(n.mesh);
      if (!entry?.data) continue;
      if (rs.frustum_culling && !this.visible(store.bounds(n))) {
        this.stats.nodes_culled++;
        continue;
      }
      const water = (n.flags & NodeFlag.Water) !== 0;
      const key = n.mesh * 4 + (reference ? 2 : 0) + (water ? 1 : 0);
      let b = by_key.get(key);
      if (!b) {
        b = { entry, reference, water, first: 0, nodes: [] };
        by_key.set(key, b);
        buckets.push(b);
      }
      b.nodes.push(n);
    }
    // a level shown for reference last, see-through (the ghost pass below)
    const own = buckets.filter((b) => !b.reference),
      refs = buckets.filter((b) => b.reference);
    this.stats.ms_cull = performance.now() - t0;
    t0 = performance.now();
    this.inst_count = 0;
    for (const b of [...own, ...refs]) {
      b.first = this.inst_count;
      for (const n of b.nodes) {
        // a level shown for reference (the game's level a new one replaces): washed in blue
        let tint = b.reference ? TINT_REFERENCE : TINT_NONE;
        if (n.flags & NodeFlag.Selected) tint = TINT_SELECTED;
        else if (hovered && n.id === hovered) tint = TINT_HOVERED;
        this.pushInstance(store.world(n), tint);
      }
    }
    this.uploadInstances();
    this.stats.ms_instances = performance.now() - t0;
    t0 = performance.now();

    const u = this.mesh_prog.u;
    this.meshUniforms(rs);
    gl.uniform1i(u.u_game_light, rs.game_light ? 1 : 0);
    if (rs.game_light) {
      gl.uniform3fv(u.u_light_ambient, rs.light_ambient);
      gl.uniform3fv(u.u_light_dir, rs.light_dir.flat());
      gl.uniform3fv(u.u_light_color, rs.light_color.flat());
      gl.uniform4fv(u.u_game_fog, rs.game_fog);
      gl.uniform2f(u.u_game_fog_range, rs.game_fog_start, rs.game_fog_end);
      gl.uniform3fv(u.u_cam_fwd, this.cam_fwd);
    }
    const material = (i: number) =>
      i >= 0 && i < store.materials.length ? store.materials[i] : null;
    const draw = (gp: GpuPrim, instances: number) => {
      if (rs.wireframe) {
        gl.bindVertexArray(gp.line_vao);
        gl.drawElementsInstanced(
          gl.LINES,
          gp.src.indices * 2,
          gl.UNSIGNED_INT,
          gp.line_first,
          instances,
        );
      } else {
        gl.bindVertexArray(gp.vao);
        gl.drawElementsInstanced(
          gl.TRIANGLES,
          gp.src.indices,
          gl.UNSIGNED_INT,
          gp.first,
          instances,
        );
      }
      this.stats.draws++;
      this.stats.triangles += Math.floor(gp.src.indices / 3) * instances;
    };
    gl.depthMask(true);
    // opaque and masked primitives, then blended ones (with depth writes too: the PS2 draws most
    // blended level geometry that way, relying on the alpha test for the holes), then the additive
    // and subtractive ones (glows, light beams) without depth writes
    for (let pass = 0; pass < 3; pass++) {
      if (pass === 0) {
        gl.disable(gl.BLEND);
      } else {
        gl.enable(gl.BLEND);
        this.setBlend(BlendMode.Alpha);
        if (pass === 2) gl.depthMask(false);
      }
      for (const b of own) {
        const gm = this.gpuMesh(b.entry);
        if (!gm) continue;
        if (rs.wireframe) this.lineVaos(gm);
        const backdrop = rs.fog && gm.data.backdrop;
        let based = false;
        for (const gp of gm.prims) {
          const mat = material(gp.material);
          if (Renderer.materialPass(mat) !== pass) continue;
          if (!based) {
            gl.uniform1i(u.u_instance_base, b.first);
            gl.uniform1i(u.u_fog, backdrop ? 1 : 0);
            based = true;
          }
          if (pass === 2) this.setBlend(mat!.blend);
          this.setMaterial(mat, store, gp.has_colors, rs, 1, b.water);
          draw(gp, b.nodes.length);
        }
      }
    }
    // the level shown for reference (the game's level a new one replaces), see-through over the
    // document: what the document has in its place shows through it. Its surfaces that the document
    // has too (a level made from it) are at the same depth: LESS leaves them out instead of
    // washing the document's in blue. The glows are left out.
    if (refs.length) {
      this.setBlend(BlendMode.Alpha);
      gl.enable(gl.BLEND);
      gl.depthMask(false);
      gl.depthFunc(this.depthFunc(gl.LESS));
      gl.uniform1i(u.u_fog, 0);
      for (const b of refs) {
        const gm = this.gpuMesh(b.entry);
        if (!gm) continue;
        if (rs.wireframe) this.lineVaos(gm);
        gl.uniform1i(u.u_instance_base, b.first);
        for (const gp of gm.prims) {
          const mat = material(gp.material);
          if (Renderer.materialPass(mat) === 2) continue;
          this.setMaterial(mat, store, gp.has_colors, rs, 0.4, b.water);
          draw(gp, b.nodes.length);
        }
      }
      gl.depthFunc(this.depthFunc(gl.LEQUAL));
    }
    this.setBlend(BlendMode.Alpha);
    gl.disable(gl.BLEND);
    gl.depthMask(true);
    gl.bindVertexArray(null);
    this.stats.ms_draw = performance.now() - t0;
    t0 = performance.now();

    // selection outline of render meshes
    const su = this.solid_prog.u;
    gl.useProgram(this.solid_prog.p);
    gl.uniform1f(su.u_depth_bias, 1e-6);
    gl.enable(gl.BLEND);
    gl.depthMask(false);
    const outline = (id: number, c: Vec4) => {
      const n = store.nodes.get(id);
      if (
        !n ||
        n.kind !== NodeKind.Render ||
        !n.mesh ||
        !(n.flags & NodeFlag.Visible) ||
        (this.only && !this.only.has(id))
      )
        return;
      const gm = this.gpuMesh(store.meshes.get(n.mesh)!);
      if (!gm) return;
      this.lineVaos(gm);
      gl.uniformMatrix4fv(
        su.u_mvp,
        false,
        matMul(this.draw_vp, store.world(n)),
      );
      gl.uniform4f(su.u_color, c[0], c[1], c[2], c[3]);
      for (const gp of gm.prims) {
        gl.bindVertexArray(gp.line_vao);
        gl.drawElements(
          gl.LINES,
          gp.src.indices * 2,
          gl.UNSIGNED_INT,
          gp.line_first,
        );
      }
    };
    // the group pointed at, faint; the element pointed at; the selection
    for (const id of this.outlineGroup) outline(id, [0.55, 0.85, 1, 0.22]);
    for (const id of this.outlineHover) outline(id, [1, 1, 0.75, 0.4]);
    for (const id of this.outlineSelection) outline(id, [1, 0.62, 0.15, 0.55]);
    gl.bindVertexArray(null);
    gl.disable(gl.BLEND);
    gl.depthMask(true);

    this.markers(store, rs, hovered);
    this.stats.ms_overlays = performance.now() - t0;
  }

  private markers(store: SceneStore, rs: RenderSettings, hovered: number) {
    // an actor has a marker when the library has no model for it
    const r = K_MARKER_RADIUS;
    for (const n of store.list) {
      if (
        n.kind !== NodeKind.Actor ||
        !(n.flags & NodeFlag.Visible) ||
        store.hasModel(n.id) ||
        (this.only && !this.only.has(n.id))
      )
        continue;
      const sel = (n.flags & NodeFlag.Selected) !== 0;
      if (!rs.show_actors && !sel) continue;
      const w = store.world(n);
      const c = col3(w, 3);
      if (!sel && length2(sub(c, this.cam_pos)) > 300 * 300) continue;
      const box = aabb();
      aabbAdd(box, sub(c, [r, r, r]));
      aabbAdd(box, add(c, [r, r, r]));
      if (!this.visible(box)) continue;
      const col = sel
        ? kSelected
        : n.id === hovered
          ? kHovered
          : hashColor(n.extra);
      const px = add(c, [r, 0, 0]),
        nx = sub(c, [r, 0, 0]);
      const py = add(c, [0, r, 0]),
        ny = sub(c, [0, r, 0]);
      const pz = add(c, [0, 0, r]),
        nz = sub(c, [0, 0, r]);
      for (const a of [px, nx]) for (const b of [pz, nz]) this.line(a, b, col);
      for (const a of [px, nx, pz, nz]) {
        this.line(a, py, col);
        this.line(a, ny, col);
      }
      // facing: local +Z
      this.line(c, add(c, mul(normalize(dir(w, [0, 0, 1])), r * 2.2)), col);
    }
    this.flush(true, true);
  }

  // The ocean (a flat mesh at its height): waves of a few lengths moving as deep water waves do
  // (rs.time), the sky they reflect, the sun on them; the game's fog over the game view.
  drawWater(
    mesh: MeshSource,
    world: Mat4,
    look: WaterLook,
    rs: RenderSettings,
  ) {
    const gm = this.gpuMesh(mesh);
    if (!gm) return;
    const gl = this.gl,
      u = this.water_prog.u;
    gl.useProgram(this.water_prog.p);
    gl.uniformMatrix4fv(u.u_viewproj, false, this.draw_vp);
    gl.uniformMatrix4fv(u.u_world, false, world);
    gl.uniform1f(u.u_time, rs.time);
    gl.uniform3fv(u.u_color, look.color);
    gl.uniform3fv(u.u_deep, look.deep);
    gl.uniform3fv(u.u_sky, look.sky);
    gl.uniform3fv(u.u_sun_dir, look.sun_dir);
    gl.uniform3fv(u.u_sun_color, look.sun_color);
    gl.uniform3fv(u.u_cam_pos, this.cam_pos);
    gl.uniform3fv(u.u_cam_fwd, this.cam_fwd);
    gl.uniform1f(u.u_alpha, look.alpha);
    gl.uniform4fv(u.u_fog, rs.game_light ? rs.game_fog : [0, 0, 0, 0]);
    gl.uniform2f(u.u_fog_range, rs.game_fog_start, rs.game_fog_end);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(this.depthFunc(gl.LEQUAL));
    gl.depthMask(true);
    gl.disable(gl.CULL_FACE);
    gl.enable(gl.BLEND);
    this.setBlend(BlendMode.Alpha);
    for (const gp of gm.prims) {
      gl.bindVertexArray(gp.vao);
      gl.drawElements(gl.TRIANGLES, gp.src.indices, gl.UNSIGNED_INT, gp.first);
      this.stats.draws++;
      this.stats.triangles += Math.floor(gp.src.indices / 3);
    }
    gl.bindVertexArray(null);
    gl.disable(gl.BLEND);
  }

  // One mesh with explicit materials (placement preview, the move's ghost). `tint` is mixed into
  // the color by its alpha; `alpha` < 1 draws it see-through.
  drawMesh(
    mesh: MeshSource,
    world: Mat4,
    materials: Material[],
    store: SceneStore,
    rs: RenderSettings,
    tint: Vec4 = [0, 0, 0, 0],
    alpha = 1,
  ) {
    const gm = this.gpuMesh(mesh);
    if (!gm) return;
    const gl = this.gl,
      u = this.mesh_prog.u;
    this.inst_count = 0;
    this.pushInstance(world, tint);
    this.uploadInstances();
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(this.depthFunc(gl.LEQUAL));
    gl.disable(gl.CULL_FACE);
    this.meshUniforms(rs);
    gl.uniform1i(u.u_fog, 0); // a single mesh drawn apart (preview, thumbnail): no fog
    gl.uniform1i(u.u_game_light, 0);
    gl.uniform1i(u.u_instance_base, 0);
    // a see-through preview does not hide what is behind it
    const ghost = alpha < 1;
    for (let pass = 0; pass < 3; pass++) {
      for (const gp of gm.prims) {
        const mat =
          gp.material >= 0 && gp.material < materials.length
            ? materials[gp.material]
            : null;
        const mat_pass = ghost
          ? Math.max(Renderer.materialPass(mat), 1)
          : Renderer.materialPass(mat);
        if (mat_pass !== pass) continue;
        if (pass > 0) {
          gl.enable(gl.BLEND);
          this.setBlend(pass === 2 ? mat!.blend : BlendMode.Alpha);
        } else {
          gl.disable(gl.BLEND);
        }
        gl.depthMask(!(ghost || pass === 2));
        this.setMaterial(mat, store, gp.has_colors, rs, alpha);
        gl.bindVertexArray(gp.vao);
        gl.drawElementsInstanced(
          gl.TRIANGLES,
          gp.src.indices,
          gl.UNSIGNED_INT,
          gp.first,
          1,
        );
        this.stats.draws++;
        this.stats.triangles += Math.floor(gp.src.indices / 3);
      }
    }
    gl.bindVertexArray(null);
    this.setBlend(BlendMode.Alpha);
    gl.disable(gl.BLEND);
    gl.depthMask(true);
  }
}

export { col32, packRgba };
