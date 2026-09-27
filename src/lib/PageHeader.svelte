<script lang="ts">
  import type { Snippet } from "svelte";
  import Titlebar from "./Titlebar.svelte";

  let { title, children, actions }: { title: string; children?: Snippet; actions?: Snippet } = $props();
</script>

<Titlebar />
<header class="navbar min-h-0 gap-2 border-b border-base-300 px-4 py-2.5">
  <h1 class="shrink-0 text-lg font-semibold leading-8">{title}</h1>
  {@render children?.()}
  {#if actions}
    <div class="ml-auto flex shrink-0 items-center gap-2">
      {@render actions()}
    </div>
  {/if}
</header>
