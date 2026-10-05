import { persisted } from "./persisted.svelte";

export type ClickLookup = "off" | "left" | "right" | "middle";

export type HotkeyConfig = {
  scanModifier: string;
  scanDelay: number;
  clickLookup: ClickLookup;
  hidePopupOnCursorExit: boolean;
  hidePopupOnCursorExitDelay: number;
  disableReaderWheel: boolean;
  sasayakiPreviousCue: string;
  sasayakiNextCue: string;
  sasayakiPlayback: string;
  toggleTracking: string;
  previousPage: string;
  nextPage: string;
  reversePageVertical: boolean;
  pageClickZone: number;
};

const defaults: HotkeyConfig = {
  scanModifier: "Shift",
  scanDelay: 20,
  clickLookup: "off",
  hidePopupOnCursorExit: false,
  hidePopupOnCursorExitDelay: 0,
  disableReaderWheel: false,
  sasayakiPreviousCue: "[",
  sasayakiNextCue: "]",
  sasayakiPlayback: " ",
  toggleTracking: "p",
  previousPage: "ArrowLeft",
  nextPage: "ArrowRight",
  reversePageVertical: true,
  pageClickZone: 0,
};

export const readerHotkeys = [
  { key: "sasayakiPreviousCue", label: "Previous Cue", section: "Sasayaki" },
  { key: "sasayakiNextCue", label: "Next Cue", section: "Sasayaki" },
  { key: "sasayakiPlayback", label: "Play / Pause", section: "Sasayaki" },
  { key: "toggleTracking", label: "Pause / Resume Statistics", section: "Reader" },
  { key: "previousPage", label: "Previous Page", section: "Reader" },
  { key: "nextPage", label: "Next Page", section: "Reader" },
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
  if (key === "") return "None";
  if (key === "Control") return "Ctrl";
  if (key === " ") return "Space";
  if (key in mouseButtonLabels) return mouseButtonLabels[key];
  return key.length === 1 ? key.toUpperCase() : key;
}
