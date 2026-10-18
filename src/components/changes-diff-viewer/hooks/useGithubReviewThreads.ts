import { useEffect, useRef, useState } from "react";
import useSWR from "swr";
import {
  useGitRemoteInfo,
  usePrInfoViaGh,
} from "../../../hooks/useMergeQueueStatus";
import { ghListPrReviewThreads } from "../../../lib/api";
import type { GhReviewThread } from "../../../lib/api-types";
import { pollMs } from "../../../lib/swr-cache";
import { placeGithubReviewThreads } from "../placeGithubReviewThreads";
import type { CommentLineQuery, FileHunksData } from "../types";

interface UseGithubReviewThreadsParams {
  repoPath: string | undefined;
  branchName: string | undefined;
  allFileHunks: Map<string, FileHunksData>;
  /** Committed PR/workspace hunks shown in the Changes tab. */
  committedFileHunks?: Map<string, FileHunksData>;
  /** A known PR; skips the remote and branch lookups (PR review screen). */
  pr?: { owner: string; repo: string; number: number };
}

/**
 * Read-only GitHub PR review comment threads for the current workspace's
 * branch. Threads are matched onto the *current* local diff (uncommitted and
 * committed Review-tab hunks) so they can render inline at the right line --
 * a thread that no longer matches any visible line is reported as "unplaced".
 */
export function useGithubReviewThreads({
  repoPath,
  branchName,
  allFileHunks,
  committedFileHunks,
  pr,
}: UseGithubReviewThreadsParams) {
  const { data: remoteInfo } = useGitRemoteInfo(pr ? undefined : repoPath);
  const { data: prInfo } = usePrInfoViaGh(
    pr ? undefined : repoPath,
    pr ? undefined : branchName,
  );
  const remoteOwner = pr?.owner ?? remoteInfo?.owner;
  const remoteRepo = pr?.repo ?? remoteInfo?.repo;
  const repoFullName = pr ? `${pr.owner}/${pr.repo}` : remoteInfo?.full_name;
  const prNumber = pr?.number ?? prInfo?.number;

  const { data: threads = [], mutate } = useSWR<GhReviewThread[]>(
    repoFullName != null && prNumber != null && remoteOwner && remoteRepo
      ? ["gh-pr-review-threads", repoFullName, prNumber]
      : null,
    () => {
      if (!remoteOwner || !remoteRepo || prNumber == null) {
        return Promise.resolve([]);
      }
      return ghListPrReviewThreads(remoteOwner, remoteRepo, prNumber);
    },
    {
      dedupingInterval: 60_000,
      refreshInterval: pollMs(2 * 60_000),
    },
  );

  const { threadsByLineKey, unplacedThreadsByFile } = (() => {
    const hunkMaps: Array<Map<string, FileHunksData>> = [allFileHunks];
    if (committedFileHunks && committedFileHunks.size > 0) {
      hunkMaps.push(committedFileHunks);
    }
    return placeGithubReviewThreads(threads, hunkMaps);
  })();

  const getThreadsForLine = ({
    filePath,
    hunkId,
    lineNumber,
    side,
  }: CommentLineQuery): GhReviewThread[] =>
    threadsByLineKey.get(`${filePath}:${hunkId}:${lineNumber}:${side}`) ?? [];

  const getUnplacedThreadsForFile = (filePath: string): GhReviewThread[] =>
    unplacedThreadsByFile.get(filePath) ?? [];

  // Resolved threads start collapsed; unresolved start expanded. Only seed
  // collapse state the first time a thread is seen, so manual toggles survive
  // background refetches.
  const [collapsedThreadIds, setCollapsedThreadIds] = useState<Set<string>>(
    new Set(),
  );
  const seenThreadIdsRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    const newlyResolved: string[] = [];
    for (const thread of threads) {
      if (!seenThreadIdsRef.current.has(thread.id)) {
        seenThreadIdsRef.current.add(thread.id);
        if (thread.is_resolved) newlyResolved.push(thread.id);
      }
    }
    if (newlyResolved.length > 0) {
      setCollapsedThreadIds((prev) => new Set([...prev, ...newlyResolved]));
    }
  }, [threads]);

  const toggleThreadCollapse = (threadId: string) => {
    setCollapsedThreadIds((prev) => {
      const next = new Set(prev);
      if (next.has(threadId)) next.delete(threadId);
      else next.add(threadId);
      return next;
    });
  };

  return {
    threads,
    refreshThreads: mutate,
    getThreadsForLine,
    getUnplacedThreadsForFile,
    collapsedThreadIds,
    toggleThreadCollapse,
  };
}
