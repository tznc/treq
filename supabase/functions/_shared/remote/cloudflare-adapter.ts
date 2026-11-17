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
import { BRIDGE_SCRIPT } from "./cloudflare-bridge.generated.ts";
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
  /** The owner's bridge Worker, e.g. https://treq-sandbox.acme.workers.dev. Absent until provisioned. */
  bridgeUrl?: string;
  token?: string;
  /** The owner's account and API token, which deploy the bridge (BYOS). */
  accountId?: string;
  apiToken?: string;
  /** sleepAfter, the backstop behind our idle controller. */
  autoStopMinutes?: number;
}

type ParsedCredential = Omit<CloudflareConfig, "autoStopMinutes">;

function parseBridge(key: string): { bridgeUrl: string; token: string } {
  const hash = key.indexOf("#");
  const token = hash >= 0 ? key.slice(hash + 1).trim() : "";
  let url: URL | null = null;
  try {
    url = new URL(hash >= 0 ? key.slice(0, hash) : key);
  } catch {
    url = null;
  }
  if (!url || url.protocol !== "https:" || !token) {
    throw new ProviderError("invalid_request", "Cloudflare bridge must be https://<worker>#<token>");
  }
  return { bridgeUrl: url.origin + url.pathname.replace(/\/+$/, ""), token };
}

/**
 * A BYOS key, as pasted: `<account id>:<api token>`. Saving it deploys the
 * bridge (provisionBridge), and the stored key becomes
 * `<account id>:<api token>|https://<worker>#<bridge token>`. A bare
 * `https://<worker>#<bridge token>` is a bridge the owner deployed by hand.
 */
export function parseCloudflareCredential(key: string): ParsedCredential {
  if (key.startsWith("https://")) return parseBridge(key);
  const bar = key.indexOf("|");
  const account = bar >= 0 ? key.slice(0, bar) : key;
  const m = account.match(/^([0-9A-Za-z]{1,64}):([A-Za-z0-9_-]{1,512})$/);
  if (!m) throw new ProviderError("invalid_request", "Cloudflare key must be <account id>:<api token>");
  const base = { accountId: m[1], apiToken: m[2] };
  return bar >= 0 ? { ...base, ...parseBridge(key.slice(bar + 1)) } : base;
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

// ── Provisioning the bridge (BYOS) ───────────────────────────────────────

/** The Sandbox SDK's base image; its version matches the bundled SDK. */
export const BRIDGE_IMAGE = "docker.io/cloudflare/sandbox:0.4.18";
const SCRIPT_NAME = "treq-sandbox";
const KV_TITLE = "treq-sandbox-types";
const SIZES = [
  { tier: "s", className: "SandboxS", binding: "SANDBOX_S", instanceType: "standard-3" },
  { tier: "m", className: "SandboxM", binding: "SANDBOX_M", instanceType: "standard-4" },
] as const;

export interface ProvisionDeps {
  fetch?: typeof fetch;
  apiBase?: string;
  randomToken?: () => string;
}

function randomHex(): string {
  return [...crypto.getRandomValues(new Uint8Array(32))].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Deploys treq's bridge Worker into the owner's account with their API token
 * and returns the key to store. Safe to re-run: it reuses the KV namespace
 * and container apps, and only the first upload carries the Durable Object
 * migration. The token needs Workers Scripts, Workers KV Storage and
 * Containers edit permissions.
 * UNVERIFIED against a live account: the containers applications API shape
 * (as wrangler uses it) and Docker Hub pulls of the base image.
 */
export async function provisionBridge(
  account: { accountId: string; apiToken: string },
  deps: ProvisionDeps = {},
): Promise<string> {
  const fetchImpl = deps.fetch ?? fetch;
  const base = `${(deps.apiBase ?? "https://api.cloudflare.com/client/v4").replace(/\/+$/, "")}/accounts/${
    encodeURIComponent(account.accountId)
  }`;
  const call = async <T>(path: string, method = "GET", body?: unknown, okStatuses: number[] = []): Promise<T | null> => {
    const isForm = body instanceof FormData;
    const response = await providerFetch(fetchImpl, "Cloudflare", `${base}${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${account.apiToken}`,
        ...(isForm || body === undefined ? {} : { "Content-Type": "application/json" }),
      },
      body: isForm ? body : body === undefined ? undefined : JSON.stringify(body),
    }, 60_000);
    if (okStatuses.includes(response.status)) {
      await response.body?.cancel();
      return null;
    }
    if (!response.ok) throw await cloudflareError(response);
    return ((await response.json()) as { result: T }).result;
  };

  const need = <T>(value: T | null, what: string): T => {
    if (value === null) throw new ProviderError("other", `Cloudflare returned no ${what}`);
    return value;
  };
  const { subdomain } = need(await call<{ subdomain: string }>("/workers/subdomain"), "subdomain");
  if (!subdomain) throw new ProviderError("invalid_request", "Cloudflare account has no workers.dev subdomain");

  const namespaces = (await call<Array<{ id: string; title: string }>>("/storage/kv/namespaces?per_page=100")) ?? [];
  const kvId = namespaces.find((n) => n.title === KV_TITLE)?.id ??
    need(await call<{ id: string }>("/storage/kv/namespaces", "POST", { title: KV_TITLE }), "KV namespace").id;

  const exists = (await call<unknown>(`/workers/scripts/${SCRIPT_NAME}/settings`, "GET", undefined, [404])) !== null;
  const bridgeToken = (deps.randomToken ?? randomHex)();
  const metadata = {
    main_module: "index.js",
    compatibility_date: "2026-10-01",
    compatibility_flags: ["nodejs_compat"],
    bindings: [
      ...SIZES.map((s) => ({ type: "durable_object_namespace", name: s.binding, class_name: s.className })),
      { type: "kv_namespace", name: "TYPES", namespace_id: kvId },
      { type: "secret_text", name: "BRIDGE_TOKEN", text: bridgeToken },
    ],
    containers: SIZES.map((s) => ({ class_name: s.className })),
    ...(exists ? {} : { migrations: { new_tag: "v1", new_sqlite_classes: SIZES.map((s) => s.className) } }),
  };
  const form = new FormData();
  form.set("metadata", new Blob([JSON.stringify(metadata)], { type: "application/json" }));
  form.set("index.js", new Blob([BRIDGE_SCRIPT], { type: "application/javascript+module" }), "index.js");
  await call(`/workers/scripts/${SCRIPT_NAME}`, "PUT", form);
  await call(`/workers/scripts/${SCRIPT_NAME}/subdomain`, "POST", { enabled: true });

  const doNamespaces = (await call<Array<{ id: string; class: string; script: string }>>(
    "/workers/durable_objects/namespaces?per_page=100",
  )) ?? [];
  const apps = (await call<Array<{ id: string; name: string }>>("/containers/applications")) ?? [];
  for (const size of SIZES) {
    const ns = doNamespaces.find((n) => n.script === SCRIPT_NAME && n.class === size.className);
    if (!ns) throw new ProviderError("other", `Cloudflare did not create the ${size.className} namespace`);
    const app = {
      name: `${SCRIPT_NAME}-${size.tier}`,
      max_instances: 20,
      scheduling_policy: "default",
      configuration: { image: BRIDGE_IMAGE, instance_type: size.instanceType },
      durable_objects: { namespace_id: ns.id },
    };
    const existing = apps.find((a) => a.name === app.name);
    if (existing) await call(`/containers/applications/${existing.id}`, "PATCH", app);
    else await call("/containers/applications", "POST", app);
  }
  return `${account.accountId}:${account.apiToken}|https://${SCRIPT_NAME}.${subdomain}.workers.dev#${bridgeToken}`;
}

export interface CloudflareDeps extends ProvisionDeps {}

export class CloudflareProvider implements SandboxProvider {
  readonly kind = "cloudflare";
  private readonly fetchImpl: typeof fetch;

  constructor(private readonly config: CloudflareConfig, private readonly deps: CloudflareDeps = {}) {
    this.fetchImpl = deps.fetch ?? fetch;
  }

  // A pasted key carries the account and API token but no bridge yet.
  private get account(): { accountId: string; apiToken: string } | null {
    const { accountId, apiToken } = this.config;
    return accountId && apiToken ? { accountId, apiToken } : null;
  }

  async provisionCredential(): Promise<string> {
    const account = this.account;
    if (!account) throw new ProviderError("invalid_request", "a Cloudflare bridge key cannot provision a bridge");
    return await provisionBridge(account, this.deps);
  }

  capabilities(): ProviderCapabilities {
    return { ...CLOUDFLARE_CAPABILITIES };
  }

  private get sleepAfterSec(): number {
    return (this.config.autoStopMinutes ?? DEFAULT_PROVIDER_AUTOSTOP_MIN) * 60;
  }

  private url(path: string): string {
    if (!this.config.bridgeUrl) throw new ProviderError("invalid_request", "Cloudflare bridge is not provisioned");
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

  // A pasted key is checked by provisioning, which calls the account API
  // first; a bridge key by the bridge's health route.
  async validateCredentials(): Promise<void> {
    if (!this.config.bridgeUrl && this.account) return;
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
