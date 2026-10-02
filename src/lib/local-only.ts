import { remoteRepositoryContaining } from "./repository-adapter";

/**
 * Runs `read` for a local repository. A remote repository has no local clone
 * for `git`/`gh` to read, so it gets `empty` until typed remote routes exist.
 */
export function localReadOr<T>(
  repoPath: string,
  empty: T,
  read: () => Promise<T>,
): Promise<T> {
  return remoteRepositoryContaining(repoPath) ? Promise.resolve(empty) : read();
}

/** `repoPath` for a local repository; `null` (application scope) for a remote one. */
export function localRepoPathOrNull(repoPath?: string | null): string | null {
  return repoPath && !remoteRepositoryContaining(repoPath) ? repoPath : null;
}
