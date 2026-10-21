// The camera's controls (src/app/app.cpp update_camera): the right button held looks around and
// flies with WASD / QE (Shift faster, Ctrl slower, the wheel sets the speed), the wheel moves
// forward, the middle button pans, Alt + left button orbits. frameAll / frameSelection: Home and
// F (the frame's shortcuts send them as the window event jle:frame).

import type { Camera } from "./camera";
import {
  aabbValid,
  add,
  clamp,
  col3,
  length,
  length2,
  mul,
  normalize,
  sub,
  type Vec3,
} from "./math";
import { shortcuts } from "../lib/shortcuts";
import type { SceneStore } from "./scene_store";

export interface ControlsHooks {
  // the right button pressed over the view: true when a tool takes it (the C++ start_free_rotation:
  // on the selection it turns it)
  rightPress?: (x: number, y: number, e: MouseEvent) => boolean;
  // the wheel over the view: true when a tool takes it (Ctrl + wheel turns what is placed)
  wheel?: (wheel: number, e: MouseEvent) => boolean;
}

const typing = (e: Event) => {
  const t = e.target as HTMLElement | null;
  return (
    !!t &&
    (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName))
  );
};

/**
 * Locks the pointer to the view while `wanted()` (the cursor stays where it is, the mouse turns
 * as far as wanted: SDL's relative mode); raw moves unless `raw` is false. The lock comes later
 * than the press: one that arrives once the button is let go is given back at once.
 */
export function lockPointer(
  el: HTMLElement,
  wanted: () => boolean,
  raw = true,
): void {
  const lock = (o?: object) =>
    Promise.resolve((el as any).requestPointerLock?.(o));
  (raw ? lock({ unadjustedMovement: true }).catch(() => lock()) : lock())
    .then(() => {
      if (!wanted() && document.pointerLockElement === el)
        document.exitPointerLock();
    })
    .catch(() => {});
}

export class CameraControls {
  looking = false;
  panning = false;
  orbiting = false;
  hooks: ControlsHooks = {};
  private keys = new Set<string>();
  private orbit_pivot: Vec3 = [0, 0, 0];
  private orbit_dist = 1;
  // the pivot known (picked after the press): the moves before it are not an orbit around another
  private orbit_ready = false;
  private off: (() => void)[] = [];

  constructor(
    private cam: Camera,
    private el: HTMLElement,
    private store: SceneStore,
    // the camera moved (the view draws again)
    private changed: () => void,
    // the surface under a point of the view (the orbit's pivot), null when none
    private pickPoint: (x: number, y: number) => Promise<Vec3 | null>,
  ) {
    const on = <K extends keyof WindowEventMap>(
      t: EventTarget,
      type: K,
      fn: (e: WindowEventMap[K]) => void,
      opts?: AddEventListenerOptions,
    ) => {
      t.addEventListener(type, fn as EventListener, opts);
      this.off.push(() =>
        t.removeEventListener(type, fn as EventListener, opts),
      );
    };
    on(el, "mousedown", (e) => this.down(e));
    on(window, "mouseup", (e) => this.up(e));
    on(window, "mousemove", (e) => this.move(e));
    on(el, "wheel", (e) => this.wheel(e), { passive: false });
    on(el, "contextmenu", (e) => e.preventDefault());
    // in the capture phase: while looking the keys are the camera's, no shortcut sees them
    on(window, "keydown", (e) => this.key(e, true), { capture: true });
    on(window, "keyup", (e) => this.key(e, false), { capture: true });
    on(window, "blur", () => this.keys.clear());
  }

  dispose() {
    for (const f of this.off) f();
    this.stopLooking();
  }

  // a camera move going on: the view draws every frame
  get busy(): boolean {
    return this.looking || this.panning || this.orbiting;
  }

  private local(e: MouseEvent): [number, number] {
    const r = this.el.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  private down(e: MouseEvent) {
    const [x, y] = this.local(e);
    if (e.button === 2 && !this.busy) {
      // on the selection (or what is placed) the right button turns it, elsewhere it looks around
      if (this.hooks.rightPress?.(x, y, e)) return;
      this.looking = true;
      lockPointer(this.el as HTMLElement, () => this.looking);
      this.changed();
    } else if (e.button === 1 && !this.busy) {
      // middle button: pan
      e.preventDefault();
      this.panning = true;
    } else if (e.button === 0 && e.altKey && !this.busy) {
      // Alt + left button: orbit around the selection or the point under the cursor
      e.preventDefault();
      this.orbiting = true;
      this.orbit_ready = false;
      void this.startOrbit(x, y);
    }
  }

  private async startOrbit(x: number, y: number) {
    const sel = this.store.selectionRoots();
    let pivot: Vec3 | null = sel.length
      ? col3(this.store.selectionFrame(), 3)
      : await this.pickPoint(x, y);
    pivot ??= add(this.cam.pos, mul(this.cam.forward(), 10));
    if (!this.orbiting) return;
    this.orbit_pivot = pivot;
    this.orbit_dist = Math.max(length(sub(pivot, this.cam.pos)), 0.5);
    this.orbit_ready = true;
    this.cam.lookAt(pivot);
    this.changed();
  }

  private stopLooking() {
    if (!this.looking) return;
    this.looking = false;
    if (document.pointerLockElement === this.el) document.exitPointerLock();
  }

  private up(e: MouseEvent) {
    if (e.button === 2) this.stopLooking();
    if (e.button === 1) this.panning = false;
    if (e.button === 0) this.orbiting = false;
    this.changed();
  }

  private move(e: MouseEvent) {
    const dx = e.movementX,
      dy = e.movementY;
    const cam = this.cam;
    if (this.looking) {
      cam.yaw -= dx * 0.0035;
      cam.pitch = clamp(cam.pitch - dy * 0.0035, -1.55, 1.55);
    } else if (this.panning) {
      const k = Math.max(cam.speed, 1) * 0.006;
      cam.pos = add(
        sub(cam.pos, mul(cam.right(), dx * k)),
        mul(cam.up(), dy * k),
      );
    } else if (this.orbiting) {
      if (!this.orbit_ready) return;
      cam.yaw -= dx * 0.005;
      cam.pitch = clamp(cam.pitch - dy * 0.005, -1.55, 1.55);
      cam.pos = sub(this.orbit_pivot, mul(cam.forward(), this.orbit_dist));
    } else {
      return;
    }
    this.changed();
  }

  private wheel(e: WheelEvent) {
    e.preventDefault();
    // ImGui's wheel: 1 a notch (100 pixels, or 3 lines)
    const wheel =
      -e.deltaY / (e.deltaMode === 1 ? 3 : e.deltaMode === 2 ? 0.1 : 100);
    if (!wheel) return;
    const cam = this.cam;
    if (this.looking) {
      cam.speed = clamp(cam.speed * Math.pow(1.2, wheel), 0.5, 2000);
      return;
    }
    // the right button turning the selection, what is placed, raising the ground: not the camera
    if (shortcuts.paused) return;
    if (this.hooks.wheel?.(wheel, e)) return;
    // wheel: move forward
    cam.pos = add(
      cam.pos,
      mul(cam.forward(), wheel * Math.max(cam.speed * 0.35, 0.4)),
    );
    this.changed();
  }

  private key(e: KeyboardEvent, down: boolean) {
    // physical keys: WASD on QWERTY is ZQSD on AZERTY
    if (down) this.keys.add(e.code);
    else this.keys.delete(e.code);
    if (this.looking && down && !typing(e)) e.preventDefault();
  }

  // flies with the keys while looking; true when the camera moved
  tick(dt: number): boolean {
    if (!this.looking) return false;
    const cam = this.cam,
      k = this.keys;
    let mv: Vec3 = [0, 0, 0];
    const fwd = cam.forward(),
      right = cam.right();
    if (k.has("KeyW")) mv = add(mv, fwd);
    if (k.has("KeyS")) mv = sub(mv, fwd);
    if (k.has("KeyD")) mv = add(mv, right);
    if (k.has("KeyA")) mv = sub(mv, right);
    if (k.has("KeyE") || k.has("Space")) mv = add(mv, [0, 1, 0]);
    if (k.has("KeyQ") || k.has("KeyC")) mv = sub(mv, [0, 1, 0]);
    let sp = cam.speed;
    if (k.has("ShiftLeft") || k.has("ShiftRight")) sp *= 4;
    if (k.has("ControlLeft")) sp *= 0.25;
    if (length2(mv) === 0) return false;
    cam.pos = add(cam.pos, mul(normalize(mv), sp * dt));
    this.changed();
    return true;
  }

  frameAll() {
    this.cam.yaw = 0.6;
    this.cam.pitch = -0.45;
    this.cam.frame(this.store.focusBounds());
    this.changed();
  }

  frameSelection() {
    const b = this.store.selectionBounds();
    if (!aabbValid(b)) return;
    this.cam.frame(b);
    this.changed();
  }
}
