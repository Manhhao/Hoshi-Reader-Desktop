import { persisted } from "./persisted.svelte";

export type ShellConfig = {
  sidebarWidth: number;
  showSeries: boolean;
  showAuthors: boolean;
  showCompleted: boolean;
  hiddenShelves: string[];
  bookshelfCoverMode: "Show" | "Blur" | "Hide";
  bookshelfLayout: "Grid" | "List";
  bookshelfShowProgressBar: boolean;
  bookshelfShowPercentage: boolean;
  bookshelfShowCharacters: boolean;
  bookshelfShowTitle: boolean;
  bookshelfShowAuthor: boolean;
};

const defaults: ShellConfig = {
  sidebarWidth: 176,
  showSeries: false,
  showAuthors: true,
  showCompleted: true,
  hiddenShelves: [],
  bookshelfCoverMode: "Show",
  bookshelfLayout: "Grid",
  bookshelfShowProgressBar: true,
  bookshelfShowPercentage: true,
  bookshelfShowCharacters: false,
  bookshelfShowTitle: true,
  bookshelfShowAuthor: false,
};

const store = persisted<ShellConfig>("shell.config", defaults);

export const shellConfig = store.config;
export const saveShellConfig = store.save;
