// Cloudflare Sandboxes provider adapter. Cloudflare's Sandbox SDK
// (developers.cloudflare.com/sandbox) runs inside a Worker as a Durable
// Object bound to a Container; it has no account-level REST API. So the
// owner deploys treq's bridge Worker (cloudflare/sandbox-bridge) to their
// account, and this adapter calls its small REST API with a bearer token.
// Containers bill per 10 ms while awake; a sleeping container's disk is
// discarded (developers.cloudflare.com/containers/platform-details/architecture).
// Vendor states and response shapes must never leave this module.

import type {
  CreateInstanceParams,
  DeleteInstanceOptions,
  GatewayEndpoint,
  MachineExecResult,
  ManagedInstanceState,
  ProviderCapabilities,
  ProviderCredential,
  ProviderInstance,
  ReplaceInstanceParams,
  SandboxProvider,
  ServiceSpec,
} from "./provider.ts";
import { DEFAULT_PROVIDER_AUTOSTOP_MIN, GATEWAY_PORT, ProviderError, providerAutoStopMinutes } from "./provider.ts";
import {
  assertExecArgs,
  ensurePidfileService,
  expectOk,
  labelValue,
  providerErrorFrom,
  providerFetch,
  providerInstance,
} from "./provider-http.ts";

export const CLOUDFLARE_CAPABILITIES: Readonly<ProviderCapabilities> = Object.freeze({
  // A container that sleeps restarts on a fresh disk.
  stopKeepsDisk: false,
  execApi: true,
  // No service API: ensureService runs a pidfile supervisor through exec.
  serviceSupervisor: false,
  // The bridge proxies a WebSocket to the gateway port (Sandbox SDK wsConnect).
  tcpProxy: true,
  // Billed per 10 ms; the ledger counts whole seconds.
  billingGranularitySec: 1,
});

// Treq machine types are the tier ids. The bridge maps each to a container
// class: s → standard-3 (2 vCPU / 8 GiB / 16 GB), m → standard-4
// (4 vCPU / 12 GiB / 20 GB). Cloudflare's largest instance is 4 vCPU, so
// there is no l or xl.
const CLOUDFLARE_TYPES = new Set(["s", "m"]);

export function normalizeCloudflareState(vendorState: string): ManagedInstanceState {
  switch (vendorState) {
    case "running":
      return "ready";
    case "starting":
      return "waking";
    case "sleeping":
    case "stopped":
      return "suspended";
    default:
      return "degraded";
  }
}

interface BridgeSandbox {
  id: string;
  state?: string;
}

function cloudflareError(response: Response): Promise<ProviderError> {
  return providerErrorFrom(response, (status, _text, detail) => {
    if (status === 504) return new ProviderError("timeout", detail);
  });
}

export interface CloudflareConfig {
  /** The owner's bridge Worker, e.g. https://treq-sandbox.acme.workers.dev */
  bridgeUrl: string;
  token: string;
  /** sleepAfter, the backstop behind our idle controller. */
  autoStopMinutes?: number;
}

/**
 * A BYOS key is the bridge URL with the bridge token as its fragment:
 * `https://treq-sandbox.acme.workers.dev#<token>`. One string, so it fits
 * the single key field every other provider uses.
 */
export function parseCloudflareCredential(key: string): { bridgeUrl: string; token: string } {
  const hash = key.indexOf("#");
  const token = hash >= 0 ? key.slice(hash + 1).trim() : "";
  let url: URL | null = null;
  try {
    url = new URL(hash >= 0 ? key.slice(0, hash) : key);
  } catch {
    url = null;
  }
  if (!url || url.protocol !== "https:" || !token) {
    throw new ProviderError("invalid_request", "Cloudflare key must be https://<bridge-worker>#<token>");
  }
  return { bridgeUrl: url.origin + url.pathname.replace(/\/+$/, ""), token };
}

/// Reads the bridge from Edge Function secrets, or from a BYOS credential.
/// Never logged; never returned to a client.
export function cloudflareConfigFromEnv(
  env: (key: string) => string | undefined = (key) => Deno.env.get(key),
  credential?: ProviderCredential,
): CloudflareConfig {
  const autoStopMinutes = providerAutoStopMinutes(env);
  if (credential) return { ...parseCloudflareCredential(credential.apiKey), autoStopMinutes };
  const bridgeUrl = env("TREQ_CLOUDFLARE_BRIDGE_URL");
  const token = env("TREQ_CLOUDFLARE_BRIDGE_TOKEN");
  if (!bridgeUrl || !token) {
    throw new ProviderError("invalid_request", "TREQ_CLOUDFLARE_BRIDGE_URL and TREQ_CLOUDFLARE_BRIDGE_TOKEN must be set");
  }
  return { bridgeUrl, token, autoStopMinutes };
}

export interface CloudflareDeps {
  fetch?: typeof fetch;
}

export class CloudflareProvider implements SandboxProvider {
  readonly kind = "cloudflare";
  private readonly fetchImpl: typeof fetch;

  constructor(private readonly config: CloudflareConfig, deps: CloudflareDeps = {}) {
    this.fetchImpl = deps.fetch ?? fetch;
  }

  capabilities(): ProviderCapabilities {
    return { ...CLOUDFLARE_CAPABILITIES };
  }

  private get sleepAfterSec(): number {
    return (this.config.autoStopMinutes ?? DEFAULT_PROVIDER_AUTOSTOP_MIN) * 60;
  }

  private url(path: string): string {
    return `${this.config.bridgeUrl.replace(/\/+$/, "")}${path}`;
  }

  private sandboxPath(id: string, action = ""): string {
    return `/v1/sandboxes/${encodeURIComponent(id)}${action}`;
  }

  private api(path: string, method = "GET", body?: unknown, timeoutMs?: number): Promise<Response> {
    return providerFetch(this.fetchImpl, "Cloudflare bridge", this.url(path), {
      method,
      headers: { Authorization: `Bearer ${this.config.token}`, "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    }, timeoutMs);
  }

  private async sandboxFrom(response: Response): Promise<ProviderInstance> {
    if (!response.ok) throw await cloudflareError(response);
    const body = await response.json() as BridgeSandbox;
    if (!body?.id) throw new ProviderError("other", "Cloudflare bridge response had no sandbox");
    return providerInstance(body.id, normalizeCloudflareState(body.state ?? "running"));
  }

  // The sandbox id is the Durable Object name, derived from the placement,
  // so PUT is idempotent and a migration target never collides with its source.
  async createInstance(params: CreateInstanceParams): Promise<ProviderInstance> {
    const type = params.machineType ?? "s";
    if (!CLOUDFLARE_TYPES.has(type)) throw new ProviderError("invalid_request", `unknown Cloudflare machine type: ${type}`);
    const placement = labelValue(params.placementId ?? params.idempotencyKey);
    const response = await this.api(this.sandboxPath(`treq-${placement}`), "PUT", {
      instanceType: type,
      sleepAfterSec: this.sleepAfterSec,
      labels: {
        treq_owner: labelValue(params.ownerUserId),
        treq_placement: placement,
        treq_manifest: String(params.manifestVersion),
      },
    }, 60_000);
    return await this.sandboxFrom(response);
  }

  async validateCredentials(): Promise<void> {
    await expectOk(await this.api("/v1/health"), cloudflareError);
  }

  async getInstance(providerId: string): Promise<ProviderInstance> {
    return await this.sandboxFrom(await this.api(this.sandboxPath(providerId)));
  }

  async wakeInstance(providerId: string): Promise<void> {
    await expectOk(await this.api(this.sandboxPath(providerId, "/start"), "POST", undefined, 60_000), cloudflareError);
  }

  async stopInstance(providerId: string): Promise<void> {
    await expectOk(await this.api(this.sandboxPath(providerId, "/stop"), "POST", undefined, 60_000), cloudflareError, [404]);
  }

  async keepAlive(providerId: string): Promise<void> {
    const response = await this.api(this.sandboxPath(providerId, "/keepalive"), "POST", {
      sleepAfterSec: this.sleepAfterSec,
    });
    await expectOk(response, cloudflareError);
  }

  async replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance> {
    return await this.getInstance(params.providerResourceId);
  }

  async deleteInstance(providerId: string, _opts?: DeleteInstanceOptions): Promise<void> {
    await expectOk(await this.api(this.sandboxPath(providerId), "DELETE"), cloudflareError, [404]);
  }

  async execOnMachine(
    providerId: string,
    command: string[],
    timeoutSeconds = 20,
    environment: Readonly<Record<string, string>> = {},
  ): Promise<MachineExecResult> {
    assertExecArgs(command, environment);
    const timeoutMs = Math.max(Math.ceil(timeoutSeconds), 1) * 1_000;
    const response = await this.api(
      this.sandboxPath(providerId, "/exec"),
      "POST",
      { argv: command, env: { ...environment }, timeoutMs },
      timeoutMs + 10_000,
    );
    if (!response.ok) throw await cloudflareError(response);
    const body = await response.json() as Partial<MachineExecResult>;
    if (typeof body?.exitCode !== "number") throw new ProviderError("other", "Cloudflare bridge exec had no exit code");
    return { exitCode: body.exitCode, stdout: body.stdout ?? "", stderr: body.stderr ?? "" };
  }

  async ensureService(providerId: string, name: string, spec: ServiceSpec): Promise<void> {
    await ensurePidfileService(
      (command, timeout) => this.execOnMachine(providerId, command, timeout),
      name,
      spec,
      { vendor: "Cloudflare", dir: "/tmp/treq" },
    );
  }

  // The bridge upgrades `/v1/sandboxes/{id}/ports/{port}` and forwards the
  // WebSocket to the container port with the Sandbox SDK's wsConnect.
  // UNVERIFIED: binary frames end to end and reaching a 127.0.0.1 listener.
  async gatewayEndpoint(providerId: string): Promise<GatewayEndpoint> {
    await this.wakeInstance(providerId);
    const base = new URL(this.url(this.sandboxPath(providerId, `/ports/${GATEWAY_PORT}`)));
    base.protocol = "wss:";
    return { url: base.toString(), headers: { Authorization: `Bearer ${this.config.token}` } };
  }
}
