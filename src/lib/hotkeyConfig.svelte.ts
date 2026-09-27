import { persisted } from "./persisted.svelte";

export type HotkeyConfig = {
  scanModifier: string;
  clickLookup: boolean;
  disableReaderWheel: boolean;
  sasayakiPreviousCue: string;
  sasayakiNextCue: string;
  sasayakiPlayback: string;
  toggleTracking: string;
};

const defaults: HotkeyConfig = {
  scanModifier: "Shift",
  clickLookup: false,
  disableReaderWheel: false,
  sasayakiPreviousCue: "[",
  sasayakiNextCue: "]",
  sasayakiPlayback: " ",
  toggleTracking: "p",
};

export const readerHotkeys = [
  { key: "sasayakiPreviousCue", label: "Previous Cue", section: "Sasayaki" },
  { key: "sasayakiNextCue", label: "Next Cue", section: "Sasayaki" },
  { key: "sasayakiPlayback", label: "Play / Pause", section: "Sasayaki" },
  { key: "toggleTracking", label: "Pause / Resume Statistics", section: "Reader" },
] as const;

const store = persisted<HotkeyConfig>("hotkeys.config", defaults);

export const hotkeyConfig = store.config;
export const saveHotkeyConfig = store.save;

export function normalizeHotkey(key: string): string {
  return key.length === 1 ? key.toLowerCase() : key;
}

export function hotkeyLabel(key: string): string {
  if (key === "Control") return "Ctrl";
  if (key === " ") return "Space";
  return key.length === 1 ? key.toUpperCase() : key;
}
