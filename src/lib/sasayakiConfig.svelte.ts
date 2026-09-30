import { persisted } from "./persisted.svelte";

export type SasayakiConfig = {
  enableSasayaki: boolean;
  sasayakiAutoScroll: boolean;
  sasayakiAutoPause: boolean;
  sasayakiImagePause: boolean;
  sasayakiImagePauseDuration: number;
  sasayakiPageAdvance: boolean;
  sasayakiShowControlBar: boolean;
  sasayakiTextColor: string;
  sasayakiBackgroundColor: string;
  sasayakiDarkTextColor: string;
  sasayakiDarkBackgroundColor: string;
  sasayakiAudioFormat: "mp3" | "opus";
};

const defaults: SasayakiConfig = {
  enableSasayaki: true,
  sasayakiAutoScroll: true,
  sasayakiAutoPause: true,
  sasayakiImagePause: true,
  sasayakiImagePauseDuration: 3,
  sasayakiPageAdvance: false,
  sasayakiShowControlBar: true,
  sasayakiTextColor: "#000000ff",
  sasayakiBackgroundColor: "#87cefa66",
  sasayakiDarkTextColor: "#ffffffff",
  sasayakiDarkBackgroundColor: "#87cefa66",
  sasayakiAudioFormat: "mp3",
};

const store = persisted<SasayakiConfig>("sasayaki.config", defaults);

export const sasayakiConfig = store.config;
export const saveSasayakiConfig = store.save;

export function colorHex(value: string): string {
  return value.slice(0, 7);
}

export function colorAlpha(value: string): number {
  return value.length >= 9 ? parseInt(value.slice(7, 9), 16) / 255 : 1;
}

export function withAlpha(value: string, alpha: number): string {
  return colorHex(value) + Math.round(alpha * 255).toString(16).padStart(2, "0");
}
