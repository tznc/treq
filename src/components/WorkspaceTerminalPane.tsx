import { useEffect, useImperativeHandle, useRef, useState } from "react";
import { type ConsolidatedTerminalHandle } from "./ConsolidatedTerminal";
import { ptyClose } from "../lib/api";
import { ptyWrite } from "../lib/api-extra";
import { type AgentSessionData } from "./terminal/types";
import { type RemoteTerminalTarget } from "./RemoteTerminalPanel";
import { WorkspaceTerminalPaneView } from "./WorkspaceTerminalPaneView";
import { resolveTerminalWorkspace } from "./workspace-terminal-pane/resolveTerminalWorkspace";
import { resizeTerminalPair } from "./workspace-terminal-pane/resizeTerminalPair";
import { useScrollContainerWidth } from "./workspace-terminal-pane/useScrollContainerWidth";
import { useScrollTerminalIntoView } from "./workspace-terminal-pane/useScrollTerminalIntoView";
import { useTerminalPaneHeightResize } from "./workspace-terminal-pane/useTerminalPaneHeightResize";
import { useTerminalPaneKeyboardShortcuts } from "./workspace-terminal-pane/useTerminalPaneKeyboardShortcuts";
import { useTerminalSessionActions } from "./workspace-terminal-pane/useTerminalSessionActions";
import { useTerminalSessionSummaries } from "./workspace-terminal-pane/useTerminalSessionSummaries";
import {
  type ShellTerminalData,
  type WorkspaceTerminalPaneHandle,
  type WorkspaceTerminalPaneProps,
} from "./workspace-terminal-pane/types";
import { agentSessionIdOf, agentTerminalId } from "./terminal/agentTerminalId";

export type { WorkspaceTerminalPaneHandle };

const WorkspaceTerminalPaneInner = ({
  workingDirectory,
  remoteHost,
  onSessionError,
  currentBranch,
  agentSessions = [],
  activeAgentSessionId = null,
  onActiveSessionChange,
  onCreateNewSession,
  onCloseSession,
  onNavigateToWorkspace,
  workspaceBranchByPath,
  onTerminalsChange,
  resolveRemoteShell,
  className,
  ref,
}: WorkspaceTerminalPaneProps) => {
  // Shared pane state
  const [collapsed, setCollapsed] = useState(true);
  const [maximized, setMaximized] = useState(false);
  const { height, isResizingHeight, handleHeightResizeMouseDown } =
    useTerminalPaneHeightResize(maximized);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const paneRef = useRef<HTMLDivElement>(null);
  const { scrollToTerminal, handleTerminalDoubleClick } =
    useScrollTerminalIntoView(scrollContainerRef);
  const scrollToTerminalRef = useRef(scrollToTerminal);
  scrollToTerminalRef.current = scrollToTerminal;

  // Track which terminal is focused (last-clicked)
  const [activePtySessionId, setActivePtySessionId] = useState<string | null>(
    null,
  );

  // Clear active terminal when clicking outside the pane
  useEffect(() => {
    const handleMouseDown = (e: MouseEvent) => {
      if (
        activePtySessionId &&
        paneRef.current &&
        !paneRef.current.contains(e.target as Node)
      ) {
        setActivePtySessionId(null);
      }
    };
    document.addEventListener("mousedown", handleMouseDown);
    return () => document.removeEventListener("mousedown", handleMouseDown);
  }, [activePtySessionId]);

  // Track scroll container width for computing 40% min terminal width
  const containerWidth = useScrollContainerWidth(scrollContainerRef, collapsed);

  // Shell terminals - start empty (agent sessions are opened by default instead)
  const [shellTerminals, setShellTerminals] = useState<ShellTerminalData[]>([]);

  // Track mounted agent sessions to keep them alive
  const [mountedAgentSessions, setMountedAgentSessions] = useState<Set<number>>(
    new Set(),
  );

  // Track order of all terminals (shell and claude) by their IDs
  const [terminalOrder, setTerminalOrder] = useState<string[]>([]);

  // Track terminal widths by ID (null means flex-1, number is fixed pixel width)
  const [terminalWidths, setTerminalWidths] = useState<
    Map<string, number | null>
  >(new Map());

  // Shared refs for all terminals
  const terminalRefs = useRef<Map<string, ConsolidatedTerminalHandle | null>>(
    new Map(),
  );

  // Auto-mount active session when it changes (after creation or selection)
  useEffect(() => {
    if (activeAgentSessionId === null) return;

    const agentColumnId = agentTerminalId(activeAgentSessionId);

    setMountedAgentSessions((prev) => {
      if (prev.has(activeAgentSessionId)) return prev;
      const next = new Set(prev);
      next.add(activeAgentSessionId);
      return next;
    });

    setTerminalOrder((prev) => {
      if (prev.includes(agentColumnId)) return prev;
      return [...prev, agentColumnId];
    });

    setCollapsed(false);

    // Scroll to the new terminal after it's rendered
    scrollToTerminalRef.current(agentColumnId);
  }, [activeAgentSessionId]);

  // Derive the working directory for new terminals based on the active terminal's workspace.
  // Falls back to the sidebar-selected workspace (workingDirectory prop).
  const activeWorkspaceDir = (() => {
    if (!activePtySessionId) return null;

    // Check claude sessions
    const activeClaude = agentSessions.find(
      (s) => s.ptySessionId === activePtySessionId,
    );
    if (activeClaude) {
      return activeClaude.workspacePath || activeClaude.repoPath;
    }

    // Check shell terminals
    const activeShell = shellTerminals.find((s) => s.id === activePtySessionId);
    if (activeShell) {
      return activeShell.workingDirectory;
    }

    return null;
  })();

  // Add new shell terminal in the active terminal's workspace, or sidebar-selected workspace
  const handleAddShell = (dirOverride?: string) => {
    const dir = dirOverride || activeWorkspaceDir || workingDirectory;
    if (resolveRemoteShell) {
      // A remote repository's paths exist only on the host, so a local PTY
      // here would start in a directory that does not exist.
      const target = resolveRemoteShell(dir);
      if (target) handleOpenRemoteSession(target);
      return;
    }
    const newId = `shell-${dir.replace(/[^a-zA-Z0-9]/g, "-")}-${Date.now()}`;
    setShellTerminals((prev) => [
      ...prev,
      { id: newId, workingDirectory: dir, remoteHost },
    ]);
    // Add to terminal order (rightmost position)
    setTerminalOrder((prev) => [...prev, newId]);
    if (collapsed) {
      setCollapsed(false);
    }
    scrollToTerminal(newId);
  };

  // Remote sessions share the shell slot in the pane (ids start with
  // `shell-`) so ordering, focus, resize and close work unchanged.
  const handleOpenRemoteSession = (target: RemoteTerminalTarget) => {
    const newId = `shell-remote-${target.label.replace(/[^a-zA-Z0-9]/g, "-")}-${Date.now()}`;
    setShellTerminals((prev) => [
      ...prev,
      {
        id: newId,
        workingDirectory: target.remoteWorkingDirectory,
        remote: target,
      },
    ]);
    setTerminalOrder((prev) => [...prev, newId]);
    if (collapsed) {
      setCollapsed(false);
    }
    setActivePtySessionId(newId);
    scrollToTerminal(newId);
  };

  // Create Agent session in the active terminal's workspace, or sidebar-selected workspace
  const handleCreateAgentSession = () => {
    onCreateNewSession?.(activeWorkspaceDir);
  };

  const ptyIdOf = (terminalId: string) => {
    const sessionId = agentSessionIdOf(terminalId);
    if (sessionId === null) return terminalId;
    return (
      agentSessions.find((s) => s.sessionId === sessionId)?.ptySessionId ?? null
    );
  };

  // When the focused terminal closes, focus moves to the terminal that takes
  // its place, or to the one before it if it was the last, as closing a tab
  // does. Cmd+W can then keep closing terminals.
  const focusNeighbourOf = (terminalId: string) => {
    const index = terminalOrder.indexOf(terminalId);
    const remaining = terminalOrder.filter((id) => id !== terminalId);
    const neighbour = remaining[Math.min(index, remaining.length - 1)];
    setActivePtySessionId(neighbour ? ptyIdOf(neighbour) : null);
  };

  // Close shell terminal
  const handleCloseShell = (terminalId: string) => {
    console.info(
      "[WorkspaceTerminalPane] shell close requested",
      JSON.stringify({
        terminalId,
        activePtySessionId,
      }),
    );
    // Closing a remote entry detaches: the remote panel closes its SSH
    // channel on unmount and the persistent session keeps running. There is
    // no local PTY to close.
    const isRemote = shellTerminals.some(
      (t) => t.id === terminalId && t.remote,
    );
    (isRemote ? Promise.resolve() : ptyClose(terminalId))
      .then(() => {
        console.info(
          "[WorkspaceTerminalPane] ptyClose succeeded",
          JSON.stringify({ terminalId }),
        );
      })
      .catch((error) => {
        console.warn(
          "[WorkspaceTerminalPane] ptyClose failed",
          JSON.stringify({
            terminalId,
            error: error instanceof Error ? error.message : String(error),
          }),
        );
      });
    terminalRefs.current.delete(terminalId);
    console.info(
      "[WorkspaceTerminalPane] terminal ref deleted",
      JSON.stringify({ terminalId }),
    );
    setShellTerminals((prev) => prev.filter((t) => t.id !== terminalId));
    setTerminalOrder((prev) => prev.filter((id) => id !== terminalId));
    if (activePtySessionId === terminalId) {
      focusNeighbourOf(terminalId);
    }
  };

  // Close agent session
  const handleCloseAgentSession = (sessionId: number) => {
    const agentColumnId = agentTerminalId(sessionId);
    console.info(
      "[WorkspaceTerminalPane] agent session close requested",
      JSON.stringify({
        sessionId,
        agentColumnId,
        activePtySessionId,
      }),
    );
    const sessionData = agentSessions.find((s) => s.sessionId === sessionId);
    if (sessionData) {
      ptyClose(sessionData.ptySessionId).catch(console.error);
      terminalRefs.current.delete(agentColumnId);
      console.info(
        "[WorkspaceTerminalPane] agent terminal ref deleted",
        JSON.stringify({
          sessionId,
          agentColumnId,
          ptySessionId: sessionData.ptySessionId,
        }),
      );
    }
    setMountedAgentSessions((prev) => {
      const next = new Set(prev);
      next.delete(sessionId);
      return next;
    });
    setTerminalOrder((prev) => prev.filter((id) => id !== agentColumnId));
    onCloseSession?.(sessionId);
    console.info(
      "[WorkspaceTerminalPane] onCloseSession callback fired",
      JSON.stringify({ sessionId }),
    );
    if (activePtySessionId === sessionData?.ptySessionId) {
      focusNeighbourOf(agentColumnId);
    }
  };

  // Terminal width resize handler
  const handleTerminalResize = (
    leftId: string,
    rightId: string,
    deltaX: number,
  ) => {
    if (!scrollContainerRef.current) return;

    setTerminalWidths((prev) => {
      const newWidths = new Map(prev);
      const container = scrollContainerRef.current;
      if (!container) return prev;

      // Minimum width is 2/5 of scroll container viewport
      const minWidth = containerWidth * 0.4 || 300;

      // Get current widths - if null, calculate from actual element width
      const leftEl = container.querySelector(
        `[data-terminal-id="${leftId}"]`,
      ) as HTMLElement | null;
      const rightEl = container.querySelector(
        `[data-terminal-id="${rightId}"]`,
      ) as HTMLElement | null;

      if (!leftEl || !rightEl) return prev;

      const leftCurrentWidth =
        prev.get(leftId) ?? leftEl.getBoundingClientRect().width;
      const rightCurrentWidth =
        prev.get(rightId) ?? rightEl.getBoundingClientRect().width;

      const { left: newLeftWidth, right: newRightWidth } = resizeTerminalPair({
        leftWidth: leftCurrentWidth,
        rightWidth: rightCurrentWidth,
        deltaX,
        minWidth,
      });

      newWidths.set(leftId, newLeftWidth);
      newWidths.set(rightId, newRightWidth);

      return newWidths;
    });
  };

  // Show ALL mounted agent sessions (no workspace filtering)
  const agentSessionsToRender = agentSessions.filter((s) => {
    const isActiveSession = activeAgentSessionId === s.sessionId;
    return isActiveSession || mountedAgentSessions.has(s.sessionId);
  });

  useTerminalPaneKeyboardShortcuts({
    setCollapsed,
    maximized,
    setMaximized,
    handleCreateAgentSession,
    handleAddShell,
    activePtySessionId,
    agentSessions,
    handleCloseShell,
    handleCloseAgentSession,
  });

  // Build ordered list of all terminals for rendering based on terminalOrder
  const shellTerminalMap = new Map(shellTerminals.map((t) => [t.id, t]));
  const agentSessionMap = new Map(
    agentSessionsToRender.map((s) => [agentTerminalId(s.sessionId), s]),
  );

  const orderedTerminals: Array<
    | { type: "shell"; data: ShellTerminalData }
    | { type: "agent"; data: AgentSessionData }
  > = terminalOrder
    .map((id) => {
      if (id.startsWith("shell-")) {
        const shellData = shellTerminalMap.get(id);
        if (shellData) {
          return { type: "shell" as const, data: shellData };
        }
      } else if (agentSessionIdOf(id) !== null) {
        const agentData = agentSessionMap.get(id);
        if (agentData) {
          return { type: "agent" as const, data: agentData };
        }
      }
      return null;
    })
    .filter((t): t is NonNullable<typeof t> => t !== null);

  // Ensure newly created agent sessions render immediately even before their IDs
  // are added to terminalOrder (e.g., pending agent sessions).
  const missingAgentTerminals = agentSessionsToRender
    .filter(
      (session) => !terminalOrder.includes(agentTerminalId(session.sessionId)),
    )
    .map((session) => ({ type: "agent" as const, data: session }));

  const allTerminals = [...orderedTerminals, ...missingAgentTerminals];

  // Auto-collapse when all terminals are closed
  useEffect(() => {
    if (allTerminals.length === 0) {
      setCollapsed(true);
      setMaximized(false);
    }
  }, [allTerminals.length]);

  // Track which terminals are streaming, for the sidebar spinner.
  const { handleTerminalOutput, handleTerminalIdlePulse } =
    useTerminalSessionSummaries({
      allTerminals,
      workspaceBranchByPath,
      currentBranch,
      onTerminalsChange,
    });

  const { handleFocusTerminalById, closeTerminalsForWorkspace } =
    useTerminalSessionActions({
      agentSessions,
      shellTerminals,
      workspaceBranchByPath,
      setCollapsed,
      setMountedAgentSessions,
      setTerminalOrder,
      setActivePtySessionId,
      onActiveSessionChange,
      onNavigateToWorkspace,
      scrollToTerminal,
      handleCloseAgentSession,
      handleCloseShell,
    });

  // Expose methods via ref for command palette + sidebar terminal list
  useImperativeHandle(
    ref,
    () => ({
      toggleCollapse: () => setCollapsed((prev) => !prev),
      toggleMaximize: () => {
        if (maximized) {
          setMaximized(false);
        } else {
          setCollapsed(false);
          setMaximized(true);
        }
      },
      createShellSession: handleAddShell,
      openRemoteSession: handleOpenRemoteSession,
      closeTerminalsForWorkspace,
      sendToTerminal: (id: string, text: string) => {
        const session = agentSessions.find(
          (item) => agentTerminalId(item.sessionId) === id,
        );
        if (!session) return;
        void ptyWrite(session.ptySessionId, `${text}\n`);
        handleFocusTerminalById(id);
      },
    }),
    [
      maximized,
      handleAddShell,
      handleOpenRemoteSession,
      agentSessions,
      closeTerminalsForWorkspace,
      handleFocusTerminalById,
    ],
  );

  const terminals = allTerminals.map((terminal) => ({
    terminal,
    workspace: resolveTerminalWorkspace(terminal, {
      agentSessions,
      workspaceBranchByPath,
      currentBranch,
    }),
  }));

  // Scroll to a terminal in the selected workspace when workingDirectory changes
  useEffect(() => {
    if (collapsed || !scrollContainerRef.current) return;
    const matching = terminals.find(
      ({ workspace }) => workspace.workspaceKey === workingDirectory,
    );
    if (!matching) return;

    const terminalId =
      matching.terminal.type === "shell"
        ? matching.terminal.data.id
        : agentTerminalId(matching.terminal.data.sessionId);

    requestAnimationFrame(() => {
      const el = scrollContainerRef.current?.querySelector(
        `[data-terminal-id="${CSS.escape(terminalId)}"]`,
      );
      if (el) {
        el.scrollIntoView({
          behavior: "smooth",
          block: "nearest",
          inline: "start",
        });
      }
    });

    if (matching.terminal.type === "shell") {
      setActivePtySessionId(matching.terminal.data.id);
    } else {
      setActivePtySessionId(matching.terminal.data.ptySessionId);
    }
  }, [workingDirectory]);

  const totalTerminals = allTerminals.length;

  return (
    <WorkspaceTerminalPaneView
      paneRef={paneRef}
      className={className}
      collapsed={collapsed}
      maximized={maximized}
      height={height}
      isResizingHeight={isResizingHeight}
      handleHeightResizeMouseDown={handleHeightResizeMouseDown}
      totalTerminals={totalTerminals}
      setCollapsed={setCollapsed}
      setMaximized={setMaximized}
      scrollContainerRef={scrollContainerRef}
      terminals={terminals}
      containerWidth={containerWidth}
      onNavigateToWorkspace={onNavigateToWorkspace}
      remoteHost={remoteHost}
      activePtySessionId={activePtySessionId}
      setActivePtySessionId={setActivePtySessionId}
      handleCloseShell={handleCloseShell}
      onSessionError={onSessionError}
      terminalRefs={terminalRefs}
      terminalWidths={terminalWidths}
      handleTerminalResize={handleTerminalResize}
      handleCloseAgentSession={handleCloseAgentSession}
      onTerminalDoubleClick={handleTerminalDoubleClick}
      onTerminalOutput={handleTerminalOutput}
      onTerminalIdle={handleTerminalIdlePulse}
    />
  );
};

export const WorkspaceTerminalPane = WorkspaceTerminalPaneInner;
