<script module lang="ts">
  export const settingsTabs = [
    ["appearance", "Appearance"],
    ["dictionaries", "Dictionaries"],
    ["anki", "Anki"],
    ["audio", "Audio"],
    ["syncing", "Syncing"],
    ["input", "Input"],
    ["sasayaki", "Sasayaki"],
    ["backup", "Backup"],
    ["about", "About"],
  ] as const;
  export type SettingsTab = (typeof settingsTabs)[number][0];
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { ask, message, open, save } from "@tauri-apps/plugin-dialog";
  import {
    ALargeSmall,
    ArrowDown,
    ArrowUp,
    ArrowUpDown,
    BookA,
    ChevronDown,
    ChevronRight,
    CircleAlert,
    CirclePlus,
    DiamondPlus,
    Menu,
    Plus,
    SquarePlus,
    Trash2,
  } from "@lucide/svelte";
  import type {
    AnkiCardFormat,
    AnkiSettings,
    GoogleDriveSyncStatus,
  } from "./types";
  import {
    dictConfig,
    dictionaryUpdate,
    dictionaryUpdateIntervals,
    loadCollapsedDictionaries,
    saveCollapsedDictionaries,
    saveDictConfig,
    updateDictionaries,
    type CollapseMode,
  } from "./dictConfig.svelte";
  import { dropdownFlip } from "./dropdownFlip";
  import {
    hotkeyConfig,
    hotkeyLabel,
    mouseButtonTokens,
    normalizeHotkey,
    readerHotkeys,
    saveHotkeyConfig,
    type HotkeyConfig,
  } from "./hotkeyConfig.svelte";
  import { Sortable } from "./sortable.svelte";
  import { sasayakiConfig, saveSasayakiConfig } from "./sasayakiConfig.svelte";
  import { configureSync, syncConfig, saveSyncConfig, syncProviders } from "./syncConfig.svelte";
  import Appearance from "./Appearance.svelte";
  import About from "./About.svelte";
  import { defaultFonts } from "./readerConfig.svelte";
  import SasayakiSettings from "./SasayakiSettings.svelte";
  import SettingStepper from "./SettingStepper.svelte";
  import SettingToggle from "./SettingToggle.svelte";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import SharingSettings from "./SharingSettings.svelte";
  import PageHeader from "./PageHeader.svelte";

  let {
    tab,
    onReload,
  }: {
    tab: SettingsTab;
    onReload: () => Promise<void>;
  } = $props();

  const tabLabel = $derived(settingsTabs.find(([id]) => id === tab)![1]);

  let syncAuthenticated = $state(false);
  let syncConnecting = $state(false);
  let driveStatus = $state<GoogleDriveSyncStatus>({
    lastSync: null,
    isSyncing: false,
    errorMessage: null,
    queue: [],
    progress: null,
  });
  const failedBooks = $derived(driveStatus.queue.filter((item) => item.error).length);

  const lastSyncFormat = new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "numeric",
  });

  async function refreshSyncStatus() {
    syncAuthenticated = await invoke<boolean>("sync_authenticated");
  }

  refreshSyncStatus();

  $effect(() => {
    invoke<GoogleDriveSyncStatus>("gdrive_sync_state").then((status) => (driveStatus = status));
    const unlisten = listen<GoogleDriveSyncStatus>("sync://status", ({ payload }) => {
      driveStatus = payload;
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  async function changeEnableSync() {
    saveSyncConfig();
    await configureSync();
    invoke(syncConfig.enableSync ? "gdrive_sync_start" : "gdrive_sync_stop");
  }

  async function connectGoogleDrive() {
    syncConnecting = true;
    try {
      await invoke("sync_connect");
      await refreshSyncStatus();
    } catch (e) {
      await message(String(e), { title: "Error", kind: "error" });
    } finally {
      syncConnecting = false;
    }
  }

  async function clearSyncCache() {
    if (!(await ask("This will clear cached folder ids and book covers.", {
      title: "Clear Cache?",
      kind: "warning",
      okLabel: "Clear",
    }))) return;
    try {
      await invoke("sync_clear_cache");
    } catch (e) {
      await message(String(e), { title: "Error", kind: "error" });
    }
  }

  async function signOutSync() {
    if (!(await ask("Signing out will clear authorization tokens, cached folder ids and book covers.", {
      title: "Sign out?",
      kind: "warning",
      okLabel: "Confirm",
    }))) return;
    try {
      await invoke("sync_disconnect");
    } catch (e) {
      await message(String(e), { title: "Error", kind: "error" });
    }
    await refreshSyncStatus();
  }

  let bindingKey = $state<Exclude<keyof HotkeyConfig, "clickLookup" | "disableReaderWheel"> | null>(null);
  let bindingError = $state("");

  function startBinding(key: Exclude<keyof HotkeyConfig, "clickLookup" | "disableReaderWheel">, button: HTMLButtonElement) {
    button.focus();
    bindingKey = key;
    bindingError = "";
  }

  function onBindKeydown(e: KeyboardEvent) {
    if (!bindingKey) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat || e.isComposing) return;
    if (bindingKey !== "scanModifier" && (e.ctrlKey || e.altKey || e.metaKey || ["Shift", "Control", "Alt", "Meta"].includes(e.key))) {
      bindingError = "Modifier keys are not allowed for this action.";
      return;
    }
    const key = e.key === "Escape" ? "" : normalizeHotkey(e.key);
    const conflict = [{ key: "scanModifier" as const, label: "Scan Modifier" }, ...readerHotkeys]
      .find((binding) => key && binding.key !== bindingKey && hotkeyConfig[binding.key] === key);
    if (conflict) {
      bindingError = `Already used by ${conflict.label}.`;
      return;
    }
    hotkeyConfig[bindingKey] = key;
    saveHotkeyConfig();
    bindingKey = null;
    bindingError = "";
  }

  function onBindMouse(e: MouseEvent) {
    if (!bindingKey || bindingKey === "scanModifier" || e.button === 0) return;
    const key = mouseButtonTokens[e.button];
    if (!key) return;
    e.preventDefault();
    e.stopPropagation();
    if (key === "Mouse:Right" && hotkeyConfig.clickLookup === "right") {
      bindingError = "Already used by Scan on Click.";
      return;
    }
    const conflict = readerHotkeys.find(
      (binding) => binding.key !== bindingKey && hotkeyConfig[binding.key] === key,
    );
    if (conflict) {
      bindingError = `Already used by ${conflict.label}.`;
      return;
    }
    hotkeyConfig[bindingKey] = key;
    saveHotkeyConfig();
    bindingKey = null;
    bindingError = "";
  }

  type DictionaryType = "term" | "frequency" | "pitch" | "kanji";
  type DictionaryCategory = "none" | "monolingual" | "bilingual" | "exclude";

  type DictionaryInfo = {
    title: string;
    revision: string;
    fileName: string;
    isEnabled: boolean;
    order: number;
    category: DictionaryCategory;
    isUpdatable: boolean;
  };

  type DictionaryLists = {
    term: DictionaryInfo[];
    frequency: DictionaryInfo[];
    pitch: DictionaryInfo[];
    kanji: DictionaryInfo[];
  };

  type ImportSummary = {
    imported: string[];
    failed: string[];
  };

  let selected = $state<DictionaryType>("term");
  let dictionaries = $state<DictionaryLists>({ term: [], frequency: [], pitch: [], kanji: [] });
  let importing = $state(false);
  let error = $state("");
  let hasStrokeOrderFont = $state(false);
  let cssFonts = $state<string[]>([]);
  let cssEditor = $state<HTMLTextAreaElement>();
  let downloadingStrokeOrderFont = $state(false);
  const enabledFrequencyDictionaries = $derived(dictionaries.frequency.filter((dict) => dict.isEnabled));
  const usesFrequencyDictionary = $derived(
    dictConfig.frequencySortOrder === "Ascending" || dictConfig.frequencySortOrder === "Descending",
  );

  $effect(() => {
    if (tab === "dictionaries") {
      invoke<{ name: string }[]>("list_fonts").then((fonts) => {
        cssFonts = [...defaultFonts, ...fonts.map((font) => font.name)];
        hasStrokeOrderFont = fonts.some((font) => font.name === "KanjiStrokeOrders_v4.005");
      });
    }
  });

  async function downloadStrokeOrderFont() {
    if (!(await ask("This will download and automatically import the kanji stroke order font (17 MB)", {
      title: "Download Font",
      okLabel: "Download",
      cancelLabel: "Cancel",
    }))) return;
    downloadingStrokeOrderFont = true;
    error = "";
    try {
      await invoke("download_stroke_order_font");
      hasStrokeOrderFont = true;
      cssFonts = [...cssFonts, "KanjiStrokeOrders_v4.005"];
    } catch {
      error = "Failed to download the stroke order font";
    } finally {
      downloadingStrokeOrderFont = false;
    }
  }

  function insertCSS(text: string) {
    const editor = cssEditor!;
    editor.setRangeText(text, editor.selectionStart, editor.selectionEnd, "end");
    dictConfig.customCSS = editor.value;
    saveDictConfig();
    editor.focus();
  }

  function changeFrequencySortOrder() {
    if (usesFrequencyDictionary && !enabledFrequencyDictionaries.some((dict) => dict.title === dictConfig.frequencySortDictionary)) {
      dictConfig.frequencySortDictionary = enabledFrequencyDictionaries[0]?.title ?? "";
    }
    saveDictConfig();
  }

  let anki = $state<AnkiSettings | null>(null);
  let ankiUrl = $state("");
  let ankiApiKey = $state("");
  let connecting = $state(false);
  let ankiReachable = $state(false);
  let ankiError = $state("");
  let backupBusy = $state<BackupFolder | null>(null);
  let backupError = $state("");
  let audioNameInput = $state("");
  let audioUrlInput = $state("");
  const localAudioName = $derived(anki?.localAudioPath?.split(/[\\/]/).pop() || "database");
  const localAudioUrl = "local-audio://get/?term={term}&reading={reading}";
  const localAudioAddonUrl = "http://127.0.0.1:5050/?term={term}&reading={reading}";
  const localAudioType = $derived(
    anki?.audioSources.some((source) => source.url === localAudioAddonUrl) ? "anki"
      : anki?.audioSources.some((source) => source.url === localAudioUrl) ? "database"
      : "none",
  );

  const maxFormats = 3;

  const handlebars: { name: string; advanced?: boolean }[] = [
    { name: "{expression}" },
    { name: "{reading}" },
    { name: "{furigana-plain}" },
    { name: "{audio}" },
    { name: "{glossary}" },
    { name: "{glossary-brief}", advanced: true },
    { name: "{glossary-no-dictionary}", advanced: true },
    { name: "{glossary-first}" },
    { name: "{glossary-first-brief}", advanced: true },
    { name: "{glossary-first-no-dictionary}", advanced: true },
    { name: "{monolingual-definition}", advanced: true },
    { name: "{monolingual-definition-brief}", advanced: true },
    { name: "{monolingual-definition-no-dictionary}", advanced: true },
    { name: "{bilingual-definition}", advanced: true },
    { name: "{bilingual-definition-brief}", advanced: true },
    { name: "{bilingual-definition-no-dictionary}", advanced: true },
    { name: "{monolingual-definition-fallback}", advanced: true },
    { name: "{monolingual-definition-fallback-brief}", advanced: true },
    { name: "{monolingual-definition-fallback-no-dictionary}", advanced: true },
    { name: "{bilingual-definition-fallback}", advanced: true },
    { name: "{bilingual-definition-fallback-brief}", advanced: true },
    { name: "{bilingual-definition-fallback-no-dictionary}", advanced: true },
    { name: "{selected-glossary}" },
    { name: "{selected-glossary-brief}", advanced: true },
    { name: "{selected-glossary-no-dictionary}", advanced: true },
    { name: "{popup-selection-text}" },
    { name: "{sentence}" },
    { name: "{cloze-prefix}", advanced: true },
    { name: "{cloze-body}", advanced: true },
    { name: "{cloze-suffix}", advanced: true },
    { name: "{frequencies}" },
    { name: "{frequency-harmonic-rank}" },
    { name: "{pitch-accent-positions}" },
    { name: "{pitch-accent-categories}" },
    { name: "{pitch-accent-graphs}" },
    { name: "{pitch-accent-graphs-first}", advanced: true },
    { name: "{document-title}" },
    { name: "{book-cover}" },
    { name: "{sasayaki-audio}" },
  ];

  const fallbackOptions = [
    "{glossary-first}",
    "{monolingual-definition}",
    "{bilingual-definition}",
    "{monolingual-definition-fallback}",
    "{bilingual-definition-fallback}",
  ];

  const availableHandlebars = $derived.by(() => {
    const showAll = anki?.showAllHandlebars ?? false;
    const options = handlebars.filter((h) => showAll || !h.advanced).map((h) => h.name);
    for (const d of dictionaries.term) {
      options.push(`{single-glossary-${d.title}}`);
      if (showAll) {
        options.push(`{single-glossary-${d.title}-brief}`);
        options.push(`{single-glossary-${d.title}-no-dictionary}`);
      }
    }
    return options;
  });

  const labels: Record<DictionaryType, string> = {
    term: "Term",
    frequency: "Frequency",
    pitch: "Pitch",
    kanji: "Kanji",
  };

  const visible = $derived(dictionaries[selected]);

  const updatableTitles = $derived.by(() => {
    const titles: string[] = [];
    for (const kind of Object.keys(labels) as DictionaryType[]) {
      for (const dict of dictionaries[kind]) {
        if (dict.isUpdatable && !titles.includes(dict.title)) titles.push(dict.title);
      }
    }
    return titles;
  });

  const updateMessage = $derived(
    `This will check for and install updates for these dictionaries:\n${updatableTitles.join("\n")}`,
  );

  const lastUpdate = $derived(
    dictConfig.lastDictionaryUpdate === null
      ? "Never"
      : new Date(dictConfig.lastDictionaryUpdate).toLocaleString(),
  );

  async function runUpdate() {
    error = "";
    try {
      const summary = await updateDictionaries();
      if (summary?.failed.length) error = summary.failed.join("\n");
    } catch (err) {
      error = String(err);
    }
    await refresh();
  }

  async function refresh() {
    dictionaries = await invoke<DictionaryLists>("list_dictionaries");
  }

  type BackupFolder = "Books" | "Dictionaries";

  function backupFileName(folder: BackupFolder) {
    const date = new Date();
    const pad = (value: number) => String(value).padStart(2, "0");
    return `${folder}_${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}_${pad(date.getHours())}-${pad(date.getMinutes())}-${pad(date.getSeconds())}.hoshi`;
  }

  async function backupStorage(folder: BackupFolder) {
    const destination = await save({
      defaultPath: backupFileName(folder),
      filters: [{ name: "Hoshi Backup", extensions: ["hoshi"] }],
    });
    if (!destination) return;
    backupBusy = folder;
    backupError = "";
    try {
      await invoke("backup_folder", { folder, destination });
    } catch (error) {
      backupError = String(error);
    } finally {
      backupBusy = null;
    }
  }

  async function restoreStorage(folder: BackupFolder) {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Hoshi Backup", extensions: ["hoshi"] }],
    });
    if (!selected) return;
    backupBusy = folder;
    backupError = "";
    try {
      await invoke("restore_folder", { folder, path: selected });
      if (folder === "Books") {
        invoke("gdrive_sync_start");
        await onReload();
      } else {
        await refresh();
        await loadCollapsedDictionaries();
      }
    } catch (error) {
      if (folder === "Books") {
        syncConfig.enableSync = false;
        saveSyncConfig();
        await configureSync();
        invoke("gdrive_sync_start");
      }
      backupError = String(error);
    } finally {
      backupBusy = null;
    }
  }

  async function loadAnki() {
    anki = await invoke<AnkiSettings>("anki_get_settings");
    ankiUrl = anki.url ?? "";
    ankiApiKey = anki.apiKey ?? "";
    if (anki.disabled) return;
    ankiReachable = await invoke<boolean>("anki_reachable");
    if (!ankiReachable) ankiReachable = await invoke<boolean>("anki_ping");
    if (ankiReachable) await refreshAnkiData();
  }

  async function refreshAnkiData() {
    try {
      anki = await invoke<AnkiSettings>("anki_fetch");
      ankiReachable = true;
      ankiError = "";
    } catch (err) {
      ankiReachable = false;
      ankiError = String(err);
    }
  }

  async function saveAnki() {
    if (!anki) return;
    anki.url = ankiUrl.trim() || null;
    anki.apiKey = ankiApiKey.trim() || null;
    await invoke("anki_save_settings", { config: $state.snapshot(anki) });
  }

  async function changeAnkiDisabled() {
    await saveAnki();
    ankiReachable = await invoke<boolean>("anki_ping");
    if (ankiReachable) await refreshAnkiData();
  }

  async function connectAnki() {
    if (!anki) return;
    connecting = true;
    ankiError = "";
    try {
      await saveAnki();
      ankiReachable = await invoke<boolean>("anki_ping");
      if (ankiReachable) await refreshAnkiData();
      else ankiError = "Could not connect to AnkiConnect";
    } catch (err) {
      ankiError = String(err);
    } finally {
      connecting = false;
    }
  }

  function noteTypeFields(format: AnkiCardFormat): string[] {
    return (
      anki?.availableNoteTypes.find((n) => n.name === format.selectedNoteType)?.fields ?? []
    );
  }

  let selectedFormatId = $state<string | null>(null);
  const activeFormat = $derived(
    anki?.cardFormats.find((f) => f.id === selectedFormatId) ?? anki?.cardFormats[0] ?? null,
  );

  const duplicateScopeLabels = {
    collection: "Collection",
    deck: "Deck",
    deckroot: "Deck Root",
  } as const;

  const formatIcons = [
    "plus-square",
    "plus-square-small",
    "plus-circle",
    "plus-circle-small",
    "plus-diamond",
    "plus-diamond-small",
  ];

  async function addCardFormat() {
    if (!anki) return;
    const id = crypto.randomUUID();
    anki.cardFormats.push({
      id,
      name: `Format ${anki.cardFormats.length + 1}`,
      selectedDeck:
        anki.availableDecks.find((d) => d.toLowerCase() !== "default") ??
        anki.availableDecks[0] ??
        null,
      selectedNoteType: anki.availableNoteTypes[0]?.name ?? null,
      fieldMappings: {},
      tags: "hoshi",
      icon: "plus-square",
    });
    selectedFormatId = id;
    await saveAnki();
    anki = await invoke<AnkiSettings>("anki_autofill_fields", { formatId: id });
  }

  async function deleteCardFormat(format: AnkiCardFormat) {
    if (!anki) return;
    const name = format.name ? `"${format.name}"` : "this card format";
    if (!(await ask(`Delete ${name}?`, { kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" }))) return;
    anki.cardFormats = anki.cardFormats.filter((f) => f.id !== format.id);
    if (selectedFormatId === format.id) selectedFormatId = null;
    saveAnki();
  }

  async function modelChanged(format: AnkiCardFormat) {
    format.fieldMappings = {};
    await saveAnki();
    anki = await invoke<AnkiSettings>("anki_autofill_fields", { formatId: format.id });
  }

  function setMapping(format: AnkiCardFormat, field: string, value: string) {
    const trimmed = value.trim();
    if (trimmed) format.fieldMappings[field] = value;
    else delete format.fieldMappings[field];
    saveAnki();
  }

  function pickHandlebar(
    format: AnkiCardFormat,
    field: string,
    value: string,
    e: MouseEvent & { currentTarget: HTMLElement },
  ) {
    setMapping(format, field, value);
    e.currentTarget.blur();
  }

  function pickFallback(value: string, e: MouseEvent & { currentTarget: HTMLElement }) {
    if (!anki) return;
    anki.selectedGlossaryFallback = value;
    saveAnki();
    e.currentTarget.blur();
  }

  const categoryLabels: Record<DictionaryCategory, string> = {
    none: "None",
    monolingual: "Monolingual",
    bilingual: "Bilingual",
    exclude: "Exclude",
  };
  const dictionaryCategories = Object.keys(categoryLabels) as DictionaryCategory[];

  async function setDictionaryCategory(dict: DictionaryInfo, category: DictionaryCategory) {
    if (dict.category === category) return;
    dict.category = category;
    await invoke("set_dictionary_category", { fileName: dict.fileName, category });
  }

  let collapsedDialog = $state<HTMLDialogElement | null>(null);
  let categorizeDialog = $state<HTMLDialogElement | null>(null);
  let updateDialog = $state<HTMLDialogElement | null>(null);
  let transfersDialog = $state<HTMLDialogElement | null>(null);

  function addAudioSource() {
    if (!anki) return;
    const url = audioUrlInput.trim();
    const name = audioNameInput.trim();
    if (!url || !name || anki.audioSources.some((s) => s.url === url)) return;
    anki.audioSources.push({ name, url, isEnabled: true });
    audioNameInput = "";
    audioUrlInput = "";
    saveAnki();
  }

  const collapseModes: CollapseMode[] = ["Expand All", "Collapse All", "Custom"];

  function toggleCollapsedDictionary(title: string) {
    const list = dictConfig.collapsedDictionaries;
    const at = list.indexOf(title);
    if (at >= 0) list.splice(at, 1);
    else list.push(title);
    saveDictConfig();
    saveCollapsedDictionaries();
  }

  function deleteAudioSource(url: string) {
    if (!anki) return;
    anki.audioSources = anki.audioSources.filter((s) => s.url !== url);
    saveAnki();
  }

  async function changeLocalAudioType(type: string) {
    const config = anki!;
    const index = config.audioSources.findIndex((source) => source.url === localAudioUrl || source.url === localAudioAddonUrl);
    const isEnabled = config.audioSources[index]?.isEnabled ?? true;
    config.audioSources = config.audioSources.filter((source) => source.url !== localAudioUrl && source.url !== localAudioAddonUrl);
    config.enableLocalAudio = type !== "none";
    if (config.enableLocalAudio) {
      config.audioSources.splice(Math.max(index, 0), 0, {
        name: "Local",
        url: type === "anki" ? localAudioAddonUrl : localAudioUrl,
        isEnabled,
      });
    }
    await saveAnki();
  }

  async function selectLocalAudio() {
    if (!anki) return;
    const result = await open({
      filters: [{ name: "Audio Database", extensions: ["db"] }],
    });
    if (!result) return;
    anki.localAudioPath = result;
    await changeLocalAudioType("database");
  }

  async function removeLocalAudio() {
    if (!anki) return;
    anki.localAudioPath = null;
    anki.enableLocalAudio = false;
    anki.audioSources = anki.audioSources.filter((s) => s.url !== localAudioUrl);
    await saveAnki();
  }

  const audioSort = new Sortable({ list: () => anki?.audioSources ?? [], commit: saveAnki });

  $effect(() => {
    const timer = setInterval(async () => {
      if (tab !== "anki" || !anki || anki.disabled || ankiReachable) return;
      if (await invoke<boolean>("anki_ping")) await refreshAnkiData();
    }, 15000);
    return () => clearInterval(timer);
  });

  async function importZip() {
    const paths = await open({
      multiple: true,
      filters: [{ name: "Yomitan Dictionary", extensions: ["zip"] }],
    });
    if (!paths) return;
    importing = true;
    error = "";
    try {
      const summary = await invoke<ImportSummary>("import_dictionaries", { paths });
      await refresh();
      if (summary.failed.length) error = summary.failed.join("\n");
    } catch (err) {
      error = String(err);
    } finally {
      importing = false;
    }
  }

  async function setEnabled(dict: DictionaryInfo, enabled: boolean) {
    dict.isEnabled = enabled;
    await invoke("set_dictionary_enabled", {
      kind: selected,
      fileName: dict.fileName,
      enabled,
    });
    await refresh();
  }

  async function deleteDictionary(dict: DictionaryInfo) {
    if (!(await ask(`Delete "${dict.title}"?`, { kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" }))) return;
    await invoke("delete_dictionary", { kind: selected, fileName: dict.fileName });
    await refresh();
  }

  async function commitDictOrder() {
    await invoke("reorder_dictionaries", {
      kind: selected,
      fileNames: dictionaries[selected].map((d) => d.fileName),
    });
    await refresh();
  }

  const dictSort = new Sortable({ list: () => dictionaries[selected], commit: commitDictOrder });

  refresh();
  loadAnki();
</script>

{#snippet cssMenu(Icon: typeof BookA, label: string, items: string[], snippet: (item: string) => string)}
  <div class="dropdown" use:dropdownFlip>
    <div tabindex="0" role="button" class="btn btn-ghost btn-sm gap-1.5 font-normal">
      <Icon class="size-4" />
      {label}
      <ChevronDown class="size-3.5 text-base-content/50" />
    </div>
    <ul class="dropdown-content menu z-30 my-1 max-h-72 w-56 flex-nowrap overflow-x-hidden overflow-y-auto overscroll-contain rounded-box border border-base-300 bg-base-100 p-1 shadow-lg">
      {#each items as item (item)}
        <li><button class="block truncate" onclick={() => insertCSS(snippet(item))}>{item}</button></li>
      {/each}
    </ul>
  </div>
{/snippet}

<svelte:window
  onkeydowncapture={onBindKeydown}
  onmousedowncapture={onBindMouse}
  oncontextmenucapture={(e) => bindingKey && e.preventDefault()}
/>

<div class="flex h-full flex-col bg-base-100">
  <PageHeader title={tabLabel} />

  <main class="min-h-0 flex-1 overflow-y-auto">
    <div class="mx-auto flex max-w-2xl flex-col gap-6 p-6">
      {#if tab === "appearance"}
        <Appearance />
      {:else if tab === "about"}
        <About />
      {:else if tab === "dictionaries"}
        <SharingSettings onImport={refresh} />
        {#if updatableTitles.length > 0}
          <SettingsSection title="Updates">
            <SettingToggle
              label="Update Automatically"
              bind:checked={dictConfig.autoUpdateDictionaries}
              onchange={saveDictConfig}
            />
            {#if dictConfig.autoUpdateDictionaries}
              <SettingRow label="Interval">
                <select
                  class="select select-sm w-36"
                  bind:value={dictConfig.dictionaryUpdateInterval}
                  onchange={saveDictConfig}
                >
                  {#each dictionaryUpdateIntervals as interval (interval)}
                    <option>{interval}</option>
                  {/each}
                </select>
              </SettingRow>
            {/if}
            <SettingRow label="Last Update">
              <span class="text-sm text-base-content/60">{lastUpdate}</span>
            </SettingRow>
            <button
              class="btn btn-sm"
              onclick={() => updateDialog?.showModal()}
              disabled={dictionaryUpdate.running}
            >
              {#if dictionaryUpdate.running}
                <span class="loading loading-xs loading-spinner"></span>
                {dictionaryUpdate.status}
              {:else}
                Update
              {/if}
            </button>
          </SettingsSection>
        {/if}

        <div class="card border border-base-300 bg-base-100">
          <div class="card-body gap-4">
            <div class="flex items-center justify-between">
              <h2 class="card-title text-base">Dictionaries</h2>
              <button
                class="btn btn-neutral btn-sm btn-square"
                onclick={importZip}
                disabled={importing}
              >
                <Plus class="size-4" />
              </button>
            </div>

            {#if dictionaries.kanji.length > 0}
              <button
                class="btn btn-outline btn-sm self-start"
                disabled={downloadingStrokeOrderFont || hasStrokeOrderFont}
                onclick={downloadStrokeOrderFont}
              >
                {#if downloadingStrokeOrderFont}
                  <span class="loading loading-xs loading-spinner"></span>
                  Downloading Stroke Order Font
                {:else}
                  Download Stroke Order Font
                {/if}
              </button>
            {/if}

            <div role="tablist" class="tabs tabs-border">
              {#each Object.keys(labels) as key (key)}
                <button
                  role="tab"
                  class="tab {selected === key ? 'tab-active text-primary' : ''}"
                  onclick={() => (selected = key as DictionaryType)}
                >
                  {labels[key as DictionaryType]}
                </button>
              {/each}
            </div>

            {#if visible.length === 0}
              <div class="h-10 rounded-box border border-base-300"></div>
            {:else}
              <ul class="list rounded-box border border-base-300" use:dictSort.container>
                {#each visible as dict, index (dict.fileName)}
                  <li
                    class="list-row items-center {dictSort.index === index ? 'opacity-50' : ''}"
                  >
                    <input
                      type="checkbox"
                      class="toggle toggle-primary toggle-sm"
                      checked={dict.isEnabled}
                      onchange={(e) => setEnabled(dict, e.currentTarget.checked)}
                    />
                    <div class="min-w-0">
                      <p class="truncate text-sm font-medium">{dict.title}</p>
                      <p class="truncate text-xs text-base-content/60">
                        {dict.revision || dict.fileName}
                      </p>
                    </div>
                    <button
                      class="btn btn-ghost btn-sm btn-square text-base-content/60 hover:text-error"
                      onclick={() => deleteDictionary(dict)}
                    >
                      <Trash2 class="size-4" />
                    </button>
                    <span
                      role="button"
                      tabindex="-1"
                      class="cursor-grab touch-none text-base-content/40"
                      onpointerdown={(e) => dictSort.start(index, e)}
                    >
                      <Menu class="size-4" />
                    </span>
                  </li>
                {/each}
              </ul>
            {/if}

            {#if error}
              <div role="alert" class="alert alert-error">
                <CircleAlert class="size-4" />
                <pre class="select-text whitespace-pre-wrap font-sans text-sm">{error}</pre>
              </div>
            {/if}
          </div>
        </div>

        <SettingsSection title="Lookup">
          <SettingToggle
            label="Scan Non-Japanese Text"
            bind:checked={dictConfig.scanNonJapaneseText}
            onchange={saveDictConfig}
          />
          <SettingStepper
            label="Max Results"
            bind:value={dictConfig.maxResults}
            min={1}
            max={50}
            onchange={saveDictConfig}
          />
          <SettingStepper
            label="Scan Length"
            bind:value={dictConfig.scanLength}
            min={1}
            max={64}
            onchange={saveDictConfig}
          />
          <SettingRow label="Frequency Sorting">
            <select
              class="select select-sm w-36"
              bind:value={dictConfig.frequencySortOrder}
              onchange={changeFrequencySortOrder}
            >
              {#each ["Auto", "Ascending", "Descending", "Disabled"] as order (order)}
                <option>{order}</option>
              {/each}
            </select>
          </SettingRow>
          {#if usesFrequencyDictionary && enabledFrequencyDictionaries.length > 0}
            <SettingRow label="Frequency Dictionary">
              <select
                class="select select-sm max-w-64"
                bind:value={dictConfig.frequencySortDictionary}
                onchange={saveDictConfig}
              >
                {#each enabledFrequencyDictionaries as dict (dict.title)}
                  <option value={dict.title}>{dict.title}</option>
                {/each}
              </select>
            </SettingRow>
          {/if}
        </SettingsSection>

        <SettingsSection title="Search Text">
          <SettingStepper
            label="Text Size"
            bind:value={dictConfig.searchTextSize}
            min={12}
            max={48}
            onchange={saveDictConfig}
          />
        </SettingsSection>

        <SettingsSection title="Collapse Dictionaries">
          <SettingRow label="Mode">
            <select
              class="select select-sm w-36"
              bind:value={dictConfig.collapseMode}
              onchange={saveDictConfig}
            >
              {#each collapseModes as mode (mode)}
                <option>{mode}</option>
              {/each}
            </select>
          </SettingRow>
          {#if dictConfig.collapseMode !== "Expand All"}
            <SettingToggle
              label="Expand First Dictionary"
              bind:checked={dictConfig.expandFirstDictionary}
              onchange={saveDictConfig}
            />
          {/if}
          {#if dictConfig.collapseMode === "Custom"}
            <button
              class="btn btn-ghost btn-sm -mx-2 justify-between px-2 text-sm font-normal"
              onclick={() => collapsedDialog?.showModal()}
            >
              Configure
              <ChevronRight class="size-4 text-base-content/60" />
            </button>
          {/if}
        </SettingsSection>

        <SettingsSection title="Behaviour">
          <SettingToggle
            label="Two-Column Layout"
            bind:checked={dictConfig.twoColumnLayout}
            onchange={saveDictConfig}
          />
          <SettingToggle
            label="Compact Glossaries"
            bind:checked={dictConfig.compactGlossaries}
            onchange={saveDictConfig}
          />
          <SettingToggle
            label="Show Expression Tags"
            bind:checked={dictConfig.showExpressionTags}
            onchange={saveDictConfig}
          />
          <SettingToggle
            label="Harmonic Frequency"
            bind:checked={dictConfig.harmonicFrequency}
            onchange={saveDictConfig}
          />
          <SettingToggle
            label="Deduplicate Pitch Accents"
            bind:checked={dictConfig.deduplicatePitchAccents}
            onchange={saveDictConfig}
          />
          <SettingToggle
            label="Compact Pitch Accents"
            bind:checked={dictConfig.compactPitchAccents}
            onchange={saveDictConfig}
          />
        </SettingsSection>

        <div class="card border border-base-300 bg-base-100">
          <div class="card-body gap-4">
            <div class="flex items-center justify-between">
              <h2 class="card-title text-base">Custom CSS</h2>
              <button
                class="btn btn-ghost btn-sm text-error"
                onclick={() => {
                  dictConfig.customCSS = "";
                  saveDictConfig();
                }}
              >
                Reset
              </button>
            </div>
            <div class="rounded-field border border-base-300">
              <div class="flex gap-1 border-b border-base-300 p-1">
                {@render cssMenu(ALargeSmall, "Font", cssFonts, (font) => `font-family: "${font}" !important;`)}
                {#if dictionaries.term.length > 0}
                  {@render cssMenu(BookA, "Selector", dictionaries.term.map((dict) => dict.title), (title) => `[data-dictionary="${title}"] {\n    \n}\n`)}
                {/if}
              </div>
              <textarea
                bind:this={cssEditor}
                class="textarea h-40 w-full border-0 bg-transparent font-mono text-xs"
                spellcheck="false"
                bind:value={dictConfig.customCSS}
                onchange={saveDictConfig}
              ></textarea>
            </div>
          </div>
        </div>
      {:else if tab === "audio" && anki}
        <SettingsSection title="Sources">
          <ul class="list rounded-box border border-base-300" use:audioSort.container>
            {#each anki.audioSources as source, index (source.url)}
              <li
                class="list-row items-center {audioSort.index === index ? 'opacity-50' : ''}"
              >
                <input
                  type="checkbox"
                  class="toggle toggle-primary toggle-sm"
                  bind:checked={source.isEnabled}
                  onchange={saveAnki}
                />
                <div class="min-w-0">
                  <p class="truncate text-sm font-medium">{source.url === localAudioAddonUrl ? "Local" : source.name}</p>
                  {#if source.url !== localAudioUrl}
                    <p class="truncate text-xs text-base-content/60">{source.url}</p>
                  {/if}
                </div>
                <button
                  class="btn btn-ghost btn-sm btn-square text-base-content/60 hover:text-error"
                  onclick={() => deleteAudioSource(source.url)}
                  disabled={source.url === localAudioUrl || source.url === localAudioAddonUrl}
                >
                  <Trash2 class="size-4" />
                </button>
                <span
                  role="button"
                  tabindex="-1"
                  class="cursor-grab touch-none text-base-content/40"
                  onpointerdown={(e) => audioSort.start(index, e)}
                >
                  <Menu class="size-4" />
                </span>
              </li>
            {/each}
          </ul>

          <div class="flex items-center gap-2">
            <input class="input w-40" bind:value={audioNameInput} placeholder="Name" />
            <input class="input flex-1" bind:value={audioUrlInput} placeholder="URL" />
            <button
              class="btn btn-outline btn-square"
              onclick={addAudioSource}
              disabled={!audioNameInput.trim() || !audioUrlInput.trim()}
            >
              <Plus class="size-4" />
            </button>
          </div>

          <div class="flex flex-col gap-2 border-t border-base-300 pt-3">
            <SettingRow label="Local Audio">
              <select
                class="select select-sm w-40"
                value={localAudioType}
                onchange={(e) => changeLocalAudioType(e.currentTarget.value)}
              >
                <option value="none">None</option>
                <option value="anki">Anki Addon</option>
                <option value="database">Database</option>
              </select>
            </SettingRow>
            {#if localAudioType === "database"}
              <div class="flex items-center gap-2">
                <button class="btn btn-outline btn-sm" onclick={selectLocalAudio}>Select Database</button>
                {#if anki.localAudioPath}
                  <button class="btn btn-outline btn-error btn-sm" onclick={removeLocalAudio}>
                    Remove {localAudioName}
                  </button>
                {/if}
              </div>
              {#if anki.localAudioPath}
                <p class="truncate text-xs text-base-content/60">{anki.localAudioPath}</p>
              {/if}
            {/if}
          </div>
        </SettingsSection>

        <div class="card border border-base-300 bg-base-100">
          <div class="card-body">
            <SettingToggle
              label="Auto-play on Lookup"
              bind:checked={anki.audioEnableAutoplay}
              onchange={saveAnki}
            />
          </div>
        </div>
      {:else if tab === "syncing"}
        <SettingsSection title="Syncing">
          <SettingToggle
            label="Enable"
            bind:checked={syncConfig.enableSync}
            onchange={changeEnableSync}
          />
          <SettingRow label="Provider">
            <select class="select select-sm w-36" bind:value={syncConfig.syncProvider} onchange={saveSyncConfig}>
              {#each syncProviders as provider (provider.id)}
                <option value={provider.id}>{provider.label}</option>
              {/each}
            </select>
          </SettingRow>
          <p class="text-xs text-base-content/60">Sync your library between Hoshi Reader devices.</p>
        </SettingsSection>

        {#if syncConfig.enableSync}
          <div class="card border border-base-300 bg-base-100">
            <div class="card-body gap-3">
              <SettingRow label="Status">
                <span class="text-sm text-base-content/60">
                  {syncAuthenticated ? "Connected" : "Not connected"}
                </span>
              </SettingRow>
              {#if syncAuthenticated}
                <div class="flex items-center gap-2">
                  <button class="btn btn-outline btn-error btn-sm" onclick={clearSyncCache}>
                    Clear Cache
                  </button>
                  <button class="btn btn-outline btn-error btn-sm" onclick={signOutSync}>
                    Sign out
                  </button>
                </div>
              {:else}
                <div class="flex items-center gap-2">
                  <button
                    class="btn btn-outline btn-sm"
                    onclick={connectGoogleDrive}
                    disabled={syncConnecting}
                  >
                    Connect Google Drive
                  </button>
                  {#if syncConnecting}
                    <span class="loading loading-spinner loading-xs"></span>
                  {/if}
                </div>
              {/if}
            </div>
          </div>

          {#if syncAuthenticated}
            <div class="card border border-base-300 bg-base-100">
              <div class="card-body gap-3">
                {#if driveStatus.lastSync !== null}
                  <SettingRow label="Last Sync">
                    <span class="text-sm text-base-content/60">
                      {lastSyncFormat.format(new Date(driveStatus.lastSync))}
                    </span>
                  </SettingRow>
                {/if}
                <button
                  class="btn btn-ghost btn-sm -mx-2 h-auto min-h-8 flex-col items-stretch gap-1 px-2 py-1.5 text-sm font-normal"
                  onclick={() => transfersDialog?.showModal()}
                >
                  <span class="flex items-center justify-between">
                    Queue
                    <span class="flex items-center gap-1">
                      {#if driveStatus.progress}
                        <span class="tabular-nums text-base-content/60">
                          {driveStatus.progress.done} / {driveStatus.progress.total}
                        </span>
                      {:else if failedBooks > 0}
                        <span class="text-error">{failedBooks} failed</span>
                      {:else if driveStatus.queue.length > 0}
                        <span class="tabular-nums text-base-content/60">{driveStatus.queue.length}</span>
                      {:else}
                        <span class="text-base-content/60">Empty</span>
                      {/if}
                      <ChevronRight class="size-4 text-base-content/60" />
                    </span>
                  </span>
                  {#if driveStatus.progress}
                    <progress
                      class="progress w-full"
                      value={driveStatus.progress.done}
                      max={driveStatus.progress.total}
                    ></progress>
                  {/if}
                </button>
                {#if driveStatus.errorMessage}
                  <p class="select-text whitespace-pre-line text-xs text-error">
                    {driveStatus.errorMessage}
                  </p>
                {/if}
                <button
                  class="btn btn-outline btn-sm self-start"
                  disabled={driveStatus.isSyncing}
                  onclick={() => invoke("gdrive_sync_now")}
                >
                  Sync Now
                </button>
              </div>
            </div>
          {/if}
        {/if}
      {:else if tab === "sasayaki"}
        <SettingsSection title="Sasayaki (Audiobooks)">
          <SettingToggle
            label="Enable"
            bind:checked={sasayakiConfig.enableSasayaki}
            onchange={saveSasayakiConfig}
          />
        </SettingsSection>

        {#if sasayakiConfig.enableSasayaki}
          <SasayakiSettings />
        {/if}
      {:else if tab === "backup"}
        {#each ["Books", "Dictionaries"] as const as folder (folder)}
          <SettingsSection title={folder}>
            <div class="flex gap-2">
              <button
                class="btn btn-outline btn-sm"
                onclick={() => backupStorage(folder)}
                disabled={backupBusy !== null}
              >
                Backup
              </button>
              <button
                class="btn btn-outline btn-sm"
                onclick={() => restoreStorage(folder)}
                disabled={backupBusy !== null}
              >
                Restore
              </button>
            </div>
            {#if folder === "Dictionaries"}
              <p class="text-xs text-base-content/60">Restoring will overwrite the current collection.</p>
            {/if}
          </SettingsSection>
        {/each}

        {#if backupError}
          <div role="alert" class="alert alert-error">
            <CircleAlert class="size-4" />
            <pre class="select-text whitespace-pre-wrap font-sans text-sm">{backupError}</pre>
          </div>
        {/if}
      {:else if tab === "input"}
        <SettingsSection title="Scanning">
          <SettingRow label="Scan Modifier">
            <button
              class="btn btn-sm min-w-24 {bindingKey === 'scanModifier' ? 'btn-active' : ''}"
              onclick={(e) => startBinding("scanModifier", e.currentTarget)}
              onblur={() => (bindingKey = null)}
            >
              {bindingKey === "scanModifier" ? "…" : hotkeyLabel(hotkeyConfig.scanModifier)}
            </button>
          </SettingRow>
          <SettingRow label="Scan on Click">
            <select
              class="select select-sm w-36"
              bind:value={hotkeyConfig.clickLookup}
              onchange={saveHotkeyConfig}
            >
              <option value="off">Off</option>
              <option value="left">Left Click</option>
              <option value="right">Right Click</option>
            </select>
          </SettingRow>
        </SettingsSection>
        {#each ["Sasayaki", "Reader"] as section}
          <SettingsSection title={section}>
            {#if section === "Reader"}
              <SettingToggle
                label="Disable Mouse Wheel"
                bind:checked={hotkeyConfig.disableReaderWheel}
                onchange={saveHotkeyConfig}
              />
            {/if}
            {#each readerHotkeys.filter((binding) => binding.section === section) as binding (binding.key)}
              <SettingRow label={binding.label}>
                <button
                  class="btn btn-sm min-w-24 {bindingKey === binding.key ? 'btn-active' : ''}"
                  onclick={(e) => startBinding(binding.key, e.currentTarget)}
                  onblur={() => (bindingKey = null)}
                >
                  {bindingKey === binding.key ? "…" : hotkeyLabel(hotkeyConfig[binding.key])}
                </button>
              </SettingRow>
            {/each}
          </SettingsSection>
        {/each}
        {#if bindingError}
          <p class="select-text text-sm text-error">{bindingError}</p>
        {/if}
      {:else if anki}
        <SettingsSection title="Anki">
          <SettingRow label="Enable">
            <input
              type="checkbox"
              class="toggle toggle-primary toggle-sm"
              checked={!anki.disabled}
              onchange={(e) => {
                anki!.disabled = !e.currentTarget.checked;
                changeAnkiDisabled();
              }}
            />
          </SettingRow>
        </SettingsSection>

        {#if !anki.disabled}
          <div class="card border border-base-300 bg-base-100">
            <div class="card-body gap-3">
              <div class="flex items-center justify-between gap-3">
                <h2 class="card-title text-base">AnkiConnect</h2>
                <span class="text-xs text-base-content/60">
                  {ankiReachable ? "Connected" : "Not connected"}
                </span>
              </div>
              <div class="grid gap-1.5">
                <label class="text-sm font-medium" for="anki-address">Address</label>
                <div class="join w-full">
                  <input
                    id="anki-address"
                    type="text"
                    class="input join-item w-full"
                    bind:value={ankiUrl}
                    onchange={saveAnki}
                  />
                  <button class="btn join-item" onclick={connectAnki} disabled={connecting}>
                    Connect
                  </button>
                </div>
              </div>

              <div class="grid gap-1.5">
                <label class="text-sm font-medium" for="anki-key">API Key (optional)</label>
                <input
                  id="anki-key"
                  type="password"
                  class="input w-full"
                  bind:value={ankiApiKey}
                  onchange={saveAnki}
                />
              </div>

              {#if ankiError}
                <p class="select-text text-sm text-error">{ankiError}</p>
              {/if}
            </div>
          </div>

          {#if ankiReachable && anki.availableDecks.length}
            <div class="card border border-base-300 bg-base-100">
              <div class="card-body gap-4">
                <div class="flex items-center justify-between">
                  <h2 class="card-title text-base">Formats</h2>
                  <button
                    class="btn btn-outline btn-sm"
                    onclick={addCardFormat}
                    disabled={anki.cardFormats.length >= maxFormats}
                  >
                    <Plus class="size-4" />
                    Add Format
                  </button>
                </div>

                <div role="tablist" class="tabs tabs-border">
                  {#each anki.cardFormats as format (format.id)}
                    <button
                      role="tab"
                      class="tab {activeFormat?.id === format.id ? 'tab-active text-primary' : ''}"
                      onclick={() => (selectedFormatId = format.id)}
                    >
                      {format.name || "Format"}
                    </button>
                  {/each}
                </div>

                {#if activeFormat}
                  {@const format = activeFormat}
                  <div class="flex items-center gap-2">
                    <input
                      type="text"
                      class="input flex-1"
                      bind:value={format.name}
                      onchange={saveAnki}
                      placeholder="Name"
                    />
                    <button
                      class="btn btn-ghost btn-square text-base-content/60 hover:text-error"
                      onclick={() => deleteCardFormat(format)}
                      disabled={anki.cardFormats.length <= 1}
                    >
                      <Trash2 class="size-4" />
                    </button>
                  </div>

                  <div class="grid grid-cols-2 gap-3">
                    <div class="grid gap-1.5">
                      <span class="text-sm font-medium">Deck</span>
                      <select
                        class="select w-full"
                        value={format.selectedDeck ?? ""}
                        onchange={(e) => {
                          format.selectedDeck = e.currentTarget.value;
                          saveAnki();
                        }}
                      >
                        {#each anki.availableDecks as deck (deck)}
                          <option value={deck}>{deck}</option>
                        {/each}
                      </select>
                    </div>

                    <div class="grid gap-1.5">
                      <span class="text-sm font-medium">Model</span>
                      <select
                        class="select w-full"
                        value={format.selectedNoteType ?? ""}
                        onchange={(e) => {
                          format.selectedNoteType = e.currentTarget.value;
                          modelChanged(format);
                        }}
                      >
                        {#each anki.availableNoteTypes as noteType (noteType.name)}
                          <option value={noteType.name}>{noteType.name}</option>
                        {/each}
                      </select>
                    </div>
                  </div>

                  <SettingRow label="Icon">
                    <div class="join">
                      {#each formatIcons as icon (icon)}
                        {@const FormatIcon = icon.startsWith("plus-square")
                          ? SquarePlus
                          : icon.startsWith("plus-circle")
                            ? CirclePlus
                            : DiamondPlus}
                        <button
                          class="btn join-item btn-sm btn-square {format.icon === icon
                            ? 'btn-active'
                            : ''}"
                          onclick={() => {
                            format.icon = icon;
                            saveAnki();
                          }}
                        >
                          <FormatIcon class={icon.endsWith("-small") ? "size-3" : "size-4"} />
                        </button>
                      {/each}
                    </div>
                  </SettingRow>

                  <div class="grid gap-2">
                    <h4 class="text-sm font-medium">Fields</h4>
                    {#each noteTypeFields(format) as field (format.selectedNoteType + field)}
                      <div class="grid grid-cols-[8rem_1fr] items-center gap-3">
                        <span class="truncate text-sm text-base-content/60">{field}</span>
                        <div class="join w-full">
                          <input
                            class="input join-item w-full"
                            value={format.fieldMappings[field] ?? ""}
                            onchange={(e) => setMapping(format, field, e.currentTarget.value)}
                            placeholder="None"
                          />
                          <div class="dropdown dropdown-end" use:dropdownFlip>
                            <div
                              tabindex="0"
                              role="button"
                              class="btn join-item btn-square"
                            >
                              <ChevronDown class="size-4" />
                            </div>
                            <ul
                              class="dropdown-content menu z-30 my-1 max-h-72 w-56 flex-nowrap overflow-y-auto overscroll-contain rounded-box border border-base-300 bg-base-100 p-1 shadow-lg"
                            >
                              <li>
                                <button onclick={(e) => pickHandlebar(format, field, "", e)}>
                                  -
                                </button>
                              </li>
                              <div class="divider my-0"></div>
                              {#each availableHandlebars as handlebar (handlebar)}
                                <li>
                                  <button
                                    class="font-mono text-xs"
                                    onclick={(e) => pickHandlebar(format, field, handlebar, e)}
                                  >
                                    {handlebar}
                                  </button>
                                </li>
                              {/each}
                            </ul>
                          </div>
                        </div>
                      </div>
                    {/each}
                    <div class="grid grid-cols-[8rem_1fr] items-center gap-3">
                      <span class="truncate text-sm text-base-content/60">Tags</span>
                      <input
                        type="text"
                        class="input w-full"
                        bind:value={format.tags}
                        onchange={saveAnki}
                        placeholder="None"
                      />
                    </div>
                  </div>
                {/if}
              </div>
            </div>

            <div class="card border border-base-300 bg-base-100">
              <div class="card-body gap-4">
                <SettingToggle
                  label="Allow Duplicates"
                  bind:checked={anki.allowDupes}
                  onchange={saveAnki}
                />
                <SettingToggle
                  label="Disable Show Notes Button"
                  bind:checked={anki.disableShowNotes}
                  onchange={saveAnki}
                />
                <SettingRow label="Duplicate Scope">
                  <select
                    class="select select-sm w-36"
                    bind:value={anki.duplicateScope}
                    onchange={saveAnki}
                  >
                    {#each Object.entries(duplicateScopeLabels) as [value, label] (value)}
                      <option {value}>{label}</option>
                    {/each}
                  </select>
                </SettingRow>
                <SettingToggle
                  label="Compact Glossaries"
                  bind:checked={anki.compactGlossaries}
                  onchange={saveAnki}
                />
                <SettingToggle
                  label="Check All Models"
                  bind:checked={anki.checkAllModels}
                  onchange={saveAnki}
                />
                <SettingToggle
                  label="Force Sync on adding card"
                  bind:checked={anki.forceSync}
                  onchange={saveAnki}
                />
              </div>
            </div>

            <SettingsSection title="Advanced">
              <SettingToggle
                label="Show All Handlebars"
                bind:checked={anki.showAllHandlebars}
                onchange={saveAnki}
              />

              <div class="grid gap-1.5">
                <span class="text-sm font-medium">{"{selected-glossary}"} Fallback</span>
                <div class="join w-full">
                  <input
                    class="input join-item w-full"
                    bind:value={anki.selectedGlossaryFallback}
                    onchange={saveAnki}
                    placeholder="None"
                  />
                  <div class="dropdown dropdown-end" use:dropdownFlip>
                    <div
                      tabindex="0"
                      role="button"
                      class="btn join-item btn-square"
                    >
                      <ChevronDown class="size-4" />
                    </div>
                    <ul
                      class="dropdown-content menu z-30 my-1 w-72 rounded-box border border-base-300 bg-base-100 p-1 shadow-lg"
                    >
                      <li>
                        <button onclick={(e) => pickFallback("", e)}>None</button>
                      </li>
                      <div class="divider my-0"></div>
                      {#each fallbackOptions as option (option)}
                        <li>
                          <button
                            class="font-mono text-xs"
                            onclick={(e) => pickFallback(option, e)}
                          >
                            {option}
                          </button>
                        </li>
                      {/each}
                    </ul>
                  </div>
                </div>
              </div>

              <button
                class="btn btn-ghost btn-sm -mx-2 justify-between px-2 text-sm font-normal"
                onclick={() => categorizeDialog?.showModal()}
              >
                Categorize Dictionaries
                <ChevronRight class="size-4 text-base-content/60" />
              </button>
            </SettingsSection>
          {/if}
        {/if}
      {/if}
    </div>
  </main>

  <dialog class="modal" bind:this={transfersDialog}>
    <div class="modal-box flex max-h-[80vh] flex-col gap-4">
      <h3 class="text-base font-semibold">Queue</h3>

      <div class="flex min-h-0 flex-col">
        {#if driveStatus.queue.length === 0}
          <div
            class="flex h-12 items-center justify-center rounded-box border border-base-300 text-sm text-base-content/60"
          >
            All Books Synced
          </div>
        {:else}
          <ul class="list min-h-0 overflow-y-auto rounded-box border border-base-300">
            {#each driveStatus.queue as item (item.key)}
              <li class="list-row items-center">
                <div class="list-col-grow min-w-0">
                  <div class="flex items-center gap-1.5 text-sm">
                    {#if item.direction === "upload"}
                      <ArrowUp class="size-3.5 shrink-0 text-base-content/60" />
                    {:else if item.direction === "download"}
                      <ArrowDown class="size-3.5 shrink-0 text-base-content/60" />
                    {:else if item.direction === "both"}
                      <ArrowUpDown class="size-3.5 shrink-0 text-base-content/60" />
                    {/if}
                    <span class="truncate">{item.title}</span>
                  </div>
                  {#if item.error}
                    <div class="select-text whitespace-pre-line text-xs text-error">{item.error}</div>
                  {/if}
                </div>
                {#if driveStatus.progress?.current.includes(item.key)}
                  <span class="loading loading-spinner loading-xs"></span>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <div class="modal-action mt-0">
        <form method="dialog">
          <button class="btn btn-sm">Done</button>
        </form>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>

  <dialog class="modal" bind:this={updateDialog}>
    <div class="modal-box">
      <h3 class="mb-4 text-base font-semibold">Update Dictionaries</h3>
      <p class="whitespace-pre-wrap text-sm">{updateMessage}</p>
      <div class="modal-action">
        <form method="dialog" class="flex gap-2">
          <button class="btn btn-sm">Cancel</button>
          <button class="btn btn-neutral btn-sm" onclick={runUpdate}>Update</button>
        </form>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>

  <dialog class="modal" bind:this={collapsedDialog}>
    <div class="modal-box">
      <h3 class="mb-4 text-base font-semibold">Collapse Dictionaries</h3>
      <ul class="list rounded-box border border-base-300">
        {#each dictionaries.term as dict (dict.fileName)}
          <li>
            <button
              class="list-row w-full items-center text-left"
              onclick={() => toggleCollapsedDictionary(dict.title)}
            >
              {#if dictConfig.collapsedDictionaries.includes(dict.title)}
                <ChevronRight class="size-4 text-base-content/60" />
              {:else}
                <ChevronDown class="size-4" />
              {/if}
              <span class="truncate text-sm">{dict.title}</span>
            </button>
          </li>
        {/each}
      </ul>
      <div class="modal-action">
        <form method="dialog">
          <button class="btn btn-sm">Close</button>
        </form>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>

  <dialog class="modal" bind:this={categorizeDialog}>
    <div class="modal-box">
      <h3 class="mb-4 text-base font-semibold">Categorize Dictionaries</h3>
      <ul class="list rounded-box border border-base-300">
        {#each dictionaries.term as dict (dict.fileName)}
          <li class="list-row">
            <span class="list-col-grow truncate text-sm text-base-content/60">{dict.title}</span>
            <div class="list-col-wrap join w-full">
              {#each dictionaryCategories as category (category)}
                <button
                  class="btn join-item btn-sm flex-1 px-0 {dict.category === category
                    ? 'btn-active'
                    : ''}"
                  onclick={() => setDictionaryCategory(dict, category)}
                >
                  {categoryLabels[category]}
                </button>
              {/each}
            </div>
          </li>
        {/each}
      </ul>
      <div class="modal-action">
        <form method="dialog">
          <button class="btn btn-sm">Close</button>
        </form>
      </div>
    </div>
    <form method="dialog" class="modal-backdrop">
      <button>close</button>
    </form>
  </dialog>
</div>
