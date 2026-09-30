<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { CheckMenuItem, Menu } from "@tauri-apps/api/menu";
  import { Pause, Play, RotateCcw, StepForward } from "@lucide/svelte";
  import type {
    FontInfo,
    KanjiResponse,
    LookupEntry,
    LookupResponse,
    MineContent,
    PopupAnkiConfig,
    SelectionRect,
  } from "./types";
  import type { DictConfig } from "./dictConfig.svelte";
  import type { ClickLookup } from "./hotkeyConfig.svelte";
  import type { PopupPlacement } from "./popupLayout";
  import { schemeUrl } from "./scheme";

  let {
    entries,
    styles,
    fonts,
    anki,
    dict,
    scale,
    actionBar,
    scanModifier,
    clickLookup,
    disableTransparency,
    fill = false,
    placement,
    zIndex,
    searchText = "",
    searchTextSize = 22,
    sasayaki = null,
    readerHotkeys = [],
    onReaderHotkey,
    onRedirect,
    onKanjiRedirect,
    onSelected,
    onPress,
    onClose,
    onMine,
    onDuplicateCheck,
    onShowNotes,
  }: {
    entries: LookupEntry[];
    styles: Record<string, string>;
    fonts: FontInfo[];
    anki: PopupAnkiConfig | null;
    dict: DictConfig;
    scale: number;
    actionBar: boolean;
    scanModifier: string;
    clickLookup: ClickLookup;
    disableTransparency: boolean;
    fill?: boolean;
    placement?: PopupPlacement;
    zIndex?: number;
    searchText?: string;
    searchTextSize?: number;
    sasayaki?: {
      playing: boolean;
      onReplay: () => void;
      onToggle: () => void;
      onResume: () => void;
    } | null;
    readerHotkeys?: string[];
    onReaderHotkey?: (key: string) => void;
    onRedirect: (query: string) => Promise<LookupResponse>;
    onKanjiRedirect: (character: string) => Promise<KanjiResponse | null>;
    onPress: () => void;
    onClose: () => void;
    onSelected: (
      text: string,
      sentence: string | null,
      clozeOffset: number | null,
      rect: SelectionRect | null,
    ) => Promise<number | null>;
    onMine: (content: MineContent) => Promise<boolean>;
    onDuplicateCheck: (fields: Record<string, string>) => Promise<boolean[]>;
    onShowNotes: (fields: Record<string, string>) => void;
  } = $props();

  let iframeEl: HTMLIFrameElement;
  let channelReady = $state(false);

  function send(msg: unknown) {
    iframeEl?.contentWindow?.postMessage(msg, "*");
  }

  function popupConfig() {
    return {
      scale,
      actionBar,
      showClose: !fill,
      scanModifier,
      clickLookup,
      readerHotkeys: [...readerHotkeys],
      fonts: fonts.map((font) => ({
        name: font.name,
        url: schemeUrl("book", `__hoshi/Fonts/${encodeURIComponent(font.fileName)}`),
      })),
    };
  }

  async function showAudioMenu(entryIndex: number, names: string[], selected: number) {
    const items = await Promise.all(
      names.map((text, index) =>
        CheckMenuItem.new({
          text,
          checked: index === selected,
          action: () => send({ hoshi: "play-audio", entryIndex, sourceIndex: index }),
        }),
      ),
    );
    const menu = await Menu.new({ items });
    await menu.popup();
  }

  function onMessage(e: MessageEvent) {
    if (!iframeEl || e.source !== iframeEl.contentWindow) return;
    const m = e.data;
    switch (m?.hoshi) {
      case "reader-hotkey":
        onReaderHotkey?.(m.key);
        break;
      case "popup-ready":
        channelReady = true;
        send({ hoshi: "ack" });
        break;
      case "popup-redirect":
        onRedirect(m.query).then((r) =>
          send({
            hoshi: "redirect-result",
            id: m.id,
            count: r.entries.length,
            entries: r.entries,
            styles: r.styles,
          }),
        );
        break;
      case "audio-menu":
        showAudioMenu(m.entryIndex, m.names, m.selected);
        break;
      case "popup-kanji":
        onKanjiRedirect(m.character).then((result) =>
          send({ hoshi: "kanji-result", id: m.id, result }),
        );
        break;
      case "popup-selected":
        onSelected(m.text, m.sentence, m.clozeOffset, m.rect).then((count) => {
          if (count) send({ hoshi: "highlight", count });
        });
        break;
      case "mine":
        onMine(m.content).then((result) => send({ hoshi: "mine-result", id: m.id, result }));
        break;
      case "dupecheck":
        onDuplicateCheck(m.fields).then((result) =>
          send({ hoshi: "dupecheck-result", id: m.id, result }),
        );
        break;
      case "show-notes":
        onShowNotes(m.fields);
        break;
      case "open-link":
        invoke("open_external", { url: m.url });
        break;
      case "popup-press":
        onPress();
        break;
      case "popup-close":
        onClose();
        break;
    }
  }

  $effect(() => {
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  });

  $effect(() => {
    if (!channelReady) return;
    send({ hoshi: "popup-config", popup: popupConfig() });
  });

  let lastShown: LookupEntry[] | null = null;
  $effect(() => {
    if (!channelReady || entries === lastShown || (!entries.length && !fill)) return;
    lastShown = entries;
    send({
      hoshi: "show",
      entries: $state.snapshot(entries),
      styles: $state.snapshot(styles),
      anki: anki ? $state.snapshot(anki) : null,
      dict: $state.snapshot(dict),
      popup: popupConfig(),
    });
  });

  $effect(() => {
    if (!channelReady || !fill) return;
    send({ hoshi: "search-text", text: searchText, size: searchTextSize });
  });
</script>

<div
  class={fill
    ? "flex h-full w-full flex-col overflow-hidden"
    : `fixed flex flex-col overflow-hidden rounded-lg border border-base-content/30 shadow-md ${disableTransparency ? "bg-base-100" : "bg-base-100/45 backdrop-blur-2xl backdrop-saturate-150"}`}
  style={fill
    ? ""
    : `left:${placement?.left}px; top:${placement?.top}px; width:${placement?.width}px; height:${placement?.height}px; z-index:${zIndex}`}
>
  {#if sasayaki}
    <div class="flex shrink-0 items-center justify-center gap-5 border-b border-base-content/20 py-1.5 text-base-content/60">
      <button class="btn btn-ghost btn-xs btn-square" onclick={sasayaki.onReplay}>
        <RotateCcw class="size-4" />
      </button>
      <button class="btn btn-ghost btn-xs btn-square" onclick={sasayaki.onToggle}>
        {#if sasayaki.playing}
          <Pause class="size-4" fill="currentColor" />
        {:else}
          <Play class="size-4" fill="currentColor" />
        {/if}
      </button>
      <button class="btn btn-ghost btn-xs btn-square" onclick={sasayaki.onResume}>
        <StepForward class="size-4" />
      </button>
    </div>
  {/if}
  <iframe
    bind:this={iframeEl}
    src="/popup/popup.html"
    title=""
    class:invisible={!channelReady}
    class="min-h-0 w-full flex-1 border-0 bg-transparent"
  ></iframe>
</div>
