import { afterEach, describe, expect, it, vi } from "vitest";
import {
  REFRESH_WORKSPACE_CHANGES_EVENT,
  WORKSPACE_CHANGES_REFRESH_DEBOUNCE_MS,
  cancelScheduledRefreshWorkspaceChanges,
  scheduleRefreshWorkspaceChanges,
} from "./change-file-drag";

describe("scheduleRefreshWorkspaceChanges", () => {
  afterEach(() => {
    cancelScheduledRefreshWorkspaceChanges();
    vi.useRealTimers();
  });

  it("coalesces rapid file-system refresh requests into one event", () => {
    vi.useFakeTimers();
    const onRefresh = vi.fn();
    window.addEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, onRefresh);

    scheduleRefreshWorkspaceChanges();
    scheduleRefreshWorkspaceChanges();
    scheduleRefreshWorkspaceChanges();
    expect(onRefresh).not.toHaveBeenCalled();

    vi.advanceTimersByTime(WORKSPACE_CHANGES_REFRESH_DEBOUNCE_MS - 1);
    expect(onRefresh).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(onRefresh).toHaveBeenCalledTimes(1);

    window.removeEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, onRefresh);
  });

  it("merges changed paths from bursts into one event", () => {
    vi.useFakeTimers();
    const onRefresh = vi.fn();
    window.addEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, onRefresh);

    scheduleRefreshWorkspaceChanges({
      workspaceId: 7,
      changedPaths: ["/ws/a.ts"],
    });
    scheduleRefreshWorkspaceChanges({
      workspaceId: 7,
      changedPaths: ["/ws/b.ts"],
    });
    vi.advanceTimersByTime(WORKSPACE_CHANGES_REFRESH_DEBOUNCE_MS);

    expect(onRefresh).toHaveBeenCalledTimes(1);
    const event = onRefresh.mock.calls[0][0] as CustomEvent<{
      workspaceId?: number;
      changedPaths?: string[];
    }>;
    expect(event.detail.workspaceId).toBe(7);
    expect(event.detail.changedPaths).toEqual(["/ws/a.ts", "/ws/b.ts"]);

    window.removeEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, onRefresh);
  });
});
