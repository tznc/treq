/* eslint-disable max-lines */
import { invoke } from "@tauri-apps/api/core";
import type { InstalledSkill, RepoYamlConfig } from "./api-extra";
import type { RustAccount, RustCloudAction } from "./rust-account";
import type {
  BranchStatus,
  EditorAppsResponse,
  GitRemoteInfo,
  HomeRebaseDryRunResult,
  JjBranch,
  JjCommitsAhead,
  JjDiffHunk,
  JjFileChange,
  JjFileDiff,
  JjFileLines,
  JjLogResult,
  JjRebaseResult,
  JjRevisionDiff,
  LocalSshIdentity,
  LocalMergeOptions,
  MergeLandingPlan,
  MergeResult,
  MergeStrategy,
  PrCiStatus,
  PrerequisiteStatus,
  PullWorkspaceResult,
  RenameWorkspaceResult,
  RepoBranch,
  SingleRebaseResult,
  SshHost,
  Workspace,
  WorkspaceSidebarStatus,
  WorkspaceStatus,
} from "./api-types";
import { enqueueJjExclusive } from "./enqueue-jj-exclusive";
import { localReadOr, localRepoPathOrNull } from "./local-only";
import {
  assertLocalOperation,
  remoteRepositoryContaining,
  transportCreateCommit,
  transportGetCommitDiff,
  transportGetCommitFileDiff,
  transportGetGitRemoteInfo,
  transportGetWorkspaceFileHunks,
  transportGetRepoCurrentBranch,
  transportGetRepoDefaultBranch,
  transportGetWorkspaces,
  transportGetWorkspaceChangedFiles,
  transportGetWorkspaceDiff,
  transportGetWorkspaceFileHunksBatch,
  transportGetWorkspaceFileLines,
  transportGetWorkspaceStatus,
  transportGitFetch,
  transportListCommits,
  transportListRepoBranches,
  transportListWorkspaceStatuses,
} from "./repository-adapter";
import { repoStateScope } from "./repo-state-scope";
import {
  transportCheckBranchExists,
  transportCreateWorkspace,
  transportDeleteWorkspace,
  transportMoveWorkspaceChanges,
  transportPullWorkspace,
  transportPushWorkspace,
  transportRenameWorkspace,
  transportRestoreFile,
  transportSetWorkspaceTargetBranch,
  transportSplitWorkingCopy,
  transportSwitchRepoBranch,
  transportUpdateWorkspace,
  transportWorkingCopyMutation,
} from "./repository-adapter-mutations";
import { currentWindowLabel } from "./window-label";

export * from "./api-browser";
export * from "./api-checks-logs";
export * from "./api-extra";
export * from "./api-github";
export * from "./api-remote-ssh";
export * from "./api-types";

/**
 * Sets up `.treq/` and jj for a repository. Rejects with a readable message
 * (and writes nothing) when the folder is not a Git repository. Resolves
 * `false` when jj setup failed for another reason; the repo still opens.
 */
export const initRepo = async (repoPath: string): Promise<boolean> => {
  assertLocalOperation(repoPath, "Initializing a repository");
  return invoke("init_repo", { repoPath });
};

/** Push the GPUI sidebar snapshot to the native shell; null hides it. */
export const gpuiSidebarUpdate = (
  snapshot: import("./gpui-shell").GpuiSidebarSnapshot | null,
): Promise<void> =>
  invoke("gpui_sidebar_update", {
    windowLabel: currentWindowLabel(),
    snapshot,
  });

/** Draws a screen in GPUI in place of this webview; null shows the webview. */
export const gpuiScreenUpdate = (
  snapshot: import("./gpui-screen").GpuiScreenSnapshot | null,
): Promise<void> =>
  invoke("gpui_screen_update", {
    windowLabel: currentWindowLabel(),
    snapshot,
  });

/** Draws a dialog in GPUI over this window's screen; null closes it. */
export const gpuiModalUpdate = (
  modal: import("./gpui-screen").GpuiModal | null,
): Promise<void> =>
  invoke("gpui_modal_update", {
    windowLabel: currentWindowLabel(),
    modal,
  });

/** Shows a dialog in GPUI over this whole window; null closes it. */
export const gpuiOverlayUpdate = (
  snapshot: import("./gpui-overlay").GpuiOverlaySnapshot | null,
): Promise<void> =>
  invoke("gpui_overlay_update", {
    windowLabel: currentWindowLabel(),
    snapshot,
  });

/** Opens a GPUI menu at the last right-click in this window's sidebar. */
export const gpuiMenuShow = (
  entries: import("./native-context-menu").GpuiMenuEntry[],
): Promise<void> =>
  invoke("gpui_menu_show", { windowLabel: currentWindowLabel(), entries });

export const gpuiMenuHide = (): Promise<void> => invoke("gpui_menu_hide");

/** Shows this window's toasts in GPUI, above the webview. */
export const gpuiToastsUpdate = (
  toasts: import("../stores/toastStore").GpuiToast[],
): Promise<void> =>
  invoke("gpui_toasts_update", { windowLabel: currentWindowLabel(), toasts });

/** Runs the GPUI toast button `id`, once. */
export const gpuiToastAction = (id: number): Promise<void> =>
  invoke("gpui_toast_action", { id });

// Database API
export const getWorkspaces = (repoPath: string): Promise<Workspace[]> =>
  transportGetWorkspaces(repoPath, () =>
    invoke("get_workspaces", { repoPath }),
  );

export const getRepoCurrentBranch = (repoPath: string): Promise<RepoBranch> =>
  transportGetRepoCurrentBranch(repoPath, () =>
    invoke("get_repo_current_branch", { repoPath }),
  );

export const getRepoDefaultBranch = (repoPath: string): Promise<string> =>
  transportGetRepoDefaultBranch(repoPath, () =>
    invoke("get_repo_default_branch", { repoPath }),
  );

export const createWorkspace = (
  repoPath: string,
  branchName: string,
  sourceBranch?: string,
  metadata?: string,
): Promise<number> =>
  transportCreateWorkspace(repoPath, branchName, sourceBranch, metadata, () =>
    invoke("create_workspace", {
      repoPath,
      branchName,
      sourceBranch: sourceBranch ?? null,
      metadata: metadata ?? null,
    }),
  );

export const deleteWorkspace = (repoPath: string, id: number): Promise<void> =>
  transportDeleteWorkspace(repoPath, id, () =>
    invoke("delete_workspace", {
      repoPath,
      id,
    }),
  );

export const archiveWorkspace = (repoPath: string, id: number): Promise<void> =>
  transportDeleteWorkspace(
    repoPath,
    id,
    () => invoke("archive_workspace", { repoPath, id }),
    "ArchiveWorkspace",
  );

export const ensureWorkspaceIndexed = async (
  repoPath: string,
  workspaceId: number | null,
  workspacePath: string,
): Promise<boolean> => {
  assertLocalOperation(repoPath, "Indexing workspace files");
  return invoke("ensure_workspace_indexed", {
    repoPath,
    workspaceId,
    workspacePath,
  });
};

export const getSetting = (key: string): Promise<string | null> =>
  invoke("get_setting", { key });

export const getSettingsBatch = (
  keys: string[],
): Promise<Record<string, string | null>> =>
  invoke("get_settings_batch", { keys });

export const setSetting = (key: string, value: string): Promise<void> =>
  invoke("set_setting", { key, value });

export interface AppSetupScriptStatus {
  script: string;
  always_run: boolean;
  last_run_at: string | null;
  last_status: "passed" | "failed" | null;
  running: boolean;
}

export const getAppSetupScriptStatus = (): Promise<AppSetupScriptStatus> =>
  invoke("get_app_setup_script_status");

export const saveAppSetupScript = (
  script: string,
  alwaysRun: boolean,
): Promise<void> => invoke("save_app_setup_script", { script, alwaysRun });

/** Runs the saved script in the background; poll the status for the result. */
export const runAppSetupScript = (): Promise<void> =>
  invoke("run_app_setup_script");

// Repository settings are keyed by local path. A remote repository has none
// yet, so it reads defaults and refuses writes.
export const getRepoSetting = async (
  repoPath: string,
  key: string,
): Promise<string | null> =>
  remoteRepositoryContaining(repoPath)
    ? null
    : invoke("get_repo_setting", { repoPath, key });

export const setRepoSetting = async (
  repoPath: string,
  key: string,
  value: string,
): Promise<void> => {
  assertLocalOperation(repoPath, "Changing repository settings");
  return invoke("set_repo_setting", { repoPath, key, value });
};

export const listAgentReviewComments = (
  repoPath: string,
  targetType: string,
  targetId: string,
): Promise<import("./api-types-review").AgentReviewComment[]> =>
  invoke("list_agent_review_comments", {
    repoPath: repoStateScope(repoPath),
    targetType,
    targetId,
  });

export const resolveAgentReviewComment = (
  repoPath: string,
  commentId: string,
): Promise<void> =>
  invoke("resolve_agent_review_comment", {
    repoPath: repoStateScope(repoPath),
    commentId,
  });

export interface ReviewDraftInput {
  targetType: string;
  targetId: string;
  filePath: string;
  startLine: number;
  endLine: number;
  side: "old" | "new";
  commentText: string;
  lineHash: string | null;
}

/** Store a draft comment the user wrote (a PR review draft). */
export const createReviewDraft = (
  repoPath: string,
  draft: ReviewDraftInput,
): Promise<import("./api-types-review").AgentReviewComment> =>
  invoke("create_review_draft", {
    repoPath: repoStateScope(repoPath),
    ...draft,
  });

/** Edit a stored comment's text and/or record its line hash. */
export const updateAgentReviewComment = (
  repoPath: string,
  commentId: string,
  update: { commentText?: string; lineHash?: string },
): Promise<void> =>
  invoke("update_agent_review_comment", {
    repoPath: repoStateScope(repoPath),
    commentId,
    commentText: update.commentText ?? null,
    lineHash: update.lineHash ?? null,
  });

export const deleteAgentReviewComment = (
  repoPath: string,
  commentId: string,
): Promise<void> =>
  invoke("delete_agent_review_comment", {
    repoPath: repoStateScope(repoPath),
    commentId,
  });

export const applyAgentReviewSuggestion = async (
  repoPath: string,
  commentId: string,
): Promise<void> => {
  assertLocalOperation(repoPath, "Applying a review suggestion");
  return invoke("apply_agent_review_suggestion", { repoPath, commentId });
};

// A remote repository's `.treq/config.yaml` lives on the remote host.
const NO_REPO_YAML: RepoYamlConfig = {
  branch_name_pattern: null,
  default_model: null,
  default_agent: null,
  target_branch: null,
  included_copy_files: null,
  review_prompt: null,
  review_agent: null,
  auto_review_trigger: null,
};

export const loadRepoYamlConfig = (repoPath: string): Promise<RepoYamlConfig> =>
  localReadOr(repoPath, NO_REPO_YAML, () =>
    invoke("load_repo_yaml_config", { repoPath }),
  );

export const setWindowRepoPath = (repoPath: string): Promise<void> =>
  localReadOr(repoPath, undefined, () =>
    invoke("set_window_repo_path", {
      repoPath,
      windowLabel: currentWindowLabel(),
    }),
  );

const SUPPORTING_REPOS = "Linking supporting repositories";

export interface SupportingRepo {
  path: string;
  exists: boolean;
}

// Supporting repositories are linked by local path, so a remote repository
// has none and cannot link one yet.
export const listSupportingRepos = (
  repoPath: string,
): Promise<SupportingRepo[]> =>
  localReadOr(repoPath, [], () =>
    invoke("list_supporting_repos", { repoPath }),
  );

/** Validates and links a supporting repository; returns its canonical path. */
export const addSupportingRepo = async (
  repoPath: string,
  supportingPath: string,
): Promise<string> => {
  assertLocalOperation(repoPath, SUPPORTING_REPOS);
  return invoke("add_supporting_repo", { repoPath, supportingPath });
};

export const removeSupportingRepo = async (
  repoPath: string,
  supportingPath: string,
): Promise<void> => {
  assertLocalOperation(repoPath, SUPPORTING_REPOS);
  return invoke("remove_supporting_repo", { repoPath, supportingPath });
};

export const setWindowSupportingRepoPaths = (
  repoPaths: string[],
): Promise<void> =>
  invoke("set_window_supporting_repo_paths", {
    repoPaths,
    windowLabel: currentWindowLabel(),
  });

export const detectEditorApps = (): Promise<EditorAppsResponse> =>
  invoke("detect_editor_apps");

export const checkPrerequisites = (): Promise<PrerequisiteStatus[]> =>
  invoke("check_prerequisites");

export const getGitRemoteUrl = (
  repoPath: string,
): Promise<GitRemoteInfo | null> =>
  transportGetGitRemoteInfo(repoPath, () =>
    invoke("get_git_remote_url", { repoPath }),
  );

export const getPrChecksViaGh = (
  repoPath: string,
  branchName: string,
): Promise<PrCiStatus | null> =>
  localReadOr(repoPath, null, () =>
    invoke("get_pr_checks_via_gh", { repoPath, branchName }),
  );

export const getPrChecksForPr = (
  repoFullName: string,
  prNumber: number,
): Promise<PrCiStatus | null> =>
  invoke("get_pr_checks_for_pr", { repoFullName, prNumber });

// JJ Workspace API
// JJ Diff API
export const getWorkspaceChangedFiles = (
  repoPath: string,
  workspaceId: number | null,
): Promise<JjFileChange[]> =>
  enqueueJjExclusive(() =>
    transportGetWorkspaceChangedFiles(repoPath, workspaceId, () =>
      invoke("get_workspace_changed_files", { repoPath, workspaceId }),
    ),
  );

export const listGitignoredPathSuggestions = (
  repoPath: string,
): Promise<string[]> =>
  localReadOr(repoPath, [], () =>
    invoke("list_gitignored_path_suggestions", { repoPath }),
  );

export const getWorkspaceReadme = async (
  repoPath: string,
  workspaceId: number | null,
): Promise<string | null> => {
  assertLocalOperation(repoPath, "Reading the workspace README");
  return invoke("get_workspace_readme", { repoPath, workspaceId });
};

export const getWorkspaceFileHunks = (
  repoPath: string,
  workspaceId: number | null,
  filePath: string,
): Promise<JjDiffHunk[]> =>
  enqueueJjExclusive(() =>
    transportGetWorkspaceFileHunks(repoPath, workspaceId, filePath, () =>
      invoke("get_workspace_file_hunks", {
        repoPath,
        workspaceId,
        filePath,
      }),
    ),
  );

export interface WorkspaceFileHunksBatchFile {
  path: string;
  contentHash: string;
  hunks: JjDiffHunk[];
  error?: string;
}

export interface WorkspaceFileHunksBatch {
  snapshotToken: string;
  files: WorkspaceFileHunksBatchFile[];
}

export const getWorkspaceFileHunksBatch = (
  repoPath: string,
  workspaceId: number | null,
  filePaths: string[],
  snapshotToken?: string,
): Promise<WorkspaceFileHunksBatch> =>
  enqueueJjExclusive(() =>
    transportGetWorkspaceFileHunksBatch(repoPath, workspaceId, filePaths, () =>
      invoke("get_workspace_file_hunks_batch", {
        repoPath,
        workspaceId,
        filePaths,
        snapshotToken: snapshotToken ?? null,
      }),
    ),
  );

export const getWorkspaceFileLines = (
  repoPath: string,
  workspaceId: number | null,
  filePath: string,
  fromParent: boolean,
  startLine: number,
  endLine: number,
): Promise<JjFileLines> =>
  transportGetWorkspaceFileLines(
    repoPath,
    workspaceId,
    filePath,
    fromParent,
    startLine,
    endLine,
    () =>
      invoke("get_workspace_file_lines", {
        repoPath,
        workspaceId,
        filePath,
        fromParent,
        startLine,
        endLine,
      }),
  );

export const jjRestoreFile = (
  workspacePath: string,
  filePath: string,
): Promise<string> =>
  transportRestoreFile(workspacePath, filePath, () =>
    invoke("jj_restore_file", {
      workspacePath,
      filePath,
    }),
  );

export const jjRestoreAll = (workspacePath: string): Promise<string> =>
  transportWorkingCopyMutation(
    workspacePath,
    (repo, workspace) => ({ kind: "RestoreAll", repo, workspace }),
    () => invoke("jj_restore_all", { workspacePath }),
  );

export const jjSnapshotWorkingCopy = (workspacePath: string): Promise<string> =>
  transportWorkingCopyMutation(
    workspacePath,
    (repo, workspace) => ({ kind: "SnapshotWorkingCopy", repo, workspace }),
    () => invoke("jj_snapshot_working_copy", { workspacePath }),
    // The id is the undo handle `jjRestoreSnapshot` takes; never invent one.
    { requireValue: true },
  );

export const jjRestoreSnapshot = (
  workspacePath: string,
  snapshotId: string,
): Promise<string> =>
  transportWorkingCopyMutation(
    workspacePath,
    (repo, workspace) => ({
      kind: "RestoreSnapshot",
      repo,
      workspace,
      snapshot_id: snapshotId,
    }),
    () => invoke("jj_restore_snapshot", { workspacePath, snapshotId }),
  );

export const createCommit = (
  repoPath: string,
  workspaceId: number | null,
  message: string,
): Promise<string> =>
  transportCreateCommit(repoPath, workspaceId, message, () =>
    invoke("create_commit", {
      repoPath,
      workspaceId,
      message,
    }),
  );

export const listCommits = (
  repoPath: string,
  workspaceId: number | null,
  includeTargetBranchHistory?: boolean,
  targetBranchLimit?: number,
  limit?: number,
): Promise<JjLogResult> =>
  transportListCommits(repoPath, workspaceId, () =>
    invoke("list_commits", {
      repoPath,
      workspaceId,
      includeTargetBranchHistory: includeTargetBranchHistory ?? false,
      targetBranchLimit: targetBranchLimit ?? null,
      limit: limit ?? null,
    }),
  );

export const jjSplit = (
  workspacePath: string,
  message: string,
  filePaths: string[],
): Promise<string> =>
  transportSplitWorkingCopy(workspacePath, message, filePaths, () =>
    invoke("jj_split", {
      workspacePath,
      message,
      filePaths,
    }),
  );

export const listRepoBranches = (repoPath: string): Promise<JjBranch[]> =>
  transportListRepoBranches(repoPath, () =>
    invoke("list_repo_branches", { repoPath }),
  );

export const switchRepoBranch = (
  repoPath: string,
  bookmarkName: string,
): Promise<string> =>
  transportSwitchRepoBranch(repoPath, bookmarkName, () =>
    invoke("switch_repo_branch", { repoPath, bookmarkName }),
  );

export const jjGitFetchBackground = (repoPath: string): Promise<void> =>
  transportGitFetch(repoPath, () =>
    invoke("jj_git_fetch_background", { repoPath }),
  );

export const pullWorkspaceFromRemote = (
  repoPath: string,
  workspaceId: number | null,
): Promise<PullWorkspaceResult> =>
  transportPullWorkspace(repoPath, workspaceId, () =>
    invoke("pull_workspace_from_remote", { repoPath, workspaceId }),
  );

export const checkBranchExists = (
  repoPath: string,
  branchName: string,
): Promise<BranchStatus> =>
  transportCheckBranchExists(repoPath, branchName, () =>
    invoke("jj_check_branch_exists", {
      repoPath,
      branchName,
    }),
  );

export const getCommitDiff = (
  repoPath: string,
  workspaceId: number | null,
  revision: string,
): Promise<JjRevisionDiff> =>
  transportGetCommitDiff(repoPath, workspaceId, revision, () =>
    invoke("get_commit_diff", { repoPath, workspaceId, revision }),
  );

export const getCommitFileDiff = (
  repoPath: string,
  workspaceId: number | null,
  revision: string,
  filePath: string,
): Promise<JjFileDiff> =>
  transportGetCommitFileDiff(repoPath, workspaceId, revision, filePath, () =>
    invoke("get_commit_file_diff", {
      repoPath,
      workspaceId,
      revision,
      filePath,
    }),
  );

export const jjGetCommitsAhead = async (
  workspacePath: string,
  targetBranch: string,
): Promise<JjCommitsAhead> => {
  assertLocalOperation(workspacePath, "Merge preview");
  return invoke("jj_get_commits_ahead", { workspacePath, targetBranch });
};

export const getWorkspaceDiff = (
  repoPath: string,
  workspaceId: number,
): Promise<JjRevisionDiff> =>
  enqueueJjExclusive(() =>
    transportGetWorkspaceDiff(repoPath, workspaceId, () =>
      invoke("get_workspace_diff", { repoPath, workspaceId }),
    ),
  );

interface HunkSpec {
  file_path: string;
  start_line: number;
  end_line: number;
}

export interface WorkspaceMoveRequest {
  files: string[];
  hunks: HunkSpec[];
  commits: string[];
}

export interface WorkspaceMoveResult {
  commits_moved: number;
  files_moved: number;
  hunks_applied: number;
  hunks_skipped: number;
  warnings: string[];
}

export const moveWorkspaceChanges = (
  repoPath: string,
  sourceBranch: string,
  destinationBranch: string,
  request: WorkspaceMoveRequest,
): Promise<WorkspaceMoveResult> =>
  transportMoveWorkspaceChanges(
    repoPath,
    sourceBranch,
    destinationBranch,
    request,
    () =>
      invoke("move_workspace_changes", {
        repoPath,
        sourceBranch,
        destinationBranch,
        request,
      }),
  );

export const renameWorkspace = (
  repoPath: string,
  workspaceId: number,
  newBranchName: string,
  dryRun: boolean,
): Promise<RenameWorkspaceResult> =>
  transportRenameWorkspace(repoPath, workspaceId, newBranchName, dryRun, () =>
    invoke("rename_workspace", {
      repoPath,
      workspaceId,
      newBranchName,
      dryRun,
    }),
  );

export const mergeWorkspace = async (
  repoPath: string,
  workspaceId: number,
  message: string,
  mergeStrategy: MergeStrategy,
  options: LocalMergeOptions = { rollup: false, push: false },
): Promise<MergeResult> => {
  assertLocalOperation(repoPath, "Merging a workspace");
  return invoke("merge_workspace", {
    repoPath,
    workspaceId,
    message,
    mergeStrategy,
    rollup: options.rollup,
    push: options.push,
  });
};

/** The preview diff; with `rollup`, everything the stack lands. */
export const getMergePreviewDiff = (
  repoPath: string,
  workspaceId: number,
  rollup: boolean,
): Promise<JjRevisionDiff> => {
  assertLocalOperation(repoPath, "Previewing a workspace merge");
  return invoke("get_merge_preview_diff", { repoPath, workspaceId, rollup });
};

/** Sync the target and stack locally after the PR was merged on GitHub. */
export const syncWorkspaceAfterRemoteMerge = (
  repoPath: string,
  workspaceId: number,
): Promise<MergeResult> => {
  assertLocalOperation(repoPath, "Syncing a merged workspace");
  return invoke("sync_workspace_after_remote_merge", {
    repoPath,
    workspaceId,
  });
};

export const getMergeLandingPlan = (
  repoPath: string,
  workspaceId: number,
  rollup: boolean,
): Promise<MergeLandingPlan> => {
  assertLocalOperation(repoPath, "Planning a workspace merge");
  return invoke("get_merge_landing_plan", { repoPath, workspaceId, rollup });
};

export const pushWorkspaceToRemote = (
  repoPath: string,
  workspaceId: number | null,
): Promise<string> =>
  transportPushWorkspace(repoPath, workspaceId, () =>
    invoke("push_workspace_to_remote", {
      repoPath,
      workspaceId,
    }),
  );

export const listWorkspaceStatuses = (
  repoPath: string,
): Promise<WorkspaceSidebarStatus[]> =>
  transportListWorkspaceStatuses(repoPath, () =>
    invoke("list_workspace_statuses", {
      repoPath,
    }),
  );

export const getWorkspaceStatus = (
  repoPath: string,
  workspaceId: number | null,
): Promise<WorkspaceStatus> =>
  transportGetWorkspaceStatus(repoPath, workspaceId, () =>
    invoke("get_workspace_status", {
      repoPath,
      workspaceId,
    }),
  );

export const updateWorkspace = (
  repoPath: string,
  workspaceId: number,
  targetBranch?: string,
  title?: string,
  description?: string,
): Promise<Workspace> =>
  transportUpdateWorkspace(
    repoPath,
    workspaceId,
    { targetBranch, title, description },
    () =>
      invoke("update_workspace", {
        repoPath,
        workspaceId,
        ...(targetBranch !== undefined && { targetBranch }),
        ...(title !== undefined && { title }),
        ...(description !== undefined && { description }),
      }),
  );

export const setWorkspaceTargetBranch = (
  repoPath: string,
  workspacePath: string,
  id: number,
  targetBranch: string,
): Promise<JjRebaseResult> =>
  transportSetWorkspaceTargetBranch(repoPath, id, targetBranch, () =>
    invoke("set_workspace_target_branch", {
      repoPath,
      workspacePath,
      id,
      targetBranch,
    }),
  );

// Kept for compatibility with existing mocks/consumers while unused in app runtime.
// Intentionally left empty.

export const checkAndRebaseWorkspaces = async (
  repoPath: string,
  workspaceId?: number | null,
  defaultBranch?: string | null,
  force?: boolean,
): Promise<SingleRebaseResult> => {
  assertLocalOperation(repoPath, "Rebasing workspaces");
  return invoke("check_and_rebase_workspaces", {
    repoPath,
    workspaceId: workspaceId ?? null,
    defaultBranch: defaultBranch ?? null,
    force: force ?? null,
  });
};

export const rebaseHomeRepoBranch = async (
  repoPath: string,
  currentBranch: string,
  targetBranch: string,
): Promise<JjRebaseResult> => {
  assertLocalOperation(repoPath, "Rebasing the repository branch");
  return invoke("rebase_home_repo_branch", {
    repoPath,
    currentBranch,
    targetBranch,
  });
};

export const dryRunHomeRepoRebase = async (
  repoPath: string,
  currentBranch: string,
  targetBranch: string,
): Promise<HomeRebaseDryRunResult> => {
  assertLocalOperation(repoPath, "Rebasing the repository branch");
  return invoke("dry_run_home_repo_rebase", {
    repoPath,
    currentBranch,
    targetBranch,
  });
};

// Remote SSH API

export const listSshHosts = (): Promise<SshHost[]> => invoke("list_ssh_hosts");

export const listLocalSshIdentities = (): Promise<LocalSshIdentity[]> =>
  invoke("list_local_ssh_identities");

/**
 * Reads the raw OpenSSH public-key text for a `listLocalSshIdentities`
 * `reference`, so it can be registered with the control plane. Never reads
 * or returns private key material.
 */
export const readLocalSshPublicKey = (reference: string): Promise<string> =>
  invoke("read_local_ssh_public_key", { reference });

export const listInstalledSkills = (
  repoPath?: string | null,
): Promise<InstalledSkill[]> =>
  invoke("list_installed_skills", { repoPath: localRepoPathOrNull(repoPath) });

// The Supabase account Rust owns in the desktop app (`rust-account.ts`).

/** The account, or null when Rust does not own it in this window. */
export const cloudAccount = (): Promise<RustAccount | null> =>
  invoke("cloud_account");

export const cloudSignOut = (): Promise<void> => invoke("cloud_sign_out");

export const cloudExchangeToken = (token: string): Promise<void> =>
  invoke("cloud_exchange_token", { token });

export const cloudRefreshSubscription = (): Promise<void> =>
  invoke("cloud_refresh_subscription");

/** Runs a cloud workspace action; an endpoint it connects comes back as `managed-endpoint`. */
export const cloudWorkspaceAction = (action: RustCloudAction): Promise<void> =>
  invoke("cloud_workspace_action", { action });
