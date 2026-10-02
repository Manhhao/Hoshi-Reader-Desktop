import { readerConfig, type ThemeName } from "./readerConfig.svelte";

const media = window.matchMedia("(prefers-color-scheme: dark)");
const os = $state({ dark: media.matches });
media.addEventListener("change", (e) => {
  os.dark = e.matches;
});

function schemeOf(theme: ThemeName): "light" | "dark" | null {
  if (theme === "Light" || theme === "Sepia") return "light";
  if (theme === "Dark") return "dark";
  return null;
}

export function chromeOverride(): "light" | "dark" | null {
  if (readerConfig.theme === "Custom") return schemeOf(readerConfig.uiTheme);
  if (readerConfig.theme === "Sepia" && readerConfig.sepiaInvertInDark) return null;
  return schemeOf(readerConfig.theme);
}

export function resolvedScheme(): "light" | "dark" {
  return chromeOverride() ?? (os.dark ? "dark" : "light");
}

function sepiaInverted(): boolean {
  return readerConfig.theme === "Sepia" && readerConfig.sepiaInvertInDark && os.dark;
}

function sepiaActive(): boolean {
  return (
    readerConfig.theme === "Sepia" ||
    (readerConfig.theme === "System" && readerConfig.systemLightSepia && !os.dark)
  );
}

export function readerBackground(): string {
  if (sepiaInverted()) return "#18150c";
  if (sepiaActive()) return "#f2e2c9";
  if (readerConfig.theme === "Custom") return readerConfig.customBackgroundColor;
  return resolvedScheme() === "dark" ? "#000000" : "#ffffff";
}

export function readerTextColor(): string | null {
  if (sepiaInverted()) return "#f2e2c9";
  if (sepiaActive()) return "#332a1b";
  return readerConfig.theme === "Custom" ? readerConfig.customTextColor : null;
}

export function infoColor(): string | null {
  return readerConfig.theme === "Custom" ? readerConfig.customInfoColor : null;
}

export function chromeTheme(): "hoshi-light" | "hoshi-dark" | "hoshi-sepia" | "hoshi-sepia-dark" {
  if (sepiaInverted()) return "hoshi-sepia-dark";
  if (sepiaActive()) return "hoshi-sepia";
  return resolvedScheme() === "dark" ? "hoshi-dark" : "hoshi-light";
}
