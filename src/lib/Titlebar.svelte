<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isMac } from "./platform";

  let { class: className = "bg-base-100" }: { class?: string } = $props();

  let fullscreen = $state(false);

  $effect(() => {
    const win = getCurrentWindow();
    win.isFullscreen().then((f) => (fullscreen = f));
    const unlisten = win.onResized(async () => {
      fullscreen = await win.isFullscreen();
    });
    return () => {
      unlisten.then((f) => f());
    };
  });
</script>

{#if isMac && !fullscreen}
  <div data-tauri-drag-region class="h-[1.4375rem] shrink-0 {className}"></div>
{/if}
