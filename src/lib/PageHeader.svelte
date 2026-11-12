<script lang="ts">
  import type { Snippet } from "svelte";
  import Titlebar from "./Titlebar.svelte";

  let { title, subtitle, children, actions }: { title: string; subtitle?: Snippet; children?: Snippet; actions?: Snippet } = $props();
</script>

<Titlebar />
<header class="@container navbar min-h-0 gap-2 border-b border-base-300 px-4 py-2.5">
  <div class="flex items-baseline gap-2.5">
    <h1 class="shrink-0 text-lg font-semibold leading-8">{title}</h1>
    {#if subtitle}
      <span class="truncate text-sm tabular-nums text-base-content/60">{@render subtitle()}</span>
    {/if}
  </div>
  {@render children?.()}
  {#if actions}
    <div class="ml-auto flex shrink-100 items-center gap-2 whitespace-nowrap">
      {@render actions()}
    </div>
  {/if}
</header>
