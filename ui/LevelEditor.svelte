<script lang="ts">
  // The whole editor: the standalone app and the launcher mount this component.
  import { invoke } from "@tauri-apps/api/core";

  let { embedded = false }: { embedded?: boolean } = $props();
  let engine = $state("");

  $effect(() => {
    invoke("plugin:jak-level-editor|call", { name: "ping", args: {} }).then(
      (r: any) => (engine = `${r.engine} ${r.version}`),
      (e) => (engine = String(e)),
    );
  });
</script>

<div class="flex h-screen items-center justify-center bg-[#141414] text-white">
  <p class="font-mono text-sm text-neutral-400">
    Jak Level Editor {embedded ? "(launcher)" : ""} — {engine}
  </p>
</div>
