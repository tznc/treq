import type {
  FileSearchResult,
  JjDiffHunk,
  JjFileChange,
  JjFileDiff,
  JjFileLines,
  JjLogResult,
  JjRevisionDiff,
  RepoBranch,
  Workspace,
  WorkspaceSidebarStatus,
  WorkspaceStatus,
} from "./api-types";
import type { SshEndpoint, WorkspaceChangeMarker } from "./api-types-remote";
import {
  matchesActiveCanonicalPath,
  peekActiveRepository,
  repositoryCacheKey,
  type ActiveRepository,
} from "./active-repository";
import {
  dispatch,
  dispatchMutationOverSsh,
  type MutationDispatchResult,
  type TreqCommandRequest,
} from "./remote-dispatch";
import { remoteActionKeys, type UnkeyedRequest } from "./remote-idempotency";
import { applyMutationDispatchResult } from "./remote-mutation-ui";
import { useRemoteCutoffStore } from "../stores/remoteCutoffStore";
import type { CutoffReason } from "./remote-cert-lifecycle";

export function workspaceArg(
  workspaceId: number | null | undefined,
): string | null {
  if (workspaceId == null) return null;
  return String(workspaceId);
}

type SshActiveRepository = ActiveRepository & {
  transport: { type: "ssh"; endpoint: SshEndpoint };
};

/** Raised for a remote repository whose SSH endpoint is not resolved. */
export class RemoteEndpointUnresolvedError extends Error {
  constructor(displayName: string) {
    super(
      `endpoint_unresolved: ${displayName} has no SSH endpoint; reopen it from the remote setup dialog`,
    );
    this.name = "RemoteEndpointUnresolvedError";
  }
}

function requireSshTransport(repo: ActiveRepository): SshActiveRepository {
  if (repo.transport.type !== "ssh") {
    throw new RemoteEndpointUnresolvedError(repo.displayName);
  }
  return repo as SshActiveRepository;
}

// Local repositories run locally; a remote one routes over its endpoint, and
// one without an endpoint throws rather than falling through to this machine.
export function activeForPath(repoPath: string): SshActiveRepository | null {
  const active = peekActiveRepository();
  if (!active || active.transport.type === "local") return null;
  if (!matchesActiveCanonicalPath(active, repoPath)) return null;
  return requireSshTransport(active);
}

function trimTrailingSlashes(path: string): string {
  return path.length > 1 ? path.replace(/\/+$/, "") : path;
}

/**
 * The active remote repository when `path` is its root or any path inside
 * it (a workspace directory or a file). Paths like these must never reach a
 * local Tauri command: the local machine does not have them.
 */
export function remoteRepositoryContaining(
  path: string | null | undefined,
): ActiveRepository | null {
  if (!path) return null;
  const active = peekActiveRepository();
  if (!active || active.transport.type === "local") return null;
  if (matchesActiveCanonicalPath(active, path)) return active;
  const root = trimTrailingSlashes(active.canonicalPath);
  return path.startsWith(`${root}/`) ? active : null;
}

/** Raised instead of running a remote repository operation on this machine. */
export class RemoteOperationUnsupportedError extends Error {
  constructor(operation: string) {
    super(
      `unsupported: ${operation} is not available for remote repositories yet`,
    );
    this.name = "RemoteOperationUnsupportedError";
  }
}

/**
 * Guards a local-only operation. When `path` belongs to the active remote
 * repository the operation fails with a structured error rather than running
 * against a path that only exists on the remote host.
 */
export function assertLocalOperation(
  path: string | null | undefined,
  operation: string,
): void {
  if (remoteRepositoryContaining(path)) {
    throw new RemoteOperationUnsupportedError(operation);
  }
}

interface RemoteLocation {
  repo: ActiveRepository;
  /** Workspace id as the typed commands expect it, or null for the root. */
  workspace: string | null;
  /** Path relative to the workspace (or repository) root. */
  relative: string;
}

const WORKSPACES_DIR = ".treq/workspaces/";

/**
 * Maps an absolute remote path (repository root, workspace directory, or a
 * file inside either) to the repository, workspace id, and relative path
 * the typed commands take. Returns null for paths outside the active remote
 * repository.
 */
export async function resolveRemoteLocation(
  path: string,
): Promise<RemoteLocation | null> {
  const repo = remoteRepositoryContaining(path);
  if (!repo) return null;
  const root = trimTrailingSlashes(repo.canonicalPath);
  const trimmed = trimTrailingSlashes(path);
  if (trimmed === root || matchesActiveCanonicalPath(repo, path)) {
    return { repo, workspace: null, relative: "" };
  }
  const rest = trimmed.slice(root.length + 1);
  if (!rest.startsWith(WORKSPACES_DIR)) {
    return { repo, workspace: null, relative: rest };
  }
  const [dirName, ...tail] = rest.slice(WORKSPACES_DIR.length).split("/");
  const workspaces = await remoteDispatch<Workspace[]>(repo, {
    kind: "ListWorkspaces",
    repo: repo.canonicalPath,
  });
  const match = workspaces.find((ws) => ws.workspace_path === dirName);
  if (!match) {
    throw new Error(`workspace_not_found: no remote workspace at ${dirName}`);
  }
  return { repo, workspace: String(match.id), relative: tail.join("/") };
}

/** Maps the Rust `CutoffReason` display text carried in a
 * `credential_cut_off: endpoint <id> (<reason>)` error to its reason. */
const CUTOFF_REASON_TEXT: [string, CutoffReason][] = [
  ["(session ended)", "session_ended"],
  ["(client key revoked)", "key_revoked"],
  ["(instance no longer accessible)", "instance_inaccessible"],
];

function cutoffReasonFromError(error: unknown): CutoffReason | null {
  const message = error instanceof Error ? error.message : String(error);
  if (
    !message.includes("credential_cut_off") &&
    !message.includes("CredentialCutOff")
  ) {
    return null;
  }
  const match = CUTOFF_REASON_TEXT.find(([text]) => message.includes(text));
  return match ? match[1] : "certificate_expired";
}

function noteCutoffFromError(error: unknown, endpointId: string | null) {
  if (!endpointId) return;
  const reason = cutoffReasonFromError(error);
  if (reason) useRemoteCutoffStore.getState().recordCutoff(endpointId, reason);
}

export async function remoteDispatch<T>(
  repo: ActiveRepository,
  request: TreqCommandRequest,
): Promise<T> {
  const { transport } = requireSshTransport(repo);
  try {
    return await dispatch<T>(transport.endpoint, request);
  } catch (error) {
    noteCutoffFromError(error, repo.endpointId);
    throw error;
  }
}

export async function transportGetWorkspaces(
  repoPath: string,
  local: () => Promise<Workspace[]>,
): Promise<Workspace[]> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  return remoteDispatch<Workspace[]>(repo, {
    kind: "ListWorkspaces",
    repo: repo.canonicalPath,
  });
}

export async function transportListWorkspaceStatuses(
  repoPath: string,
  local: () => Promise<WorkspaceSidebarStatus[]>,
): Promise<WorkspaceSidebarStatus[]> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const workspaces = await transportGetWorkspaces(repoPath, async () => []);
  return Promise.all(
    workspaces.map(async (workspace) => {
      try {
        const detailed = await transportGetWorkspaceStatus(
          repoPath,
          workspace.id,
          async () =>
            ({
              current: workspace,
              has_conflicts: false,
              has_changes: false,
              conflicted_files: [],
              remote_sync: { type: "NotOnRemote" },
              target: null,
              children: [],
              dag_nodes: [],
              conflicted_workspace_ids: [],
              commits_ahead_of_target: [],
            }) as WorkspaceStatus,
        );
        return {
          current: workspace,
          has_conflicts: detailed.has_conflicts,
        };
      } catch {
        return { current: workspace, has_conflicts: false };
      }
    }),
  );
}

export async function transportGetWorkspaceStatus(
  repoPath: string,
  workspaceId: number | null,
  local: () => Promise<WorkspaceStatus>,
): Promise<WorkspaceStatus> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  if (workspaceId == null) {
    return remoteDispatch<WorkspaceStatus>(repo, {
      kind: "RepositoryStatus",
      repo: repo.canonicalPath,
    });
  }
  return remoteDispatch<WorkspaceStatus>(repo, {
    kind: "InspectWorkspace",
    repo: repo.canonicalPath,
    workspace: String(workspaceId),
  });
}

export async function transportGetRepoCurrentBranch(
  repoPath: string,
  local: () => Promise<RepoBranch>,
): Promise<RepoBranch> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const inspection = await remoteDispatch<{
    current_branch: string | null;
    default_branch?: string;
  }>(repo, { kind: "InspectRepository", repo: repo.canonicalPath });
  return {
    current_branch: inspection.current_branch,
    display_ref: inspection.current_branch ?? inspection.default_branch ?? "",
    is_detached: false,
  };
}

export async function transportGetRepoDefaultBranch(
  repoPath: string,
  local: () => Promise<string>,
): Promise<string> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const inspection = await remoteDispatch<{ default_branch: string }>(repo, {
    kind: "InspectRepository",
    repo: repo.canonicalPath,
  });
  return inspection.default_branch;
}

export async function transportListRepoBranches<T>(
  repoPath: string,
  local: () => Promise<T>,
): Promise<T> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  return remoteDispatch<T>(repo, {
    kind: "ListBranches",
    repo: repo.canonicalPath,
  });
}

export async function transportGetWorkspaceChangedFiles(
  repoPath: string,
  workspaceId: number | null,
  local: () => Promise<JjFileChange[]>,
): Promise<JjFileChange[]> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  return remoteDispatch<JjFileChange[]>(repo, {
    kind: "ListChanges",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
  });
}

export async function transportListCommits(
  repoPath: string,
  workspaceId: number | null,
  local: () => Promise<JjLogResult>,
): Promise<JjLogResult> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const result = await remoteDispatch<
    JjLogResult | { commits?: JjLogResult["commits"] }
  >(repo, {
    kind: "ListCommits",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
  });
  if (Array.isArray(result)) {
    return { commits: result } as JjLogResult;
  }
  return result as JjLogResult;
}

export async function transportGetWorkspaceDiff(
  repoPath: string,
  workspaceId: number,
  local: () => Promise<JjRevisionDiff>,
): Promise<JjRevisionDiff> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  return remoteDispatch<JjRevisionDiff>(repo, {
    kind: "WorkspaceDiff",
    repo: repo.canonicalPath,
    workspace: String(workspaceId),
  });
}

// eslint-disable-next-line max-params -- matches local invoke arity
export async function transportGetWorkspaceFileHunks(
  repoPath: string,
  workspaceId: number | null,
  filePath: string,
  local: () => Promise<JjDiffHunk[]>,
): Promise<JjDiffHunk[]> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const hunks = await remoteDispatch<JjDiffHunk[] | null>(repo, {
    kind: "DiffFile",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
    path: filePath,
  });
  return hunks ?? [];
}

// eslint-disable-next-line max-params -- matches local invoke arity
export async function transportGetCommitDiff(
  repoPath: string,
  workspaceId: number | null,
  revision: string,
  local: () => Promise<JjRevisionDiff>,
): Promise<JjRevisionDiff> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  return remoteDispatch<JjRevisionDiff>(repo, {
    kind: "CommitDiff",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
    revision,
  });
}

// eslint-disable-next-line max-params -- matches local invoke arity
export async function transportGetCommitFileDiff(
  repoPath: string,
  workspaceId: number | null,
  revision: string,
  filePath: string,
  local: () => Promise<JjFileDiff>,
): Promise<JjFileDiff> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  return remoteDispatch<JjFileDiff>(repo, {
    kind: "CommitFileDiff",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
    revision,
    path: filePath,
  });
}

// eslint-disable-next-line max-params -- matches local invoke arity
export async function transportSearchWorkspaceFiles(
  repoPath: string,
  workspaceId: number | null,
  query: string,
  limit: number,
  local: () => Promise<FileSearchResult[]>,
): Promise<FileSearchResult[]> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  return remoteDispatch<FileSearchResult[]>(repo, {
    kind: "SearchFiles",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
    query,
    limit,
  });
}

/** Large enough to read any file the viewer would display in one request. */
const WHOLE_FILE_END_LINE = 1_000_000;

/**
 * Reads a file by absolute path. A path inside the active remote repository
 * is read from that workspace's working copy through the typed `ReadFile`
 * command instead of the local filesystem.
 */
export async function transportReadFile(
  path: string,
  local: () => Promise<string>,
): Promise<string> {
  const location = await resolveRemoteLocation(path);
  if (!location) return local();
  if (!location.relative) {
    throw new Error(`invalid_arguments: ${path} is a directory`);
  }
  const result = await remoteDispatch<JjFileLines>(location.repo, {
    kind: "ReadFile",
    repo: location.repo.canonicalPath,
    workspace: location.workspace,
    path: location.relative,
    revision: "WorkingCopy",
    start_line: 1,
    end_line: WHOLE_FILE_END_LINE,
  });
  return result.lines.length > 0 ? `${result.lines.join("\n")}\n` : "";
}

/* eslint-disable max-params -- matches local invoke arity */
export async function transportGetWorkspaceFileHunksBatch<T>(
  repoPath: string,
  workspaceId: number | null,
  filePaths: string[],
  local: () => Promise<T>,
): Promise<T> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const files = await Promise.all(
    filePaths.map(async (path) => {
      const hunks = await remoteDispatch<JjFileDiff["hunks"]>(repo, {
        kind: "DiffFile",
        repo: repo.canonicalPath,
        workspace: workspaceArg(workspaceId),
        path,
      });
      return { path, content_hash: "", hunks: hunks ?? [] };
    }),
  );
  return { snapshotToken: "remote", files } as T;
}

export async function transportGetWorkspaceFileLines(
  repoPath: string,
  workspaceId: number | null,
  filePath: string,
  fromParent: boolean,
  startLine: number,
  endLine: number,
  local: () => Promise<JjFileLines>,
): Promise<JjFileLines> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  return remoteDispatch<JjFileLines>(repo, {
    kind: "ReadFile",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
    path: filePath,
    revision: fromParent ? "Parent" : "WorkingCopy",
    start_line: startLine,
    end_line: endLine,
  });
}

export async function transportChangeMarker(
  repo: ActiveRepository,
  workspaceId: number | null | undefined,
): Promise<WorkspaceChangeMarker> {
  return remoteDispatch<WorkspaceChangeMarker>(repo, {
    kind: "WorkspaceChangeMarker",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId ?? null),
  });
}

/**
 * Runs a typed mutation against a remote repository with the PRD's
 * verify-before-retry semantics, reports the outcome to the mutation
 * feedback store, and returns the value when the mutation ran in this call.
 * `already_applied` resolves to `undefined`; `ambiguous` throws so the caller
 * never treats an unknown outcome as success.
 */
export async function remoteMutation<T>(
  repo: ActiveRepository,
  request: TreqCommandRequest,
): Promise<T | undefined> {
  return finishMutation(await dispatchMutation<T>(repo, request));
}

/**
 * `remoteMutation` for a command that carries an idempotency key. A retry of
 * the same inputs on the same endpoint generation, sent before a confirmed
 * outcome, reuses the earlier key so the VM runs the command at most once.
 */
export async function remoteKeyedMutation<T>(
  repo: ActiveRepository,
  request: UnkeyedRequest,
): Promise<T | undefined> {
  const key = remoteActionKeys.keyFor(
    request.kind,
    [request],
    repositoryCacheKey(repo),
  );
  const result = await dispatchMutation<T>(repo, {
    ...request,
    idempotency_key: key,
  } as TreqCommandRequest);
  remoteActionKeys.settle(key, result);
  return finishMutation(result);
}

async function dispatchMutation<T>(
  repo: ActiveRepository,
  request: TreqCommandRequest,
): Promise<MutationDispatchResult<T>> {
  const { transport } = requireSshTransport(repo);
  try {
    return await dispatchMutationOverSsh<T>(transport.endpoint, request);
  } catch (error) {
    noteCutoffFromError(error, repo.endpointId);
    throw error;
  }
}

function finishMutation<T>(result: MutationDispatchResult<T>): T | undefined {
  const value = applyMutationDispatchResult(result);
  if (result.status === "ambiguous") {
    throw new Error(result.reason);
  }
  return value;
}

export async function transportCreateCommit(
  repoPath: string,
  workspaceId: number | null,
  message: string,
  local: () => Promise<string>,
): Promise<string> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  const value = await remoteKeyedMutation<string>(repo, {
    kind: "CreateCommit",
    repo: repo.canonicalPath,
    workspace: workspaceArg(workspaceId),
    message,
  });
  return value ?? "Commit applied";
}

export async function transportGitFetch(
  repoPath: string,
  local: () => Promise<void>,
): Promise<void> {
  const repo = activeForPath(repoPath);
  if (!repo) return local();
  await remoteMutation(repo, {
    kind: "GitFetch",
    repo: repo.canonicalPath,
  });
}
