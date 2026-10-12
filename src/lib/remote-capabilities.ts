/**
 * Capabilities the current client may offer for a remote repository.
 *
 * Remote shells and agents run over the native russh PTY channel
 * (`remote_pty_*` commands), inside a persistent tmux/screen session on the
 * host so they can be detached and reattached. A system `ssh` executable is
 * never used.
 */
export const REMOTE_MERGE_REASON =
  "Merging a remote workspace is not available yet. Push the branch and merge it from your hosting provider instead.";

export const REMOTE_FILE_TREE_REASON =
  "Browsing the file tree is not available for remote repositories yet. Use Go to file to open a file.";

export interface ActionCapability {
  supported: boolean;
  reason?: string;
}

export interface RemoteCapabilities {
  shell: ActionCapability;
  agentPty: ActionCapability;
  agentLifecycle: ActionCapability;
  splitCommit: ActionCapability;
  agentInput: ActionCapability;
  mergeWorkspace: ActionCapability;
}

export function remoteCapabilities(): RemoteCapabilities {
  return {
    shell: { supported: true },
    agentPty: { supported: true },
    agentLifecycle: { supported: true },
    // Routed through the typed `SplitCommit` command (whole files of a
    // workspace's working copy).
    splitCommit: { supported: true },
    agentInput: { supported: true },
    mergeWorkspace: { supported: false, reason: REMOTE_MERGE_REASON },
  };
}

export function localCapabilities(): RemoteCapabilities {
  return {
    shell: { supported: true },
    agentPty: { supported: true },
    agentLifecycle: { supported: true },
    splitCommit: { supported: true },
    agentInput: { supported: true },
    mergeWorkspace: { supported: true },
  };
}

export function capabilitiesFor(isRemote: boolean): RemoteCapabilities {
  return isRemote ? remoteCapabilities() : localCapabilities();
}
