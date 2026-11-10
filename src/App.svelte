<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { message, open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { FolderCog } from "@lucide/svelte";
  import { chromeOverride, chromeTheme } from "./lib/theme.svelte";
  import Bookshelf from "./lib/Bookshelf.svelte";
  import DictionarySearch from "./lib/DictionarySearch.svelte";
  import Statistics from "./lib/Statistics.svelte";
  import ManageShelves from "./lib/ManageShelves.svelte";
  import Rail from "./lib/Rail.svelte";
  import Reader from "./lib/Reader.svelte";
  import { readerConfig } from "./lib/readerConfig.svelte";
  import type { BookHighlight } from "./lib/highlights";
  import Settings, { settingsTabs, type SettingsTab } from "./lib/Settings.svelte";
  import SourceList from "./lib/SourceList.svelte";
  import ChangelogDialog from "./lib/ChangelogDialog.svelte";
  import UpdateDialog from "./lib/UpdateDialog.svelte";
  import AttachDialog from "./lib/AttachDialog.svelte";
  import type {
    AppView,
    BookMetadata,
    BookDocument,
    BookInfo,
    Bookmark,
    ShelfSource,
    TocItem,
  } from "./lib/types";
  import {
    autoUpdateDictionaries,
    loadCollapsedDictionaries,
  } from "./lib/dictConfig.svelte";
  import {
    authors,
    isCompleted,
    loadShelves,
    migrateShelfOrders,
    sameSource,
    shelfBooks,
    shelves,
  } from "./lib/shelves.svelte";
  import { series } from "./lib/series";
  import { loadSeries, renameSeries, seriesStore } from "./lib/seriesStore.svelte";
  import { Menu } from "@tauri-apps/api/menu";
  import { shellConfig } from "./lib/shellConfig.svelte";
  import { configureSync, network } from "./lib/syncConfig.svelte";

  type OpenedBook = {
    id: string;
    folder: string;
    title: string;
    spine: string[];
    toc: TocItem[];
    cover: string | null;
    savedHighlights: BookHighlight[];
    bookInfo: BookInfo;
    bookmark: Bookmark | null;
    completed: boolean;
    skipSyncOnOpen: boolean;
  };

  let books = $state<BookMetadata[]>([]);
  let current = $state<OpenedBook | null>(null);
  let view = $state<AppView>("books");
  let source = $state<ShelfSource>({ kind: "home" });
  let settingsTab = $state<SettingsTab>("dictionaries");
  let manageShelves = $state<ReturnType<typeof ManageShelves>>();

  const hasSource = $derived(view === "books" || view === "settings");
  const bookSeries = $derived(shellConfig.showSeries ? series(books, seriesStore.names) : []);
  const bookAuthors = $derived(shellConfig.showAuthors ? authors(books) : []);
  const bookShelves = $derived(
    shelves.list
      .map((shelf) => ({ name: shelf.name, count: shelfBooks(books, { kind: "shelf", name: shelf.name }).length }))
      .filter((shelf) => shelf.count > 0),
  );
  const unshelvedCount = $derived(shelfBooks(books, { kind: "unshelved" }).length);
  const completedCount = $derived(shellConfig.showCompleted ? shelfBooks(books, { kind: "completed" }).length : 0);

  $effect(() => {
    if (source.kind === "completed" && !shellConfig.showCompleted) source = { kind: "home" };
    const key = source.kind === "series" ? source.key : null;
    if (key === null) return;
    const group = bookSeries.find((entry) => entry.key === key);
    if (!group) source = { kind: "home" };
    else if (source.kind === "series" && source.name !== group.name) source = { ...source, name: group.name };
  });

  async function refresh() {
    books = await invoke<BookMetadata[]>("list_books");
  }

  let reloadTimer: ReturnType<typeof setTimeout> | undefined;

  function scheduleReload() {
    if (reloadTimer) return;
    reloadTimer = setTimeout(async () => {
      reloadTimer = undefined;
      if (current) return;
      await loadShelves();
      await refresh();
    }, 2000);
  }

  async function importPaths(paths: string[]) {
    const errors: string[] = [];
    for (const path of paths) {
      try {
        await invoke("import_book", { path });
      } catch (error) {
        errors.push(`${path.split(/[\\/]/).pop()}: ${error}`);
      }
    }
    if (paths.length) await refresh();
    if (errors.length) await message(errors.join("\n"), { title: "Error", kind: "error" });
  }

  $effect(() => {
    document.documentElement.dataset.theme = chromeTheme(!!current);
    const scheme = chromeOverride();
    getCurrentWindow().setTheme(scheme);
    invoke("set_menu_theme", { dark: scheme && scheme === "dark" });
  });

  $effect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(async (event) => {
      if (current || view !== "books" || event.payload.type !== "drop") return;
      await importPaths(event.payload.paths.filter((path) => path.toLowerCase().endsWith(".epub")));
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  async function importBook() {
    const result = await open({
      multiple: true,
      filters: [{ name: "EPUB", extensions: ["epub"] }],
    });
    if (result) await importPaths(result);
  }

  async function enterFullscreen() {
    const win = getCurrentWindow();
    if (await win.isFullscreen()) return;
    await new Promise<void>((resolve) => {
      let timer = window.setTimeout(done, 1000);
      function done() {
        window.removeEventListener("resize", onResize);
        resolve();
      }
      function onResize() {
        clearTimeout(timer);
        timer = window.setTimeout(done, 150);
      }
      window.addEventListener("resize", onResize);
      win.setFullscreen(true);
    });
  }

  async function openBook(id: string, skipSyncOnOpen = false) {
    const fullscreen = readerConfig.openInFullscreen ? enterFullscreen() : null;
    const document = await invoke<BookDocument>("open_book", { id });
    const bookInfo = await invoke<BookInfo>("load_book_info", { id });
    const bookmark = await invoke<Bookmark | null>("load_bookmark", { id });
    const book = books.find((b) => b.id === id)!;
    const savedHighlights = await invoke<BookHighlight[]>("load_highlights", { id });
    await fullscreen;
    current = {
      id,
      folder: book.folder,
      title: book.renamedTitle ?? document.title,
      spine: document.spine,
      toc: document.toc,
      cover: book.cover ?? null,
      savedHighlights,
      bookInfo,
      bookmark,
      completed: isCompleted(book),
      skipSyncOnOpen,
    };
  }

  async function deleteBook(id: string) {
    await invoke("delete_book", { id });
    await loadShelves();
    await refresh();
  }

  async function deleteLocalBook(id: string) {
    await invoke("delete_local_book", { id });
    await refresh();
  }

  async function renameBook(id: string, title: string) {
    await invoke("rename_book", { id, title });
    await refresh();
  }

  async function editBookAuthor(id: string, author: string) {
    await invoke("set_book_author", { id, author });
    await refresh();
  }

  async function editBookSeries(id: string, series: string) {
    await invoke("set_book_series", { id, series });
    await refresh();
  }

  let renameSeriesDialog = $state<HTMLDialogElement>();
  let renameSeriesKey = $state<string | null>(null);
  let renameSeriesText = $state("");

  async function showSeriesMenu(key: string, name: string, e: MouseEvent) {
    e.preventDefault();
    const menu = await Menu.new({
      items: [
        {
          id: `rename-series-${key}`,
          text: "Rename Series",
          action: () => {
            renameSeriesKey = key;
            renameSeriesText = name;
            renameSeriesDialog?.showModal();
          },
        },
      ],
    });
    await menu.popup();
  }

  function saveSeriesName() {
    if (renameSeriesKey !== null) renameSeries(renameSeriesKey, renameSeriesText.trim());
    renameSeriesDialog?.close();
  }

  $effect(() => {
    const unlisten = [
      listen("hoshi://hidden", () => {
        view = "books";
        invoke("gdrive_sync_pause");
      }),
      listen("sync://books-changed", scheduleReload),
      getCurrentWindow().onFocusChanged(({ payload: focused }) => {
        invoke(focused ? "gdrive_sync_start" : "gdrive_sync_pause");
      }),
    ];
    const online = async () => {
      network.online = true;
      await configureSync();
      invoke("gdrive_sync_start");
    };
    const offline = () => {
      network.online = false;
      configureSync();
    };
    window.addEventListener("online", online);
    window.addEventListener("offline", offline);
    return () => {
      for (const promise of unlisten) promise.then((fn) => fn());
      window.removeEventListener("online", online);
      window.removeEventListener("offline", offline);
    };
  });

  $effect(() => {
    invoke("anki_ping");
    const timer = setInterval(async () => {
      if (!(await invoke<boolean>("anki_reachable"))) invoke("anki_ping");
    }, 15000);
    return () => clearInterval(timer);
  });

  loadSeries();
  Promise.all([refresh(), loadShelves()]).then(() => migrateShelfOrders(books));
  configureSync().then(() => invoke("gdrive_sync_start"));
  loadCollapsedDictionaries();
  autoUpdateDictionaries();
</script>

{#snippet sourceRow(target: ShelfSource, label: string, count: number | null, onmenu?: (e: MouseEvent) => void)}
  <li>
    <button
      class={sameSource(source, target) ? "menu-active" : ""}
      onclick={() => (source = target)}
      oncontextmenu={onmenu}
    >
      <span class="truncate">{label}</span>
      {#if count !== null}
        <span class="ml-auto text-[11px] tabular-nums opacity-50">{count}</span>
      {/if}
    </button>
  </li>
{/snippet}

{#if current}
  <Reader
    {...current}
    onClose={() => {
      current = null;
      loadShelves();
      refresh();
    }}
  />
{:else}
  <div class="flex h-full">
    <Rail {view} onNavigate={(v) => (view = v)} />

    {#if hasSource}
      <SourceList>
        {#if view === "books"}
          <ul class="menu w-full gap-0.5 p-2">
            {@render sourceRow({ kind: "home" }, "Home", null)}
            {@render sourceRow({ kind: "all" }, "All Books", books.length)}
            {#if completedCount > 0}
              {@render sourceRow({ kind: "completed" }, "Completed", completedCount)}
            {/if}
            {#if bookShelves.length > 0}
              {@render sourceRow({ kind: "unshelved" }, "Unshelved", unshelvedCount)}
            {/if}
            {#if bookShelves.length > 0}
              <li class="menu-title text-[11px] font-medium uppercase tracking-wider">Shelves</li>
              {#each bookShelves as shelf (shelf.name)}
                {@render sourceRow({ kind: "shelf", name: shelf.name }, shelf.name, shelf.count)}
              {/each}
            {/if}
            {#if bookSeries.length > 0}
              <li class="menu-title text-[11px] font-medium uppercase tracking-wider">Series</li>
              {#each bookSeries as group (group.key)}
                {@render sourceRow(
                  { kind: "series", name: group.name, key: group.key },
                  group.name,
                  group.books.length,
                  (e) => showSeriesMenu(group.key, group.name, e),
                )}
              {/each}
            {/if}
            {#if bookAuthors.length > 0}
              <li class="menu-title text-[11px] font-medium uppercase tracking-wider">Authors</li>
              {#each bookAuthors as author (author.name)}
                {@render sourceRow(
                  { kind: "author", name: author.name },
                  author.name,
                  author.count,
                )}
              {/each}
            {/if}
          </ul>
        {:else}
          <ul class="menu w-full gap-0.5 p-2">
            {#each settingsTabs as [id, label] (id)}
              <li>
                <button
                  class={settingsTab === id ? "menu-active" : ""}
                  onclick={() => (settingsTab = id)}
                >
                  {label}
                </button>
              </li>
            {/each}
          </ul>
        {/if}

        {#snippet footer()}
          {#if view === "books"}
            <button
              class="btn btn-ghost w-full justify-start gap-2 px-2"
              onclick={() => manageShelves?.show()}
            >
              <FolderCog class="size-5 shrink-0" />
              Shelves
            </button>
          {/if}
        {/snippet}
      </SourceList>
    {/if}

    <main class="relative min-w-0 flex-1">
      {#if view === "books"}
        <Bookshelf
          books={shelfBooks(books, source)}
          library={books}
          {source}
          onImport={importBook}
          onOpen={openBook}
          onDelete={deleteBook}
          onDeleteLocal={deleteLocalBook}
          onRename={renameBook}
          onEditAuthor={editBookAuthor}
          onEditSeries={editBookSeries}
          onReload={refresh}
          onStatistics={() => (view = "statistics")}
        />
      {:else if view === "statistics"}
        <Statistics />
      {:else if view === "settings"}
        <Settings
          tab={settingsTab}
          onReload={async () => {
            await loadShelves();
            await refresh();
            source = { kind: "home" };
          }}
        />
      {/if}
      <div class="absolute inset-0 {view === 'dictionary' ? '' : 'invisible'}" inert={view !== "dictionary"}>
        <DictionarySearch active={view === "dictionary"} />
      </div>
    </main>
  </div>

  <ManageShelves
    bind:this={manageShelves}
    onRename={(name, newName) => {
      if (sameSource(source, { kind: "shelf", name })) source = { kind: "shelf", name: newName };
    }}
  />

  <dialog class="modal" bind:this={renameSeriesDialog}>
    <div class="modal-box">
      <h3 class="mb-4 text-base font-semibold">Rename Series</h3>
      <input
        class="input w-full"
        placeholder="Series"
        bind:value={renameSeriesText}
        onkeydown={(e) => {
          if (e.key === "Enter") saveSeriesName();
        }}
      />
      <div class="modal-action">
        <form method="dialog">
          <button class="btn btn-sm">Cancel</button>
        </form>
        <button class="btn btn-neutral btn-sm" onclick={saveSeriesName}>Save</button>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>
{/if}

<ChangelogDialog />
<UpdateDialog />
<AttachDialog />
