<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { Snippet } from "svelte";
  import { BookCheck, Cloud, Plus } from "@lucide/svelte";
  import BookCover from "./BookCover.svelte";
  import HomeStatistics from "./HomeStatistics.svelte";
  import { sortBooks } from "./bookSort.svelte";
  import { relativeDate } from "./relativeDate";
  import { schemeUrl } from "./scheme";
  import { statsConfig } from "./statsConfig.svelte";
  import { formatUnits, readingSpeed, StatisticsModel } from "./statsModel.svelte";
  import type { BookDocument, BookInfo, Bookmark, BookMetadata } from "./types";

  let {
    books,
    library,
    downloads,
    tile,
    onImport,
    onSelect,
    onMenu,
    onStatistics,
  }: {
    books: BookMetadata[];
    library: BookMetadata[];
    downloads: Record<string, number>;
    tile: Snippet<[BookMetadata, number]>;
    onImport: () => void;
    onSelect: (book: BookMetadata) => void;
    onMenu: (book: BookMetadata, e: MouseEvent) => void;
    onStatistics: () => void;
  } = $props();

  const model = new StatisticsModel();
  let showAllReading = $state(false);
  const readingBooks = $derived(sortBooks(books, "Recent", []));
  const visibleReadingBooks = $derived(showAllReading ? readingBooks : readingBooks.slice(0, 4));
  const recentBooks = $derived(sortBooks(library.filter((book) => book.progress === 0), "Recent", []).slice(0, 12));

  let readingInfo = $state<Record<string, { characterCount: number; chapter: string | null }>>({});

  function timeLeft(book: BookMetadata) {
    const totals = (model.books.find((entry) => entry.id === book.id)?.days ?? []).reduce(
      (total, day) => ({
        charactersRead: total.charactersRead + day.charactersRead,
        readingTime: total.readingTime + day.readingTime,
      }),
      { charactersRead: 0, readingTime: 0 },
    );
    const speed = readingSpeed(totals);
    const count = readingInfo[book.id]?.characterCount ?? book.characterCount ?? 0;
    if (count === 0 || speed === 0) return null;
    return `${formatUnits((count * (1 - book.progress) * 3600) / speed)} left`;
  }

  $effect(() => {
    library;
    statsConfig.statisticsResetTime;
    model.load();
  });

  $effect(() => {
    const timer = setInterval(() => model.refreshDate(), 1000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    let active = true;
    Promise.all(visibleReadingBooks.filter((book) => book.epub && book.hasBookInfo).map(async (book) => {
      const [info, bookmark, document] = await Promise.all([
        invoke<BookInfo>("load_book_info", { id: book.id }),
        invoke<Bookmark | null>("load_bookmark", { id: book.id }),
        invoke<BookDocument>("load_contents", { id: book.id }),
      ]);
      const chapter = bookmark ? document.toc.findLast((item) => {
        const chapterInfo = info.chapterInfo[document.spine[item.spineIndex]];
        if (!chapterInfo) return false;
        const offset = item.fragment ? (chapterInfo.fragmentOffsets?.[item.fragment] ?? 0) : 0;
        return chapterInfo.currentTotal + offset <= bookmark.characterCount;
      })?.label ?? null : null;
      return [book.id, { characterCount: info.characterCount, chapter }] as const;
    })).then((entries) => {
      if (active) readingInfo = Object.fromEntries(entries);
    });
    return () => { active = false; };
  });

  function displayTitle(book: BookMetadata) {
    return book.renamedTitle ?? book.title;
  }
</script>

<svelte:window onfocus={() => model.refreshDate()} />

{#snippet cover(book: BookMetadata)}
  <span class="flex w-full flex-col gap-1">
    <span class="relative block aspect-[0.709] w-full overflow-hidden rounded-sm border border-base-300 bg-base-200">
      <BookCover
        title={displayTitle(book)}
        author={book.author}
        src={book.cover ? schemeUrl("cover", book.id) + "?w=512" : null}
      />
    </span>
    <span class="flex items-center gap-1">
      <progress
        class="progress h-1 flex-1 text-base-content/60"
        value={Math.min(book.progress, 1) * 100}
        max="100"
      ></progress>
      <span class="shrink-0 text-right text-[11px] font-medium tabular-nums text-base-content/60">{(book.progress * 100).toFixed(1)}%</span>
    </span>
  </span>
{/snippet}

{#snippet details(book: BookMetadata)}
  <span class="line-clamp-2 text-base font-medium leading-snug transition-colors group-hover:text-secondary group-focus-visible:text-secondary">
    {#if !book.epub}
      <Cloud class="mb-0.5 inline size-3.5" />
    {/if}
    {displayTitle(book)}
  </span>
  {#if book.author}
    <span class="mt-1 block truncate text-xs text-base-content/60">{book.author}</span>
  {/if}
  {#if downloads[book.id] !== undefined}
    <progress
      class="progress progress-primary mt-2 h-1 w-full"
      value={downloads[book.id] * 100}
      max="100"
    ></progress>
  {/if}
{/snippet}

<div class="min-h-0 flex-1 overflow-y-auto">
  <div class="@container flex flex-col gap-6 p-5">
    <h1 class="text-xl font-semibold">Home</h1>

    <HomeStatistics {model} onOpen={onStatistics} />

    <div class="grid items-start gap-7 @3xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
      <section class="@container/reading flex min-w-0 flex-col gap-3">
        <h2 class="text-xs font-medium">Currently Reading</h2>

        {#if readingBooks.length > 0}
          {#each visibleReadingBooks as book, index (book.id)}
            {@const remaining = timeLeft(book)}
            {@const chapter = readingInfo[book.id]?.chapter}
            <button
              class="group grid grid-cols-[5rem_minmax(0,1fr)] gap-x-3 gap-y-4 rounded-lg border border-base-300 p-3 text-left transition-colors hover:bg-primary/10 focus-visible:bg-primary/10 @sm/reading:grid-cols-[6rem_minmax(0,1fr)] @sm/reading:gap-x-5 @sm/reading:p-4 @xl/reading:grid-cols-[6rem_minmax(0,1fr)_8.25rem] {index === 0 ? 'bg-base-200/40' : ''}"
              onclick={() => onSelect(book)}
              oncontextmenu={(e) => onMenu(book, e)}
            >
              <span class="col-start-1 row-span-2 row-start-1 @xl/reading:row-span-1">
                {@render cover(book)}
              </span>
              <span class="col-start-2 row-start-1 flex min-w-0 flex-col justify-between gap-4 pt-1 @xl/reading:pb-4">
                <span>{@render details(book)}</span>
                {#if chapter}
                  <span class="flex flex-col gap-1">
                    <span class="text-[11px] text-base-content/60">Current chapter</span>
                    <span class="line-clamp-2 text-sm">{chapter}</span>
                  </span>
                {/if}
              </span>
              <span class="col-start-2 row-start-2 flex flex-wrap items-end gap-x-6 gap-y-2 pb-4 tabular-nums @xl/reading:col-start-3 @xl/reading:row-start-1 @xl/reading:flex-col @xl/reading:justify-between @xl/reading:pt-1 @xl/reading:text-right">
                {#if remaining}
                  <span class="text-sm font-medium">{remaining}</span>
                {/if}
                <span class="flex flex-col gap-1 text-xs text-base-content/60 {remaining ? '' : '@xl/reading:mt-auto'}">
                  <span class="text-[11px]">Last read</span>
                  <span>{relativeDate(book.lastAccess)}</span>
                </span>
              </span>
            </button>
          {/each}
          {#if readingBooks.length > 4}
            <button class="btn btn-ghost btn-sm self-start" onclick={() => (showAllReading = !showAllReading)}>
              {showAllReading ? "Show less" : "Show more"}
            </button>
          {/if}
        {:else}
          <div class="flex flex-col items-start gap-3 rounded-lg border border-base-300 bg-base-200/40 p-5">
            <BookCheck class="size-6 text-base-content/50" />
            <h3 class="text-sm font-medium">No books in progress.</h3>
          </div>
        {/if}
      </section>

      <section class="flex min-w-0 flex-col gap-4 border-t border-base-300 pt-5 @3xl:border-t-0 @3xl:border-l @3xl:pt-0 @3xl:pl-6">
        <div class="flex items-center justify-between gap-3">
          <h2 class="text-xs font-medium">Recently Imported</h2>
          <button class="btn btn-neutral btn-sm" onclick={onImport}>
            <Plus class="size-4" />
            Import
          </button>
        </div>
        {#if recentBooks.length > 0}
          <div class="grid items-start gap-5" style="grid-template-columns: repeat(auto-fill, minmax(112px, 1fr));">
            {#each recentBooks as book, index (book.id)}
              {@render tile(book, index)}
            {/each}
          </div>
        {/if}
      </section>
    </div>
  </div>
</div>
