import { persisted } from "./persisted.svelte";

export type ShellConfig = {
  sidebarWidth: number;
  showAuthors: boolean;
  bookshelfCoverMode: "Show" | "Blur" | "Hide";
  bookshelfLayout: "Grid" | "List";
};

const defaults: ShellConfig = {
  sidebarWidth: 176,
  showAuthors: true,
  bookshelfCoverMode: "Show",
  bookshelfLayout: "Grid",
};

const store = persisted<ShellConfig>("shell.config", defaults);

export const shellConfig = store.config;
export const saveShellConfig = store.save;
