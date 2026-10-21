// The viewport's math (src/core/math.h): vectors as [x, y, z], matrices column-major in a
// Float32Array (what WebGL takes and what the jle:// protocol sends), quaternions [x, y, z, w].

export type Vec2 = [number, number];
export type Vec3 = [number, number, number];
export type Vec4 = [number, number, number, number];
export type Quat = [number, number, number, number];
export type Mat4 = Float32Array;

export const kPi = Math.PI;
export const radians = (deg: number) => (deg * Math.PI) / 180;
export const degrees = (rad: number) => (rad * 180) / Math.PI;
export const clamp = (v: number, lo: number, hi: number) =>
  Math.min(Math.max(v, lo), hi);

export const add = (a: Vec3, b: Vec3): Vec3 => [
  a[0] + b[0],
  a[1] + b[1],
  a[2] + b[2],
];
export const sub = (a: Vec3, b: Vec3): Vec3 => [
  a[0] - b[0],
  a[1] - b[1],
  a[2] - b[2],
];
export const mul = (a: Vec3, s: number): Vec3 => [a[0] * s, a[1] * s, a[2] * s];
export const dot = (a: Vec3, b: Vec3) =>
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
export const length2 = (a: Vec3) => dot(a, a);
export const length = (a: Vec3) => Math.sqrt(dot(a, a));
export function normalize(a: Vec3): Vec3 {
  const l = length(a);
  return l > 1e-20 ? [a[0] / l, a[1] / l, a[2] / l] : [0, 0, 0];
}

export function identity(): Mat4 {
  const r = new Float32Array(16);
  r[0] = r[5] = r[10] = r[15] = 1;
  return r;
}
// a * b
export function matMul(
  a: Mat4,
  b: Mat4,
  out: Mat4 = new Float32Array(16),
): Mat4 {
  for (let c = 0; c < 4; c++) {
    const b0 = b[c * 4],
      b1 = b[c * 4 + 1],
      b2 = b[c * 4 + 2],
      b3 = b[c * 4 + 3];
    for (let r = 0; r < 4; r++)
      out[c * 4 + r] =
        a[r] * b0 + a[4 + r] * b1 + a[8 + r] * b2 + a[12 + r] * b3;
  }
  return out;
}
export function translate(t: Vec3): Mat4 {
  const r = identity();
  r[12] = t[0];
  r[13] = t[1];
  r[14] = t[2];
  return r;
}
export function scale(s: Vec3): Mat4 {
  const r = identity();
  r[0] = s[0];
  r[5] = s[1];
  r[10] = s[2];
  return r;
}
export function rotate(q: Quat): Mat4 {
  const [x, y, z, w] = q;
  const r = identity();
  const xx = x * x,
    yy = y * y,
    zz = z * z,
    xy = x * y,
    xz = x * z,
    yz = y * z;
  const wx = w * x,
    wy = w * y,
    wz = w * z;
  r[0] = 1 - 2 * (yy + zz);
  r[1] = 2 * (xy + wz);
  r[2] = 2 * (xz - wy);
  r[4] = 2 * (xy - wz);
  r[5] = 1 - 2 * (xx + zz);
  r[6] = 2 * (yz + wx);
  r[8] = 2 * (xz + wy);
  r[9] = 2 * (yz - wx);
  r[10] = 1 - 2 * (xx + yy);
  return r;
}
export function perspective(
  fovy: number,
  aspect: number,
  znear: number,
  zfar: number,
): Mat4 {
  const r = new Float32Array(16);
  const f = 1 / Math.tan(fovy * 0.5);
  r[0] = f / aspect;
  r[5] = f;
  r[10] = (zfar + znear) / (znear - zfar);
  r[11] = -1;
  r[14] = (2 * zfar * znear) / (znear - zfar);
  return r;
}
export function lookAt(eye: Vec3, target: Vec3, up: Vec3): Mat4 {
  const f = normalize(sub(target, eye));
  const s = normalize(cross(f, up));
  const u = cross(s, f);
  const r = identity();
  r[0] = s[0];
  r[4] = s[1];
  r[8] = s[2];
  r[1] = u[0];
  r[5] = u[1];
  r[9] = u[2];
  r[2] = -f[0];
  r[6] = -f[1];
  r[10] = -f[2];
  r[12] = -dot(s, eye);
  r[13] = -dot(u, eye);
  r[14] = dot(f, eye);
  return r;
}
export function inverse(a: Mat4): Mat4 {
  const inv = new Float32Array(16);
  inv[0] =
    a[5] * a[10] * a[15] -
    a[5] * a[11] * a[14] -
    a[9] * a[6] * a[15] +
    a[9] * a[7] * a[14] +
    a[13] * a[6] * a[11] -
    a[13] * a[7] * a[10];
  inv[4] =
    -a[4] * a[10] * a[15] +
    a[4] * a[11] * a[14] +
    a[8] * a[6] * a[15] -
    a[8] * a[7] * a[14] -
    a[12] * a[6] * a[11] +
    a[12] * a[7] * a[10];
  inv[8] =
    a[4] * a[9] * a[15] -
    a[4] * a[11] * a[13] -
    a[8] * a[5] * a[15] +
    a[8] * a[7] * a[13] +
    a[12] * a[5] * a[11] -
    a[12] * a[7] * a[9];
  inv[12] =
    -a[4] * a[9] * a[14] +
    a[4] * a[10] * a[13] +
    a[8] * a[5] * a[14] -
    a[8] * a[6] * a[13] -
    a[12] * a[5] * a[10] +
    a[12] * a[6] * a[9];
  inv[1] =
    -a[1] * a[10] * a[15] +
    a[1] * a[11] * a[14] +
    a[9] * a[2] * a[15] -
    a[9] * a[3] * a[14] -
    a[13] * a[2] * a[11] +
    a[13] * a[3] * a[10];
  inv[5] =
    a[0] * a[10] * a[15] -
    a[0] * a[11] * a[14] -
    a[8] * a[2] * a[15] +
    a[8] * a[3] * a[14] +
    a[12] * a[2] * a[11] -
    a[12] * a[3] * a[10];
  inv[9] =
    -a[0] * a[9] * a[15] +
    a[0] * a[11] * a[13] +
    a[8] * a[1] * a[15] -
    a[8] * a[3] * a[13] -
    a[12] * a[1] * a[11] +
    a[12] * a[3] * a[9];
  inv[13] =
    a[0] * a[9] * a[14] -
    a[0] * a[10] * a[13] -
    a[8] * a[1] * a[14] +
    a[8] * a[2] * a[13] +
    a[12] * a[1] * a[10] -
    a[12] * a[2] * a[9];
  inv[2] =
    a[1] * a[6] * a[15] -
    a[1] * a[7] * a[14] -
    a[5] * a[2] * a[15] +
    a[5] * a[3] * a[14] +
    a[13] * a[2] * a[7] -
    a[13] * a[3] * a[6];
  inv[6] =
    -a[0] * a[6] * a[15] +
    a[0] * a[7] * a[14] +
    a[4] * a[2] * a[15] -
    a[4] * a[3] * a[14] -
    a[12] * a[2] * a[7] +
    a[12] * a[3] * a[6];
  inv[10] =
    a[0] * a[5] * a[15] -
    a[0] * a[7] * a[13] -
    a[4] * a[1] * a[15] +
    a[4] * a[3] * a[13] +
    a[12] * a[1] * a[7] -
    a[12] * a[3] * a[5];
  inv[14] =
    -a[0] * a[5] * a[14] +
    a[0] * a[6] * a[13] +
    a[4] * a[1] * a[14] -
    a[4] * a[2] * a[13] -
    a[12] * a[1] * a[6] +
    a[12] * a[2] * a[5];
  inv[3] =
    -a[1] * a[6] * a[11] +
    a[1] * a[7] * a[10] +
    a[5] * a[2] * a[11] -
    a[5] * a[3] * a[10] -
    a[9] * a[2] * a[7] +
    a[9] * a[3] * a[6];
  inv[7] =
    a[0] * a[6] * a[11] -
    a[0] * a[7] * a[10] -
    a[4] * a[2] * a[11] +
    a[4] * a[3] * a[10] +
    a[8] * a[2] * a[7] -
    a[8] * a[3] * a[6];
  inv[11] =
    -a[0] * a[5] * a[11] +
    a[0] * a[7] * a[9] +
    a[4] * a[1] * a[11] -
    a[4] * a[3] * a[9] -
    a[8] * a[1] * a[7] +
    a[8] * a[3] * a[5];
  inv[15] =
    a[0] * a[5] * a[10] -
    a[0] * a[6] * a[9] -
    a[4] * a[1] * a[10] +
    a[4] * a[2] * a[9] +
    a[8] * a[1] * a[6] -
    a[8] * a[2] * a[5];
  const det = a[0] * inv[0] + a[1] * inv[4] + a[2] * inv[8] + a[3] * inv[12];
  if (Math.abs(det) < 1e-30) return identity();
  for (let i = 0; i < 16; i++) inv[i] /= det;
  return inv;
}
export const col3 = (m: Mat4, c: number): Vec3 => [
  m[c * 4],
  m[c * 4 + 1],
  m[c * 4 + 2],
];
export function setCol3(m: Mat4, c: number, v: Vec3) {
  m[c * 4] = v[0];
  m[c * 4 + 1] = v[1];
  m[c * 4 + 2] = v[2];
}
// m * (p, 1)
export const point = (m: Mat4, p: Vec3): Vec3 => [
  m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
  m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
  m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
];
// m * (d, 0)
export const dir = (m: Mat4, d: Vec3): Vec3 => [
  m[0] * d[0] + m[4] * d[1] + m[8] * d[2],
  m[1] * d[0] + m[5] * d[1] + m[9] * d[2],
  m[2] * d[0] + m[6] * d[1] + m[10] * d[2],
];

export function axisAngle(axis: Vec3, rad: number): Quat {
  const a = normalize(axis);
  const s = Math.sin(rad * 0.5);
  return [a[0] * s, a[1] * s, a[2] * s, Math.cos(rad * 0.5)];
}
export function quatMul(a: Quat, b: Quat): Quat {
  const [x, y, z, w] = a;
  return [
    w * b[0] + x * b[3] + y * b[2] - z * b[1],
    w * b[1] - x * b[2] + y * b[3] + z * b[0],
    w * b[2] + x * b[1] - y * b[0] + z * b[3],
    w * b[3] - x * b[0] - y * b[1] - z * b[2],
  ];
}

export interface Ray {
  origin: Vec3;
  dir: Vec3;
}
export const rayAt = (r: Ray, t: number) => add(r.origin, mul(r.dir, t));
export function rayPlane(r: Ray, p: Vec3, n: Vec3): number | null {
  const den = dot(n, r.dir);
  if (Math.abs(den) < 1e-9) return null;
  return dot(sub(p, r.origin), n) / den;
}

export interface AABB {
  lo: Vec3;
  hi: Vec3;
}
export const aabb = (): AABB => ({
  lo: [Infinity, Infinity, Infinity],
  hi: [-Infinity, -Infinity, -Infinity],
});
export const aabbValid = (b: AABB) =>
  b.lo[0] <= b.hi[0] && b.lo[1] <= b.hi[1] && b.lo[2] <= b.hi[2];
export function aabbAdd(b: AABB, p: Vec3) {
  for (let i = 0; i < 3; i++) {
    b.lo[i] = Math.min(b.lo[i], p[i]);
    b.hi[i] = Math.max(b.hi[i], p[i]);
  }
}
export function aabbAddBox(b: AABB, o: AABB) {
  if (!aabbValid(o)) return;
  aabbAdd(b, o.lo);
  aabbAdd(b, o.hi);
}
export const aabbCenter = (b: AABB): Vec3 => mul(add(b.lo, b.hi), 0.5);
export const aabbRadius = (b: AABB) => length(sub(b.hi, b.lo)) * 0.5;
export const aabbExpanded = (b: AABB, e: number): AABB => ({
  lo: sub(b.lo, [e, e, e]),
  hi: add(b.hi, [e, e, e]),
});
export const aabbContains = (b: AABB, p: Vec3) =>
  p[0] >= b.lo[0] &&
  p[0] <= b.hi[0] &&
  p[1] >= b.lo[1] &&
  p[1] <= b.hi[1] &&
  p[2] >= b.lo[2] &&
  p[2] <= b.hi[2];
// the box around the transformed box (center and half extents: one point transform)
export function aabbTransformed(b: AABB, m: Mat4): AABB {
  if (!aabbValid(b)) return aabb();
  const c = aabbCenter(b),
    e = mul(sub(b.hi, b.lo), 0.5);
  const nc = point(m, c);
  const ne: Vec3 = [
    Math.abs(m[0]) * e[0] + Math.abs(m[4]) * e[1] + Math.abs(m[8]) * e[2],
    Math.abs(m[1]) * e[0] + Math.abs(m[5]) * e[1] + Math.abs(m[9]) * e[2],
    Math.abs(m[2]) * e[0] + Math.abs(m[6]) * e[1] + Math.abs(m[10]) * e[2],
  ];
  return { lo: sub(nc, ne), hi: add(nc, ne) };
}
// The box of most of them: the 2nd and 98th percentiles on each axis, so that a stray piece far
// away does not make the view frame the void (scene.cpp focus_box)
export function focusBox(boxes: AABB[]): AABB {
  const all = aabb();
  const lo: number[][] = [[], [], []],
    hi: number[][] = [[], [], []];
  for (const b of boxes) {
    if (!aabbValid(b)) continue;
    aabbAddBox(all, b);
    for (let a = 0; a < 3; a++) {
      lo[a].push(b.lo[a]);
      hi[a].push(b.hi[a]);
    }
  }
  if (lo[0].length < 20) return all;
  const out = aabb();
  const l: Vec3 = [0, 0, 0],
    h: Vec3 = [0, 0, 0];
  for (let a = 0; a < 3; a++) {
    // ponytail: a full sort where the C++ uses nth_element, fine up to a few 100k boxes
    lo[a].sort((x, y) => x - y);
    hi[a].sort((x, y) => x - y);
    l[a] = lo[a][Math.floor((lo[a].length * 2) / 100)];
    h[a] = hi[a][Math.floor((hi[a].length * 98) / 100)];
  }
  aabbAdd(out, l);
  aabbAdd(out, h);
  return aabbValid(out) ? out : all;
}

// colors of the immediate batch: packed as the C++ packRgba / IM_COL32 (red in the low byte, the
// bytes in memory r, g, b, a)
export const col32 = (r: number, g: number, b: number, a = 255) =>
  ((a << 24) | (b << 16) | (g << 8) | r) >>> 0;
export const packRgba = (r: number, g: number, b: number, a = 1) =>
  col32(
    Math.round(clamp(r, 0, 1) * 255),
    Math.round(clamp(g, 0, 1) * 255),
    Math.round(clamp(b, 0, 1) * 255),
    Math.round(clamp(a, 0, 1) * 255),
  );
