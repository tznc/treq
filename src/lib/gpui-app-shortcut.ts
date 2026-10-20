/**
 * Replays one of Dashboard's shortcuts that GPUI saw (Cmd+I, N, Shift+N, P
 * or "?") as the keydown `useKeyboardShortcut` listens for. `key` is upper
 * case for Shift: "N" is Cmd+Shift+N.
 */
export function dispatchGpuiAppShortcut(key: string): void {
  const mac = navigator.platform.toUpperCase().includes("MAC");
  const command = key !== "?";
  window.dispatchEvent(
    new KeyboardEvent("keydown", {
      key,
      metaKey: command && mac,
      ctrlKey: command && !mac,
      shiftKey: key === "?" || key !== key.toLowerCase(),
      bubbles: true,
    }),
  );
}
