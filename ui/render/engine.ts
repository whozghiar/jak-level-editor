/// <reference types="vite/client" />
// The viewport's line to the engine: calls, events and the jle:// protocol's bytes. One object the
// dev mock (dev/mock.ts) replaces in a browser without Tauri.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { jleUrl } from "../lib/rpc";

// rpc.ts's call() shows its errors in a toast: the viewport's calls (a pick at each mouse move)
// fail quietly, to the caller

export const transport = {
  call: (name: string, args: Record<string, unknown> = {}): Promise<any> =>
    invoke("plugin:jak-level-editor|call", { name, args }),
  fetch: async (path: string): Promise<ArrayBuffer> => {
    const r = await fetch(jleUrl(path));
    if (!r.ok) throw new Error(`jle://${path}: ${r.status}`);
    return r.arrayBuffer();
  },
  listen: (event: string, cb: (payload: any) => void): (() => void) => {
    const off = listen(event, (e) => cb(e.payload));
    return () => void off.then((f) => f());
  },
};

export const call = (name: string, args: Record<string, unknown> = {}) =>
  transport.call(name, args);
export const onEngine = (event: string, cb: (payload: any) => void) =>
  transport.listen(event, cb);
export const fetchBytes = (path: string) => transport.fetch(path);

// the dev mock, in `npx vite` opened in a browser (no Tauri): a scene to fly around
export const engineReady: Promise<void> =
  import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)
    ? import("./dev/mock").then((m) => m.install(transport))
    : Promise.resolve();
