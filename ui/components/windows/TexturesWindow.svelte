<script lang="ts">
  // The inspector's Textures of the game... (ui_texture_window, surface_panel.cpp): for one material
  // (`slot`: its place in the first piece's list) of the pieces `ids`, a texture of the games.
  // Closed when they are gone.
  import { engine } from "../../lib/engine.svelte";
  import { t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import { muted } from "../common/classes";
  import TextureBrowser from "./TextureBrowser.svelte";

  let { ids = [], slot = 0 }: { ids?: number[]; slot?: number } = $props();

  let name = $state("");
  let texture = $state("");
  let there = $state<number[]>([]);
  // (a value: read again when it changes, not at each state the engine sends)
  const historyKey = $derived(
    JSON.stringify([engine.state.history.revision, engine.state.document.open]),
  );
  $effect(() => {
    void historyKey;
    // the pieces it was opened for, while they are there
    Promise.all(ids.map((id) => call<{ name: string } | null>("node", { id }, true))).then((nodes) => {
      there = ids.filter((_, i) => nodes[i]);
      if (!there.length) return windows.close("textures");
      name = nodes.find(Boolean)!.name;
      // the material of the first piece still there
      call("materials", { ids: there }, true).then((m) => (texture = m?.slots?.[slot]?.texture?.name ?? ""));
    });
  });
</script>

<div class="flex h-full min-h-0 flex-col gap-2">
  <p>{t("textures.for", name, texture || t("inspector.material.no_texture"))}</p>
  {#if there.length > 1}<p class={muted}>{t("textures.for_more", there.length - 1)}</p>{/if}
  <div class="flex min-h-0 flex-1 flex-col border-t border-white/10 pt-2">
    <TextureBrowser ids={there} {slot} />
  </div>
</div>
