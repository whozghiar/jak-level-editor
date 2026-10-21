<script lang="ts" module>
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { firstRunGuide } from "./GuideWindow.svelte";

  /** library_setup (crates/core/src/editor/library.rs) */
  export type LibrarySetup = {
    chosen: boolean;
    default: string;
    default_game: boolean;
    root: string;
    root_game: boolean;
    host: string;
    extractor: string;
    extractor_dirs: string[];
    can_fetch: boolean;
  };

  // the first start: where the library goes (until the user chose), then the guided tour
  call<LibrarySetup>("library_setup", {}, true).then(
    (l) => l && (l.chosen ? firstRunGuide() : windows.open("welcome")),
  );
</script>

<script lang="ts">
  // The first-run assistant: what the library holds, the host's folder for it (Host::default_library)
  // or another; Confirm creates it and uses it, then leads to the extraction when it holds no game.
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import IconFolder from "~icons/mdi/folder-outline";
  import { t } from "../../lib/i18n";
  import { button, muted, primary } from "../common/classes";

  let setup = $state<LibrarySetup>();
  let path = $state("");
  const isDefault = $derived(!!setup && path === setup.default);

  onMount(async () => {
    setup = await call<LibrarySetup>("library_setup", {}, true);
    path = setup?.default ?? "";
  });

  async function choose() {
    const p = await open({ directory: true, defaultPath: path || undefined });
    if (typeof p === "string") path = p;
  }

  async function confirm() {
    const r = await call<LibrarySetup>("choose_library", { path });
    if (!r) return; // why: a toast
    windows.close("welcome");
    await firstRunGuide();
    // the extraction in front of the guide (its first step says the same)
    if (!r.root_game) windows.open("extract");
  }

  const holds = [
    ["jak3/  jak2/", "welcome.holds.games"],
    ["projects/", "welcome.holds.projects"],
    ["exports/", "welcome.holds.exports"],
    ["prefabricated/", "welcome.holds.prefabs"],
  ];
</script>

<div class="flex flex-col gap-4 text-sm">
  <p class="text-neutral-300">{t("welcome.intro")}</p>
  <div>
    <p class="mb-1.5 text-neutral-400">{t("welcome.holds")}</p>
    <ul class="flex flex-col gap-1">
      {#each holds as [folder, key]}
        <li class="flex gap-3">
          <span
            class="w-36 shrink-0 font-mono text-xs leading-5 text-orange-400"
            >{folder}</span
          >
          <span class="text-neutral-300">{t(key)}</span>
        </li>
      {/each}
    </ul>
  </div>

  <section>
    <h3
      class="mb-1.5 border-b border-white/10 pb-1 text-xs font-semibold tracking-wide text-neutral-400 uppercase"
    >
      {t(isDefault ? "welcome.default" : "welcome.folder")}
    </h3>
    <div
      class="flex items-center gap-2 rounded border border-[#2a2a2a] bg-[#0b0b0b] px-2 py-1.5"
    >
      <IconFolder class="shrink-0 text-neutral-400" />
      <span
        class="min-w-0 flex-1 font-mono text-xs break-all text-neutral-200 select-text"
        >{path}</span
      >
    </div>
    {#if isDefault && setup?.default_game}
      <p class="mt-1.5 text-green-400">{t("welcome.default_game")}</p>
    {:else if isDefault}
      <p class="mt-1.5 {muted}">{t("welcome.next_extract")}</p>
    {/if}
  </section>

  <div class="flex items-center gap-2">
    <button class="{button} shrink-0 whitespace-nowrap" onclick={choose}
      >{t("welcome.choose")}</button
    >
    <span class="ml-auto {muted}">{t("welcome.later")}</span>
    <button class="{primary} px-6" disabled={!path} onclick={confirm}
      >{t("welcome.confirm")}</button
    >
  </div>
</div>
