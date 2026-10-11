import { useState } from "react";
import useSWR from "swr";
import {
  dispatchOverSsh,
  dispatchMutationOverSsh,
} from "../lib/remote-dispatch";
import type { SshEndpoint } from "../lib/api-types-remote";
import { dispatchKeyedMutationOverSsh } from "../lib/remote-idempotency";
import {
  MutationButton,
  describeMutationOutcome,
} from "./remote/RemoteScreenControls";

interface AgentStatusResult {
  workspace: string;
  running: boolean;
  agent: string | null;
  pid: number | null;
  started_at: string | null;
  should_refresh: boolean;
}

const AGENTS = ["claude", "codex", "cursor-agent", "copilot"];

/**
 * Agent control (mobile PRD, "Agents"): start/status/logs/stop/input
 * against the VM-local agent supervisor via typed `TreqCommandRequest`
 * dispatch. Status comes from the VM on every mount, so reopening the app
 * shows the agent as the VM reports it. Input here is non-interactive (a
 * single message sent to the agent's stdin via `AgentInput`), not a live
 * PTY attach; that is the terminal screen's job.
 */
export function RemoteAgentScreen({
  endpoint,
  repo,
  workspace,
}: {
  endpoint: SshEndpoint;
  repo: string;
  workspace: string;
}) {
  const [agent, setAgent] = useState(AGENTS[0]);
  const [prompt, setPrompt] = useState("");
  const [input, setInput] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const {
    data: status,
    error: statusError,
    mutate: mutateStatus,
  } = useSWR(
    ["remote-agent-status", endpoint.hostname, repo, workspace],
    () =>
      dispatchOverSsh<AgentStatusResult>(endpoint, {
        kind: "AgentStatus",
        repo,
        workspace,
      }),
    { refreshInterval: 4_000 },
  );

  const { data: logs, mutate: mutateLogs } = useSWR(
    status?.running
      ? ["remote-agent-logs", endpoint.hostname, repo, workspace]
      : null,
    () =>
      dispatchOverSsh<string>(endpoint, { kind: "AgentLogs", repo, workspace }),
    { refreshInterval: 4_000 },
  );

  async function startAgent() {
    setActionError(null);
    setBusy(true);
    try {
      const result = await dispatchKeyedMutationOverSsh(endpoint, {
        kind: "AgentStart",
        repo,
        workspace,
        agent,
        prompt,
      });
      if (result.status === "ambiguous") {
        setActionError(`Could not confirm the agent started: ${result.reason}`);
      }
      await mutateStatus();
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function sendInput() {
    if (!input.trim()) return;
    setActionError(null);
    setBusy(true);
    try {
      const result = await dispatchKeyedMutationOverSsh(endpoint, {
        kind: "AgentInput",
        repo,
        workspace,
        input,
      });
      if (result.status === "ambiguous") {
        setActionError(
          `Could not confirm the input was sent: ${result.reason}`,
        );
      } else {
        setInput("");
      }
      await mutateLogs();
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  // Stopping is a no-op when repeated, so `AgentStop` carries no
  // idempotency key. It still goes through verify-before-retry, and an
  // ambiguous result is shown rather than treated as stopped.
  async function stopAgent() {
    setActionError(null);
    try {
      const result = await dispatchMutationOverSsh(endpoint, {
        kind: "AgentStop",
        repo,
        workspace,
      });
      const outcome = describeMutationOutcome(result);
      if (outcome) setActionError(outcome);
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err));
    }
    await mutateStatus();
    await mutateLogs();
  }

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold">Agent · {workspace}</h2>
        <button
          type="button"
          onClick={() => mutateStatus()}
          className="self-end rounded-md border px-2 py-1 text-xs"
        >
          Refresh
        </button>
      </div>
      {statusError && (
        <p className="text-sm text-destructive">{String(statusError)}</p>
      )}
      {actionError && <p className="text-sm text-destructive">{actionError}</p>}

      {status?.running ? (
        <div className="flex flex-col gap-2 rounded-md border px-3 py-2 text-sm">
          <p>
            Running <span className="font-mono">{status.agent}</span> (pid{" "}
            {status.pid})
          </p>
          <p className="text-xs text-muted-foreground">
            Started {status.started_at}
          </p>
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="Send input to the agent"
            className="min-h-16 rounded-md border px-3 py-2 text-sm"
          />
          <div className="flex gap-2">
            <button
              type="button"
              onClick={sendInput}
              disabled={busy || !input.trim()}
              className="flex-1 rounded-md border px-3 py-2 text-sm"
            >
              {busy ? "Sending..." : "Send input"}
            </button>
            <MutationButton
              label="Stop agent"
              confirmLabel="Confirm stop"
              variant="destructive"
              disabled={busy}
              onConfirm={stopAgent}
            />
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          <select
            value={agent}
            onChange={(e) => setAgent(e.target.value)}
            className="rounded-md border px-3 py-2 text-sm"
          >
            {AGENTS.map((a) => (
              <option key={a} value={a}>
                {a}
              </option>
            ))}
          </select>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="Prompt for the agent"
            className="min-h-24 rounded-md border px-3 py-2 text-sm"
          />
          <button
            type="button"
            onClick={startAgent}
            disabled={busy || !prompt}
            className="rounded-md border px-3 py-2 text-sm"
          >
            {busy ? "Starting..." : "Start agent"}
          </button>
        </div>
      )}

      {logs && (
        <details open className="rounded-md border px-3 py-2 text-xs">
          <summary className="cursor-pointer text-sm font-semibold">
            Logs
          </summary>
          <pre className="max-h-96 overflow-y-auto whitespace-pre-wrap font-mono">
            {logs}
          </pre>
        </details>
      )}
    </section>
  );
}
