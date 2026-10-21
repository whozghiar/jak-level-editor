// What the viewport draws, as the engine last answered it: the nodes by id, the meshes by uid and
// version, the textures by index and version, the materials. Kept up to date on jle:changed.

import { onEngine } from "./engine";
import {
  aabb,
  aabbAdd,
  aabbAddBox,
  aabbCenter,
  aabbContains,
  aabbExpanded,
  aabbTransformed,
  aabbValid,
  add,
  col3,
  focusBox,
  identity,
  inverse,
  matMul,
  mul,
  normalize,
  point,
  setCol3,
  type AABB,
  type Mat4,
  type Vec3,
} from "./math";
import {
  fetchMaterials,
  fetchMesh,
  fetchScene,
  fetchTexture,
  type MeshData,
  type TextureData,
} from "./protocol";
import {
  K_MARKER_RADIUS,
  NodeFlag,
  NodeKind,
  type Material,
  type NodeRecord,
} from "./types";

export interface MeshEntry {
  uid: number;
  version: number; // the version wanted (the node records')
  data: MeshData | null; // what was fetched last (an older version until the new one arrives)
  loading: boolean;
  failed: number; // the version that could not be read (not asked again)
  refs: number; // nodes using it, and retainMesh
  gpu: unknown; // the renderer's buffers of `data`
}

export interface TextureEntry {
  version: number; // wanted
  loaded: number; // of `data` (-1: none yet)
  data: TextureData | null;
  loading: boolean;
  gpu: unknown;
}

const MESH_FETCHES = 8; // in flight at once

export class SceneStore {
  nodes = new Map<number, NodeRecord>();
  meshes = new Map<number, MeshEntry>();
  textures: TextureEntry[] = [];
  materials: Material[] = [];
  revision = 0; // bumped on every change (the viewport redraws)
  // the renderer frees what it made of a mesh or a texture no longer drawn
  freeGpu: (gpu: unknown) => void = () => {};

  // derived from the nodes, rebuilt after a change
  private dirty = true;
  private _list: NodeRecord[] = [];
  private _children = new Map<number, number[]>();
  private _with_model = new Set<number>(); // actors that have a model under them (no marker)
  private _water = false;
  private node_bounds = new Map<number, AABB>();
  // a drag's preview: the worlds of the nodes moved (their subtrees included); setPreview changes it
  readonly preview = new Map<number, Mat4>();
  private listeners = new Set<() => void>();
  private queue: MeshEntry[] = [];
  private fetching = 0;
  private scene_busy: Promise<void> | null = null;
  private idle_waiters: (() => void)[] = [];
  private scene_again = false;
  private full_again = false;

  // Subscribes to the engine's changes and reads the whole scene. Returns the unsubscribe.
  start(): () => void {
    const off = onEngine("jle:changed", (p: any) => {
      p ??= { scene: true, materials: true }; // no payload: anything may have changed
      if (p.document) this.reset();
      if (p.document || p.scene || p.selection) void this.refresh(!!p.document);
      if (p.document || p.materials || p.textures)
        void this.refreshMaterials(!!p.textures && !p.document);
    });
    void this.refresh(true);
    void this.refreshMaterials(false);
    return off;
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }
  private changed() {
    this.revision++;
    for (const fn of this.listeners) fn();
  }

  // forgets everything (another document)
  reset() {
    for (const m of this.meshes.values()) if (m.gpu) this.freeGpu(m.gpu);
    for (const t of this.textures) if (t.gpu) this.freeGpu(t.gpu);
    this.nodes.clear();
    this.meshes.clear();
    this.textures = [];
    this.node_bounds.clear();
    this.preview.clear();
    this.queue = [];
    this.dirty = true;
    this.changed();
  }

  // The scene again: what changed since the last answer (full: everything). One read at a time;
  // what changes meanwhile is read right after.
  refresh(full = false): Promise<void> {
    if (this.scene_busy) {
      this.scene_again = true;
      this.full_again ||= full;
      return this.scene_busy;
    }
    this.scene_busy = (async () => {
      try {
        do {
          const f = full || this.full_again;
          this.scene_again = this.full_again = false;
          full = false;
          try {
            this.apply(await fetchScene(f));
          } catch (e) {
            console.warn("[viewport] scene:", e);
          }
        } while (this.scene_again);
      } finally {
        this.scene_busy = null;
      }
    })();
    return this.scene_busy;
  }

  private apply(answer: {
    full: boolean;
    nodes: NodeRecord[];
    removed: Uint32Array;
  }) {
    if (answer.full) {
      const keep = new Set(answer.nodes.map((n) => n.id));
      for (const id of [...this.nodes.keys()])
        if (!keep.has(id)) this.removeNode(id);
    }
    for (const id of answer.removed) this.removeNode(id);
    for (const n of answer.nodes) {
      const old = this.nodes.get(n.id);
      if (old?.mesh !== n.mesh) {
        if (old?.mesh) this.unref(old.mesh);
        if (n.mesh) this.ref(n.mesh, n.mesh_version);
      } else if (n.mesh) {
        this.want(this.meshes.get(n.mesh)!, n.mesh_version);
      }
      this.nodes.set(n.id, n);
      this.node_bounds.delete(n.id);
    }
    this.dirty = true;
    this.changed();
  }

  private removeNode(id: number) {
    const old = this.nodes.get(id);
    if (!old) return;
    if (old.mesh) this.unref(old.mesh);
    this.nodes.delete(id);
    this.node_bounds.delete(id);
    this.preview.delete(id);
  }

  // A mesh drawn without a node (a preview, the ocean): kept while retained
  retainMesh(uid: number, version: number): MeshEntry {
    return this.ref(uid, version);
  }
  releaseMesh(uid: number) {
    this.unref(uid);
  }

  private ref(uid: number, version: number): MeshEntry {
    let m = this.meshes.get(uid);
    if (!m) {
      m = {
        uid,
        version,
        data: null,
        loading: false,
        failed: -1,
        refs: 0,
        gpu: null,
      };
      this.meshes.set(uid, m);
    }
    m.refs++;
    this.want(m, version);
    return m;
  }
  private unref(uid: number) {
    const m = this.meshes.get(uid);
    if (!m || --m.refs > 0) return;
    if (m.gpu) this.freeGpu(m.gpu);
    this.meshes.delete(uid);
  }
  private want(m: MeshEntry, version: number) {
    m.version = version;
    if (m.data?.version === version || m.loading || m.failed === version)
      return;
    m.loading = true;
    this.queue.push(m);
    this.pump();
  }
  private pump() {
    while (this.fetching < MESH_FETCHES && this.queue.length) {
      const m = this.queue.shift()!;
      if (this.meshes.get(m.uid) !== m) continue; // dropped meanwhile
      const version = m.version;
      this.fetching++;
      fetchMesh(m.uid, version)
        .then((data) => {
          if (this.meshes.get(m.uid) !== m) return;
          if (m.gpu) this.freeGpu(m.gpu);
          m.gpu = null;
          // the boxes of the old version (nothing is kept for a mesh not read yet)
          if (m.data)
            for (const id of this.node_bounds.keys())
              if (this.nodes.get(id)?.mesh === m.uid)
                this.node_bounds.delete(id);
          m.data = data;
          this.changed();
        })
        .catch((e) => {
          m.failed = version;
          console.warn(`[viewport] mesh ${m.uid}:`, e);
        })
        .finally(() => {
          m.loading = false;
          this.fetching--;
          // a newer version asked while this one was read
          if (
            this.meshes.get(m.uid) === m &&
            m.data?.version !== m.version &&
            m.failed !== m.version
          )
            this.want(m, m.version);
          this.pump();
          if (!this.fetching && !this.queue.length)
            for (const f of this.idle_waiters.splice(0)) f();
        });
    }
  }

  // resolves once the meshes asked for are read (their boxes known: what frameAll needs)
  meshesLoaded(): Promise<void> {
    return !this.fetching && !this.queue.length
      ? Promise.resolve()
      : new Promise((r) => this.idle_waiters.push(r));
  }

  // textures being read (a capture waits for them)
  texturesLoading(): boolean {
    return this.textures.some((t) => t.loading);
  }

  // materials (and the textures' versions): textures_changed with no versions given reads every
  // texture again
  async refreshMaterials(textures_changed: boolean) {
    try {
      const a = await fetchMaterials();
      this.materials = a.materials;
      if (a.textureVersions) {
        a.textureVersions.forEach((v, i) => (this.textureEntry(i).version = v));
      } else if (textures_changed) {
        for (const t of this.textures) t.version++;
      }
    } catch (e) {
      console.warn("[viewport] materials:", e);
    }
    this.changed();
  }

  private textureEntry(index: number): TextureEntry {
    while (this.textures.length <= index)
      this.textures.push({
        version: 0,
        loaded: -1,
        data: null,
        loading: false,
        gpu: null,
      });
    return this.textures[index];
  }

  // A texture to draw: read when first asked (null until it is there; the old one meanwhile)
  texture(index: number): TextureEntry | null {
    if (index < 0) return null;
    const t = this.textureEntry(index);
    if (t.loaded !== t.version && !t.loading) {
      t.loading = true;
      const version = t.version;
      fetchTexture(index)
        .then((data) => {
          t.data = data;
          t.loaded = version;
          if (t.gpu) this.freeGpu(t.gpu);
          t.gpu = null;
          this.changed();
        })
        .catch((e) => {
          t.loaded = version; // not asked again until it changes
          console.warn(`[viewport] texture ${index}:`, e);
        })
        .finally(() => (t.loading = false));
    }
    return t.data ? t : null;
  }

  // ---------------------------------------------------------------------------------------------
  // derived

  private derive() {
    if (!this.dirty) return;
    this.dirty = false;
    this._list = [...this.nodes.values()];
    this._children.clear();
    this._with_model.clear();
    this._water = false;
    for (const n of this._list) {
      if (n.parent) {
        const c = this._children.get(n.parent);
        if (c) c.push(n.id);
        else this._children.set(n.parent, [n.id]);
      }
      if (n.flags & NodeFlag.Water && n.flags & NodeFlag.Visible)
        this._water = true;
    }
    // an actor has a marker when it has no model (no node under it is one)
    for (const n of this._list) {
      if (!(n.flags & NodeFlag.UnderActor)) continue;
      for (
        let p = this.nodes.get(n.parent), guard = 0;
        p && guard < 1000;
        p = this.nodes.get(p.parent), guard++
      )
        if (p.kind === NodeKind.Actor) {
          this._with_model.add(p.id);
          break;
        }
    }
  }
  get list(): NodeRecord[] {
    this.derive();
    return this._list;
  }
  children(id: number): number[] {
    this.derive();
    return this._children.get(id) ?? [];
  }
  hasModel(actor: number): boolean {
    this.derive();
    return this._with_model.has(actor);
  }
  // something animates (a water surface shown): the viewport draws every frame
  get animated(): boolean {
    this.derive();
    return this._water;
  }

  // the node's world matrix as drawn (a drag's preview first)
  world(n: NodeRecord): Mat4 {
    return this.preview.get(n.id) ?? n.world;
  }
  meshOf(n: NodeRecord): MeshData | null {
    return n.mesh ? (this.meshes.get(n.mesh)?.data ?? null) : null;
  }
  // world box of a node with a mesh (invalid without one, or before its mesh is read)
  bounds(n: NodeRecord): AABB {
    const p = this.preview.get(n.id);
    if (p) {
      const d = this.meshOf(n);
      return d ? aabbTransformed(d.bounds, p) : aabb();
    }
    let b = this.node_bounds.get(n.id);
    if (!b) {
      const d = this.meshOf(n);
      b = d ? aabbTransformed(d.bounds, n.world) : aabb();
      if (d) this.node_bounds.set(n.id, b);
    }
    return b;
  }

  // the node and everything under it (ids), at most `cap`
  subtree(id: number, out: number[] = [], cap = Infinity): number[] {
    const stack = [id];
    while (stack.length && out.length < cap) {
      const i = stack.pop()!;
      if (!this.nodes.has(i)) continue;
      out.push(i);
      for (const k of this.children(i)) stack.push(k);
    }
    return out;
  }

  // ---------------------------------------------------------------------------------------------
  // the selection (JLES flag 32), as app.cpp / actions.cpp make it

  get selection(): number[] {
    return this.list
      .filter((n) => n.flags & NodeFlag.Selected)
      .map((n) => n.id);
  }
  // each selected node, unless one of its ancestors is selected too
  selectionRoots(): number[] {
    const out: number[] = [];
    for (const n of this.list) {
      if (!(n.flags & NodeFlag.Selected)) continue;
      let under = false;
      for (
        let p = this.nodes.get(n.parent), guard = 0;
        p && !under && guard < 1000;
        p = this.nodes.get(p.parent), guard++
      )
        under = (p.flags & NodeFlag.Selected) !== 0;
      if (!under) out.push(n.id);
    }
    return out;
  }

  subtreeBounds(id: number): AABB {
    const b = aabb();
    for (const i of this.subtree(id)) {
      const n = this.nodes.get(i)!;
      const w = this.world(n);
      if (n.mesh) {
        aabbAddBox(b, this.bounds(n));
      } else if (n.kind === NodeKind.Actor) {
        const p = col3(w, 3),
          r = K_MARKER_RADIUS;
        aabbAdd(b, add(p, [-r, -r, -r]));
        aabbAdd(b, add(p, [r, r, r]));
      } else {
        aabbAdd(b, col3(w, 3));
      }
    }
    return b;
  }

  selectionBounds(): AABB {
    const b = aabb();
    for (const id of this.selectionRoots())
      aabbAddBox(b, this.subtreeBounds(id));
    return b;
  }

  // a large group (a whole level): the middle of most of it, a stray piece far away left out
  private groupCenter(id: number): Vec3 {
    const boxes = this.subtree(id)
      .slice(1)
      .map((i) => this.bounds(this.nodes.get(i)!))
      .filter(aabbValid);
    const focus = focusBox(boxes);
    return aabbValid(focus)
      ? aabbCenter(focus)
      : col3(this.world(this.nodes.get(id)!), 3);
  }

  // The gizmo's frame: the middle of the selection, turned as the primary element (or the first)
  selectionFrame(primary = 0): Mat4 {
    const roots = this.selectionRoots();
    const frame = identity();
    if (!roots.length) return frame;
    let p: Vec3 = [0, 0, 0];
    for (const id of roots) {
      const origin = col3(this.world(this.nodes.get(id)!), 3);
      // more than 64 under it (subtree_size: the node itself not counted)
      if (this.subtree(id, [], 66).length > 65) {
        p = add(p, this.groupCenter(id));
        continue;
      }
      const b = this.subtreeBounds(id);
      // the node's origin, unless it is far from its geometry (meshes in world space have their
      // origin at 0,0,0): then the center of its box
      p = add(
        p,
        aabbValid(b) && !aabbContains(aabbExpanded(b, 1), origin)
          ? aabbCenter(b)
          : origin,
      );
    }
    p = mul(p, 1 / roots.length);
    const pn = this.nodes.get(primary);
    const ref = pn && pn.flags & NodeFlag.Selected ? primary : roots[0];
    const w = this.world(this.nodes.get(ref)!);
    for (let c = 0; c < 3; c++) setCol3(frame, c, normalize(col3(w, c)));
    setCol3(frame, 3, p);
    return frame;
  }

  // what Home frames: the world box of the nodes but the groups and the collision, most of them
  focusBounds(): AABB {
    const boxes: AABB[] = [];
    for (const n of this.list) {
      if (n.kind === NodeKind.Group || n.kind === NodeKind.Collision) continue;
      const b = this.bounds(n);
      if (aabbValid(b)) {
        boxes.push(b);
      } else {
        const p = col3(this.world(n), 3);
        const r = n.kind === NodeKind.Actor ? K_MARKER_RADIUS : 0;
        boxes.push({ lo: add(p, [-r, -r, -r]), hi: add(p, [r, r, r]) });
      }
    }
    return focusBox(boxes);
  }

  // ---------------------------------------------------------------------------------------------
  // a drag's preview (the gizmo, a tool): the new world matrices of some nodes, drawn in their
  // place with what is under them until the engine's answer comes back. null clears it.
  setPreview(worlds: Map<number, Mat4> | null) {
    this.preview.clear();
    if (worlds)
      for (const [id, w] of worlds) {
        const root = this.nodes.get(id);
        if (!root) continue;
        const delta = matMul(w, inverse(root.world));
        for (const i of this.subtree(id)) {
          const n = this.nodes.get(i)!;
          this.preview.set(i, i === id ? w : matMul(delta, n.world));
        }
      }
    this.changed();
  }

  // a world point of a node (its origin)
  origin(id: number): Vec3 | null {
    const n = this.nodes.get(id);
    return n ? point(this.world(n), [0, 0, 0]) : null;
  }
}

export const store = new SceneStore();
