import type {
  RemoteRepository,
  RepositoryLocation,
  SshEndpoint,
} from "./api-types-remote";
import { remoteRepoIdentity } from "./remote-query-keys";

type RepositoryTransport =
  | { type: "local" }
  | { type: "ssh"; endpoint: SshEndpoint }
  /** A remote repository with no resolved endpoint. Every operation on it fails closed. */
  | { type: "unresolved" };

/**
 * Transport-aware descriptor for the repository the desktop UI is showing.
 * Local and SSH repositories share this shape so cache keys, adapters, and
 * the workspace tree do not branch on a parallel "remote screen".
 */
export interface ActiveRepository {
  id: string;
  location: RepositoryLocation;
  endpoint: SshEndpoint | null;
  endpointId: string | null;
  endpointGeneration: number;
  canonicalPath: string;
  displayName: string;
  transport: RepositoryTransport;
}

export function isRemoteRepository(
  repo: ActiveRepository | null | undefined,
): repo is ActiveRepository & {
  location: { type: "ssh"; host: string; path: string };
} {
  return repo?.location.type === "ssh";
}

export function localActiveRepository(path: string): ActiveRepository {
  return {
    id: path,
    location: { type: "local", path },
    endpoint: null,
    endpointId: null,
    endpointGeneration: 0,
    transport: { type: "local" },
    canonicalPath: path,
    displayName: path,
  };
}

export function repositoryCacheKey(repo: ActiveRepository): string {
  return remoteRepoIdentity(repo.location, {
    endpointGeneration: repo.endpointGeneration,
    endpointId: repo.endpointId ?? repo.endpoint?.id ?? null,
  });
}

export type PersistedRemoteRepository = RemoteRepository & {
  endpoint?: SshEndpoint | null;
  endpoint_id?: string | null;
  endpoint_generation?: number | null;
};

export function activeRepositoryFromRemote(
  saved: PersistedRemoteRepository,
  endpoint?: SshEndpoint | null,
): ActiveRepository {
  const sshEndpoint = endpoint ?? saved.endpoint ?? null;
  // A typed `InspectRepository` runs on the remote host, so its descriptor
  // describes the repository as local to that host. Never take a local
  // location from it: this repository is remote from here.
  const inspected = saved.inspection?.descriptor;
  const descriptor = inspected?.location.type === "ssh" ? inspected : null;
  const location: RepositoryLocation = descriptor?.location ?? {
    type: "ssh",
    host: saved.host,
    path: saved.path,
  };
  const canonicalPath = location.path;
  const endpointId =
    sshEndpoint?.id ?? saved.endpoint_id ?? descriptor?.id ?? null;
  const generation =
    saved.endpoint_generation ??
    (sshEndpoint?.source &&
    typeof sshEndpoint.source === "object" &&
    "generation" in sshEndpoint.source
      ? Number(sshEndpoint.source.generation)
      : 0);
  return {
    id: descriptor?.id ?? `${saved.host}:${saved.path}`,
    location,
    endpoint: sshEndpoint,
    endpointId,
    endpointGeneration: generation ?? 0,
    canonicalPath,
    displayName: saved.display_name,
    transport: sshEndpoint
      ? { type: "ssh", endpoint: sshEndpoint }
      : { type: "unresolved" },
  };
}

let current: ActiveRepository | null = null;

export function peekActiveRepository(): ActiveRepository | null {
  return current;
}

export function setActiveRepositorySingleton(repo: ActiveRepository | null) {
  current = repo;
}

export function matchesActiveCanonicalPath(
  repo: ActiveRepository | null,
  repoPath: string | undefined,
): boolean {
  if (!repo || !repoPath) return false;
  return (
    repo.canonicalPath === repoPath || repositoryCacheKey(repo) === repoPath
  );
}
