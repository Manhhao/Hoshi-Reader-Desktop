<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Search } from "@lucide/svelte";
  import Popup from "./Popup.svelte";
  import PageHeader from "./PageHeader.svelte";
  import { dictConfig } from "./dictConfig.svelte";
  import { hotkeyConfig } from "./hotkeyConfig.svelte";
  import { readerConfig } from "./readerConfig.svelte";
  import { calculatePopupLayout, type PopupPlacement } from "./popupLayout";
  import type {
    FontInfo,
    KanjiResponse,
    LookupEntry,
    LookupResponse,
    MineContent,
    PopupAnkiConfig,
    SelectionRect,
  } from "./types";

  type PopupInstance = {
    id: string;
    entries: LookupEntry[];
    styles: Record<string, string>;
    placement: PopupPlacement;
    sentence: string;
    clozeOffset: number | null;
  };

  let query = $state("");
  let searchText = $state("");
  let entries = $state<LookupEntry[]>([]);
  let styles = $state<Record<string, string>>({});
  let popups = $state<PopupInstance[]>([]);
  let popupAnki = $state<PopupAnkiConfig | null>(null);
  let importedFonts = $state<FontInfo[]>([]);
  let clozeOffset = $state<number | null>(null);
  let paneEl = $state<HTMLDivElement | null>(null);
  let lookupSeq = 0;

  function lookup(text: string): Promise<LookupResponse> {
    return invoke<LookupResponse>("lookup", {
      text,
      maxResults: dictConfig.maxResults,
      scanLength: dictConfig.scanLength,
      frequencySortOrder: dictConfig.frequencySortOrder,
      frequencySortDictionary: dictConfig.frequencySortDictionary,
    });
  }

  function placePopup(rect: SelectionRect): PopupPlacement {
    return calculatePopupLayout(
      rect,
      { width: window.innerWidth, height: window.innerHeight },
      readerConfig.popupWidth,
      readerConfig.popupHeight,
      false,
    );
  }

  function resetResults() {
    searchText = "";
    entries = [];
    styles = {};
  }

  function closePopups(keep = 0) {
    lookupSeq++;
    popups = popups.slice(0, keep);
  }

  async function runLookup() {
    closePopups();
    clozeOffset = null;
    const trimmed = query.trim();
    const seq = lookupSeq;
    if (!trimmed) {
      resetResults();
      return;
    }
    const response = await lookup(trimmed);
    if (seq !== lookupSeq) return;
    popupAnki = await invoke<PopupAnkiConfig>("anki_config");
    if (seq !== lookupSeq) return;
    searchText = trimmed;
    entries = response.entries;
    styles = response.styles;
  }

  function clearResults() {
    closePopups();
    resetResults();
  }

  async function paneRedirect(text: string): Promise<LookupResponse> {
    closePopups();
    const response = await lookup(text);
    if (response.entries.length) {
      clozeOffset = searchText.endsWith(text) ? searchText.length - text.length : null;
    }
    return response;
  }

  function redirectKanji(character: string): Promise<KanjiResponse | null> {
    return invoke<KanjiResponse | null>("lookup_kanji", { character });
  }

  async function openPopup(
    index: number,
    text: string,
    sentence: string | null,
    offset: number | null,
    rect: SelectionRect | null,
  ): Promise<number | null> {
    const seq = ++lookupSeq;
    const response = await lookup(text);
    if (seq !== lookupSeq || !response.entries.length) return null;
    popupAnki = await invoke<PopupAnkiConfig>("anki_config");
    if (seq !== lookupSeq) return null;
    const base = index < 0 ? paneEl?.getBoundingClientRect() : null;
    const parent = index < 0
      ? { left: base?.left ?? 0, top: base?.top ?? 0, width: base?.width ?? 0 }
      : {
          left: popups[index].placement.left,
          top: popups[index].placement.top,
          width: popups[index].placement.width,
        };
    const screenRect = rect
      ? { x: parent.left + rect.x, y: parent.top + rect.y, width: rect.width, height: rect.height }
      : { x: parent.left, y: parent.top, width: parent.width, height: 0 };
    popups = [
      ...popups.slice(0, index + 1),
      {
        id: popups[index + 1]?.id ?? crypto.randomUUID(),
        entries: response.entries,
        styles: response.styles,
        placement: placePopup(screenRect),
        sentence: sentence ?? (index < 0 ? searchText : popups[index].sentence),
        clozeOffset: sentence !== null ? offset : index < 0 ? clozeOffset : popups[index].clozeOffset,
      },
    ];
    return [...response.entries[0].matched].length;
  }

  function mineEntry(
    content: MineContent,
    sentence: string,
    offset: number | null,
  ): Promise<boolean> {
    return invoke<boolean>("anki_mine", {
      content,
      context: {
        sentence,
        clozeOffset: offset,
        documentTitle: null,
        bookId: null,
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

  invoke<FontInfo[]>("list_fonts").then((fonts) => (importedFonts = fonts));
</script>

<div class="flex h-full flex-col bg-base-100">
  <PageHeader title="Dictionary">
    <label class="input input-sm min-w-0 flex-1">
      <Search class="size-4 text-base-content/50" />
      <input
        type="search"
        lang="ja"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        bind:value={query}
        onkeydown={(e) => {
          if (e.key === "Enter") runLookup();
        }}
      />
    </label>
  </PageHeader>

  <div class="min-h-0 flex-1" bind:this={paneEl}>
    <Popup
      {entries}
      {styles}
      fonts={importedFonts}
      anki={popupAnki}
      dict={dictConfig}
      scale={readerConfig.popupScale}
      actionBar={false}
      scanModifier={hotkeyConfig.scanModifier}
      clickLookup={hotkeyConfig.clickLookup}
      disableTransparency
      fill
      {searchText}
      searchTextSize={dictConfig.searchTextSize}
      onRedirect={paneRedirect}
      onKanjiRedirect={redirectKanji}
      onSelected={(text, sentence, offset, rect) => openPopup(-1, text, sentence, offset, rect)}
      onPress={() => closePopups()}
      onClose={clearResults}
      onMine={(content) => mineEntry(content, searchText, clozeOffset)}
      onDuplicateCheck={checkDuplicates}
      onShowNotes={showNotes}
    />
  </div>
</div>

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
    clickLookup={hotkeyConfig.clickLookup}
    disableTransparency={readerConfig.popupDisableTransparency}
    placement={popup.placement}
    zIndex={30 + i}
    onRedirect={lookup}
    onKanjiRedirect={redirectKanji}
    onSelected={(text, sentence, offset, rect) => openPopup(i, text, sentence, offset, rect)}
    onPress={() => closePopups(i + 1)}
    onClose={() => closePopups(i)}
    onMine={(content) => mineEntry(content, popup.sentence, popup.clozeOffset)}
    onDuplicateCheck={checkDuplicates}
    onShowNotes={showNotes}
  />
{/each}
