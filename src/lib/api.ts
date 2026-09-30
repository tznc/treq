/* eslint-disable max-lines */
import { invoke } from "@tauri-apps/api/core";
import type { InstalledSkill, RepoYamlConfig } from "./api-extra";
import type {
  BookmarkConflictResolutionResult,
  BranchStatus,
  DeviceKeyInfo,
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
import {
  assertLocalOperation,
  transportCreateCommit,
  transportGetCommitDiff,
  transportGetCommitFileDiff,
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
import {
  transportCreateWorkspace,
  transportDeleteWorkspace,
  transportMoveWorkspaceChanges,
  transportPushWorkspace,
  transportRenameWorkspace,
  transportRestoreFile,
  transportSetWorkspaceTargetBranch,
  transportSplitWorkingCopy,
  transportUpdateWorkspace,
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
export const initRepo = (repoPath: string): Promise<boolean> =>
  invoke("init_repo", { repoPath });

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

export const archiveWorkspace = async (
  repoPath: string,
  id: number,
): Promise<void> => {
  assertLocalOperation(repoPath, "Archiving a workspace");
  return invoke("archive_workspace", {
    repoPath,
    id,
  });
};

export const ensureWorkspaceIndexed = (
  repoPath: string,
  workspaceId: number | null,
  workspacePath: string,
): Promise<boolean> =>
  invoke("ensure_workspace_indexed", {
    repoPath,
    workspaceId,
    workspacePath,
  });

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
  last_hash: string | null;
  last_run_at: string | null;
  last_status: "passed" | "failed" | null;
  running: boolean;
}

export const getAppSetupScriptStatus = (): Promise<AppSetupScriptStatus> =>
  invoke("get_app_setup_script_status");

/** Saves the script and runs it in the background when its hash changed. */
export const saveAppSetupScript = (
  script: string,
  alwaysRun: boolean,
): Promise<void> => invoke("save_app_setup_script", { script, alwaysRun });

export const getRepoSetting = (
  repoPath: string,
  key: string,
): Promise<string | null> => invoke("get_repo_setting", { repoPath, key });

export const setRepoSetting = (
  repoPath: string,
  key: string,
  value: string,
): Promise<void> => invoke("set_repo_setting", { repoPath, key, value });

export const listAgentReviewComments = (
  repoPath: string,
  targetType: string,
  targetId: string,
): Promise<import("./api-types-review").AgentReviewComment[]> =>
  invoke("list_agent_review_comments", { repoPath, targetType, targetId });

export const resolveAgentReviewComment = (
  repoPath: string,
  commentId: string,
): Promise<void> =>
  invoke("resolve_agent_review_comment", { repoPath, commentId });

export const deleteAgentReviewComment = (
  repoPath: string,
  commentId: string,
): Promise<void> =>
  invoke("delete_agent_review_comment", { repoPath, commentId });

export const applyAgentReviewSuggestion = (
  repoPath: string,
  commentId: string,
): Promise<void> =>
  invoke("apply_agent_review_suggestion", { repoPath, commentId });

export const loadRepoYamlConfig = (repoPath: string): Promise<RepoYamlConfig> =>
  invoke("load_repo_yaml_config", { repoPath });

export const setWindowRepoPath = (repoPath: string): Promise<void> =>
  invoke("set_window_repo_path", {
    repoPath,
    windowLabel: currentWindowLabel(),
  });

export const detectEditorApps = (): Promise<EditorAppsResponse> =>
  invoke("detect_editor_apps");

export const checkPrerequisites = (): Promise<PrerequisiteStatus[]> =>
  invoke("check_prerequisites");

export const getGitRemoteUrl = (
  repoPath: string,
): Promise<GitRemoteInfo | null> => invoke("get_git_remote_url", { repoPath });

export const getPrChecksViaGh = (
  repoPath: string,
  branchName: string,
): Promise<PrCiStatus | null> =>
  invoke("get_pr_checks_via_gh", { repoPath, branchName });

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
  invoke("list_gitignored_path_suggestions", { repoPath });

export const getWorkspaceReadme = (
  repoPath: string,
  workspaceId: number | null,
): Promise<string | null> =>
  invoke("get_workspace_readme", { repoPath, workspaceId });

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

export const jjRestoreAll = async (workspacePath: string): Promise<string> => {
  assertLocalOperation(workspacePath, "Discarding all changes");
  return invoke("jj_restore_all", { workspacePath });
};

export const jjSnapshotWorkingCopy = async (
  workspacePath: string,
): Promise<string> => {
  assertLocalOperation(workspacePath, "Snapshotting the working copy");
  return invoke("jj_snapshot_working_copy", { workspacePath });
};

export const jjRestoreSnapshot = async (
  workspacePath: string,
  snapshotId: string,
): Promise<string> => {
  assertLocalOperation(workspacePath, "Restoring a working-copy snapshot");
  return invoke("jj_restore_snapshot", { workspacePath, snapshotId });
};

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

export const switchRepoBranch = async (
  repoPath: string,
  bookmarkName: string,
): Promise<string> => {
  assertLocalOperation(repoPath, "Switching the repository branch");
  return invoke("switch_repo_branch", {
    repoPath,
    bookmarkName,
  });
};

export interface SyncStatus {
  ahead: number;
  behind: number;
}

export const jjGitFetchBackground = (repoPath: string): Promise<void> =>
  transportGitFetch(repoPath, () =>
    invoke("jj_git_fetch_background", { repoPath }),
  );

export const pullWorkspaceFromRemote = async (
  repoPath: string,
  workspaceId: number | null,
): Promise<PullWorkspaceResult> => {
  assertLocalOperation(repoPath, "Pulling a workspace");
  return invoke("pull_workspace_from_remote", {
    repoPath,
    workspaceId,
  });
};

export const checkBranchExists = (
  repoPath: string,
  branchName: string,
): Promise<BranchStatus> =>
  invoke("jj_check_branch_exists", {
    repoPath,
    branchName,
  });

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

export interface HunkSpec {
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
): Promise<void> => {
  assertLocalOperation(repoPath, "Merging a workspace");
  return invoke("merge_workspace", {
    repoPath,
    workspaceId,
    message,
    mergeStrategy,
  });
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

export const scheduleWorkspaces = async (
  repoPath: string,
  workspaceIds: number[],
  hiddenUntil: string | null,
): Promise<Workspace[]> => {
  assertLocalOperation(repoPath, "Scheduling workspaces");
  return invoke("schedule_workspaces", {
    repoPath,
    workspaceIds,
    hiddenUntil,
  });
};

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

export const resolveBookmarkConflict = async (
  repoPath: string,
  workspaceId: number,
  workspacePath: string,
  branchName: string,
): Promise<BookmarkConflictResolutionResult> => {
  assertLocalOperation(repoPath, "Resolving a bookmark conflict");
  return invoke("resolve_workspace_bookmark_conflict", {
    repoPath,
    workspaceId,
    workspacePath,
    branchName,
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

/**
 * Generates (on first call) or loads this device's ed25519 keypair for
 * control-plane registration and certificate-based SSH auth. Mobile-only in
 * practice - desktop uses the user's existing `~/.ssh` identities via
 * `listLocalSshIdentities` instead.
 */
export const ensureMobileDeviceKey = (): Promise<DeviceKeyInfo> =>
  invoke("ensure_mobile_device_key");

export const listInstalledSkills = (
  repoPath?: string | null,
): Promise<InstalledSkill[]> =>
  invoke("list_installed_skills", { repoPath: repoPath ?? null });
