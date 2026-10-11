// Runs a callback when the app window comes back to the foreground, for
// example after the user finishes checkout in the browser. A window that
// regains focus fires `focus` and often `visibilitychange` together, so the
// callback is debounced into one call.

export const FOCUS_REFRESH_DEBOUNCE_MS = 1000;

/** Returns a function that stops listening. */
export function onWindowFocus(
  callback: () => void,
  debounceMs = FOCUS_REFRESH_DEBOUNCE_MS,
): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;

  const schedule = () => {
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      callback();
    }, debounceMs);
  };
  const onVisibilityChange = () => {
    if (document.visibilityState === "visible") schedule();
  };

  window.addEventListener("focus", schedule);
  document.addEventListener("visibilitychange", onVisibilityChange);
  return () => {
    window.removeEventListener("focus", schedule);
    document.removeEventListener("visibilitychange", onVisibilityChange);
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };
}
