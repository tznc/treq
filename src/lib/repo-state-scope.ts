import {
  matchesActiveCanonicalPath,
  repositoryCacheKey,
} from "./active-repository";
import {
  remoteRepositoryContaining,
  trimTrailingSlashes,
} from "./repository-adapter";

/** Read by the Rust local DB as a remote repository's state scope (`RepoStateScope::Remote`). */
const REMOTE_STATE_SCOPE_PREFIX = "treq-remote-state:";

/**
 * The key client-side review state (drafts, viewed marks, review comments)
 * is stored under. A local path is its own key. A path in the active remote
 * repository maps to that repository's descriptor identity plus the path
 * below its root, so no state is written at the remote path on this machine.
 */
export function repoStateScope(path: string): string {
  const repo = remoteRepositoryContaining(path);
  if (!repo) return path;
  const below = matchesActiveCanonicalPath(repo, path)
    ? ""
    : trimTrailingSlashes(path).slice(
        trimTrailingSlashes(repo.canonicalPath).length,
      );
  return `${REMOTE_STATE_SCOPE_PREFIX}${repositoryCacheKey(repo)}${below}`;
}
