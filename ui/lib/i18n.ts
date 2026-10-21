/// <reference types="vite/client" />
// The interface's texts: lang/<code>.json, the keys of the C++ editor, their printf arguments
// (%s %d %zu %.1f...) formatted here. Independent of the launcher's svelte-i18n: the editor keeps
// its own texts when the launcher mounts it.

import { SvelteMap } from "svelte/reactivity";

const files = import.meta.glob<{ default: Record<string, string> }>(
  "../../lang/*.json",
  { eager: true },
);
const texts: Record<string, Record<string, string>> = {};
for (const [path, m] of Object.entries(files))
  texts[path.replace(/^.*\/|\.json$/g, "")] = m.default;

/** The languages of lang/, English first: their code and their own name (`_language`). */
export const languages = Object.keys(texts)
  .sort((a, b) => (a === "en" ? -1 : b === "en" ? 1 : a.localeCompare(b)))
  .map((code) => ({ code, name: texts[code]._language ?? code }));

// read by t(): what runs it in a template or an effect runs again when the language changes
const current = new SvelteMap([["code", "en"]]);

export function language(): string {
  return current.get("code")!;
}

// "fr-FR" -> "fr"; null when lang/ has no such language
function known(code: string | null | undefined): string | null {
  if (!code) return null;
  const c = code.toLowerCase();
  return texts[c] ? c : texts[c.split(/[-_]/)[0]] ? c.split(/[-_]/)[0] : null;
}

/** Chooses the language (View > Language): kept for the next start. Returns the one chosen. */
export function setLanguage(code: string): string {
  const c = known(code) ?? "en";
  current.set("code", c);
  try {
    localStorage.setItem("jle.language", c);
  } catch {}
  return c;
}

/** The first language: the host's (the launcher's locale), else the one chosen last, else the system's. */
export function initLanguage(locale?: string): string {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem("jle.language");
  } catch {}
  const c = known(locale) ?? known(saved) ?? known(navigator.language) ?? "en";
  current.set("code", c);
  return c;
}

/** Whether the language (or English) has a text for `key`. */
export function has(key: string): boolean {
  return key in (texts[language()] ?? {}) || key in (texts.en ?? {});
}

/** The text of `key` in the language, else in English, else the key; with printf arguments. */
export function t(key: string, ...args: unknown[]): string {
  const s = texts[language()]?.[key] ?? texts.en?.[key] ?? key;
  return args.length ? printf(s, args) : s;
}

const SPEC =
  /%([-+ 0#]*)(\d*)(?:\.(\d+))?(?:hh|h|ll|l|z|j|t)?([diufFgGeExXsc%])/g;

/** C's printf for the conversions the texts use: %s %d %i %u %zu %f %.Nf %g %e %x %c %%, flags - + 0. */
export function printf(fmt: string, args: unknown[]): string {
  let i = 0;
  return fmt.replace(
    SPEC,
    (
      _m,
      flags: string,
      width: string,
      prec: string | undefined,
      conv: string,
    ) => {
      if (conv === "%") return "%";
      const a = args[i++];
      const n = Number(a);
      let s: string;
      switch (conv) {
        case "d":
        case "i":
        case "u":
          s = String(Math.trunc(n));
          break;
        case "f":
        case "F":
          s = n.toFixed(prec === undefined ? 6 : +prec);
          break;
        case "g":
        case "G":
          // significant digits (6 by default), without the trailing zeros
          s = String(
            Number(n.toPrecision(prec === undefined ? 6 : Math.max(1, +prec))),
          );
          break;
        case "e":
        case "E":
          s = n.toExponential(prec === undefined ? 6 : +prec);
          break;
        case "x":
        case "X":
          s = Math.trunc(n).toString(16);
          break;
        case "c":
          s = typeof a === "number" ? String.fromCharCode(a) : String(a ?? "");
          break;
        default:
          s = String(a ?? "");
      }
      if (conv === "X" || conv === "E" || conv === "G") s = s.toUpperCase();
      if (flags.includes("+") && "diufFgGeE".includes(conv) && n >= 0)
        s = "+" + s;
      const w = +width;
      if (s.length < w)
        s = flags.includes("-")
          ? s.padEnd(w)
          : flags.includes("0") && conv !== "s"
            ? s.replace(/^([+-]?)/, (sign) => sign + "0".repeat(w - s.length))
            : s.padStart(w);
      return s;
    },
  );
}
