import { renderHook, waitFor } from "@testing-library/react";
import React from "react";
import { SWRConfig } from "swr";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useEnqueueWorkspace,
  usePrCiStatus,
  usePrInfoViaGh,
} from "./useMergeQueueStatus";
import * as api from "../lib/api";
import type { GitRemoteInfo, PrCiStatus, PrInfo } from "../lib/api-types";
import { SWRMutateScope, testSWRConfig } from "../lib/swr-cache";

const { mockEdgeFn, mockRpc, queueEnabled } = vi.hoisted(() => {
  const queueEnabled = { current: true };
  return {
    queueEnabled,
    mockEdgeFn: vi.fn(),
    mockRpc: vi.fn(async (fn: string) =>
      fn === "get_merge_queue_enabled"
        ? { data: queueEnabled.current, error: null }
        : { data: [], error: null },
    ),
  };
});
const mockFeatures = vi.hoisted(() => ({ mergeQueue: true }));
vi.mock("../lib/features", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/features")>();
  return {
    ...actual,
    FEATURES: Object.assign(mockFeatures, {
      ...actual.FEATURES,
      ...mockFeatures,
    }),
  };
});
vi.mock("../lib/supabase", () => ({
  supabase: {
    rpc: mockRpc,
    functions: { invoke: mockEdgeFn },
  },
}));

vi.mock("../lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    startPrStatusPolling: vi.fn(async () => undefined),
    stopPrStatusPolling: vi.fn(async () => undefined),
    refreshPrBranchStatus: vi.fn(async () => undefined),
    refreshPrStatuses: vi.fn(async () => undefined),
  };
});

function makeWrapper() {
  return ({ children }: { children: React.ReactNode }) =>
    React.createElement(
      SWRConfig,
      { value: testSWRConfig },
      React.createElement(SWRMutateScope, null, children),
    );
}

const OPEN_PR: PrInfo = {
  number: 1,
  title: "My PR",
  state: "OPEN",
  url: "https://github.com/ziinc/treq/pull/1",
  head_ref_name: "feat",
  base_ref_name: "main",
  merge_state_status: "CLEAN",
};

const GITHUB_REMOTE: GitRemoteInfo = {
  owner: "ziinc",
  repo: "treq",
  full_name: "ziinc/treq",
};

describe("usePrInfoViaGh", () => {
  const repoPath = "/tmp/fake-pr-info-repo";

  beforeEach(() => {
    vi.mocked(api.startPrStatusPolling).mockClear();
    vi.mocked(api.refreshPrBranchStatus).mockClear();
    vi.spyOn(api, "getCachedPrInfo").mockResolvedValue(OPEN_PR);
  });

  afterEach(() => {
    vi.mocked(api.getCachedPrInfo).mockReset();
  });

  it("reads from the Rust PR-status cache", async () => {
    const { result } = renderHook(() => usePrInfoViaGh(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
      expect(result.current.error).toBeUndefined();
    });
    expect(result.current.data).toEqual(OPEN_PR);
    expect(api.getCachedPrInfo).toHaveBeenCalledWith(repoPath, "feat");
  });

  it("queues an out-of-band PR+CI refresh when a branch is opened", async () => {
    renderHook(() => usePrInfoViaGh(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() =>
      expect(api.refreshPrBranchStatus).toHaveBeenCalledWith(repoPath, "feat"),
    );
  });

  it("is disabled when branchName is undefined", () => {
    const { result } = renderHook(() => usePrInfoViaGh(repoPath, undefined), {
      wrapper: makeWrapper(),
    });
    expect(result.current.isLoading).toBe(false);
    expect(result.current.isValidating).toBe(false);
  });
});

const SUCCESS_CI: PrCiStatus = {
  state: "success",
  total: 2,
  passed: 2,
  failed: 0,
  pending: 0,
  checks: [
    { name: "build", bucket: "pass", link: "https://x/1" },
    { name: "lint", bucket: "pass", link: "https://x/2" },
  ],
};

describe("usePrCiStatus", () => {
  const repoPath = "/tmp/fake-pr-ci-repo";

  beforeEach(() => {
    vi.mocked(api.startPrStatusPolling).mockClear();
    vi.mocked(api.refreshPrBranchStatus).mockClear();
    vi.spyOn(api, "getCachedPrCiStatus").mockResolvedValue(SUCCESS_CI);
  });

  afterEach(() => {
    vi.mocked(api.getCachedPrCiStatus).mockReset();
  });

  it("reads from the Rust CI-status cache", async () => {
    const { result } = renderHook(() => usePrCiStatus(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
      expect(result.current.error).toBeUndefined();
    });
    expect(result.current.data).toEqual(SUCCESS_CI);
    expect(api.getCachedPrCiStatus).toHaveBeenCalledWith(repoPath, "feat");
  });

  it("queues an out-of-band PR+CI refresh when a branch is opened", async () => {
    renderHook(() => usePrCiStatus(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() =>
      expect(api.refreshPrBranchStatus).toHaveBeenCalledWith(repoPath, "feat"),
    );
  });

  it("is disabled when branchName is undefined", () => {
    const { result } = renderHook(() => usePrCiStatus(repoPath, undefined), {
      wrapper: makeWrapper(),
    });
    expect(result.current.isLoading).toBe(false);
    expect(result.current.isValidating).toBe(false);
  });
});

describe("useEnqueueWorkspace", () => {
  const repoPath = "/tmp/fake-enqueue-repo";

  beforeEach(() => {
    mockEdgeFn.mockReset();
    mockRpc.mockClear();
    mockFeatures.mergeQueue = true;
    queueEnabled.current = true;
    vi.mocked(api.startPrStatusPolling).mockClear();
    vi.mocked(api.refreshPrBranchStatus).mockClear();
    vi.spyOn(api, "getCachedPrInfo").mockResolvedValue(null);
    vi.spyOn(api, "getGitRemoteUrl").mockResolvedValue(GITHUB_REMOTE);
  });

  afterEach(() => {
    vi.mocked(api.getCachedPrInfo).mockReset();
    vi.mocked(api.getGitRemoteUrl).mockReset();
  });

  it("calls enqueue-workspace edge function with correct payload", async () => {
    mockEdgeFn.mockResolvedValue({ error: null });

    const { result } = renderHook(() => useEnqueueWorkspace(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => expect(result.current.remoteInfo).toBeTruthy());

    await result.current.enqueue.mutateAsync();

    expect(mockEdgeFn).toHaveBeenCalledWith("enqueue-workspace", {
      body: {
        repo_full_name: "ziinc/treq",
        branch_name: "feat",
        action: "enqueue",
      },
    });
  });

  it("blocks enqueue when gh reports PR state is not OPEN", async () => {
    vi.mocked(api.getCachedPrInfo).mockResolvedValue({
      ...OPEN_PR,
      state: "MERGED",
    });

    const { result } = renderHook(() => useEnqueueWorkspace(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => {
      expect(result.current.remoteInfo).toBeTruthy();
      expect(result.current.prInfoGh).not.toBeUndefined();
    });

    await expect(result.current.enqueue.mutateAsync()).rejects.toThrow(
      "No open PR found",
    );
    expect(mockEdgeFn).not.toHaveBeenCalled();
  });

  it("allows dequeue even when gh reports PR state is MERGED", async () => {
    vi.mocked(api.getCachedPrInfo).mockResolvedValue({
      ...OPEN_PR,
      state: "MERGED",
    });
    mockEdgeFn.mockResolvedValue({ error: null });

    const { result } = renderHook(() => useEnqueueWorkspace(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => expect(result.current.remoteInfo).toBeTruthy());

    await result.current.dequeue.mutateAsync();

    expect(mockEdgeFn).toHaveBeenCalledWith(
      "enqueue-workspace",
      expect.objectContaining({
        body: expect.objectContaining({ action: "dequeue" }),
      }),
    );
  });

  it("skips pre-flight and proceeds when gh returns null", async () => {
    mockEdgeFn.mockResolvedValue({ error: null });

    const { result } = renderHook(() => useEnqueueWorkspace(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => expect(result.current.remoteInfo).toBeTruthy());

    await result.current.enqueue.mutateAsync();
    expect(mockEdgeFn).toHaveBeenCalled();
  });

  it("does not enqueue when the gh pre-flight fails", async () => {
    vi.mocked(api.getCachedPrInfo).mockRejectedValue(
      new Error("gh authentication failed"),
    );

    const { result } = renderHook(() => useEnqueueWorkspace(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });
    await waitFor(() => expect(result.current.prInfoGhError).toBeTruthy());

    await expect(result.current.enqueue.mutateAsync()).rejects.toThrow(
      "gh authentication failed",
    );
    expect(mockEdgeFn).not.toHaveBeenCalled();
  });

  it("throws when no GitHub remote is detected", async () => {
    vi.mocked(api.getGitRemoteUrl).mockResolvedValue(null);

    const { result } = renderHook(() => useEnqueueWorkspace(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });

    await waitFor(() => {
      expect(result.current.remoteInfo).toBeNull();
    });

    await expect(result.current.enqueue.mutateAsync()).rejects.toThrow(
      "Repository or branch not detected",
    );
    expect(mockEdgeFn).not.toHaveBeenCalled();
  });

  it("skips the opt-in lookup and refuses to enqueue when the flag is off", async () => {
    mockFeatures.mergeQueue = false;
    vi.mocked(api.getCachedPrInfo).mockResolvedValue(OPEN_PR);
    mockEdgeFn.mockResolvedValue({ error: null });

    const { result } = renderHook(() => useEnqueueWorkspace(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });

    await waitFor(() => {
      expect(result.current.remoteInfo).toBeTruthy();
      expect(result.current.enqueue.isPending).toBe(false);
    });

    await expect(result.current.enqueue.mutateAsync()).rejects.toThrow(
      /not enabled for this repository/i,
    );
    expect(mockRpc).not.toHaveBeenCalledWith(
      "get_merge_queue_enabled",
      expect.anything(),
    );
    expect(mockEdgeFn).not.toHaveBeenCalled();
  });

  it("refuses to enqueue when the repo has not enabled the merge queue", async () => {
    queueEnabled.current = false;
    vi.mocked(api.getCachedPrInfo).mockResolvedValue(OPEN_PR);
    mockEdgeFn.mockResolvedValue({ error: null });

    const { result } = renderHook(() => useEnqueueWorkspace(repoPath, "feat"), {
      wrapper: makeWrapper(),
    });

    await waitFor(() => {
      expect(result.current.remoteInfo).toBeTruthy();
      expect(result.current.enqueue.isPending).toBe(false);
    });

    await expect(result.current.enqueue.mutateAsync()).rejects.toThrow(
      /not enabled for this repository/i,
    );
    expect(mockEdgeFn).not.toHaveBeenCalled();
  });
});
