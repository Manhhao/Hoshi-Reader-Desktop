export const isMac = navigator.userAgent.includes("Macintosh");
export const isChromium = navigator.userAgent.includes("Chrome/");
export const useHttpScheme = /Windows|Android/.test(navigator.userAgent);
