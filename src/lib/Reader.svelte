<script lang="ts">
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Menu } from "@tauri-apps/api/menu";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { message, open, save } from "@tauri-apps/plugin-dialog";
  import {
    ArrowLeft,
    ArrowRightToLine,
    AudioLines,
    Bookmark as BookmarkIcon,
    Captions,
    ChartLine,
    Download,
    FastForward,
    Highlighter,
    Images,
    List,
    Maximize,
    Minimize,
    Pause,
    Play,
    Rewind,
    RotateCcw,
    RotateCw,
    Search,
    Timer,
    TimerOff,
    Upload,
    Volume1,
    Volume2,
    VolumeX,
    X,
    ZoomIn,
    ZoomOut,
  } from "@lucide/svelte";
  import Popup from "./Popup.svelte";
  import ZoomableImage from "./ZoomableImage.svelte";
  import BookSearch from "./BookSearch.svelte";
  import HighlightList from "./HighlightList.svelte";
  import { highlightColors, type BookHighlight } from "./highlights";
  import { isChromium, isMac } from "./platform";
  import { schemeUrl } from "./scheme";
  import { readerConfig } from "./readerConfig.svelte";
  import Appearance from "./Appearance.svelte";
  import SasayakiSettings from "./SasayakiSettings.svelte";
  import SettingGroup from "./SettingGroup.svelte";
  import SettingSlider from "./SettingSlider.svelte";
  import { infoColor, readerBackground, readerTextColor, resolvedScheme } from "./theme.svelte";
  import { sasayakiConfig } from "./sasayakiConfig.svelte";
  import { SasayakiPlayer } from "./sasayakiPlayer.svelte";
  import MatchDialog from "./MatchDialog.svelte";
  import { dictConfig } from "./dictConfig.svelte";
  import { hotkeyConfig, normalizeHotkey, readerHotkeys } from "./hotkeyConfig.svelte";
  import { statsConfig } from "./statsConfig.svelte";
  import { syncConfig } from "./syncConfig.svelte";
  import { StatsTracker } from "./statsTracker.svelte";
  import { readingSpeed } from "./statsModel.svelte";
  import { calculatePopupLayout, type PopupPlacement } from "./popupLayout";
  import { cursorExitHider } from "./popupHover";
  import type {
    BookInfo,
    BookMetadata,
    Bookmark,
    BookSearchResult,
    FontInfo,
    KanjiResponse,
    LookupEntry,
    LookupResponse,
    MineContent,
    PopupAnkiConfig,
    SasayakiMatch,
    SyncBook,
    SelectionRect,
    TocItem,
  } from "./types";

  let {
    id,
    folder,
    title,
    spine,
    toc,
    cover,
    savedHighlights,
    bookInfo,
    bookmark,
    skipSyncOnOpen,
    onClose,
  }: {
    id: string;
    folder: string;
    title: string;
    spine: string[];
    toc: TocItem[];
    cover: string | null;
    savedHighlights: BookHighlight[];
    bookInfo: BookInfo;
    bookmark: Bookmark | null;
    skipSyncOnOpen: boolean;
    onClose: () => void;
  } = $props();

  const syncKey = $derived(folder.normalize("NFC"));
  const vertical = $derived(readerConfig.verticalWriting);
  const readerBg = $derived(readerBackground());
  const readerText = $derived(readerTextColor());
  const readerInfo = $derived(infoColor());
  const sasayakiTextColor = $derived(
    resolvedScheme() === "dark"
      ? sasayakiConfig.sasayakiDarkTextColor
      : sasayakiConfig.sasayakiTextColor,
  );
  const sasayakiBackgroundColor = $derived(
    resolvedScheme() === "dark"
      ? sasayakiConfig.sasayakiDarkBackgroundColor
      : sasayakiConfig.sasayakiBackgroundColor,
  );

  let index = $state((() => bookmark?.chapterIndex ?? 0)());
  let progress = $state((() => bookmark?.progress ?? 0)());

  let loading = $state(true);
  let bookDeleted = false;
  let applyingBookmark = false;
  let fullscreen = $state(false);
  let readerPanel = $state<"Chapters" | "Search" | "Highlights" | "Appearance" | "Sasayaki" | null>(null);
  let leftPanel = $state<typeof readerPanel>(null);
  let rightPanel = $state<typeof readerPanel>(null);
  let wasPaused = $state(false);
  let pressHeld = false;
  let pressLookup: Promise<void> | null = null;
  let showBar = $state(false);

  let titleDownAt: { x: number; y: number } | null = null;
  let titleDragging = false;

  function onTitleBarPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    if (e.detail >= 2) {
      titleDownAt = null;
      getCurrentWindow().toggleMaximize();
      return;
    }
    titleDownAt = { x: e.clientX, y: e.clientY };
    titleDragging = false;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onTitleBarPointerMove(e: PointerEvent) {
    if (!titleDownAt || titleDragging) return;
    const dx = e.clientX - titleDownAt.x;
    const dy = e.clientY - titleDownAt.y;
    if (dx * dx + dy * dy > 16) {
      titleDragging = true;
      getCurrentWindow().startDragging();
    }
  }

  function onTitleBarPointerUp() {
    if (titleDownAt && !titleDragging) showBar = !showBar;
    titleDownAt = null;
    titleDragging = false;
  }
  let contentEl: HTMLDivElement;
  let pendingFragment: string | null = null;
  let frameWidth = $state(0);
  let frameHeight = $state(0);
  const spreadGap = 48;
  const spreadMode = $derived(readerConfig.spreadLayout && !readerConfig.paragraphMode && frameWidth > frameHeight);

  function spreadAt(spineIndex: number) {
    return spreadMode && bookInfo.chapterInfo[spine[spineIndex]]?.chapterCount !== 0;
  }

  function effectivePadding(padding: number, max: number, size: number) {
    return Math.max(padding, max ? 100 - (max / size) * 100 : 0);
  }

  function spreadInset(spread: boolean) {
    return spread
      ? Math.round(
          Math.max(
            (frameWidth * readerConfig.horizontalPadding) / 200,
            readerConfig.maxWidth ? (frameWidth - spreadGap) / 2 - readerConfig.maxWidth : 0,
          ),
        ) -
          spreadGap / 2
      : 0;
  }

  function frameStyle(inset: number) {
    return `left: ${inset}px; width: calc(100% - ${2 * inset}px); height: ${vertical && !isChromium ? `calc(100% + ${readerConfig.fontSize}px)` : "100%"}`;
  }

  const spread = $derived(spreadAt(index));
  const frameInset = $derived(spreadInset(spread));

  function layoutQuery(spread: boolean) {
    const mode = vertical ? "vertical" : "horizontal";
    return (
      `mode=${mode}&fs=${readerConfig.fontSize}&hp=${readerConfig.horizontalPadding}&vp=${readerConfig.verticalPadding}&mw=${readerConfig.maxWidth}&mh=${readerConfig.maxHeight}` +
      `&j=${readerConfig.justifyText ? 1 : 0}&apb=${readerConfig.avoidPageBreak ? 1 : 0}` +
      `&adv=${readerConfig.layoutAdvanced ? 1 : 0}&lh=${readerConfig.lineHeight}` +
      `&cs=${readerConfig.characterSpacing}&ps=${readerConfig.paragraphSpacing}` +
      `&fm=${readerConfig.furiganaMode}&bi=${readerConfig.blurImages ? 1 : 0}` +
      `&pm=${readerConfig.paragraphMode ? 1 : 0}&spp=${readerConfig.maxSentencesPerPage}&sd=${readerConfig.splitDialogue ? 1 : 0}` +
      `&font=${encodeURIComponent(readerConfig.selectedFont)}` +
      `&ffile=${encodeURIComponent(readerConfig.selectedFontFile)}&sp=${spread ? spreadGap : 0}`
    );
  }

  function frameSrc() {
    const href = encodeURI(spine[index]);
    return schemeUrl(
      "book",
      `${id}/${href}?${layoutQuery(spread)}` +
        `&ta=${readerConfig.textAnimation ? 1 : 0}&ts=${readerConfig.textSpeed}&ca=${readerConfig.clickToAdvance ? 1 : 0}` +
        `&sl=${dictConfig.scanLength}&snj=${dictConfig.scanNonJapaneseText ? 1 : 0}` +
        `&mod=${encodeURIComponent(hotkeyConfig.scanModifier)}&cl=${hotkeyConfig.clickLookup}` +
        `&sdl=${hotkeyConfig.scanDelay}&cz=${hotkeyConfig.pageClickZone}` +
        `&tc=${readerText ? readerText.slice(1) : ""}` +
        `&stc=${encodeURIComponent(sasayakiTextColor)}&sbc=${encodeURIComponent(sasayakiBackgroundColor)}`,
    );
  }

  const sasayaki = (() =>
    sasayakiConfig.enableSasayaki
      ? new SasayakiPlayer(
          id,
          {
            highlightCue: (cueId, reveal) =>
              postToFrame({ hoshi: "sasayaki-highlight", id: cueId, reveal }),
            clearCue: () => postToFrame({ hoshi: "sasayaki-clear" }),
            scrollToImage: (imageIndex) =>
              postToFrame({ hoshi: "sasayaki-image", index: imageIndex }),
          },
          (chapterIndex) => navigateTo(chapterIndex, sasayakiCueProgress(chapterIndex) ?? 0),
          () => index,
          {
            title,
            cover: cover ? schemeUrl("cover", id) : null,
            chapter: (cue) => {
              const character = (bookInfo.chapterInfo[spine[cue.chapterIndex]]?.currentTotal ?? 0) + cue.start;
              return toc.findLast((item) => (chapterChars(item) ?? Infinity) <= character)?.label ?? null;
            },
          },
        )
      : null)();

  let matchDialog = $state<ReturnType<typeof MatchDialog>>();

  async function onMatched() {
    if (!sasayaki) return;
    sasayaki.teardown();
    await sasayaki.load();
    pushFrame();
  }

  function sasayakiCueProgress(chapterIndex: number): number | null {
    const cue = sasayaki?.pendingCue;
    if (!cue || cue.chapterIndex !== chapterIndex) return null;
    const info = bookInfo.chapterInfo[spine[chapterIndex]];
    if (!info || info.chapterCount <= 0) return null;
    return cue.start / info.chapterCount;
  }

  function formatTime(seconds: number): string {
    const total = Math.floor(seconds);
    return [total / 3600, (total % 3600) / 60, total % 60]
      .map((part) => String(Math.floor(part)).padStart(2, "0"))
      .join(":");
  }

  async function loadSasayakiAudio() {
    const result = await open({
      filters: [{ name: "Audio", extensions: ["mp3", "m4b", "mp4"] }],
    });
    if (!result || !sasayaki) return;
    await sasayaki.importAudio(result);
  }

  const syncOnOpen = syncConfig.enableSync;

  const blankSrc = "about:blank";

  let frameSeq = 0;
  let frames = $state<{ key: number; src: string }[]>([
    { key: 0, src: syncOnOpen || sasayaki ? blankSrc : frameSrc() },
  ]);
  const frameEls = new Map<number, HTMLIFrameElement>();
  const frameWheelAt = new Map<number, number>();
  let frameCleanupTimer = 0;

  function frameRef(el: HTMLIFrameElement, key: number) {
    frameEls.set(key, el);
    return {
      destroy() {
        frameEls.delete(key);
        frameWheelAt.delete(key);
      },
    };
  }

  function activeEl() {
    const frame = frames[frames.length - 1];
    return frame ? frameEls.get(frame.key) : undefined;
  }

  function cleanupFrames() {
    const now = performance.now();
    const keep = frames.filter(
      (frame, i) =>
        i === frames.length - 1 || now - (frameWheelAt.get(frame.key) ?? 0) < 600,
    );
    if (keep.length !== frames.length) frames = keep;
    if (frames.length > 1) scheduleFrameCleanup();
  }

  function scheduleFrameCleanup() {
    clearTimeout(frameCleanupTimer);
    frameCleanupTimer = window.setTimeout(cleanupFrames, 700);
  }

  function pushFrame() {
    currentPage = null;
    sasayaki?.prepareTransition();
    loading = true;
    frames = [...frames, { key: ++frameSeq, src: frameSrc() }];
    scheduleFrameCleanup();
  }

  let frozen = $state<{ w: number; h: number } | null>(null);
  let resizing = $state(false);
  let settleTimer = 0;

  let pagesFrame = $state<HTMLIFrameElement>();
  let pagesSrc = $state<string | null>(null);
  let pagesSpread = $state(false);
  let pagesRun = 0;
  let measuredStarts: number[][] = [];
  let pageStarts = $state.raw<number[]>([]);
  let spineFirstPages = $state.raw<number[]>([]);
  let currentPage = $state<number | null>(null);

  const showPages = $derived(readerConfig.progressCount === "Pages");
  const topProgress = $derived(spreadMode && readerConfig.spreadTopProgress);
  const layout = $derived(`${layoutQuery(spreadMode)}&w=${frameWidth}&h=${frameHeight}`);

  function updatePages(starts: number[][]) {
    const all: number[] = [];
    const firstPages: number[] = [];
    starts.forEach((spineStarts, spineIndex) => {
      firstPages.push(all.length);
      const spineStart = bookInfo.chapterInfo[spine[spineIndex]]?.currentTotal ?? 0;
      for (const start of spineStarts) all.push(spineStart + start);
    });
    spineFirstPages = firstPages;
    pageStarts = all;
  }

  function measureSpine() {
    pagesSpread = spreadAt(measuredStarts.length);
    pagesSrc = schemeUrl(
      "book",
      `${id}/${encodeURI(spine[measuredStarts.length])}?${layoutQuery(pagesSpread)}&pages=${++pagesRun}`,
    );
  }

  function onSpineMeasured(starts: number[]) {
    measuredStarts.push(starts);
    if (measuredStarts.length < spine.length) {
      measureSpine();
      return;
    }
    invoke("save_pages", { id, pages: { layout, pageStarts: measuredStarts } });
    updatePages(measuredStarts);
    pagesSrc = null;
  }

  $effect(() => {
    if (!(showPages || (topProgress && readerConfig.progressCount === "Characters")) || resizing || !frameWidth || !frameHeight) return;
    const key = layout;
    let cancelled = false;
    invoke<{ layout: unknown; pageStarts: number[][] } | null>("load_pages", { id }).then((cache) => {
      if (cancelled) return;
      if (cache?.layout === key) {
        updatePages(cache.pageStarts);
      } else {
        measuredStarts = [];
        measureSpine();
      }
    });
    return () => {
      cancelled = true;
      pagesRun++;
      pagesSrc = null;
      updatePages([]);
    };
  });

  function pageAt(character: number, spineIndex: number) {
    const firstPage = spineFirstPages[spineIndex];
    const endPage = spineFirstPages[spineIndex + 1] ?? pageStarts.length;
    for (let page = endPage - 1; page > firstPage; page--) {
      if (pageStarts[page] <= character) return page;
    }
    return firstPage;
  }

  function positionLabel(character: number, spineIndex?: number) {
    spineIndex ??= resolveCharacterPosition(character)?.spineIndex;
    if (!showPages || !pageStarts.length || spineIndex === undefined) return String(character);
    return String(pageAt(character, spineIndex) + 1);
  }

  type PopupInstance = {
    id: string;
    entries: LookupEntry[];
    styles: Record<string, string>;
    placement: PopupPlacement;
    anchor: SelectionRect | null;
    anchorVertical: boolean;
    sasayakiCue: SasayakiMatch | null;
    sentence: string;
    clozeOffset: number | null;
  };

  let marker = $state<{ x: number; y: number; inset: number } | null>(null);
  let lookupSeq = 0;
  let popups = $state<PopupInstance[]>([]);
  let popupAnki = $state<PopupAnkiConfig | null>(null);
  const hider = cursorExitHider(closePopups);

  function postToFrame(message: unknown) {
    activeEl()?.contentWindow?.postMessage(message, "*");
  }

  function toggleFullscreen() {
    fullscreen = !fullscreen;
    getCurrentWindow().setFullscreen(fullscreen);
  }

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

  function resumeAfterPopups() {
    if (popups.length || pressHeld) return;
    if (wasPaused && sasayaki && !sasayaki.isPlaying) sasayaki.togglePlayback();
    wasPaused = false;
  }

  async function releasePress() {
    const lookup = pressLookup;
    await lookup;
    if (pressLookup !== lookup) return;
    pressHeld = false;
    resumeAfterPopups();
  }

  function toggleSasayakiPlayback() {
    if (wasPaused) wasPaused = false;
    else sasayaki?.togglePlayback();
  }

  function closePopups(keep = 0, clearSelection = true) {
    lookupSeq++;
    if (popups.length <= keep) return;
    popups = popups.slice(0, keep);
    if (!popups.length && clearSelection) postToFrame({ hoshi: "clear-selection" });
    resumeAfterPopups();
  }

  function relayoutPopups() {
    if (!popups.length) return;
    popups = popups.map((p) =>
      p.anchor ? { ...p, placement: placePopup(p.anchor, p.anchorVertical) } : p,
    );
  }

  function placePopup(rect: SelectionRect, popupVertical: boolean): PopupPlacement {
    const area = contentEl.getBoundingClientRect();
    return calculatePopupLayout(
      rect,
      { width: window.innerWidth, height: window.innerHeight },
      readerConfig.popupWidth,
      readerConfig.popupHeight,
      popupVertical,
      area.top,
      window.innerHeight - area.bottom,
    );
  }

  async function openLookup(
    text: string,
    rect: SelectionRect | null,
    normalizedOffset: number | null = null,
    sentence: string = "",
    offset: number | null = null,
  ) {
    hider.cancel();
    const seq = ++lookupSeq;
    const response = await lookup(text);
    if (seq !== lookupSeq || !response.entries.length) return;
    popupAnki = await invoke<PopupAnkiConfig>("anki_config");
    if (seq !== lookupSeq) return;
    let placement = popups[0]?.placement ?? { left: 0, top: 0, width: 0, height: 0 };
    let anchor = popups[0]?.anchor ?? null;
    let anchorVertical = popups[0]?.anchorVertical ?? vertical;
    const el = activeEl();
    if (rect && el) {
      el.focus();
      const frame = el.getBoundingClientRect();
      anchor = {
        x: frame.left + rect.x,
        y: frame.top + rect.y,
        width: rect.width,
        height: rect.height,
      };
      anchorVertical = vertical;
      placement = placePopup(anchor, anchorVertical);
    }
    const cue =
      sasayaki?.hasAudio && normalizedOffset !== null
        ? sasayaki.findCue(index, normalizedOffset)
        : null;
    popups = [
      {
        id: popups[0]?.id ?? crypto.randomUUID(),
        entries: response.entries,
        styles: response.styles,
        placement,
        anchor,
        anchorVertical,
        sasayakiCue: cue,
        sentence,
        clozeOffset: offset,
      },
    ];
    if (sasayaki?.isPlaying) {
      if (sasayakiConfig.sasayakiAutoPause) {
        sasayaki.togglePlayback();
        wasPaused = true;
      } else {
        wasPaused = false;
      }
    }
    const matched = response.entries[0].matched;
    postToFrame({ hoshi: "highlight", count: [...matched].length });
  }

  async function popupLookup(
    index: number,
    text: string,
    sentence: string | null,
    offset: number | null,
    rect: SelectionRect | null,
  ): Promise<number | null> {
    hider.cancel();
    const seq = ++lookupSeq;
    const response = await lookup(text);
    if (seq !== lookupSeq || !response.entries.length) return null;
    popupAnki = await invoke<PopupAnkiConfig>("anki_config");
    if (seq !== lookupSeq) return null;
    const base = popups[index].placement;
    const screenRect = rect
      ? { x: base.left + rect.x, y: base.top + rect.y, width: rect.width, height: rect.height }
      : { x: base.left, y: base.top, width: base.width, height: 0 };
    const placement = placePopup(screenRect, false);
    popups = [
      ...popups.slice(0, index + 1),
      {
        id: popups[index + 1]?.id ?? crypto.randomUUID(),
        entries: response.entries,
        styles: response.styles,
        placement,
        anchor: screenRect,
        anchorVertical: false,
        sasayakiCue: null,
        sentence: sentence ?? popups[index].sentence,
        clozeOffset: sentence !== null ? offset : popups[index].clozeOffset,
      },
    ];
    return [...response.entries[0].matched].length;
  }

  function lookup(text: string): Promise<LookupResponse> {
    return invoke<LookupResponse>("lookup", {
      text,
      maxResults: dictConfig.maxResults,
      scanLength: dictConfig.scanLength,
      frequencySortOrder: dictConfig.frequencySortOrder,
      frequencySortDictionary: dictConfig.frequencySortDictionary,
    });
  }

  function redirectKanji(character: string): Promise<KanjiResponse | null> {
    return invoke<KanjiResponse | null>("lookup_kanji", { character });
  }

  function mineEntry(content: MineContent, popup: PopupInstance): Promise<boolean> {
    return invoke<boolean>("anki_mine", {
      content,
      context: {
        sentence: popup.sentence,
        clozeOffset: popup.clozeOffset,
        documentTitle: title,
        bookId: id,
        sasayakiCue: popup.sasayakiCue?.id ?? null,
        sasayakiAudioFormat: sasayakiConfig.sasayakiAudioFormat,
      },
      slotIndex: Number(content.slotIndex) || 0,
    });
  }

  function checkDuplicates(fields: Record<string, string>): Promise<boolean[]> {
    return invoke<boolean[]>("anki_check_duplicates", { fields });
  }

  function showNotes(fields: Record<string, string>) {
    invoke("anki_show_notes", { fields, slotIndex: Number(fields.slotIndex) || 0 });
  }

  const chromeInset = 28;
  const chromeInsetX = 25;
  const speedPresets = [0.75, 1, 1.25, 1.5, 1.75, 2];

  const macChrome = $derived(isMac && !fullscreen);

  const iframeStyle = $derived(
    frozen
      ? `left: ${frameInset}px; width: ${frozen.w}px; height: ${frozen.h}px`
      : frameStyle(frameInset),
  );

  function calculateCharacterProgress(chapterIndex: number, p: number) {
    const ci = bookInfo.chapterInfo[spine[chapterIndex]];
    return ci ? Math.round(ci.currentTotal + ci.chapterCount * p) : 0;
  }
  const currentChar = $derived(calculateCharacterProgress(index, progress));

  const pageProgress = $derived.by(() => {
    if (!pageStarts.length) return null;
    let page =
      currentPage !== null
        ? spineFirstPages[index] + currentPage
        : progress === 0
          ? spineFirstPages[index]
          : pageAt(currentChar, index);
    if (spread) page -= (page - spineFirstPages[index]) % 2;
    const nextPage = spread && page + 1 < (spineFirstPages[index + 1] ?? pageStarts.length) ? page + 2 : null;
    const chapterEnd = chapterRange.start + chapterRange.total;
    let chapterFirstPage = pageStarts.indexOf(chapterRange.start);
    if (chapterFirstPage < 0) chapterFirstPage = Math.max(pageStarts.findLastIndex((start) => start < chapterRange.start), 0);
    let chapterLastPage = chapterEnd === bookInfo.characterCount ? pageStarts.length - 1 : pageStarts.findLastIndex((start) => start < chapterEnd);
    if (chapterLastPage < 0) chapterLastPage = chapterFirstPage;
    const chapterTotal = chapterLastPage - chapterFirstPage + 1;
    const chapterPage = Math.min(Math.max(page - chapterFirstPage + 1, 1), chapterTotal);
    return { page: page + 1, nextPage, total: pageStarts.length, chapterPage, chapterTotal };
  });

  function progressLine(current: number, total: number, pages: [number, number] | null): string {
    const parts: string[] = [];
    if (showPages) parts.push(pages ? `${pages[0]} of ${pages[1]}` : "…");
    else if (readerConfig.progressCount !== "Off") parts.push(`${current} / ${total}`);
    if (readerConfig.showPercentage) {
      const percent = total > 0 ? ((current / total) * 100).toFixed(2) : "0";
      parts.push(`${percent}%`);
    }
    return parts.join(" ");
  }
  const progressText = $derived.by(() => {
    if (topProgress) return "";
    const lines: string[] = [];
    if (readerConfig.showProgress) {
      const line = progressLine(currentChar, bookInfo.characterCount, pageProgress && [pageProgress.page, pageProgress.total]);
      if (line) lines.push(line);
    }
    if (readerConfig.showChapterProgress) {
      const line = progressLine(chapterRange.character, chapterRange.total, pageProgress && [pageProgress.chapterPage, pageProgress.chapterTotal]);
      if (line) lines.push(`(${line})`);
    }
    return lines.join(" ");
  });
  const spreadCorners = $derived.by(() => {
    if (!topProgress) return null;
    let corners = ["", ""];
    if (readerConfig.progressCount !== "Off") {
      const label = (page: number) => String(showPages ? page : pageStarts[page - 1]);
      corners = pageProgress
        ? [label(pageProgress.page), pageProgress.nextPage ? label(pageProgress.nextPage) : ""]
        : ["…", ""];
    }
    if (vertical) corners.reverse();
    return {
      left: corners[0],
      right: corners[1],
      title: readerConfig.spreadChapterTitle ? (toc[currentTocIndex]?.label ?? "") : "",
    };
  });

  const stats = (() =>
    new StatsTracker(
      folder,
      () => Math.round(statsConfig.statisticsResetTime * 60),
      () => calculateCharacterProgress(index, progress),
      () => applyingBookmark,
    ))();
  const transportVisible = $derived(
    sasayakiConfig.enableSasayaki &&
      sasayakiConfig.sasayakiShowControlBar &&
      (sasayaki?.hasAudio ?? false),
  );

  let statsDialog = $state<HTMLDialogElement>();
  let galleryDialog = $state<HTMLDialogElement>();
  let carouselEl = $state<HTMLDivElement>();
  let thumbStripEl = $state<HTMLDivElement>();
  const galleryImages = $derived(bookInfo.images ?? []);
  let directImage = $state<string | null>(null);
  const displayedImages = $derived(directImage ? [directImage] : galleryImages.map((image) => schemeUrl("book", `${id}/${encodeURI(image)}`)));
  let galleryIndex = $state(0);
  let galleryViews = $state<ReturnType<typeof ZoomableImage>[]>([]);
  const galleryZoom = $derived(galleryViews[galleryIndex]?.getZoom() ?? 1);

  async function downloadGalleryImage() {
    const index = Math.round(carouselEl!.scrollLeft / carouselEl!.clientWidth);
    const image = new URL(displayedImages[index]);
    const destination = await save({ defaultPath: decodeURIComponent(image.pathname.split("/").pop()!) });
    if (!destination) return;
    try {
      await invoke("save_book_image", { path: image.pathname, destination });
    } catch (error) {
      await message(String(error), { title: "Error", kind: "error" });
    }
  }

  function galleryScroll(dir: 1 | -1) {
    if (!carouselEl?.clientWidth) return;
    const current = Math.round(carouselEl.scrollLeft / carouselEl.clientWidth);
    galleryGoTo(current + dir);
  }

  function galleryGoTo(i: number) {
    if (!carouselEl?.clientWidth || !displayedImages.length) return;
    const next = Math.max(0, Math.min(i, displayedImages.length - 1));
    carouselEl.scrollTo({ left: next * carouselEl.clientWidth, behavior: "instant" });
    onCarouselScroll();
  }

  function onCarouselScroll() {
    if (!carouselEl) return;
    const i = Math.round(carouselEl.scrollLeft / carouselEl.clientWidth);
    if (i === galleryIndex) return;
    galleryIndex = i;
    thumbStripEl?.children[i]?.scrollIntoView({
      behavior: "smooth",
      block: "nearest",
    });
  }

  let lastGalleryWheel = 0;
  function onGalleryWheel(dx: number, dy: number) {
    const now = performance.now();
    if (now - lastGalleryWheel < 50) return;
    lastGalleryWheel = now;
    const delta = Math.abs(dx) > Math.abs(dy) ? dx : dy;
    if (!delta) return;
    galleryScroll(delta > 0 ? 1 : -1);
  }

  stats.loadSessions();
  if (statsConfig.statisticsAutostartMode === "On") stats.startTracking();

  (async () => {
    if (syncOnOpen) {
      const book = skipSyncOnOpen
        ? (await invoke<BookMetadata[]>("list_books")).find((book) => book.id === id)
        : await invoke<BookMetadata | null>("gdrive_sync_book", { id });
      if (bookDeleted || !book?.epub) {
        closeDeletedBook();
        return;
      }
    }
    if (sasayaki) await sasayaki.load();
    frames = [{ key: 0, src: frameSrc() }];
  })();

  function closeDeletedBook() {
    stats.stopTracking();
    sasayaki?.teardown();
    bookDeleted = true;
    onClose();
  }

  function resolveCharacterPosition(characterCount: number) {
    const clamped = Math.min(characterCount, Math.max(bookInfo.characterCount - 1, 0));
    for (const info of Object.values(bookInfo.chapterInfo)) {
      if (info.spineIndex === null || info.chapterCount === 0) continue;
      const start = info.currentTotal;
      if (clamped >= start && clamped < start + info.chapterCount) {
        return { spineIndex: info.spineIndex, progress: (clamped - start) / info.chapterCount };
      }
    }
    return null;
  }

  async function applySyncedState(book: SyncBook, bookmarkChanged: boolean) {
    if (bookmarkChanged) {
      const position = resolveCharacterPosition(book.bookmark!.value.characterCount);
      if (position) {
        applyingBookmark = true;
        index = position.spineIndex;
        progress = position.progress;
        stats.resetTrackingBaseline();
        if (frames[frames.length - 1].src !== blankSrc) {
          closePopups();
          pushFrame();
        }
      }
    }

    const saved = await invoke<BookHighlight[]>("load_highlights", { id });
    if (JSON.stringify(saved) !== JSON.stringify($state.snapshot(highlights))) highlights = saved;

    if (book.audiobook && sasayaki) {
      const value = book.audiobook.value;
      const playback = sasayaki.playback;
      if (
        playback.lastPosition !== value.lastPosition ||
        playback.delay !== value.delay ||
        Math.fround(playback.rate) !== value.rate
      ) {
        await sasayaki.reloadPlayback();
      }
    }

    stats.applySessions(book.sessions);
  }

  async function reloadSyncedMatch() {
    if (!sasayaki) return;
    await sasayaki.reloadMatch();
    postToFrame({ hoshi: "sasayaki-cues", cues: sasayaki.cues(index) });
  }

  $effect(() => {
    const unlisten = [
      listen<{ key: string; book: SyncBook; bookmarkChanged: boolean }>("sync://book-applied", ({ payload }) => {
        if (payload.key === syncKey && !bookDeleted) applySyncedState(payload.book, payload.bookmarkChanged);
      }),
      listen<{ key: string }>("sync://book-replaced", ({ payload }) => {
        if (payload.key === syncKey && !bookDeleted) closeDeletedBook();
      }),
      listen<{ key: string }>("sync://match-changed", ({ payload }) => {
        if (payload.key === syncKey) reloadSyncedMatch();
      }),
    ];
    return () => {
      for (const promise of unlisten) promise.then((fn) => fn());
    };
  });

  $effect(() => {
    const unlisten = getCurrentWindow().onFocusChanged(async ({ payload: focused }) => {
      if (focused) {
        stats.resume();
        invoke("gdrive_sync_book", { id });
      } else {
        await stats.pause();
        invoke("gdrive_sync_now");
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  let trackingStoppedManually = false;
  function toggleTracking() {
    if (stats.isTracking) stats.stopTracking();
    else stats.startTracking();
    trackingStoppedManually = !stats.isTracking;
  }

  function formatDuration(seconds: number): string {
    const total = Math.round(seconds);
    const h = Math.trunc(total / 3600);
    const m = Math.trunc((total % 3600) / 60);
    const s = total % 60;
    return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }

  function formatHourMinute(seconds: number): string {
    const total = Math.round(seconds / 60);
    const h = Math.trunc(total / 60);
    const m = total % 60;
    return `${h}:${String(m).padStart(2, "0")}`;
  }

  const currentChapterCount = $derived.by(() => {
    const ci = bookInfo.chapterInfo[spine[index]];
    return ci ? ci.currentTotal + ci.chapterCount : 0;
  });
  const chapterStarts = $derived.by(() => {
    const starts = new Set<number>([0]);
    for (const item of toc) {
      const chars = chapterChars(item);
      if (chars !== null) starts.add(chars);
    }
    return [...starts].sort((a, b) => a - b);
  });
  const chapterRange = $derived.by(() => {
    const position = currentChar;
    const xhtmlEnd = currentChapterCount;
    const at = xhtmlEnd > 0 ? Math.min(position, xhtmlEnd - 1) : position;
    let next = chapterStarts.findIndex((s) => s > at);
    if (next < 0) next = chapterStarts.length;
    const start = chapterStarts[next - 1];
    const end = next < chapterStarts.length ? chapterStarts[next] : bookInfo.characterCount;
    return { start, character: position - start, total: end - start };
  });
  const timeToFinishBook = $derived(
    readingSpeed(stats.allTimeTotal) > 0
      ? (bookInfo.characterCount - currentChar) / (readingSpeed(stats.allTimeTotal) / 3600)
      : 0,
  );
  const timeToFinishChapter = $derived(
    readingSpeed(stats.allTimeTotal) > 0
      ? (chapterRange.total - chapterRange.character) / (readingSpeed(stats.allTimeTotal) / 3600)
      : 0,
  );
  const statsString = $derived.by(() => {
    const parts: string[] = [];
    if (readerConfig.showReadingSpeed) parts.push(`${readingSpeed(stats.currentSession)} / h`);
    if (readerConfig.showReadingTime) parts.push(formatHourMinute(stats.currentSession.readingTime));
    return parts.join(" ");
  });

  $effect(() => {
    const timer = setInterval(() => {
      if (stats.isTracking && !stats.isPaused) stats.updateStats();
    }, 1000);
    return () => clearInterval(timer);
  });

  function saveBookmark() {
    return invoke("save_bookmark", {
      id,
      chapterIndex: index,
      progress,
      characterCount: calculateCharacterProgress(index, progress),
    });
  }

  $effect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      if (isMac) event.preventDefault();
      await stats.stopTracking();
      if (isMac) onClose();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  function navigateTo(spineIndex: number, p: number, fragment: string | null = null) {
    stats.flushStats();
    if (spineIndex === index && !fragment) {
      progress = p;
      saveBookmark();
      postToFrame({ hoshi: "restore", progress: p });
      stats.resetTrackingBaseline();
      return;
    }
    index = spineIndex;
    progress = p;
    pendingFragment = fragment;
    saveBookmark();
    pushFrame();
    stats.resetTrackingBaseline();
  }

  let highlights = $state<BookHighlight[]>((() => savedHighlights)());

  $effect(() => {
    if (readerPanel === "Highlights" && !highlights.length) readerPanel = null;
  });

  function chapterHighlights() {
    const chapter = bookInfo.chapterInfo[spine[index]];
    return chapter ? highlights.filter((highlight) => highlight.character >= chapter.currentTotal && highlight.character < chapter.currentTotal + chapter.chapterCount) : [];
  }

  function saveHighlights() {
    return invoke("save_highlights", { id, highlights: $state.snapshot(highlights) });
  }

  async function showSelectionMenu(source: Window) {
    const menu = await Menu.new({ items: [
      { item: "Copy" },
      { text: "Highlight", items: Object.keys(highlightColors).map((color) => ({
        text: color.charAt(0).toUpperCase() + color.slice(1),
        action: () => source.postMessage({ hoshi: "create-highlight", id: crypto.randomUUID().toUpperCase(), color }, "*"),
      })) },
    ] });
    await menu.popup();
  }

  function removeHighlight(highlight: BookHighlight) {
    highlights = highlights.filter((entry) => entry.id !== highlight.id);
    saveHighlights();
    postToFrame({ hoshi: "remove-highlight", id: highlight.id });
  }

  let pendingSearchHighlight: { character: number; length: number } | null = null;

  function jumpToSearchResult(result: BookSearchResult) {
    pendingSearchHighlight = { character: result.character, length: result.length };
    closePopups();
    jumpToCharacter(result.character);
  }

  function jumpToCharacter(characterCount: number) {
    const position = resolveCharacterPosition(characterCount);
    if (position) navigateTo(position.spineIndex, position.progress);
  }

  function jumpToLink(spineIndex: number, fragment: string | null) {
    if (spineIndex === index && fragment) {
      stats.flushStats();
      postToFrame({ hoshi: "fragment", fragment });
      return;
    }
    navigateTo(spineIndex, 0, fragment);
  }

  function handleLink(href: string) {
    let url: URL;
    try {
      url = new URL(href);
    } catch {
      return;
    }
    const base = new URL(schemeUrl("book"));
    if (url.protocol !== base.protocol || url.host !== base.host) return;
    const path = decodeURIComponent(url.pathname).replace(/^\/+/, "");
    if (!path.startsWith(`${id}/`)) return;
    const target = path.slice(id.length + 1);
    const spineIndex = spine.indexOf(target);
    if (spineIndex < 0) return;
    const fragment = url.hash ? decodeURIComponent(url.hash.slice(1)) : null;
    closePopups();
    jumpToLink(spineIndex, fragment);
  }

  let jumpToOpen = $state(false);
  let jumpToInput = $state("");
  let jumpToInvalid = $state(false);

  let chapterList = $state<HTMLUListElement>();
  let bookSearch = $state<ReturnType<typeof BookSearch>>()!;

  async function openReaderPanel(panel: typeof readerPanel) {
    readerPanel = panel;
    closePopups();
    if (panel) {
      if (panel === "Appearance" || panel === "Sasayaki") rightPanel = panel;
      else leftPanel = panel;
      jumpToOpen = false;
      jumpToInput = "";
      jumpToInvalid = false;
      showBar = true;
      if (panel === "Chapters") scrollToCurrentChapter();
      if (panel === "Search") {
        await tick();
        bookSearch.focus();
      }
    }
  }

  async function scrollToCurrentChapter() {
    await tick();
    chapterList?.querySelector(`[data-chapter="${currentTocIndex}"]`)?.scrollIntoView({ block: "center" });
  }

  async function openGalleryImage(url?: string) {
    const i = url ? galleryImages.findIndex((image) => new URL(schemeUrl("book", `${id}/${encodeURI(image)}`)).href === url) : galleryIndex;
    directImage = i < 0 ? url! : null;
    closePopups();
    galleryDialog?.showModal();
    await tick();
    galleryGoTo(Math.max(i, 0));
  }

  function confirmJumpTo() {
    if (/^\d+$/.test(jumpToInput.trim())) {
      jumpToCharacter(Number(jumpToInput.trim()));
      jumpToOpen = false;
      closePopups();
    } else {
      jumpToInvalid = true;
    }
  }

  const bookPercent = $derived(
    bookInfo.characterCount > 0
      ? ((currentChar / bookInfo.characterCount) * 100).toFixed(1)
      : "0.0",
  );

  function chapterChars(item: TocItem): number | null {
    const info = bookInfo.chapterInfo[spine[item.spineIndex]];
    if (!info) return null;
    const offset = item.fragment ? (info.fragmentOffsets?.[item.fragment] ?? 0) : 0;
    return info.currentTotal + offset;
  }

  const currentTocIndex = $derived(
    toc.findLastIndex((item) => (chapterChars(item) ?? Infinity) <= currentChar),
  );

  const pageAdvance = $derived(readerConfig.paragraphMode && sasayakiConfig.sasayakiPageAdvance && !!sasayaki);
  let pendingPagePlayback: boolean | null = null;
  let pressDismissed = false;

  function postTurn(dir: "forward" | "backward") {
    if (loading) return;
    if (pageAdvance) sasayaki?.handlePageChanged([], false);
    postToFrame({ hoshi: "turn", dir });
  }

  let lastWheel = 0;

  function onWheelInput(dx: number, dy: number) {
    if (hotkeyConfig.disableReaderWheel) return;
    const now = performance.now();
    if (now - lastWheel < 50) return;
    lastWheel = now;
    const forward = dx ? dx > 0 !== vertical : dy > 0;
    postTurn(forward ? "forward" : "backward");
  }

  function pageDirection(key: string): "forward" | "backward" | null {
    if (!key) return null;
    const reverse = vertical && hotkeyConfig.reversePageVertical;
    if (key === hotkeyConfig.nextPage) return reverse ? "backward" : "forward";
    if (key === hotkeyConfig.previousPage) return reverse ? "forward" : "backward";
    return null;
  }

  const readerShortcutKeys = $derived(
    readerHotkeys
      .filter((binding) => binding.key === "toggleTracking" || (binding.section === "Sasayaki" && sasayaki?.hasAudio))
      .map((binding) => hotkeyConfig[binding.key])
      .filter(Boolean),
  );

  const frameHotkeys = $derived(
    [...readerShortcutKeys, hotkeyConfig.previousPage, hotkeyConfig.nextPage].filter(Boolean),
  );

  function onReaderHotkey(key: string, repeat = false) {
    if (document.querySelector("dialog[open]")) return;
    const dir = pageDirection(key);
    if (dir) {
      postTurn(dir);
      return;
    }
    if (repeat || !readerShortcutKeys.includes(key)) return;
    if (key === hotkeyConfig.toggleTracking) toggleTracking();
    else if (key === hotkeyConfig.sasayakiPlayback) toggleSasayakiPlayback();
    else if (key === hotkeyConfig.sasayakiPreviousCue) sasayaki?.prevCue();
    else if (key === hotkeyConfig.sasayakiNextCue) sasayaki?.nextCue();
  }

  function onMessage(e: MessageEvent) {
    if (pagesFrame && e.source === pagesFrame.contentWindow) {
      if (e.data?.hoshi === "pages" && e.data.run === String(pagesRun)) onSpineMeasured(e.data.starts);
      return;
    }
    let sourceKey: number | null = null;
    for (const [key, el] of frameEls) {
      if (el.contentWindow === e.source) {
        sourceKey = key;
        break;
      }
    }
    if (sourceKey === null) return;
    const m = e.data;
    if (m?.hoshi === "wheel") {
      frameWheelAt.set(sourceKey, performance.now());
      onWheelInput(m.dx, m.dy);
      return;
    }
    if (sourceKey !== frames[frames.length - 1]?.key) return;
    switch (m?.hoshi) {
      case "selected":
        pressLookup = openLookup(m.text, m.rect, m.normalizedOffset, m.sentence, m.clozeOffset);
        break;
      case "lookup-miss":
        closePopups();
        if (pressDismissed || m.dismissed) break;
        if (m.edge) postTurn((m.edge === "right") !== (vertical && hotkeyConfig.reversePageVertical) ? "forward" : "backward");
        else if (readerConfig.paragraphMode && readerConfig.clickToAdvance) postTurn("forward");
        break;
      case "press":
        pressDismissed = popups.length > 0 || readerPanel !== null || showBar;
        readerPanel = null;
        showBar = false;
        pressHeld = true;
        pressLookup = null;
        closePopups(0, false);
        break;
      case "release":
        releasePress();
        break;
      case "reader-hotkey":
        onReaderHotkey(m.key, m.repeat);
        break;
      case "selection-menu":
        showSelectionMenu(e.source as Window);
        break;
      case "highlight-created": {
        const result = m.result;
        if (result.id) {
          const existing = highlights.find((highlight) => highlight.id === result.id)!;
          if (existing.color === m.color) {
            highlights = highlights.filter((highlight) => highlight.id !== result.id);
          } else {
            highlights = highlights.map((highlight) => highlight.id === result.id ? { ...highlight, color: m.color } : highlight);
          }
        } else {
          highlights = [...highlights, {
            id: m.id, character: bookInfo.chapterInfo[spine[index]].currentTotal + result.start,
            offset: result.offset, text: result.text, textFurigana: result.textFurigana,
            color: m.color, createdAt: Date.now() / 1000 - 978307200,
          }];
        }
        saveHighlights();
        break;
      }
      case "escape":
        handleEscape();
        break;
      case "search":
        openReaderPanel("Search");
        break;
      case "open-image":
        openGalleryImage(m.url);
        break;
      case "link":
        if (typeof m.href === "string") handleLink(m.href);
        break;
      case "ready": {
        if (spread) postRestyle();
        postToFrame({ hoshi: "reader-hotkeys", keys: [...frameHotkeys] });
        const cues = sasayaki?.hasMatch ? sasayaki.cues(index) : null;
        const saved = $state.snapshot(chapterHighlights());
        if (pendingFragment) {
          postToFrame({ hoshi: "fragment", fragment: pendingFragment, cues, highlights: saved });
          pendingFragment = null;
        } else {
          postToFrame({ hoshi: "restore", progress, cues, highlights: saved });
        }
        break;
      }
      case "page-cues":
        sasayaki?.handlePageChanged(m.ids, m.play);
        break;
      case "sasayaki-image-result":
        sasayaki?.handleImageResult(m.paused);
        break;
      case "positioned": {
        loading = false;
        applyingBookmark = false;
        if (pendingSearchHighlight) {
          const chapter = bookInfo.chapterInfo[spine[index]];
          postToFrame({ hoshi: "search-highlight", offset: pendingSearchHighlight.character - chapter.currentTotal, length: pendingSearchHighlight.length });
          pendingSearchHighlight = null;
        }
        sasayaki?.handleRestoreCompleted(index);
        if (pageAdvance) postToFrame({ hoshi: "page-cues", play: pendingPagePlayback });
        pendingPagePlayback = null;
        stats.resetTrackingBaseline();
        const focused = document.activeElement;
        if (
          focused instanceof HTMLIFrameElement &&
          focused !== activeEl() &&
          [...frameEls.values()].includes(focused)
        ) {
          activeEl()?.focus();
        }
        break;
      }
      case "marker":
        marker = { x: m.x, y: m.y, inset: m.inset };
        break;
      case "page":
        currentPage = m.page;
        break;
      case "progress":
        if (applyingBookmark || bookDeleted) break;
        progress = m.frac;
        saveBookmark();
        if (pageAdvance) postToFrame({ hoshi: "page-cues", play: m.jump ? null : m.dir === "forward" });
        if (m.jump) {
          stats.resetTrackingBaseline();
        } else {
          stats.flushStats();
          if (statsConfig.statisticsAutostartMode !== "Off" && !stats.isTracking && !trackingStoppedManually) {
            stats.startTracking();
          }
        }
        break;
      case "boundary":
        if (m.dir === "forward" && index < spine.length - 1) {
          pendingPagePlayback = true;
          progress = 0;
          index += 1;
          saveBookmark();
          pushFrame();
          stats.flushStats();
        } else if (m.dir === "backward" && index > 0) {
          pendingPagePlayback = false;
          progress = 1;
          index -= 1;
          saveBookmark();
          pushFrame();
          stats.flushStats();
        }
        break;
    }
  }

  function onWheel(e: WheelEvent) {
    if (galleryDialog?.open) {
      if (thumbStripEl?.contains(e.target as Node)) return;
      e.preventDefault();
      onGalleryWheel(e.deltaX, e.deltaY);
      return;
    }
    const t = e.target as Node | null;
    if (!(t && contentEl?.contains(t))) return;
    e.preventDefault();
    onWheelInput(e.deltaX, e.deltaY);
  }

  function handleEscape(): boolean {
    if (galleryDialog?.open) {
      galleryDialog.close();
      return true;
    }
    if (statsDialog?.open) {
      statsDialog.close();
      return true;
    }
    if (popups.length) {
      closePopups(popups.length - 1);
      return true;
    }
    if (readerPanel) {
      openReaderPanel(null);
      return true;
    }
    if (showBar) {
      showBar = false;
      return true;
    }
    if (fullscreen) {
      toggleFullscreen();
      return true;
    }
    return false;
  }

  function onKey(e: KeyboardEvent) {
    if ((isMac ? e.metaKey : e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "f" && !document.querySelector("dialog[open]")) {
      e.preventDefault();
      openReaderPanel("Search");
      return;
    }
    if (e.key === "Escape") {
      if (handleEscape()) e.preventDefault();
      return;
    }
    if ((e.target as HTMLElement).closest("input, textarea, select, [contenteditable]")) return;
    if (galleryDialog?.open) {
      if (e.key === "ArrowRight") {
        e.preventDefault();
        galleryScroll(1);
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        galleryScroll(-1);
      } else if (e.key === "+" || e.key === "=") {
        e.preventDefault();
        galleryViews[galleryIndex]?.setZoom(galleryZoom * 1.25);
      } else if (e.key === "-") {
        e.preventDefault();
        galleryViews[galleryIndex]?.setZoom(galleryZoom / 1.25);
      } else if (e.key === "0") {
        e.preventDefault();
        galleryViews[galleryIndex]?.reset();
      }
      return;
    }
    if (!e.defaultPrevented && !e.isComposing && !e.ctrlKey && !e.altKey && !e.metaKey && !document.querySelector("dialog[open]")) {
      const key = normalizeHotkey(e.key);
      const control = (e.target as HTMLElement).closest("button, a, [role='button'], summary");
      const page = pageDirection(key) && !(e.target as HTMLElement).closest("aside");
      if ((page || readerShortcutKeys.includes(key)) && !(control && [" ", "Enter", "Tab"].includes(e.key))) {
        e.preventDefault();
        onReaderHotkey(key, e.repeat);
      }
    }
  }

  function settle() {
    frozen = null;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (!frozen) resizing = false;
      }),
    );
  }

  function onResize() {
    const el = activeEl();
    if (!frozen && el) {
      const rect = el.getBoundingClientRect();
      frozen = { w: rect.width, h: rect.height };
    }
    resizing = true;
    clearTimeout(settleTimer);
    settleTimer = window.setTimeout(settle, 150);
  }

  $effect(() => {
    window.addEventListener("message", onMessage);
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", onResize);
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      window.removeEventListener("message", onMessage);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("wheel", onWheel);
      stats.flushStats();
      clearTimeout(settleTimer);
      clearTimeout(frameCleanupTimer);
      sasayaki?.dispose();
      if (fullscreen) getCurrentWindow().setFullscreen(false);
    };
  });

  $effect(() => {
    postToFrame({ hoshi: "reader-hotkeys", keys: [...frameHotkeys] });
  });

  $effect(() => {
    postToFrame({
      hoshi: "sasayaki-colors",
      text: sasayakiTextColor,
      background: sasayakiBackgroundColor,
    });
  });

  $effect(() => {
    postToFrame({ hoshi: "textcolor", tc: readerText });
  });

  $effect(() => {
    postToFrame({ hoshi: "text-animation", speed: readerConfig.textAnimation ? readerConfig.textSpeed : 0 });
  });

  $effect(() => {
    postToFrame({ hoshi: "click-advance", enabled: readerConfig.clickToAdvance });
  });

  function postRestyle() {
    postToFrame({
      hoshi: "restyle",
      fs: readerConfig.fontSize,
      hp: readerConfig.horizontalPadding,
      vp: readerConfig.verticalPadding,
      mw: readerConfig.maxWidth,
      mh: readerConfig.maxHeight,
      j: readerConfig.justifyText,
      apb: readerConfig.avoidPageBreak,
      adv: readerConfig.layoutAdvanced,
      lh: readerConfig.lineHeight,
      cs: readerConfig.characterSpacing,
      ps: readerConfig.paragraphSpacing,
      sp: spread ? spreadGap : 0,
    });
  }

  $effect(() => {
    if (!topProgress || !readerConfig.selectedFontFile) return;
    const face = new FontFace(
      readerConfig.selectedFont,
      `url("${schemeUrl("book", `__hoshi/Fonts/${encodeURIComponent(readerConfig.selectedFontFile)}`)}")`,
    );
    document.fonts.add(face);
    return () => {
      document.fonts.delete(face);
    };
  });

  let lastSpread: boolean | null = null;
  $effect(() => {
    const current = spreadMode;
    if (lastSpread !== null && current !== lastSpread) postRestyle();
    lastSpread = current;
  });

  let importedFonts = $state<FontInfo[]>([]);
  invoke<FontInfo[]>("list_fonts").then((fonts) => (importedFonts = fonts));
</script>

<div class="relative flex h-full select-none flex-col">
  {#if macChrome}
    <button
      class="absolute inset-x-0 top-0 z-[2210] h-[2.0625rem] cursor-default"
      onpointerdown={onTitleBarPointerDown}
      onpointermove={onTitleBarPointerMove}
      onpointerup={onTitleBarPointerUp}
    ></button>
  {/if}
  <div class="pointer-events-none absolute inset-x-0 top-0 z-[2203]">
    {#if !macChrome}
      <button
        class="pointer-events-auto absolute inset-x-0 top-0 h-6 cursor-default"
        onclick={() => (showBar = !showBar)}
      ></button>
    {/if}
    <header
      style={macChrome ? "padding-top: 2.0625rem" : ""}
      inert={!showBar && !readerPanel}
      class="relative flex items-center gap-3 border-b border-base-300 bg-base-100/80 px-4 py-2.5 backdrop-blur transition-all duration-300 {showBar || readerPanel
        ? 'pointer-events-auto translate-y-0 opacity-100'
        : 'pointer-events-none -translate-y-full opacity-0'}"
    >
      <div class="flex shrink-0 items-center gap-1">
        <button
          class="btn btn-ghost btn-sm btn-square text-base-content/60"
          title="Bookshelf"
          onclick={() => {
            stats.stopTracking();
            onClose();
          }}
        >
          <ArrowLeft class="size-4" />
        </button>

        {#each [["Chapters", List], ["Search", Search], ["Highlights", Highlighter]] as const as [panel, Icon] (panel)}
          {#if panel !== "Highlights" || highlights.length}
            <button
              class="btn btn-ghost btn-sm btn-square {readerPanel === panel ? 'btn-active text-primary' : 'text-base-content/60'}"
              onclick={() => openReaderPanel(readerPanel === panel ? null : panel)}
              title={panel === "Search" ? `Search (${isMac ? '⌘F' : 'Ctrl+F'})` : panel}
            >
              <Icon class="size-4" />
            </button>
          {/if}
        {/each}

        <button class="btn btn-ghost btn-sm btn-square text-base-content/60" title="Gallery" onclick={() => openGalleryImage()}>
          <Images class="size-4" />
        </button>
      </div>

      <div class="pointer-events-none absolute left-1/2 max-w-[calc(100%-26rem)] -translate-x-1/2 text-center">
        <span class="block truncate text-sm font-semibold">{title}</span>
      </div>

      <div class="ml-auto flex shrink-0 items-center gap-1">
        <button
          class="btn btn-ghost btn-sm btn-square text-base-content/60"
          title="Statistics"
          onclick={() => statsDialog?.showModal()}
        >
          <ChartLine class="size-4" />
        </button>

        {#if sasayaki}
          <button
            class="btn btn-ghost btn-sm btn-square {readerPanel === 'Sasayaki'
              ? 'btn-active text-primary'
              : 'text-base-content/60'}"
            title="Sasayaki"
            onclick={() => openReaderPanel(readerPanel === "Sasayaki" ? null : "Sasayaki")}
          >
            <AudioLines class="size-4" />
          </button>
        {/if}
        <button
          class="btn btn-ghost btn-sm btn-square {readerPanel === 'Appearance'
            ? 'btn-active text-primary'
            : 'text-base-content/60'}"
          title="Appearance"
          onclick={() => openReaderPanel(readerPanel === "Appearance" ? null : "Appearance")}
        >
          Aa
        </button>

        <button
          class="btn btn-ghost btn-sm btn-square text-base-content/60"
          title={fullscreen ? "Exit Fullscreen" : "Fullscreen"}
          onclick={toggleFullscreen}
        >
          {#if fullscreen}
            <Minimize class="size-4" />
          {:else}
            <Maximize class="size-4" />
          {/if}
        </button>
      </div>
    </header>
  </div>

  <div
    bind:this={contentEl}
    class="relative min-h-0 flex-1 overflow-hidden"
    style="background-color: {readerBg}"
  >
    <div class="absolute" style="inset: {chromeInset}px {chromeInsetX}px" bind:clientWidth={frameWidth} bind:clientHeight={frameHeight}>
      {#key pagesSrc}
        {#if pagesSrc}
          <iframe
            bind:this={pagesFrame}
            src={pagesSrc}
            title=""
            style={frameStyle(spreadInset(pagesSpread))}
            class="pointer-events-none invisible absolute inset-x-0 top-0 border-0"
          ></iframe>
        {/if}
      {/key}
      {#each frames as frame, i (frame.key)}
        <iframe
          use:frameRef={frame.key}
          src={frame.src}
          title=""
          style="{iframeStyle}; background-color: {readerBg}"
          class:opacity-0={i < frames.length - 1 || resizing}
          class:pointer-events-none={i < frames.length - 1}
          class="absolute inset-x-0 top-0 border-0 transition-opacity duration-200"
        ></iframe>
      {/each}
    </div>
    {#if loading}
      <div
        class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center"
        style="background-color: {readerBg}"
      >
        <span class="loading loading-dots loading-lg text-base-content/40"></span>
      </div>
    {/if}
    {#if marker && !loading && !(readerConfig.paragraphMode && readerConfig.paragraphHideBookmark)}
      <div
        class="pointer-events-none absolute text-base-content/50 transition-opacity duration-200"
        class:opacity-0={resizing}
        style="{vertical
          ? `top: ${topProgress ? Math.min(chromeInset + frameHeight - marker.inset + 2, 2 * chromeInset + frameHeight - 18) : Math.max(2, chromeInset + marker.inset - 18)}px; left: ${chromeInsetX + frameInset + marker.x}px`
          : `top: ${chromeInset + marker.y - 8}px; left: ${Math.max(
              0,
              chromeInsetX + frameInset + marker.x - 20,
            )}px`}{readerInfo
          ? `; color: ${readerInfo}`
          : ''}"
      >
        <BookmarkIcon size={16} fill="currentColor" />
      </div>
    {/if}

    {#if spreadCorners}
      {@const edge = chromeInsetX + (spread ? frameInset + spreadGap / 2 : (frameWidth * effectivePadding(readerConfig.horizontalPadding, readerConfig.maxWidth, frameWidth)) / 200)}
      {@const top = Math.round((frameHeight * effectivePadding(readerConfig.verticalPadding, readerConfig.maxHeight, frameHeight)) / 200)}
      {@const size = Math.max(11, Math.round(readerConfig.fontSize * 0.6))}
      {@const color = `; font: ${size}px "${readerConfig.selectedFont}", "Hiragino Mincho ProN", "Yu Mincho", serif${readerInfo ? `; color: ${readerInfo}` : ""}`}
      <span
        class="pointer-events-none absolute z-20 flex h-7 max-w-[40%] items-center gap-[1em] text-base-content/60 transition-opacity duration-200"
        class:opacity-0={resizing}
        style="top: {top}px; left: {Math.max(edge, macChrome && top + 14 - size / 2 < 28 ? 78 : 0)}px{color}"
      >
        {#if spreadCorners.left}
          <span class="tabular-nums">{spreadCorners.left}</span>
        {/if}
        <span class="truncate">{spreadCorners.title}</span>
      </span>
      <span
        class="pointer-events-none absolute z-20 flex h-7 items-center tabular-nums text-base-content/60 transition-opacity duration-200"
        class:opacity-0={resizing}
        style="top: {top}px; right: {edge}px{color}"
      >
        {spreadCorners.right}
      </span>
    {/if}

    {#snippet transportButton(Icon: typeof Play, action: () => void)}
      <button
        class="flex size-5 items-center justify-center rounded hover:bg-current/10"
        onclick={action}
      >
        <Icon class="size-3.5" />
      </button>
    {/snippet}

    {#snippet transport(player: SasayakiPlayer)}
      {@const percent = player.duration ? (player.currentTime / player.duration) * 100 : 0}
      <div
        class="absolute bottom-0 left-1/2 z-20 flex w-max max-w-full -translate-x-1/2 flex-wrap items-center justify-center gap-1 py-1.5 text-xs opacity-55 transition-opacity duration-200 hover:opacity-100"
        style={readerInfo ? `color: ${readerInfo}` : ""}
      >
        <div class="flex items-center gap-1 {vertical ? '-scale-x-100' : ''}">
          {@render transportButton(RotateCcw, () => player.skip(false))}
          {@render transportButton(Rewind, () => player.prevCue())}
          <button
            class="flex size-5 items-center justify-center rounded hover:bg-current/10 {vertical
              ? '-scale-x-100'
              : ''}"
            onclick={() => player.togglePlayback()}
          >
            {#if player.isPlaying}
              <Pause class="size-3.5" fill="currentColor" />
            {:else}
              <Play class="size-3.5" fill="currentColor" />
            {/if}
          </button>
          {@render transportButton(FastForward, () => player.nextCue())}
          {@render transportButton(RotateCw, () => player.skip(true))}
        </div>
        <input
          type="range"
          class="mx-1 h-3 w-32 cursor-pointer appearance-none rounded-full bg-current/25 bg-clip-content py-[5px] [&::-webkit-slider-thumb]:size-2 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-current"
          style="background-image: linear-gradient(to right, currentColor {percent}%, transparent {percent}%)"
          min="0"
          max={player.duration}
          step="0.1"
          value={player.currentTime}
          oninput={(e) => player.scrub(Number(e.currentTarget.value))}
        />
        <span class="whitespace-nowrap tabular-nums">
          {formatTime(player.currentTime)} / {formatTime(player.duration)}
        </span>
        <div class="group relative flex items-center">
          <button
            class="flex size-5 items-center justify-center rounded hover:bg-current/10"
            onclick={() => player.toggleMute()}
          >
            {#if player.volume === 0}
              <VolumeX class="size-3.5" />
            {:else if player.volume < 0.5}
              <Volume1 class="size-3.5 translate-x-[1.5px]" />
            {:else}
              <Volume2 class="size-3.5" />
            {/if}
          </button>
          <div
            class="invisible absolute top-1/2 left-full z-30 flex -translate-y-1/2 items-center rounded opacity-0 transition-opacity group-hover:visible group-hover:opacity-100 group-has-[:active]:visible group-has-[:active]:opacity-100"
            style="background-color: {readerBg}"
          >
            <input
              type="range"
              class="mx-2 h-6 w-16 cursor-pointer appearance-none rounded-full bg-current/25 bg-clip-content py-[11px] [&::-webkit-slider-thumb]:size-2 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-current"
              style="background-image: linear-gradient(to right, currentColor {player.volume *
                100}%, transparent {player.volume * 100}%)"
              min="0"
              max="1"
              step="0.05"
              value={player.volume}
              oninput={(e) => player.setVolume(Number(e.currentTarget.value))}
            />
          </div>
        </div>
        <div class="dropdown dropdown-end dropdown-top -ml-1">
          <div
            tabindex="0"
            role="button"
            class="cursor-pointer whitespace-nowrap rounded px-1 tabular-nums hover:bg-current/10"
          >
            {player.rate.toFixed(2)}x
          </div>
          <ul
            class="dropdown-content menu z-30 mb-1 w-24 rounded-box border border-base-300 bg-base-100 text-base-content shadow-md"
          >
            {#each speedPresets as preset (preset)}
              <li>
                <button
                  class="tabular-nums {player.rate === preset ? 'menu-active' : ''}"
                  onclick={() => {
                    player.setRate(preset);
                    (document.activeElement as HTMLElement | null)?.blur();
                  }}
                >
                  {preset.toFixed(2)}x
                </button>
              </li>
            {/each}
          </ul>
        </div>
      </div>
    {/snippet}

    {#if readerConfig.showStatisticsToggle}
      <div class="absolute bottom-0.5 left-1.5 z-20 flex items-center">
        <button
          class="btn btn-ghost btn-xs btn-square text-base-content/60"
          style={readerInfo ? `color: ${readerInfo}` : ""}
          onclick={toggleTracking}
        >
          {#if stats.isTracking}
            <Timer class="size-4" />
          {:else}
            <TimerOff class="size-4" />
          {/if}
        </button>
      </div>
    {/if}

    {#if transportVisible && sasayaki}
      {@render transport(sasayaki)}
    {/if}

    {#if statsString}
      <footer
        class="pointer-events-none absolute bottom-0 left-0 z-20 flex flex-col items-start py-1.5 pr-3 text-xs text-base-content/60"
        style="padding-left: {0.75 + (readerConfig.showStatisticsToggle ? 1.5 : 0)}rem{readerInfo
          ? `; color: ${readerInfo}`
          : ''}"
      >
        <span class="tabular-nums">{statsString}</span>
      </footer>
    {/if}
    {#if progressText}
      <footer
        class="pointer-events-none absolute bottom-0 right-0 z-20 flex flex-col items-end px-3 py-1.5 text-xs text-base-content/60"
        style={readerInfo ? `color: ${readerInfo}` : ""}
      >
        <span class="tabular-nums">{progressText}</span>
      </footer>
    {/if}
  </div>

  {#snippet statTile(title: string, value: string, unit = "")}
    <div class="stat content-start">
      <div class="stat-title">{title}</div>
      <div class="stat-value font-mono text-2xl font-semibold tabular-nums">
        {value}{#if unit}<span class="ml-1 font-sans text-base text-base-content/60">{unit}</span>{/if}
      </div>
    </div>
  {/snippet}
  <dialog class="modal" bind:this={statsDialog}>
    <div class="modal-box max-w-xl border border-base-300 bg-base-100/80 backdrop-blur-lg">
      <div class="mb-4 flex items-center justify-between">
        <h3 class="text-base font-semibold">Statistics</h3>
        <form method="dialog">
          <button class="btn btn-ghost btn-sm btn-square">
            <X class="size-4" />
          </button>
        </form>
      </div>
      <div class="flex flex-col gap-6">
        <section class="flex flex-col gap-2">
          <div
            class="flex items-center gap-1 text-xs font-medium tracking-wide text-base-content/50 uppercase"
          >
            <span>Session</span>
            <button
              class="btn btn-ghost btn-xs btn-square"
              onclick={toggleTracking}
            >
              {#if stats.isTracking}
                <Pause class="size-3.5" fill="currentColor" />
              {:else}
                <Play class="size-3.5" fill="currentColor" />
              {/if}
            </button>
          </div>
          <div class="stats w-full border border-base-300">
            {@render statTile("Characters Read", String(stats.currentSession.charactersRead))}
            {@render statTile("Reading Speed", String(readingSpeed(stats.currentSession)), "/ h")}
            {@render statTile("Reading Time", formatDuration(stats.currentSession.readingTime))}
          </div>
        </section>
        <section class="flex flex-col gap-2">
          <span class="text-xs font-medium tracking-wide text-base-content/50 uppercase">
            Today
          </span>
          <div class="stats w-full border border-base-300">
            {@render statTile("Characters Read", String(stats.todaysTotal.charactersRead))}
            {@render statTile("Reading Speed", String(readingSpeed(stats.todaysTotal)), "/ h")}
            {@render statTile("Reading Time", formatDuration(stats.todaysTotal.readingTime))}
          </div>
        </section>
        <section class="flex flex-col gap-2">
          <span class="text-xs font-medium tracking-wide text-base-content/50 uppercase">
            All Time
          </span>
          <div class="stats w-full border border-base-300">
            {@render statTile("Characters Read", String(stats.allTimeTotal.charactersRead))}
            {@render statTile("Reading Speed", String(readingSpeed(stats.allTimeTotal)), "/ h")}
            {@render statTile("Reading Time", formatDuration(stats.allTimeTotal.readingTime))}
          </div>
        </section>
        <section class="flex flex-col gap-2">
          <span class="text-xs font-medium tracking-wide text-base-content/50 uppercase">
            Time to Finish
          </span>
          <div class="stats w-full border border-base-300">
            {@render statTile("Chapter", formatDuration(timeToFinishChapter))}
            {@render statTile("Book", formatDuration(timeToFinishBook))}
          </div>
        </section>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>

  <dialog class="modal" bind:this={galleryDialog} onclose={() => galleryViews[galleryIndex]?.reset(false)}>
    <div
      class="modal-box relative flex h-screen max-h-none w-screen max-w-none flex-col overflow-hidden rounded-none bg-base-100/95 p-0 backdrop-blur-lg"
    >
      <div
        style={macChrome ? "padding-top: 2.0625rem" : ""}
        class="flex items-center justify-between border-b border-base-300 px-4 py-2.5"
      >
        <h3 class="text-base font-semibold">Gallery</h3>
        <div class="flex items-center gap-1">
          {#if displayedImages.length}
            <button class="btn btn-ghost btn-sm btn-square" disabled={galleryZoom <= 1} onclick={() => galleryViews[galleryIndex]?.setZoom(galleryZoom / 1.25)}>
              <ZoomOut class="size-4" />
            </button>
            <button class="btn btn-ghost btn-sm min-w-14 tabular-nums" onclick={() => galleryViews[galleryIndex]?.reset()}>
              {galleryZoom.toFixed(1)}×
            </button>
            <button class="btn btn-ghost btn-sm btn-square" disabled={galleryZoom >= 5} onclick={() => galleryViews[galleryIndex]?.setZoom(galleryZoom * 1.25)}>
              <ZoomIn class="size-4" />
            </button>
            <button class="btn btn-ghost btn-sm btn-square" onclick={downloadGalleryImage}>
              <Download class="size-4" />
            </button>
          {/if}
          <form method="dialog">
            <button class="btn btn-ghost btn-sm btn-square">
              <X class="size-4" />
            </button>
          </form>
        </div>
      </div>
      {#if displayedImages.length}
        <div class="flex min-h-0 flex-1">
          {#if displayedImages.length > 1}
            <div class="flex w-28 shrink-0 flex-col items-center gap-0.5 overflow-y-auto border-r border-base-300 bg-base-200/50 py-2 [scrollbar-width:thin]" bind:this={thumbStripEl}>
              {#each displayedImages as image, i (image)}
                <button
                  class="group flex shrink-0 rounded-md p-2 transition-colors {i === galleryIndex ? 'bg-base-300' : 'hover:bg-base-300/50'}"
                  onclick={() => galleryGoTo(i)}
                >
                  <img
                    src={image + "?w=256"}
                    alt=""
                    loading="lazy"
                    class="h-[90px] w-16 rounded-sm object-cover transition-opacity {i === galleryIndex ? '' : 'opacity-50 group-hover:opacity-100'}"
                  />
                </button>
              {/each}
            </div>
          {/if}
          <div
            class="carousel h-full min-w-0 flex-1 overflow-y-hidden"
            bind:this={carouselEl}
            tabindex="-1"
            onscroll={onCarouselScroll}
          >
            {#each displayedImages as image, i (image)}
              <div class="carousel-item h-full w-full items-center justify-center">
                <ZoomableImage src={image} active={i === galleryIndex} bind:this={galleryViews[i]} />
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <div class="flex flex-1 flex-col items-center justify-center gap-2 text-base-content/50">
          <Images class="size-8" />
          <span class="text-sm">No Images</span>
        </div>
      {/if}
    </div>
  </dialog>

  {#each popups as popup, i (popup.id)}
    <Popup
      entries={popup.entries}
      styles={popup.styles}
      fonts={importedFonts}
      anki={popupAnki}
      dict={dictConfig}
      scale={readerConfig.popupScale}
      actionBar={readerConfig.popupActionBar}
      scanModifier={hotkeyConfig.scanModifier}
      scanDelay={hotkeyConfig.scanDelay}
      clickLookup={hotkeyConfig.clickLookup}
      readerHotkeys={readerShortcutKeys}
      {onReaderHotkey}
      disableTransparency={readerConfig.popupDisableTransparency}
      hasChild={i < popups.length - 1}
      placement={popup.placement}
      zIndex={30 + i}
      sasayaki={popup.sasayakiCue && sasayaki?.hasAudio
        ? {
            playing: sasayaki.isPlaying || wasPaused,
            onReplay: () => sasayaki.playCue(popup.sasayakiCue!, true),
            onToggle: toggleSasayakiPlayback,
            onResume: () => {
              sasayaki.playCue(popup.sasayakiCue!, false);
              wasPaused = false;
              closePopups();
            },
          }
        : null}
      onRedirect={lookup}
      onKanjiRedirect={redirectKanji}
      onSelected={(text, sentence, offset, rect) => popupLookup(i, text, sentence, offset, rect)}
      onPress={() => closePopups(i + 1)}
      onClose={() => closePopups(i)}
      onHover={(over) => hider.hover(over ? i : -1)}
      onMine={(content) => mineEntry(content, popup)}
      onDuplicateCheck={checkDuplicates}
      onShowNotes={showNotes}
    />
  {/each}

  {#each ["left", "right"] as side}
    {@const panel = side === "left" ? leftPanel : rightPanel}
    {@const panelOpen = readerPanel !== null && readerPanel === panel}
    <aside
      inert={!panelOpen}
      class="absolute inset-y-0 z-[2203] flex w-88 flex-col overflow-hidden border-base-300 bg-base-100/80 backdrop-blur transition-all duration-300 {side === 'right' ? 'right-0 border-l' : 'left-0 border-r'} {panelOpen ? 'pointer-events-auto translate-x-0 opacity-100' : `pointer-events-none opacity-0 ${side === 'right' ? 'translate-x-full' : '-translate-x-full'}`}"
    >
      <header
        style={macChrome ? "padding-top: 2.0625rem" : ""}
        class="flex shrink-0 items-center justify-between border-b border-base-300 px-4 py-2.5"
      >
        <h2 class="text-sm font-semibold">{panel}</h2>
        <button
          class="btn btn-ghost btn-sm btn-square"
          onclick={() => openReaderPanel(null)}
        >
          <X class="size-4" />
        </button>
      </header>

      {#if side === "left"}
        <div class="min-h-0 flex-1 flex-col {panel === 'Search' ? 'flex' : 'hidden'}">
          <BookSearch {id} {positionLabel} onJump={jumpToSearchResult} bind:this={bookSearch} />
        </div>
      {/if}

      {#if panel === "Chapters"}
        <div class="flex shrink-0 items-start gap-3 px-4 py-3">
          {#if cover}
            <img
              src={schemeUrl("cover", id)}
              alt=""
              class="h-[75px] w-[50px] shrink-0 rounded object-cover"
            />
          {:else}
            <div class="h-[75px] w-[50px] shrink-0 rounded bg-base-content/20"></div>
          {/if}
          <div class="flex min-w-0 flex-1 flex-col gap-1">
            <span class="line-clamp-2 text-sm font-semibold">{title}</span>
            <div class="grid {pageProgress ? 'grid-cols-[auto_auto_auto_minmax(0,1fr)]' : 'grid-cols-[auto_auto_minmax(0,1fr)]'} items-center gap-x-2 whitespace-nowrap text-xs tabular-nums text-base-content/60">
              <span></span>
              <span class="flex items-center gap-1 text-base-content/40">
                Characters
                <button class="btn btn-ghost btn-xs btn-square h-4 min-h-0 text-base-content/60" title="Jump to" onclick={() => (jumpToOpen = !jumpToOpen)}>
                  <ArrowRightToLine class="size-3.5" />
                </button>
              </span>
              {#if pageProgress}
                <span class="text-base-content/40">Pages</span>
              {/if}
              <span></span>
              <span>Book</span>
              <span>{currentChar} / {bookInfo.characterCount}</span>
              {#if pageProgress}
                <span>{pageProgress.page} / {pageProgress.total}</span>
              {/if}
              <span class="overflow-hidden text-right">{bookPercent}%</span>
              <span>Chapter</span>
              <span>{chapterRange.character} / {chapterRange.total}</span>
              {#if pageProgress}
                <span>{pageProgress.chapterPage} / {pageProgress.chapterTotal}</span>
              {/if}
              <span class="overflow-hidden text-right">{(chapterRange.total > 0 ? chapterRange.character / chapterRange.total * 100 : 0).toFixed(1)}%</span>
            </div>
          </div>
        </div>

        {#if jumpToOpen}
          <div class="flex flex-col gap-2 border-b border-base-300 px-4 pb-3">
            <span class="text-sm font-semibold">Jump to</span>
            <input
              class="input input-sm w-full"
              placeholder="Character count"
              bind:value={jumpToInput}
              onkeydown={(e) => e.key === "Enter" && confirmJumpTo()}
            />
            {#if jumpToInvalid}
              <span class="text-xs text-error">Please enter a valid character count</span>
            {/if}
            <div class="flex justify-end gap-2">
              <button
                class="btn btn-ghost btn-sm"
                onclick={() => {
                  jumpToOpen = false;
                  jumpToInput = "";
                  jumpToInvalid = false;
                }}
              >
                Cancel
              </button>
              <button class="btn btn-neutral btn-sm" onclick={confirmJumpTo}>Go</button>
            </div>
          </div>
        {/if}

        <ul class="min-h-0 w-full flex-1 overflow-y-auto py-1" bind:this={chapterList}>
          {#each toc as item, i (i)}
            <li data-chapter={i}>
              <button
                class="relative isolate flex w-full items-center justify-between gap-3 py-2 pr-4 text-left hover:bg-base-200 {i === currentTocIndex ? 'bg-base-200' : ''} {i < currentTocIndex ? 'text-base-content/50' : ''}"
                style="padding-left: {16 + item.indentLevel * 16}px"
                title={item.label}
                onclick={() => {
                  closePopups();
                  navigateTo(item.spineIndex, 0, item.fragment);
                }}
              >
                {#if i === currentTocIndex && chapterRange.total > 0}
                  <span
                    class="absolute inset-y-0 left-0 -z-10 bg-base-300"
                    style:width="{(chapterRange.character / chapterRange.total) * 100}%"
                  ></span>
                {/if}
                <span
                  class="min-w-0 flex-1 truncate {item.indentLevel > 0 ? 'text-[13px]' : 'text-sm'}"
                >
                  {item.label}
                </span>
                {#if chapterChars(item) !== null}
                  <span class="shrink-0 text-xs tabular-nums text-base-content/50">
                    {positionLabel(chapterChars(item)!, item.spineIndex)}
                  </span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {:else if panel === "Highlights"}
        <HighlightList {highlights} {bookInfo} {toc} {positionLabel} onDelete={removeHighlight} onJump={(highlight) => {
          closePopups();
          jumpToCharacter(highlight.character);
        }} />
      {:else if panel === "Sasayaki" && sasayaki}
        <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
          <div class="card border border-base-300">
            <div class="card-body gap-4 p-3">
              <div class="flex min-w-0 items-center gap-3">
                {#if sasayaki.cover}
                  <img src={sasayaki.cover} alt="" class="size-12 shrink-0 rounded-md object-cover" />
                {/if}
                <span class="truncate text-sm">{sasayaki.playback.audioPath?.split(/[\\/]/).pop() ?? "No file selected"}</span>
              </div>
              {#if sasayaki.hasAudio}
                <div class="flex items-center justify-center gap-3 text-base-content/60 {vertical ? '-scale-x-100' : ''}">
                  <button class="btn btn-ghost btn-sm btn-square" onclick={() => sasayaki.skip(false)}>
                    <RotateCcw class="size-4" />
                  </button>
                  <button class="btn btn-ghost btn-sm btn-square" onclick={() => sasayaki.prevCue()}>
                    <Rewind class="size-4" />
                  </button>
                  <button
                    class="btn btn-circle border-0 bg-base-content text-base-100 {vertical ? '-scale-x-100' : ''}"
                    onclick={() => sasayaki.togglePlayback()}
                  >
                    {#if sasayaki.isPlaying}
                      <Pause class="size-4" fill="currentColor" />
                    {:else}
                      <Play class="size-4" fill="currentColor" />
                    {/if}
                  </button>
                  <button class="btn btn-ghost btn-sm btn-square" onclick={() => sasayaki.nextCue()}>
                    <FastForward class="size-4" />
                  </button>
                  <button class="btn btn-ghost btn-sm btn-square" onclick={() => sasayaki.skip(true)}>
                    <RotateCw class="size-4" />
                  </button>
                </div>
                <div class="flex flex-col gap-1">
                  <input
                    type="range"
                    class="range range-xs text-primary"
                    min="0"
                    max={sasayaki.duration}
                    step="0.1"
                    value={sasayaki.currentTime}
                    oninput={(e) => sasayaki.scrub(Number(e.currentTarget.value))}
                  />
                  <div class="flex justify-between text-xs tabular-nums text-base-content/60">
                    <span>{formatTime(sasayaki.currentTime)}</span>
                    <span>−{formatTime(Math.max(sasayaki.duration - sasayaki.currentTime, 0))}</span>
                  </div>
                </div>
              {/if}
            </div>
          </div>
          {#if sasayaki.errorMessage}
            <span class="select-text text-sm text-error">{sasayaki.errorMessage}</span>
          {/if}

          <SettingGroup>
            <SettingSlider
              label="Delay"
              compact
              value={sasayaki.delay}
              display={`${sasayaki.delay >= 0 ? "+" : ""}${sasayaki.delay.toFixed(2)}s`}
              min={-4}
              max={4}
              step={0.05}
              onchange={(value) => sasayaki.setDelay(value)}
            />
            <SettingSlider
              label="Speed"
              compact
              value={sasayaki.rate}
              display={`${sasayaki.rate.toFixed(2)}x`}
              min={0.5}
              max={3}
              step={0.05}
              onchange={(value) => sasayaki.setRate(value)}
            />
            <SettingSlider
              label="Volume"
              compact
              value={sasayaki.volume}
              display={`${Math.round(sasayaki.volume * 100)}%`}
              min={0}
              max={1}
              step={0.05}
              onchange={(value) => sasayaki.setVolume(value)}
            />
          </SettingGroup>

          <SasayakiSettings compact />

          <div class="mt-auto flex gap-2">
            <button class="btn btn-outline btn-sm flex-1" onclick={loadSasayakiAudio}>
              <Upload class="size-4" />
              Load Audio
            </button>
            <button class="btn btn-outline btn-sm flex-1" onclick={() => matchDialog?.show(id)}>
              <Captions class="size-4" />
              Match
            </button>
          </div>
        </div>
      {:else if panel === "Appearance"}
        <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
          <Appearance
            compact
            bind:fonts={importedFonts}
            onRestyle={postRestyle}
            onReload={pushFrame}
            onPopupResize={relayoutPopups}
          />
        </div>
      {/if}
    </aside>
  {/each}
  <MatchDialog bind:this={matchDialog} {onMatched} />
</div>
