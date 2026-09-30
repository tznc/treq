import { useState } from "react";
import useSWR from "swr";
import { dispatchOverSsh } from "../lib/remote-dispatch";
import type { SshEndpoint } from "../lib/api-types-remote";
import type { WorkspaceStatus, JjFileChange } from "../lib/api-types";
import {
  RefreshButton,
  MutationButton,
  describeMutationOutcome,
} from "./remote/RemoteScreenControls";
import { dispatchKeyedMutationOverSsh } from "../lib/remote-idempotency";

interface WorkspaceChangeMarker {
  operation_id: string;
}

/**
 * Read-only review plus controlled mutations for a single workspace
 * (mobile PRD, "Navigation and review" and "Mutations"): status/changes/rebase/commit/push, all over
 * `dispatchOverSsh`/`dispatchMutationOverSsh`. Split out of
 * `RemoteRepoScreen.tsx` to keep that file under the line-count lint limit.
 */
export function WorkspaceDetailScreen({
  endpoint,
  repo,
  workspace,
  onOpenDiff,
  onOpenCommits,
  onOpenConflicts,
  onOpenAgent,
  onOpenTerminal,
}: {
  endpoint: SshEndpoint;
  repo: string;
  workspace: string;
  onOpenDiff: (path: string) => void;
  onOpenCommits: () => void;
  onOpenConflicts: () => void;
  onOpenAgent: () => void;
  onOpenTerminal: () => void;
}) {
  const statusKey = [
    "remote-workspace-status",
    endpoint.hostname,
    repo,
    workspace,
  ];
  const {
    data: status,
    error: statusError,
    isLoading: statusLoading,
    mutate: mutateStatus,
  } = useSWR(statusKey, () =>
    dispatchOverSsh<WorkspaceStatus>(endpoint, {
      kind: "InspectWorkspace",
      repo,
      workspace,
    }),
  );

  const {
    data: changes,
    error: changesError,
    isLoading: changesLoading,
    mutate: mutateChanges,
  } = useSWR(["remote-changes", endpoint.hostname, repo, workspace], () =>
    dispatchOverSsh<JjFileChange[]>(endpoint, {
      kind: "ListChanges",
      repo,
      workspace,
    }),
  );

  const { data: marker, mutate: mutateMarker } = useSWR(
    ["remote-change-marker", endpoint.hostname, repo, workspace],
    () =>
      dispatchOverSsh<WorkspaceChangeMarker>(endpoint, {
        kind: "WorkspaceChangeMarker",
        repo,
        workspace,
      }),
    { refreshInterval: 15_000 },
  );

  const refreshAll = () => {
    mutateStatus();
    mutateChanges();
    mutateMarker();
  };

  const [targetBranch, setTargetBranch] = useState("");
  const [rebaseError, setRebaseError] = useState<string | null>(null);
  const [commitMessage, setCommitMessage] = useState("");
  const [commitError, setCommitError] = useState<string | null>(null);
  const [pushError, setPushError] = useState<string | null>(null);

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold">{workspace}</h2>
        <RefreshButton
          onClick={refreshAll}
          loading={statusLoading || changesLoading}
        />
      </div>
      {marker && (
        <p className="text-xs text-muted-foreground">
          op {marker.operation_id.slice(0, 12)}
        </p>
      )}
      {statusError && (
        <p className="text-sm text-destructive">{String(statusError)}</p>
      )}
      {status && (
        <div className="rounded-md border px-3 py-2 text-sm">
          <p>{status.has_changes ? "Has uncommitted changes" : "Clean"}</p>
          <p>{status.has_conflicts ? "Has conflicts" : "No conflicts"}</p>
        </div>
      )}

      <div className="flex flex-col gap-2 rounded-md border px-3 py-2">
        <p className="text-xs font-semibold text-muted-foreground">Mutations</p>
        <div className="flex flex-wrap items-center gap-2">
          <input
            value={targetBranch}
            onChange={(e) => setTargetBranch(e.target.value)}
            placeholder="Rebase onto branch"
            className="min-w-0 flex-1 rounded-md border px-3 py-2 text-sm"
          />
          <MutationButton
            label="Rebase"
            confirmLabel="Confirm rebase"
            disabled={!targetBranch.trim()}
            onConfirm={async () => {
              setRebaseError(null);
              const target = targetBranch.trim();
              try {
                const result = await dispatchKeyedMutationOverSsh(endpoint, {
                  kind: "RebaseWorkspace",
                  repo,
                  workspace,
                  target_branch: target,
                });
                const outcome = describeMutationOutcome(result);
                if (outcome) {
                  setRebaseError(outcome);
                  return;
                }
                refreshAll();
              } catch (err) {
                setRebaseError(
                  err instanceof Error ? err.message : String(err),
                );
              }
            }}
          />
        </div>
        {rebaseError && (
          <p className="text-sm text-destructive">{rebaseError}</p>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <input
            value={commitMessage}
            onChange={(e) => setCommitMessage(e.target.value)}
            placeholder="Commit message"
            className="min-w-0 flex-1 rounded-md border px-3 py-2 text-sm"
          />
          <MutationButton
            label="Commit"
            confirmLabel="Confirm commit"
            disabled={!commitMessage.trim()}
            onConfirm={async () => {
              setCommitError(null);
              const message = commitMessage.trim();
              try {
                const result = await dispatchKeyedMutationOverSsh(endpoint, {
                  kind: "CreateCommit",
                  repo,
                  workspace,
                  message,
                });
                const outcome = describeMutationOutcome(result);
                if (outcome) {
                  setCommitError(outcome);
                  return;
                }
                setCommitMessage("");
                refreshAll();
              } catch (err) {
                setCommitError(
                  err instanceof Error ? err.message : String(err),
                );
              }
            }}
          />
        </div>
        {commitError && (
          <p className="text-sm text-destructive">{commitError}</p>
        )}

        <MutationButton
          label="Push bookmark"
          confirmLabel="Confirm push"
          onConfirm={async () => {
            setPushError(null);
            try {
              const result = await dispatchKeyedMutationOverSsh(endpoint, {
                kind: "GitPush",
                repo,
                workspace,
              });
              const outcome = describeMutationOutcome(result);
              if (outcome) {
                setPushError(outcome);
                return;
              }
              refreshAll();
            } catch (err) {
              setPushError(err instanceof Error ? err.message : String(err));
            }
          }}
        />
        {pushError && <p className="text-sm text-destructive">{pushError}</p>}
      </div>

      <div className="flex gap-2">
        <button
          type="button"
          onClick={onOpenCommits}
          className="flex-1 rounded-md border px-3 py-2 text-sm"
        >
          Commits
        </button>
        <button
          type="button"
          onClick={onOpenConflicts}
          className="flex-1 rounded-md border px-3 py-2 text-sm"
        >
          Conflicts
        </button>
        <button
          type="button"
          onClick={onOpenAgent}
          className="flex-1 rounded-md border px-3 py-2 text-sm"
        >
          Agent
        </button>
        <button
          type="button"
          onClick={onOpenTerminal}
          className="flex-1 rounded-md border px-3 py-2 text-sm"
        >
          Terminal
        </button>
      </div>

      <h3 className="text-sm font-semibold">Changed files</h3>
      {changesError && (
        <p className="text-sm text-destructive">{String(changesError)}</p>
      )}
      {changes?.length === 0 && (
        <p className="text-sm text-muted-foreground">No changed files.</p>
      )}
      <ul className="flex flex-col gap-2">
        {changes?.map((file) => (
          <li key={file.path}>
            <button
              type="button"
              onClick={() => onOpenDiff(file.path)}
              className="w-full rounded-md border px-3 py-2 text-left text-sm"
            >
              <span className="font-mono text-xs uppercase text-muted-foreground">
                {file.status}
              </span>{" "}
              {file.path}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function ConflictsScreen({
  endpoint,
  repo,
  workspace,
}: {
  endpoint: SshEndpoint;
  repo: string;
  workspace: string;
}) {
  const { data, error, isLoading, mutate } = useSWR(
    ["remote-conflicts", endpoint.hostname, repo, workspace],
    () =>
      dispatchOverSsh<string[]>(endpoint, {
        kind: "ListConflicts",
        repo,
        workspace,
      }),
  );

  const [revision, setRevision] = useState("@");
  const [side, setSide] = useState("side1");
  const [resolveError, setResolveError] = useState<string | null>(null);

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold">Conflicts</h2>
        <RefreshButton onClick={() => mutate()} loading={isLoading} />
      </div>
      {error && <p className="text-sm text-destructive">{String(error)}</p>}
      {data?.length === 0 && (
        <p className="text-sm text-muted-foreground">No conflicted files.</p>
      )}
      <ul className="flex flex-col gap-2">
        {data?.map((path) => (
          <li
            key={path}
            className="rounded-md border px-3 py-2 font-mono text-sm"
          >
            {path}
          </li>
        ))}
      </ul>

      {data && data.length > 0 && (
        <div className="flex flex-col gap-2 rounded-md border px-3 py-2">
          <p className="text-xs font-semibold text-muted-foreground">
            Resolve conflicts in a revision
          </p>
          <input
            value={revision}
            onChange={(e) => setRevision(e.target.value)}
            placeholder="Revision (e.g. @)"
            className="rounded-md border px-3 py-2 text-sm font-mono"
          />
          <select
            value={side}
            onChange={(e) => setSide(e.target.value)}
            className="rounded-md border px-3 py-2 text-sm"
          >
            <option value="side1">Keep ours (side1)</option>
            <option value="side2">Keep theirs (side2)</option>
            <option value="both">Keep both</option>
          </select>
          {resolveError && (
            <p className="text-sm text-destructive">{resolveError}</p>
          )}
          <MutationButton
            label="Resolve"
            confirmLabel="Confirm resolve"
            disabled={!revision.trim()}
            onConfirm={async () => {
              setResolveError(null);
              const target = revision.trim();
              try {
                const result = await dispatchKeyedMutationOverSsh(endpoint, {
                  kind: "ResolveConflict",
                  repo,
                  revision: target,
                  sides: [side],
                });
                const outcome = describeMutationOutcome(result);
                if (outcome) {
                  setResolveError(outcome);
                  return;
                }
                await mutate();
              } catch (err) {
                setResolveError(
                  err instanceof Error ? err.message : String(err),
                );
              }
            }}
          />
        </div>
      )}
    </section>
  );
}
