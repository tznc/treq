import { useEffect, useRef, useState } from "react";
import type { SshEndpoint } from "../lib/api-types-remote";
import {
  type GpuiRemoteRoute,
  type GpuiScreenAction,
  gpuiDrawsRemoteDialogs,
  listenGpuiScreenActions,
  useGpuiDrawsRemoteDialogs,
  useGpuiRemoteStore,
} from "../lib/gpui-screen";
import {
  refreshRemoteRepository,
  useRemoteMutationFeedback,
} from "../lib/remote-mutation-ui";
import { newRemoteSessionLabel } from "../lib/remote-terminal-target";
import type { RemoteTerminalTarget } from "./RemoteTerminalPanel";
import type { CloudWorkspaceCardProps } from "./remote/CloudWorkspaceCard";
import type { UserManagedFormValues } from "./remote/remoteSetupLabels";
import { toGpuiCloudState } from "./useGpuiSettingsScreen";

/** The cloud workspace card's buttons GPUI hands back, by press id. */
const CLOUD_ACTIONS = {
  "remote.cloud.provision": "onProvision",
  "remote.cloud.wake": "onWake",
  "remote.cloud.repair": "onRepair",
  "remote.cloud.delete": "onDelete",
} as const;

interface TerminalDialog {
  endpoint: SshEndpoint;
  repositoryId: string;
  workspaceId: string;
  remoteWorkingDirectory: string;
}

/**
 * Draws Dashboard's remote repository dialogs (RemoteSetupDialog,
 * RemoteTerminalDialog, RemoteAmbiguousMutationDialog) in GPUI over whichever
 * screen GPUI draws, which reads the SSH keys, host keys and persistent
 * sessions from the backend itself
 * (src-tauri/src/gpui_shell/screen/remote.rs). React keeps what Dashboard
 * state owns: the cloud workspace flows, registering an endpoint, opening a
 * terminal session and the mutation feedback store.
 *
 * Returns whether GPUI draws the dialogs now. While it does, the React
 * dialogs stay closed; over a view GPUI does not draw, they open as before.
 */
export function useGpuiRemoteDialogs(params: {
  setupOpen: boolean;
  allowUserManaged: boolean;
  cloudWorkspace: CloudWorkspaceCardProps;
  onCloseSetup: () => void;
  onRegisterUserManaged: (values: UserManagedFormValues) => Promise<void>;
  /** Null while the terminal dialog is closed. */
  terminal: TerminalDialog | null;
  onCloseTerminal: () => void;
  onOpenTerminalTarget: (target: RemoteTerminalTarget) => void;
}): boolean {
  const enabled = gpuiDrawsRemoteDialogs();
  const draws = useGpuiDrawsRemoteDialogs();
  const ambiguous = useRemoteMutationFeedback((s) => s.ambiguousReason);
  const clearAmbiguous = useRemoteMutationFeedback((s) => s.clearAmbiguous);
  const [cloudSubmitting, setCloudSubmitting] = useState(false);
  const [registering, setRegistering] = useState(false);

  const setup = params.setupOpen
    ? {
        allowUserManaged: params.allowUserManaged,
        cloud: toGpuiCloudState(params.cloudWorkspace, cloudSubmitting),
        registering,
      }
    : null;
  const terminal = params.terminal
    ? {
        endpoint: params.terminal.endpoint,
        repositoryId: params.terminal.repositoryId,
        workspaceId: params.terminal.workspaceId,
      }
    : null;
  const route: GpuiRemoteRoute | null =
    setup || terminal || ambiguous
      ? { setup, terminal, ambiguous: ambiguous || null }
      : null;
  const json = enabled && route ? JSON.stringify(route) : null;

  useEffect(() => {
    useGpuiRemoteStore.setState({ route: json ? JSON.parse(json) : null });
  }, [json]);
  useEffect(() => () => useGpuiRemoteStore.setState({ route: null }), []);

  const runCloud = async (action: () => Promise<void>) => {
    setCloudSubmitting(true);
    try {
      await action();
    } finally {
      setCloudSubmitting(false);
    }
  };

  const register = async (values: UserManagedFormValues) => {
    setRegistering(true);
    try {
      await params.onRegisterUserManaged(values);
    } finally {
      setRegistering(false);
    }
  };

  const handle = (action: GpuiScreenAction) => {
    if (action.kind === "press") {
      if (action.id in CLOUD_ACTIONS) {
        if (!params.setupOpen) return;
        const name = CLOUD_ACTIONS[action.id as keyof typeof CLOUD_ACTIONS];
        void runCloud(params.cloudWorkspace[name]);
      } else if (action.id === "remote.cloud.openRepositories") {
        if (params.setupOpen) params.cloudWorkspace.onOpenRepositories?.();
      } else if (action.id === "remote.setup.close") {
        params.onCloseSetup();
      } else if (action.id === "remote.terminal.close") {
        params.onCloseTerminal();
      } else if (action.id === "remote.ambiguous.dismiss") {
        clearAmbiguous();
      } else if (action.id === "remote.ambiguous.refresh") {
        // Pending idempotency keys stay: the refresh may fail while the
        // connection is down, and a retry must still carry the original
        // key so the VM replays the first result instead of rerunning.
        refreshRemoteRepository();
        clearAmbiguous();
      }
    } else if (action.kind === "field") {
      if (action.field === "remote.registerUserManaged") {
        if (params.setupOpen) {
          void register(action.value as unknown as UserManagedFormValues);
        }
      } else if (action.field === "remote.terminal.open" && params.terminal) {
        const { label, reattach } = action.value as unknown as {
          label: string | null;
          reattach: boolean;
        };
        params.onOpenTerminalTarget({
          endpoint: params.terminal.endpoint,
          repositoryId: params.terminal.repositoryId,
          workspaceId: params.terminal.workspaceId,
          remoteWorkingDirectory: params.terminal.remoteWorkingDirectory,
          // A new session is always fresh: reusing a label that is
          // already running would attach to that session instead.
          label: label ?? newRemoteSessionLabel(null),
          reattach,
        });
        params.onCloseTerminal();
      }
    }
  };

  const latest = useRef(handle);
  latest.current = handle;
  useEffect(() => {
    if (!enabled) return;
    const unlisten = listenGpuiScreenActions((action) =>
      latest.current(action),
    );
    return () => {
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [enabled]);

  return draws;
}
