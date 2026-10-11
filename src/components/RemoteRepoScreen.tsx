import { useState } from "react";
import useSWR from "swr";
import { dispatchOverSsh } from "../lib/remote-dispatch";
import type { SshEndpoint } from "../lib/api-types-remote";
import type {
  Workspace,
  JjDiffHunk,
  JjFileLines,
  JjLogCommit,
} from "../lib/api-types";
import { RemoteAgentScreen } from "./RemoteAgentScreen";
import { RemoteTerminalScreen } from "./mobile/RemoteTerminalScreen";
import {
  WorkspaceDetailScreen,
  ConflictsScreen,
} from "./RemoteWorkspaceMutationScreens";
import {
  RefreshButton,
  MutationButton,
  describeMutationOutcome,
} from "./remote/RemoteScreenControls";
import { dispatchKeyedMutationOverSsh } from "../lib/remote-idempotency";

/**
 * Where the user is inside a remote repository. It only names remote
 * objects (workspace name, file path), so the mobile shell persists it and
 * reopens the same screen after a restart; each screen then re-reads its
 * data from the VM.
 */
export type RemoteRepoScreenState =
  | { name: "workspaces" }
  | { name: "workspace"; workspace: string }
  | { name: "diff"; workspace: string; path: string }
  | { name: "commits"; workspace: string }
  | { name: "conflicts"; workspace: string }
  | { name: "agent"; workspace: string }
  | { name: "terminal"; workspace: string };

/**
 * Mobile review, agent control, and controlled mutations (mobile PRD,
 * "Mobile product behavior"), driven entirely over
 * `dispatchOverSsh`/`dispatchMutationOverSsh` against the connected VM.
 * Single-column navigation: workspace list -> workspace detail ->
 * diff/commits/conflicts/agent, each with manual refresh. Mutations
 * (workspace creation, rebase, commit creation, conflict resolution,
 * bookmark push) use `MutationButton`'s arm/confirm pattern so nothing
 * dispatches on a single tap.
 */
export function RemoteRepoScreen({
  endpoint,
  repo,
  initialScreen,
  onScreenChange,
}: {
  endpoint: SshEndpoint;
  repo: string;
  initialScreen?: RemoteRepoScreenState;
  onScreenChange?: (screen: RemoteRepoScreenState) => void;
}) {
  const [screen, setScreen] = useState<RemoteRepoScreenState>(
    initialScreen ?? { name: "workspaces" },
  );
  const navigate = (next: RemoteRepoScreenState) => {
    setScreen(next);
    onScreenChange?.(next);
  };

  return (
    <div className="flex flex-col gap-3">
      {screen.name !== "workspaces" && (
        <button
          type="button"
          className="self-start text-sm text-muted-foreground"
          onClick={() =>
            "workspace" in screen && screen.name !== "workspace"
              ? navigate({ name: "workspace", workspace: screen.workspace })
              : navigate({ name: "workspaces" })
          }
        >
          ← Back
        </button>
      )}
      {screen.name === "workspaces" && (
        <WorkspaceListScreen
          endpoint={endpoint}
          repo={repo}
          onSelect={(workspace) => navigate({ name: "workspace", workspace })}
        />
      )}
      {screen.name === "workspace" && (
        <WorkspaceDetailScreen
          endpoint={endpoint}
          repo={repo}
          workspace={screen.workspace}
          onOpenDiff={(path) =>
            navigate({ name: "diff", workspace: screen.workspace, path })
          }
          onOpenCommits={() =>
            navigate({ name: "commits", workspace: screen.workspace })
          }
          onOpenConflicts={() =>
            navigate({ name: "conflicts", workspace: screen.workspace })
          }
          onOpenAgent={() =>
            navigate({ name: "agent", workspace: screen.workspace })
          }
          onOpenTerminal={() =>
            navigate({ name: "terminal", workspace: screen.workspace })
          }
        />
      )}
      {screen.name === "diff" && (
        <DiffScreen
          endpoint={endpoint}
          repo={repo}
          workspace={screen.workspace}
          path={screen.path}
        />
      )}
      {screen.name === "commits" && (
        <CommitsScreen
          endpoint={endpoint}
          repo={repo}
          workspace={screen.workspace}
        />
      )}
      {screen.name === "conflicts" && (
        <ConflictsScreen
          endpoint={endpoint}
          repo={repo}
          workspace={screen.workspace}
        />
      )}
      {screen.name === "agent" && (
        <RemoteAgentScreen
          endpoint={endpoint}
          repo={repo}
          workspace={screen.workspace}
        />
      )}
      {screen.name === "terminal" && (
        <RemoteTerminalScreen
          endpoint={endpoint}
          repo={repo}
          workspace={screen.workspace}
        />
      )}
    </div>
  );
}

function WorkspaceListScreen({
  endpoint,
  repo,
  onSelect,
}: {
  endpoint: SshEndpoint;
  repo: string;
  onSelect: (workspace: string) => void;
}) {
  const { data, error, isLoading, mutate } = useSWR(
    ["remote-workspaces", endpoint.hostname, repo],
    () =>
      dispatchOverSsh<Workspace[]>(endpoint, { kind: "ListWorkspaces", repo }),
  );

  const [branchName, setBranchName] = useState("");
  const [sourceBranch, setSourceBranch] = useState("");
  const [createError, setCreateError] = useState<string | null>(null);

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold">Workspaces</h2>
        <RefreshButton onClick={() => mutate()} loading={isLoading} />
      </div>
      {error && <p className="text-sm text-destructive">{String(error)}</p>}
      <div className="flex flex-col gap-2 rounded-md border px-3 py-2">
        <p className="text-xs font-semibold text-muted-foreground">
          New workspace
        </p>
        <input
          value={branchName}
          onChange={(e) => setBranchName(e.target.value)}
          placeholder="Branch name"
          className="rounded-md border px-3 py-2 text-sm"
        />
        <input
          value={sourceBranch}
          onChange={(e) => setSourceBranch(e.target.value)}
          placeholder="Source branch (optional)"
          className="rounded-md border px-3 py-2 text-sm"
        />
        {createError && (
          <p className="text-sm text-destructive">{createError}</p>
        )}
        <MutationButton
          label="Create workspace"
          confirmLabel="Confirm create"
          disabled={!branchName.trim()}
          onConfirm={async () => {
            setCreateError(null);
            const branch = branchName.trim();
            const source = sourceBranch.trim() || null;
            try {
              const result = await dispatchKeyedMutationOverSsh(endpoint, {
                kind: "CreateWorkspace",
                repo,
                branch_name: branch,
                source_branch: source,
              });
              const outcome = describeMutationOutcome(result);
              if (outcome) {
                setCreateError(outcome);
                return;
              }
              setBranchName("");
              setSourceBranch("");
              await mutate();
            } catch (err) {
              setCreateError(err instanceof Error ? err.message : String(err));
            }
          }}
        />
      </div>
      {data?.length === 0 && (
        <p className="text-sm text-muted-foreground">No workspaces found.</p>
      )}
      <ul className="flex flex-col gap-2">
        {data?.map((ws) => (
          <li key={ws.id}>
            <button
              type="button"
              onClick={() => onSelect(ws.workspace_name)}
              className="w-full rounded-md border px-3 py-2 text-left text-sm"
            >
              <div className="font-medium">{ws.title || ws.workspace_name}</div>
              <div className="text-xs text-muted-foreground">
                {ws.branch_name}
              </div>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function DiffScreen({
  endpoint,
  repo,
  workspace,
  path,
}: {
  endpoint: SshEndpoint;
  repo: string;
  workspace: string;
  path: string;
}) {
  const { data, error, isLoading, mutate } = useSWR(
    ["remote-diff", endpoint.hostname, repo, workspace, path],
    () =>
      dispatchOverSsh<JjDiffHunk[]>(endpoint, {
        kind: "DiffFile",
        repo,
        workspace,
        path,
      }),
  );

  const { data: workingCopy } = useSWR(
    ["remote-file", endpoint.hostname, repo, workspace, path, "wc"],
    () =>
      dispatchOverSsh<JjFileLines>(endpoint, {
        kind: "ReadFile",
        repo,
        workspace,
        path,
        revision: "WorkingCopy",
      }),
  );

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h2 className="truncate text-sm font-semibold">{path}</h2>
        <RefreshButton onClick={() => mutate()} loading={isLoading} />
      </div>
      {error && <p className="text-sm text-destructive">{String(error)}</p>}
      {data?.map((hunk) => (
        <pre
          key={hunk.id}
          className="overflow-x-auto whitespace-pre rounded-md border bg-muted px-3 py-2 font-mono text-xs"
        >
          {hunk.header}
          {"\n"}
          {hunk.lines.join("\n")}
        </pre>
      ))}
      {workingCopy && (
        <details className="rounded-md border px-3 py-2 text-xs">
          <summary className="cursor-pointer text-sm font-semibold">
            Working-copy content
          </summary>
          <pre className="overflow-x-auto whitespace-pre font-mono">
            {workingCopy.lines.join("\n")}
          </pre>
        </details>
      )}
    </section>
  );
}

function CommitsScreen({
  endpoint,
  repo,
  workspace,
}: {
  endpoint: SshEndpoint;
  repo: string;
  workspace: string;
}) {
  const { data, error, isLoading, mutate } = useSWR(
    ["remote-commits", endpoint.hostname, repo, workspace],
    () =>
      dispatchOverSsh<JjLogCommit[]>(endpoint, {
        kind: "ListCommits",
        repo,
        workspace,
      }),
  );

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold">Commits</h2>
        <RefreshButton onClick={() => mutate()} loading={isLoading} />
      </div>
      {error && <p className="text-sm text-destructive">{String(error)}</p>}
      <ul className="flex flex-col gap-2">
        {data?.map((commit) => (
          <li
            key={commit.commit_id}
            className="rounded-md border px-3 py-2 text-sm"
          >
            <p className="font-mono text-xs text-muted-foreground">
              {commit.short_id}
            </p>
            <p>{commit.description || "(no description)"}</p>
            <p className="text-xs text-muted-foreground">
              {commit.author_name} · {commit.timestamp}
            </p>
          </li>
        ))}
      </ul>
    </section>
  );
}
