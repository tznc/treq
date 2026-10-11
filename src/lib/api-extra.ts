/* eslint-disable max-lines */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  CachedDirectoryEntry,
  DiffCacheEntry,
  DirectoryEntry,
  FileBrowserPendingReview,
  FileSearchResult,
  JjRevisionDiff,
  LineComment,
  PendingReview,
  PromptHistoryEntry,
  ResolveCommitResult,
  ResolveConflictsSession,
  Session,
  StashEntry,
} from "./api-types";
import type { ConflictCommentRecord } from "./api-types-review";
import {
  assertLocalOperation,
  remoteRepositoryContaining,
  transportReadFile,
  transportSearchWorkspaceFiles,
} from "./repository-adapter";
import {
  transportAbandonCommit,
  transportDescribeCommit,
  transportGetCommitDescription,
  transportMoveCommit,
  transportResolveCommit,
} from "./repository-adapter-mutations";
import { currentWindowLabel } from "./window-label";

export * from "./api-pr-status";

export const setGitSubmoduleSynced = (
  repoPath: string,
  path: string,
  enabled: boolean,
): Promise<void> =>
  invoke("set_git_submodule_synced", { repoPath, path, enabled });

export const startResolveConflicts = async (
  repoPath: string,
  workspaceId: number | null,
  changeIds?: string[] | null,
): Promise<ResolveConflictsSession> => {
  assertLocalOperation(repoPath, "Starting a conflict resolution session");
  return invoke("start_resolve_conflicts", {
    repoPath,
    workspaceId,
    changeIds: changeIds ?? null,
  });
};

export const buildResolveAgentPrompt = (
  userPrompt: string,
  session: ResolveConflictsSession,
): Promise<string> =>
  invoke("build_resolve_agent_prompt", {
    userPrompt,
    session,
  });

export const resolveCommit = (
  repoPath: string,
  revision: string,
  sides: string[],
): Promise<ResolveCommitResult> =>
  transportResolveCommit(repoPath, revision, sides, () =>
    invoke("resolve_commit", {
      repoPath,
      revision,
      sides,
    }),
  );

export const ptyCreateSession = (
  sessionId: string,
  workingDir?: string,
  shell?: string,
  initialCommand?: string,
  suppressEchoFor?: string,
  remoteHost?: string,
  repoPath?: string,
  workspaceId?: number | null,
): Promise<void> =>
  invoke("pty_create_session", {
    sessionId,
    windowLabel: currentWindowLabel(),
    workingDir,
    shell,
    initialCommand,
    suppressEchoFor,
    remoteHost: remoteHost ?? null,
    repoPath: repoPath ?? null,
    workspaceId: workspaceId ?? null,
  });

export const ptyWrite = (sessionId: string, data: string): Promise<void> =>
  invoke("pty_write", { sessionId, data });

export const ptyWriteSuppressEcho = (
  sessionId: string,
  data: string,
): Promise<void> => invoke("pty_write_suppress_echo", { sessionId, data });

export const ptyResize = (
  sessionId: string,
  rows: number,
  cols: number,
): Promise<void> => invoke("pty_resize", { sessionId, rows, cols });

export const ptyClose = (sessionId: string): Promise<void> =>
  invoke("pty_close", { sessionId });

export const ptySessionExists = (sessionId: string): Promise<boolean> =>
  invoke("pty_session_exists", { sessionId });

export const ptyListen = (
  sessionId: string,
  callback: (data: string) => void,
) =>
  listen<string>(`pty-data-${sessionId}`, (event) => callback(event.payload));

// -- Native remote PTY sessions (shell/agent over SSH) -----------------------
//
// Mirrors the local `pty*`/`ptyListen` API shape above so a caller that
// already knows the local terminal API is not learning a second vocabulary.
// `launch` must be a typed `PtyLaunchSpec` (see `api-types-remote.ts`) -
// never an arbitrary shell command string - so the backend's injection
// guard (typed launch fields, each individually shell-quoted) holds at the
// IPC boundary too.

export const remotePtyCreate = (
  sessionId: string,
  endpoint: unknown,
  repositoryId: string,
  workspaceId: string,
  remoteWorkingDirectory: string,
  launch: unknown,
  cols: number,
  rows: number,
): Promise<void> =>
  invoke("remote_pty_create", {
    sessionId,
    windowLabel: currentWindowLabel(),
    endpoint,
    repositoryId,
    workspaceId,
    remoteWorkingDirectory,
    launch,
    cols,
    rows,
  });

export const remotePtyWrite = (
  sessionId: string,
  data: string,
): Promise<void> => invoke("remote_pty_write", { sessionId, data });

export const remotePtyResize = (
  sessionId: string,
  cols: number,
  rows: number,
): Promise<void> => invoke("remote_pty_resize", { sessionId, cols, rows });

export const remotePtyClose = (sessionId: string): Promise<void> =>
  invoke("remote_pty_close", { sessionId });

// eslint-disable-next-line local/no-unused-exported-ts-functions, local/require-tauri-api-exports-used
export const remotePtySessionExists = (sessionId: string): Promise<boolean> =>
  invoke("remote_pty_session_exists", { sessionId });

/** Streamed output chunks, named `remote-pty-data-<sessionId>` (bytes decoded as UTF-8 on the backend). */
export const remotePtyListen = (
  sessionId: string,
  callback: (data: string) => void,
) =>
  listen<string>(`remote-pty-data-${sessionId}`, (event) =>
    callback(event.payload),
  );

/** Payload of the `remote-pty-exit-<sessionId>` event, emitted exactly once when the session's process/channel ends. */
export interface RemotePtyExitPayload {
  exit_status: number | null;
}

export const remotePtyListenExit = (
  sessionId: string,
  callback: (payload: RemotePtyExitPayload) => void,
) =>
  listen<RemotePtyExitPayload>(`remote-pty-exit-${sessionId}`, (event) =>
    callback(event.payload),
  );

// -- Persistent `pty-remote` sessions (Phase 8: desktop reattach parity) ---
//
// Wraps `remote_pty_list_persistent_sessions`/`remote_pty_reattach` — the
// VM-local `tmux`/`screen`-backed sessions that outlive any one desktop
// process's `remote_pty_create` connection, so a desktop client can list
// what is still running on the VM and reattach to it instead of always
// starting fresh (mirrors mobile's `pty-remote list`/reattach flow).

/** One persistent `pty-remote` session as reported by the VM's `pty-remote list` (`PtySessionInfo` in Rust). */
export interface RemotePersistentPtySession {
  session_name: string;
  workspace: string;
  label: string;
  running: boolean;
}

export const remotePtyListPersistentSessions = (
  endpoint: unknown,
  repo: string,
  workspaceId?: string | null,
): Promise<RemotePersistentPtySession[]> =>
  invoke("remote_pty_list_persistent_sessions", {
    endpoint,
    repo,
    workspaceId: workspaceId ?? null,
  });

export const remotePtyReattach = (
  sessionId: string,
  endpoint: unknown,
  repositoryId: string,
  workspaceId: string,
  label: string,
  remoteWorkingDirectory: string,
  launch: unknown,
  cols: number,
  rows: number,
): Promise<void> =>
  invoke("remote_pty_reattach", {
    sessionId,
    windowLabel: currentWindowLabel(),
    endpoint,
    repositoryId,
    workspaceId,
    label,
    remoteWorkingDirectory,
    launch,
    cols,
    rows,
  });

// File System API
export const readFile = (path: string): Promise<string> =>
  transportReadFile(path, () => invoke("read_file", { path }));

export const writeSendReviewImage = (
  repoPath: string,
  suggestedName: string,
  contentsBase64: string,
): Promise<string> =>
  invoke("write_send_review_image", {
    repoPath,
    suggestedName,
    contentsBase64,
  });

export const writeAgentCliFiles = (
  prompt: string,
  settingsJson?: string | null,
  cwd?: string | null,
): Promise<{
  promptPath: string;
  settingsPath?: string;
  skillDir: string;
  agentsSkillPath?: string;
  claudeSkillPath?: string;
  skillWriteWarning?: string;
}> =>
  invoke("write_agent_cli_files", {
    prompt,
    settingsJson: settingsJson ?? null,
    cwd: cwd ?? null,
  });

export const cleanupAgentCliFiles = (paths: string[]): Promise<void> =>
  invoke("cleanup_agent_cli_files", { paths });

export const getFileModifiedAt = (path: string): Promise<string | null> =>
  invoke("get_file_modified_at", { path });

export const listDirectory = (path: string): Promise<DirectoryEntry[]> =>
  invoke("list_directory", { path });

export interface DirectoryBatchResult {
  path: string;
  entries: DirectoryEntry[];
  error?: string;
}

export const listDirectoriesBatch = (
  paths: string[],
): Promise<DirectoryBatchResult[]> =>
  invoke("list_directories_batch", { paths });

export const lsWorkspaceWithStatus = (
  repoPath: string,
  workspaceId: number | null,
): Promise<DirectoryEntry[]> =>
  invoke("ls_workspace_with_status", { repoPath, workspaceId });

// Kept as the typed frontend counterpart of the registered Tauri command.
// eslint-disable-next-line local/no-unused-exported-ts-functions, local/require-tauri-api-exports-used
export const listDirectoryCached = (
  repoPath: string,
  workspaceId: number | null,
  parentPath: string,
): Promise<CachedDirectoryEntry[]> =>
  invoke("list_directory_cached", { repoPath, workspaceId, parentPath });

export interface SendArtifactRecord {
  id: string;
  repo: string;
  pty_session_id?: string | null;
  media_type: string;
  path: string;
  title?: string | null;
  received_at: number;
}

export const listSendArtifacts = (
  repoPath: string,
): Promise<SendArtifactRecord[]> => invoke("list_send_artifacts", { repoPath });

export const searchWorkspaceFiles = (
  repoPath: string,
  workspaceId: number | null,
  query: string,
  limit?: number,
): Promise<FileSearchResult[]> =>
  transportSearchWorkspaceFiles(repoPath, workspaceId, query, limit ?? 50, () =>
    invoke("search_workspace_files", {
      repoPath,
      workspaceId,
      query,
      limit: limit ?? 50,
    }),
  );

// Folder picker
export const selectFolder = async (): Promise<string | null> =>
  open({
    directory: true,
    multiple: false,
    title: "Select Folder",
  });

// Session management API
export const createSession = (
  repoPath: string,
  workspaceId: number | null,
  name: string,
): Promise<number> => invoke("create_session", { repoPath, workspaceId, name });

export type AgentDispatchAcknowledgmentStatus = "accepted" | "rejected";

export const acknowledgeAgentDispatch = (
  requestId: string,
  status: AgentDispatchAcknowledgmentStatus,
  reason?: string,
): Promise<void> =>
  invoke("acknowledge_agent_dispatch", {
    requestId,
    status,
    reason: reason ?? null,
  });
export const getSessions = (repoPath: string): Promise<Session[]> =>
  invoke("get_sessions", { repoPath });

export const updateSessionAccess = (
  repoPath: string,
  id: number,
): Promise<void> => invoke("update_session_access", { repoPath, id });

export const getSessionModel = (
  repoPath: string,
  id: number,
): Promise<string | null> => invoke("get_session_model", { repoPath, id });

export const setSessionModel = (
  repoPath: string,
  id: number,
  model: string | null,
): Promise<void> => invoke("set_session_model", { repoPath, id, model });

// Prompt history API
export const addPromptHistory = (
  repoPath: string,
  workspaceId: number | null,
  sessionId: number | null,
  promptText: string,
  agent?: string | null,
): Promise<number> =>
  invoke("add_prompt_history", {
    repoPath,
    workspaceId,
    sessionId,
    promptText,
    agent: agent ?? null,
  });

export const getPromptHistory = (
  repoPath: string,
): Promise<PromptHistoryEntry[]> => invoke("get_prompt_history", { repoPath });

export const getWorkspaceStartingPrompt = (
  repoPath: string,
  workspaceId: number,
): Promise<PromptHistoryEntry | null> =>
  invoke("get_workspace_starting_prompt", { repoPath, workspaceId });

// Stash API — immutable local gist storage for working-copy change sets
export const stashWorkspaceChanges = (
  repoPath: string,
  workspaceId: number | null,
): Promise<StashEntry> =>
  invoke("stash_workspace_changes", { repoPath, workspaceId });

export const stashCommit = (
  repoPath: string,
  workspaceId: number | null,
  changeId: string,
): Promise<StashEntry> =>
  invoke("stash_commit", { repoPath, workspaceId, changeId });

export const listStashes = (repoPath: string): Promise<StashEntry[]> =>
  invoke("list_stashes", { repoPath });

export const deleteStash = (repoPath: string, stashId: number): Promise<void> =>
  invoke("delete_stash", { repoPath, stashId });

export const applyStash = (
  repoPath: string,
  stashId: number,
  targetBranch: string,
): Promise<void> => invoke("apply_stash", { repoPath, stashId, targetBranch });

export const getStashDiff = (
  repoPath: string,
  stashId: number,
): Promise<JjRevisionDiff> => invoke("get_stash_diff", { repoPath, stashId });

export const exportStashGitPatch = (
  repoPath: string,
  stashId: number,
): Promise<string> => invoke("export_stash_git_patch", { repoPath, stashId });

export const markFileViewed = (
  workspacePath: string,
  filePath: string,
  contentHash: string,
): Promise<void> =>
  invoke("mark_file_viewed", { workspacePath, filePath, contentHash });

export const unmarkFileViewed = (
  workspacePath: string,
  filePath: string,
): Promise<void> => invoke("unmark_file_viewed", { workspacePath, filePath });

// Diff cache API (in-memory stub implementation)
const diffCache = new Map<string, { data: string; timestamp: number }>();

export const getDiffCache = async (
  workspacePath: string,
  cacheType: string,
  filePath?: string,
): Promise<DiffCacheEntry | null> => {
  const key = filePath
    ? `${workspacePath}:${cacheType}:${filePath}`
    : `${workspacePath}:${cacheType}`;
  return diffCache.get(key) ?? null;
};

export const loadPendingReview = (
  repoPath: string,
  workspaceId: number,
): Promise<PendingReview | null> =>
  invoke("load_pending_review", { repoPath, workspaceId }).then((review) => {
    if (!review) return null;
    const normalized = { ...review } as PendingReview & {
      comments: unknown;
      viewed_files: unknown;
      conflict_comments: unknown;
    };
    if (typeof normalized.comments === "string") {
      normalized.comments = JSON.parse(normalized.comments);
    }
    if (typeof normalized.viewed_files === "string") {
      normalized.viewed_files = JSON.parse(normalized.viewed_files);
    }
    normalized.conflict_comments =
      typeof normalized.conflict_comments === "string"
        ? JSON.parse(normalized.conflict_comments)
        : [];
    return normalized as PendingReview;
  });

export const savePendingReview = (
  repoPath: string,
  workspaceId: number,
  comments: LineComment[],
  viewedFiles?: string[],
  summaryText?: string,
  conflictComments?: ConflictCommentRecord[],
): Promise<number> =>
  invoke("save_pending_review", {
    repoPath,
    workspaceId,
    comments: JSON.stringify(comments),
    viewedFiles: viewedFiles ? JSON.stringify(viewedFiles) : null,
    summaryText: summaryText ?? null,
    conflictComments:
      conflictComments && conflictComments.length > 0
        ? JSON.stringify(conflictComments)
        : null,
  });

export const clearPendingReview = (
  repoPath: string,
  workspaceId: number,
): Promise<void> => invoke("clear_pending_review", { repoPath, workspaceId });

// The file browser's draft review lives in the repository's `.treq/local.db`.
// No typed command exposes that table on a remote host, so a remote draft is
// kept in memory for this session instead of writing a local database for a
// path that only exists remotely.
const remoteFileBrowserReviews = new Map<string, FileBrowserPendingReview>();

function remoteReviewKey(repoPath: string, workspaceId: number): string | null {
  return remoteRepositoryContaining(repoPath)
    ? JSON.stringify([repoPath, workspaceId])
    : null;
}

export const loadFileBrowserReview = async (
  repoPath: string,
  workspaceId: number,
): Promise<FileBrowserPendingReview | null> => {
  const remoteKey = remoteReviewKey(repoPath, workspaceId);
  if (remoteKey) return remoteFileBrowserReviews.get(remoteKey) ?? null;
  return invoke("load_file_browser_review", { repoPath, workspaceId }).then(
    (review) => {
      if (!review) return null;
      const normalized = { ...review } as FileBrowserPendingReview & {
        comments: unknown;
      };
      if (typeof normalized.comments === "string") {
        normalized.comments = JSON.parse(normalized.comments);
      }
      return normalized as FileBrowserPendingReview;
    },
  );
};

export const saveFileBrowserReview = async (
  repoPath: string,
  workspaceId: number,
  comments: LineComment[],
  summaryText?: string,
): Promise<number> => {
  const remoteKey = remoteReviewKey(repoPath, workspaceId);
  if (remoteKey) {
    const now = new Date().toISOString();
    const previous = remoteFileBrowserReviews.get(remoteKey);
    remoteFileBrowserReviews.set(remoteKey, {
      id: previous?.id ?? remoteFileBrowserReviews.size + 1,
      workspace_id: workspaceId,
      comments,
      summary_text: summaryText ?? null,
      created_at: previous?.created_at ?? now,
      updated_at: now,
    });
    return remoteFileBrowserReviews.get(remoteKey)!.id;
  }
  return invoke("save_file_browser_review", {
    repoPath,
    workspaceId,
    comments: JSON.stringify(comments),
    summaryText: summaryText ?? null,
  });
};

export const clearFileBrowserReview = async (
  repoPath: string,
  workspaceId: number,
): Promise<void> => {
  const remoteKey = remoteReviewKey(repoPath, workspaceId);
  if (remoteKey) {
    remoteFileBrowserReviews.delete(remoteKey);
    return;
  }
  return invoke("clear_file_browser_review", { repoPath, workspaceId });
};

// File Watcher API
export const startFileWatcher = (
  workspaceId: number,
  workspacePath: string,
): Promise<void> =>
  invoke("start_file_watcher", { workspaceId, workspacePath });

export const stopFileWatcher = (
  workspaceId: number,
  workspacePath: string,
): Promise<void> => invoke("stop_file_watcher", { workspaceId, workspacePath });

// Wrapper required by local/require-tauri-api-command-wrappers; moveCommitToExistingWorkspace
// is not yet called from the UI.
export const moveCommitToExistingWorkspace = (
  repoPath: string,
  sourceWorkspaceId: number,
  commitChangeId: string,
  targetWorkspaceId: number,
): Promise<void> =>
  transportMoveCommit(
    { repoPath, workspaceId: sourceWorkspaceId, commitChangeId },
    targetWorkspaceId,
    () =>
      invoke("move_commit_to_existing_workspace", {
        repoPath,
        sourceWorkspaceId,
        commitChangeId,
        targetWorkspaceId,
      }),
  );

export const abandonCommit = (
  repoPath: string,
  workspaceId: number,
  commitChangeId: string,
): Promise<string> =>
  transportAbandonCommit({ repoPath, workspaceId, commitChangeId }, () =>
    invoke("abandon_commit", {
      repoPath,
      workspaceId,
      commitChangeId,
    }),
  );

export const undoRepoOperation = async (
  repoPath: string,
  workspaceId: number | null,
  operationId: string,
): Promise<string> => {
  assertLocalOperation(repoPath, "Undoing an operation");
  return invoke("undo_repo_operation", {
    repoPath,
    workspaceId,
    operationId,
  });
};

/**
 * Undo the latest commit in a workspace's own lineage (not the working copy,
 * not a commit on the target branch). Must be undone sequentially from the tip.
 */
export const undoCommit = async (
  repoPath: string,
  workspaceId: number,
  commitChangeId: string,
): Promise<void> => {
  assertLocalOperation(repoPath, "Undoing a commit");
  return invoke("undo_commit", {
    repoPath,
    workspaceId,
    commitChangeId,
  });
};

/**
 * Revert a commit by creating a new commit that reverses its changes on top
 * of the workspace's current tip. Can target any commit except the working copy.
 */
export const revertCommit = async (
  repoPath: string,
  workspaceId: number,
  commitChangeId: string,
): Promise<void> => {
  assertLocalOperation(repoPath, "Reverting a commit");
  return invoke("revert_commit", {
    repoPath,
    workspaceId,
    commitChangeId,
  });
};

export const getCommitDescription = (
  repoPath: string,
  workspaceId: number,
  commitChangeId: string,
): Promise<string> =>
  transportGetCommitDescription({ repoPath, workspaceId, commitChangeId }, () =>
    invoke("get_commit_description", {
      repoPath,
      workspaceId,
      commitChangeId,
    }),
  );

export const describeCommit = (
  repoPath: string,
  workspaceId: number,
  commitChangeId: string,
  description: string,
): Promise<void> =>
  transportDescribeCommit(
    { repoPath, workspaceId, commitChangeId },
    description,
    () =>
      invoke("describe_commit", {
        repoPath,
        workspaceId,
        commitChangeId,
        description,
      }),
  );

export const shiftCommitTimestamp = async (
  repoPath: string,
  workspaceId: number,
  commitChangeId: string,
  shift: {
    days?: number;
    hours?: number;
    minutes?: number;
    toDay?: string | null;
  },
): Promise<void> => {
  assertLocalOperation(repoPath, "Shifting commit timestamps");
  return invoke("shift_commit_timestamp", {
    repoPath,
    workspaceId,
    commitChangeId,
    days: shift.days ?? 0,
    hours: shift.hours ?? 0,
    minutes: shift.minutes ?? 0,
    toDay: shift.toDay ?? null,
  });
};

export const shiftMutableCommitsToNow = async (
  repoPath: string,
  workspaceId: number,
): Promise<void> => {
  assertLocalOperation(repoPath, "Shifting commit timestamps");
  return invoke("shift_mutable_commits_to_now", {
    repoPath,
    workspaceId,
  });
};

export const getTreqBinDir = (): Promise<string> => invoke("get_treq_bin_dir");

/** Settings parsed from a repository's `.treq/config.yaml`, if present. */
export interface RepoYamlConfig {
  branch_name_pattern: string | null;
  default_model: string | null;
  default_agent: string | null;
  target_branch: string | null;
  included_copy_files: string[] | null;
  review_prompt: string | null;
  review_agent: string | null;
  auto_review_trigger: string | null;
}

/**
 * Typed remote command dispatch. `request` must be a fully typed
 * `TreqCommandRequest` variant from `remote-dispatch.ts` - never a raw
 * string - so the allow-list holds at the IPC boundary the same way it
 * holds inside the exec transport.
 */
export const remoteDispatchLocal = <T = unknown>(
  request: unknown,
): Promise<T> => invoke("remote_dispatch_local", { request });

/** Same as {@link remoteDispatchLocal}, but runs over the SSH exec channel for `endpoint`. */
export const remoteDispatchOverSsh = <T = unknown>(
  endpoint: unknown,
  request: unknown,
): Promise<T> => invoke("remote_dispatch_over_ssh", { endpoint, request });

/**
 * Runs a *mutating* `TreqCommandRequest` over the SSH exec channel with
 * verify-before-retry semantics (PRD "Retrying after network loss"): a
 * network failure while the command was in flight never triggers a blind
 * resend. The result is a discriminated union (see `MutationDispatchResult`
 * in `remote-dispatch.ts`) so the caller can render "already applied",
 * "applied" (first try or a verified retry), or "ambiguous - ask the user"
 * distinctly instead of guessing from a thrown error.
 */
export const remoteDispatchMutationOverSsh = <T = unknown>(
  endpoint: unknown,
  request: unknown,
): Promise<T> =>
  invoke("remote_dispatch_mutation_over_ssh", { endpoint, request });

/**
 * Snapshot of this session's SSH transport telemetry (PRD Phase 7 "Client
 * and transport telemetry"). Kept as the typed frontend counterpart of the
 * registered Tauri command; a diagnostics panel to display it is future
 * work.
 */
// eslint-disable-next-line local/no-unused-exported-ts-functions, local/require-tauri-api-exports-used
export const remoteTransportMetrics = <T = unknown>(): Promise<T> =>
  invoke("remote_transport_metrics");

/**
 * Forces the native SSH transport's hard cutoff for `endpointId` (PRD "Hard
 * cutoff on revocation or expiry"): tears down open exec/PTY channels and
 * refuses further commands until {@link remoteClearCutoff} is called. Called
 * by the certificate-renewal loop in `src/lib/remote-cert-lifecycle.ts`.
 */
export const remoteForceCutoff = (
  endpointId: string,
  reason: string,
): Promise<void> => invoke("remote_force_cutoff", { endpointId, reason });

/**
 * Cuts off every managed endpoint on sign-out: tears down pooled managed
 * connections and PTYs, emits `remote://cutoff` for each, and refuses new
 * managed connections until a signed-in token reaches the native pool.
 */
export const remoteCutOffManaged = (): Promise<void> =>
  invoke("remote_cut_off_managed");

/**
 * Clears a previously forced cutoff after the user reauthenticates and a
 * fresh certificate is issued through the normal registration and issuance
 * flow. Used by `src/stores/remoteCutoffStore.ts`.
 */
export const remoteClearCutoff = (endpointId: string): Promise<void> =>
  invoke("remote_clear_cutoff", { endpointId });

/**
 * Gives the native SSH pool the Supabase access token it sends to the
 * `remote-ssh-relay` Edge Function; `null` clears it on sign-out. Used by
 * `src/lib/remote-relay-auth.ts`.
 */
export const remoteSetRelayAccessToken = (
  token: string | null,
): Promise<void> => invoke("remote_set_relay_access_token", { token });

/**
 * Returns the current cutoff reason for `endpointId`, if any - the
 * synchronous counterpart to the `remote://cutoff` event, for a component
 * that wants to check state on mount.
 */
// eslint-disable-next-line local/no-unused-exported-ts-functions, local/require-tauri-api-exports-used
export const remoteCutoffReason = (
  endpointId: string,
): Promise<string | null> => invoke("remote_cutoff_reason", { endpointId });

export type SkillInstallScope = "application" | "repository";

export interface InstalledSkill {
  id: string;
  name: string;
  checksum: string;
  scope: SkillInstallScope;
}

export interface SkillCatalogFile {
  path: string;
  size: number;
  binary: boolean;
  githubUrl?: string | null;
  rawUrl?: string | null;
}

export interface SkillCatalogSkill {
  id: string;
  name: string;
  description?: string | null;
  source: string;
  category?: string | null;
  license?: string | null;
  proprietary: boolean;
  url?: string | null;
  checksum?: string | null;
  files: SkillCatalogFile[];
  installed?: InstalledSkill | null;
}

export interface SkillCatalogView {
  generatedAt?: string | null;
  sources: unknown[];
  skills: SkillCatalogSkill[];
}

export const listSkillCatalog = (
  repoPath?: string | null,
  catalogUrl?: string | null,
): Promise<SkillCatalogView> =>
  invoke("list_skill_catalog", {
    repoPath: repoPath ?? null,
    catalogUrl: catalogUrl ?? null,
  });

export const installSkill = (
  skillId: string,
  scope: SkillInstallScope,
  repoPath?: string | null,
  catalogUrl?: string | null,
): Promise<InstalledSkill> =>
  invoke("install_skill", {
    skillId,
    scope,
    repoPath: repoPath ?? null,
    catalogUrl: catalogUrl ?? null,
  });

export const uninstallSkill = (
  skillId: string,
  repoPath?: string | null,
): Promise<void> =>
  invoke("uninstall_skill", {
    skillId,
    repoPath: repoPath ?? null,
  });

export const setSkillInstallScope = (
  skillId: string,
  scope: SkillInstallScope,
  repoPath?: string | null,
): Promise<InstalledSkill> =>
  invoke("set_skill_install_scope", {
    skillId,
    scope,
    repoPath: repoPath ?? null,
  });
