import { invoke } from "@tauri-apps/api/core";
import { persisted } from "./persisted.svelte";
import { statsConfig } from "./statsConfig.svelte";

export const syncProviders = [{ id: "gdrive", label: "Google Drive" }];

const store = persisted("sync.config", { enableSync: false, syncProvider: "gdrive" });

export const syncConfig = store.config;
export const saveSyncConfig = store.save;

export const network = $state({ online: navigator.onLine });

export function configureSync() {
  return invoke("sync_configure", {
    settings: {
      enableSync: syncConfig.enableSync,
      statisticsResetTime: Math.round(statsConfig.statisticsResetTime * 60),
      online: network.online,
    },
  });
}
