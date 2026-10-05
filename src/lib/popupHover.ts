import { hotkeyConfig } from "./hotkeyConfig.svelte";

export function cursorExitHider(close: (keep: number) => void) {
  let timer = 0;
  return {
    hover(index: number) {
      clearTimeout(timer);
      if (!hotkeyConfig.hidePopupOnCursorExit) return;
      timer = setTimeout(() => close(index + 1), hotkeyConfig.hidePopupOnCursorExitDelay);
    },
    cancel() {
      clearTimeout(timer);
    },
  };
}
