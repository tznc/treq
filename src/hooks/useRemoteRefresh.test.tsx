import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { SWRConfig } from "swr";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  repositoryCacheKey,
  type ActiveRepository,
} from "../lib/active-repository";
import { remoteActionKeys } from "../lib/remote-idempotency";

vi.mock("../lib/repository-adapter", () => ({
  transportChangeMarker: vi.fn(),
}));
vi.mock("../lib/remote-mutation-ui", () => ({
  invalidateRemoteRepositoryData: vi.fn(),
}));
vi.mock("../lib/change-file-drag", () => ({
  scheduleRefreshWorkspaceChanges: vi.fn(),
}));

import { scheduleRefreshWorkspaceChanges } from "../lib/change-file-drag";
import { invalidateRemoteRepositoryData } from "../lib/remote-mutation-ui";
import { transportChangeMarker } from "../lib/repository-adapter";
import { useRemoteChangeMarkerWatch } from "./useRemoteRefresh";

const endpoint = {
  id: "endpoint-1",
  hostname: "box",
} as unknown as NonNullable<ActiveRepository["endpoint"]>;

const remoteRepo: ActiveRepository = {
  id: "remote-1",
  location: { type: "ssh", host: "box", path: "/srv/project" },
  endpoint,
  endpointId: "endpoint-1",
  endpointGeneration: 1,
  canonicalPath: "/srv/project",
  displayName: "project",
  transport: { type: "ssh", endpoint },
};

function wrapper({ children }: { children: ReactNode }) {
  return (
    <SWRConfig value={{ provider: () => new Map() }}>{children}</SWRConfig>
  );
}

async function poll() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(4_100);
  });
}

describe("useRemoteChangeMarkerWatch", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(transportChangeMarker).mockReset();
    vi.mocked(invalidateRemoteRepositoryData).mockClear();
    vi.mocked(scheduleRefreshWorkspaceChanges).mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("refreshes repository data and the workspace's changes when the marker moves", async () => {
    // The VM snapshots the working copy on every marker read, so a file an
    // agent wrote outside jj shows up here as a new operation id.
    vi.mocked(transportChangeMarker)
      .mockResolvedValueOnce({ operation_id: "op-1" })
      .mockResolvedValueOnce({ operation_id: "op-1" })
      .mockResolvedValue({ operation_id: "op-2" });

    renderHook(() => useRemoteChangeMarkerWatch(remoteRepo, 7), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(transportChangeMarker).toHaveBeenCalledWith(remoteRepo, 7);
    // The first value is a baseline, and an unchanged marker is not a change.
    await poll();
    expect(invalidateRemoteRepositoryData).not.toHaveBeenCalled();
    expect(scheduleRefreshWorkspaceChanges).not.toHaveBeenCalled();

    const release = vi.spyOn(remoteActionKeys, "release");
    await poll();
    expect(invalidateRemoteRepositoryData).toHaveBeenCalledTimes(1);
    expect(release).toHaveBeenCalledWith(repositoryCacheKey(remoteRepo));
    expect(scheduleRefreshWorkspaceChanges).toHaveBeenCalledWith({
      workspaceId: 7,
    });
  });

  it("does not poll for a local repository", async () => {
    renderHook(() => useRemoteChangeMarkerWatch(null, 7), { wrapper });
    await poll();
    expect(transportChangeMarker).not.toHaveBeenCalled();
  });
});
