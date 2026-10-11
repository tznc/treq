import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  type ActiveRepository,
  repositoryCacheKey,
  setActiveRepositorySingleton,
} from "./active-repository";
import {
  invalidateRemoteRepositoryData,
  useRemoteMutationFeedback,
} from "./remote-mutation-ui";

vi.mock("./swr-cache", () => ({
  invalidateQueries: vi.fn(() => Promise.resolve()),
}));

vi.mock("./change-file-drag", () => ({
  scheduleRefreshWorkspaceChanges: vi.fn(),
}));

import { invalidateQueries } from "./swr-cache";
import { scheduleRefreshWorkspaceChanges } from "./change-file-drag";

describe("useRemoteMutationFeedback", () => {
  beforeEach(() => {
    vi.mocked(invalidateQueries).mockClear();
    useRemoteMutationFeedback.setState({
      ambiguousReason: null,
      lastStatus: null,
    });
  });

  it("refreshes repository data for applied and already_applied", () => {
    useRemoteMutationFeedback.getState().report({
      status: "applied",
      value: "ok",
    });
    expect(invalidateQueries).toHaveBeenCalled();
    expect(useRemoteMutationFeedback.getState().ambiguousReason).toBeNull();

    vi.mocked(invalidateQueries).mockClear();
    useRemoteMutationFeedback.getState().report({ status: "already_applied" });
    expect(invalidateQueries).toHaveBeenCalled();
    expect(useRemoteMutationFeedback.getState().lastStatus).toBe(
      "already_applied",
    );
  });

  it("refreshes cached data on ambiguous so the user can check before retrying", () => {
    useRemoteMutationFeedback.getState().report({
      status: "ambiguous",
      reason: "unknown",
    });
    expect(invalidateQueries).toHaveBeenCalled();
    expect(scheduleRefreshWorkspaceChanges).toHaveBeenCalled();
    expect(useRemoteMutationFeedback.getState().ambiguousReason).toBe(
      "unknown",
    );
  });
});

describe("invalidateRemoteRepositoryData", () => {
  const endpoint = {
    id: "endpoint-1",
    hostname: "box",
  } as unknown as NonNullable<ActiveRepository["endpoint"]>;
  const repo: ActiveRepository = {
    id: "remote-1",
    location: { type: "ssh", host: "box", path: "/srv/project" },
    endpoint,
    endpointId: "endpoint-1",
    endpointGeneration: 1,
    canonicalPath: "/srv/project",
    displayName: "project",
    transport: { type: "ssh", endpoint },
  };

  beforeEach(() => {
    vi.mocked(invalidateQueries).mockClear();
    setActiveRepositorySingleton(repo);
  });

  afterEach(() => {
    setActiveRepositorySingleton(null);
  });

  it("covers changed files, diffs, commits, status and conflicts", () => {
    invalidateRemoteRepositoryData();
    const key = repositoryCacheKey(repo);
    const prefixes = vi
      .mocked(invalidateQueries)
      .mock.calls.map(([prefix]) => prefix);
    expect(prefixes).toEqual(
      expect.arrayContaining([
        ["workspace-changed-files"],
        ["workspace-diff"],
        ["workspace-commits", key],
        ["commit-diff-viewer-commits"],
        ["workspace-statuses", key],
        // Per-workspace status carries the conflict state the overview shows.
        ["workspace-status", key],
        ["workspace-overview", key],
      ]),
    );
  });
});
