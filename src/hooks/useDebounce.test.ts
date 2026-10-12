import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useDebounce } from "./useDebounce";

describe("useDebounce", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns the initial value immediately", () => {
    const { result } = renderHook(() => useDebounce("test", 100));
    expect(result.current).toBe("test");
  });

  it("updates only after the delay elapses", () => {
    const { result, rerender } = renderHook(
      ({ value, delay }) => useDebounce(value, delay),
      { initialProps: { value: "initial", delay: 100 } },
    );

    rerender({ value: "updated", delay: 100 });
    expect(result.current).toBe("initial");

    act(() => {
      vi.advanceTimersByTime(99);
    });
    expect(result.current).toBe("initial");

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(result.current).toBe("updated");
  });

  it("resets the timer on rapid changes", () => {
    const { result, rerender } = renderHook(
      ({ value, delay }) => useDebounce(value, delay),
      { initialProps: { value: "initial", delay: 100 } },
    );

    rerender({ value: "change1", delay: 100 });
    act(() => {
      vi.advanceTimersByTime(60);
    });
    rerender({ value: "change2", delay: 100 });
    act(() => {
      vi.advanceTimersByTime(60);
    });
    rerender({ value: "final", delay: 100 });
    act(() => {
      vi.advanceTimersByTime(99);
    });
    expect(result.current).toBe("initial");

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(result.current).toBe("final");
  });
});
