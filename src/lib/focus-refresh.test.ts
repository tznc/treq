import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FOCUS_REFRESH_DEBOUNCE_MS, onWindowFocus } from "./focus-refresh";

function setVisibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => state,
  });
}

describe("onWindowFocus", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    setVisibility("visible");
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("calls back once after a burst of focus events settles", () => {
    const callback = vi.fn();
    const stop = onWindowFocus(callback);

    window.dispatchEvent(new Event("focus"));
    document.dispatchEvent(new Event("visibilitychange"));
    window.dispatchEvent(new Event("focus"));
    expect(callback).not.toHaveBeenCalled();

    vi.advanceTimersByTime(FOCUS_REFRESH_DEBOUNCE_MS);
    expect(callback).toHaveBeenCalledTimes(1);
    stop();
  });

  it("ignores the page becoming hidden", () => {
    const callback = vi.fn();
    const stop = onWindowFocus(callback);

    setVisibility("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    vi.advanceTimersByTime(FOCUS_REFRESH_DEBOUNCE_MS * 2);
    expect(callback).not.toHaveBeenCalled();
    stop();
  });

  it("stops listening and drops a pending call when stopped", () => {
    const callback = vi.fn();
    const stop = onWindowFocus(callback);

    window.dispatchEvent(new Event("focus"));
    stop();
    vi.advanceTimersByTime(FOCUS_REFRESH_DEBOUNCE_MS * 2);
    window.dispatchEvent(new Event("focus"));
    vi.advanceTimersByTime(FOCUS_REFRESH_DEBOUNCE_MS * 2);
    expect(callback).not.toHaveBeenCalled();
  });
});
