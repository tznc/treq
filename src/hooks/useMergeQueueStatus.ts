import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import useSWR from "swr";
import {
  getCachedPrCiStatus,
  getCachedPrInfo,
  getGitRemoteUrl,
  getPrChecksForPr,
  getPrChecksViaGh,
  getPrInfoViaGh,
  listCachedPrCiStatuses,
  listCachedPrStatuses,
  refreshPrBranchStatus,
  refreshPrStatuses,
  startPrStatusPolling,
  stopPrStatusPolling,
} from "../lib/api";
import type {
  PrCiStatus,
  PrInfo,
  WorkspaceQueueStatus,
} from "../lib/api-types";
import { FEATURES } from "../lib/features";
import { supabase } from "../lib/supabase";
import { invalidateQueries, pollMs, setQueryData } from "../lib/swr-cache";
import { useMutation } from "./useMutation";

/** Query key for the per-repo merge queue opt-in. */
const mergeQueueEnabledKey = (repoFullName: string | undefined) => [
  "merge-queue-enabled",
  repoFullName,
];

/** Query key for the Rust-side cached PR statuses map for a repo. */
export const cachedPrStatusesKey = (repoPath: string | undefined) => [
  "cached-pr-statuses",
  repoPath,
];

/** Query key for the Rust-side cached CI statuses map for a repo. */
const cachedPrCiStatusesKey = (repoPath: string | undefined) => [
  "cached-pr-ci-statuses",
  repoPath,
];

/**
 * Shared mutation key for "create a PR for this workspace" actions. Multiple
 * surfaces can trigger PR creation for the same workspace (the header's
 * Create PR button, the Review tab's "Commit and create PR" dropdown item);
 * tagging each mutation with this key lets every surface detect via
 * `useIsMutating` that a create-PR request is in flight, regardless of which
 * one started it, so they can all render a loading state together.
 */
export const createPrMutationKey = (
  repoPath: string | undefined,
  workspaceId: number | null | undefined,
) => ["create-pr", repoPath, workspaceId ?? null] as const;

type PrStatusesPayload = {
  repo_path: string;
  statuses: Record<string, PrInfo | null>;
  ci_statuses?: Record<string, PrCiStatus | null>;
};

function applyPrStatusesToQueryCache(payload: {
  repoPath: string;
  statuses: Record<string, PrInfo | null>;
  ciStatuses?: Record<string, PrCiStatus | null>;
}) {
  const { repoPath, statuses, ciStatuses } = payload;
  void setQueryData(cachedPrStatusesKey(repoPath), statuses);
  for (const [branch, info] of Object.entries(statuses)) {
    void setQueryData(["pr-info-gh", repoPath, branch], info);
  }
  if (ciStatuses) {
    void setQueryData(cachedPrCiStatusesKey(repoPath), ciStatuses);
    for (const [branch, status] of Object.entries(ciStatuses)) {
      void setQueryData(["pr-ci-status", repoPath, branch], status);
    }
  }
}

/**
 * Apply backend `pr-statuses-updated` events for `repoPath` to the SWR cache.
 * `listen` resolves asynchronously, so an unmount can land before the
 * unlisten handle exists; the handle is then released as soon as it arrives.
 */
function usePrStatusesUpdatedListener(repoPath: string | undefined) {
  useEffect(() => {
    if (!repoPath) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<PrStatusesPayload>("pr-statuses-updated", (event) => {
      if (event.payload.repo_path !== repoPath) return;
      applyPrStatusesToQueryCache({
        repoPath,
        statuses: event.payload.statuses,
        ciStatuses: event.payload.ci_statuses,
      });
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [repoPath]);
}

export function useGitRemoteInfo(repoPath: string | undefined) {
  return useSWR(
    repoPath ? ["git-remote-info", repoPath] : null,
    () => getGitRemoteUrl(repoPath!),
    { dedupingInterval: 5 * 60 * 1000 },
  );
}

/**
 * Ensure the Rust background poller is watching this repo, and keep the
 * SWR cache in sync via `pr-statuses-updated` events. Cache reads
 * never shell out to `gh`, so the sidebar can refresh without UI stutter.
 */
export function usePrStatusPolling(repoPath: string | undefined) {
  useEffect(() => {
    if (!repoPath) return;
    void startPrStatusPolling(repoPath);
    return () => {
      void stopPrStatusPolling(repoPath);
    };
  }, [repoPath]);

  usePrStatusesUpdatedListener(repoPath);

  return useSWR(
    repoPath ? cachedPrStatusesKey(repoPath) : null,
    async () => {
      const [statuses, ciStatuses] = await Promise.all([
        listCachedPrStatuses(repoPath!),
        listCachedPrCiStatuses(repoPath!),
      ]);
      applyPrStatusesToQueryCache({
        repoPath: repoPath!,
        statuses,
        ciStatuses,
      });
      return statuses;
    },
    {
      dedupingInterval: 5_000,
      refreshInterval: pollMs(15_000),
    },
  );
}

/**
 * PR info for a branch, served from the Rust background cache.
 * Starts the poller if needed; never invokes `gh` from the UI thread.
 * Also queues an out-of-band PR+CI refresh for this branch so opening a
 * workspace does not wait for the next poll tick.
 */
export function usePrInfoViaGh(
  repoPath: string | undefined,
  branchName: string | undefined,
) {
  useEffect(() => {
    if (!repoPath) return;
    void startPrStatusPolling(repoPath);
    if (branchName) {
      void refreshPrBranchStatus(repoPath, branchName);
    }
  }, [repoPath, branchName]);

  usePrStatusesUpdatedListener(repoPath);

  return useSWR<PrInfo | null>(
    repoPath && branchName ? ["pr-info-gh", repoPath, branchName] : null,
    () => getCachedPrInfo(repoPath!, branchName!),
    {
      dedupingInterval: 5_000,
      refreshInterval: pollMs(15_000),
    },
  );
}

/** Force-refresh PR statuses after mutations (create PR, etc.). */
export async function invalidatePrStatuses(
  repoPath: string,
  branchName?: string,
) {
  try {
    if (branchName) {
      // allSettled so a failed checks fetch never skips applying the PR info.
      const [prInfo] = await Promise.allSettled([
        getPrInfoViaGh(repoPath, branchName),
        getPrChecksViaGh(repoPath, branchName),
      ]);
      if (prInfo.status === "fulfilled") {
        await setQueryData(["pr-info-gh", repoPath, branchName], prInfo.value);
      }
    } else {
      await refreshPrStatuses(repoPath);
    }
  } catch {
    // Cache warm is best-effort; background poller will catch up.
  }
  await invalidateQueries(cachedPrStatusesKey(repoPath));
  await invalidateQueries(["pr-info-gh", repoPath]);
  await invalidateQueries(cachedPrCiStatusesKey(repoPath));
  await invalidateQueries(["pr-ci-status", repoPath]);
}

export function usePrCiStatus(
  repoPath: string | undefined,
  branchName: string | undefined,
) {
  useEffect(() => {
    if (!repoPath) return;
    void startPrStatusPolling(repoPath);
    if (branchName) {
      void refreshPrBranchStatus(repoPath, branchName);
    }
  }, [repoPath, branchName]);

  usePrStatusesUpdatedListener(repoPath);

  return useSWR<PrCiStatus | null>(
    repoPath && branchName ? ["pr-ci-status", repoPath, branchName] : null,
    () => getCachedPrCiStatus(repoPath!, branchName!),
    {
      dedupingInterval: 10_000,
      refreshInterval: pollMs(30_000),
    },
  );
}

export function usePrChecksForPr(
  repoFullName: string | undefined,
  prNumber: number | undefined,
) {
  return useSWR<PrCiStatus | null>(
    repoFullName && prNumber !== undefined
      ? ["pr-ci-status-for-pr", repoFullName, prNumber]
      : null,
    () => getPrChecksForPr(repoFullName!, prNumber!),
    {
      dedupingInterval: 10_000,
      refreshInterval: pollMs(30_000),
    },
  );
}

export function useMergeQueueEnabled(repoPath: string | undefined) {
  const { data: remoteInfo } = useGitRemoteInfo(repoPath);
  const repoFullName = remoteInfo?.full_name;

  return useSWR<boolean>(
    FEATURES.mergeQueue && repoFullName
      ? mergeQueueEnabledKey(repoFullName)
      : null,
    async () => {
      const { data, error } = await supabase.rpc("get_merge_queue_enabled", {
        p_repo_full_name: repoFullName!,
      });
      if (error) throw error;
      return data === true;
    },
    { dedupingInterval: 60_000 },
  );
}

export function useSetMergeQueueEnabled(repoPath: string | undefined) {
  const { data: remoteInfo } = useGitRemoteInfo(repoPath);

  return useMutation({
    mutationFn: async (enabled: boolean) => {
      if (!remoteInfo) throw new Error("No GitHub remote detected");
      const { error } = await supabase.rpc("set_merge_queue_enabled", {
        p_repo_full_name: remoteInfo.full_name,
        p_enabled: enabled,
      });
      if (error) throw error;
      return enabled;
    },
    onSuccess: () => {
      void invalidateQueries(mergeQueueEnabledKey(remoteInfo?.full_name));
    },
  });
}

export function useDequeueBranches(repoPath: string | undefined) {
  const { data: remoteInfo } = useGitRemoteInfo(repoPath);

  return useMutation({
    mutationFn: async (branchNames: string[]) => {
      if (!remoteInfo) throw new Error("No GitHub remote detected");
      const fullName = remoteInfo.full_name;
      await [...branchNames].reverse().reduce(
        (prev, branchName) =>
          prev.then(async () => {
            const { error } = await supabase.functions.invoke(
              "enqueue-workspace",
              {
                body: {
                  repo_full_name: fullName,
                  branch_name: branchName,
                  action: "dequeue",
                },
              },
            );
            if (error) throw error;
          }),
        Promise.resolve(),
      );
      return branchNames;
    },
    onSuccess: () => {
      void invalidateQueries([
        "repo-branch-queue-statuses-panel",
        remoteInfo?.full_name,
      ]);
      void invalidateQueries([
        "repo-branch-queue-statuses",
        remoteInfo?.full_name,
      ]);
      void invalidateQueries(["merge-queue-status"]);
    },
  });
}

export function useMergeQueueStatus(
  repoPath: string | undefined,
  branchName: string | undefined,
) {
  const { data: remoteInfo } = useGitRemoteInfo(repoPath);
  const { data: queueEnabled } = useMergeQueueEnabled(repoPath);
  const repoFullName = remoteInfo?.full_name;

  return useSWR<WorkspaceQueueStatus | null>(
    FEATURES.mergeQueue && queueEnabled === true && repoFullName && branchName
      ? ["merge-queue-status", repoFullName, branchName]
      : null,
    async () => {
      if (!repoFullName || !branchName) return null;
      const { data, error } = await supabase.rpc("get_workspace_queue_status", {
        p_repo_full_name: repoFullName,
        p_branch_name: branchName,
      });
      if (error) throw error;
      return (data as WorkspaceQueueStatus[] | null)?.[0] ?? null;
    },
    { refreshInterval: pollMs(30_000) },
  );
}

export function useEnqueueWorkspace(
  repoPath: string | undefined,
  branchName: string | undefined,
) {
  const { data: remoteInfo } = useGitRemoteInfo(repoPath);
  const { data: prInfoGh, error: prInfoGhError } = usePrInfoViaGh(
    repoPath,
    branchName,
  );
  const { data: queueEnabled } = useMergeQueueEnabled(repoPath);

  const mutate = async (action: "enqueue" | "dequeue") => {
    if (!remoteInfo || !branchName)
      throw new Error("Repository or branch not detected");
    if (action === "enqueue" && !queueEnabled)
      throw new Error(
        "The merge queue is not enabled for this repository. Turn it on in the GitHub panel's Merge Queue tab.",
      );
    if (action === "enqueue" && prInfoGhError) throw prInfoGhError;

    if (prInfoGh !== undefined && prInfoGh !== null) {
      if (prInfoGh.state !== "OPEN" && action === "enqueue") {
        throw new Error(
          `No open PR found for branch '${branchName}' (gh reports: ${prInfoGh.state})`,
        );
      }
    }

    const { error } = await supabase.functions.invoke("enqueue-workspace", {
      body: {
        repo_full_name: remoteInfo.full_name,
        branch_name: branchName,
        action,
      },
    });
    if (error) throw error;

    void invalidateQueries([
      "merge-queue-status",
      remoteInfo.full_name,
      branchName,
    ]);
    void invalidateQueries([
      "repo-branch-queue-statuses",
      remoteInfo.full_name,
    ]);
    void invalidateQueries([
      "repo-branch-queue-statuses-panel",
      remoteInfo.full_name,
    ]);
  };

  const enqueue = useMutation({ mutationFn: () => mutate("enqueue") });
  const dequeue = useMutation({ mutationFn: () => mutate("dequeue") });

  return { enqueue, dequeue, remoteInfo, prInfoGh, prInfoGhError };
}
