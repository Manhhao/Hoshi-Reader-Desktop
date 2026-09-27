<script lang="ts">
  import { ALargeSmall, ArrowDown, ArrowUp, Clock, GripVertical, Hourglass, Percent, SlidersHorizontal } from "@lucide/svelte";
  import { bookSort, reverseBookSort, setBookSort, sortOption } from "./bookSort.svelte";
  import { saveShellConfig, shellConfig } from "./shellConfig.svelte";
  import type { SortOption } from "./types";

  let { shelf }: { shelf: string | null } = $props();

  const option = $derived(sortOption(shelf));

  const options = [
    ["Recent", Clock],
    ["Title", ALargeSmall],
    ["Progress", Percent],
    ["Time Read", Hourglass],
  ] as const;

  let open = $state(false);
  let menu = $state<HTMLDivElement>();

  $effect(() => {
    if (!open) return;
    const outside = (event: Event) => {
      if (!menu?.contains(event.target as Node)) open = false;
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") open = false;
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape);
    };
  });

  function pick(next: SortOption) {
    if (option === next && next !== "Custom") reverseBookSort();
    else setBookSort(next, shelf);
    open = false;
  }
</script>

<div bind:this={menu} class="dropdown dropdown-end dropdown-open">
  <button class="btn btn-ghost btn-sm btn-square" onclick={() => (open = !open)}>
    <SlidersHorizontal class="size-4" />
  </button>
  {#if open}
    <div class="dropdown-content z-10 mt-1 flex w-56 flex-col gap-1 rounded-box border border-base-300 bg-base-100 p-2 shadow-md">
      <ul class="menu w-full p-0">
        <li class="menu-title">Sorting by...</li>
        {#each options as [value, Icon] (value)}
          <li>
            <button
              class={option === value ? "menu-active" : ""}
              onclick={() => pick(value)}
            >
              <Icon class="size-4" />
              {value}
              {#if option === value}
                {#if (value === "Title") !== bookSort.reversed}
                  <ArrowUp class="ml-auto size-3.5" />
                {:else}
                  <ArrowDown class="ml-auto size-3.5" />
                {/if}
              {/if}
            </button>
          </li>
        {/each}
        {#if shelf !== null}
          <li>
            <button
              class={option === "Custom" ? "menu-active" : ""}
              onclick={() => pick("Custom")}
            >
              <GripVertical class="size-4" />
              Custom
            </button>
          </li>
        {/if}
        <li class="menu-title">Layout</li>
      </ul>
      <div class="join w-full px-1">
        {#each ["Grid", "List"] as const as layout (layout)}
          <button
            class="btn btn-sm join-item flex-1 {shellConfig.bookshelfLayout === layout ? 'btn-active' : ''}"
            onclick={() => {
              shellConfig.bookshelfLayout = layout;
              saveShellConfig();
            }}
          >
            {layout}
          </button>
        {/each}
      </div>
      <ul class="menu w-full p-0">
        <li class="menu-title">Covers</li>
      </ul>
      <div class="join w-full px-1 pb-1">
        {#each ["Show", "Blur", "Hide"] as const as mode (mode)}
          <button
            class="btn btn-sm join-item flex-1 {shellConfig.bookshelfCoverMode === mode ? 'btn-active' : ''}"
            onclick={() => {
              shellConfig.bookshelfCoverMode = mode;
              saveShellConfig();
            }}
          >
            {mode}
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>
