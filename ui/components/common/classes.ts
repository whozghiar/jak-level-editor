// The launcher's look of the panels' and windows' controls (the frame's colors: black fields, grey
// borders, orange for what is chosen).

export const button =
  "rounded border border-[#2a2a2a] bg-[#0b0b0b] px-3 py-1 text-sm text-neutral-200 hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-40";
export const small =
  "rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-0.5 text-xs text-neutral-200 hover:border-[#3a3a3a] hover:bg-[#1a1a1a] disabled:pointer-events-none disabled:opacity-40";
export const primary =
  "rounded border border-orange-600 bg-orange-600 px-4 py-1.5 text-sm font-semibold text-white hover:bg-orange-500 disabled:pointer-events-none disabled:opacity-40";
export const input =
  "w-full min-w-0 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-1 text-sm text-neutral-200 outline-none placeholder:text-neutral-600 focus:border-orange-600";
// ImGui's SeparatorText
export const heading =
  "mt-4 mb-2 border-b border-white/10 pb-1 text-xs font-semibold tracking-wide text-neutral-400 uppercase first:mt-0";
export const muted = "text-xs text-neutral-500";
// a checkbox or a radio: Flowbite's forms reset draws them (accent-color is lost), checked in the
// text's color
export const check =
  "size-3.5 shrink-0 cursor-pointer border-[#3a3a3a] bg-[#0b0b0b] text-orange-500 focus:ring-0 focus:ring-offset-0";
export const range = "w-full accent-orange-500";
// the "Work in progress" mark of what is planned, not available yet (another game of the series)
export const wip =
  "shrink-0 rounded border border-amber-400/40 bg-amber-400/10 px-1.5 text-[10px] font-semibold tracking-wide text-amber-300 uppercase";
