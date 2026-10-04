import { persisted } from "./persisted.svelte";

export type ClickLookup = "off" | "left" | "right" | "middle";

export type HotkeyConfig = {
  scanModifier: string;
  clickLookup: ClickLookup;
  disableReaderWheel: boolean;
  sasayakiPreviousCue: string;
  sasayakiNextCue: string;
  sasayakiPlayback: string;
  toggleTracking: string;
};

const defaults: HotkeyConfig = {
  scanModifier: "Shift",
  clickLookup: "off",
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
const legacyClickLookup: unknown = hotkeyConfig.clickLookup;
if (typeof legacyClickLookup === "boolean") hotkeyConfig.clickLookup = legacyClickLookup ? "left" : "off";
export const saveHotkeyConfig = store.save;

export const mouseButtonTokens: Record<number, string> = {
  1: "Mouse:Middle",
  2: "Mouse:Right",
  3: "Mouse:Back",
  4: "Mouse:Forward",
};

const mouseButtonLabels: Record<string, string> = {
  "Mouse:Middle": "Middle Click",
  "Mouse:Right": "Right Click",
  "Mouse:Back": "Back Button",
  "Mouse:Forward": "Forward Button",
};

export function isMouseHotkey(key: string): boolean {
  return key.startsWith("Mouse:");
}

export function normalizeHotkey(key: string): string {
  return key.length === 1 ? key.toLowerCase() : key;
}

export function hotkeyLabel(key: string): string {
  if (key === "Control") return "Ctrl";
  if (key === " ") return "Space";
  if (key in mouseButtonLabels) return mouseButtonLabels[key];
  return key.length === 1 ? key.toUpperCase() : key;
}
