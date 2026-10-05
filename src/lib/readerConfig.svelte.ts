import { persisted } from "./persisted.svelte";
import { isMac } from "./platform";

export type FuriganaMode = "Off" | "Dimmed" | "Toggle" | "Hidden";
export type ThemeName = "System" | "Light" | "Dark" | "Sepia" | "Custom";
export type ProgressCount = "Off" | "Characters" | "Pages";

export const defaultFonts = isMac
  ? ["Hiragino Mincho ProN", "Hiragino Kaku Gothic ProN"]
  : ["Yu Mincho", "Yu Gothic", "Noto Serif JP", "Noto Sans JP"];

export type ReaderConfig = {
  theme: ThemeName;
  selectedFont: string;
  selectedFontFile: string;
  uiTheme: ThemeName;
  systemLightSepia: boolean;
  sepiaInvertInDark: boolean;
  customBackgroundColor: string;
  customTextColor: string;
  customInfoColor: string;
  verticalWriting: boolean;
  paragraphMode: boolean;
  maxSentencesPerPage: number;
  splitDialogue: boolean;
  textAnimation: boolean;
  textSpeed: number;
  clickToAdvance: boolean;
  paragraphHideBookmark: boolean;
  fontSize: number;
  furiganaMode: FuriganaMode;
  horizontalPadding: number;
  verticalPadding: number;
  maxWidth: number;
  maxHeight: number;
  avoidPageBreak: boolean;
  justifyText: boolean;
  blurImages: boolean;
  layoutAdvanced: boolean;
  spreadLayout: boolean;
  spreadTopProgress: boolean;
  spreadChapterTitle: boolean;
  lineHeight: number;
  characterSpacing: number;
  paragraphSpacing: number;
  showProgress: boolean;
  showChapterProgress: boolean;
  progressCount: ProgressCount;
  showPercentage: boolean;
  showStatisticsToggle: boolean;
  showReadingSpeed: boolean;
  showReadingTime: boolean;
  popupWidth: number;
  popupHeight: number;
  popupScale: number;
  popupActionBar: boolean;
  popupDisableTransparency: boolean;
};

const defaults: ReaderConfig = {
  theme: "System",
  selectedFont: defaultFonts[0],
  selectedFontFile: "",
  uiTheme: "System",
  systemLightSepia: false,
  sepiaInvertInDark: false,
  customBackgroundColor: "#ffffff",
  customTextColor: "#000000",
  customInfoColor: "#999999",
  verticalWriting: true,
  paragraphMode: false,
  maxSentencesPerPage: 0,
  splitDialogue: false,
  textAnimation: false,
  textSpeed: 35,
  clickToAdvance: true,
  paragraphHideBookmark: true,
  fontSize: 22,
  furiganaMode: "Off",
  horizontalPadding: 0,
  verticalPadding: 0,
  maxWidth: 0,
  maxHeight: 0,
  avoidPageBreak: false,
  justifyText: false,
  blurImages: false,
  layoutAdvanced: false,
  spreadLayout: false,
  spreadTopProgress: true,
  spreadChapterTitle: false,
  lineHeight: 1.65,
  characterSpacing: 0,
  paragraphSpacing: 0,
  showProgress: true,
  showChapterProgress: false,
  progressCount: "Characters",
  showPercentage: true,
  showStatisticsToggle: false,
  showReadingSpeed: false,
  showReadingTime: false,
  popupWidth: 400,
  popupHeight: 300,
  popupScale: 1.0,
  popupActionBar: false,
  popupDisableTransparency: false,
};

const store = persisted<ReaderConfig>("reader.config", defaults);

export const readerConfig = store.config;
export const saveReaderConfig = store.save;

const legacy = readerConfig as ReaderConfig & { showCharacters?: boolean };
if (legacy.showCharacters !== undefined) {
  if (!legacy.showCharacters) readerConfig.progressCount = "Off";
  delete legacy.showCharacters;
  saveReaderConfig();
}
