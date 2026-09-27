<script lang="ts">
  import type { Snippet } from "svelte";

  let { label, compact = false, children }: { label: string; compact?: boolean; children: Snippet } = $props();
</script>

<div class="flex items-center justify-between {compact ? 'gap-2' : 'gap-4'}">
  <span class="text-sm">{label}</span>
  {@render children()}
</div>
