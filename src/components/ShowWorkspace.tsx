/* eslint-disable max-lines, max-params */

import useSWR from "swr";
import { useMutation } from "../hooks/useMutation";
import { invalidateQueries } from "../lib/swr-cache";
import {
  AlertTriangle,
  ArrowRight,
  Code2,
  Copy,
  File,
  FileDiff,
  Folder,
  FolderGit2,
  GitBranch,
  GitCommitHorizontal,
  GitCompareArrows,
  Info,
  Layers2,
  Loader2,
  MoreVertical,
  RefreshCw,
  Search,
  Trash2,
  Upload,
  Database,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useGitRemoteInfo, usePrCiStatus } from "../hooks/useMergeQueueStatus";
import { useTerminalSettingsStore } from "../stores/terminalSettingsStore";
import { useTreqSendStore } from "../stores/treqSendStore";
import { listen } from "@tauri-apps/api/event";
import {
  createSession,
  type DirectoryEntry,
  jjRestoreAll,
  jjRestoreSnapshot,
  jjSnapshotWorkingCopy,
  dryRunHomeRepoRebase,
  getRepoSetting,
  getSetting,
  getWorkspaceChangedFiles,
  getWorkspaceDiff,
  getWorkspaceReadme,
  getWorkspaceStartingPrompt,
  getWorkspaceStatus,
  type JjLogResult,
  listCommits,
  lsWorkspaceWithStatus,
  pullWorkspaceFromRemote,
  pushWorkspaceToRemote,
  rebaseHomeRepoBranch,
  setGitSubmoduleSynced,
  updateWorkspace,
  type Workspace,
} from "../lib/api";
import { usePreviewFeature } from "../stores/featurePreviewStore";
import { getStatusBgColor } from "../lib/git-status-colors";
import { useRepositoryCacheKey } from "../lib/active-repository-context";
import {
  countUniqueReviewChangePaths,
  type ParsedFileChange,
} from "../lib/git-utils";
import { reviewChangeCountQueryKey } from "../lib/review-change-count";
import {
  REFRESH_WORKSPACE_CHANGES_EVENT,
  scheduleRefreshWorkspaceChanges,
} from "../lib/change-file-drag";
import {
  type WorkspaceChangesRefreshDetail,
  visibleWorkspaceRefreshTarget,
} from "../lib/workspace-refresh";
import { getReviewTabPill, reviewTabPillClassName } from "../lib/reviewTabPill";
import {
  commitsTabCountClassName,
  getCommitsTabLabel,
} from "../lib/commitsTabLabel";
import { cn, getFullWorkspacePath, resolveReadmeImageSrc } from "../lib/utils";
import { sumWorkspaceLocFromLog } from "../lib/workspace-stack";
import { REMOTE_FILE_TREE_REASON } from "../lib/remote-capabilities";
import { RemoteOperationUnsupportedError } from "../lib/repository-adapter";
import type { AgentReviewComment } from "../lib/api-types-review";
import type { SessionCreationInfo } from "../types/sessions";
import { ChangesDiffViewer } from "./changes-diff-viewer/ChangesDiffViewerMain";
import { BrowserPanel } from "./browser-panel/BrowserPanel";
import { useReviewSubView } from "./browser-panel/useReviewSubView";
import { LocDiffMarker } from "./LocDiffMarker";
import { type LogSource, LogsTab } from "./LogsTab";
import { CommitDiffViewer } from "./CommitDiffViewer";
import { CreatePrButtonGroup } from "./CreatePrButtonGroup";
import { FileBrowser } from "./FileBrowser";
import { LinearCommitHistory } from "./LinearCommitHistory";
import { MarkdownContent } from "./MarkdownContent";
import {
  type BranchListItem,
  TargetBranchSelector,
} from "./TargetBranchSelector";
import { TaskInput } from "./TaskInput";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { Kbd, KbdGroup } from "./ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";
import { WorkspaceDescription } from "./WorkspaceDescription";
import { useGpuiWorkspaceScreen } from "./useGpuiWorkspaceScreen";
import { GpuiPartsProvider, useGpuiPartsHost } from "../lib/gpui-parts";
import type { GpuiWorkspaceTab } from "../lib/gpui-screen";
import { Tabs, TabsList, TabsTrigger } from "./ui/tabs";
import { useToast } from "./ui/toast";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "./ui/tooltip";
import { ViewPrButton } from "./ViewPrButton";
import { WorkspaceStackPanel } from "./WorkspaceStackPanel";
import { MergeQueueButton } from "./workspace-header/MergeQueueButton";
import { WorkspaceChecksButton } from "./workspace-header/WorkspaceChecksButton";
import { checksTargetOf } from "../hooks/useWorkspaceChecks";
import {
  clearOpenSystemLogsRequest,
  clearOpenWorkspaceLogsRequest,
  useOpenLogsRequest,
} from "../lib/open-logs-request";
import {
  type WorkspaceTrackerMetadata,
  WorkspaceTrackerBadges,
} from "./workspace-header/WorkspaceTrackerBadges";
import type { TerminalSessionSummary } from "./terminal/types";
import { toAgentKind, type AgentKind } from "../lib/agents";
import { agentSessionIdOf } from "./terminal/agentTerminalId";

interface ShowWorkspaceProps {
  repositoryPath?: string;
  workspace: Workspace | null;
  mainRepoBranch?: string | null;
  initialSelectedFile: string | null;
  onDeleteWorkspace?: (workspace: Workspace) => void;
  onOpenFilePicker?: () => void;
  onSessionCreated?: (session: SessionCreationInfo) => void;
  idleAgentSession?: TerminalSessionSummary;
  onSendToIdleAgent?: (sessionId: number, prompt: string) => void;
  /** Called when the user clicks "View full prompt" on the workspace's starting prompt */
  onViewFullPrompt?: (promptId: number) => void;
  onOpenMergePreview?: () => void;
  onOpenBranchSwitcher?: () => void;
  onCreateStackedWorkspace?: () => void;
  /** Called when the user clicks a sibling workspace in the stack panel */
  onNavigateToWorkspace?: (workspace: Workspace) => void;
  onViewPrInApp?: (prNumber: number, prState: string) => void;
  /** Called when user wants to move a commit to a new workspace */
  onMoveCommitToNewWorkspace?: (
    commit: import("../lib/api").JjLogCommit,
    workspace: Workspace | null,
  ) => void;
  /** Called when user wants to move a commit to an existing workspace */
  onMoveCommitToExistingWorkspace?: (
    commit: import("../lib/api").JjLogCommit,
    workspace: Workspace | null,
  ) => void;
  /** Called after a commit is stashed from the Commits tab */
  onCommitStashed?: () => void;
  /** Called when user wants to move files to a new workspace */
  onMoveFilesToNewWorkspace?: (
    files: string[],
    workspace: Workspace | null,
  ) => void;
  onActiveTabChange?: (tab: string) => void;
  availableBranches?: BranchListItem[];
  branchesLoading?: boolean;
  onLoadAvailableBranches?: () => void | Promise<void>;
  /**
   * Under the GPUI shell, draw the header and tab row in GPUI. Off while
   * another page covers the workspace or the terminal pane fills it.
   */
  gpuiActive?: boolean;
  /** Under GPUI's Code tab: how tall the terminal pane it frames below is. */
  gpuiFrameHeight?: number | null;
  /**
   * The height GPUI's Code tab takes, which the terminal pane's layer spans
   * so the pane sits in GPUI's frame; null while React draws the tab.
   */
  onGpuiContentHeight?: (height: number | null) => void;
}

const StatusPip = ({ status }: { status?: string }) =>
  status ? (
    <span
      className={cn(
        "w-2 h-2 rounded-full flex-shrink-0",
        getStatusBgColor(status),
      )}
    />
  ) : null;

// jj-derived file status from core::files::list_workspace_files /
// core::ls_workspace: "conflict" | "workingCopy" | "committed", or absent
// (untouched). Conflicts are red, committed-but-not-working-copy changes are
// blue, and working-copy changes are yellow.
const JJ_STATUS_PIP_CLASSES: Record<string, string> = {
  conflict: "bg-red-500",
  committed: "bg-blue-500",
  workingCopy: "bg-yellow-500",
};

const JjStatusPip = ({ status }: { status?: string | null }) => {
  const pipClass = status ? JJ_STATUS_PIP_CLASSES[status] : undefined;
  return pipClass ? (
    <span className={cn("w-2 h-2 rounded-full flex-shrink-0", pipClass)} />
  ) : null;
};

export const ShowWorkspace = ({
  repositoryPath,
  workspace,
  mainRepoBranch,
  initialSelectedFile,
  onDeleteWorkspace,
  onOpenFilePicker,
  onSessionCreated,
  idleAgentSession,
  onSendToIdleAgent,
  onViewFullPrompt,
  onOpenMergePreview,
  onOpenBranchSwitcher,
  onCreateStackedWorkspace,
  onNavigateToWorkspace,
  onViewPrInApp,
  onMoveCommitToNewWorkspace,
  onMoveCommitToExistingWorkspace,
  onCommitStashed,
  onMoveFilesToNewWorkspace,
  onActiveTabChange,
  availableBranches = [],
  branchesLoading = false,
  onLoadAvailableBranches,
  gpuiActive = false,
  gpuiFrameHeight = null,
  onGpuiContentHeight,
}: ShowWorkspaceProps) => {
  const workingDirectory = workspace
    ? getFullWorkspacePath(workspace)
    : repositoryPath || "";
  const effectiveRepoPath = workspace?.repo_path || repositoryPath || "";
  const repoCacheKey = useRepositoryCacheKey(effectiveRepoPath);

  const { addToast } = useToast();
  const logsEnabled = usePreviewFeature("logs");
  const gpuiParts = useGpuiPartsHost();
  const checksTarget = checksTargetOf(workspace);
  const { fontSize } = useTerminalSettingsStore();

  const { data: remoteInfo } = useGitRemoteInfo(effectiveRepoPath || undefined);
  const { data: mergeButtonCiStatus } = usePrCiStatus(
    effectiveRepoPath || undefined,
    workspace?.branch_name,
  );
  const ciAllPassing =
    !!mergeButtonCiStatus &&
    mergeButtonCiStatus.total > 0 &&
    mergeButtonCiStatus.state === "success";

  const [changedFiles, setChangedFiles] = useState<
    Map<string, ParsedFileChange>
  >(new Map());
  const [initialSelectedFileForBrowser, setInitialSelectedFileForBrowser] =
    useState<string | null>(null);
  const [initialExpandedDir, setInitialExpandedDir] = useState<string | null>(
    null,
  );

  const [actionPending, _setActionPending] = useState<"push" | "sync" | null>(
    null,
  );

  // Target branch and conflicts state
  const [targetBranch, setTargetBranch] = useState<string | null>(null);
  const defaultBranch = "main";
  const defaultTargetBranch = mainRepoBranch || defaultBranch;

  // Committed changes toggle state
  const [showCommittedChanges, setShowCommittedChanges] = useState(true);

  // Home-repo branch divergence: counts of target-ahead commits and conflict dry-run result
  const [homeRepoTargetAheadCount, setHomeRepoTargetAheadCount] = useState(0);
  const [homeRebasing, setHomeRebasing] = useState(false);

  const [rebasing, setRebasing] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [refreshingFiles, setRefreshingFiles] = useState(false);

  // Show overview tab by default for main repo, changes tab for workspaces
  const [activeTab, setActiveTab] = useState("overview");
  const reviewSubView = useReviewSubView({
    activeTab,
    setActiveTab,
    treqSendAssets: useTreqSendStore((s) => s.assets),
    dismissTreqSendAsset: useTreqSendStore((s) => s.dismissAsset),
  });
  const [scrollToCommitId, setScrollToCommitId] = useState<string | null>(null);
  const [showFileBrowserInCode, setShowFileBrowserInCode] = useState(false);

  useEffect(() => {
    if (!logsEnabled && activeTab === "logs") {
      setActiveTab("overview");
    }
  }, [activeTab, logsEnabled]);

  const handleChangedFilesUpdate = (parsedFiles: ParsedFileChange[]) => {
    const map = new Map<string, ParsedFileChange>();
    for (const file of parsedFiles) {
      const fullPath = `${workingDirectory}/${file.path}`;
      map.set(fullPath, file);
    }
    setChangedFiles(map);
  };

  useEffect(() => {
    setActiveTab("overview");
    setChangedFiles(new Map());
  }, [workspace?.id]);

  // Bumped by "open in Logs" links so the Logs tab remounts on the requested
  // source: check logs for all checks, or the app's system logs.
  const [logsKey, setLogsKey] = useState(0);
  const [logsSource, setLogsSource] = useState<LogSource>("checks");
  const openLogs = (source: LogSource) => {
    setLogsSource(source);
    setLogsKey((key) => key + 1);
    setActiveTab("logs");
  };
  const openCheckLogs = () => openLogs("checks");

  // Runs after the reset above, so a Logs request for this workspace wins.
  const openLogsFor = useOpenLogsRequest((state) => state.workspaceId);
  useEffect(() => {
    if (openLogsFor === null || openLogsFor !== workspace?.id) return;
    clearOpenWorkspaceLogsRequest();
    if (logsEnabled) openCheckLogs();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [openLogsFor, workspace?.id, logsEnabled]);

  const openSystemLogs = useOpenLogsRequest((state) => state.system);
  useEffect(() => {
    if (!openSystemLogs) return;
    clearOpenSystemLogsRequest();
    if (logsEnabled) openLogs("system");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [openSystemLogs, logsEnabled]);

  // After a child refresh (e.g. post-commit), re-check if workspace now has commits
  const handleRefreshingChange = (r: boolean) => {
    setRefreshingFiles(r);
    if (!r && workspace?.id !== undefined) {
      void invalidateQueries(["workspace-commits", repoCacheKey, workspace.id]);
    }
  };

  // When commits are loaded from LinearCommitHistory, capture divergence counts
  const handleCommitsLoaded = (result: JjLogResult) => {
    const targetAheadCount =
      result.commits?.filter((commit) => commit.on_target_only).length ?? 0;
    setHomeRepoTargetAheadCount(targetAheadCount);
  };

  useEffect(() => {
    if (!workspace) {
      setTargetBranch(null);
      return;
    }
    setTargetBranch(workspace.target_branch ?? defaultTargetBranch);
  }, [workspace?.id, workspace?.target_branch, defaultTargetBranch]);

  const isHomeRepo = !workspace;
  const { data: homeRebaseDryRun = null } = useSWR(
    isHomeRepo &&
      effectiveRepoPath &&
      homeRepoTargetAheadCount > 0 &&
      mainRepoBranch
      ? [
          "home-rebase-dry-run",
          effectiveRepoPath,
          mainRepoBranch,
          defaultBranch,
          homeRepoTargetAheadCount,
        ]
      : null,
    () =>
      dryRunHomeRepoRebase(effectiveRepoPath, mainRepoBranch!, defaultBranch),
  );

  useEffect(() => {
    onActiveTabChange?.(activeTab);
  }, [activeTab, onActiveTabChange]);

  const {
    data: workspaceStatusData,
    isLoading: workspaceStatusPending,
    mutate: refetchWorkspaceStatus,
  } = useSWR(
    effectiveRepoPath
      ? ["workspace-status", repoCacheKey, workspace?.id ?? null]
      : null,
    () => getWorkspaceStatus(effectiveRepoPath, workspace?.id ?? null),
  );

  // Derive from the status query — copying into local state lagged a render
  // behind resolve+commit refetches and kept the Review Conflicts section /
  // Code-tab alert visible after the backend had already cleared them.
  const conflictedFiles = workspaceStatusData?.conflicted_files ?? [];
  const normalizedConflictedFiles = (() => {
    const seen = new Set<string>();
    const normalized: string[] = [];
    for (const path of conflictedFiles) {
      const trimmed = path.trim();
      if (!trimmed || seen.has(trimmed)) continue;
      seen.add(trimmed);
      normalized.push(trimmed);
    }
    return normalized;
  })();
  const conflictCount = normalizedConflictedFiles.length;

  // Ahead/behind counts against the remote.
  const syncStatus = (() => {
    const sync = workspaceStatusData?.remote_sync;
    if (!sync) return null;
    if (sync.type === "Ahead") return { ahead: sync.data.count, behind: 0 };
    if (sync.type === "Behind") return { ahead: 0, behind: sync.data.count };
    if (sync.type === "Diverged") {
      return { ahead: sync.data.ahead, behind: sync.data.behind };
    }
    return { ahead: 0, behind: 0 };
  })();

  // Review badge: unique working-copy + committed files, independent of
  // whether the Changes tab (ChangesDiffViewer) is mounted. Mounting Review
  // must not change this number.
  const includeCommittedInReviewCount =
    Boolean(workspace) && workspace!.branch_name !== defaultTargetBranch;
  const reviewWorkspaceId = workspace?.id ?? null;
  const { data: reviewChangeCount = 0, isLoading: reviewChangeCountPending } =
    useSWR<number>(
      effectiveRepoPath
        ? [
            ...reviewChangeCountQueryKey(effectiveRepoPath, reviewWorkspaceId),
            includeCommittedInReviewCount,
            defaultTargetBranch,
          ]
        : null,
      async (queryKey) => {
        const repoPath = queryKey[1] as string | undefined;
        const workspaceId = queryKey[2] as number | null;
        const includeCommitted = queryKey[3] as boolean;
        if (!repoPath) return 0;
        if (includeCommitted && workspaceId !== null) {
          const diff = await getWorkspaceDiff(repoPath, workspaceId);
          return countUniqueReviewChangePaths(
            diff.uncommitted_files ?? [],
            diff.committed_files ?? [],
          );
        }
        const files = await getWorkspaceChangedFiles(repoPath, workspaceId);
        return countUniqueReviewChangePaths(files);
      },
    );
  const visibleReviewChangeCount = reviewChangeCountPending
    ? 0
    : reviewChangeCount;

  // Workspace LOC (committed + working copy) for the Gerrit-style marker
  // on the tab row. Shares the stack panel's query key for cache reuse.
  const { data: workspaceCommitsLog } = useSWR(
    effectiveRepoPath && workspace?.id !== undefined
      ? ["workspace-commits", repoCacheKey, workspace?.id ?? null]
      : null,
    () => listCommits(effectiveRepoPath, workspace!.id),
  );
  const workspaceLocStats = workspaceCommitsLog
    ? sumWorkspaceLocFromLog(workspaceCommitsLog)
    : undefined;
  // Whether the workspace has any of its own commits (not target-only, not
  // working-copy). Controls the Committed toggle and Create PR availability.
  const hasWorkspaceCommits = (workspaceCommitsLog?.commits ?? []).some(
    (c) => !c.on_target_only && !c.is_working_copy,
  );

  const reviewTabPill = (() => {
    if (visibleReviewChangeCount <= 0) return null;
    const derived = getReviewTabPill({
      conflictCount,
      uncommittedCount: changedFiles.size,
      hasUncommittedFromStatus: workspaceStatusData?.has_changes ?? false,
      committedFileCount: visibleReviewChangeCount,
      commitsAheadCount:
        workspaceStatusData?.commits_ahead_of_target?.length ?? 0,
      hasWorkspaceCommits,
    });
    return {
      tone: derived?.tone ?? "committed",
      count: visibleReviewChangeCount,
    };
  })();

  const commitsTabLabel = (() => {
    const commitCount =
      workspaceStatusData?.commits_ahead_of_target?.length ?? 0;
    const hasConflict =
      conflictCount > 0 || Boolean(workspaceStatusData?.has_conflicts);
    return getCommitsTabLabel({
      isHomeRepo,
      commitCount,
      hasConflict,
    });
  })();

  const { data: overviewData, isLoading: overviewPending } = useSWR(
    effectiveRepoPath && activeTab === "overview"
      ? ["workspace-overview", repoCacheKey, workspace?.id ?? null]
      : null,
    async () => {
      try {
        const [entries, readme] = await Promise.all([
          lsWorkspaceWithStatus(effectiveRepoPath, workspace?.id ?? null),
          getWorkspaceReadme(effectiveRepoPath, workspace?.id ?? null),
        ]);
        return { entries, readme };
      } catch (error) {
        const unavailable = error instanceof RemoteOperationUnsupportedError;
        if (!unavailable) {
          console.error("Failed to load workspace overview:", error);
        }
        return { entries: [], readme: null as string | null, unavailable };
      }
    },
  );

  const { data: startingPromptEntry } = useSWR(
    // Only needed in the Details popover; fetch when it opens.
    detailsOpen && effectiveRepoPath && workspace?.id !== undefined
      ? ["workspace-starting-prompt", repoCacheKey, workspace?.id ?? null]
      : null,
    () => getWorkspaceStartingPrompt(effectiveRepoPath, workspace!.id),
  );

  const handleCopyStartingPrompt = async () => {
    if (!startingPromptEntry?.prompt_text) return;
    try {
      await navigator.clipboard.writeText(startingPromptEntry.prompt_text);
      addToast({
        title: "Copied",
        description: "Starting prompt copied to clipboard",
        type: "success",
      });
    } catch (error) {
      addToast({
        title: "Failed to copy",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    }
  };

  const rootEntries = overviewData?.entries ?? [];
  const readmeContent = overviewData?.readme ?? null;
  const readmeBaseDir = workspace
    ? getFullWorkspacePath(workspace)
    : effectiveRepoPath;

  const submoduleSync = useMutation({
    mutationFn: ({ path, enabled }: { path: string; enabled: boolean }) =>
      setGitSubmoduleSynced(effectiveRepoPath, path, enabled),
    onSuccess: () => {
      void invalidateQueries([
        "workspace-overview",
        effectiveRepoPath,
        workspace?.id ?? null,
      ]);
    },
  });

  useEffect(() => {
    if (workspace?.id === undefined || !effectiveRepoPath) return;
    const workspaceId = workspace.id;
    const unlisten = listen<{
      workspace_id: number;
      changed_paths?: string[];
    }>("workspace-files-changed", (event) => {
      if (event.payload.workspace_id !== workspaceId) return;
      scheduleRefreshWorkspaceChanges({
        workspaceId,
        changedPaths: event.payload.changed_paths,
      });
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [workspace?.id, effectiveRepoPath]);

  useEffect(() => {
    const handler = (event: Event) => {
      const { detail } = event as CustomEvent<WorkspaceChangesRefreshDetail>;
      if (
        detail?.workspaceId !== undefined &&
        workspace?.id !== undefined &&
        detail.workspaceId !== workspace.id
      ) {
        return;
      }
      const target = visibleWorkspaceRefreshTarget({
        activeTab,
        showFileBrowser: showFileBrowserInCode,
      });
      if (target === "changes-diff" && workspace?.id !== undefined) {
        // Diff reload is handled by ChangesDiffViewer; refresh status metadata
        // so the Review pill tone and overview conflict alerts stay in sync.
        void invalidateQueries([
          "workspace-status",
          effectiveRepoPath,
          workspace.id,
        ]);
        void invalidateQueries(["workspace-statuses", repoCacheKey]);
        void invalidateQueries(
          reviewChangeCountQueryKey(effectiveRepoPath, workspace.id),
        );
      }
      if (target === "commits-list") {
        void invalidateQueries([
          "commit-diff-viewer-commits",
          effectiveRepoPath,
          workspace?.id ?? null,
        ]);
        void invalidateQueries([
          "workspace-commits",
          effectiveRepoPath,
          workspace?.id ?? null,
        ]);
      }
    };
    window.addEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, handler);
    return () => {
      window.removeEventListener(REFRESH_WORKSPACE_CHANGES_EVENT, handler);
    };
  }, [activeTab, showFileBrowserInCode, workspace?.id, effectiveRepoPath]);

  // Handle file selection from Cmd+P (or other external sources)
  useEffect(() => {
    if (initialSelectedFile) {
      // FilePicker returns a repository-relative path, while FileBrowser's
      // read and metadata APIs require the full path in the active workspace.
      const selectedFile = initialSelectedFile.startsWith("/")
        ? initialSelectedFile
        : `${effectiveRepoPath.replace(/\/$/, "")}/${initialSelectedFile}`;
      setInitialSelectedFileForBrowser(selectedFile);
      // Extract parent directory from file path
      const parentDir = selectedFile.substring(
        0,
        selectedFile.lastIndexOf("/"),
      );
      setInitialExpandedDir(parentDir);
      setShowFileBrowserInCode(true);
    }
  }, [initialSelectedFile, effectiveRepoPath]);

  const handleTargetBranchSelect = async (branch: string) => {
    if (branch === targetBranch || !workspace) return;

    setRebasing(true);
    try {
      // Cycle-safe retarget (bridge lift) lives in Rust core so the
      // CLI and UI share one plan via update_workspace / retarget_workspace.
      await updateWorkspace(effectiveRepoPath, workspace.id, branch);

      addToast({
        title: "Rebased successfully",
        description: `Workspace rebased onto ${branch}`,
        type: "success",
      });

      // Invalidate sidebar queries so hierarchy updates
      void invalidateQueries(["workspaces", repoCacheKey]);
      void invalidateQueries(["workspace-statuses", repoCacheKey]);
      void invalidateQueries([
        "workspace-status",
        effectiveRepoPath,
        workspace.id,
      ]);

      setTargetBranch(branch);
    } catch (error) {
      addToast({
        title: "Rebase failed",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setRebasing(false);
    }
  };

  // Helper to get status for a directory entry (legacy M/A/D/R fallback,
  // used only when the entry has no jj status from core::ls_workspace).
  const getEntryStatus = (entry: DirectoryEntry): string | undefined => {
    const fullPath = `${workingDirectory}/${entry.name}`;
    if (!entry.is_directory) {
      const file = changedFiles.get(fullPath);
      if (!file) return undefined;
      // Prefer workspaceStatus (unstaged) over stagedStatus
      return file.workspaceStatus || file.stagedStatus || undefined;
    }
    // For directories, check if any child has changes
    for (const [path] of changedFiles) {
      if (path.startsWith(`${fullPath}/`)) {
        return "M"; // Show modified indicator if any child changed
      }
    }
    return undefined;
  };

  // Handler for clicking on Overview entries
  const handleOverviewEntryClick = (entry: DirectoryEntry) => {
    const fullPath = `${workingDirectory}/${entry.name}`;
    if (entry.is_directory) {
      setInitialExpandedDir(fullPath);
      setInitialSelectedFileForBrowser(null); // Will select README in browser
    } else {
      setInitialSelectedFileForBrowser(fullPath);
      setInitialExpandedDir(null);
    }
    setShowFileBrowserInCode(true);
  };

  const handlePushToRemote = async () => {
    if (!effectiveRepoPath) return;

    _setActionPending("push");

    try {
      await pushWorkspaceToRemote(effectiveRepoPath, workspace?.id ?? null);

      addToast({
        title: "Pushed to remote",
        description: "Changes pushed successfully",
        type: "success",
      });
      // Refresh sync status after push
      await refetchWorkspaceStatus();
      void invalidateQueries();
    } catch (error) {
      console.error("Push failed:", error);
      addToast({
        title: "Push failed",
        description: String(error),
        type: "error",
      });
    } finally {
      _setActionPending(null);
    }
  };

  const handleSync = async () => {
    if (!effectiveRepoPath) return;

    _setActionPending("sync");
    try {
      const pullResult = await pullWorkspaceFromRemote(
        effectiveRepoPath,
        workspace?.id ?? null,
      );
      await pushWorkspaceToRemote(effectiveRepoPath, workspace?.id ?? null);

      if (pullResult.has_conflicts) {
        addToast({
          title: "Synced with conflicts",
          description:
            "Fetched and pushed; resolve remaining conflicts locally",
          type: "warning",
        });
      } else {
        addToast({
          title: "Synced with remote",
          description: "Fetched and pushed changes",
          type: "success",
        });
      }
    } catch (error) {
      console.error("Sync failed:", error);
      addToast({
        title: "Sync failed",
        description: String(error),
        type: "error",
      });
    } finally {
      // Always refresh so conflicted_files / sync counts reflect pull outcome,
      // including when push fails after a divergent pull.
      await refetchWorkspaceStatus();
      void invalidateQueries();
      _setActionPending(null);
    }
  };

  const handleViewTentativeChanges = () => {
    setActiveTab("changes");
  };

  const handleDeleteTentativeChanges = async () => {
    if (!workspace?.workspace_path || !effectiveRepoPath) return;

    const refreshAfterRestore = () =>
      Promise.all([
        refetchWorkspaceStatus(),
        invalidateQueries([
          "commit-diff-viewer-commits",
          effectiveRepoPath,
          workspace.id,
        ]),
        invalidateQueries([
          "workspace-status",
          effectiveRepoPath,
          workspace.id,
        ]),
        invalidateQueries([
          "workspace-overview",
          effectiveRepoPath,
          workspace.id,
        ]),
      ]);

    try {
      const snapshotId = await jjSnapshotWorkingCopy(workingDirectory);
      await jjRestoreAll(workingDirectory);
      addToast({
        title: "Changes discarded",
        description: "Working copy changes were removed",
        type: "success",
        action: {
          label: "Undo",
          onClick: () => {
            void (async () => {
              try {
                await jjRestoreSnapshot(workingDirectory, snapshotId);
                await refreshAfterRestore();
                addToast({
                  title: "Restored",
                  description: "Working copy changes were restored",
                  type: "success",
                });
              } catch (undoError) {
                addToast({
                  title: "Undo Failed",
                  description:
                    undoError instanceof Error
                      ? undoError.message
                      : String(undoError),
                  type: "error",
                });
              }
            })();
          },
        },
      });
      await refreshAfterRestore();
    } catch (error) {
      addToast({
        title: "Failed to discard changes",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    }
  };

  const handleHomeRebase = async () => {
    if (!effectiveRepoPath || !mainRepoBranch) return;
    setHomeRebasing(true);
    try {
      const result = await rebaseHomeRepoBranch(
        effectiveRepoPath,
        mainRepoBranch,
        defaultBranch,
      );
      if (result.success) {
        addToast({
          title: "Rebase complete",
          description: result.message || "Branch rebased onto target",
          type: "success",
        });
        void invalidateQueries();
        // Reset divergence state — LinearCommitHistory will refetch
        setHomeRepoTargetAheadCount(0);
      } else {
        addToast({
          title: "Rebase failed",
          description: result.message,
          type: "error",
        });
      }
    } catch (error) {
      addToast({
        title: "Rebase failed",
        description: String(error),
        type: "error",
      });
    } finally {
      setHomeRebasing(false);
    }
  };

  const toRelativePath = (filePath: string) =>
    filePath.startsWith(`${workingDirectory}/`)
      ? filePath.slice(workingDirectory.length + 1)
      : filePath;

  /** Sends to the idle agent in this workspace, if any. Returns false when none. */
  const sendToIdleAgent = (prompt: string) => {
    if (!idleAgentSession || !onSendToIdleAgent) return false;
    const sessionId = agentSessionIdOf(idleAgentSession.id);
    if (sessionId === null) return false;
    onSendToIdleAgent(sessionId, prompt);
    return true;
  };

  /**
   * Creates a session record and hands it to the parent, which opens the agent
   * terminal and sends `pendingPrompt` once the agent initializes.
   */
  const startAgentSession = async (
    sessionName: string,
    pendingPrompt: string,
    permissionMode?: "plan" | "acceptEdits",
    agent?: SessionCreationInfo["agent"],
  ) => {
    const dbSessionId = await createSession(
      effectiveRepoPath,
      workspace?.id ?? null,
      sessionName,
    );
    onSessionCreated?.({
      sessionId: dbSessionId,
      sessionName,
      workspaceId: workspace?.id ?? null,
      workspacePath: null,
      repoPath: effectiveRepoPath || workingDirectory,
      pendingPrompt,
      permissionMode,
      ...(agent ? { agent } : {}),
    });
  };

  const toastError = (title: string, error: unknown) =>
    addToast({
      title,
      description: error instanceof Error ? error.message : String(error),
      type: "error",
    });

  const handleCreateAgentWithComment = async (
    filePath: string,
    startLine: number,
    endLine: number,
    lineContent: string[],
    commentText: string,
    commitShortId?: string,
    mode?: "plan" | "acceptEdits",
  ) => {
    const lineRef = `${toRelativePath(filePath)}:${startLine}${
      startLine !== endLine ? `-${endLine}` : ""
    }${commitShortId ? ` (commit ${commitShortId})` : ""}`;
    const formattedComment = `${lineRef}\n\`\`\`\n${lineContent.join(
      "\n",
    )}\n\`\`\`\n> ${commentText}\n`;
    try {
      await startAgentSession("Code Comment", formattedComment, mode);
      addToast({
        title: "Comment sent to agent",
        description: `Created new agent session and sent comment`,
        type: "success",
      });
    } catch (error) {
      toastError("Failed to create agent", error);
    }
  };

  /**
   * Send one local review comment to the idle agent, or a fresh agent terminal,
   * formatted like {@link handleCreateAgentWithComment} plus the agent's
   * suggested replacement when it left one.
   */
  const handleSendAgentReviewCommentToAgent = async (
    comment: AgentReviewComment,
  ) => {
    const lineRef = `${toRelativePath(comment.file_path)}:${comment.start_line}${
      comment.start_line !== comment.end_line ? `-${comment.end_line}` : ""
    }`;
    const suggestion =
      comment.suggested_replacement != null
        ? `\nSuggested change:\n\`\`\`\n${comment.suggested_replacement}\n\`\`\`\n`
        : "";
    const formattedComment = `Please address this review comment.\n\n${lineRef}\n> ${comment.comment_text}\n${suggestion}`;
    try {
      if (sendToIdleAgent(formattedComment)) {
        addToast({
          title: "Comment sent to agent",
          description: "Sent review comment to the idle agent",
          type: "success",
        });
        return;
      }
      await startAgentSession(
        "Review Comment",
        formattedComment,
        "acceptEdits",
      );
      addToast({
        title: "Comment sent to agent",
        description: "Created new agent session and sent review comment",
        type: "success",
      });
    } catch (error) {
      toastError("Failed to create agent", error);
    }
  };

  /** Resolves the default agent from repo-level, then app-level settings. */
  const resolveDefaultAgent = async (
    agentOverride?: string,
  ): Promise<AgentKind | undefined> => {
    const repoPathForSettings = effectiveRepoPath || workingDirectory;
    const repoDefault = await getRepoSetting(
      repoPathForSettings,
      "default_agent",
    ).catch(() => null); // repo may not be initialized yet
    const appDefault = await getSetting("default_agent").catch(() => null);
    // undefined falls back to Dashboard's default (claude)
    return toAgentKind(agentOverride || repoDefault || appDefault);
  };

  const createAgentWithReview = async (
    reviewMarkdown: string,
    mode: "plan" | "acceptEdits",
    sessionName: string,
    agentOverride?: string,
    reuseIdleAgent = false,
  ) => {
    try {
      if (reuseIdleAgent && sendToIdleAgent(reviewMarkdown)) {
        addToast({
          title: "Review sent to agent",
          description: "Sent review to the idle agent",
          type: "success",
        });
        return;
      }
      await startAgentSession(
        sessionName,
        reviewMarkdown,
        mode,
        await resolveDefaultAgent(agentOverride),
      );
    } catch (error) {
      toastError("Failed to create agent", error);
      throw error;
    }
  };

  /** Opens a fresh agent session seeded with the selected log content. */
  const workspaceMetadata = workspace?.metadata
    ? (() => {
        try {
          return JSON.parse(workspace.metadata) as WorkspaceTrackerMetadata & {
            title?: string;
            description?: string;
          };
        } catch {
          return null;
        }
      })()
    : null;
  const workspaceTitle =
    workspace?.title ||
    workspaceMetadata?.title ||
    workspace?.branch_name ||
    null;
  const workspaceDescription =
    workspace?.description || workspaceMetadata?.description || null;

  const handleSendLogsToAgent = async (prompt: string) => {
    try {
      await startAgentSession("Logs", prompt, "plan");
    } catch (error) {
      toastError("Failed to send logs to agent", error);
    }
  };

  /**
   * Sends a finished review to a new agent terminal, or to the idle agent when
   * the user ticked "Send to idle agent".
   */
  const handleCreateAgentWithReview = (
    reviewMarkdown: string,
    mode: "plan" | "acceptEdits",
    useIdleAgent = false,
  ) =>
    createAgentWithReview(
      reviewMarkdown,
      mode,
      "Code Review",
      undefined,
      useIdleAgent,
    );

  /**
   * Launches a review agent terminal seeded with a generated review prompt.
   * Always a new terminal: a reviewer that inherits an idle agent's context
   * would review its own work.
   */
  const handleStartAgentReview = (prompt: string, agent?: string) =>
    createAgentWithReview(prompt, "acceptEdits", "AI Review", agent);

  // Display branch name as title: workspace branch if available, otherwise main repo branch
  const branchTitle = workspace?.branch_name || mainRepoBranch || "main";
  const hasSyncChanges =
    !!syncStatus && (syncStatus.ahead > 0 || syncStatus.behind > 0);
  const homeRebaseTooltip = homeRebasing
    ? "Rebasing..."
    : `Rebase ${branchTitle} onto ${defaultBranch} (${homeRepoTargetAheadCount} new commit${
        homeRepoTargetAheadCount === 1 ? "" : "s"
      })`;
  const mergeBlocked = rebasing || conflictCount > 0;
  const mergeBlockedTooltip = rebasing
    ? "Rebasing in progress..."
    : `Cannot merge: ${conflictCount} conflict${
        conflictCount === 1 ? "" : "s"
      } detected`;
  // GPUI draws the Code and Commits tabs, with the terminal pane framed
  // below them.
  const gpuiCode =
    (activeTab === "overview" && !showFileBrowserInCode) ||
    activeTab === "commits";
  const gpuiFramed = useGpuiWorkspaceScreen({
    active: gpuiActive && !!workingDirectory && !!effectiveRepoPath,
    parts: gpuiParts,
    detailsOpen,
    route: {
      repoPath: effectiveRepoPath,
      workspaceId: workspace?.id ?? null,
      homeBranch: mainRepoBranch ?? "",
      tab: activeTab as GpuiWorkspaceTab,
      // The Browser view's switcher and address bar live in the tab row.
      tabRow: !(reviewSubView.switcher || reviewSubView.showBrowser),
      retargeting: rebasing,
      refreshing: refreshingFiles,
      code: gpuiCode,
      scrollToCommit: activeTab === "commits" ? scrollToCommitId : null,
      frameHeight: gpuiFrameHeight,
    },
    handlers: {
      setTab: setActiveTab,
      contentHeight: (height) => onGpuiContentHeight?.(height),
      openEntry: ({ name, isDirectory }) =>
        handleOverviewEntryClick({
          name,
          is_directory: isDirectory,
        } as DirectoryEntry),
      openFilePicker: onOpenFilePicker,
      openCommit: (changeId) => {
        setScrollToCommitId(changeId);
        setActiveTab("commits");
      },
      navigateToWorkspace: onNavigateToWorkspace,
      sessionCreated: onSessionCreated,
      commitScrolled: () => setScrollToCommitId(null),
      moveCommitToNew: onMoveCommitToNewWorkspace
        ? (commit) => onMoveCommitToNewWorkspace(commit, workspace)
        : undefined,
      moveCommitToExisting: onMoveCommitToExistingWorkspace
        ? (commit) => onMoveCommitToExistingWorkspace(commit, workspace)
        : undefined,
      commitStashed: onCommitStashed,
      openBranchSwitcher: onOpenBranchSwitcher,
      stack: onCreateStackedWorkspace,
      merge: onOpenMergePreview,
      openDetails: () => setDetailsOpen(true),
      deleteWorkspace:
        workspace && onDeleteWorkspace
          ? () => onDeleteWorkspace(workspace)
          : undefined,
    },
  });
  const gpuiDrawsCode = gpuiFramed && gpuiCode;
  useEffect(() => {
    if (!gpuiDrawsCode) onGpuiContentHeight?.(null);
  }, [gpuiDrawsCode, onGpuiContentHeight]);
  // GPUI draws the header and tab row; React keeps them mounted but hidden,
  // so the parts they publish stay current.
  const gpuiTabRow =
    gpuiFramed && !(reviewSubView.switcher || reviewSubView.showBrowser);

  const executionPanel = workingDirectory ? (
    <div className="flex flex-col h-full">
      <div
        data-testid="workspace-tab-row"
        className={cn(
          "flex-shrink-0 bg-background px-4 py-2 border-b border-border flex items-center gap-3",
          gpuiTabRow && "hidden",
        )}
      >
        <div className="flex items-center gap-2 flex-shrink-0">
          <Tabs value={activeTab} onValueChange={setActiveTab}>
            <TabsList>
              <TabsTrigger
                value="overview"
                className="inline-flex items-center"
              >
                <Code2 className="w-4 h-4 mr-1.5" />
                Code
              </TabsTrigger>
              <TabsTrigger
                value="commits"
                className="inline-flex items-center gap-1.5"
              >
                <GitCommitHorizontal className="w-4 h-4" />
                <span>Commits</span>
                {commitsTabLabel?.kind === "count" && (
                  <span
                    data-testid="commits-tab-count"
                    className={cn(
                      "rounded-full px-2 py-0.5 text-xs font-medium transition-colors",
                      commitsTabCountClassName(commitsTabLabel.tone),
                    )}
                  >
                    {commitsTabLabel.count}
                  </span>
                )}
                {commitsTabLabel?.kind === "home-conflict" && (
                  <AlertTriangle
                    data-testid="commits-tab-conflict-icon"
                    className="h-3.5 w-3.5 text-destructive"
                    aria-label="Conflicts in home repository"
                  />
                )}
              </TabsTrigger>
              <TabsTrigger
                value="changes"
                className="inline-flex items-center gap-1.5"
              >
                <FileDiff className="w-4 h-4" />
                <span>Changes</span>
                {reviewTabPill && (
                  <span
                    data-testid="review-change-count"
                    className={cn(
                      "rounded-full px-2 py-0.5 text-xs font-medium transition-colors",
                      reviewTabPillClassName(reviewTabPill.tone),
                    )}
                  >
                    {reviewTabPill.count}
                  </span>
                )}
              </TabsTrigger>
              {logsEnabled && (
                <TabsTrigger
                  value="logs"
                  className="inline-flex items-center gap-1.5"
                >
                  <Database className="w-4 h-4" />
                  <span>Logs</span>
                </TabsTrigger>
              )}
            </TabsList>
          </Tabs>
          {reviewSubView.switcher}
        </div>
        <div
          ref={reviewSubView.setToolbarSlot}
          className="flex-1 min-w-0 flex items-center gap-2"
        />
        <div className="flex items-center gap-3 flex-shrink-0">
          {(rebasing || refreshingFiles) && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="w-4 h-4 animate-spin" />
              <span>{rebasing ? "Rebasing..." : "Refreshing..."}</span>
            </div>
          )}
          {workspaceLocStats && <LocDiffMarker diffStats={workspaceLocStats} />}
        </div>
      </div>
      <div className="flex-1 overflow-auto">
        {activeTab === "overview" ? (
          showFileBrowserInCode ? (
            <FileBrowser
              workspace={workspace}
              repoPath={effectiveRepoPath}
              initialSelectedFile={initialSelectedFileForBrowser}
              initialExpandedDir={initialExpandedDir}
              onBack={() => setShowFileBrowserInCode(false)}
              onCreateAgentWithReview={handleCreateAgentWithReview}
            />
          ) : (
            <div className="flex h-full">
              {/* LEFT: Files + README */}
              <div className="flex-1 overflow-auto border-r border-border">
                <div className="p-4 space-y-4">
                  {overviewPending && !overviewData && (
                    <div
                      className="space-y-4"
                      data-testid="workspace-overview-skeleton"
                    >
                      <div className="h-10 rounded-lg bg-muted/50 animate-pulse" />
                      <div className="h-10 rounded-lg bg-muted/50 animate-pulse" />
                      <div className="border rounded-lg p-4 space-y-2">
                        <div className="h-4 w-32 rounded bg-muted/50 animate-pulse" />
                        <div className="h-3 rounded bg-muted/40 animate-pulse" />
                        <div className="h-3 w-11/12 rounded bg-muted/40 animate-pulse" />
                        <div className="h-3 w-10/12 rounded bg-muted/40 animate-pulse" />
                      </div>
                    </div>
                  )}
                  {/* Conflicts Alert */}
                  {conflictCount > 0 && (
                    <div
                      role="alert"
                      className="border border-destructive/30 rounded-md bg-destructive/5 p-4"
                    >
                      <div className="flex items-start gap-3">
                        <AlertTriangle className="w-5 h-5 text-destructive flex-shrink-0 mt-0.5" />
                        <div className="flex-1">
                          <h3 className="font-medium text-destructive">
                            {conflictCount}{" "}
                            {conflictCount === 1 ? "conflict" : "conflicts"}{" "}
                            detected
                          </h3>
                          <p className="text-sm text-muted-foreground mt-1">
                            Some files have conflicts that need to be resolved
                          </p>
                        </div>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => setActiveTab("changes")}
                          className="border-destructive/30 text-destructive hover:bg-destructive/10"
                        >
                          View conflicts
                        </Button>
                      </div>
                    </div>
                  )}
                  {/* Task Input */}
                  <TaskInput
                    repoPath={effectiveRepoPath}
                    workspaceId={workspace?.id ?? null}
                    workingDirectory={workingDirectory}
                    onSessionCreated={onSessionCreated}
                    offerNewWorkspace={!workspace}
                  />
                  {workspace && workspaceDescription && (
                    <WorkspaceDescription
                      repoPath={effectiveRepoPath}
                      workspaceId={workspace.id}
                      description={workspaceDescription}
                    />
                  )}
                  {/* Stack — shown for any workspace in a multi-workspace
										    stack, including the root (whose target is the default
										    branch, not another workspace). The panel returns null
										    when the workspace is alone. */}
                  {workspace && (
                    <WorkspaceStackPanel
                      repoPath={effectiveRepoPath}
                      workspace={workspace}
                      defaultBranch={defaultTargetBranch}
                      onSelectWorkspace={onNavigateToWorkspace}
                    />
                  )}
                  {/* File Search Input */}
                  <div className="flex justify-end">
                    <button
                      type="button"
                      onClick={onOpenFilePicker}
                      className="flex items-center gap-3 px-4 py-2 border border-border rounded-lg bg-background hover:bg-muted/30 transition-colors text-left w-full max-w-xs"
                    >
                      <Search className="w-4 h-4 text-muted-foreground" />
                      <span className="flex-1 text-sm text-muted-foreground">
                        Go to file
                      </span>
                      <KbdGroup className="shrink-0">
                        <Kbd>⌘ + P</Kbd>
                      </KbdGroup>
                    </button>
                  </div>

                  {/* File Listing */}
                  <div className="border rounded-lg divide-y divide-border">
                    {rootEntries.map((entry) => {
                      const submodulePin = entry.submodule_pin;
                      const isSubmodule = Boolean(submodulePin);
                      const shortPin = submodulePin
                        ? submodulePin.slice(0, 7)
                        : "";
                      const showSyncToggle = !workspace && isSubmodule;
                      return (
                        <div
                          key={entry.path}
                          className="flex items-center hover:bg-muted/60 transition"
                          data-testid={
                            isSubmodule
                              ? `submodule-row-${entry.name}`
                              : undefined
                          }
                        >
                          <button
                            type="button"
                            onClick={() => handleOverviewEntryClick(entry)}
                            className="flex items-center gap-3 px-4 py-1 text-sm flex-1 min-w-0 text-left"
                          >
                            {isSubmodule ? (
                              <FolderGit2 className="w-4 h-4 text-blue-500 shrink-0" />
                            ) : entry.is_directory ? (
                              <Folder className="w-4 h-4 text-blue-500 shrink-0" />
                            ) : (
                              <File className="w-4 h-4 text-muted-foreground shrink-0" />
                            )}
                            <span
                              className={cn(
                                "flex-1 font-mono truncate",
                                !entry.status && "text-muted-foreground/70",
                              )}
                              style={{ fontSize: `${fontSize}px` }}
                            >
                              {entry.name}
                              {isSubmodule && (
                                <span className="text-muted-foreground">
                                  {" "}
                                  @ {shortPin}
                                </span>
                              )}
                            </span>
                            {entry.status ? (
                              <JjStatusPip status={entry.status} />
                            ) : (
                              <StatusPip status={getEntryStatus(entry)} />
                            )}
                          </button>
                          {showSyncToggle && (
                            <label className="flex items-center gap-1.5 shrink-0 pr-4 pl-2 text-xs text-muted-foreground cursor-pointer">
                              <input
                                type="checkbox"
                                className="size-3.5 accent-primary"
                                key={`${entry.name}-${Boolean(entry.submodule_synced)}`}
                                defaultChecked={Boolean(entry.submodule_synced)}
                                disabled={submoduleSync.isPending}
                                aria-label={`Sync ${entry.name}`}
                                onChange={(event) =>
                                  submoduleSync.mutate({
                                    path: entry.name,
                                    enabled: event.target.checked,
                                  })
                                }
                              />
                              Sync
                            </label>
                          )}
                        </div>
                      );
                    })}
                    {!overviewPending && rootEntries.length === 0 && (
                      <div className="px-4 py-8 text-center text-sm text-muted-foreground">
                        {overviewData?.unavailable
                          ? REMOTE_FILE_TREE_REASON
                          : "No files found"}
                      </div>
                    )}
                  </div>

                  {/* README Section */}
                  <div className="border rounded-lg p-6">
                    {readmeContent ? (
                      <>
                        <h2 className="text-lg font-semibold mb-4">
                          README.md
                        </h2>
                        <MarkdownContent
                          content={readmeContent}
                          resolveImageSrc={(src) =>
                            resolveReadmeImageSrc(src, readmeBaseDir)
                          }
                        />
                      </>
                    ) : (
                      <div className="text-muted-foreground text-sm text-center py-4">
                        No README.md found
                      </div>
                    )}
                  </div>
                </div>
              </div>

              {/* RIGHT: Commit History (fixed width matching sidebar) */}
              <div className="w-[240px] shrink-0 bg-muted/20">
                <LinearCommitHistory
                  repoPath={effectiveRepoPath}
                  workspaceId={workspace?.id ?? null}
                  onCommitClick={(changeId) => {
                    setScrollToCommitId(changeId);
                    setActiveTab("commits");
                  }}
                  onCommitsLoaded={handleCommitsLoaded}
                />
              </div>
            </div>
          )
        ) : activeTab === "commits" ? (
          <CommitDiffViewer
            repoPath={effectiveRepoPath}
            workspaceId={workspace?.id ?? null}
            scrollToCommitId={scrollToCommitId}
            onScrollComplete={() => setScrollToCommitId(null)}
            onCommitStashed={onCommitStashed}
            onCreateAgentWithComment={handleCreateAgentWithComment}
            onSessionCreated={onSessionCreated}
            onMoveCommitToNewWorkspace={
              onMoveCommitToNewWorkspace
                ? (commit) => onMoveCommitToNewWorkspace(commit, workspace)
                : undefined
            }
            onMoveCommitToExistingWorkspace={
              onMoveCommitToExistingWorkspace
                ? (commit) => onMoveCommitToExistingWorkspace(commit, workspace)
                : undefined
            }
            onViewTentativeChanges={handleViewTentativeChanges}
            onDeleteTentativeChanges={handleDeleteTentativeChanges}
          />
        ) : activeTab === "logs" ? (
          <LogsTab
            key={logsKey}
            initialSource={logsSource}
            repoPath={effectiveRepoPath ?? ""}
            onSendToAgent={handleSendLogsToAgent}
          />
        ) : reviewSubView.showBrowser ? (
          <BrowserPanel
            openRequest={reviewSubView.openRequest}
            toolbarSlot={reviewSubView.toolbarSlot}
          />
        ) : (
          <ChangesDiffViewer
            key={`changes-${workingDirectory}`}
            workspacePath={workingDirectory}
            workspaceId={workspace?.id}
            isHomeRepo={!workspace}
            repoPath={effectiveRepoPath}
            branchName={workspace?.branch_name}
            onChangedFilesChange={handleChangedFilesUpdate}
            onRefreshingChange={handleRefreshingChange}
            initialSelectedFile={initialSelectedFile}
            conflictedFiles={normalizedConflictedFiles}
            onCreateAgentWithReview={handleCreateAgentWithReview}
            onStartAgentReview={handleStartAgentReview}
            onSendAgentReviewCommentToAgent={
              handleSendAgentReviewCommentToAgent
            }
            showCommittedChanges={
              workspace && workspace.branch_name !== defaultTargetBranch
                ? showCommittedChanges
                : false
            }
            onShowCommittedChangesChange={
              workspace && workspace.branch_name !== defaultTargetBranch
                ? setShowCommittedChanges
                : undefined
            }
            onMoveFilesToNewWorkspace={
              onMoveFilesToNewWorkspace
                ? (files) => onMoveFilesToNewWorkspace(files, workspace)
                : undefined
            }
            workspace={workspace}
            baseBranch={targetBranch ?? defaultTargetBranch}
          />
        )}
      </div>
    </div>
  ) : (
    <div className="h-full flex items-center justify-center text-center p-6 text-sm text-muted-foreground">
      Configure a workspace or repository path to manage commits.
    </div>
  );

  return (
    <GpuiPartsProvider registry={gpuiParts.registry}>
      <div className="h-full w-full flex flex-col bg-background">
        <div
          className={cn(
            "border-b p-2 flex flex-col gap-1 flex-shrink-0",
            gpuiFramed && "hidden",
          )}
          data-testid="show-workspace-header"
        >
          {/* Row 1: Branch name. Wraps so the actions drop to a second row
              instead of overlapping the branch controls in a narrow pane. */}
          <div className="flex flex-wrap items-center justify-between gap-x-2 gap-y-1">
            <div className="flex items-center gap-2 min-w-0">
              <GitBranch className="w-4 h-4 text-muted-foreground shrink-0" />

              {workspace && (
                <span
                  className="text-sm font-semibold font-mono truncate min-w-0 max-w-[220px]"
                  title={branchTitle}
                >
                  {branchTitle}
                </span>
              )}
              {workspace && (
                <WorkspaceTrackerBadges metadata={workspaceMetadata} />
              )}
              {workspace && workspace.branch_name !== defaultBranch && (
                <>
                  <ArrowRight className="w-4 h-4 text-muted-foreground shrink-0" />
                  <TargetBranchSelector
                    branches={availableBranches}
                    loading={branchesLoading}
                    targetBranch={targetBranch}
                    onSelect={handleTargetBranchSelect}
                    onOpenChange={(open) => {
                      if (open) {
                        void onLoadAvailableBranches?.();
                      }
                    }}
                    disabled={rebasing}
                  />
                </>
              )}
              {!workspace && (
                <>
                  <button
                    type="button"
                    onClick={onOpenBranchSwitcher}
                    className="text-sm font-semibold font-mono hover:underline cursor-pointer truncate min-w-0 max-w-[220px]"
                    title={branchTitle}
                  >
                    {branchTitle}
                  </button>
                  {/* Target branch label + rebase button for non-default home repo branches */}
                  {isHomeRepo && branchTitle !== defaultBranch && (
                    <>
                      <ArrowRight className="w-4 h-4 text-muted-foreground shrink-0" />
                      <span
                        className="text-sm font-mono text-muted-foreground truncate min-w-0 max-w-[220px]"
                        title={defaultBranch}
                      >
                        {defaultBranch}
                      </span>
                      {homeRepoTargetAheadCount > 0 && (
                        <>
                          {homeRebaseDryRun?.would_conflict && (
                            <TooltipProvider delay={200}>
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <span className="flex items-center text-yellow-600 dark:text-yellow-400">
                                    <AlertTriangle className="w-4 h-4" />
                                  </span>
                                </TooltipTrigger>
                                <TooltipContent>
                                  <p className="font-medium mb-1">
                                    Potential conflicts detected
                                  </p>
                                  {homeRebaseDryRun.conflicted_files.length >
                                    0 && (
                                    <ul className="text-xs space-y-0.5">
                                      {homeRebaseDryRun.conflicted_files.map(
                                        (f) => (
                                          <li key={f} className="font-mono">
                                            {f}
                                          </li>
                                        ),
                                      )}
                                    </ul>
                                  )}
                                </TooltipContent>
                              </Tooltip>
                            </TooltipProvider>
                          )}
                          <TooltipProvider delay={200}>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button
                                  variant="outline"
                                  size="sm"
                                  onClick={handleHomeRebase}
                                  disabled={homeRebasing}
                                  className="gap-1 px-2 py-1"
                                >
                                  {homeRebasing ? (
                                    <Loader2 className="w-4 h-4 animate-spin" />
                                  ) : (
                                    <GitCompareArrows className="w-4 h-4" />
                                  )}
                                  Rebase
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>
                                {homeRebaseTooltip}
                              </TooltipContent>
                            </Tooltip>
                          </TooltipProvider>
                        </>
                      )}
                    </>
                  )}
                  {/* Stack button for home repo */}
                  {onCreateStackedWorkspace && (
                    <TooltipProvider delay={200}>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant="default"
                            size="sm"
                            onClick={onCreateStackedWorkspace}
                            className="gap-1 px-2 py-1"
                          >
                            <Layers2 className="w-4 h-4" />
                            Stack
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          {`Create stacked workspace from ${branchTitle}`}
                        </TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                  )}
                </>
              )}
            </div>
            <div className="flex items-center gap-2 ml-auto">
              {/* Push button - shown when the branch isn't on remote
								    and we can't offer the combined push+create-PR flow (either
								    there's no GitHub remote, or this is the default branch, which
								    can't be PR'd into itself). */}
              {workspace &&
                workspace.not_on_remote &&
                (!remoteInfo || workspace.branch_name === defaultBranch) && (
                  <TooltipProvider delay={200}>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="default"
                          size="sm"
                          onClick={handlePushToRemote}
                          disabled={!!actionPending}
                          className="bg-blue-600 hover:bg-blue-700"
                        >
                          <Upload className="w-4 h-4" />
                          Push
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent>Push to remote</TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                )}

              {/* Create / View PR. When the branch has a GitHub remote but
								    hasn't been pushed yet, CreatePrButtonGroup pushes it first so
								    there's no separate "Push" click required. */}
              {workspace &&
                workspace.branch_name !== defaultBranch &&
                effectiveRepoPath && (
                  <>
                    <ViewPrButton
                      repoPath={effectiveRepoPath}
                      branchName={workspace.branch_name}
                      onViewInApp={onViewPrInApp}
                    />
                    <CreatePrButtonGroup
                      repoPath={effectiveRepoPath}
                      workspace={workspace}
                      baseBranch={targetBranch ?? defaultTargetBranch}
                      // Diverged means the branch was rewritten locally (e.g.
                      // rebased). jj_push is lease-protected and refuses a
                      // conflicted bookmark, so pushing here is safe.
                      needsPush={
                        workspace.not_on_remote ||
                        workspaceStatusData?.remote_sync.type === "Ahead" ||
                        workspaceStatusData?.remote_sync.type === "Diverged"
                      }
                      hasCommits={
                        hasWorkspaceCommits ||
                        (workspaceStatusData?.commits_ahead_of_target?.length ??
                          0) > 0
                      }
                    />
                  </>
                )}

              {/* Status is independent of the header and overview requests. Keep
								    the already-known workspace controls interactive while it loads. */}
              {workspaceStatusPending && (
                <div
                  className="h-6 w-14 animate-pulse rounded bg-muted/60"
                  data-testid="workspace-status-skeleton"
                  aria-label="Loading workspace status"
                />
              )}

              {/* Sync control - status + icon in one clickable button */}
              {(!workspace || !workspace.not_on_remote) && hasSyncChanges && (
                <TooltipProvider delay={200}>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant={syncStatus.ahead > 0 ? "outline" : "ghost"}
                        size="sm"
                        className="relative h-6 gap-1 px-2 text-xs text-muted-foreground"
                        onClick={handleSync}
                        disabled={!!actionPending}
                      >
                        <span
                          className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-yellow-600 opacity-50 animate-pulse"
                          aria-hidden="true"
                        />
                        {syncStatus.behind > 0 && (
                          <span className="flex items-center">
                            ↓{syncStatus.behind}
                          </span>
                        )}
                        {syncStatus.ahead > 0 && (
                          <span className="flex items-center">
                            ↑{syncStatus.ahead}
                          </span>
                        )}
                        <RefreshCw
                          className={cn(
                            "w-4 h-4",
                            actionPending === "sync" && "animate-spin",
                          )}
                        />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>
                      Sync with remote (fetch and push)
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              )}
              {checksTarget && (
                <WorkspaceChecksButton
                  target={checksTarget}
                  prBranch={
                    workspace && workspace.branch_name !== defaultBranch
                      ? workspace.branch_name
                      : undefined
                  }
                  onOpenLogs={logsEnabled ? openCheckLogs : undefined}
                />
              )}
              {/* Merge queue button (build flag + repo opt-in) */}
              {workspace &&
                workspace.branch_name !== defaultBranch &&
                !workspace.not_on_remote && (
                  <MergeQueueButton
                    repoPath={effectiveRepoPath}
                    branchName={workspace.branch_name}
                  />
                )}
              {/* Merge button moved here */}
              {workspace && workspace.branch_name !== defaultBranch && (
                <TooltipProvider delay={200}>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <div className="inline-flex">
                        <Button
                          variant={ciAllPassing ? "default" : "outline"}
                          size="sm"
                          onClick={onOpenMergePreview}
                          disabled={rebasing || conflictCount > 0}
                          className={cn(
                            "gap-1",
                            ciAllPassing &&
                              "bg-green-600 hover:bg-green-700 text-white",
                          )}
                        >
                          <GitCompareArrows className="w-4 h-4" />
                          Merge...
                        </Button>
                      </div>
                    </TooltipTrigger>
                    {mergeBlocked && (
                      <TooltipContent>{mergeBlockedTooltip}</TooltipContent>
                    )}
                  </Tooltip>
                </TooltipProvider>
              )}
              {workspace && (
                <Popover open={detailsOpen} onOpenChange={setDetailsOpen}>
                  <PopoverTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="gap-1 px-2"
                      data-testid="workspace-details-button"
                    >
                      <Info className="w-4 h-4" />
                      Details
                    </Button>
                  </PopoverTrigger>
                  <PopoverContent
                    className="w-96"
                    align="end"
                    data-testid="workspace-details-popover"
                  >
                    <div className="space-y-3">
                      <div>
                        <h3 className="text-sm font-semibold">
                          Workspace details
                        </h3>
                        <dl className="mt-2 space-y-1 text-sm">
                          <div className="flex justify-between gap-2">
                            <dt className="text-muted-foreground">Branch</dt>
                            <dd className="font-mono truncate">
                              {workspace.branch_name}
                            </dd>
                          </div>
                          {targetBranch && (
                            <div className="flex justify-between gap-2">
                              <dt className="text-muted-foreground">
                                Target branch
                              </dt>
                              <dd className="font-mono truncate">
                                {targetBranch}
                              </dd>
                            </div>
                          )}
                          <div className="flex justify-between gap-2">
                            <dt className="text-muted-foreground">Created</dt>
                            <dd>
                              {new Date(workspace.created_at).toLocaleString()}
                            </dd>
                          </div>
                        </dl>
                      </div>
                      <div>
                        <div className="flex items-center justify-between">
                          <h4 className="text-sm font-medium">
                            Starting prompt
                          </h4>
                          {startingPromptEntry && (
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-6 px-2 text-xs"
                              onClick={handleCopyStartingPrompt}
                            >
                              <Copy className="w-3 h-3 mr-1" />
                              Copy
                            </Button>
                          )}
                        </div>
                        {startingPromptEntry ? (
                          <>
                            <p
                              className="mt-1 text-sm text-muted-foreground whitespace-pre-wrap max-h-40 overflow-y-auto"
                              data-testid="workspace-starting-prompt-text"
                            >
                              {startingPromptEntry.prompt_text}
                            </p>
                            <button
                              type="button"
                              className="mt-1 text-xs text-blue-600 dark:text-blue-400 hover:underline"
                              data-testid="view-full-prompt-button"
                              onClick={() => {
                                setDetailsOpen(false);
                                onViewFullPrompt?.(startingPromptEntry.id);
                              }}
                            >
                              View full prompt
                            </button>
                          </>
                        ) : (
                          <p className="mt-1 text-sm text-muted-foreground">
                            No prompt recorded for this workspace yet.
                          </p>
                        )}
                      </div>
                    </div>
                  </PopoverContent>
                </Popover>
              )}
              {workspace && onDeleteWorkspace && (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="px-1"
                      disabled={!!actionPending}
                      aria-label="More workspace actions"
                    >
                      <MoreVertical className="w-4 h-4" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" sideOffset={4}>
                    <DropdownMenuItem
                      onSelect={() => onDeleteWorkspace(workspace)}
                      className="text-destructive focus:text-destructive"
                    >
                      <Trash2 className="w-4 h-4 mr-2" />
                      Delete Workspace
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
            </div>
          </div>
          {/* Row 2: Title (if workspace title exists) */}
          {workspace && workspaceTitle && workspaceTitle !== branchTitle && (
            <div className="flex items-center px-1 mt-2">
              <h1 className="text-sm font-medium text-foreground">
                {workspaceTitle}
              </h1>
            </div>
          )}
        </div>

        <div className="flex-1 flex overflow-hidden min-h-0">
          <div className="w-full flex flex-col overflow-hidden">
            {executionPanel}
          </div>
        </div>
      </div>
    </GpuiPartsProvider>
  );
};
