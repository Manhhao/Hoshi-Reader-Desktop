<script module lang="ts">
  const downloadingBooks = $state<Record<string, number>>({});
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Menu, Submenu, type MenuOptions } from "@tauri-apps/api/menu";
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import {
    ArrowDown,
    ArrowUp,
    CircleArrowDown,
    CircleCheck,
    Cloud,
    Folder,
    ListChecks,
    Plus,
    Search,
    Trash2,
  } from "@lucide/svelte";
  import BookCover from "./BookCover.svelte";
  import PageHeader from "./PageHeader.svelte";
  import ReadingOverview from "./ReadingOverview.svelte";
  import Titlebar from "./Titlebar.svelte";
  import ViewMenu from "./ViewMenu.svelte";
  import { isMac } from "./platform";
  import { relativeDate } from "./relativeDate";
  import { schemeUrl } from "./scheme";
  import { Sortable } from "./sortable.svelte";
  import { bookSort, reverseBookSort, setBookSort, sortBooks, sortOption } from "./bookSort.svelte";
  import { createShelf, moveBook, saveShelfOrders, setShelfOrder, shelves, sourceTitle } from "./shelves.svelte";
  import { shellConfig } from "./shellConfig.svelte";
  import { formatNumber, formatUnits } from "./statsModel.svelte";
  import { syncConfig } from "./syncConfig.svelte";
  import type { Bookmark, BookMetadata, BookStatistics, GoogleDriveSyncStatus, Sessions, ShelfSource, SortOption } from "./types";
  import Toast from "./Toast.svelte";

  let {
    books,
    library,
    source,
    onImport,
    onOpen,
    onDelete,
    onDeleteLocal,
    onRename,
    onEditAuthor,
    onReload,
    onStatistics,
  }: {
    books: BookMetadata[];
    library: BookMetadata[];
    source: ShelfSource;
    onImport: () => void;
    onOpen: (id: string, skipSyncOnOpen?: boolean) => void;
    onDelete: (id: string) => Promise<void>;
    onDeleteLocal: (id: string) => Promise<void>;
    onRename: (id: string, title: string) => void;
    onEditAuthor: (id: string, author: string) => void;
    onReload: () => Promise<void>;
    onStatistics: () => void;
  } = $props();

  function displayTitle(book: BookMetadata) {
    return book.renamedTitle ?? book.title;
  }

  const shelfName = $derived(source.kind === "shelf" ? source.name : null);

  const option = $derived(sortOption(shelfName));

  let readingTimes = $state<Record<string, number>>({});

  $effect(() => {
    library;
    if (shellConfig.bookshelfLayout !== "List" && option !== "Time Read") return;
    invoke<BookStatistics[]>("load_all_statistics", { resetTime: 0 }).then((statistics) => {
      readingTimes = Object.fromEntries(
        statistics
          .filter((book) => !book.isDeleted)
          .map((book) => [book.id, book.days.reduce((total, day) => total + day.readingTime, 0)]),
      );
    });
  });

  const sortedBooks = $derived(
    sortBooks(books, option, shelfName ? (shelves.orders[shelfName] ?? []) : [], bookSort.reversed, readingTimes),
  );

  function sortBy(next: SortOption) {
    if (option === next) reverseBookSort();
    else setBookSort(next, shelfName);
  }

  let query = $state("");

  const visibleBooks = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return sortedBooks;
    return sortedBooks.filter(
      (book) => displayTitle(book).toLowerCase().includes(needle) || book.author?.toLowerCase().includes(needle),
    );
  });

  let isSelecting = $state(false);
  let selectedIds = $state<string[]>([]);
  let anchor = $state<number | null>(null);

  const selectedBooks = $derived(library.filter((book) => selectedIds.includes(book.id)));
  const canReorder = $derived(shelfName !== null && option === "Custom" && isSelecting && !query.trim());

  const sort = new Sortable({
    commit: saveShelfOrders,
    threshold: 5,
    move: (from, to) => {
      if (!shelfName) return;
      const ids = sortedBooks.map((book) => book.id);
      ids.splice(to, 0, ids.splice(from, 1)[0]);
      setShelfOrder(shelfName, ids);
    },
  });

  function startDrag(e: PointerEvent, index: number) {
    sort.dragged = false;
    if (canReorder && e.button === 0) sort.start(index, e);
  }

  function clearSelection() {
    isSelecting = false;
    selectedIds = [];
    anchor = null;
  }

  $effect(() => {
    source;
    clearSelection();
    query = "";
  });

  function selectBook(book: BookMetadata, e?: MouseEvent) {
    if (sort.dragged) {
      sort.dragged = false;
      return;
    }
    const index = visibleBooks.indexOf(book);
    const modifier = source.kind !== "home" && e !== undefined && (e.shiftKey || (isMac ? e.metaKey : e.ctrlKey));
    if (e?.shiftKey && isSelecting && anchor !== null) {
      const [from, to] = anchor < index ? [anchor, index] : [index, anchor];
      selectedIds = [...new Set([...selectedIds, ...visibleBooks.slice(from, to + 1).map((selected) => selected.id)])];
    } else if (isSelecting || modifier) {
      isSelecting = true;
      selectedIds = selectedIds.includes(book.id) ? selectedIds.filter((selected) => selected !== book.id) : [...selectedIds, book.id];
      anchor = index;
    } else if (!book.epub) {
      downloadBook(book);
    } else {
      invoke("gdrive_cancel_download");
      onOpen(book.id);
    }
  }

  $effect(() => {
    const unlisten = listen<{ id: string; progress: number }>("sync://download-progress", ({ payload }) => {
      if (downloadingBooks[payload.id] !== undefined) downloadingBooks[payload.id] = payload.progress;
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  async function downloadBook(book: BookMetadata) {
    if (downloadingBooks[book.id] !== undefined) return;
    downloadingBooks[book.id] = 0;
    try {
      const downloaded = await invoke<BookMetadata | null>("gdrive_download_book", { id: book.id });
      if (!downloaded) return;
      await onReload();
      const status = await invoke<GoogleDriveSyncStatus>("gdrive_sync_state");
      onOpen(downloaded.id, status.errorMessage === null);
    } catch (e) {
      toast?.show(String(e), "error");
    } finally {
      delete downloadingBooks[book.id];
    }
  }

  async function popupMenu(items: MenuOptions["items"]) {
    const menu = await Menu.new({ items });
    await menu.popup();
  }

  async function moveTo(ids: string[], name: string | null) {
    for (const id of ids) await moveBook(id, name);
    clearSelection();
  }

  function moveItems(ids: string[]) {
    return [
      { id: "move-none", text: "None", action: () => moveTo(ids, null) },
      ...shelves.list.map((shelf) => ({ id: `move-${shelf.name}`, text: shelf.name, action: () => moveTo(ids, shelf.name) })),
      { item: "Separator" as const },
      { id: "move-new", text: "New Shelf…", action: () => openNewShelf(ids) },
    ];
  }

  let newShelfDialog = $state<HTMLDialogElement>();
  let newShelfIds = $state<string[]>([]);
  let newShelfName = $state("");

  function openNewShelf(ids: string[]) {
    newShelfIds = ids;
    newShelfName = "";
    newShelfDialog?.showModal();
  }

  async function saveNewShelf() {
    const name = newShelfName.trim().normalize("NFC");
    if (!name) return;
    newShelfDialog?.close();
    await createShelf(name);
    await moveTo(newShelfIds, name);
  }

  async function deleteSelected() {
    if (!(await ask(`Delete ${selectedIds.length} book(s)?`, { kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" }))) return;
    for (const id of selectedIds) await onDelete(id);
    clearSelection();
  }

  async function deleteBook(book: BookMetadata) {
    const text = `Delete "${displayTitle(book)}"?`;
    if (syncConfig.enableSync && book.epub && (await invoke<boolean>("gdrive_published_epub", { id: book.id }))) {
      const result = await message(text, {
        kind: "warning",
        buttons: { yes: "Delete Local", no: "Delete Everywhere", cancel: "Cancel" },
      });
      if (result === "Delete Local") await onDeleteLocal(book.id);
      if (result === "Delete Everywhere") await onDelete(book.id);
      return;
    }
    const okLabel = syncConfig.enableSync ? "Delete Everywhere" : "Delete";
    if (!(await ask(text, { kind: "warning", okLabel, cancelLabel: "Cancel" }))) return;
    await onDelete(book.id);
  }

  async function markRead(books: BookMetadata[]) {
    const target = books.length === 1 ? `"${displayTitle(books[0])}"` : `${books.length} books`;
    if (!(await ask(`Mark ${target} as read?`, { okLabel: "Confirm", cancelLabel: "Cancel" }))) return;
    for (const book of books) await invoke("mark_book_read", { id: book.id });
    await onReload();
    clearSelection();
  }

  async function markUnread(books: BookMetadata[]) {
    const target = books.length === 1 ? `"${displayTitle(books[0])}"` : `${books.length} books`;
    if (!(await ask(`This will reset the bookmark and statistics for ${target}.`, {
      title: "Mark Unread?",
      kind: "warning",
      okLabel: "Mark Unread",
      cancelLabel: "Cancel",
    }))) return;
    for (const book of books) {
      const sessions = await invoke<Sessions>("load_statistics", { folder: book.folder });
      await invoke("delete_reading_sessions", { folder: book.folder, ids: Object.keys(sessions) });
      await invoke("save_bookmark", { id: book.id, chapterIndex: 0, progress: 0, characterCount: 0 });
    }
    await onReload();
    clearSelection();
  }

  async function markItems(books: BookMetadata[]) {
    const canMarkUnread = await Promise.all(books.map(async (book) => {
      const [bookmark, sessions] = await Promise.all([
        invoke<Bookmark | null>("load_bookmark", { id: book.id }),
        invoke<Sessions>("load_statistics", { folder: book.folder }),
      ]);
      return (bookmark !== null && (bookmark.chapterIndex > 0 || bookmark.progress > 0 || bookmark.characterCount > 0)) ||
        Object.values(sessions).some((session) => session.value !== null);
    }));
    const read = books.filter((book) => book.hasBookInfo && book.progress < 1);
    const unread = books.filter((_, index) => canMarkUnread[index]);
    return [
      { id: "mark-read", text: "Read", enabled: read.length > 0, action: () => markRead(read) },
      { id: "mark-unread", text: "Unread", enabled: unread.length > 0, action: () => markUnread(unread) },
    ];
  }

  let renameDialog = $state<HTMLDialogElement>();
  let renameId = $state<string | null>(null);
  let renameText = $state("");

  function openRename(book: BookMetadata) {
    renameId = book.id;
    renameText = displayTitle(book);
    renameDialog?.showModal();
  }

  function saveRename() {
    if (renameId) onRename(renameId, renameText.trim());
    renameDialog?.close();
  }

  let authorDialog = $state<HTMLDialogElement>();
  let authorId = $state<string | null>(null);
  let authorText = $state("");

  async function openEditAuthor(book: BookMetadata) {
    authorId = book.id;
    authorText = book.author ?? (await invoke<string | null>("epub_author", { id: book.id })) ?? "";
    authorDialog?.showModal();
  }

  function saveAuthor() {
    if (authorId) onEditAuthor(authorId, authorText.trim());
    authorDialog?.close();
  }

  let isSyncing = $state(false);
  let toast = $state<ReturnType<typeof Toast>>();

  async function syncBook(book: BookMetadata) {
    isSyncing = true;
    await invoke("gdrive_sync_book", { id: book.id });
    isSyncing = false;
  }

  async function showMenu(book: BookMetadata, e: MouseEvent) {
    e.preventDefault();
    if (isSelecting) {
      if (!selectedIds.includes(book.id)) return;
      await popupMenu([
        await Submenu.new({ text: "Move to", items: moveItems(selectedIds) }),
        await Submenu.new({ text: "Mark as", items: await markItems(selectedBooks) }),
        { id: "delete-selected", text: "Delete", action: deleteSelected },
      ]);
      return;
    }
    await popupMenu([
      ...(syncConfig.enableSync ? [{ id: `sync-${book.id}`, text: "Sync", action: () => syncBook(book) }] : []),
      await Submenu.new({ text: "Move to", items: moveItems([book.id]) }),
      await Submenu.new({ text: "Mark", items: await markItems([book]) }),
      { id: `rename-${book.id}`, text: "Rename", action: () => openRename(book) },
      { id: `author-${book.id}`, text: "Edit Author", action: () => openEditAuthor(book) },
      { id: `delete-${book.id}`, text: "Delete", action: () => deleteBook(book) },
    ]);
  }

  function lastRead(book: BookMetadata) {
    const label = relativeDate(book.lastAccess);
    return label.charAt(0).toUpperCase() + label.slice(1);
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape" && isSelecting) clearSelection();
  }}
/>

{#snippet status(book: BookMetadata)}
  <div class="flex items-center gap-1">
    <progress
      class="progress h-1 flex-1 text-base-content/60"
      value={Math.min(book.progress, 1) * 100}
      max="100"
    ></progress>
    <span class="w-10 shrink-0 text-right text-[11px] font-medium tabular-nums text-base-content/60">
      {(book.progress * 100).toFixed(1)}%
    </span>
  </div>
{/snippet}

{#snippet bookTile(book: BookMetadata, index: number)}
  <button
    onclick={(e) => selectBook(book, e)}
    oncontextmenu={(e) => showMenu(book, e)}
    onpointerdown={(e) => startDrag(e, index)}
    class="group -m-2 flex flex-col gap-1 rounded-lg p-2 text-left transition-colors hover:bg-primary/10 focus-visible:bg-primary/10 {canReorder
      ? 'cursor-grab'
      : ''} {sort.index === index ? 'opacity-40' : ''}"
  >
    <div
      class="relative aspect-[0.709] w-full overflow-hidden rounded-sm border border-base-300 bg-base-200 {isSelecting && selectedIds.includes(book.id) ? 'outline-2 outline-offset-2 outline-primary' : ''}"
    >
      <BookCover title={displayTitle(book)} author={book.author} src={book.cover ? schemeUrl("cover", book.id) + "?w=512" : null} />
      {#if isSelecting && selectedIds.includes(book.id)}
        <CircleCheck class="absolute right-1.5 top-1.5 size-[22px] rounded-full bg-blue-500 text-white" />
      {:else if isSelecting}
        <span class="absolute right-1.5 top-1.5 size-[22px] rounded-full border-2 border-white/85 bg-black/25"></span>
      {/if}
    </div>
    {#if source.kind !== "home"}
      {@render status(book)}
    {/if}
    <span class="{downloadingBooks[book.id] === undefined ? 'line-clamp-2' : 'line-clamp-1'} text-sm leading-snug transition-colors group-hover:text-secondary group-focus-visible:text-secondary">
      {#if !book.epub}
        <Cloud class="mb-0.5 inline size-3.5" />
      {/if}
      {displayTitle(book)}
    </span>
    {#if downloadingBooks[book.id] !== undefined}
      <div class="flex items-center gap-1 px-0.5 text-base-content/60">
        <CircleArrowDown class="size-3.5 shrink-0" />
        <progress
          class="progress h-1 flex-1"
          value={downloadingBooks[book.id] * 100}
          max="100"
        ></progress>
      </div>
    {/if}
  </button>
{/snippet}

{#snippet sortHeader(label: string, value: SortOption, className = "")}
  <th class={className}>
    <button class="inline-flex items-center gap-1 hover:text-base-content {option === value ? 'text-base-content' : ''}" onclick={() => sortBy(value)}>
      {label}
      {#if option === value}
        {#if (value === "Title") !== bookSort.reversed}
          <ArrowUp class="size-3" />
        {:else}
          <ArrowDown class="size-3" />
        {/if}
      {/if}
    </button>
  </th>
{/snippet}

{#snippet bookRow(book: BookMetadata, index: number)}
  <tr
    tabindex="0"
    onclick={(e) => selectBook(book, e)}
    onkeydown={(e) => e.key === "Enter" && selectBook(book)}
    oncontextmenu={(e) => showMenu(book, e)}
    onpointerdown={(e) => startDrag(e, index)}
    class="row-hover focus-visible:bg-base-200 {isSelecting && selectedIds.includes(book.id)
      ? 'bg-base-300'
      : ''} {canReorder ? 'cursor-grab' : 'cursor-pointer'} {sort.index === index ? 'opacity-40' : ''}"
  >
    <td>
      <div class="flex min-w-0 items-center gap-3">
        <span class="relative block h-[45px] w-8 shrink-0 overflow-hidden rounded-sm bg-base-200 ring-1 ring-black/5">
          <BookCover title={displayTitle(book)} author={book.author} src={book.cover ? schemeUrl("cover", book.id) + "?w=128" : null} />
        </span>
        <div class="flex min-w-0 flex-col">
          <span class="truncate">
            {#if !book.epub}
              <Cloud class="mb-0.5 inline size-3.5" />
            {/if}
            {displayTitle(book)}
          </span>
          {#if book.author}
            <span class="truncate text-xs text-base-content/60">{book.author}</span>
          {/if}
        </div>
      </div>
    </td>
    <td>
      {@render status(book)}
      {#if book.charactersTotal > 0}
        <div class="text-[11px] tabular-nums text-base-content/60">{formatNumber(book.charactersRead)} / {formatNumber(book.charactersTotal)}</div>
      {/if}
    </td>
    <td class="text-right text-xs tabular-nums text-base-content/60">
      {readingTimes[book.id] ? formatUnits(readingTimes[book.id]) : "—"}
    </td>
    <td class="text-right text-xs text-base-content/60">{book.progress > 0 ? lastRead(book) : "—"}</td>
  </tr>
{/snippet}

<div class="flex h-full flex-col bg-base-100">
  {#if source.kind === "home"}
    <Titlebar />
  {:else}
    <PageHeader title={isSelecting ? `${selectedIds.length} Selected` : sourceTitle(source)}>
      {#snippet actions()}
        {#if isSelecting}
          <button class="btn btn-ghost btn-sm" disabled={selectedIds.length === 0} onclick={() => popupMenu(moveItems(selectedIds))}>
            <Folder class="size-4" />
            Move to
          </button>
          <button class="btn btn-ghost btn-sm" disabled={selectedIds.length === 0} onclick={async () => popupMenu(await markItems(selectedBooks))}>
            <CircleCheck class="size-4" />
            Mark as
          </button>
          <button class="btn btn-ghost btn-sm enabled:text-error" disabled={selectedIds.length === 0} onclick={deleteSelected}>
            <Trash2 class="size-4" />
            Delete
          </button>
          <button class="btn btn-neutral btn-sm" onclick={clearSelection}>Done</button>
        {:else}
          <label class="input input-sm w-52">
            <Search class="size-4 text-base-content/50" />
            <input type="search" placeholder="Search" autocomplete="off" spellcheck="false" bind:value={query} />
          </label>
          <ViewMenu shelf={shelfName} />
          <button class="btn btn-ghost btn-sm btn-square" title="Select" onclick={() => (isSelecting = true)}><ListChecks class="size-4" /></button>
          <button class="btn btn-neutral btn-sm btn-square" onclick={onImport}>
            <Plus class="size-4" />
          </button>
        {/if}
      {/snippet}
    </PageHeader>
  {/if}

  {#if source.kind === "home" && library.length > 0}
    <ReadingOverview {books} {library} downloads={downloadingBooks} tile={bookTile} {onImport} onSelect={selectBook} onMenu={showMenu} {onStatistics} />
  {:else if books.length === 0}
    <div
      class="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center text-sm text-base-content/60"
    >
      No Books
      {#if source.kind === "home"}
        <button class="btn btn-neutral btn-sm" onclick={onImport}>
          <Plus class="size-4" />
          Import
        </button>
      {/if}
    </div>
  {:else if visibleBooks.length === 0}
    <div class="flex flex-1 items-center justify-center px-6 text-center text-sm text-base-content/60">
      No Results
    </div>
  {:else if shellConfig.bookshelfLayout === "List"}
    <div class="flex-1 overflow-y-auto">
      <table class="table table-sm table-pin-rows table-fixed [&_:is(th,td):first-child]:ps-5 [&_:is(th,td):last-child]:pe-5">
        <thead>
          <tr>
            {@render sortHeader("Title", "Title")}
            {@render sortHeader("Progress", "Progress", "w-1/4")}
            {@render sortHeader("Time Read", "Time Read", "w-28 text-right")}
            {@render sortHeader("Last Read", "Recent", "w-36 text-right")}
          </tr>
        </thead>
        <tbody use:sort.container>
          {#each visibleBooks as book, index (book.id)}
            {@render bookRow(book, index)}
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="flex-1 overflow-y-auto">
      <div
        class="grid content-start gap-5 p-5"
        style="grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));"
        use:sort.container
      >
        {#each visibleBooks as book, index (book.id)}
          {@render bookTile(book, index)}
        {/each}
      </div>
    </div>
  {/if}

  {#if isSyncing}
    <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/30">
      <div class="flex items-center gap-3 rounded-box bg-base-100 px-6 py-4 shadow-lg">
        <span class="loading loading-spinner loading-sm"></span>
        <span class="text-sm">Syncing...</span>
      </div>
    </div>
  {/if}

  <Toast bind:this={toast} />

  <dialog class="modal" bind:this={newShelfDialog}>
    <div class="modal-box">
      <h3 class="mb-4 text-base font-semibold">New Shelf</h3>
      <input
        class="input w-full"
        placeholder="Shelf name"
        bind:value={newShelfName}
        onkeydown={(e) => {
          if (e.key === "Enter") saveNewShelf();
        }}
      />
      <div class="modal-action">
        <form method="dialog">
          <button class="btn btn-sm">Cancel</button>
        </form>
        <button class="btn btn-neutral btn-sm" disabled={newShelfName.trim() === ""} onclick={saveNewShelf}>Create</button>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>

  <dialog class="modal" bind:this={renameDialog}>
    <div class="modal-box">
      <h3 class="mb-4 text-base font-semibold">Rename</h3>
      <input
        class="input w-full"
        placeholder="Title"
        bind:value={renameText}
        onkeydown={(e) => {
          if (e.key === "Enter") saveRename();
        }}
      />
      <div class="modal-action">
        <form method="dialog">
          <button class="btn btn-sm">Cancel</button>
        </form>
        <button class="btn btn-neutral btn-sm" onclick={saveRename}>Save</button>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>

  <dialog class="modal" bind:this={authorDialog}>
    <div class="modal-box">
      <h3 class="mb-4 text-base font-semibold">Edit Author</h3>
      <input
        class="input w-full"
        placeholder="Author"
        bind:value={authorText}
        onkeydown={(e) => {
          if (e.key === "Enter") saveAuthor();
        }}
      />
      <div class="modal-action">
        <form method="dialog">
          <button class="btn btn-sm">Cancel</button>
        </form>
        <button class="btn btn-neutral btn-sm" onclick={saveAuthor}>Save</button>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>
</div>
