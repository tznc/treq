// E2B (e2b.dev) provider adapter. E2B sandboxes are Firecracker microVMs
// started from a template whose CPU and RAM are fixed when the template is
// built. They bill per second while running and pause with their filesystem
// (and memory) kept. Sources: the REST spec at
// github.com/e2b-dev/E2B/blob/main/spec/openapi.yml (api.e2b.app,
// X-API-Key; read 2026-10-28) and envd's `process.Process` Connect service
// (port 49983) used by the JS SDK in packages/js-sdk/src/envd. Vendor states
// and response shapes must never leave this module.

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

export const E2B_CAPABILITIES: Readonly<ProviderCapabilities> = Object.freeze({
  // `POST /sandboxes/{id}/pause` keeps the filesystem (and memory by
  // default); connect resumes it.
  stopKeepsDisk: true,
  execApi: true,
  // No service API: ensureService runs a pidfile supervisor through envd.
  serviceSupervisor: false,
  // The relay reaches treq-gateway through E2B's port proxy; see
  // gatewayEndpoint.
  tcpProxy: true,
  // e2b.dev/pricing: per second while running.
  billingGranularitySec: 1,
});

const ENVD_PORT = 49983;

// Treq machine types (`provider_machine_types.vendor_type`) are the size
// tier ids. CPU and RAM are template build settings, so each size is its own
// template, `<TREQ_E2B_TEMPLATE>-<size>`.
// UNVERIFIED: templates are built by hand (`e2b template build --cpu-count 2 --memory-mb 4096` for s, 4/8192 m, 8/16384 l, 16/32768 xl); l/xl exceed default plan limits.
const E2B_TYPES = new Set(["s", "m", "l", "xl"]);

export function normalizeE2BState(vendorState: string): ManagedInstanceState {
  switch (vendorState) {
    case "running":
      return "ready";
    case "paused":
    case "pausing":
      return "suspended";
    default:
      return "degraded";
  }
}

interface E2BSandbox {
  sandboxID: string;
  state?: string;
  domain?: string | null;
  envdAccessToken?: string | null;
  trafficAccessToken?: string | null;
}

function normalizeInstance(sandbox: E2BSandbox): ProviderInstance {
  // Create and connect answer with a Sandbox, which has no state: both mean
  // it is running. E2B reports no pause time.
  return providerInstance(sandbox.sandboxID, normalizeE2BState(sandbox.state ?? "running"));
}

function e2bError(response: Response): Promise<ProviderError> {
  return providerErrorFrom(response, (status, text, detail) => {
    // Create's 409 with `idempotency_in_progress` is a retry-later.
    if (status === 409 && text.includes("idempotency_in_progress")) return new ProviderError("unavailable", detail);
    if (status === 504) return new ProviderError("timeout", detail);
  });
}

export interface E2BConfig {
  baseUrl: string;
  apiKey: string;
  // Template name prefix; the size is appended, e.g. `treq-sandbox-m`.
  template: string;
  /** The sandbox timeout (auto-pause), the backstop behind our idle controller. */
  autoStopMinutes?: number;
}

/// Reads E2B configuration from Edge Function secrets. Never logged; never
/// returned to a client. With a BYOS credential, the user's key replaces
/// treq's.
export function e2bConfigFromEnv(
  env: (key: string) => string | undefined = (key) => Deno.env.get(key),
  credential?: ProviderCredential,
): E2BConfig {
  const apiKey = credential?.apiKey ?? env("TREQ_E2B_API_KEY");
  if (!apiKey) throw new ProviderError("invalid_request", "TREQ_E2B_API_KEY must be set");
  return {
    baseUrl: env("TREQ_E2B_API_URL") || "https://api.e2b.app",
    apiKey,
    template: env("TREQ_E2B_TEMPLATE") || "treq-sandbox",
    autoStopMinutes: providerAutoStopMinutes(env),
  };
}

export interface E2BDeps {
  fetch?: typeof fetch;
}

export class E2BProvider implements SandboxProvider {
  readonly kind = "e2b";
  private readonly fetchImpl: typeof fetch;

  constructor(private readonly config: E2BConfig, deps: E2BDeps = {}) {
    this.fetchImpl = deps.fetch ?? fetch;
  }

  capabilities(): ProviderCapabilities {
    return { ...E2B_CAPABILITIES };
  }

  // Seconds until E2B auto-pauses the sandbox. keepAlive and connect push it out.
  private get timeoutSec(): number {
    return (this.config.autoStopMinutes ?? DEFAULT_PROVIDER_AUTOSTOP_MIN) * 60;
  }

  private url(path: string): string {
    return `${this.config.baseUrl.replace(/\/+$/, "")}${path}`;
  }

  private sandboxUrl(id: string, action = "", v2 = false): string {
    return this.url(`${v2 ? "/v2" : ""}/sandboxes/${encodeURIComponent(id)}${action}`);
  }

  private request(url: string, init: RequestInit, timeoutMs?: number): Promise<Response> {
    return providerFetch(this.fetchImpl, "E2B", url, init, timeoutMs);
  }

  // The REST API; envd calls go through `request` with their own headers.
  private api(
    url: string,
    init: { method?: string; body?: string; headers?: Record<string, string> } = {},
    timeoutMs?: number,
  ): Promise<Response> {
    const headers = { "X-API-Key": this.config.apiKey, "Content-Type": "application/json", ...init.headers };
    return this.request(url, { ...init, headers }, timeoutMs);
  }

  private async sandboxFrom(response: Response): Promise<E2BSandbox> {
    if (!response.ok) throw await e2bError(response);
    const body = await response.json() as E2BSandbox;
    if (!body?.sandboxID) throw new ProviderError("other", "E2B response had no sandbox");
    return body;
  }

  // Looks up by owner *and* placement: an owner can have two sandboxes at
  // once during a resize or migration, and the target must never be handed
  // the source. Only running and paused sandboxes are listed; a killed one
  // is gone, so a retry creates a fresh sandbox.
  private async findByPlacement(metadata: Record<string, string>): Promise<E2BSandbox | null> {
    const url = new URL(this.url("/v2/sandboxes"));
    url.searchParams.set("metadata", new URLSearchParams(metadata).toString());
    url.searchParams.set("state", "running,paused");
    const response = await this.api(url.toString());
    if (!response.ok) throw await e2bError(response);
    const list = await response.json() as E2BSandbox[];
    return (Array.isArray(list) ? list : []).find((s) =>
      normalizeE2BState(s.state ?? "running") !== "degraded"
    ) ?? null;
  }

  async createInstance(params: CreateInstanceParams): Promise<ProviderInstance> {
    const type = params.machineType ?? "s";
    if (!E2B_TYPES.has(type)) throw new ProviderError("invalid_request", `unknown E2B machine type: ${type}`);
    const lookup = {
      treq_owner: labelValue(params.ownerUserId),
      treq_placement: labelValue(params.placementId ?? params.idempotencyKey),
    };
    // Idempotency-Key is honoured for a limited window; the metadata lookup
    // covers a retry after that, or one sent with a fresh key.
    const existing = await this.findByPlacement(lookup);
    if (existing) return normalizeInstance(existing);

    const key = params.idempotencyKey.toLowerCase().replace(/[^a-z0-9-]/g, "-").slice(0, 255);
    const response = await this.api(this.url("/v2/sandboxes"), {
      method: "POST",
      headers: { "Idempotency-Key": key },
      body: JSON.stringify({
        templateID: `${this.config.template}-${type}`,
        // At the timeout the sandbox pauses instead of being killed.
        // UNVERIFIED: the team's max sandbox length (1 h Hobby, 24 h Pro) caps the timeout.
        timeout: this.timeoutSec,
        autoPause: true,
        metadata: { ...lookup, treq_manifest: String(params.manifestVersion) },
        // Port URLs then require the traffic access token (gatewayEndpoint).
        network: { allowPublicTraffic: false },
      }),
    }, 60_000);
    return normalizeInstance(await this.sandboxFrom(response));
  }

  async validateCredentials(): Promise<void> {
    const url = new URL(this.url("/v2/sandboxes"));
    url.searchParams.set("limit", "1");
    await expectOk(await this.api(url.toString()), e2bError);
  }

  async getInstance(providerId: string): Promise<ProviderInstance> {
    return normalizeInstance(await this.sandboxFrom(await this.api(this.sandboxUrl(providerId))));
  }

  // Resumes a paused sandbox (201) or returns a running one (200), extends
  // its timeout, and returns the tokens envd and the port proxy need.
  private async connect(providerId: string): Promise<E2BSandbox> {
    const response = await this.api(
      this.sandboxUrl(providerId, "/connect", true),
      { method: "POST", body: JSON.stringify({ timeout: this.timeoutSec }) },
      60_000,
    );
    return await this.sandboxFrom(response);
  }

  async wakeInstance(providerId: string): Promise<void> {
    await this.connect(providerId);
  }

  async stopInstance(providerId: string): Promise<void> {
    const response = await this.api(this.sandboxUrl(providerId, "/pause"), { method: "POST" }, 60_000);
    // UNVERIFIED: pause's bare 409 is taken to mean already paused.
    await expectOk(response, e2bError, [404, 409]);
  }

  // `POST /sandboxes/{id}/timeout` restarts the TTL from now.
  async keepAlive(providerId: string): Promise<void> {
    const response = await this.api(this.sandboxUrl(providerId, "/timeout"), {
      method: "POST",
      body: JSON.stringify({ timeout: this.timeoutSec }),
    });
    await expectOk(response, e2bError);
  }

  async replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance> {
    // The sandbox is the user's durable environment: repair keeps it.
    return await this.getInstance(params.providerResourceId);
  }

  // E2B has nothing named after the placement, so `opts` is unused.
  async deleteInstance(providerId: string, _opts?: DeleteInstanceOptions): Promise<void> {
    await expectOk(await this.api(this.sandboxUrl(providerId), { method: "DELETE" }), e2bError, [404]);
  }

  async execOnMachine(
    providerId: string,
    command: string[],
    timeoutSeconds = 20,
    environment: Readonly<Record<string, string>> = {},
  ): Promise<MachineExecResult> {
    assertExecArgs(command, environment);
    // Connect resumes a paused sandbox, turns a missing one into not_found,
    // and hands back the envd token.
    const sandbox = await this.connect(providerId);
    const timeoutMs = Math.max(Math.ceil(timeoutSeconds), 1) * 1_000;
    const headers: Record<string, string> = {
      "Content-Type": "application/connect+json",
      "Connect-Protocol-Version": "1",
      "Connect-Timeout-Ms": String(timeoutMs),
      "E2b-Sandbox-Id": providerId,
      "E2b-Sandbox-Port": String(ENVD_PORT),
    };
    if (sandbox.envdAccessToken) headers["X-Access-Token"] = sandbox.envdAccessToken;
    const [cmd, ...args] = command;
    const response = await this.request(
      `https://${ENVD_PORT}-${providerId}.${sandbox.domain || "e2b.app"}/process.Process/Start`,
      {
        method: "POST",
        headers,
        body: connectEnvelope({ process: { cmd, args, envs: { ...environment } } }),
      },
      timeoutMs + 10_000,
    );
    if (!response.ok) {
      const err = await e2bError(response);
      // envd answers 404 only for an unknown RPC; the sandbox was found by
      // connect, so treat its errors as unavailable.
      throw err.kind === "not_found" ? new ProviderError("unavailable", "envd unavailable") : err;
    }
    return parseProcessStream(new Uint8Array(await response.arrayBuffer()));
  }

  // UNVERIFIED: that a nohup/setsid child outlives the envd process call that started it.
  async ensureService(providerId: string, name: string, spec: ServiceSpec): Promise<void> {
    await ensurePidfileService(
      (command, timeout) => this.execOnMachine(providerId, command, timeout),
      name,
      spec,
      { vendor: "E2B", dir: "/tmp/treq" },
    );
  }

  // E2B exposes sandbox ports as HTTP(S)/WebSocket at
  // `https://{port}-{sandboxId}.{domain}`. Sandboxes are created with
  // `allowPublicTraffic: false`, so the proxy needs the sandbox's traffic
  // access token (returned by connect).
  // UNVERIFIED: the `e2b-traffic-access-token` header name, binary WebSocket frames, and reaching a 127.0.0.1 port (else listen on 0.0.0.0 as Fly does).
  async gatewayEndpoint(providerId: string): Promise<GatewayEndpoint> {
    const sandbox = await this.connect(providerId);
    return {
      url: `wss://${GATEWAY_PORT}-${sandbox.sandboxID}.${sandbox.domain || "e2b.app"}/`,
      headers: sandbox.trafficAccessToken ? { "e2b-traffic-access-token": sandbox.trafficAccessToken } : {},
    };
  }
}

// Connect protocol streaming envelope: 1 flag byte, 4-byte big-endian
// length, then the JSON message.
function connectEnvelope(message: unknown): Uint8Array<ArrayBuffer> {
  const payload = new TextEncoder().encode(JSON.stringify(message));
  const out = new Uint8Array(5 + payload.length);
  new DataView(out.buffer).setUint32(1, payload.length);
  out.set(payload, 5);
  return out;
}

function decodeBase64(b64: string): Uint8Array {
  return Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
}

function parseProcessStream(bytes: Uint8Array): MachineExecResult {
  const stdout: Uint8Array[] = [];
  const stderr: Uint8Array[] = [];
  let exitCode: number | null = null;
  let off = 0;
  while (off + 5 <= bytes.length) {
    const flags = bytes[off];
    const len = new DataView(bytes.buffer, bytes.byteOffset + off + 1, 4).getUint32(0);
    const message = JSON.parse(new TextDecoder().decode(bytes.subarray(off + 5, off + 5 + len)));
    off += 5 + len;
    if (flags & 0x02) {
      // End-of-stream trailer; carries an error when the RPC failed.
      if (message?.error) {
        const code = String(message.error.code ?? "");
        throw new ProviderError(
          code === "deadline_exceeded" ? "timeout" : code === "not_found" ? "not_found" : "other",
          `envd: ${String(message.error.message ?? code).slice(0, 500)}`,
        );
      }
      continue;
    }
    const event = message?.event ?? {};
    if (event.data?.stdout) stdout.push(decodeBase64(event.data.stdout));
    if (event.data?.stderr) stderr.push(decodeBase64(event.data.stderr));
    // proto3 JSON omits a zero exit code.
    if (event.end) exitCode = event.end.exitCode ?? 0;
  }
  if (exitCode === null) throw new ProviderError("other", "envd stream ended without an exit");
  const text = (parts: Uint8Array[]) => parts.map((p) => new TextDecoder().decode(p)).join("");
  return { exitCode, stdout: text(stdout), stderr: text(stderr) };
}
