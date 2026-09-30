/**
 * PTY id for an agent session. Session ids are per repository and PTYs are
 * shared by every window, so the id carries a hash of the repository path.
 * Tauri event names allow only `[A-Za-z0-9-/:_]`, hence the base-36 hash.
 */
export const agentPtySessionId = (repoPath: string, sessionId: number) => {
  let hash = 0;
  for (const char of repoPath) {
    hash = (hash * 31 + (char.codePointAt(0) ?? 0)) % 2_147_483_647;
  }
  return `session-${hash.toString(36)}-${sessionId}`;
};
