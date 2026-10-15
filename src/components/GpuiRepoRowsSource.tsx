import { useEffect } from "react";
import type { PrInfo } from "../lib/api-types";
import type { Workspace } from "../lib/api";
import { buildGpuiRepoSection, NO_GPUI_SELECTION } from "../lib/gpui-shell";
import { useWorkspaceSidebarRows } from "../hooks/useWorkspaceSidebarRows";
import type { TerminalSessionSummary } from "./terminal/types";

/** What the GPUI sidebar needs to act on a row of a repository off screen. */
export interface GpuiRepoLookup {
  workspaces: Workspace[];
  prStatusesByBranch: Record<string, PrInfo | null>;
}

/**
 * Loads the rows of one repository that is not on screen and reports its
 * GPUI sidebar section, as JSON so unchanged data does not re-render the
 * caller. Renders nothing.
 */
export function GpuiRepoRowsSource({
  path,
  terminalSessions,
  onSection,
  onLookup,
}: {
  path: string;
  terminalSessions: TerminalSessionSummary[];
  onSection: (path: string, section: string | null) => void;
  onLookup: (path: string, lookup: GpuiRepoLookup | null) => void;
}) {
  const rows = useWorkspaceSidebarRows({
    repoPath: path,
    cacheKey: path,
    terminalSessions,
  });
  const section = JSON.stringify(
    buildGpuiRepoSection(
      {
        path,
        // The React sidebar shows the branch only for the repository on screen.
        homeRef: null,
        active: false,
        loading: rows.workspacesPending && rows.flattenedNodes.length === 0,
        nodes: rows.flattenedNodes,
        streamingByBranchWithAgentSession:
          rows.streamingByBranchWithAgentSession,
        prStatusesByBranch: rows.prStatusesByBranch,
        hasRemote: !!rows.remoteInfo,
        queueStatusesByBranch: rows.branchQueueStatuses,
      },
      NO_GPUI_SELECTION,
    ),
  );

  useEffect(() => {
    onSection(path, section);
  }, [path, section, onSection]);

  const { workspaces, prStatusesByBranch } = rows;
  useEffect(() => {
    onLookup(path, { workspaces, prStatusesByBranch });
  }, [path, workspaces, prStatusesByBranch, onLookup]);

  useEffect(
    () => () => {
      onSection(path, null);
      onLookup(path, null);
    },
    [path, onSection, onLookup],
  );

  return null;
}
