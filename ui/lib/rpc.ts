// The interface's line to the engine (docs/architecture.md, 13 and 14): one command carrying every
// call, the engine's events, the URLs of the jle:// protocol; and the toasts that show its errors.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { SvelteMap } from "svelte/reactivity";
import type { CallName } from "./api";
import { t } from "./i18n";

export type ToastLevel = "info" | "warn" | "error";

/** The toasts shown (bottom right), by id: each goes after 5 seconds. */
export const toasts = new SvelteMap<
  number,
  { msg: string; level: ToastLevel }
>();
let lastToast = 0;

export function toast(msg: string, level: ToastLevel = "info"): void {
  const id = ++lastToast;
  toasts.set(id, { msg, level });
  setTimeout(() => toasts.delete(id), 5000);
}

/** An error of the engine as text: a translation key (with its arguments: {key, args}), or English text. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return t(e);
  if (e && typeof e === "object" && "key" in e) {
    const { key, args } = e as { key: string; args?: unknown[] };
    return t(key, ...(args ?? []));
  }
  return String(e);
}

/**
 * Calls the engine (`Editor::call`): its answer, or undefined when it failed (the error shown in a
 * toast, unless `quiet`).
 */
export async function call<T = any>(
  name: CallName,
  args: Record<string, unknown> = {},
  quiet = false,
): Promise<T | undefined> {
  try {
    return await invoke<T>("plugin:jak-level-editor|call", { name, args });
  } catch (e) {
    console.error(`call ${name}`, args, e);
    if (!quiet) toast(errorText(e), "error");
    return undefined;
  }
}

export type EngineEvent =
  "jle:changed" | "jle:log" | "jle:progress" | "jle:request";

/** Listens to an event of the engine; returns what stops listening. */
export function onEngine<T = any>(
  event: EngineEvent,
  cb: (payload: T) => void,
): () => void {
  const off = listen<T>(event, (e) => cb(e.payload));
  return () => void off.then((f) => f());
}

const windows = /Windows/.test(navigator.userAgent);

/** The URL of a path of the jle:// protocol (`scene?full=1`, `mesh/12`...). */
export function jleUrl(path: string): string {
  path = path.replace(/^\//, "");
  // WebView2 serves custom protocols as http(s)://<scheme>.localhost, https with useHttpsScheme
  return windows
    ? `${location.protocol === "https:" ? "https" : "http"}://jle.localhost/${path}`
    : `jle://localhost/${path}`;
}
