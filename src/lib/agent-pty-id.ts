const repoHash = (repoPath: string) => {
  let hash = 0;
  for (const char of repoPath) {
    hash = (hash * 31 + (char.codePointAt(0) ?? 0)) % 2_147_483_647;
  }
  return hash;
};

/**
 * PTY id for an agent session. Session ids are per repository and PTYs are
 * shared by every window, so the id carries a hash of the repository path.
 * Tauri event names allow only `[A-Za-z0-9-/:_]`, hence the base-36 hash.
 */
export const agentPtySessionId = (repoPath: string, sessionId: number) =>
  `session-${repoHash(repoPath).toString(36)}-${sessionId}`;

/**
 * Terminal-pane id for a session. The pane shows sessions from the main and
 * supporting repositories, whose database ids overlap. Main-repository ids
 * stay as they are; others are offset by a hash of their repository path.
 */
export const paneSessionId = (
  repoPath: string,
  mainRepoPath: string,
  sessionId: number,
) =>
  repoPath === mainRepoPath
    ? sessionId
    : (repoHash(repoPath) + 1) * 1_000_000 + sessionId;
