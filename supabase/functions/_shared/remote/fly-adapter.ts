// Fly.io Machines provider adapter (plain Machines, not Sprites). REST
// reference: docs.fly.io/machines/api, base https://api.machines.dev/v1,
// Bearer token. One Fly app per environment (TREQ_FLY_APP) holds one Machine
// per placement, each with its own volume for the persistent workspace disk.
// Vendor states and response shapes must never leave this module.

import type {
  CreateInstanceParams,
  DeleteInstanceOptions,
  GatewayEndpoint,
  MachineExecResult,
  ManagedInstanceState,
  ProviderCapabilities,
  ProviderInstance,
  ReplaceInstanceParams,
  SandboxProvider,
  ServiceSpec,
} from "./provider.ts";
import { GATEWAY_PORT, ProviderError } from "./provider.ts";
import type { RegionCode } from "./catalog.ts";
import {
  assertExecArgs,
  ensurePidfileService,
  expectOk,
  providerErrorFrom,
  providerFetch,
  providerInstance,
} from "./provider-http.ts";

export const FLY_CAPABILITIES: Readonly<ProviderCapabilities> = Object.freeze({
  // docs.fly.io/about/pricing: a stopped Machine bills only rootfs and its
  // volumes; the volume survives stop/start.
  stopKeepsDisk: true,
  execApi: true,
  // No in-machine service API: ensureService runs a pidfile supervisor through exec.
  serviceSupervisor: false,
  // The relay reaches treq-gateway through Fly Proxy; see gatewayEndpoint.
  tcpProxy: true,
  // Billed per second while running.
  billingGranularitySec: 1,
});

/** Fly Proxy forwards to the machine's interface, not loopback. */
export const FLY_GATEWAY_LISTEN = `0.0.0.0:${GATEWAY_PORT}`;

const FLY_REGIONS: Record<RegionCode, string> = {
  us_east: "iad",
  us_west: "sjc",
  eu_west: "lhr",
  ap_southeast: "sin",
};

// Volume size per vCPU count, matching size_tiers.min_disk_gb.
const DISK_GB_BY_CPUS: Record<number, number> = { 2: 10, 4: 40, 8: 100, 16: 200 };

/** Where the volume mounts: the workspace disk. */
const VOLUME_PATH = "/home";

/** Parses a vendor_type like `performance-2x:4096` into a Fly guest. */
export function parseFlyMachineType(
  vendorType: string,
): { cpu_kind: string; cpus: number; memory_mb: number } | null {
  const m = vendorType.match(/^(shared|performance)-cpu-(\d+)x:(\d+)$|^(performance)-(\d+)x:(\d+)$/);
  if (!m) return null;
  return m[1]
    ? { cpu_kind: m[1], cpus: Number(m[2]), memory_mb: Number(m[3]) }
    : { cpu_kind: m[4], cpus: Number(m[5]), memory_mb: Number(m[6]) };
}

export function normalizeFlyState(vendorState: string): ManagedInstanceState {
  switch (vendorState) {
    case "created":
      return "provisioning";
    case "started":
      return "ready";
    case "starting":
      return "waking";
    case "stopping":
    case "stopped":
    case "suspending":
    case "suspended":
      return "suspended";
    case "failed":
      return "failed";
    case "destroying":
      return "deleting";
    case "destroyed":
      return "deleted";
    default:
      // replacing, updating, launch_failed variants, anything new.
      return "degraded";
  }
}

interface FlyMachine {
  id: string;
  name?: string;
  state: string;
  config?: { mounts?: Array<{ volume: string }>; metadata?: Record<string, string> };
  // UNVERIFIED: that `updated_at` moves on every state change.
  updated_at?: string | null;
}

interface FlyVolume {
  id: string;
  name: string;
  attached_machine_id?: string | null;
  state?: string;
}

// The deterministic per-placement key, machine name and volume name.
function placementKey(placementOrKey: string): string {
  return placementOrKey.toLowerCase().replace(/[^a-z0-9]/g, "-");
}

function volumeNameFor(key: string): string {
  // Volume names: lowercase alphanumerics and underscores, at most 30.
  return `treq_${key.replace(/-/g, "_")}`.slice(0, 30);
}

// Backoff for a volume DELETE while Fly still detaches it from the
// destroyed machine: about 15 seconds in all.
const VOLUME_DELETE_BACKOFF_MS = [500, 1000, 2000, 4000, 8000];

function normalizeInstance(m: FlyMachine): ProviderInstance {
  return providerInstance(m.id, normalizeFlyState(m.state), m.updated_at ?? null);
}

// Fly takes no BYOS key, so 401 stays `other` and 403 is a plan limit.
function flyError(response: Response): Promise<ProviderError> {
  return providerErrorFrom(response, (status, _text, detail) => {
    if (status === 401) return new ProviderError("other", detail);
    if (status === 403) return new ProviderError("quota_exceeded", "provider quota exceeded");
    if (status === 412) return new ProviderError("invalid_request", detail);
  });
}

export interface FlyConfig {
  baseUrl: string;
  apiToken: string;
  app: string;
  image: string;
}

/// Reads Fly configuration from Edge Function secrets. Never logged; never
/// returned to a client. TREQ_FLY_IMAGE is an Ubuntu-based dev image that
/// ops must build and push (it needs sshd, curl, sha256sum, setsid).
export function flyConfigFromEnv(
  env: (key: string) => string | undefined = (key) => Deno.env.get(key),
): FlyConfig {
  const apiToken = env("TREQ_FLY_API_TOKEN");
  const app = env("TREQ_FLY_APP");
  const image = env("TREQ_FLY_IMAGE");
  if (!apiToken || !app || !image) {
    throw new ProviderError("invalid_request", "TREQ_FLY_API_TOKEN, TREQ_FLY_APP and TREQ_FLY_IMAGE must be set");
  }
  return { baseUrl: env("TREQ_FLY_API_URL") || "https://api.machines.dev", apiToken, app, image };
}

export interface FlyDeps {
  fetch?: typeof fetch;
  sleep?: (ms: number) => Promise<void>;
}

export class FlyMachinesProvider implements SandboxProvider {
  readonly kind = "fly_machines";
  readonly gatewayListenAddress = FLY_GATEWAY_LISTEN;
  private readonly fetchImpl: typeof fetch;
  private readonly sleep: (ms: number) => Promise<void>;

  constructor(private readonly config: FlyConfig, deps: FlyDeps = {}) {
    this.fetchImpl = deps.fetch ?? fetch;
    this.sleep = deps.sleep ?? ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
  }

  capabilities(): ProviderCapabilities {
    return { ...FLY_CAPABILITIES };
  }

  private url(path: string): string {
    return `${this.config.baseUrl.replace(/\/+$/, "")}/v1/apps/${encodeURIComponent(this.config.app)}${path}`;
  }

  private machineUrl(id: string, action = ""): string {
    return this.url(`/machines/${encodeURIComponent(id)}${action}`);
  }

  private request(url: string, init: { method?: string; body?: string } = {}, timeoutMs?: number): Promise<Response> {
    const headers = { Authorization: `Bearer ${this.config.apiToken}`, "Content-Type": "application/json" };
    return providerFetch(this.fetchImpl, "Fly Machines API", url, { ...init, headers }, timeoutMs);
  }

  private async json<T>(response: Response): Promise<T> {
    if (!response.ok) throw await flyError(response);
    return await response.json() as T;
  }

  async createInstance(params: CreateInstanceParams): Promise<ProviderInstance> {
    const type = params.machineType ?? "shared-cpu-2x:4096";
    const guest = parseFlyMachineType(type);
    if (!guest) throw new ProviderError("invalid_request", `unknown Fly machine type: ${type}`);
    const region = FLY_REGIONS[params.region];
    if (!region) throw new ProviderError("invalid_request", `unknown region: ${params.region}`);
    const key = placementKey(params.placementId ?? params.idempotencyKey);
    const name = `treq-${key}`.slice(0, 63);
    const volumeName = volumeNameFor(key);

    // The Machines API has no idempotency key: look up by name first.
    const machines = await this.json<FlyMachine[]>(await this.request(this.url("/machines")));
    const existing = machines.find((m) => m.name === name && m.state !== "destroyed");
    if (existing) return normalizeInstance(existing);

    // A create that failed after the volume reuses the orphaned volume.
    const volumes = await this.json<FlyVolume[]>(await this.request(this.url("/volumes")));
    let volume = volumes.find((v) => v.name === volumeName && !v.attached_machine_id && v.state !== "destroyed");
    if (!volume) {
      volume = await this.json<FlyVolume>(
        await this.request(this.url("/volumes"), {
          method: "POST",
          body: JSON.stringify({ name: volumeName, region, size_gb: DISK_GB_BY_CPUS[guest.cpus] ?? 10 }),
        }, 60_000),
      );
    }

    const machine = await this.json<FlyMachine>(
      await this.request(this.url("/machines"), {
        method: "POST",
        body: JSON.stringify({
          name,
          region,
          config: {
            image: this.config.image,
            guest,
            mounts: [{ volume: volume.id, path: VOLUME_PATH }],
            // Fly Proxy terminates TLS on 443 and forwards HTTP (with the
            // WebSocket upgrade) to treq-gateway. autostart is off because
            // the ingress is public; Treq wakes through the API. autostop
            // is off: Fly Proxy closes open WebSockets to stop a machine, so
            // it would cut live sessions. The idle controller stops it.
            services: [{
              protocol: "tcp",
              internal_port: GATEWAY_PORT,
              autostop: "off",
              autostart: false,
              ports: [{ port: 443, handlers: ["tls", "http"] }],
            }],
            restart: { policy: "on-failure" },
            metadata: {
              treq_owner: params.ownerUserId,
              treq_placement: key,
              treq_manifest: String(params.manifestVersion),
            },
          },
        }),
      }, 60_000),
    );
    return normalizeInstance(machine);
  }

  private async getMachine(id: string): Promise<FlyMachine> {
    return await this.json<FlyMachine>(await this.request(this.machineUrl(id)));
  }

  async getInstance(providerId: string): Promise<ProviderInstance> {
    return normalizeInstance(await this.getMachine(providerId));
  }

  async wakeInstance(providerId: string): Promise<void> {
    const response = await this.request(this.machineUrl(providerId, "/start"), { method: "POST" }, 60_000);
    if (response.ok) return await expectOk(response, flyError);
    const err = await flyError(response);
    // UNVERIFIED: the status Fly returns for starting a started machine.
    if (err.kind === "invalid_request" || err.kind === "already_exists") {
      const state = normalizeFlyState((await this.getMachine(providerId)).state);
      if (state === "ready" || state === "waking") return;
    }
    throw err;
  }

  async stopInstance(providerId: string): Promise<void> {
    await expectOk(await this.request(this.machineUrl(providerId, "/stop"), { method: "POST" }, 60_000), flyError, [404]);
  }

  // No provider timer to push out: autostop is off.
  async keepAlive(_providerId: string): Promise<void> {}

  async replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance> {
    // The machine and its volume are the user's environment: repair keeps it.
    return await this.getInstance(params.providerResourceId);
  }

  async deleteInstance(providerId: string, opts: DeleteInstanceOptions = {}): Promise<void> {
    let machine: FlyMachine | null = null;
    try {
      machine = await this.getMachine(providerId);
    } catch (err) {
      if (!(err instanceof ProviderError && err.kind === "not_found")) throw err;
    }
    if (machine) {
      await expectOk(await this.request(this.machineUrl(providerId, "?force=true"), { method: "DELETE" }), flyError, [404]);
    }
    // The volume is billed until deleted. Find it by the machine's mounts and
    // by its deterministic name, so a retry after the machine is gone still
    // removes it.
    const volumeIds = new Set((machine?.config?.mounts ?? []).map((m) => m.volume));
    const key = opts.placementId ? placementKey(opts.placementId) : machine?.config?.metadata?.treq_placement;
    if (key) {
      const volumes = await this.json<FlyVolume[]>(await this.request(this.url("/volumes")));
      for (const v of volumes) {
        if (v.name === volumeNameFor(key) && v.state !== "destroyed") volumeIds.add(v.id);
      }
    }
    for (const id of volumeIds) await this.deleteVolume(id);
  }

  // The volume detaches asynchronously after its machine is destroyed, and a
  // DELETE before then fails with 409/412: retry with a bounded backoff.
  private async deleteVolume(id: string): Promise<void> {
    for (let attempt = 0;; attempt++) {
      const response = await this.request(this.url(`/volumes/${encodeURIComponent(id)}`), { method: "DELETE" });
      const retry = attempt < VOLUME_DELETE_BACKOFF_MS.length && [409, 412].includes(response.status);
      if (!retry) return await expectOk(response, flyError, [404]);
      await response.body?.cancel();
      await this.sleep(VOLUME_DELETE_BACKOFF_MS[attempt]);
    }
  }

  async execOnMachine(
    providerId: string,
    command: string[],
    timeoutSeconds = 20,
    environment: Readonly<Record<string, string>> = {},
  ): Promise<MachineExecResult> {
    assertExecArgs(command, environment);
    // argv, not a shell string, so `env NAME=value` needs no quoting.
    const envArgs = Object.entries(environment).map(([k, v]) => `${k}=${v}`);
    const timeout = Math.max(Math.ceil(timeoutSeconds), 1);
    const body = await this.json<{ exit_code?: number; stdout?: string; stderr?: string }>(
      await this.request(this.machineUrl(providerId, "/exec"), {
        method: "POST",
        body: JSON.stringify({ command: envArgs.length ? ["env", ...envArgs, ...command] : command, timeout }),
      }, (timeout + 30) * 1_000),
    );
    return { exitCode: body.exit_code ?? -1, stdout: body.stdout ?? "", stderr: body.stderr ?? "" };
  }

  // Machines exec runs as root. Chosen over Machine `processes`/init because
  // a config change there restarts the machine, and the gateway's args are
  // only known after create.
  async ensureService(providerId: string, name: string, spec: ServiceSpec): Promise<void> {
    await ensurePidfileService(
      (command, timeout) => this.execOnMachine(providerId, command, timeout),
      name,
      spec,
      { vendor: "Fly", dir: "/var/run/treq" },
    );
  }

  // All machines share `<app>.fly.dev`; the `fly-force-instance-id` header
  // pins the request to this machine (docs.fly.io/networking/dynamic-request-routing).
  // Fly ingress is public, so the gateway HMAC (`x-treq-gateway-token`,
  // added by the relay) is the auth.
  // UNVERIFIED: that fly-force-instance-id pins a WebSocket upgrade.
  async gatewayEndpoint(providerId: string): Promise<GatewayEndpoint> {
    const state = normalizeFlyState((await this.getMachine(providerId)).state);
    if (state === "suspended") await this.wakeInstance(providerId);
    return {
      url: `wss://${this.config.app}.fly.dev/`,
      headers: { "fly-force-instance-id": providerId },
    };
  }
}
