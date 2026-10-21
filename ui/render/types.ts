// What the viewport reads from the engine (docs/rust-architecture.md §4) and what it answers.

import type { AABB, Mat4, Vec3 } from "./math";

export type { AABB, Mat4, Vec3 };

// call("pick", {origin, dir}): the first surface the ray meets, or null
export interface PickHit {
  node: number;
  point: Vec3;
  normal: Vec3;
  distance: number;
}

// Node kinds (scene.h NodeKind)
export const NodeKind = {
  Group: 0,
  Render: 1,
  Collision: 2,
  Actor: 3,
  NavMesh: 4,
} as const;

// JLES node flags
export const NodeFlag = {
  Visible: 1, // with its parents, and the story
  Locked: 2,
  Reference: 4, // a level shown for reference (washed in blue)
  UnderActor: 8, // an actor's model
  GameLight: 16, // has game lighting (tod/<node>)
  Selected: 32,
  Water: 64, // a water surface: its texture flows
} as const;

// JLEM primitive flags
export const PrimFlag = {
  Uv: 1,
  Colors: 2,
  // the mesh is a low-detail backdrop (mesh extras og_backdrop: the distant mountains): the
  // distance fog fades it (RenderSettings.fog). Not in the spec's first draft: set on every
  // primitive of such a mesh.
  Backdrop: 4,
} as const;

export interface NodeRecord {
  id: number;
  parent: number; // 0: none
  kind: number;
  flags: number;
  // the u16 "reserved" field. For an actor: the low 16 bits of the FNV-1a hash of its etype (the
  // color of its marker, renderer.cpp hash_color), 0 when unknown
  extra: number;
  mesh: number; // mesh uid (low 32 bits), 0: none
  mesh_version: number;
  world: Mat4; // column-major, a view into the answer's bytes
}

export const AlphaMode = { Opaque: 0, Mask: 1, Blend: 2 } as const;
export const BlendMode = {
  Alpha: 0,
  Add: 1,
  AddAlpha: 2,
  Subtract: 3,
  Half: 4,
} as const;

export interface Material {
  name: string;
  texture: number; // index into the textures, -1 none
  base_color: [number, number, number, number];
  alpha: number; // AlphaMode
  blend: number; // BlendMode
  cutoff: number;
  double_sided: boolean;
  translucent: boolean;
  water: boolean;
}

export interface RenderSettings {
  wireframe: boolean;
  show_grid: boolean;
  show_actors: boolean;
  show_ocean: boolean;
  textures: boolean;
  vertex_colors: boolean;
  vertex_color_scale: number; // PS2 vertex colors: 0x80 is neutral, stored as 128/255
  shade_uncolored: boolean; // simple lighting for meshes without vertex colors
  frustum_culling: boolean;
  clear_color: [number, number, number];
  // the far ocean: the sea to the horizon at that height, drawn behind everything (the game draws
  // it with the sky)
  far_ocean: boolean;
  far_ocean_y: number;
  far_ocean_color: [number, number, number];
  // distance fog: what the low-detail backdrop meshes (mesh extras og_backdrop: the distant
  // mountains beyond a level's playable area, "-tfrag-lowres") fade into, by how far the camera
  // is from them. They are made to be seen through the game's fog, which the editor has none of
  // otherwise, and look wrong (a harsh, dithered pattern) without it; nothing else fogs.
  fog: boolean;
  fog_color: [number, number, number];
  fog_start: number; // meters
  fog_end: number;
  // the game view draws the decor: the editor draws the rest over it
  game_view: boolean;
  // the models lit as the game lights them (merc: the mood's ambient light and up to 3 lights, by
  // their normals) and fogged like the game's, in the game view
  game_light: boolean;
  light_ambient: [number, number, number];
  light_dir: [number, number, number][];
  light_color: [number, number, number][];
  game_fog: [number, number, number, number]; // its color (0..1) and how much of it at most
  game_fog_start: number; // meters
  game_fog_end: number;
  time: number; // seconds: the water's animation (Material.water, drawWater)
}

export function defaultRenderSettings(): RenderSettings {
  return {
    wireframe: false,
    show_grid: true,
    show_actors: true,
    show_ocean: true,
    textures: true,
    vertex_colors: true,
    vertex_color_scale: 2,
    shade_uncolored: true,
    frustum_culling: true,
    clear_color: [0.16, 0.18, 0.21],
    far_ocean: false,
    far_ocean_y: 0,
    far_ocean_color: [0.02, 0.24, 0.29],
    fog: true,
    fog_color: [0.6, 0.68, 0.72],
    fog_start: 50,
    fog_end: 400,
    game_view: false,
    game_light: false,
    light_ambient: [0, 0, 0],
    light_dir: [
      [0, 0, 0],
      [0, 0, 0],
      [0, 0, 0],
    ],
    light_color: [
      [0, 0, 0],
      [0, 0, 0],
      [0, 0, 0],
    ],
    game_fog: [0, 0, 0, 0],
    game_fog_start: 32,
    game_fog_end: 1000,
    time: 0,
  };
}

// The ocean's look (drawWater): its waves move, the sun glints on them, they reflect the sky
export interface WaterLook {
  color: [number, number, number]; // the water near (the ocean map's colors)
  deep: [number, number, number]; // far away (the far ocean's)
  sky: [number, number, number]; // what it reflects
  sun_dir: [number, number, number]; // toward the sun (the moon at night)
  sun_color: [number, number, number];
  alpha: number; // seen from above (more opaque at grazing angles)
}

export function defaultWaterLook(): WaterLook {
  return {
    color: [0.24, 0.56, 0.64],
    deep: [0.02, 0.24, 0.29],
    sky: [0.5, 0.62, 0.72],
    sun_dir: [0.32, 0.92, 0.23],
    sun_color: [1, 0.95, 0.85],
    alpha: 0.62,
  };
}

// Actors without a model are drawn as a marker of this radius (scene.h kMarkerRadius)
export const K_MARKER_RADIUS = 0.6;
