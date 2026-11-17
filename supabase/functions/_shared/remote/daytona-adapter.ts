// Daytona (daytona.io) provider adapter. Daytona sandboxes are containers
// sized by cpu/memory/disk, billed per second while running; a stopped
// sandbox keeps its disk and bills storage. The REST reference is the
// OpenAPI file at daytona.io/docs/openapi.json (base
// https://app.daytona.io/api), and commands run through the toolbox API
// (daytona.io/docs/toolbox-openapi.json) at `{toolboxProxyUrl}/{sandboxId}`.
// Vendor states and response shapes must never leave this module.

import type {
  CreateInstanceParams,
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
  shellQuote,
} from "./provider-http.ts";

export const DAYTONA_CAPABILITIES: Readonly<ProviderCapabilities> = Object.freeze({
  // daytona.io/pricing: stopped sandboxes keep their disk and bill storage.
  // Auto-stop: `autoStopInterval` minutes of inactivity
  // (TREQ_PROVIDER_AUTOSTOP_MIN, 60) as the backstop behind the idle
  // controller; keepAlive refreshes last activity.
  stopKeepsDisk: true,
  execApi: true,
  // Toolbox sessions run background commands but do not restart them, so
  // ensureService runs a pidfile supervisor through exec.
  serviceSupervisor: false,
  // The relay reaches treq-gateway through the authenticated preview
  // proxy; see gatewayEndpoint.
  tcpProxy: true,
  // daytona.io/pricing: billed per second.
  billingGranularitySec: 1,
});

// Treq machine types (`provider_machine_types.vendor_type`) to Daytona
// resources. Matches size_tiers; disk equals the tier's min_disk_gb.
// Daytona's default per-sandbox limit is 4 vCPU / 8 GiB / 10 GiB, so l/xl
// (unselectable in the catalog) need treq's account limits raised.
const DAYTONA_TYPES: Record<string, { cpu: number; memory: number; disk: number }> = {
  s: { cpu: 2, memory: 4, disk: 10 },
  m: { cpu: 4, memory: 8, disk: 40 },
  l: { cpu: 8, memory: 16, disk: 100 },
  xl: { cpu: 16, memory: 32, disk: 200 },
};

// Vendor states a placement lookup may return: alive and not failed.
const LIVE_STATES = [
  "creating",
  "pending_build",
  "building_snapshot",
  "pulling_snapshot",
  "started",
  "starting",
  "restoring",
  "resuming",
  "stopping",
  "stopped",
  "archiving",
  "archived",
  "pausing",
  "paused",
];

// Daytona targets are `us` and `eu`; there is no Asia-Pacific target.
const DAYTONA_TARGETS: Partial<Record<string, string>> = { us_east: "us", us_west: "us", eu_west: "eu" };

export function normalizeDaytonaState(vendorState: string): ManagedInstanceState {
  switch (vendorState) {
    case "creating":
    case "pending_build":
    case "building_snapshot":
    case "pulling_snapshot":
      return "provisioning";
    case "started":
      return "ready";
    case "starting":
    case "restoring":
    case "resuming":
      return "waking";
    case "stopping":
    case "stopped":
    case "archiving":
    case "archived":
    case "pausing":
    case "paused":
      return "suspended";
    case "error":
    case "build_failed":
      return "failed";
    case "destroying":
      return "deleting";
    case "destroyed":
      return "deleted";
    default:
      // resizing, snapshotting, forking, unknown, and anything new.
      return "degraded";
  }
}

interface DaytonaSandbox {
  id: string;
  state: string;
  toolboxProxyUrl?: string | null;
  updatedAt?: string | null;
}

function normalizeInstance(sandbox: DaytonaSandbox): ProviderInstance {
  return providerInstance(sandbox.id, normalizeDaytonaState(sandbox.state), sandbox.updatedAt ?? null);
}

const daytonaError = (response: Response) => providerErrorFrom(response);

export interface DaytonaConfig {
  baseUrl: string;
  apiKey: string;
  /** Daytona's own auto-stop interval, the backstop behind our idle controller. */
  autoStopMinutes?: number;
}

/// Reads Daytona configuration from Edge Function secrets. Never logged;
/// never returned to a client. With a BYOS credential, the user's key
/// replaces treq's.
export function daytonaConfigFromEnv(
  env: (key: string) => string | undefined = (key) => Deno.env.get(key),
  credential?: ProviderCredential,
): DaytonaConfig {
  const apiKey = credential?.apiKey ?? env("TREQ_DAYTONA_API_KEY");
  if (!apiKey) throw new ProviderError("invalid_request", "TREQ_DAYTONA_API_KEY must be set");
  return {
    baseUrl: env("TREQ_DAYTONA_API_URL") || "https://app.daytona.io/api",
    apiKey,
    autoStopMinutes: providerAutoStopMinutes(env),
  };
}

export interface DaytonaDeps {
  fetch?: typeof fetch;
}

export class DaytonaProvider implements SandboxProvider {
  readonly kind = "daytona";
  private readonly fetchImpl: typeof fetch;

  constructor(private readonly config: DaytonaConfig, deps: DaytonaDeps = {}) {
    this.fetchImpl = deps.fetch ?? fetch;
  }

  capabilities(): ProviderCapabilities {
    return { ...DAYTONA_CAPABILITIES };
  }

  private url(path: string): string {
    return `${this.config.baseUrl.replace(/\/+$/, "")}${path}`;
  }

  private sandboxUrl(id: string, action = ""): string {
    return this.url(`/sandbox/${encodeURIComponent(id)}${action}`);
  }

  private request(url: string, init: { method?: string; body?: string } = {}, timeoutMs?: number): Promise<Response> {
    const headers = { Authorization: `Bearer ${this.config.apiKey}`, "Content-Type": "application/json" };
    return providerFetch(this.fetchImpl, "Daytona API", url, { ...init, headers }, timeoutMs);
  }

  private async sandboxFrom(response: Response): Promise<DaytonaSandbox> {
    if (!response.ok) throw await daytonaError(response);
    const body = await response.json() as DaytonaSandbox;
    if (!body?.id) throw new ProviderError("other", "Daytona response had no sandbox");
    return body;
  }

  // Looks up by owner *and* placement: an owner can have two sandboxes at
  // once during a resize or migration, and the target must never be handed
  // the source. Failed sandboxes are skipped so a retry creates a fresh one.
  private async findByPlacement(owner: string, placement: string): Promise<DaytonaSandbox | null> {
    let cursor: string | null = null;
    do {
      const url = new URL(this.url("/sandbox"));
      url.searchParams.set("labels", JSON.stringify({ treq_owner: owner, treq_placement: placement }));
      url.searchParams.set("states", LIVE_STATES.join(","));
      if (cursor) url.searchParams.set("cursor", cursor);
      const response = await this.request(url.toString());
      if (!response.ok) throw await daytonaError(response);
      const body = await response.json() as { items?: DaytonaSandbox[]; nextCursor?: string | null };
      const found = body.items?.find((s) => LIVE_STATES.includes(s.state));
      if (found) return found;
      cursor = body.nextCursor ?? null;
    } while (cursor);
    return null;
  }

  async createInstance(params: CreateInstanceParams): Promise<ProviderInstance> {
    const type = params.machineType ?? "s";
    const resources = DAYTONA_TYPES[type];
    if (!resources) throw new ProviderError("invalid_request", `unknown Daytona machine type: ${type}`);
    const target = DAYTONA_TARGETS[params.region];
    if (!target) throw new ProviderError("invalid_request", `Daytona has no target for ${params.region}`);
    const owner = labelValue(params.ownerUserId);
    const placement = labelValue(params.placementId ?? params.idempotencyKey);
    // No Idempotency-Key header; the placement label lookup makes a retried
    // create return the first sandbox.
    const existing = await this.findByPlacement(owner, placement);
    if (existing) return normalizeInstance(existing);

    const response = await this.request(this.url("/sandbox"), {
      method: "POST",
      body: JSON.stringify({
        ...resources,
        target,
        // Auto-stop is the backstop. 0 means the maximum auto-archive
        // interval; a negative auto-delete interval disables it.
        autoStopInterval: this.config.autoStopMinutes ?? DEFAULT_PROVIDER_AUTOSTOP_MIN,
        autoArchiveInterval: 0,
        autoDeleteInterval: -1,
        labels: { treq_owner: owner, treq_placement: placement, treq_manifest: String(params.manifestVersion) },
      }),
    }, 60_000);
    return normalizeInstance(await this.sandboxFrom(response));
  }

  async validateCredentials(): Promise<void> {
    await expectOk(await this.request(this.url("/sandbox")), daytonaError);
  }

  async getInstance(providerId: string): Promise<ProviderInstance> {
    return normalizeInstance(await this.getSandbox(providerId));
  }

  private async getSandbox(providerId: string): Promise<DaytonaSandbox> {
    return await this.sandboxFrom(await this.request(this.sandboxUrl(providerId)));
  }

  async wakeInstance(providerId: string): Promise<void> {
    // Starts a stopped sandbox, restores an archived one, resumes a paused one.
    const response = await this.request(this.sandboxUrl(providerId, "/start"), { method: "POST" }, 60_000);
    if (response.ok) return await expectOk(response, daytonaError);
    const err = await daytonaError(response);
    // UNVERIFIED: the code for starting a started sandbox; on 400/409 check the state.
    if (err.kind === "invalid_request" || err.kind === "already_exists") {
      const state = normalizeDaytonaState((await this.getSandbox(providerId)).state);
      if (state === "ready" || state === "waking") return;
    }
    throw err;
  }

  async stopInstance(providerId: string): Promise<void> {
    const response = await this.request(this.sandboxUrl(providerId, "/stop"), { method: "POST" }, 60_000);
    await expectOk(response, daytonaError, [404]);
  }

  // `POST /sandbox/{id}/last-activity` ("Update sandbox last activity").
  // UNVERIFIED: that it restarts the auto-stop countdown.
  async keepAlive(providerId: string): Promise<void> {
    await expectOk(await this.request(this.sandboxUrl(providerId, "/last-activity"), { method: "POST" }), daytonaError);
  }

  async replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance> {
    // The sandbox is the user's durable environment: repair keeps it.
    return await this.getInstance(params.providerResourceId);
  }

  async deleteInstance(providerId: string): Promise<void> {
    await expectOk(await this.request(this.sandboxUrl(providerId), { method: "DELETE" }), daytonaError, [404]);
  }

  async execOnMachine(
    providerId: string,
    command: string[],
    timeoutSeconds = 20,
    environment: Readonly<Record<string, string>> = {},
  ): Promise<MachineExecResult> {
    assertExecArgs(command, environment);
    // The toolbox lives behind the sandbox's `toolboxProxyUrl`; the GET also
    // turns a missing sandbox into not_found.
    const sandbox = await this.getSandbox(providerId);
    const base = (sandbox.toolboxProxyUrl || "https://proxy.app.daytona.io/toolbox").replace(/\/+$/, "");
    const timeout = Math.max(Math.ceil(timeoutSeconds), 1);
    const response = await this.request(
      `${base}/${encodeURIComponent(providerId)}/process/execute`,
      {
        method: "POST",
        // UNVERIFIED: whether the toolbox runs `command` through a shell; quoting is safe either way.
        body: JSON.stringify({ command: shellQuote(command), envs: { ...environment }, timeout }),
      },
      (timeout + 30) * 1_000,
    );
    if (!response.ok) throw await daytonaError(response);
    const body = await response.json() as { exitCode?: number | null; result?: string };
    // ExecuteResponse has one combined `result`, no separate stderr.
    return { exitCode: body.exitCode ?? -1, stdout: body.result ?? "", stderr: "" };
  }

  // UNVERIFIED: that the sandbox user can sudo and setsid/nohup exist on the image.
  async ensureService(providerId: string, name: string, spec: ServiceSpec): Promise<void> {
    await ensurePidfileService(
      (command, timeout) => this.execOnMachine(providerId, command, timeout),
      name,
      spec,
      { vendor: "Daytona", dir: "/var/run/treq", sudo: true },
    );
  }

  // Daytona's SSH gateway terminates SSH itself, so it cannot carry Treq's
  // own session. Its preview proxy carries WebSocket to any port as
  // `https://{port}-{sandboxId}.{proxyDomain}`, authenticated by the
  // `x-daytona-preview-token` header from `GET /sandbox/{id}/ports/{port}/preview-url`.
  // UNVERIFIED: binary WebSocket frames through the proxy, and reaching a 127.0.0.1 port.
  async gatewayEndpoint(providerId: string): Promise<GatewayEndpoint> {
    const state = normalizeDaytonaState((await this.getSandbox(providerId)).state);
    if (state === "suspended") await this.wakeInstance(providerId);
    const response = await this.request(this.sandboxUrl(providerId, `/ports/${GATEWAY_PORT}/preview-url`));
    if (!response.ok) throw await daytonaError(response);
    const preview = await response.json() as { url?: string; token?: string };
    if (!preview.url) throw new ProviderError("other", "Daytona returned no preview URL");
    const url = new URL(preview.url);
    url.protocol = url.protocol === "http:" ? "ws:" : "wss:";
    return {
      url: url.toString(),
      headers: preview.token ? { "x-daytona-preview-token": preview.token } : {},
    };
  }
}
