<script lang="ts">
  import { BookOpen, ChartLine, LibraryBig, Settings } from "@lucide/svelte";
  import Titlebar from "./Titlebar.svelte";
  import type { AppView } from "./types";

  let {
    view,
    onNavigate,
  }: {
    view: AppView;
    onNavigate: (view: AppView) => void;
  } = $props();

  const destinations = [
    ["books", "Books", LibraryBig],
    ["dictionary", "Dictionary", BookOpen],
    ["statistics", "Statistics", ChartLine],
  ] as const;

  const menu =
    "menu w-full gap-1 px-1.5 [--menu-active-bg:var(--color-base-300)] " +
    "[--menu-active-fg:var(--color-base-content)]";

  const item = "tooltip tooltip-right mx-auto flex size-11 items-center justify-center p-0";
</script>

<aside class="flex w-18 shrink-0 flex-col border-r border-base-300 bg-base-200">
  <Titlebar class="bg-base-200" />
  <ul class="{menu} flex-1 pt-2">
    {#each destinations as [id, label, Icon] (id)}
      <li>
        <button
          class="{item} {view === id ? 'menu-active' : ''}"
          data-tip={label}
          onclick={() => onNavigate(id)}
        >
          <Icon class="size-6 {view === id ? '' : 'opacity-55'}" />
        </button>
      </li>
    {/each}
  </ul>
  <ul class="{menu} pb-2">
    <li>
      <button
        class="{item} {view === 'settings' ? 'menu-active' : ''}"
        data-tip="Settings"
        onclick={() => onNavigate("settings")}
      >
        <Settings class="size-6 {view === 'settings' ? '' : 'opacity-55'}" />
      </button>
    </li>
  </ul>
</aside>
