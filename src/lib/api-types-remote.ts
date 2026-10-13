export interface SshHost {
  alias: string;
}

export interface LocalSshIdentity {
  reference: string;
  label: string;
  fingerprint_sha256: string;
  algorithm: string;
}

/**
 * Explicit connection fields resolved from an explicitly-selected
 * `~/.ssh/config` alias (Host/HostName/User/Port/IdentityFile, including
 * `Include`). Autocomplete data only: resolving an alias performs no network
 * I/O and never grants trust on its own.
 */
export interface ResolvedSshAlias {
  alias: string;
  hostname: string;
  port: number;
  username: string | null;
  identity_file: string | null;
}

export interface RemoteRepoProbe {
  host: string;
  path: string;
  exists: boolean;
  is_repo: boolean;
  needs_clone: boolean;
}

/**
 * Cheap, pollable change marker for a workspace's JJ operation log (PRD
 * "Change propagation across concurrent clients"). A client stores the
 * last `operation_id` it observed for a workspace and compares it against
 * the latest value; a mismatch means VM-side repository state moved for a
 * reason other than the client's own in-flight mutation, and the client
 * should refresh rather than merge or reconcile anything.
 */
export interface WorkspaceChangeMarker {
  operation_id: string;
}

/**
 * Machine-wide usage from `MachineUsage` (mirrors `MachineUsageReport` in
 * `src-tauri/src/core/remote.rs`). `disk_used_bytes` is the same figure the
 * disk quota is enforced against.
 */
export interface MachineUsageReport {
  repository_count: number;
  workspace_count: number;
  disk_used_bytes: number;
}

export type RepositoryLocation =
  | { type: "local"; path: string }
  | { type: "ssh"; host: string; path: string };

interface RepositoryDescriptor {
  id: string;
  location: RepositoryLocation;
  display_name: string;
}

export interface RepositoryInspection {
  root: string;
  repository_type: string;
  current_branch: string | null;
  default_branch: string;
  current_change_id: string;
  current_commit_id: string;
  descriptor: RepositoryDescriptor;
}

export interface RemoteRepository {
  host: string;
  path: string;
  display_name: string;
  repo_uri: string;
  inspection: RepositoryInspection;
}

// -- Managed compute provider contracts (Phase 1: neutral domain model) -----
//
// These types mirror src-tauri/src/core/remote_provider.rs and
// src-tauri/src/core/remote_control_plane.rs. Nothing here talks to a
// provider or Supabase yet; they exist so a later phase's UI and control-plane
// client agree on one wire shape. No vendor SDK type or status string may
// leak past this boundary.

export type ManagedInstanceState =
  | "unprovisioned"
  | "provisioning"
  | "bootstrapping"
  | "installing_access"
  | "verifying"
  | "ready"
  | "suspended"
  | "waking"
  | "reprovisioning"
  | "degraded"
  | "failed"
  | "deleting"
  | "deleted";

export type SizePreset = "small" | "medium" | "large";

export type RegionCode = "us_east" | "us_west" | "eu_west" | "ap_southeast";

type ProviderKind = "fly_sprites";

export interface ManagedInstanceRecord {
  instance_id: string;
  owner_user_id: string;
  provider_kind: ProviderKind;
  provider_resource_id: string | null;
  region: RegionCode;
  size_preset: SizePreset;
  status: ManagedInstanceState;
  generation: number;
  endpoint_id: string | null;
  image_manifest_version: number;
  created_at: string;
  ready_at: string | null;
  // Enforced base resource allocation (PRD "Resource quotas"). Fixed values
  // in this delivery; not yet purchasable/adjustable.
  disk_quota_gb: number;
  vcpu_quota: number;
  ram_quota_gb: number;
}

export interface TrustedHostKey {
  algorithm: string;
  fingerprint_sha256: string;
  comment: string | null;
}

/**
 * `key_reference` always names key material on this device: a local key
 * path (the `.pub` path from `listLocalSshIdentities` works). It is never a
 * control-plane key id. `certificate` carries the issued OpenSSH certificate text inline; the
 * transport falls back to `<key>-cert.pub` on disk only when it is absent.
 */
export type SshAuthentication =
  | { type: "certificate"; key_reference: string; certificate?: string }
  | { type: "public_key"; key_reference: string };

type SshEndpointSource =
  | { type: "managed"; provider: string; generation: number }
  | { type: "user_managed" }
  | { type: "explicit_alias"; alias: string };

export interface SshEndpoint {
  id: string;
  instance_id: string | null;
  source: SshEndpointSource;
  hostname: string;
  port: number;
  username: string;
  host_keys: TrustedHostKey[];
  authentication: SshAuthentication;
  /** Absent means direct TCP (mirrors `SshTransport` in Rust). */
  transport?: SshTransport;
}

/**
 * How the client reaches sshd. Managed Sprites have no raw TCP ingress, so
 * their endpoints use `relay`: a WebSocket to the `remote-ssh-relay` Edge
 * Function. SSH stays end to end either way.
 */
type SshTransport = { type: "direct" } | { type: "relay"; url: string };

/**
 * Allow-listed remote agent identifiers (mirrors `RemoteAgentId` in
 * `src-tauri/src/core/remote_pty.rs`). Closed set - never an arbitrary
 * string - so the backend can never be asked to exec an unknown binary.
 */
export type RemoteAgentId = "claude" | "codex" | "cursor_agent";

/**
 * Typed launch behavior for a new remote PTY session (mirrors
 * `PtyLaunchSpec` in `src-tauri/src/core/remote_pty.rs`). Never an arbitrary
 * shell command string - this is the type-level half of the backend's
 * shell-injection guard: only a closed set of launch shapes exists, and
 * every dynamic field within them is individually shell-quoted server-side.
 */
export type PtyLaunchSpec =
  | { type: "shell" }
  | { type: "agent"; agent: RemoteAgentId; args: string[] };

type OperationStatus = "pending" | "in_progress" | "succeeded" | "failed";

export interface OperationResponse {
  operation_id: string;
  status: OperationStatus;
}

export interface ProvisionInstanceRequest {
  idempotency_key: string;
}

export interface WakeInstanceRequest {
  instance_id: string;
  idempotency_key: string;
}

export interface ReprovisionInstanceRequest {
  instance_id: string;
  idempotency_key: string;
}

export interface DeleteInstanceRequest {
  instance_id: string;
  idempotency_key: string;
}

export interface InstanceStatusResponse {
  instance: ManagedInstanceRecord | null;
  endpoint: SshEndpoint | null;
}

export interface RegisterClientKeyRequest {
  public_key: string;
  comment: string | null;
  idempotency_key: string;
}

export interface ClientKeyResponse {
  id: string;
  algorithm: string;
  fingerprint_sha256: string;
  comment: string | null;
  created_at: string;
  revoked_at: string | null;
}

/**
 * `key` is set when this call registered (or matched) a single key; `keys`
 * is set instead when the call replayed an already-completed idempotent
 * registration. Callers that just need "the key I registered" should use
 * `registerClientKey` in `remote-control-plane.ts`, which normalizes this.
 */
export interface RegisterClientKeyResponse {
  operation_id: string;
  status: OperationStatus;
  key?: ClientKeyResponse;
  keys?: ClientKeyResponse[];
}

export interface RevokeClientKeyRequest {
  key_id: string;
  idempotency_key: string;
}

export interface IssueCertificateRequest {
  instance_id: string;
  key_id: string;
  /**
   * Set by the silent-renewal loop (`src/lib/remote-cert-lifecycle.ts`) so
   * the control plane can label the resulting audit event as a renewal
   * rather than first issuance. The signing logic is identical either way.
   */
  renewal?: boolean;
}

export interface IssueCertificateResponse {
  certificate: string;
  serial: string;
  expires_at: string;
  endpoint: SshEndpoint;
}
