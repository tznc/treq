// Boat (boat.dev) provider adapter. Boat sandboxes are persistent Ubuntu VMs
// with fixed sizes (small/default/large/xlarge), per-second billing while
// running, and a stop that snapshots the disk. The API reference is
// docs.boat.dev/api/v1 and its OpenAPI file docs.boat.dev/openapi/boat-v1.yaml.
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

export const BOAT_CAPABILITIES: Readonly<ProviderCapabilities> = Object.freeze({
  // `POST /sandboxes/{id}/stop` saves the disk; resume restores it.
  // Auto-stop: a `ttlSeconds` deadline (TREQ_PROVIDER_AUTOSTOP_MIN, 60 min)
  // as the backstop behind the idle controller, pushed out by keepAlive.
  stopKeepsDisk: true,
  execApi: true,
  // No service API: ensureService runs a pidfile supervisor through exec.
  serviceSupervisor: false,
  // The relay reaches treq-gateway through a private hosted URL
  // (`POST /sandboxes/{id}/host`); see gatewayEndpoint.
  tcpProxy: true,
  // boat.dev pricing: billed per second while running.
  billingGranularitySec: 1,
});

// Boat sizes, from `CreateSandboxRequest.type`.
const BOAT_TYPES = new Set(["small", "default", "large", "xlarge"]);

// `CommandRequest.timeoutSeconds` accepts 1 to 600.
const MAX_EXEC_TIMEOUT_SEC = 600;

export function normalizeBoatState(vendorState: string): ManagedInstanceState {
  switch (vendorState) {
    case "init":
    case "provisioning":
    case "provisioned":
    case "cloning":
      return "provisioning";
    case "ready":
    case "idle":
    case "running":
      return "ready";
    // The API reports a stopped sandbox as `archived`; `archiving` is a
    // stop in progress.
    case "archiving":
    case "archived":
      return "suspended";
    case "error":
      return "failed";
    // A create or fork that could not get a machine; terminal, then 404.
    case "cancelled":
      return "deleted";
    default:
      return "degraded";
  }
}

interface BoatSandbox {
  id: string;
  state: string;
  ip?: string | null;
  sshEndpoint?: string | null;
  // UNVERIFIED: the field name; billing falls back to the last probe without it.
  updatedAt?: string | null;
}

function normalizeInstance(sandbox: BoatSandbox): ProviderInstance {
  return providerInstance(
    sandbox.id,
    normalizeBoatState(sandbox.state),
    sandbox.updatedAt ?? null,
    sandbox.sshEndpoint ?? sandbox.ip ?? null,
  );
}

// 409 codes meaning "not ready yet": retryable, not a conflict.
const NOT_READY_CODES = ["sandbox_starting", "boat_starting", "sandbox_not_ready", "idempotency_in_progress"];

function boatError(response: Response): Promise<ProviderError> {
  return providerErrorFrom(response, (status, text, detail) => {
    let code = "";
    try {
      code = String(JSON.parse(text).code ?? "");
    } catch {
      // Not the JSON error envelope.
    }
    if (status === 409 && NOT_READY_CODES.includes(code)) return new ProviderError("unavailable", detail);
    // 403 machine_class_plan_required and the like are plan limits; any
    // other 403 is the key itself.
    if (status === 403 && (code.endsWith("_plan_required") || code === "billing_required")) {
      return new ProviderError("quota_exceeded", "provider quota exceeded");
    }
    if (status === 408) return new ProviderError("other", detail);
  });
}

const BOAT_DEFAULT_API_URL = "https://boat.dev/api/v1";

export interface BoatConfig {
  baseUrl: string;
  apiToken: string;
  /** Boat's own auto-stop TTL, the backstop behind our idle controller. */
  autoStopMinutes?: number;
}

/// Reads Boat configuration from Edge Function secrets. Never logged; never
/// returned to a client. With a BYOS credential, the user's key replaces
/// treq's token.
export function boatConfigFromEnv(
  env: (key: string) => string | undefined = (key) => Deno.env.get(key),
  credential?: ProviderCredential,
): BoatConfig {
  const baseUrl = env("TREQ_BOAT_API_URL") || (credential ? BOAT_DEFAULT_API_URL : undefined);
  const apiToken = credential?.apiKey ?? env("TREQ_BOAT_API_TOKEN");
  if (!baseUrl || !apiToken) {
    throw new ProviderError("invalid_request", "TREQ_BOAT_API_URL and TREQ_BOAT_API_TOKEN must be set");
  }
  return { baseUrl, apiToken, autoStopMinutes: providerAutoStopMinutes(env) };
}

export interface BoatDeps {
  fetch?: typeof fetch;
}

export class BoatProvider implements SandboxProvider {
  readonly kind = "boat";
  private readonly fetchImpl: typeof fetch;

  constructor(private readonly config: BoatConfig, deps: BoatDeps = {}) {
    this.fetchImpl = deps.fetch ?? fetch;
  }

  capabilities(): ProviderCapabilities {
    return { ...BOAT_CAPABILITIES };
  }

  private url(path: string): string {
    return `${this.config.baseUrl.replace(/\/+$/, "")}${path}`;
  }

  private sandboxUrl(id: string, action = ""): string {
    return this.url(`/sandboxes/${encodeURIComponent(id)}${action}`);
  }

  private request(
    url: string,
    init: { method?: string; body?: string; headers?: Record<string, string> } = {},
    timeoutMs?: number,
  ): Promise<Response> {
    const headers = {
      Authorization: `Bearer ${this.config.apiToken}`,
      "Content-Type": "application/json",
      ...init.headers,
    };
    return providerFetch(this.fetchImpl, "Boat API", url, { ...init, headers }, timeoutMs);
  }

  private async sandboxFrom(response: Response): Promise<BoatSandbox> {
    if (!response.ok) throw await boatError(response);
    const body = await response.json() as { sandbox?: BoatSandbox };
    if (!body.sandbox?.id) throw new ProviderError("other", "Boat response had no sandbox");
    return body.sandbox;
  }

  // Looks up by owner *and* placement: an owner can have two sandboxes at
  // once during a resize or migration, and the target must never be handed
  // the source. Failed sandboxes are skipped so a retry creates a fresh one.
  private async findByPlacement(owner: string, placement: string): Promise<BoatSandbox | null> {
    const url = new URL(this.url("/sandboxes"));
    url.searchParams.append("label", `treq_owner=${owner}`);
    url.searchParams.append("label", `treq_placement=${placement}`);
    const response = await this.request(url.toString());
    if (!response.ok) throw await boatError(response);
    const body = await response.json() as { sandboxes?: BoatSandbox[] };
    return (body.sandboxes ?? []).find((s) =>
      s.state !== "cancelled" && normalizeBoatState(s.state) !== "failed"
    ) ?? null;
  }

  async createInstance(params: CreateInstanceParams): Promise<ProviderInstance> {
    const type = params.machineType ?? "default";
    if (!BOAT_TYPES.has(type)) {
      throw new ProviderError("invalid_request", `unknown Boat machine type: ${type}`);
    }
    const owner = labelValue(params.ownerUserId);
    const placement = labelValue(params.placementId ?? params.idempotencyKey);
    // Boat honours Idempotency-Key for 24 hours; the placement label lookup
    // covers a retry after that, or one sent with a fresh key.
    const existing = await this.findByPlacement(owner, placement);
    if (existing) return normalizeInstance(existing);

    const response = await this.request(this.url("/sandboxes"), {
      method: "POST",
      headers: { "Idempotency-Key": params.idempotencyKey },
      body: JSON.stringify({
        type,
        // A deadline, not an idle timer, so keepAlive pushes it out on activity.
        ttlSeconds: this.ttlSeconds(),
        labels: { treq_owner: owner, treq_placement: placement, treq_manifest: String(params.manifestVersion) },
      }),
    }, 30_000);
    return normalizeInstance(await this.sandboxFrom(response));
  }

  async validateCredentials(): Promise<void> {
    const url = new URL(this.url("/sandboxes"));
    url.searchParams.set("limit", "1");
    await expectOk(await this.request(url.toString()), boatError);
  }

  async getInstance(providerId: string): Promise<ProviderInstance> {
    return normalizeInstance(await this.getSandbox(providerId));
  }

  private async getSandbox(providerId: string): Promise<BoatSandbox> {
    return await this.sandboxFrom(await this.request(this.sandboxUrl(providerId)));
  }

  private ttlSeconds(): number {
    return (this.config.autoStopMinutes ?? DEFAULT_PROVIDER_AUTOSTOP_MIN) * 60;
  }

  // `PATCH /sandboxes/{id}`: "The deadline becomes now + ttlSeconds".
  async keepAlive(providerId: string): Promise<void> {
    const response = await this.request(this.sandboxUrl(providerId), {
      method: "PATCH",
      body: JSON.stringify({ ttlSeconds: this.ttlSeconds() }),
    });
    await expectOk(response, boatError);
  }

  async wakeInstance(providerId: string): Promise<void> {
    // Resume also stores `ttlSeconds`, giving older sandboxes the backstop.
    const response = await this.request(this.sandboxUrl(providerId, "/resume"), {
      method: "POST",
      body: JSON.stringify({ ttlSeconds: this.ttlSeconds() }),
    }, 30_000);
    if (response.ok) return await expectOk(response, boatError);
    // UNVERIFIED: the code for resuming a running sandbox; any other 409 means already awake.
    const err = await boatError(response);
    if (err.kind !== "already_exists") throw err;
  }

  async stopInstance(providerId: string): Promise<void> {
    // 202: the disk save finishes in the background.
    const response = await this.request(this.sandboxUrl(providerId, "/stop"), { method: "POST" }, 30_000);
    await expectOk(response, boatError, [404]);
  }

  async replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance> {
    // The sandbox is the user's durable environment: repair keeps it.
    return await this.getInstance(params.providerResourceId);
  }

  async deleteInstance(providerId: string): Promise<void> {
    // Boat requires the sandbox id echoed in X-Ascii-Confirm-Delete.
    const response = await this.request(this.sandboxUrl(providerId), {
      method: "DELETE",
      headers: { "X-Ascii-Confirm-Delete": providerId },
    });
    await expectOk(response, boatError, [404]);
  }

  async execOnMachine(
    providerId: string,
    command: string[],
    timeoutSeconds = 20,
    environment: Readonly<Record<string, string>> = {},
  ): Promise<MachineExecResult> {
    assertExecArgs(command, environment, /^[A-Z_][A-Z0-9_]*$/);
    // One shell string and no env field: env goes in as `env NAME=value`.
    const envArgs = Object.entries(environment).map(([name, value]) => `${name}=${shellQuote([value])}`);
    const line = (envArgs.length ? `env ${envArgs.join(" ")} ` : "") + shellQuote(command);
    const timeout = Math.min(Math.max(Math.ceil(timeoutSeconds), 1), MAX_EXEC_TIMEOUT_SEC);
    const response = await this.request(this.sandboxUrl(providerId, "/commands"), {
      method: "POST",
      body: JSON.stringify({ command: line, timeoutSeconds: timeout }),
      // Boat waits up to 60 s for a starting sandbox before running it.
    }, (timeout + 70) * 1_000);
    if (!response.ok) throw await boatError(response);
    const body = await response.json() as {
      exitCode?: number | null;
      stdout?: string;
      stderr?: string;
      timedOut?: boolean;
    };
    if (body.timedOut) throw new ProviderError("timeout", "Boat command timed out");
    // A null exit code means a signal ended the command.
    return { exitCode: body.exitCode ?? -1, stdout: body.stdout ?? "", stderr: body.stderr ?? "" };
  }

  // UNVERIFIED: that the sandbox user can sudo and setsid/nohup exist on Boat's image.
  async ensureService(providerId: string, name: string, spec: ServiceSpec): Promise<void> {
    await ensurePidfileService(
      (command, timeout) => this.execOnMachine(providerId, command, timeout),
      name,
      spec,
      { vendor: "Boat", dir: "/var/run/treq", sudo: true },
    );
  }

  // Hosts treq-gateway's loopback port as a private URL and returns it as a
  // WebSocket endpoint. Re-requested on every call: a resumed sandbox may
  // have lost its earlier host.
  // UNVERIFIED: binary WebSocket frames through hosted URLs; the `/host` response shape.
  async gatewayEndpoint(providerId: string): Promise<GatewayEndpoint> {
    const sandbox = await this.getSandbox(providerId);
    if (normalizeBoatState(sandbox.state) === "suspended") await this.wakeInstance(providerId);
    const response = await this.request(this.sandboxUrl(providerId, "/host"), {
      method: "POST",
      body: JSON.stringify({ port: GATEWAY_PORT, public: false, localhost: true }),
    });
    if (!response.ok) throw await boatError(response);
    const hosted = await response.json() as { url?: string; token?: string };
    if (!hosted.url) throw new ProviderError("other", "Boat returned no hosted URL");
    const url = new URL(hosted.url);
    url.protocol = url.protocol === "http:" ? "ws:" : "wss:";
    if (hosted.token && !url.searchParams.has("_token")) url.searchParams.set("_token", hosted.token);
    return { url: url.toString(), headers: {} };
  }
}
