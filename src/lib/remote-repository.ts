// Endpoint-aware remote repository registry (prds/remote-development.md,
// "Repository opening"): persist descriptors, probe/inspect/clone/init
// through typed commands, and restore a saved repository only after
// reconnect + host-key + generation (or explicit transition) + typed
// inspect succeed. Credentials and private keys are never stored.

import { getSetting, setSetting } from "./api";
import type { RepositoryInspection } from "./api-types-remote";
import {
  listSavedRemoteRepositories,
  replaceSavedRemoteRepositories,
  saveRemoteRepository,
  type SavedRemoteRepositoryRecord,
} from "./remote-endpoints";

export const LAST_OPENED_REMOTE_REPO_ID_KEY = "last_opened_remote_repo_id";
const REMOTE_RECENT_REPO_IDS_KEY = "remote_recent_repository_ids";
/** Legacy blob. Never restore from this without the trust sequence. */
const LAST_OPENED_REMOTE_REPO_KEY = "last_opened_remote_repo";

const DESCRIPTOR_SECRET_KEYS = [
  "private_key",
  "privateKey",
  "password",
  "passphrase",
  "credential",
  "credentials",
  "secret",
  "ssh_key",
  "sshKey",
] as const;

type RestoreFailureReason =
  | "reconnect_failed"
  | "host_key_mismatch"
  | "generation_mismatch"
  | "inspect_failed";

export type RestoreResult =
  | {
      ok: true;
      descriptor: SavedRemoteRepositoryRecord;
      inspection: RepositoryInspection;
    }
  | { ok: false; reason: RestoreFailureReason };

export function canonicalizeRemotePath(path: string): string {
  const trimmed = path.trim();
  if (!trimmed) return "";
  const homeRelative = trimmed === "~" || trimmed.startsWith("~/");
  let rest = homeRelative ? trimmed.slice(1) : trimmed.replace(/\\/g, "/");
  rest = rest.replace(/\\/g, "/");
  const absolute = rest.startsWith("/");
  const parts = rest
    .split("/")
    .filter((part) => part.length > 0 && part !== ".");
  const stack: string[] = [];
  for (const part of parts) {
    if (part === "..") {
      if (stack.length > 0) {
        stack.pop();
      }
      continue;
    }
    stack.push(part);
  }
  const joined = stack.join("/");
  if (homeRelative) return joined ? `~/${joined}` : "~";
  if (absolute) return `/${joined}`;
  return joined;
}

function descriptorId(
  endpointId: string,
  generation: number,
  canonicalPath: string,
): string {
  return `remote-repo:${endpointId}:gen${generation}:${canonicalPath}`;
}

export function isSecretFreeDescriptor(
  record: SavedRemoteRepositoryRecord,
): boolean {
  const json = JSON.stringify(record);
  return !DESCRIPTOR_SECRET_KEYS.some((key) =>
    json.toLowerCase().includes(key.toLowerCase()),
  );
}

function toPersistedRecord(
  record: SavedRemoteRepositoryRecord,
): SavedRemoteRepositoryRecord {
  return {
    id: record.id,
    endpoint_id: record.endpoint_id,
    endpoint_generation: record.endpoint_generation,
    canonical_remote_path: record.canonical_remote_path,
    display_name: record.display_name,
    last_successful_trust_validation: record.last_successful_trust_validation,
  };
}

export async function upsertSavedRemoteRepository(input: {
  endpoint_id: string;
  endpoint_generation: number;
  remote_path: string;
  display_name?: string;
  last_successful_trust_validation?: string | null;
}): Promise<SavedRemoteRepositoryRecord> {
  const canonical = canonicalizeRemotePath(input.remote_path);
  if (!canonical) {
    throw new Error("Remote path is required");
  }
  const id = descriptorId(
    input.endpoint_id,
    input.endpoint_generation,
    canonical,
  );
  const existing = await listSavedRemoteRepositories();
  const duplicate = existing.find(
    (repo) =>
      repo.endpoint_id === input.endpoint_id &&
      repo.endpoint_generation === input.endpoint_generation &&
      canonicalizeRemotePath(repo.canonical_remote_path) === canonical,
  );
  const record = toPersistedRecord({
    id: duplicate?.id ?? id,
    endpoint_id: input.endpoint_id,
    endpoint_generation: input.endpoint_generation,
    canonical_remote_path: canonical,
    display_name:
      input.display_name?.trim() || duplicate?.display_name || canonical,
    last_successful_trust_validation:
      input.last_successful_trust_validation !== undefined
        ? input.last_successful_trust_validation
        : (duplicate?.last_successful_trust_validation ?? null),
  });
  if (!isSecretFreeDescriptor(record)) {
    throw new Error("Remote repository descriptors must not contain secrets");
  }
  await saveRemoteRepository(record);
  return record;
}

export async function listSavedRepositoriesForEndpoint(
  endpointId: string,
  generation: number,
): Promise<SavedRemoteRepositoryRecord[]> {
  const all = await listSavedRemoteRepositories();
  return all.filter(
    (repo) =>
      repo.endpoint_id === endpointId &&
      repo.endpoint_generation === generation,
  );
}

export async function getSavedRemoteRepository(
  id: string,
): Promise<SavedRemoteRepositoryRecord | null> {
  const all = await listSavedRemoteRepositories();
  return all.find((repo) => repo.id === id) ?? null;
}

export async function invalidateTrustAfterGenerationChange(
  endpointId: string,
  previousGeneration: number,
): Promise<void> {
  const all = await listSavedRemoteRepositories();
  await replaceSavedRemoteRepositories(
    all.map((repo) =>
      repo.endpoint_id === endpointId &&
      repo.endpoint_generation === previousGeneration &&
      repo.last_successful_trust_validation
        ? { ...toPersistedRecord(repo), last_successful_trust_validation: null }
        : repo,
    ),
  );
}

export async function rememberLastOpenedRemoteRepository(
  id: string,
): Promise<void> {
  await setSetting(LAST_OPENED_REMOTE_REPO_ID_KEY, id);
  const raw = await getSetting(REMOTE_RECENT_REPO_IDS_KEY).catch(() => null);
  const existing: string[] = raw ? (JSON.parse(raw) as string[]) : [];
  const next = [id, ...existing.filter((item) => item !== id)].slice(0, 10);
  await setSetting(REMOTE_RECENT_REPO_IDS_KEY, JSON.stringify(next));
}

export async function clearLastOpenedRemoteRepository(): Promise<void> {
  await setSetting(LAST_OPENED_REMOTE_REPO_ID_KEY, "");
  await setSetting(LAST_OPENED_REMOTE_REPO_KEY, "");
}

export async function listRemoteRecentRepositoryIds(): Promise<string[]> {
  const raw = await getSetting(REMOTE_RECENT_REPO_IDS_KEY).catch(() => null);
  return raw ? (JSON.parse(raw) as string[]) : [];
}

/**
 * Restore a saved descriptor only after reconnect, host-key validation,
 * generation match (or explicit transition), and typed inspect succeed.
 * Failed reconnect or trust validation leaves the repository closed.
 */
export async function restoreSavedRemoteRepository(args: {
  descriptor: SavedRemoteRepositoryRecord;
  currentGeneration: number;
  explicitGenerationTransition: boolean;
  reconnect: () => Promise<boolean>;
  validateHostKey: () => Promise<boolean>;
  inspect: (canonicalPath: string) => Promise<RepositoryInspection>;
}): Promise<RestoreResult> {
  const connected = await args.reconnect();
  if (!connected) {
    return { ok: false, reason: "reconnect_failed" };
  }

  const hostKeyValid = await args.validateHostKey();
  if (!hostKeyValid) {
    return { ok: false, reason: "host_key_mismatch" };
  }

  const generationMatches =
    args.descriptor.endpoint_generation === args.currentGeneration;
  if (!generationMatches && !args.explicitGenerationTransition) {
    await invalidateTrustAfterGenerationChange(
      args.descriptor.endpoint_id,
      args.descriptor.endpoint_generation,
    );
    return { ok: false, reason: "generation_mismatch" };
  }

  try {
    const inspection = await args.inspect(
      args.descriptor.canonical_remote_path,
    );
    const generation = args.explicitGenerationTransition
      ? args.currentGeneration
      : args.descriptor.endpoint_generation;
    const descriptor = await upsertSavedRemoteRepository({
      endpoint_id: args.descriptor.endpoint_id,
      endpoint_generation: generation,
      remote_path: args.descriptor.canonical_remote_path,
      display_name: args.descriptor.display_name,
      last_successful_trust_validation: new Date().toISOString(),
    });
    await rememberLastOpenedRemoteRepository(descriptor.id);
    return { ok: true, descriptor, inspection };
  } catch {
    return { ok: false, reason: "inspect_failed" };
  }
}
