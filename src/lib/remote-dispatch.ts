// Frontend entry point for Phase 5's typed remote command dispatch
// (`remote_dispatch_local` / `remote_dispatch_over_ssh` in
// `src-tauri/src/commands/remote_control.rs`). Every call sends a fully
// typed `TreqCommandRequest` variant - never a raw string - so the
// allow-list holds at the IPC boundary the same way it holds inside the
// exec transport.
//
// This is intentionally a separate, small surface from `src/lib/api.ts`
// rather than a rewrite of it: the existing ~50 local Tauri commands in
// `api.ts` are not yet mirrored one-to-one by `TreqCommandRequest` variants,
// so making every local data hook transport-aware is future work. What is
// wired here is the typed command protocol (reads and mutations) that the
// remote CLI, Tauri IPC, and TypeScript share.

import {
  remoteDispatchLocal,
  remoteDispatchMutationOverSsh,
  remoteDispatchOverSsh,
} from "./api-extra";
import type { SshEndpoint, PtyLaunchSpec } from "./api-types-remote";

/** Mirrors `core::workspaces::HunkSpec` for non-interactive split/move. */
export interface RemoteHunkSpec {
  file_path: string;
  start_line: number;
  end_line: number;
}

/**
 * Must match `TreqCommandRequest::KIND_NAMES` in
 * `src-tauri/src/core/remote.rs`. The unit test in
 * `remote-dispatch.test.ts` asserts this list stays aligned with the
 * TypeScript union.
 */
export const TREQ_COMMAND_KINDS = [
  "InspectRepository",
  "RepositoryStatus",
  "ListBranches",
  "ListWorkspaces",
  "InspectWorkspace",
  "ListChanges",
  "DiffFile",
  "ReadFile",
  "ListCommits",
  "ListConflicts",
  "WorkspaceChangeMarker",
  "MachineUsage",
  "ProbeRepo",
  "CloneRepo",
  "InitRepo",
  "CreateWorkspace",
  "RenameWorkspace",
  "UpdateWorkspace",
  "DeleteWorkspace",
  "ArchiveWorkspace",
  "MoveWorkspaceChanges",
  "RebaseWorkspace",
  "SwitchRepoBranch",
  "RestoreFile",
  "PatchFile",
  "SnapshotWorkingCopy",
  "RestoreAll",
  "RestoreSnapshot",
  "StashWorkspaceChanges",
  "CreateCommit",
  "DescribeCommit",
  "SplitCommit",
  "MoveCommit",
  "UndoCommit",
  "AbandonCommit",
  "UndoOperation",
  "RevertCommit",
  "ResolveConflict",
  "GitFetch",
  "PullWorkspace",
  "GitBookmarkTrack",
  "GitRemoteInfo",
  "GitPush",
  "AgentStart",
  "AgentInput",
  "AgentStatus",
  "AgentStop",
  "AgentLogs",
  "PtyStart",
  "PtyList",
  "PtyStop",
  "PtyAttachCommand",
  "WorkspaceDiff",
  "CommitDiff",
  "CommitFileDiff",
  "SearchFiles",
] as const;

export type TreqCommandKind = (typeof TREQ_COMMAND_KINDS)[number];

export type TreqCommandRequest =
  | { kind: "ProbeRepo"; repo: string }
  | { kind: "InspectRepository"; repo: string }
  | { kind: "RepositoryStatus"; repo: string }
  | { kind: "ListBranches"; repo: string }
  | { kind: "ListWorkspaces"; repo: string }
  | { kind: "InspectWorkspace"; repo: string; workspace: string }
  | { kind: "ListChanges"; repo: string; workspace?: string | null }
  | {
      kind: "DiffFile";
      repo: string;
      workspace?: string | null;
      path: string;
    }
  | {
      kind: "ReadFile";
      repo: string;
      workspace?: string | null;
      path: string;
      revision: "WorkingCopy" | "Parent";
      start_line?: number | null;
      end_line?: number | null;
    }
  | { kind: "ListCommits"; repo: string; workspace?: string | null }
  | { kind: "ListConflicts"; repo: string; workspace?: string | null }
  | { kind: "WorkspaceDiff"; repo: string; workspace: string }
  | {
      kind: "CommitDiff";
      repo: string;
      workspace?: string | null;
      revision: string;
    }
  | {
      kind: "CommitFileDiff";
      repo: string;
      workspace?: string | null;
      revision: string;
      path: string;
    }
  | {
      kind: "SearchFiles";
      repo: string;
      workspace?: string | null;
      query: string;
      limit?: number | null;
    }
  | {
      kind: "WorkspaceChangeMarker";
      repo: string;
      workspace?: string | null;
    }
  | { kind: "MachineUsage"; root: string }
  | {
      kind: "CloneRepo";
      repo_url: string;
      destination: string;
      idempotency_key: string;
    }
  | { kind: "InitRepo"; repo: string; idempotency_key: string }
  | {
      kind: "CreateWorkspace";
      repo: string;
      branch_name: string;
      source_branch?: string | null;
      metadata?: string | null;
      idempotency_key: string;
    }
  | {
      kind: "RenameWorkspace";
      repo: string;
      workspace: string;
      new_name: string;
      idempotency_key: string;
    }
  | {
      kind: "UpdateWorkspace";
      repo: string;
      workspace: string;
      target_branch?: string | null;
      title?: string | null;
      description?: string | null;
    }
  | { kind: "DeleteWorkspace"; repo: string; workspace: string }
  | { kind: "ArchiveWorkspace"; repo: string; workspace: string }
  | {
      kind: "MoveWorkspaceChanges";
      repo: string;
      workspace: string;
      destination: string;
      files?: string[];
      hunks?: RemoteHunkSpec[];
      commits: string[];
      idempotency_key: string;
    }
  | {
      kind: "RebaseWorkspace";
      repo: string;
      workspace: string;
      target_branch: string;
      idempotency_key: string;
    }
  | { kind: "SwitchRepoBranch"; repo: string; bookmark: string }
  | {
      kind: "RestoreFile";
      repo: string;
      workspace?: string | null;
      path: string;
    }
  | {
      kind: "PatchFile";
      repo: string;
      workspace?: string | null;
      path: string;
      patch_base64: string;
      idempotency_key: string;
    }
  | { kind: "SnapshotWorkingCopy"; repo: string; workspace?: string | null }
  | { kind: "RestoreAll"; repo: string; workspace?: string | null }
  | {
      kind: "RestoreSnapshot";
      repo: string;
      workspace?: string | null;
      snapshot_id: string;
    }
  | {
      kind: "StashWorkspaceChanges";
      repo: string;
      workspace?: string | null;
      idempotency_key: string;
    }
  | {
      kind: "CreateCommit";
      repo: string;
      workspace?: string | null;
      message: string;
      base_change_id?: string | null;
      idempotency_key: string;
    }
  | {
      kind: "DescribeCommit";
      repo: string;
      workspace: string;
      commit: string;
      message: string;
    }
  | {
      kind: "SplitCommit";
      repo: string;
      workspace: string;
      commit: string;
      files: string[];
      hunks: RemoteHunkSpec[];
      message?: string;
      idempotency_key: string;
    }
  | {
      kind: "MoveCommit";
      repo: string;
      workspace: string;
      commit: string;
      target_workspace: string;
      idempotency_key: string;
    }
  | { kind: "UndoCommit"; repo: string; workspace: string; commit: string }
  | {
      kind: "AbandonCommit";
      repo: string;
      workspace: string;
      commit: string;
      idempotency_key: string;
    }
  | {
      kind: "UndoOperation";
      repo: string;
      workspace?: string | null;
      operation_id: string;
    }
  | {
      kind: "RevertCommit";
      repo: string;
      workspace: string;
      commit: string;
      idempotency_key: string;
    }
  | {
      kind: "ResolveConflict";
      repo: string;
      revision: string;
      sides: string[];
      idempotency_key: string;
    }
  | { kind: "GitFetch"; repo: string }
  | { kind: "PullWorkspace"; repo: string; workspace?: string | null }
  | {
      kind: "GitBookmarkTrack";
      repo: string;
      bookmark: string;
      remote_name: string;
    }
  | { kind: "GitRemoteInfo"; repo: string }
  | {
      kind: "GitPush";
      repo: string;
      workspace?: string | null;
      idempotency_key: string;
    }
  | {
      kind: "AgentStart";
      repo: string;
      workspace: string;
      agent: string;
      prompt: string;
      idempotency_key: string;
    }
  | {
      kind: "AgentInput";
      repo: string;
      workspace: string;
      input: string;
      idempotency_key: string;
    }
  | { kind: "AgentStatus"; repo: string; workspace: string }
  | { kind: "AgentStop"; repo: string; workspace: string }
  | { kind: "AgentLogs"; repo: string; workspace: string }
  | {
      kind: "PtyStart";
      repo: string;
      workspace: string;
      label: string;
      remote_dir: string;
      launch: PtyLaunchSpec;
      cols: number;
      rows: number;
      idempotency_key: string;
    }
  | { kind: "PtyList"; repo: string; workspace?: string | null }
  | { kind: "PtyStop"; repo: string; workspace: string; label: string }
  | {
      kind: "PtyAttachCommand";
      repo: string;
      workspace: string;
      label: string;
      remote_dir: string;
      launch: PtyLaunchSpec;
      cols: number;
      rows: number;
    };

/** Compile-time exhaustiveness: adding a Rust variant without TS fails this. */
export function treqCommandKind(request: TreqCommandRequest): TreqCommandKind {
  return request.kind;
}

export function dispatchLocal<T = unknown>(
  request: TreqCommandRequest,
): Promise<T> {
  return remoteDispatchLocal<T>(request);
}

export function dispatchOverSsh<T = unknown>(
  endpoint: SshEndpoint,
  request: TreqCommandRequest,
): Promise<T> {
  return remoteDispatchOverSsh<T>(endpoint, request);
}

/** Dispatches locally or over SSH depending on whether an endpoint is given. */
export function dispatch<T = unknown>(
  endpoint: SshEndpoint | null,
  request: TreqCommandRequest,
): Promise<T> {
  return endpoint
    ? dispatchOverSsh<T>(endpoint, request)
    : dispatchLocal<T>(request);
}

// -- Retrying after network loss (PRD "Structured command protocol" >
// "Retrying after network loss") --------------------------------------------
//
// A network failure while a mutating command is in flight does not tell the
// client whether the command reached the VM, ran, or completed. Mutating
// kinds must go through `dispatchMutationOverSsh`, never plain
// `dispatchOverSsh`, so the backend's verify-before-retry logic
// (`core::remote::retry_after_reconnect`) runs instead of the caller
// assuming the operation is safely idempotent and blindly resending it.

/** Mirrors `MutationDispatchResult` in `src-tauri/src/commands/remote_control.rs`. */
export type MutationDispatchResult<T = unknown> =
  | { status: "applied"; value: T }
  | { status: "already_applied" }
  | { status: "ambiguous"; reason: string };

/**
 * Runs a mutating `TreqCommandRequest` over SSH with verify-before-retry
 * semantics. Resolves to a `MutationDispatchResult` rather than a bare `T` so
 * a caller can render all three PRD outcomes distinctly:
 *
 * - `"applied"`: the mutation ran (first attempt, or a same-idempotency-key
 *   retry after a "not applied" verdict) - treat like a normal success.
 * - `"already_applied"`: a network failure interrupted the original attempt,
 *   but post-reconnect state showed it had already landed - nothing was
 *   resent, so this should refresh the UI the same way a fresh success would.
 * - `"ambiguous"`: a network failure interrupted the attempt and
 *   post-reconnect state could not confirm either way - the caller must ask
 *   the user rather than assume.
 */
export function dispatchMutationOverSsh<T = unknown>(
  endpoint: SshEndpoint,
  request: TreqCommandRequest,
): Promise<MutationDispatchResult<T>> {
  return remoteDispatchMutationOverSsh<MutationDispatchResult<T>>(
    endpoint,
    request,
  );
}
