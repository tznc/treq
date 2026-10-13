// Managed-VM certificate-authenticated SSH connection state machine
// (prds/remote-development.md, "SSH identity and certificates" > "Managed VM
// certificate flow"). This is the single cohesive service the managed setup
// UI drives: it takes a selected local SSH identity, registers its public
// key, provisions or reuses the one managed instance, polls readiness,
// requests the initial certificate, and activates a managed `SshEndpoint`
// with certificate authentication - then hands back a renewal controller so
// the caller can keep credentials fresh for future connections without
// interrupting current channels.
//
// This module never sees a private key: `readPublicKey` only ever resolves
// the public half (see `core::remote_local_keys::read_local_public_key` on
// the Rust side), and Treq never generates one (PRD Goal 9).

import type {
  ClientKeyResponse,
  InstanceStatusResponse,
  IssueCertificateResponse,
  LocalSshIdentity,
  ManagedInstanceState,
  RegionCode,
  SizePreset,
  SshEndpoint,
} from "./api-types-remote";
import type { CertificateLease } from "./remote-cert-lifecycle";

/** Lifecycle states a managed instance passes through before it is connectable (PRD "State machine"). */
const READY_STATE: ManagedInstanceState = "ready";
const TERMINAL_FAILURE_STATES: ReadonlySet<ManagedInstanceState> = new Set([
  "failed",
  "deleted",
]);

export interface RenewalController {
  stop: () => void;
}

export interface ManagedConnectionDeps {
  /** Resolves the OpenSSH public-key text for a local identity reference. Never returns private key material. */
  readPublicKey: (keyReference: string) => Promise<string>;
  /** Registers (or, on replay, returns) the public key with a stable idempotency key. */
  registerClientKey: (
    publicKey: string,
    comment: string | null,
    idempotencyKey: string,
  ) => Promise<ClientKeyResponse>;
  ensureInstance: (
    region: RegionCode,
    size: SizePreset,
    idempotencyKey: string,
  ) => Promise<unknown>;
  getInstanceStatus: () => Promise<InstanceStatusResponse>;
  issueCertificate: (
    instanceId: string,
    keyId: string,
  ) => Promise<IssueCertificateResponse>;
  /** Activates the returned managed `SshEndpoint` as the connection's active endpoint. */
  activateEndpoint: (endpoint: SshEndpoint) => void;
  /**
   * Swaps in the same endpoint carrying a renewed certificate. Unlike
   * `activateEndpoint` this is not an endpoint replacement: it must not
   * stop renewal or drop open channels. The native pool keeps its current
   * connection and presents the new certificate on its next reconnect.
   */
  updateEndpointCertificate: (endpoint: SshEndpoint) => void;
  /** Starts silent renewal for the issued certificate; swappable for tests. */
  startRenewal: (
    lease: CertificateLease,
    onRenewed?: (lease: CertificateLease) => void,
  ) => RenewalController;
  /**
   * Makes sure the native pool can authenticate to the SSH relay before a
   * relayed endpoint is activated (see `remote-relay-auth.ts`). Called only
   * for endpoints whose transport is `relay`.
   */
  prepareRelay?: () => Promise<void>;
  /** Clears a previously forced hard cutoff after reauthentication succeeds. */
  clearCutoff: (endpointId: string) => Promise<void>;
  /** Asks the control plane to wake a suspended instance. Only `wakeManagedInstance` needs it. */
  wakeInstance?: (
    instanceId: string,
    idempotencyKey: string,
  ) => Promise<unknown>;
  /** Source of per-attempt ids for wake idempotency keys; swappable for tests. */
  newAttemptId?: () => string;
  sleep?: (ms: number) => Promise<void>;
  now?: () => number;
}

/**
 * Picks the local identity used for managed SSH when the user has not chosen
 * one yet: `id_ed25519` if present (the key type the control plane is built
 * around), otherwise the first listed identity. Returns null when `~/.ssh`
 * holds no public key, so the caller can ask the user to create one.
 */
export function pickDefaultKeyReference(
  identities: readonly LocalSshIdentity[],
): string | null {
  const preferred = identities.find(
    (identity) => identity.label === "id_ed25519",
  );
  return (preferred ?? identities[0])?.reference ?? null;
}

/**
 * Idempotency key for one user-initiated wake. The control plane replays
 * the stored result for a key it has seen before (PRD "Idempotency"), so a
 * key derived only from the instance id would make every wake after the
 * first a no-op. Each attempt gets a fresh id; a retry of the same attempt
 * must reuse the key this returned.
 */
export function wakeIdempotencyKey(
  instanceId: string,
  attemptId: string,
): string {
  return `wake-${instanceId}-${attemptId}`;
}

const defaultSleep = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

export interface ManagedConnectionResult {
  endpoint: SshEndpoint;
  key: ClientKeyResponse;
  renewal: RenewalController;
}

/** Stable (non-time-based) idempotency key: registering the same local identity twice must not register two keys. */
function keyRegistrationIdempotencyKey(keyReference: string): string {
  return `register-client-key:${keyReference}`;
}

/** Stable idempotency key for provisioning: repeating the same request must not create a second instance (PRD "Idempotency"). */
function provisionIdempotencyKey(
  keyReference: string,
  region: RegionCode,
  size: SizePreset,
): string {
  return `provision:${keyReference}:${region}:${size}`;
}

async function registerSelectedKey(
  deps: ManagedConnectionDeps,
  keyReference: string,
): Promise<ClientKeyResponse> {
  const publicKey = await deps.readPublicKey(keyReference);
  return deps.registerClientKey(
    publicKey,
    "treq-managed-vm",
    keyRegistrationIdempotencyKey(keyReference),
  );
}

const READINESS_POLL_INTERVAL_MS = 2_000;
const READINESS_POLL_TIMEOUT_MS = 10 * 60_000;

/** Polls structured lifecycle/readiness state until the instance is connectable (`ready`), or fails fast on a terminal failure state or timeout. */
async function waitForInstanceReady(
  deps: ManagedConnectionDeps,
  options: { intervalMs?: number; timeoutMs?: number } = {},
): Promise<InstanceStatusResponse> {
  const sleep = deps.sleep ?? defaultSleep;
  const now = deps.now ?? Date.now;
  const intervalMs = options.intervalMs ?? READINESS_POLL_INTERVAL_MS;
  const timeoutMs = options.timeoutMs ?? READINESS_POLL_TIMEOUT_MS;
  const deadline = now() + timeoutMs;

  for (;;) {
    // Sequential polling is the point here - each iteration depends on the
    // previous one's result (has the instance become ready yet?), so this
    // cannot be parallelized.
    // eslint-disable-next-line no-await-in-loop
    const status = await deps.getInstanceStatus();
    const state = status.instance?.status;
    if (state === READY_STATE) return status;
    if (state && TERMINAL_FAILURE_STATES.has(state)) {
      throw new Error(
        `Managed instance provisioning failed (status: ${state}).`,
      );
    }
    if (now() >= deadline) {
      throw new Error(
        `Timed out waiting for the managed instance to become ready (last status: ${state ?? "unknown"}).`,
      );
    }
    // eslint-disable-next-line no-await-in-loop
    await sleep(intervalMs);
  }
}

/**
 * Builds the endpoint the native transport connects with. The control plane
 * owns where to connect and what to trust (host, port, user, pinned host
 * keys, generation); this device owns how to authenticate. The server's
 * `authentication` block names its own key id, which means nothing to the
 * local transport, so it is replaced with the local key reference the user
 * registered and the certificate just issued for that key.
 */
export function composeManagedEndpoint(
  serverEndpoint: SshEndpoint,
  keyReference: string,
  certificate: string,
): SshEndpoint {
  return {
    ...serverEndpoint,
    authentication: {
      type: "certificate",
      key_reference: keyReference,
      certificate,
    },
  };
}

async function issueAndActivate(
  deps: ManagedConnectionDeps,
  instanceId: string,
  { key, keyReference }: { key: ClientKeyResponse; keyReference: string },
): Promise<{ endpoint: SshEndpoint; renewal: RenewalController }> {
  const response = await deps.issueCertificate(instanceId, key.id);
  const endpoint = composeManagedEndpoint(
    response.endpoint,
    keyReference,
    response.certificate,
  );
  if (endpoint.transport?.type === "relay") await deps.prepareRelay?.();
  deps.activateEndpoint(endpoint);

  const now = deps.now ?? Date.now;
  const issuedAt = now();
  const expiresAt = Date.parse(response.expires_at);
  const renewal = deps.startRenewal(
    {
      instanceId,
      keyId: key.id,
      endpointId: endpoint.id,
      serial: response.serial,
      certificate: response.certificate,
      issuedAt,
      expiresAt,
    },
    (renewed) =>
      deps.updateEndpointCertificate(
        composeManagedEndpoint(
          response.endpoint,
          keyReference,
          renewed.certificate,
        ),
      ),
  );

  return { endpoint, renewal };
}

export interface ConnectManagedInstanceOptions {
  region: RegionCode;
  size: SizePreset;
  keyReference: string;
}

/**
 * Full identity -> registration -> certificate -> endpoint sequence (PRD
 * "Managed VM certificate flow"): resolves the selected local identity's
 * public key, registers it, provisions or reuses the one managed instance,
 * polls readiness, requests the initial certificate, activates the endpoint,
 * and starts silent renewal.
 */
export async function connectManagedInstance(
  deps: ManagedConnectionDeps,
  options: ConnectManagedInstanceOptions,
): Promise<ManagedConnectionResult> {
  const key = await registerSelectedKey(deps, options.keyReference);

  await deps.ensureInstance(
    options.region,
    options.size,
    provisionIdempotencyKey(options.keyReference, options.region, options.size),
  );

  const status = await waitForInstanceReady(deps);
  const instanceId = status.instance?.instance_id;
  if (!instanceId) {
    throw new Error("Managed instance became ready without an instance id.");
  }

  const { endpoint, renewal } = await issueAndActivate(deps, instanceId, {
    key,
    keyReference: options.keyReference,
  });
  return { endpoint, key, renewal };
}

export interface ConnectExistingReadyInstanceOptions {
  status: InstanceStatusResponse;
  keyReference: string;
}

/**
 * Connect action for an existing ready managed instance: skips provisioning
 * and readiness polling (the caller already has a fresh `ready` status) but
 * still runs key registration, certificate issuance, endpoint activation,
 * and renewal - so "Connect" is not a shortcut around certificate auth.
 */
export async function connectExistingReadyInstance(
  deps: ManagedConnectionDeps,
  options: ConnectExistingReadyInstanceOptions,
): Promise<ManagedConnectionResult> {
  const { instance } = options.status;
  if (!instance || instance.status !== READY_STATE) {
    throw new Error(
      `Managed instance is not ready (status: ${instance?.status ?? "unprovisioned"}).`,
    );
  }
  const key = await registerSelectedKey(deps, options.keyReference);
  const { endpoint, renewal } = await issueAndActivate(
    deps,
    instance.instance_id,
    { key, keyReference: options.keyReference },
  );
  return { endpoint, key, renewal };
}

export interface WakeManagedInstanceOptions {
  instanceId: string;
  keyReference: string;
}

/**
 * Wake/reconnect for a suspended managed instance, in the order the PRD's
 * wake flow requires: wake, poll readiness, get a fresh certificate, then
 * activate the (possibly new-generation) endpoint. Each call is a new
 * attempt with its own idempotency key.
 */
export async function wakeManagedInstance(
  deps: ManagedConnectionDeps,
  options: WakeManagedInstanceOptions,
): Promise<ManagedConnectionResult> {
  if (!deps.wakeInstance) {
    throw new Error("wakeManagedInstance requires a wakeInstance dependency.");
  }
  const attemptId = (deps.newAttemptId ?? (() => crypto.randomUUID()))();
  await deps.wakeInstance(
    options.instanceId,
    wakeIdempotencyKey(options.instanceId, attemptId),
  );
  const status = await waitForInstanceReady(deps);
  return connectExistingReadyInstance(deps, {
    status,
    keyReference: options.keyReference,
  });
}

export interface ReauthenticateManagedInstanceOptions {
  instanceId: string;
  endpointId: string;
  keyReference: string;
}

/**
 * Reauthentication after a hard cutoff (PRD "The user regains access only by
 * reauthenticating and obtaining a new certificate"). Cutoff is cleared only
 * after certificate issuance actually succeeds - a failed reauthentication
 * attempt must leave the endpoint blocked, not silently restore access.
 */
export async function reauthenticateManagedInstance(
  deps: ManagedConnectionDeps,
  options: ReauthenticateManagedInstanceOptions,
): Promise<ManagedConnectionResult> {
  const key = await registerSelectedKey(deps, options.keyReference);
  const { endpoint, renewal } = await issueAndActivate(
    deps,
    options.instanceId,
    { key, keyReference: options.keyReference },
  );
  await deps.clearCutoff(options.endpointId);
  return { endpoint, key, renewal };
}
