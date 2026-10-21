// Free-flying editor camera (yaw/pitch), Y up, meters (src/render/camera.h).

import {
  aabbCenter,
  aabbRadius,
  aabbValid,
  clamp,
  cross,
  lookAt,
  mul,
  add,
  normalize,
  perspective,
  radians,
  sub,
  type AABB,
  type Mat4,
  type Ray,
  type Vec3,
} from "./math";

export class Camera {
  pos: Vec3 = [0, 10, 30];
  yaw = 0; // radians, 0 looks down -Z
  pitch = -0.3;
  fov_deg = 60;
  znear = 0.1;
  zfar = 20000;
  speed = 12; // m/s

  forward(): Vec3 {
    const cp = Math.cos(this.pitch);
    return [
      -Math.sin(this.yaw) * cp,
      Math.sin(this.pitch),
      -Math.cos(this.yaw) * cp,
    ];
  }
  right(): Vec3 {
    return normalize(cross(this.forward(), [0, 1, 0]));
  }
  up(): Vec3 {
    return cross(this.right(), this.forward());
  }
  view(): Mat4 {
    return lookAt(this.pos, add(this.pos, this.forward()), [0, 1, 0]);
  }
  proj(aspect: number): Mat4 {
    return perspective(radians(this.fov_deg), aspect, this.znear, this.zfar);
  }
  lookAt(target: Vec3) {
    const d = normalize(sub(target, this.pos));
    this.pitch = Math.asin(clamp(d[1], -0.999, 0.999));
    this.yaw = Math.atan2(-d[0], -d[2]);
  }
  // Ray through a pixel of a viewport of size w x h (pixels, origin top-left)
  ray(px: number, py: number, w: number, h: number): Ray {
    const ndc_x = (px / w) * 2 - 1;
    const ndc_y = 1 - (py / h) * 2;
    const t = Math.tan(radians(this.fov_deg) * 0.5);
    const aspect = w / h;
    const d = normalize(
      add(
        add(this.forward(), mul(this.right(), ndc_x * t * aspect)),
        mul(this.up(), ndc_y * t),
      ),
    );
    return { origin: [...this.pos], dir: d };
  }
  // Frames a box: keeps the orientation, backs off so the box fits.
  frame(box: AABB) {
    if (!aabbValid(box)) return;
    const r = Math.max(aabbRadius(box), 0.5);
    const dist = (r / Math.tan(radians(this.fov_deg) * 0.5)) * 1.1;
    this.pos = sub(aabbCenter(box), mul(this.forward(), dist));
    this.speed = clamp(r * 0.15, 2, 150);
  }
  copy(): Camera {
    return Object.assign(new Camera(), this, { pos: [...this.pos] as Vec3 });
  }
}
