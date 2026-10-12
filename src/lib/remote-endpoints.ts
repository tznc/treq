// Local persistence for user-managed SSH endpoints and the remote
// repositories opened on them. User-managed endpoints do not go through the
// control plane (prds/remote-development.md, "User modes" - no certificate issuance
// required), so they are recorded on-device, keyed by endpoint id, using the
// same `getSetting`/`setSetting` local settings store the app already uses
// for "last opened remote repo" and recent-host memory.
//
// A saved repository is restored only after the caller re-runs readiness and
// host-trust validation (see `restoreSavedRemoteRepository` in
// `remote-repository.ts`) - this module only persists the descriptors, it
// never assumes they are still trustworthy.

import { getSetting, setSetting } from "./api";
import type {
  SshAuthentication,
  SshEndpoint,
  TrustedHostKey,
} from "./api-types-remote";

const ENDPOINTS_KEY = "remote_user_managed_endpoints";
const SAVED_REPOS_KEY = "remote_saved_repositories";

export interface UserManagedEndpointRecord {
  id: string;
  display_name: string;
  hostname: string;
  port: number;
  username: string;
  host_key_fingerprint: string;
  auth_identity_reference: string;
  /** Set only when the user explicitly chose alias mode for this endpoint. */
  alias: string | null;
  created_at: string;
}

export interface SavedRemoteRepositoryRecord {
  id: string;
  endpoint_id: string;
  /** Generation of the endpoint at the time this was saved, for trust-transition detection. */
  endpoint_generation: number;
  canonical_remote_path: string;
  display_name: string;
  /** ISO timestamp of the last successful restore/trust sequence, or null. */
  last_successful_trust_validation: string | null;
}

async function readJson<T>(key: string, fallback: T): Promise<T> {
  const raw = await getSetting(key).catch(() => null);
  if (!raw) return fallback;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

export async function listUserManagedEndpoints(): Promise<
  UserManagedEndpointRecord[]
> {
  return readJson(ENDPOINTS_KEY, []);
}

export async function saveUserManagedEndpoint(
  record: UserManagedEndpointRecord,
): Promise<void> {
  const existing = await listUserManagedEndpoints();
  const next = [
    record,
    ...existing.filter((endpoint) => endpoint.id !== record.id),
  ];
  await setSetting(ENDPOINTS_KEY, JSON.stringify(next));
}

function normalizeSavedRecord(
  raw: SavedRemoteRepositoryRecord & { remote_path?: string },
): SavedRemoteRepositoryRecord {
  return {
    id: raw.id,
    endpoint_id: raw.endpoint_id,
    endpoint_generation: raw.endpoint_generation,
    canonical_remote_path: raw.canonical_remote_path ?? raw.remote_path ?? "",
    display_name: raw.display_name,
    last_successful_trust_validation:
      raw.last_successful_trust_validation ?? null,
  };
}

export async function listSavedRemoteRepositories(): Promise<
  SavedRemoteRepositoryRecord[]
> {
  const raw = await readJson<
    Array<SavedRemoteRepositoryRecord & { remote_path?: string }>
  >(SAVED_REPOS_KEY, []);
  return raw.map(normalizeSavedRecord);
}

export async function saveRemoteRepository(
  record: SavedRemoteRepositoryRecord,
): Promise<void> {
  const normalized = normalizeSavedRecord(record);
  const existing = await listSavedRemoteRepositories();
  const next = [
    normalized,
    ...existing.filter(
      (repo) =>
        repo.id !== normalized.id &&
        !(
          repo.endpoint_id === normalized.endpoint_id &&
          repo.endpoint_generation === normalized.endpoint_generation &&
          repo.canonical_remote_path === normalized.canonical_remote_path
        ),
    ),
  ];
  await replaceSavedRemoteRepositories(next);
}

export async function replaceSavedRemoteRepositories(
  records: SavedRemoteRepositoryRecord[],
): Promise<void> {
  await setSetting(
    SAVED_REPOS_KEY,
    JSON.stringify(records.map(normalizeSavedRecord)),
  );
}

/** Builds the trusted-host-key record from a user-entered fingerprint. */
function trustedHostKeyFromFingerprint(fingerprint: string): TrustedHostKey {
  return {
    algorithm: "unknown",
    fingerprint_sha256: fingerprint,
    comment: null,
  };
}

function publicKeyAuthentication(keyReference: string): SshAuthentication {
  return { type: "public_key", key_reference: keyReference };
}

/** The endpoint the native transport connects with for a saved user-managed host. */
export function sshEndpointFromUserManaged(
  record: UserManagedEndpointRecord,
): SshEndpoint {
  return {
    id: record.id,
    instance_id: null,
    source: { type: "user_managed" },
    hostname: record.hostname,
    port: record.port,
    username: record.username,
    host_keys: [trustedHostKeyFromFingerprint(record.host_key_fingerprint)],
    authentication: publicKeyAuthentication(record.auth_identity_reference),
  };
}
