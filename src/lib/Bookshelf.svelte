<script module lang="ts">
  const downloadingBooks = $state<Record<string, number>>({});
  let openTarget: string | null = null;
  let menuDownloads = Promise.resolve();
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Menu, Submenu, type MenuOptions } from "@tauri-apps/api/menu";
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import {
    Archive,
    ArrowDown,
    ArrowUp,
    CircleArrowDown,
    CircleCheck,
    Cloud,
    Folder,
    ListChecks,
    Plus,
    RotateCcw,
    Search,
    Trash2,
  } from "@lucide/svelte";
  import BookCover from "./BookCover.svelte";
  import PageHeader from "./PageHeader.svelte";
  import ReadingOverview from "./ReadingOverview.svelte";
  import Titlebar from "./Titlebar.svelte";
  import ViewMenu from "./ViewMenu.svelte";
  import { series } from "./series";
  import { resetSeriesOrder, saveSeries, seriesSort, seriesStore, setSeriesOrder, setSeriesSort } from "./seriesStore.svelte";
  import { isMac } from "./platform";
  import { relativeDate } from "./relativeDate";
  import { schemeUrl } from "./scheme";
  import { Sortable } from "./sortable.svelte";
  import { bookSort, reverseBookSort, setBookSort, sortBooks, sortOption } from "./bookSort.svelte";
  import {
    completionDate,
    createShelf,
    isCompleted,
    isFinished,
    moveBook,
    resetShelfOrder,
    saveShelfOrder,
    setShelfOrder,
    shelves,
    sourceTitle,
  } from "./shelves.svelte";
  import SettingToggle from "./SettingToggle.svelte";
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
    onEditSeries,
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
    onEditSeries: (id: string, series: string) => void;
    onReload: () => Promise<void>;
    onStatistics: () => void;
  } = $props();

  function displayTitle(book: BookMetadata) {
    return book.renamedTitle ?? book.title;
  }

  const shelfName = $derived(source.kind === "shelf" ? source.name : null);

  const seriesKey = $derived(source.kind === "series" ? source.key : null);
  const canCustom = $derived(shelfName !== null || seriesKey !== null);
  const option = $derived(seriesKey !== null ? seriesSort(seriesKey).option : sortOption(shelfName));
  const reversed = $derived(seriesKey !== null ? seriesSort(seriesKey).reversed : bookSort.reversed);
  const order = $derived(
    seriesKey !== null ? (seriesStore.orders[seriesKey] ?? []) : shelfName ? (shelves.orders[shelfName] ?? []) : [],
  );

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

  function sortCompleted(books: BookMetadata[]) {
    const dated = books
      .filter((book) => completionDate(book) !== null)
      .toSorted((a, b) => completionDate(b)!.getTime() - completionDate(a)!.getTime());
    const unknown = sortBooks(books.filter((book) => completionDate(book) === null), "Recent", []);
    return [...(bookSort.reversed ? dated.reverse() : dated), ...unknown];
  }

  const sortedBooks = $derived(
    source.kind === "completed" && option === "Recent"
      ? sortCompleted(books)
      : sortBooks(books, option, order, reversed, readingTimes),
  );

  function sortBy(next: SortOption) {
    if (seriesKey !== null) setSeriesSort(seriesKey, next);
    else if (option === next && next !== "Custom") reverseBookSort();
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
  const canReorder = $derived(canCustom && option === "Custom" && isSelecting && !query.trim());

  const sort = new Sortable({
    commit: () => {
      if (seriesKey !== null) saveSeries();
      else if (shelfName) saveShelfOrder(shelfName);
    },
    threshold: 5,
    move: (from, to) => {
      const folders = sortedBooks.map((book) => book.folder);
      folders.splice(to, 0, folders.splice(from, 1)[0]);
      if (seriesKey !== null) setSeriesOrder(seriesKey, folders);
      else if (shelfName) setShelfOrder(shelfName, folders);
    },
  });

  async function resetOrder() {
    if (!(await ask(seriesKey !== null ? "This restores the default order." : "This restores the default order on all devices.", {
      title: "Reset Order?",
      kind: "warning",
      okLabel: "Reset",
      cancelLabel: "Cancel",
    }))) return;
    if (seriesKey !== null) resetSeriesOrder(seriesKey);
    else if (shelfName) await resetShelfOrder(shelfName);
  }

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
    } else if (book.epubRemoved) {
      toast?.show("The EPUB of this book was removed.", "error");
    } else if (!book.epub) {
      openTarget = book.id;
      downloadBook(book, true);
    } else {
      openTarget = null;
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

  async function downloadBook(book: BookMetadata, open: boolean) {
    if (downloadingBooks[book.id] !== undefined) return;
    downloadingBooks[book.id] = 0;
    try {
      const downloaded = await invoke<BookMetadata | null>("gdrive_download_book", { id: book.id });
      if (!downloaded) return;
      await onReload();
      if (!open || openTarget !== book.id) return;
      openTarget = null;
      const status = await invoke<GoogleDriveSyncStatus>("gdrive_sync_state");
      onOpen(downloaded.id, status.errorMessage === null);
    } catch (e) {
      toast?.show(String(e), "error");
    } finally {
      delete downloadingBooks[book.id];
    }
  }

  function queueDownloads(books: BookMetadata[]) {
    for (const book of books) {
      menuDownloads = menuDownloads.then(() => downloadBook(book, false));
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
    if (await deleteBooks(selectedBooks, `Delete ${selectedBooks.length} book(s)?`)) clearSelection();
  }

  async function deleteBooks(books: BookMetadata[], text: string) {
    const published = syncConfig.enableSync
      ? await Promise.all(books.map((book) => !!book.epub && invoke<boolean>("gdrive_published_epub", { id: book.id })))
      : [];
    const local = books.filter((_, index) => published[index]);
    let choice: string | null = null;
    if (local.length > 0) {
      choice = await message(text, {
        kind: "warning",
        buttons: { yes: "Delete Local", no: "Delete Everywhere", cancel: "Cancel" },
      });
    } else if (await ask(text, { kind: "warning", okLabel: syncConfig.enableSync ? "Delete Everywhere" : "Delete", cancelLabel: "Cancel" })) {
      choice = "Delete Everywhere";
    }
    if (choice === "Delete Local") for (const book of local) await onDeleteLocal(book.id);
    else if (choice === "Delete Everywhere") for (const book of books) await onDelete(book.id);
    else return false;
    return true;
  }

  let completionDialog = $state<HTMLDialogElement>();
  let completionBooks = $state<BookMetadata[]>([]);
  let completionEditing = $state(false);
  let completionDay = $state("");
  let completionUnknown = $state(false);
  let completionRemove = $state(false);
  let completionSize = $state<number | null>(null);

  function dayKey(date: Date) {
    const pad = (value: number) => String(value).padStart(2, "0");
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  }

  const today = $derived(dayKey(new Date()));

  function formatSize(bytes: number) {
    return bytes >= 1e6 ? `${(bytes / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1e3))} KB`;
  }

  async function openCompletion(books: BookMetadata[]) {
    const book = books.length === 1 ? books[0] : null;
    const date = book ? completionDate(book) : null;
    completionBooks = books;
    completionEditing = book !== null && isCompleted(book);
    completionDay = dayKey(date ?? new Date());
    completionUnknown = completionEditing && date === null;
    completionRemove = false;
    const sizes = await Promise.all(
      books.filter((book) => book.epub).map((book) => invoke<number | null>("book_epub_size", { id: book.id })),
    );
    const known = sizes.filter((size) => size !== null);
    completionSize = known.length > 0 ? known.reduce((total, size) => total + size, 0) : null;
    completionDialog?.showModal();
  }

  async function saveCompletion() {
    const [year, month, day] = completionDay.split("-").map(Number);
    const completed = completionUnknown || !year ? 0 : new Date(year, month - 1, day).getTime();
    completionDialog?.close();
    try {
      for (const book of completionBooks) {
        if (completionEditing) await invoke("set_book_completed", { id: book.id, completed });
        else await invoke("mark_book_read", { id: book.id, completed });
        if (completionRemove && book.epub) await invoke("remove_book_epub", { id: book.id });
      }
    } catch (e) {
      toast?.show(String(e), "error");
    }
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
      await invoke("set_book_completed", { id: book.id, completed: null });
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
      return isCompleted(book) ||
        (bookmark !== null && (bookmark.chapterIndex > 0 || bookmark.progress > 0 || bookmark.characterCount > 0)) ||
        Object.values(sessions).some((session) => session.value !== null);
    }));
    const unread = books.filter((_, index) => canMarkUnread[index]);
    const unreadItem = { id: "mark-unread", text: "Unread", enabled: unread.length > 0, action: () => markUnread(unread) };
    if (books.length === 1 && isCompleted(books[0])) {
      return [{ id: "completion-date", text: "Completion Date…", action: () => openCompletion(books) }, unreadItem];
    }
    const read = books.filter((book) => !isCompleted(book) && (book.hasBookInfo || (book.characterCount ?? 0) > 0));
    return [{ id: "mark-read", text: "Read…", enabled: read.length > 0, action: () => openCompletion(read) }, unreadItem];
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

  let seriesDialog = $state<HTMLDialogElement>();
  let seriesBook = $state<BookMetadata | null>(null);
  let seriesName = $state("");
  let seriesText = $state("");

  function openEditSeries(book: BookMetadata) {
    seriesBook = book;
    seriesName = series(library, seriesStore.names).find((group) => group.books.some((entry) => entry.id === book.id))?.name ?? "";
    seriesText = seriesName;
    seriesDialog?.showModal();
  }

  function saveEditSeries() {
    const text = seriesText.trim();
    if (seriesBook && (seriesBook.series || text !== seriesName)) onEditSeries(seriesBook.id, text);
    seriesDialog?.close();
  }

  let toast = $state<ReturnType<typeof Toast>>();

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
      ...(syncConfig.enableSync && !book.epub && !book.epubRemoved
        ? [{ id: `download-${book.id}`, text: "Download", action: () => queueDownloads([book]) }]
        : []),
      await Submenu.new({ text: "Move to", items: moveItems([book.id]) }),
      await Submenu.new({ text: "Mark", items: await markItems([book]) }),
      { id: `rename-${book.id}`, text: "Rename", action: () => openRename(book) },
      { id: `author-${book.id}`, text: "Edit Author", action: () => openEditAuthor(book) },
      ...(shellConfig.showSeries
        ? [{ id: `series-${book.id}`, text: "Edit Series", action: () => openEditSeries(book) }]
        : []),
      { id: `delete-${book.id}`, text: "Delete", action: () => deleteBooks([book], `Delete "${displayTitle(book)}"?`) },
    ]);
  }

  function charactersRead(book: BookMetadata) {
    return isFinished(book) ? book.charactersTotal : book.charactersRead;
  }

  function characters(book: BookMetadata) {
    return `${formatNumber(charactersRead(book))} / ${formatNumber(book.charactersTotal)}`;
  }

  const shelfCharacters = $derived(
    books.reduce(
      (sum, book) => ({
        read: sum.read + charactersRead(book),
        total: sum.total + book.charactersTotal,
      }),
      { read: 0, total: 0 },
    ),
  );

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

{#snippet status(book: BookMetadata, bar = true, percentage = true)}
  <div class="flex h-4 items-center gap-1">
    {#if bar}
      <progress
        class="progress h-1 flex-1 text-base-content/60"
        value={Math.min(book.progress, 1) * 100}
        max="100"
      ></progress>
    {/if}
    {#if percentage}
      <span class="{bar ? 'w-10 shrink-0 text-right' : ''} text-[11px] font-medium tabular-nums text-base-content/60">
        {(book.progress * 100).toFixed(1)}%
      </span>
    {/if}
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
    <BookCover
      bunko
      title={displayTitle(book)}
      author={book.author}
      src={book.cover ? schemeUrl("cover", book.id) + "?w=512" : null}
      frontClass="bg-base-200 {isSelecting && selectedIds.includes(book.id) ? 'outline-2 outline-offset-2 outline-primary' : ''}"
    >
      {#if isSelecting && selectedIds.includes(book.id)}
        <CircleCheck class="absolute right-1.5 top-1.5 size-[22px] rounded-full bg-blue-500 text-white" />
      {:else if isSelecting}
        <span class="absolute right-1.5 top-1.5 size-[22px] rounded-full border-2 border-white/85 bg-black/25"></span>
      {:else if isFinished(book)}
        <CircleCheck class="absolute bottom-1.5 right-1.5 size-[22px] rounded-full bg-[#999] text-white" />
      {/if}
    </BookCover>
    {#if source.kind !== "home" && (shellConfig.bookshelfShowProgressBar || shellConfig.bookshelfShowPercentage || (shellConfig.bookshelfShowCharacters && book.charactersTotal > 0))}
      <div class="mt-0.5 flex flex-col">
        {#if shellConfig.bookshelfShowProgressBar || shellConfig.bookshelfShowPercentage}
          {@render status(book, shellConfig.bookshelfShowProgressBar, shellConfig.bookshelfShowPercentage)}
        {/if}
        {#if shellConfig.bookshelfShowCharacters && book.charactersTotal > 0}
          <span class="text-[11px] leading-4 tabular-nums text-base-content/60">{characters(book)}</span>
        {/if}
      </div>
    {/if}
    {#if source.kind === "home" || shellConfig.bookshelfShowTitle}
      <span class="{downloadingBooks[book.id] === undefined ? 'line-clamp-2' : 'line-clamp-1'} text-sm leading-snug transition-colors group-hover:text-secondary group-focus-visible:text-secondary">
        {#if book.epubRemoved}
          <Archive class="mb-0.5 inline size-3.5" />
        {:else if !book.epub}
          <Cloud class="mb-0.5 inline size-3.5" />
        {/if}
        {displayTitle(book)}
      </span>
    {/if}
    {#if source.kind !== "home" && shellConfig.bookshelfShowAuthor && book.author}
      <span class="truncate text-xs text-base-content/60">{book.author}</span>
    {/if}
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

{#snippet characterTotal()}
  <span>
    {formatNumber(shelfCharacters.read)} / {formatNumber(shelfCharacters.total)}<span class="hidden @2xl:inline">{" "}characters</span>
  </span>
{/snippet}

{#snippet sortHeader(label: string, value: SortOption, className = "")}
  <th class={className}>
    <button class="inline-flex items-center gap-1 hover:text-base-content {option === value ? 'text-base-content' : ''}" onclick={() => sortBy(value)}>
      {label}
      {#if option === value}
        {#if (value === "Title" || value === "Author") !== reversed}
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
        <span class="block w-8 shrink-0">
          <BookCover bunko title={displayTitle(book)} author={book.author} src={book.cover ? schemeUrl("cover", book.id) + "?w=128" : null} frontClass="bg-base-200" />
        </span>
        <div class="flex min-w-0 flex-col">
          <span class="truncate">
            {#if book.epubRemoved}
              <Archive class="mb-0.5 inline size-3.5" />
            {:else if !book.epub}
              <Cloud class="mb-0.5 inline size-3.5" />
            {/if}
            {displayTitle(book)}
          </span>
          {#if downloadingBooks[book.id] !== undefined}
            <div class="flex h-4 w-40 items-center gap-1 text-base-content/60">
              <CircleArrowDown class="size-3.5 shrink-0" />
              <progress
                class="progress h-1 flex-1"
                value={downloadingBooks[book.id] * 100}
                max="100"
              ></progress>
            </div>
          {:else if book.author}
            <span class="truncate text-xs text-base-content/60">{book.author}</span>
          {/if}
        </div>
      </div>
    </td>
    <td>
      {@render status(book)}
      {#if book.charactersTotal > 0}
        <div class="text-[11px] tabular-nums text-base-content/60 @3xl:hidden">{characters(book)}</div>
      {/if}
    </td>
    <td class="hidden text-right text-xs tabular-nums text-base-content/60 @3xl:table-cell">
      {book.charactersTotal > 0 ? characters(book) : "—"}
    </td>
    <td class="text-right text-xs tabular-nums text-base-content/60">
      {readingTimes[book.id] ? formatUnits(readingTimes[book.id]) : "—"}
    </td>
    <td class="text-right text-xs text-base-content/60">{book.progress > 0 ? lastRead(book) : "—"}</td>
  </tr>
{/snippet}

<div class="@container flex h-full flex-col bg-base-100">
  {#if source.kind === "home"}
    <Titlebar />
  {:else}
    <PageHeader
      title={isSelecting ? `${selectedIds.length} Selected` : sourceTitle(source)}
      subtitle={!isSelecting && shelfCharacters.total > 0 && (shellConfig.bookshelfLayout === "List" || shellConfig.bookshelfShowCharacters)
        ? characterTotal
        : undefined}
    >
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
          {#if canCustom && option === "Custom"}
            <button class="btn btn-ghost btn-sm" disabled={order.length === 0} onclick={resetOrder}>
              <RotateCcw class="size-4" />
              Reset Order
            </button>
          {/if}
          <button class="btn btn-neutral btn-sm" onclick={clearSelection}>Done</button>
        {:else}
          <label class="input input-sm w-52 min-w-32">
            <Search class="size-4 text-base-content/50" />
            <input type="search" placeholder="Search" autocomplete="off" spellcheck="false" bind:value={query} />
          </label>
          <ViewMenu {option} {reversed} custom={canCustom} onpick={sortBy} />
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
            {@render sortHeader("Characters", "Characters", "hidden w-40 text-right @3xl:table-cell")}
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
    <div class="flex-1 overflow-y-auto [scrollbar-gutter:stable]">
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

  <dialog class="modal" bind:this={seriesDialog}>
    <div class="modal-box">
      <h3 class="mb-4 text-base font-semibold">Edit Series</h3>
      <input
        class="input w-full"
        placeholder="Series"
        bind:value={seriesText}
        onkeydown={(e) => {
          if (e.key === "Enter") saveEditSeries();
        }}
      />
      <div class="modal-action">
        <form method="dialog">
          <button class="btn btn-sm">Cancel</button>
        </form>
        <button class="btn btn-neutral btn-sm" onclick={saveEditSeries}>Save</button>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>

  <dialog class="modal" bind:this={completionDialog}>
    <div class="modal-box flex flex-col gap-4">
      <h3 class="text-base font-semibold">Mark as Read</h3>
      {#if completionBooks.length === 1}
        {@const book = completionBooks[0]}
        <div class="flex min-w-0 items-center gap-3">
          <span class="block w-12 shrink-0">
            <BookCover bunko title={displayTitle(book)} author={book.author} src={book.cover ? schemeUrl("cover", book.id) + "?w=128" : null} frontClass="bg-base-200" />
          </span>
          <div class="flex min-w-0 flex-col">
            <span class="line-clamp-2 text-sm">{displayTitle(book)}</span>
            {#if book.author}
              <span class="truncate text-xs text-base-content/60">{book.author}</span>
            {/if}
          </div>
        </div>
      {:else}
        <p class="text-sm text-base-content/60">{completionBooks.length} books</p>
      {/if}

      {#if completionSize !== null}
        <div class="flex flex-col gap-1">
          <SettingToggle label={completionBooks.length === 1 ? "Delete EPUB" : "Delete EPUBs"} bind:checked={completionRemove} onchange={() => {}} />
          <span class="text-xs text-base-content/60">
            Frees {formatSize(completionSize)} on this device{syncConfig.enableSync ? " and on Google Drive" : ""}.
          </span>
        </div>
      {/if}

      <SettingToggle label="Unknown Date" bind:checked={completionUnknown} onchange={() => {}} />
      {#if !completionUnknown}
        <label class="flex items-center justify-between gap-3 text-sm">
          Completion Date
          <input type="date" class="input input-sm w-44" max={today} bind:value={completionDay} />
        </label>
      {/if}

      <div class="modal-action mt-0">
        <form method="dialog">
          <button class="btn btn-sm">Cancel</button>
        </form>
        <button class="btn btn-neutral btn-sm" disabled={!completionUnknown && !completionDay} onclick={saveCompletion}>Done</button>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>
</div>
