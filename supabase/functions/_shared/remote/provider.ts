// The provider-neutral contract every sandbox provider adapter implements
// (docs/design/sandbox-provider-adapter.md, D1). Vendor status strings and
// vendor SDK types must never leave an adapter module.

import type { RegionCode, SizePreset } from "./catalog.ts";

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

export interface ProviderInstance {
  providerResourceId: string;
  state: ManagedInstanceState;
  region: RegionCode;
  sizePreset: SizePreset;
  address: string | null;
  /** When the provider says the machine last changed state, if its API reports it (ISO 8601). */
  stateChangedAt?: string | null;
}

export class ProviderError extends Error {
  constructor(
    public readonly kind:
      | "not_found"
      | "already_exists"
      | "quota_exceeded"
      | "invalid_request"
      | "unavailable"
      | "timeout"
      // 401/403 for the key itself: a BYOS key that was revoked or is wrong.
      // Never retried (byos.ts marks the credential invalid).
      | "unauthorized"
      | "other",
    message: string,
  ) {
    super(message);
  }
}

export interface CreateInstanceParams {
  ownerUserId: string;
  region: RegionCode;
  sizePreset: SizePreset;
  manifestVersion: number;
  idempotencyKey: string;
  // The provider's own machine type (`provider_machine_types.vendor_type`).
  // Providers with one machine shape ignore it.
  machineType?: string;
  // The placement this machine backs, so a provider can name or tag it.
  placementId?: string;
}

export interface DeleteInstanceOptions {
  placementId?: string | null;
}

export interface ReplaceInstanceParams {
  providerResourceId: string;
  region: RegionCode;
  sizePreset: SizePreset;
  manifestVersion: number;
  idempotencyKey: string;
}

export interface MachineExecResult {
  exitCode: number;
  stdout: string;
  stderr: string;
}

/**
 * Message safe to return to a client. Provider messages can carry upstream
 * response bodies or transport details, so only the fixed messages for
 * well-known kinds pass through; callers log `err.message` instead.
 */
export function providerClientMessage(err: ProviderError): string {
  switch (err.kind) {
    case "not_found":
    case "already_exists":
    case "quota_exceeded":
      return err.message;
    case "invalid_request":
      return "provider rejected the request";
    case "timeout":
      return "provider request timed out";
    case "unavailable":
      return "provider unavailable";
    case "unauthorized":
      return "provider credential is invalid";
    default:
      return "provider request failed";
  }
}

export interface ManagedComputeProvider {
  createInstance(params: CreateInstanceParams): Promise<ProviderInstance>;
  getInstance(providerId: string): Promise<ProviderInstance>;
  wakeInstance(providerId: string): Promise<void>;
  // Stops the machine and keeps its disk. Used when the owner's Pro ends
  // (`lapse-sweep.ts`).
  stopInstance(providerId: string): Promise<void>;
  replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance>;
  // `placementId` lets a provider find resources named after the placement
  // (Fly's volume) after the machine itself is already gone.
  deleteInstance(providerId: string, opts?: DeleteInstanceOptions): Promise<void>;
  // Runs a command through the documented non-TTY HTTP exec endpoint. The
  // provider token remains server-side.
  execOnMachine(
    providerId: string,
    command: string[],
    timeoutSeconds?: number,
    environment?: Readonly<Record<string, string>>,
  ): Promise<MachineExecResult>;
  // Creates or updates a named service and (re)starts it so a changed
  // definition takes effect. Services run at boot and come back after a
  // cold wake, which is what keeps sshd available without Treq restarting
  // it on every connection.
  ensureService(
    providerId: string,
    name: string,
    spec: ServiceSpec,
  ): Promise<void>;
}

// What a provider can do, so callers branch on behaviour rather than on the
// provider kind (docs/design/sandbox-provider-adapter.md, D1).
export interface ProviderCapabilities {
  // Stopping keeps the disk, so a later start resumes the same filesystem.
  stopKeepsDisk: boolean;
  execApi: boolean;
  serviceSupervisor: boolean;
  // An authenticated, server-side-token TCP path to sshd for the relay.
  tcpProxy: boolean;
  billingGranularitySec: number;
}

export type ProviderKind = "boat" | "local" | "daytona" | "fly_machines" | "e2b";

export interface SandboxProvider extends ManagedComputeProvider {
  readonly kind: ProviderKind;
  capabilities(): ProviderCapabilities;
  // Tells the provider's own auto-stop that the machine is in use. The
  // idle controller (idle-controller.ts) calls it whenever a probe finds
  // activity. Provider auto-stop is only a backstop set longer than our idle
  // timeout (TREQ_PROVIDER_AUTOSTOP_MIN): Boat's is a deadline that must be
  // pushed out, Daytona's an inactivity timer. Absent on providers with no
  // auto-stop (local).
  keepAlive?(providerId: string): Promise<void>;
  // The WebSocket endpoint that speaks the gateway proxy protocol
  // (gateway-proxy.ts) for this machine: the provider's ingress to
  // treq-gateway. Headers carry the provider's own auth; they never leave
  // the server (design doc, D4). `openTcp` in provider-tcp.ts dials it.
  gatewayEndpoint(providerId: string): Promise<GatewayEndpoint>;
  // Where treq-gateway listens inside the machine. Absent means loopback
  // (`GATEWAY_LISTEN`, 127.0.0.1:8022): Boat and Daytona ingress reaches
  // loopback. Fly Proxy forwards to the machine's private interface, so Fly
  // sets 0.0.0.0:8022; the gateway HMAC is still the only auth.
  readonly gatewayListenAddress?: string;
  // One cheap read-only call that proves the API key works. Throws
  // ProviderError("unauthorized") for a bad key. Providers without it take
  // no BYOS key (byos.ts).
  validateCredentials?(): Promise<void>;
}

/** A user's own provider API key (BYOS). Loaded server-side only; never logged. */
export interface ProviderCredential {
  apiKey: string;
}

export interface GatewayEndpoint {
  url: string;
  headers: Record<string, string>;
}

/** treq-gateway's port inside a machine. */
export const GATEWAY_PORT = 8022;

// Provider auto-stop backstop, in minutes: longer than the idle controller's
// TREQ_IDLE_TIMEOUT_MIN so it only fires when our controller has failed.
export const DEFAULT_PROVIDER_AUTOSTOP_MIN = 60;

export function providerAutoStopMinutes(env: (key: string) => string | undefined): number {
  const value = Number(env("TREQ_PROVIDER_AUTOSTOP_MIN"));
  return Number.isInteger(value) && value > 0 ? value : DEFAULT_PROVIDER_AUTOSTOP_MIN;
}

export interface ServiceSpec {
  cmd: string;
  args: string[];
}

