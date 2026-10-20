import { useCallback, useMemo, useRef, useState } from "react";
import { useKeyboardShortcut } from "../hooks/useKeyboard";
import type {
  GpuiTerminalCommand,
  GpuiTerminalRoute,
} from "../lib/gpui-screen";
import { useTreqSendStore } from "../stores/treqSendStore";
import { agentSessionIdOf } from "./terminal/agentTerminalId";
import type { WorkspaceTerminalPaneHandle } from "./workspace-terminal-pane/types";

/** Commands kept in the route; GPUI runs those newer than the last it ran. */
const KEPT = 20;

type Pending = GpuiTerminalRoute["pending"][number];

/**
 * The terminal pane while GPUI draws it: the route GPUI reads, and a handle
 * like `WorkspaceTerminalPane`'s that sends it commands.
 */
export function useGpuiTerminalPane({
  enabled,
  activeSessionId,
  pending,
  sessionsRevision,
  remoteShell = false,
  onNewAgent,
}: {
  enabled: boolean;
  /** A remote repository is active: new shells run on its host. */
  remoteShell?: boolean;
  activeSessionId: number | null;
  pending: Pending[];
  sessionsRevision: number;
  /** Cmd+] while the webview has focus: an agent in the selected workspace. */
  onNewAgent: () => void;
}): { route: GpuiTerminalRoute | null; handle: WorkspaceTerminalPaneHandle } {
  const [commands, setCommands] = useState<GpuiTerminalCommand[]>([]);
  const nonce = useRef(0);
  const send = useCallback((command: Omit<GpuiTerminalCommand, "nonce">) => {
    nonce.current += 1;
    const next = { ...command, nonce: nonce.current };
    setCommands((previous) => [...previous, next].slice(-KEPT));
  }, []);

  const handle = useMemo<WorkspaceTerminalPaneHandle>(
    () => ({
      toggleCollapse: () => send({ kind: "toggleCollapse" }),
      toggleMaximize: () => send({ kind: "toggleMaximize" }),
      createShellSession: (dir) => send({ kind: "newShell", dir }),
      openRemoteSession: (target) =>
        send({ kind: "openRemote", remote: target }),
      closeTerminalsForWorkspace: (dir) =>
        send({ kind: "closeWorkspace", dir }),
      sendToTerminal: (id, text) => {
        const sessionId = agentSessionIdOf(id);
        if (sessionId !== null) send({ kind: "send", sessionId, text });
      },
    }),
    [send],
  );

  // The pane's shortcuts while the webview has focus; GPUI handles them
  // while it does.
  useKeyboardShortcut(
    "j",
    true,
    () => enabled && send({ kind: "toggleCollapse" }),
    [enabled],
  );
  useKeyboardShortcut(
    "j",
    true,
    () => enabled && send({ kind: "toggleMaximize" }),
    [enabled],
    { requireBothCmdAndCtrl: true },
  );
  useKeyboardShortcut("]", true, () => enabled && onNewAgent(), [
    enabled,
    onNewAgent,
  ]);
  useKeyboardShortcut("\\", true, () => enabled && send({ kind: "newShell" }), [
    enabled,
  ]);

  const assets = useTreqSendStore((s) => s.assets);
  const sendAssets = useMemo(
    () =>
      assets.map(({ id, repo, ptySessionId, mediaType, path, title }) => ({
        id,
        repo,
        ptySessionId,
        mediaType,
        path,
        title,
      })),
    [assets],
  );

  const route = useMemo(
    () =>
      enabled
        ? {
            activeSessionId,
            pending,
            sessionsRevision,
            commands,
            sendAssets,
            remoteShell,
          }
        : null,
    [
      enabled,
      activeSessionId,
      pending,
      sessionsRevision,
      commands,
      sendAssets,
      remoteShell,
    ],
  );
  return { route, handle };
}
