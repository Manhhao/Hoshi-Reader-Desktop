<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { Settings, Trash2, X } from "@lucide/svelte";
  import DailyGoal from "./DailyGoal.svelte";
  import PageHeader from "./PageHeader.svelte";
  import ReadingTime from "./ReadingTime.svelte";
  import StatisticsEdit from "./StatisticsEdit.svelte";
  import StatisticsSettings from "./StatisticsSettings.svelte";
  import { schemeUrl } from "./scheme";
  import { statsConfig } from "./statsConfig.svelte";
  import {
    formatNumber,
    formatUnits,
    readingSpeed,
    StatisticsModel,
  } from "./statsModel.svelte";

  const model = new StatisticsModel();
  let edit = $state<ReturnType<typeof StatisticsEdit>>();
  let settingsDialog = $state<HTMLDialogElement>();

  $effect(() => {
    statsConfig.statisticsResetTime;
    model.load();
  });

  $effect(() => {
    const timer = setInterval(() => model.refreshDate(), 1000);
    const unlisten = listen("sync://books-changed", () => model.scheduleLoad());
    return () => {
      clearInterval(timer);
      unlisten.then((fn) => fn());
    };
  });

  const books = $derived(model.periodBooks);
  const visible = $derived(books.slice(0, model.visibleBookCount));
  const longest = $derived(books[0]?.readingTime ?? 0);
</script>

<svelte:window onfocus={() => model.refreshDate()} />

<div class="flex h-full flex-col bg-base-100">
  <PageHeader title="Statistics">
    {#snippet actions()}
      <button
        class="btn btn-ghost btn-sm btn-square"
        title="Settings"
        onclick={() => settingsDialog?.showModal()}
      >
        <Settings class="size-4" />
      </button>
    {/snippet}
  </PageHeader>

  <main class="min-h-0 flex-1 overflow-y-auto">
    <div class="@container mx-auto flex max-w-6xl flex-col gap-5 p-4">
      <div class="grid items-start gap-5 @5xl:grid-cols-2">
        <section class="flex flex-col gap-2">
          <h2 class="text-xs font-medium tracking-wide text-base-content/60 uppercase">
            Daily Goal
          </h2>
          <DailyGoal {model} />
        </section>

        <section class="flex flex-col gap-2">
          <h2 class="text-xs font-medium tracking-wide text-base-content/60 uppercase">
            Reading Time
          </h2>
          <ReadingTime {model} />
          <div class="card border border-base-300 bg-base-100">
            <ul class="list">
              <li class="list-row items-center py-2.5">
                <span class="list-col-grow text-sm">Characters Read</span>
                <span class="text-sm text-base-content/70 tabular-nums">
                  {formatNumber(model.summary.charactersRead)}
                </span>
              </li>
              <li class="list-row items-center py-2.5">
                <span class="list-col-grow text-sm">Reading Speed</span>
                <span class="text-sm text-base-content/70 tabular-nums">
                  {formatNumber(readingSpeed(model.summary))} / h
                </span>
              </li>
              {#if model.selectedDay === null}
                <li class="list-row items-center py-2.5">
                  <span class="list-col-grow text-sm">Total Time</span>
                  <span class="text-sm text-base-content/70 tabular-nums">
                    {formatUnits(model.summary.readingTime)}
                  </span>
                </li>
              {/if}
            </ul>
          </div>
        </section>
      </div>

      {#if books.length > 0}
        <section class="flex flex-col gap-2">
          <h2 class="text-xs font-medium tracking-wide text-base-content/60 uppercase">Books</h2>
          <div class="card overflow-hidden border border-base-300 bg-base-100">
            <ul class="list">
              {#each visible as book (book.id)}
                <li class="list-row items-center has-[button.list-col-grow:hover]:bg-base-200">
                  {#if book.cover}
                    <img
                      src={schemeUrl("cover", book.isDeleted ? `archive/${book.id}` : book.id) +
                        "?w=120"}
                      alt=""
                      class="h-12 w-[34px] rounded object-cover"
                    />
                  {:else}
                    <div class="h-12 w-[34px] rounded bg-base-content/20"></div>
                  {/if}
                  <button
                    class="list-col-grow min-w-0 text-left after:absolute after:inset-0"
                    onclick={() => edit?.show(book.folder, book.title)}
                  >
                    <div class="flex items-center gap-1.5">
                      {#if book.isDeleted}
                        <Trash2 class="size-3.5 shrink-0 text-base-content/50" />
                      {/if}
                      <span class="truncate text-sm">{book.title}</span>
                    </div>
                    <div class="mt-1 flex items-center gap-2 pr-2">
                      <span
                        class="h-[5px] rounded-full bg-base-content/25"
                        style="width: {longest > 0
                          ? Math.max((book.readingTime / longest) * 60, 2)
                          : 0}%"
                      ></span>
                      <span class="text-xs text-base-content/50 tabular-nums">
                        {formatUnits(book.readingTime)}
                      </span>
                    </div>
                  </button>
                  <div class="hidden w-24 flex-col items-end @2xl:flex">
                    <span class="text-sm tabular-nums">{formatNumber(book.charactersRead)}</span>
                    <span class="text-xs text-base-content/50">Characters</span>
                  </div>
                  <div class="hidden w-24 flex-col items-end @2xl:flex">
                    <span class="text-sm tabular-nums">{formatNumber(readingSpeed(book))} / h</span>
                    <span class="text-xs text-base-content/50">Reading Speed</span>
                  </div>
                </li>
              {/each}
            </ul>
          </div>
          {#if model.visibleBookCount < books.length}
            <button
              class="btn btn-ghost btn-sm self-start"
              onclick={() => (model.visibleBookCount += 5)}
            >
              Show More
            </button>
          {/if}
        </section>
      {/if}
    </div>
  </main>
</div>

<StatisticsEdit bind:this={edit} onChanged={() => model.load()} />

<dialog class="modal" bind:this={settingsDialog}>
  <div class="modal-box flex max-w-md flex-col gap-6">
    <div class="flex items-center justify-between">
      <h3 class="text-base font-semibold">Statistics Settings</h3>
      <form method="dialog">
        <button class="btn btn-ghost btn-sm btn-square"><X class="size-4" /></button>
      </form>
    </div>
    <StatisticsSettings />
  </div>
  <form method="dialog" class="modal-backdrop">
    <button>close</button>
  </form>
</dialog>
