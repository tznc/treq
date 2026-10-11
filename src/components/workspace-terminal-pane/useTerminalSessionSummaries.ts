import { useEffect, useRef, useState } from "react";
import { type TerminalSessionSummary } from "../terminal/types";
import { type TerminalEntry } from "./types";
import { agentTerminalId } from "../terminal/agentTerminalId";

interface UseTerminalSessionSummariesOptions {
  allTerminals: TerminalEntry[];
  workspaceBranchByPath?: Map<string, string>;
  currentBranch?: string | null;
  onTerminalsChange?: (summaries: TerminalSessionSummary[]) => void;
}

function getTerminalSummaryId(t: TerminalEntry) {
  return t.type === "shell" ? t.data.id : agentTerminalId(t.data.sessionId);
}

function summariesEqual(
  left: TerminalSessionSummary[],
  right: TerminalSessionSummary[],
) {
  if (left.length !== right.length) return false;
  return left.every((summary, index) => {
    const other = right[index];
    return (
      summary.id === other.id &&
      summary.kind === other.kind &&
      summary.name === other.name &&
      summary.branchName === other.branchName &&
      summary.isMainRepo === other.isMainRepo &&
      summary.agent === other.agent &&
      summary.isStreaming === other.isStreaming
    );
  });
}

/**
 * Tracks whether each terminal is streaming process output, and derives the
 * `TerminalSessionSummary` list the sidebar spinner and the idle-agent lookup
 * read.
 */
export function useTerminalSessionSummaries({
  allTerminals,
  workspaceBranchByPath,
  currentBranch,
  onTerminalsChange,
}: UseTerminalSessionSummariesOptions) {
  const [streaming, setStreaming] = useState<ReadonlySet<string>>(new Set());

  const setIsStreaming = (id: string, isStreaming: boolean) => {
    setStreaming((prev) => {
      if (prev.has(id) === isStreaming) return prev;
      const next = new Set(prev);
      if (isStreaming) next.add(id);
      else next.delete(id);
      return next;
    });
  };

  // Output echoed back from the user's own typing is not the process working.
  const handleTerminalOutput = (id: string, fromProcess = true) => {
    if (fromProcess) setIsStreaming(id, true);
  };

  const handleTerminalIdlePulse = (id: string) => {
    setIsStreaming(id, false);
  };

  const terminalSummaries: TerminalSessionSummary[] = allTerminals.map((t) => {
    const id = getTerminalSummaryId(t);
    const isStreaming = streaming.has(id);
    if (t.type === "agent") {
      return {
        id,
        kind: "agent" as const,
        name: t.data.sessionName,
        branchName: t.data.workspaceName ?? null,
        repoPath: t.data.repoPath,
        isMainRepo: !t.data.workspaceName,
        agent: t.data.agent,
        isStreaming,
      };
    }
    const resolvedBranch = workspaceBranchByPath?.get(t.data.workingDirectory);
    return {
      id,
      kind: "shell" as const,
      name: "Shell",
      branchName: resolvedBranch ?? currentBranch ?? null,
      isMainRepo: resolvedBranch === undefined,
      isStreaming,
    };
  });

  const onTerminalsChangeRef = useRef(onTerminalsChange);
  useEffect(() => {
    onTerminalsChangeRef.current = onTerminalsChange;
  }, [onTerminalsChange]);

  // `allTerminals` is a fresh array reference on every parent render (it's
  // derived inline, not memoized), so `terminalSummaries` above is too even
  // when nothing actually changed. Only notify the parent (which stores this
  // in state) when the derived content genuinely differs, otherwise this
  // effect would setState every render and loop forever.
  const lastNotifiedRef = useRef<TerminalSessionSummary[]>([]);
  useEffect(() => {
    if (summariesEqual(terminalSummaries, lastNotifiedRef.current)) return;
    lastNotifiedRef.current = terminalSummaries;
    onTerminalsChangeRef.current?.(terminalSummaries);
  }, [terminalSummaries]);

  return {
    handleTerminalOutput,
    handleTerminalIdlePulse,
  };
}
