import { invoke } from "@tauri-apps/api/core";
import type { SortOption } from "./types";

export type SeriesSort = { option: SortOption; reversed: boolean };

export const seriesStore = $state<{
  names: Record<string, string>;
  sorts: Record<string, SeriesSort>;
  orders: Record<string, string[]>;
}>({ names: {}, sorts: {}, orders: {} });

export async function loadSeries() {
  const stored = await invoke<Partial<typeof seriesStore>>("load_series");
  seriesStore.names = stored.names ?? {};
  seriesStore.sorts = stored.sorts ?? {};
  seriesStore.orders = stored.orders ?? {};
}

export async function saveSeries() {
  await invoke("save_series", { store: $state.snapshot(seriesStore) });
}

export function seriesSort(key: string): SeriesSort {
  return seriesStore.sorts[key] ?? { option: "Title", reversed: false };
}

export function setSeriesSort(key: string, option: SortOption) {
  const current = seriesSort(key);
  seriesStore.sorts[key] =
    current.option === option && option !== "Custom"
      ? { option, reversed: !current.reversed }
      : { option, reversed: false };
  saveSeries();
}

export function setSeriesOrder(key: string, folders: string[]) {
  seriesStore.orders[key] = folders;
}

export function resetSeriesOrder(key: string) {
  delete seriesStore.orders[key];
  delete seriesStore.sorts[key];
  saveSeries();
}

export function renameSeries(key: string, name: string) {
  if (name) seriesStore.names[key] = name;
  else delete seriesStore.names[key];
  saveSeries();
}
