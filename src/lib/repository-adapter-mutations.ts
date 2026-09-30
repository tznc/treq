// Transport-aware workspace, file, and commit mutations (PRD "Remote mutation
// coverage"). Each function takes the local Tauri call as a fallback and, for
// the active remote repository, sends the matching typed command through
// `remoteMutation` instead, so a remote path never reaches a local command.

/* eslint-disable max-params -- each wrapper mirrors its local invoke arity plus the fallback */

import type {
  JjRebaseResult,
  RenameWorkspaceResult,
  ResolveCommitResult,
  Workspace,
} from "./api-types";
import {
  activeForPath,
  remoteDispatch,
  remoteKeyedMutation,
  remoteMutation,
  RemoteOperationUnsupportedError,
  resolveRemoteLocation,
  workspaceArg,
} from "./repository-adapter";
import type { ActiveRepository } from "./active-repository";

interface MoveRequest {
  files: string[];
  hunks: { file_path: string; start_line: number; end_line: number }[];
  commits: string[];
}

interface MoveResult {
  commits_moved: number;
  files_moved: number;
  hunks_applied: number;
  hunks_skipped: number;
  warnings: string[];
}

const ALREADY_APPLIED = "Already applied before the connection dropped";

/** A rejected remote rename arrives as `invalid_arguments: <reason>`, with the code repeated over SSH. */
const REJECTED = /^(?:invalid_arguments: )+/;

async function findWorkspace(
  repo: ActiveRepository,
  predicate: (workspace: Workspace) => boolean,
): Promise<Workspace | undefined> {
  const workspaces = await remoteDispatch<Workspace[]>(repo, {
    kind: "ListWorkspaces",
    repo: repo.canonicalPath,
  });
  return workspaces.find(predicate);
}

export async function transportCreateWorkspace(
  repoPath: string,
  branchName: string,
  sourceBranch: string | undefined,
  metadata: string | undefined,
  local: () => Promise<number>,
): Promise<number> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const created = await remoteKeyedMutation<Workspace>(repo, {
    kind: "CreateWorkspace",
    repo: repo.canonicalPath,
    branch_name: branchName,
    source_branch: sourceBranch ?? null,
    metadata: metadata ?? null,
  });
  if (created) return created.id;
  // Landed before a reconnect: look the new workspace up by branch.
  const existing = await findWorkspace(
    repo,
    (ws) => ws.branch_name === branchName,
  );
  if (!existing) {
    throw new Error(`workspace_not_found: ${branchName} after reconnect`);
  }
  return existing.id;
}

export async function transportDeleteWorkspace(
  repoPath: string,
  id: number,
  local: () => Promise<void>,
): Promise<void> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  await remoteMutation(repo, {
    kind: "DeleteWorkspace",
    repo: repo.canonicalPath,
    workspace: String(id),
  });
}

export async function transportMoveWorkspaceChanges(
  repoPath: string,
  sourceBranch: string,
  destinationBranch: string,
  request: MoveRequest,
  local: () => Promise<MoveResult>,
): Promise<MoveResult> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const result = await remoteKeyedMutation<MoveResult>(repo, {
    kind: "MoveWorkspaceChanges",
    repo: repo.canonicalPath,
    workspace: sourceBranch,
    destination: destinationBranch,
    files: request.files,
    hunks: request.hunks,
    commits: request.commits,
  });
  return (
    result ?? {
      commits_moved: 0,
      files_moved: 0,
      hunks_applied: 0,
      hunks_skipped: 0,
      warnings: [ALREADY_APPLIED],
    }
  );
}

export async function transportRenameWorkspace(
  repoPath: string,
  workspaceId: number,
  newBranchName: string,
  dryRun: boolean,
  local: () => Promise<RenameWorkspaceResult>,
): Promise<RenameWorkspaceResult> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const accepted: RenameWorkspaceResult = {
    success: true,
    message: "",
    workspace: null,
    updated_children_ids: [],
  };
  // No typed dry-run command exists; the real rename below still returns the
  // remote's structured validation error.
  if (dryRun) return accepted;
  try {
    const result = await remoteKeyedMutation<RenameWorkspaceResult>(repo, {
      kind: "RenameWorkspace",
      repo: repo.canonicalPath,
      workspace: String(workspaceId),
      new_name: newBranchName,
    });
    return result ?? { ...accepted, message: ALREADY_APPLIED };
  } catch (error) {
    // The CLI exits non-zero on a rejected rename; report it like a local one.
    const message = error instanceof Error ? error.message : String(error);
    if (!REJECTED.test(message)) throw error;
    return {
      ...accepted,
      success: false,
      message: message.replace(REJECTED, ""),
    };
  }
}

export async function transportPushWorkspace(
  repoPath: string,
  workspaceId: number | null,
  local: () => Promise<string>,
): Promise<string> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const result = await remoteKeyedMutation<string>(repo, {
    kind: "GitPush",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
  });
  return result ?? ALREADY_APPLIED;
}

interface WorkspaceUpdate {
  targetBranch?: string;
  title?: string;
  description?: string;
}

export async function transportUpdateWorkspace(
  repoPath: string,
  workspaceId: number,
  update: WorkspaceUpdate,
  local: () => Promise<Workspace>,
): Promise<Workspace> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const result = await remoteMutation<Workspace>(repo, {
    kind: "UpdateWorkspace",
    repo: repo.canonicalPath,
    workspace: String(workspaceId),
    target_branch: update.targetBranch ?? null,
    title: update.title ?? null,
    description: update.description ?? null,
  });
  if (result) return result;
  const current = await findWorkspace(repo, (ws) => ws.id === workspaceId);
  if (!current) {
    throw new Error(`workspace_not_found: ${workspaceId} after reconnect`);
  }
  return current;
}

export async function transportSetWorkspaceTargetBranch(
  repoPath: string,
  workspaceId: number,
  targetBranch: string,
  local: () => Promise<JjRebaseResult>,
): Promise<JjRebaseResult> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  await remoteKeyedMutation(repo, {
    kind: "RebaseWorkspace",
    repo: repo.canonicalPath,
    workspace: String(workspaceId),
    target_branch: targetBranch,
  });
  return {
    success: true,
    message: `Retargeted workspace onto ${targetBranch}`,
  };
}

export async function transportRestoreFile(
  workspacePath: string,
  filePath: string,
  local: () => Promise<string>,
): Promise<string> {
  const location = await resolveRemoteLocation(workspacePath);
  if (!location) return local();
  const result = await remoteMutation<string>(location.repo, {
    kind: "RestoreFile",
    repo: location.repo.canonicalPath,
    workspace: location.workspace,
    path: filePath,
  });
  return result ?? ALREADY_APPLIED;
}

/**
 * Commits the selected whole files of a workspace's working copy with
 * `message`, leaving the rest in the working copy. Remote splits need a
 * workspace; the repository root has no workspace id to address.
 */
export async function transportSplitWorkingCopy(
  workspacePath: string,
  message: string,
  filePaths: string[],
  local: () => Promise<string>,
): Promise<string> {
  const location = await resolveRemoteLocation(workspacePath);
  if (!location) return local();
  if (location.workspace == null) {
    throw new RemoteOperationUnsupportedError(
      "Committing selected files in the repository root",
    );
  }
  const result = await remoteKeyedMutation<string>(location.repo, {
    kind: "SplitCommit",
    repo: location.repo.canonicalPath,
    workspace: location.workspace,
    commit: "@",
    files: filePaths,
    hunks: [],
    message,
  });
  return result ?? ALREADY_APPLIED;
}

export async function transportResolveCommit(
  repoPath: string,
  revision: string,
  sides: string[],
  local: () => Promise<ResolveCommitResult>,
): Promise<ResolveCommitResult> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const result = await remoteKeyedMutation<ResolveCommitResult>(repo, {
    kind: "ResolveConflict",
    repo: repo.canonicalPath,
    revision,
    sides,
  });
  if (result) return result;
  // An earlier attempt landed before a reconnect, but no typed read reports
  // which conflicts remain, so do not claim the change is fully resolved.
  throw new Error(
    `ambiguous: resolving ${revision} may have applied before the connection dropped; reopen its conflicts to check what remains`,
  );
}

interface CommitTarget {
  repoPath: string;
  workspaceId: number;
  commitChangeId: string;
}

export async function transportMoveCommit(
  target: CommitTarget,
  targetWorkspaceId: number,
  local: () => Promise<void>,
): Promise<void> {
  const repo = activeForPath(target.repoPath);
  if (!repo) return local();
  await remoteKeyedMutation(repo, {
    kind: "MoveCommit",
    repo: repo.canonicalPath,
    workspace: String(target.workspaceId),
    commit: target.commitChangeId,
    target_workspace: String(targetWorkspaceId),
  });
}

export async function transportAbandonCommit(
  target: CommitTarget,
  local: () => Promise<string>,
): Promise<string> {
  const repo = activeForPath(target.repoPath);
  if (!repo) return local();
  const result = await remoteKeyedMutation<string>(repo, {
    kind: "AbandonCommit",
    repo: repo.canonicalPath,
    workspace: String(target.workspaceId),
    commit: target.commitChangeId,
  });
  return result ?? "";
}

export async function transportDescribeCommit(
  target: CommitTarget,
  description: string,
  local: () => Promise<void>,
): Promise<void> {
  const repo = activeForPath(target.repoPath);
  if (!repo) return local();
  await remoteMutation(repo, {
    kind: "DescribeCommit",
    repo: repo.canonicalPath,
    workspace: String(target.workspaceId),
    commit: target.commitChangeId,
    message: description,
  });
}

/**
 * Reads a commit description. Remote repositories have no dedicated typed
 * read, so this finds the commit in the workspace log.
 */
export async function transportGetCommitDescription(
  target: CommitTarget,
  local: () => Promise<string>,
): Promise<string> {
  const repo = activeForPath(target.repoPath);
  if (!repo) return local();
  const log = await remoteDispatch<{
    commits?: { change_id: string; description: string }[];
  }>(repo, {
    kind: "ListCommits",
    repo: repo.canonicalPath,
    workspace: String(target.workspaceId),
  });
  const id = target.commitChangeId;
  const match = (log.commits ?? []).find(
    (commit) =>
      commit.change_id.startsWith(id) || id.startsWith(commit.change_id),
  );
  if (!match) {
    throw new Error(`commit_not_found: ${id}`);
  }
  return match.description;
}
