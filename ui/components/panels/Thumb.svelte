<script lang="ts">
  // A picture of the panels (render/thumbnails.ts), asked while it is shown: "..." while it is
  // drawn, "?" when it cannot be. The same picture (its key) is kept when the panel draws again.
  import { untrack } from "svelte";
  import { thumbKey, thumbs, type ThumbRequest } from "../../render/thumbnails";

  let { req, size = 80 }: { req: ThumbRequest | null; size?: number } = $props();
  let url = $state<string | null | undefined>(undefined);
  const key = $derived(req ? thumbKey(req) : "");

  $effect(() => {
    void key;
    const r = untrack(() => req);
    url = undefined;
    if (!r) return;
    let live = true;
    thumbs.get(r).then((u) => live && (url = u));
    return () => {
      live = false;
      thumbs.release(r);
    };
  });
</script>

{#if url}
  <img src={url} alt="" draggable="false" class="block rounded-sm" style:width="{size}px" style:height="{size}px" />
{:else}
  <div class="flex items-center justify-center text-neutral-500" style:width="{size}px" style:height="{size}px">
    {url === null || !req ? "?" : "..."}
  </div>
{/if}
