import { useMemo } from "react";
import useSWR from "swr";
import { getWorkspaces, listWorkspaceStatuses } from "../lib/api";
import type { PrInfo } from "../lib/api-types";
import { pollMs } from "../lib/swr-cache";
import {
  buildWorkspaceTree,
  flattenWorkspaceTree,
  type WorkspaceSidebarStatusWithAgentDetail,
} from "../lib/workspace-tree";
import type { TerminalSessionSummary } from "../components/terminal/types";
import { useGitRemoteInfo, usePrStatusPolling } from "./useMergeQueueStatus";
import { useWorkspaceGitDetail } from "./useWorkspaceGitDetail";

/**
 * Data behind one repository's sidebar rows: workspaces, statuses, git detail,
 * PR info and agent streaming, merged into the flattened stack tree. Shared by
 * the React sidebar list and the GPUI sidebar sync so both read the same SWR
 * caches. Pass an undefined `cacheKey` to keep the hook idle.
 */
export function useWorkspaceSidebarRows({
  repoPath,
  cacheKey,
  terminalSessions,
}: {
  repoPath?: string;
  cacheKey?: string;
  terminalSessions?: TerminalSessionSummary[];
}) {
  const { data: workspaces = [], isLoading: workspacesPending } = useSWR(
    cacheKey ? ["workspaces", cacheKey] : null,
    () => getWorkspaces(repoPath || ""),
    // Agents create workspaces in supporting repositories with `treq add -r`
    // while another repository is on screen; nothing else refreshes them.
    { keepPreviousData: true, refreshInterval: pollMs(10000) },
  );
  const workspacesLoaded = workspacesPending === false && Boolean(repoPath);

  // Branches with an open agent terminal, and whether any of those sessions
  // is actively streaming -- drives the sidebar spinner's spin vs. idle pip.
  const streamingByBranchWithAgentSession = useMemo(() => {
    const branches = new Map<string, boolean>();
    for (const session of terminalSessions ?? []) {
      if (session.kind !== "agent" || !session.branchName) continue;
      branches.set(
        session.branchName,
        (branches.get(session.branchName) ?? false) || session.isStreaming,
      );
    }
    return branches;
  }, [terminalSessions]);

  // Shares its key with Dashboard.tsx's own plain `listWorkspaceStatuses`
  // poll, so this fetcher must stay a plain pass-through of that call --
  // see useWorkspaceGitDetail for the extra per-workspace detail this
  // sidebar needs on top of it.
  const { data: workspaceStatuses = [] } = useSWR(
    cacheKey && workspacesLoaded ? ["workspace-statuses", cacheKey] : null,
    () => listWorkspaceStatuses(repoPath || ""),
    { keepPreviousData: true },
  );
  const gitDetailById = useWorkspaceGitDetail({
    repoPath,
    cacheKey,
    enabled: workspacesLoaded,
    statuses: workspaceStatuses,
  });

  const { data: remoteInfo } = useGitRemoteInfo(repoPath);
  // Single Rust-backed cache for all workspace PR statuses — no per-row
  // `gh pr view` polling from the WebView.
  const { data: prStatusesByBranch = {} } = usePrStatusPolling(repoPath);

  const statuses: WorkspaceSidebarStatusWithAgentDetail[] = (() => {
    const statusById = new Map(
      (workspaceStatuses ?? []).map((status) => [status.current.id, status]),
    );
    return (workspaces ?? []).map((workspace) => {
      const status = statusById.get(workspace.id);
      const detail = gitDetailById?.get(workspace.id);
      return {
        ...(status ?? { current: workspace, has_conflicts: false }),
        ...detail,
      };
    });
  })();

  const flattenedNodes = (() => {
    const isMergedBranch = (branchName: string) =>
      (prStatusesByBranch as Record<string, PrInfo | null>)[branchName]
        ?.state === "MERGED";
    const tree = buildWorkspaceTree(statuses, isMergedBranch);
    return flattenWorkspaceTree(tree);
  })();

  return {
    workspaces,
    workspacesPending,
    statuses,
    flattenedNodes,
    remoteInfo,
    prStatusesByBranch: prStatusesByBranch as Record<string, PrInfo | null>,
    streamingByBranchWithAgentSession,
  };
}
