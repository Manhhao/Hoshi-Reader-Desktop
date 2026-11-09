import { persisted } from "./persisted.svelte";

export type ShellConfig = {
  sidebarWidth: number;
  showSeries: boolean;
  showAuthors: boolean;
  showCompleted: boolean;
  bookshelfCoverMode: "Show" | "Blur" | "Hide";
  bookshelfLayout: "Grid" | "List";
};

const defaults: ShellConfig = {
  sidebarWidth: 176,
  showSeries: false,
  showAuthors: true,
  showCompleted: true,
  bookshelfCoverMode: "Show",
  bookshelfLayout: "Grid",
};

const store = persisted<ShellConfig>("shell.config", defaults);

export const shellConfig = store.config;
export const saveShellConfig = store.save;
