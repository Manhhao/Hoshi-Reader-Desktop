<script lang="ts">
  import type { Snippet } from "svelte";

  let { title, compact = false, children }: { title: string; compact?: boolean; children: Snippet } = $props();
</script>

{#if compact}
  <section class="flex flex-col gap-3">
    <h3 class="text-xs font-medium tracking-wide text-base-content/50 uppercase">{title}</h3>
    {@render children()}
  </section>
{:else}
  <div class="card border border-base-300 bg-base-100">
    <div class="card-body gap-4">
      <h2 class="card-title text-base">{title}</h2>
      {@render children()}
    </div>
  </div>
{/if}
