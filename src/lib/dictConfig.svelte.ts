import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { persisted } from "./persisted.svelte";

export type FrequencySortOrder = "Auto" | "Ascending" | "Descending" | "Disabled";

export type CollapseMode = "Expand All" | "Collapse All" | "Custom";

export type DictionaryUpdateInterval = "Daily" | "Weekly" | "Monthly";

export const dictionaryUpdateIntervals: DictionaryUpdateInterval[] = ["Daily", "Weekly", "Monthly"];

const updateIntervalSeconds: Record<DictionaryUpdateInterval, number> = {
  Daily: 86400,
  Weekly: 604800,
  Monthly: 2592000,
};

export type DictConfig = {
  autoUpdateDictionaries: boolean;
  dictionaryUpdateInterval: DictionaryUpdateInterval;
  lastDictionaryUpdate: number | null;
  scanNonJapaneseText: boolean;
  maxResults: number;
  scanLength: number;
  frequencySortOrder: FrequencySortOrder;
  frequencySortDictionary: string;
  collapseMode: CollapseMode;
  expandFirstDictionary: boolean;
  collapsedDictionaries: string[];
  twoColumnLayout: boolean;
  compactGlossaries: boolean;
  showExpressionTags: boolean;
  harmonicFrequency: boolean;
  deduplicatePitchAccents: boolean;
  compactPitchAccents: boolean;
  customCSS: string;
  searchTextSize: number;
};

const defaults: DictConfig = {
  autoUpdateDictionaries: true,
  dictionaryUpdateInterval: "Weekly",
  lastDictionaryUpdate: null,
  scanNonJapaneseText: true,
  maxResults: 16,
  scanLength: 16,
  frequencySortOrder: "Auto",
  frequencySortDictionary: "",
  collapseMode: "Expand All",
  expandFirstDictionary: false,
  collapsedDictionaries: [],
  twoColumnLayout: false,
  compactGlossaries: true,
  showExpressionTags: false,
  harmonicFrequency: false,
  deduplicatePitchAccents: false,
  compactPitchAccents: true,
  customCSS: "",
  searchTextSize: 22,
};

const store = persisted<DictConfig>("dict.config", defaults);

export const dictConfig = store.config;
export const saveDictConfig = store.save;

export async function loadCollapsedDictionaries() {
  dictConfig.collapsedDictionaries = await invoke<string[]>("load_collapsed_dictionaries");
}

export function saveCollapsedDictionaries() {
  invoke("save_collapsed_dictionaries", {
    titles: $state.snapshot(dictConfig.collapsedDictionaries),
  });
}

export type DictionaryUpdateSummary = {
  updated: string[];
  failed: string[];
  renamed: [string, string][];
};

type UpdatableLists = Record<"term" | "frequency" | "pitch" | "kanji", { title: string; isUpdatable: boolean }[]>;

export const dictionaryUpdate = $state({ running: false, status: "" });

async function updatableDictionaryTitles(): Promise<string[]> {
  const lists = await invoke<UpdatableLists>("list_dictionaries");
  const titles: string[] = [];
  for (const kind of ["term", "frequency", "pitch", "kanji"] as const) {
    for (const dict of lists[kind]) {
      if (dict.isUpdatable && !titles.includes(dict.title)) titles.push(dict.title);
    }
  }
  return titles;
}

export async function updateDictionaries(lowRam = false): Promise<DictionaryUpdateSummary | null> {
  if (dictionaryUpdate.running) return null;
  dictionaryUpdate.running = true;
  dictionaryUpdate.status = "";
  let unlisten: (() => void) | null = null;
  try {
    const titles = await updatableDictionaryTitles();
    if (!titles.length) return null;
    unlisten = await listen<{ status: string }>("dictionary-update-progress", (event) => {
      dictionaryUpdate.status = event.payload.status;
    });
    const summary = await invoke<DictionaryUpdateSummary>("update_dictionaries", { lowRam });
    if (summary.failed.length < titles.length) {
      dictConfig.lastDictionaryUpdate = Date.now();
      saveDictConfig();
    }
    for (const [oldTitle, newTitle] of summary.renamed) {
      if (dictConfig.frequencySortDictionary === oldTitle) {
        dictConfig.frequencySortDictionary = newTitle;
        saveDictConfig();
      }
    }
    if (summary.renamed.length) await loadCollapsedDictionaries();
    return summary;
  } finally {
    unlisten?.();
    dictionaryUpdate.running = false;
    dictionaryUpdate.status = "";
  }
}

export async function autoUpdateDictionaries() {
  if (!dictConfig.autoUpdateDictionaries || dictionaryUpdate.running) return;
  const last = dictConfig.lastDictionaryUpdate;
  const elapsed = last === null ? Infinity : (Date.now() - last) / 1000;
  if (elapsed < updateIntervalSeconds[dictConfig.dictionaryUpdateInterval]) return;
  await updateDictionaries(true).catch(() => null);
}
