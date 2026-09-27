<script lang="ts">
  import type { Snippet } from "svelte";
  import Titlebar from "./Titlebar.svelte";
  import { shellConfig, saveShellConfig } from "./shellConfig.svelte";

  let {
    children,
    footer,
  }: {
    children: Snippet;
    footer?: Snippet;
  } = $props();

  let sidebarEl: HTMLElement;
  let resizeStart: { x: number; width: number } | null = null;
</script>

<aside
  bind:this={sidebarEl}
  class="relative flex max-w-[45vw] shrink-0 flex-col border-r border-base-300 bg-base-200"
  style:width="{shellConfig.sidebarWidth}px"
>
  <Titlebar class="bg-base-200" />
  <div class="min-h-0 flex-1 overflow-y-auto">
    {@render children()}
  </div>
  {#if footer}
    <div class="flex items-center border-t border-base-300 p-1.5">
      {@render footer()}
    </div>
  {/if}
  <div
    class="absolute inset-y-0 -right-1 z-10 w-2 cursor-col-resize touch-none hover:bg-base-content/10"
    onpointerdown={(event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      resizeStart = { x: event.clientX, width: sidebarEl.getBoundingClientRect().width };
      event.currentTarget.setPointerCapture(event.pointerId);
    }}
    onpointermove={(event) => {
      if (!resizeStart) return;
      shellConfig.sidebarWidth = Math.round(Math.min(
        384,
        window.innerWidth * 0.45,
        Math.max(160, resizeStart.width + event.clientX - resizeStart.x),
      ));
    }}
    onlostpointercapture={() => {
      resizeStart = null;
      saveShellConfig();
    }}
  ></div>
</aside>
