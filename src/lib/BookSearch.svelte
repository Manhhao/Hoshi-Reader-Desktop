<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Search } from "@lucide/svelte";
  import type { BookSearchResult } from "./types";

  let { id, onJump }: {
    id: string;
    onJump: (result: BookSearchResult) => void;
  } = $props();

  let input = $state("");
  let query = $state("");
  let results = $state<BookSearchResult[] | null>(null);
  let error = $state("");
  let searchInput: HTMLInputElement;

  export function focus() {
    searchInput.focus();
    searchInput.select();
  }

  $effect(() => {
    const currentQuery = query;
    results = null;
    error = "";
    if (!currentQuery) return;
    let cancelled = false;
    invoke<BookSearchResult[]>("search_book", { id, query: currentQuery }).then((found) => {
      if (!cancelled) results = found;
    }).catch((e) => {
      if (!cancelled) error = String(e);
    });
    return () => { cancelled = true; };
  });
</script>

<form class="flex gap-2 p-4" onsubmit={(e) => { e.preventDefault(); query = input.trim(); }}>
  <input type="search" class="input input-sm min-w-0 flex-1" placeholder="Search book" bind:value={input} bind:this={searchInput} oninput={(e) => { if (!e.currentTarget.value) query = ""; }} />
  <button class="btn btn-ghost btn-sm btn-square" title="Search"><Search class="size-4" /></button>
</form>

{#if query}
  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if error}
      <p class="select-text px-4 py-3 text-sm text-error">{error}</p>
    {:else if results === null}
      <div class="flex justify-center p-6"><span class="loading loading-spinner loading-sm"></span></div>
    {:else if results.length === 0}
      <p class="px-4 py-6 text-center text-sm text-base-content/60">No Results for “{query}”</p>
    {:else}
      <ul>
        {#each results as result, i (i)}
          <li>
            <button class="flex w-full flex-col gap-2 px-4 py-3 text-left hover:bg-base-200" onclick={() => onJump(result)}>
              <span class="text-sm">{result.prefix}<strong>{result.matched}</strong>{result.suffix}</span>
              <span class="flex w-full justify-between gap-2 text-xs text-base-content/60">
                <span class="truncate">{result.chapter}</span><span>{result.character}</span>
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}
