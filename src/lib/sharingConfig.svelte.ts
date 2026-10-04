import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";
import { dictConfig } from "./dictConfig.svelte";
import type { KanjiResponse, LookupResponse } from "./types";

export type SharingSettings = {
  remoteAddress: string;
  remoteApiUrl: string;
  lookupSource: "local" | "remote";
  serverEnabled: boolean;
  serverPort: number;
};

export type RemoteDictionary = {
  id: string;
  title: string;
  revision?: string | null;
};

export type RemoteStatus = {
  name: string;
  version: string;
  dictionaryCount: number;
};

export const sharingConfig = $state<SharingSettings>({
  remoteAddress: "ws://127.0.0.1:8771/link",
  remoteApiUrl: "http://127.0.0.1:19633",
  lookupSource: "local",
  serverEnabled: false,
  serverPort: 8772,
});

let initialization: Promise<void> | null = null;
let errorDialog: Promise<void> | null = null;
let lastError = "";
let lastErrorAt = 0;

export function initializeSharing(): Promise<void> {
  if (!initialization) {
    initialization = invoke<SharingSettings>("sharing_get_settings")
      .then((settings) => {
        Object.assign(sharingConfig, settings);
      })
      .catch((error) => {
        initialization = null;
        throw error;
      });
  }
  return initialization;
}

export async function configureSharing(settings: SharingSettings): Promise<void> {
  const applied = await invoke<SharingSettings>("sharing_configure", { settings });
  Object.assign(sharingConfig, applied);
}

async function showLookupError(error: unknown) {
  const detail = String(error);
  if (errorDialog || (detail === lastError && Date.now() - lastErrorAt < 30000)) return;
  lastError = detail;
  lastErrorAt = Date.now();
  errorDialog = message(detail, { title: "Dictionary Lookup Failed", kind: "error" })
    .then(() => {})
    .finally(() => {
      errorDialog = null;
    });
  await errorDialog;
}

export async function lookupDictionary(text: string): Promise<LookupResponse> {
  try {
    await initializeSharing();
    return await invoke<LookupResponse>(sharingConfig.lookupSource === "remote" ? "sharing_lookup" : "lookup", {
      text,
      maxResults: dictConfig.maxResults,
      scanLength: dictConfig.scanLength,
      frequencySortOrder: dictConfig.frequencySortOrder,
      frequencySortDictionary: dictConfig.frequencySortDictionary,
    });
  } catch (error) {
    await showLookupError(error);
    return { entries: [], styles: {} };
  }
}

export async function lookupKanji(character: string): Promise<KanjiResponse | null> {
  try {
    await initializeSharing();
    return await invoke<KanjiResponse | null>(sharingConfig.lookupSource === "remote" ? "sharing_kanji" : "lookup_kanji", { character });
  } catch (error) {
    await showLookupError(error);
    return null;
  }
}
