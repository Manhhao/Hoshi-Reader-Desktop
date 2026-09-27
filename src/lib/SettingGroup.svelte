<script lang="ts">
  import type { Snippet } from "svelte";

  let { title = "", children }: { title?: string; children: Snippet } = $props();
</script>

<section class="flex flex-col gap-2">
  {#if title}
    <h3 class="text-xs font-medium tracking-wide text-base-content/50 uppercase">{title}</h3>
  {/if}
  <div class="card border border-base-300">
    <div class="card-body gap-3 p-3">
      {@render children()}
    </div>
  </div>
</section>
