// Transform gizmo (src/tools/gizmo.{h,cpp}): move (axes, planes, view plane), rotate (rings) and
// scale (along an axis of the selection, or all of them from the center). Math in 3D, drawing in
// 2D on the viewport's overlay canvas.
//
// While dragging: Shift moves ten times finer, Ctrl turns the snap on (or off when it is on). In
// world space the snap puts the gizmo's pivot on the grid (absolute), in local space it moves by
// steps (relative). An axis seen end-on and a plane seen edge-on cannot be grabbed (they would
// jump), and a ring seen edge-on turns with the mouse along it instead of around its center. The
// value of the drag is written next to the cursor.

import type { Camera } from "./camera";
import { call } from "./engine";
import {
  add,
  axisAngle,
  col3,
  cross,
  degrees,
  dot,
  identity,
  kPi,
  length,
  length2,
  matMul,
  mul,
  normalize,
  radians,
  rayAt,
  rayPlane,
  rotate,
  scale,
  setCol3,
  sub,
  translate,
  type Mat4,
  type Ray,
  type Vec2,
  type Vec3,
} from "./math";
import type { SceneStore } from "./scene_store";

export const GizmoOp = { Translate: 0, Rotate: 1, Scale: 2 } as const;
export const GizmoSpace = { World: 0, Local: 1 } as const;

export interface GizmoSnap {
  enabled: boolean;
  translate: number; // meters
  rotate_deg: number;
  scale: number; // steps of the scale factor
}

export interface GizmoInput {
  mouse: Vec2; // viewport CSS pixels, origin top-left
  viewportSize: Vec2;
  mouseInViewport: boolean;
  pressed: boolean; // left button went down now
  down: boolean;
  fine: boolean; // Shift: ten times finer
  snapToggle: boolean; // Ctrl: the snap the other way
}

export interface GizmoResult {
  started: boolean; // drag began now
  dragging: boolean; // drag in progress (delta is valid)
  finished: boolean; // drag ended now
  // world delta: new_world = delta * start_world (move, rotate)
  delta: Mat4;
  // scale: the factors along the axes of the gizmo's frame (the selection's own axes), to multiply
  // the scale the elements had when the drag began: new_world = start_world * scale(factors)
  scale: Vec3;
}

// what the gizmo projects with (Renderer.project: CSS pixels, null behind the camera)
export type Project = (world: Vec3) => Vec2 | null;

// The value of a drag snapped: `v` to the nearest multiple of `step` (no snap when step <= 0).
export const snapValue = (v: number, step: number) =>
  step > 0 ? Math.round(v / step) * step : v;

const v2sub = (a: Vec2, b: Vec2): Vec2 => [a[0] - b[0], a[1] - b[1]];
const v2len = (a: Vec2) => Math.hypot(a[0], a[1]);

function distPointSegment(p: Vec2, a: Vec2, b: Vec2): number {
  const ab = v2sub(b, a),
    ap = v2sub(p, a);
  const len2 = ab[0] * ab[0] + ab[1] * ab[1];
  const t =
    len2 > 1e-6
      ? Math.min(Math.max((ap[0] * ab[0] + ap[1] * ab[1]) / len2, 0), 1)
      : 0;
  return v2len(v2sub(p, [a[0] + ab[0] * t, a[1] + ab[1] * t]));
}

function pointInQuad(p: Vec2, q: Vec2[]): boolean {
  // convex quad, either winding
  let sign = 0;
  for (let i = 0; i < 4; i++) {
    const a = q[i],
      b = q[(i + 1) % 4];
    const c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    const s = c > 0 ? 1 : c < 0 ? -1 : 0;
    if (s === 0) continue;
    if (sign === 0) sign = s;
    else if (s !== sign) return false;
  }
  return true;
}

const rgba = (r: number, g: number, b: number, a = 255) =>
  `rgba(${r},${g},${b},${a / 255})`;
const AXIS_RGB: [number, number, number][] = [
  [232, 72, 72],
  [96, 206, 90],
  [80, 132, 245],
];
const axisColor = (i: number, hot: boolean, a = 255) =>
  hot
    ? rgba(255, 220, 70, 255)
    : i < 3
      ? rgba(...AXIS_RGB[i], a)
      : rgba(230, 230, 230, a);

function perpendicular(v: Vec3): Vec3 {
  const o: Vec3 = Math.abs(v[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0];
  return normalize(cross(v, o));
}

// an axis seen end-on, a plane seen edge-on: a drag along them would jump
const axisUsable = (axis: Vec3, view: Vec3) =>
  Math.abs(dot(axis, view)) < 0.985;
const planeUsable = (normal: Vec3, view: Vec3) =>
  Math.abs(dot(normal, view)) > 0.12;

function wrapAngle(a: number): number {
  while (a > kPi) a -= 2 * kPi;
  while (a < -kPi) a += 2 * kPi;
  return a;
}

const kAxisPick = 10; // pixels from a handle that grab it
const kCenterPick = 9;
const NAMES = ["X", "Y", "Z"];
const fmt = (v: number, d: number) => (v >= 0 ? "+" : "") + v.toFixed(d);

export class Gizmo {
  op: number = GizmoOp.Translate;
  space: number = GizmoSpace.World;
  snap: GizmoSnap = {
    enabled: false,
    translate: 0.5,
    rotate_deg: 15,
    scale: 0.1,
  };

  // handle ids: 0-2 axes (rings when rotating), 3-5 planes (yz, xz, xy), 6 view plane (all axes
  // when scaling)
  private hot = -1;
  private active_ = -1;
  private start_frame: Mat4 = identity();
  private plane_point: Vec3 = [0, 0, 0];
  private plane_normal: Vec3 = [0, 1, 0];
  private start_hit: Vec3 = [0, 0, 0];
  private last_hit: Vec3 = [0, 0, 0];
  private start_mouse: Vec2 = [0, 0];
  private last_mouse: Vec2 = [0, 0];
  private edge_on = false; // a ring seen edge-on: turned along its tangent on screen
  private tangent: Vec2 = [1, 0]; // that tangent (unit, screen), and the ring's radius on screen
  private radius_px = 1;
  // what the mouse did since the drag began (raw), and what it does (Shift: a tenth of it)
  private raw_move: Vec3 = [0, 0, 0];
  private move: Vec3 = [0, 0, 0];
  private raw_angle = 0;
  private angle = 0;
  private raw_scale = 1;
  private scale_ = 1;
  private readout_ = "";
  private mouse: Vec2 = [0, 0];

  hovering(): boolean {
    return this.hot >= 0;
  }
  active(): boolean {
    return this.active_ >= 0;
  }
  // what the drag going on did, in words ("X +1.50 m", "Y +45.0 deg"...), "" when none
  readout(): string {
    return this.readout_;
  }

  // the orthonormal gizmo frame, its size in the world (a fixed size on screen), the view's way
  private place(cam: Camera, frame: Mat4) {
    // scaling is along the selection's own axes
    const local = this.space === GizmoSpace.Local || this.op === GizmoOp.Scale;
    const f = identity();
    if (local) {
      let x = normalize(col3(frame, 0));
      let y = col3(frame, 1);
      if (length2(x) === 0) x = [1, 0, 0];
      let z = normalize(cross(x, y));
      if (length2(z) === 0) z = perpendicular(x);
      y = cross(z, x);
      setCol3(f, 0, x);
      setCol3(f, 1, y);
      setCol3(f, 2, z);
    }
    setCol3(f, 3, col3(frame, 3));
    const pivot = col3(f, 3);
    const dist = Math.max(length(sub(pivot, cam.pos)), 0.5);
    const size = dist * Math.tan(radians(cam.fov_deg) * 0.5) * 0.24;
    const view = normalize(sub(pivot, cam.pos));
    return { f, size, view, pivot };
  }

  private rayOnDragPlane(ray: Ray): Vec3 | null {
    const t = rayPlane(ray, this.plane_point, this.plane_normal);
    return t === null || t < 0 ? null : rayAt(ray, t);
  }

  private planeQuad(
    project: Project,
    f: Mat4,
    pivot: Vec3,
    size: number,
    k: number,
  ): Vec2[] | null {
    const ai = col3(f, (k + 1) % 3),
      aj = col3(f, (k + 2) % 3);
    const at = (a: number, b: number) =>
      project(add(pivot, mul(add(mul(ai, a), mul(aj, b)), size)));
    const q = [at(0.18, 0.18), at(0.42, 0.18), at(0.42, 0.42), at(0.18, 0.42)];
    return q.every((p) => p) ? (q as Vec2[]) : null;
  }

  private ring(
    project: Project,
    f: Mat4,
    pivot: Vec3,
    size: number,
    i: number,
  ): (Vec2 | null)[] {
    const a = col3(f, i);
    const u = perpendicular(a),
      v = cross(a, u);
    const pts: (Vec2 | null)[] = [];
    for (let k = 0; k <= 64; k++) {
      const ang = (k / 64) * 2 * kPi;
      pts.push(
        project(
          add(
            pivot,
            mul(add(mul(u, Math.cos(ang)), mul(v, Math.sin(ang))), size),
          ),
        ),
      );
    }
    return pts;
  }

  private hitTest(
    project: Project,
    m: Vec2,
    f: Mat4,
    size: number,
    view: Vec3,
  ): number {
    const pivot = col3(f, 3);
    const o = project(pivot);
    if (!o) return -1;
    if (v2len(v2sub(m, o)) < kCenterPick && this.op !== GizmoOp.Rotate)
      return 6;
    if (this.op === GizmoOp.Rotate) {
      let best = -1,
        best_d = kAxisPick;
      for (let i = 0; i < 3; i++) {
        const pts = this.ring(project, f, pivot, size, i);
        for (let k = 1; k < pts.length; k++) {
          const a = pts[k - 1],
            b = pts[k];
          if (!a || !b) continue;
          const d = distPointSegment(m, a, b);
          if (d < best_d) {
            best_d = d;
            best = i;
          }
        }
      }
      return best;
    }
    if (this.op === GizmoOp.Translate) {
      for (let k = 0; k < 3; k++) {
        if (!planeUsable(col3(f, k), view)) continue;
        const q = this.planeQuad(project, f, pivot, size, k);
        if (q && pointInQuad(m, q)) return 3 + k;
      }
    }
    let best = -1,
      best_d = kAxisPick;
    for (let i = 0; i < 3; i++) {
      if (!axisUsable(col3(f, i), view)) continue;
      const e = project(add(pivot, mul(col3(f, i), size)));
      if (!e) continue;
      const d = distPointSegment(m, o, e);
      if (d < best_d) {
        best_d = d;
        best = i;
      }
    }
    return best;
  }

  // `frame`: pivot position (column 3) and orientation (columns 0-2, used in Local space)
  update(
    cam: Camera,
    project: Project,
    input: GizmoInput,
    frame: Mat4,
  ): GizmoResult {
    const res: GizmoResult = {
      started: false,
      dragging: false,
      finished: false,
      delta: identity(),
      scale: [1, 1, 1],
    };
    const { f, size, view, pivot } = this.place(cam, frame);
    this.mouse = input.mouse;
    const ray_of = () =>
      cam.ray(
        input.mouse[0],
        input.mouse[1],
        input.viewportSize[0],
        input.viewportSize[1],
      );

    if (this.active_ < 0) {
      this.readout_ = "";
      this.hot = input.mouseInViewport
        ? this.hitTest(project, input.mouse, f, size, view)
        : -1;
      if (input.pressed && this.hot >= 0) {
        this.active_ = this.hot;
        this.start_frame = f;
        this.start_mouse = this.last_mouse = input.mouse;
        this.plane_point = pivot;
        this.raw_move = [0, 0, 0];
        this.move = [0, 0, 0];
        this.raw_angle = this.angle = 0;
        this.raw_scale = this.scale_ = 1;
        this.edge_on = false;
        const a = this.active_;
        if (this.op === GizmoOp.Rotate) {
          this.plane_normal = col3(f, a);
        } else if (this.op === GizmoOp.Scale && a === 6) {
          this.plane_normal = mul(cam.forward(), -1);
        } else if (a < 3) {
          const ax = col3(f, a);
          const n = cross(ax, cross(view, ax));
          this.plane_normal =
            length2(n) > 1e-8 ? normalize(n) : perpendicular(ax);
        } else if (a < 6) {
          this.plane_normal = col3(f, a - 3);
        } else {
          this.plane_normal = mul(cam.forward(), -1);
        }
        if (
          this.op === GizmoOp.Rotate &&
          Math.abs(dot(this.plane_normal, view)) < 0.25
        ) {
          // a ring seen edge-on: the mouse runs along it on screen, where it was grabbed
          const ax = this.plane_normal;
          let radial = sub(cam.pos, pivot);
          radial = sub(radial, mul(ax, dot(radial, ax)));
          radial =
            length2(radial) > 1e-8 ? normalize(radial) : perpendicular(ax);
          const at = add(pivot, mul(radial, size));
          const p0 = project(at),
            p1 = project(add(at, mul(cross(ax, radial), size * 0.1))),
            o = project(pivot);
          if (p0 && p1 && o) {
            const t = v2sub(p1, p0);
            if (v2len(t) > 1e-3) {
              this.edge_on = true;
              this.tangent = [t[0] / v2len(t), t[1] / v2len(t)];
              this.radius_px = Math.max(v2len(v2sub(p0, o)), 20);
              this.start_hit = this.last_hit = at;
              res.started = true;
            }
          }
          if (!this.edge_on) this.active_ = -1;
        } else {
          const hit = this.rayOnDragPlane(ray_of());
          if (!hit) {
            this.active_ = -1;
          } else {
            this.start_hit = this.last_hit = hit;
            res.started = true;
          }
        }
      }
    }

    if (this.active_ >= 0) {
      const a = this.active_;
      const snapping = this.snap.enabled !== input.snapToggle;
      const k_fine = input.fine ? 0.1 : 1;
      if (input.down) {
        const sf = this.start_frame;
        const sp = col3(sf, 3);
        res.dragging = true;
        const p = this.edge_on ? null : this.rayOnDragPlane(ray_of());
        let text = "";
        if (this.op === GizmoOp.Scale && p) {
          // along an axis: how far the handle went, in sizes of the gizmo; all axes: the mouse
          // going up (or right) grows, down shrinks
          const d = sub(p, this.start_hit);
          let raw = 1;
          if (a < 3) {
            raw = 1 + dot(d, col3(sf, a)) / size;
          } else {
            const md = v2sub(input.mouse, this.start_mouse);
            raw = Math.exp((md[0] - md[1]) * 0.01);
          }
          this.scale_ += (raw - this.raw_scale) * k_fine;
          this.raw_scale = raw;
          let k = snapping
            ? snapValue(this.scale_, this.snap.scale)
            : this.scale_;
          k = Math.max(k, 0.01);
          res.scale =
            a < 3
              ? [a === 0 ? k : 1, a === 1 ? k : 1, a === 2 ? k : 1]
              : [k, k, k];
          text = `${a < 3 ? NAMES[a] : "XYZ"} x${k.toFixed(2)}`;
        } else if (this.op === GizmoOp.Translate && p) {
          const d = sub(p, this.start_hit);
          let raw: Vec3 = [0, 0, 0];
          if (a < 3) {
            const ax = col3(sf, a);
            raw = mul(ax, dot(d, ax));
          } else if (a < 6) {
            for (let i = 0; i < 3; i++)
              if (i !== a - 3)
                raw = add(raw, mul(col3(sf, i), dot(d, col3(sf, i))));
          } else {
            raw = d;
          }
          this.move = add(this.move, mul(sub(raw, this.raw_move), k_fine));
          this.raw_move = raw;
          let offset = this.move;
          if (snapping && this.snap.translate > 0) {
            if (this.space === GizmoSpace.World) {
              // the pivot on the grid, along the axes dragged
              const to = add(sp, offset);
              for (let i = 0; i < 3; i++) {
                const moved = a === 6 || (a < 3 ? i === a : i !== a - 3);
                to[i] = moved ? snapValue(to[i], this.snap.translate) : sp[i];
              }
              offset = sub(to, sp);
            } else {
              let snapped: Vec3 = [0, 0, 0];
              for (let i = 0; i < 3; i++)
                snapped = add(
                  snapped,
                  mul(
                    col3(sf, i),
                    snapValue(dot(offset, col3(sf, i)), this.snap.translate),
                  ),
                );
              offset = snapped;
            }
          }
          res.delta = translate(offset);
          text =
            a < 3
              ? `${NAMES[a]} ${fmt(dot(offset, col3(sf, a)), 2)} m`
              : `${fmt(offset[0], 2)}  ${fmt(offset[1], 2)}  ${fmt(offset[2], 2)} m`;
        } else if (this.op === GizmoOp.Rotate) {
          const ax = col3(sf, a);
          let step = 0;
          if (this.edge_on) {
            // along the ring's tangent on screen: its radius in pixels makes a radian
            const md = v2sub(input.mouse, this.last_mouse);
            step =
              (md[0] * this.tangent[0] + md[1] * this.tangent[1]) /
              this.radius_px;
          } else if (p) {
            // the angle swept since the last move, so that going round more than half a turn goes on
            const v0 = sub(this.last_hit, sp),
              v1 = sub(p, sp);
            if (length2(v0) > 1e-10 && length2(v1) > 1e-10)
              step = wrapAngle(Math.atan2(dot(cross(v0, v1), ax), dot(v0, v1)));
            this.last_hit = p;
          }
          this.raw_angle += step;
          this.angle += step * k_fine;
          let ang = this.angle;
          if (snapping)
            ang = radians(snapValue(degrees(ang), this.snap.rotate_deg));
          res.delta = matMul(
            matMul(translate(sp), rotate(axisAngle(ax, ang))),
            translate(mul(sp, -1)),
          );
          text = `${NAMES[a]} ${fmt(degrees(ang), 1)} deg`;
        }
        this.last_mouse = input.mouse;
        if (text) {
          this.readout_ = text;
          if (snapping) this.readout_ += "  [snap]";
          if (input.fine) this.readout_ += "  [fine]";
        }
      } else {
        res.finished = true;
        res.dragging = false;
        this.active_ = -1;
      }
    }
    return res;
  }

  // the gizmo on the overlay canvas (CSS pixels), with the drag's value next to the cursor
  draw(
    ctx: CanvasRenderingContext2D,
    cam: Camera,
    project: Project,
    frame: Mat4,
  ) {
    const { f, size, view, pivot } = this.place(cam, frame);
    const o = project(pivot);
    if (!o) return;
    const hot = this.active_ >= 0 ? this.active_ : this.hot;
    const line = (a: Vec2, b: Vec2, color: string, w: number) => {
      ctx.strokeStyle = color;
      ctx.lineWidth = w;
      ctx.beginPath();
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
      ctx.stroke();
    };
    const fill = (pts: Vec2[], color: string) => {
      ctx.fillStyle = color;
      ctx.beginPath();
      pts.forEach((p, i) =>
        i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1]),
      );
      ctx.closePath();
      ctx.fill();
    };
    const square = (c: Vec2, r: number, color: string) => {
      ctx.fillStyle = color;
      ctx.fillRect(c[0] - r, c[1] - r, r * 2, r * 2);
    };
    ctx.save();
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    // the axis being dragged along: a line through the pivot, to see where it goes
    if (this.active_ >= 0 && this.active_ < 3 && this.op !== GizmoOp.Rotate) {
      const d = col3(this.start_frame, this.active_);
      const a = project(sub(pivot, mul(d, size * 40))),
        b = project(add(pivot, mul(d, size * 40)));
      if (a && b) line(a, b, axisColor(this.active_, false, 0x90), 1.5);
    }
    if (this.op === GizmoOp.Scale) {
      // an axis with a square at its end for each direction, a square in the center for all
      for (let i = 0; i < 3; i++) {
        if (!axisUsable(col3(f, i), view) && hot !== i) continue;
        const e = project(add(pivot, mul(col3(f, i), size)));
        if (!e) continue;
        const c = axisColor(i, hot === i);
        line(o, e, c, hot === i ? 4 : 2.8);
        square(e, 5, c);
      }
      square(o, 6, hot === 6 ? rgba(255, 220, 70) : rgba(235, 235, 235, 230));
    } else if (this.op === GizmoOp.Rotate) {
      for (let i = 0; i < 3; i++) {
        const pts = this.ring(project, f, pivot, size, i).filter(
          (p): p is Vec2 => !!p,
        );
        if (pts.length < 2) continue;
        ctx.strokeStyle = axisColor(i, hot === i);
        ctx.lineWidth = hot === i ? 3.5 : 2.2;
        ctx.beginPath();
        pts.forEach((p, k) =>
          k ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1]),
        );
        ctx.stroke();
      }
      // the turn so far: from where the ring was grabbed to where it is now
      if (
        this.active_ >= 0 &&
        this.active_ < 3 &&
        Math.abs(this.angle) > 1e-4
      ) {
        const a = col3(this.start_frame, this.active_);
        let from = sub(this.start_hit, col3(this.start_frame, 3));
        from = sub(from, mul(a, dot(from, a)));
        if (length2(from) > 1e-8) {
          from = normalize(from);
          const side = cross(a, from);
          const fan: Vec2[] = [o];
          const steps = Math.max(
            2,
            Math.floor((Math.abs(this.angle) / kPi) * 32),
          );
          for (let k = 0; k <= steps; k++) {
            const t = (this.angle * k) / steps;
            const s = project(
              add(
                pivot,
                mul(add(mul(from, Math.cos(t)), mul(side, Math.sin(t))), size),
              ),
            );
            if (s) fan.push(s);
          }
          if (fan.length > 2) fill(fan, rgba(255, 220, 70, 50));
        }
      }
      ctx.fillStyle = rgba(240, 240, 240, 220);
      ctx.beginPath();
      ctx.arc(o[0], o[1], 3, 0, 2 * kPi);
      ctx.fill();
    } else {
      // plane handles
      for (let k = 0; k < 3; k++) {
        if (!planeUsable(col3(f, k), view) && hot !== 3 + k) continue;
        const q = this.planeQuad(project, f, pivot, size, k);
        if (q)
          fill(
            q,
            hot === 3 + k ? rgba(255, 220, 70, 150) : axisColor(k, false, 0x70),
          );
      }
      for (let i = 0; i < 3; i++) {
        if (!axisUsable(col3(f, i), view) && hot !== i) continue;
        const e = project(add(pivot, mul(col3(f, i), size)));
        if (!e) continue;
        const c = axisColor(i, hot === i);
        line(o, e, c, hot === i ? 4 : 2.8);
        const d = v2sub(e, o);
        const len = v2len(d);
        if (len > 1) {
          const dr: Vec2 = [d[0] / len, d[1] / len],
            nr: Vec2 = [-dr[1], dr[0]];
          const base: Vec2 = [e[0] - dr[0] * 13, e[1] - dr[1] * 13];
          fill(
            [
              [e[0] + dr[0] * 3, e[1] + dr[1] * 3],
              [base[0] + nr[0] * 6, base[1] + nr[1] * 6],
              [base[0] - nr[0] * 6, base[1] - nr[1] * 6],
            ],
            c,
          );
        }
      }
      ctx.strokeStyle =
        hot === 6 ? rgba(255, 220, 70) : rgba(235, 235, 235, 230);
      ctx.lineWidth = 2;
      ctx.strokeRect(o[0] - 6, o[1] - 6, 12, 12);
    }
    // the value next to the cursor
    if (this.active_ >= 0 && this.readout_) {
      ctx.font = "13px sans-serif";
      ctx.textBaseline = "top";
      const at: Vec2 = [this.mouse[0] + 18, this.mouse[1] + 14];
      const w = ctx.measureText(this.readout_).width;
      ctx.fillStyle = rgba(12, 14, 16, 210);
      ctx.beginPath();
      ctx.roundRect(at[0] - 4, at[1] - 2, w + 8, 13 + 4 + 2, 3);
      ctx.fill();
      ctx.fillStyle = rgba(255, 225, 120);
      ctx.fillText(this.readout_, at[0], at[1]);
    }
    ctx.restore();
  }
}

// ---------------------------------------------------------------------------------------------
// The gizmo on the viewport: the mouse drives it, the nodes move in the view while it is dragged
// (SceneStore.setPreview) and the engine gets the new matrices when it is released, one call:
// call("transform_nodes", {ids, worlds, label}), one undo step.

// what the controller needs of the viewport (viewport_api.ts)
export interface GizmoHost {
  camera: Camera;
  canvas: HTMLCanvasElement;
  store: SceneStore;
  project: Project;
  size(): Vec2; // CSS pixels
  requestRedraw(): void;
  addOverlay2d(fn: (ctx: CanvasRenderingContext2D) => void): () => void;
}

export interface GizmoCallbacks {
  // the roots the drag moves and the frame the gizmo stands on; null: no gizmo. Default: the
  // selection's roots (JLES flag 32) and SceneStore.selectionFrame(primary)
  targets?: () => { ids: number[]; frame: Mat4 } | null;
  primary?: () => number;
  // the drag began / goes on (the worlds the roots have now; the default previews them)
  onStart?: (ids: number[]) => void;
  onDrag?: (worlds: Map<number, Mat4>) => void;
  // the drag ended: the default sends transform_nodes and keeps the preview until the scene's
  // answer comes back. label: "undo.move", "undo.rotate" or "undo.scale" (translation keys)
  onEnd?: (
    ids: number[],
    worlds: Mat4[],
    label: string,
  ) => Promise<void> | void;
}

export class GizmoController {
  readonly gizmo = new Gizmo();
  enabled = true;
  private start = new Map<number, Mat4>();
  private moved = false;
  private off: (() => void)[] = [];

  constructor(
    private host: GizmoHost,
    public callbacks: GizmoCallbacks = {},
  ) {
    const c = host.canvas;
    const on = (
      type: string,
      fn: (e: PointerEvent) => void,
      capture: boolean,
    ) => {
      c.addEventListener(type, fn as EventListener, capture);
      this.off.push(() =>
        c.removeEventListener(type, fn as EventListener, capture),
      );
    };
    // in the capture phase: a click the gizmo takes reaches no tool (the C++ returns before them)
    on("pointerdown", (e) => this.event(e, true), true);
    on("pointermove", (e) => this.event(e, false), true);
    on("pointerup", (e) => this.event(e, false), true);
    on(
      "pointerleave",
      () => {
        if (!this.gizmo.active() && this.gizmo.hovering())
          this.host.requestRedraw();
      },
      false,
    );
    this.off.push(
      host.addOverlay2d((ctx) => {
        const t = this.enabled ? this.targets() : null;
        if (t)
          this.gizmo.draw(ctx, this.host.camera, this.host.project, t.frame);
      }),
    );
  }

  dispose() {
    for (const f of this.off) f();
  }

  private targets() {
    if (this.callbacks.targets) return this.callbacks.targets();
    const ids = this.host.store.selectionRoots();
    return ids.length
      ? {
          ids,
          frame: this.host.store.selectionFrame(
            this.callbacks.primary?.() ?? 0,
          ),
        }
      : null;
  }

  private event(e: PointerEvent, pressed: boolean) {
    if (
      !this.enabled ||
      (e.pointerType === "mouse" && pressed && e.button !== 0)
    )
      return;
    const t = this.targets();
    if (!t) return;
    const r = this.host.canvas.getBoundingClientRect();
    const mouse: Vec2 = [e.clientX - r.left, e.clientY - r.top];
    const size = this.host.size();
    const was_hot = this.gizmo.hovering();
    const res = this.gizmo.update(
      this.host.camera,
      this.host.project,
      {
        mouse,
        viewportSize: size,
        mouseInViewport:
          mouse[0] >= 0 &&
          mouse[1] >= 0 &&
          mouse[0] < size[0] &&
          mouse[1] < size[1] &&
          !(e.buttons & 6),
        pressed: pressed && !e.altKey,
        down: (e.buttons & 1) !== 0,
        fine: e.shiftKey,
        snapToggle: e.ctrlKey,
      },
      t.frame,
    );
    const store = this.host.store;
    if (res.started) {
      this.start.clear();
      this.moved = false;
      for (const id of t.ids) {
        const n = store.nodes.get(id);
        if (n) this.start.set(id, new Float32Array(store.world(n)));
      }
      this.host.canvas.setPointerCapture(e.pointerId);
      this.callbacks.onStart?.(t.ids);
    }
    if (res.dragging) {
      const worlds = new Map<number, Mat4>();
      for (const [id, s] of this.start)
        // scale: each element along its own axes, around its origin
        worlds.set(
          id,
          this.gizmo.op === GizmoOp.Scale
            ? matMul(s, scale(res.scale))
            : matMul(res.delta, s),
        );
      this.moved ||= [...worlds].some(([id, w]) =>
        w.some((v, i) => v !== this.start.get(id)![i]),
      );
      if (this.callbacks.onDrag) this.callbacks.onDrag(worlds);
      else store.setPreview(worlds);
      this.last = worlds;
    }
    if (res.finished) void this.finish();
    if (
      res.started ||
      this.gizmo.active() ||
      (pressed && this.gizmo.hovering())
    ) {
      // the gizmo takes the click
      e.preventDefault();
      e.stopImmediatePropagation();
    }
    if (
      res.started ||
      res.dragging ||
      res.finished ||
      was_hot !== this.gizmo.hovering()
    )
      this.host.requestRedraw();
  }

  private last: Map<number, Mat4> | null = null;

  private async finish() {
    const worlds = this.last;
    this.last = null;
    const label =
      this.gizmo.op === GizmoOp.Translate
        ? "undo.move"
        : this.gizmo.op === GizmoOp.Rotate
          ? "undo.rotate"
          : "undo.scale";
    const store = this.host.store;
    if (!worlds || !this.moved) {
      if (!this.callbacks.onDrag) store.setPreview(null);
      return;
    }
    const ids = [...worlds.keys()],
      list = [...worlds.values()];
    try {
      if (this.callbacks.onEnd) {
        await this.callbacks.onEnd(ids, list, label);
      } else {
        await call("transform_nodes", {
          ids,
          worlds: list.map((w) => Array.from(w)),
          label,
        });
        await store.refresh();
      }
    } catch (err) {
      console.warn("[viewport] transform_nodes:", err);
    }
    if (!this.callbacks.onDrag) store.setPreview(null);
  }
}
