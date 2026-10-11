// Fly Sprites provider adapter for the control plane, mirroring
// `core::remote_provider_sprites::SpritesProvider` in src-tauri. This is the
// Deno-side equivalent used by Edge Functions: same vendor API shape, same
// state normalization, same idempotency-header convention. Vendor status
// strings and vendor SDK types must never leave this module.

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

export interface ManagedComputeProvider {
  createInstance(params: CreateInstanceParams): Promise<ProviderInstance>;
  getInstance(providerId: string): Promise<ProviderInstance>;
  wakeInstance(providerId: string): Promise<void>;
  // Stops the machine and keeps its disk. Used when the owner's Pro ends
  // (`lapse-sweep.ts`).
  stopInstance(providerId: string): Promise<void>;
  replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance>;
  deleteInstance(providerId: string): Promise<void>;
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

export interface ServiceSpec {
  cmd: string;
  args: string[];
}

/// WebSocket URL of the Sprites TCP proxy for one Sprite
/// (`WSS /v1/sprites/{name}/proxy`, sprites.dev/api/sprites/proxy). The
/// caller authenticates with the org token in an Authorization header, then
/// sends `{"host","port"}` and waits for `{"status":"connected"}` before the
/// socket carries raw TCP bytes.
export function spritesProxyUrl(config: SpritesConfig, name: string): string {
  const url = new URL(
    `${config.baseUrl.replace(/\/+$/, "")}/v1/sprites/${encodeURIComponent(name)}/proxy`,
  );
  url.protocol = url.protocol === "http:" ? "ws:" : "wss:";
  return url.toString();
}

function normalizeState(vendorState: string): ManagedInstanceState {
  switch (vendorState) {
    case "creating":
      return "provisioning";
    case "warm":
    case "running":
      return "ready";
    case "cold":
    case "paused":
      return "suspended";
    default:
      return "degraded";
  }
}

// deno-lint-ignore no-explicit-any
function normalizeInstance(sprite: any): ProviderInstance {
  return {
    // Sprite names, rather than opaque UUIDs, address every lifecycle and
    // exec endpoint in the public API.
    providerResourceId: sprite.name,
    state: normalizeState(sprite.status),
    // Retained only while the provider-neutral database fields are migrated
    // to optional capabilities. Sprites does not accept either setting.
    region: "us_east",
    sizePreset: "small",
    address: null,
  };
}

export interface SpritesConfig {
  baseUrl: string;
  apiToken: string;
}

/// Reads Fly Sprites configuration from Edge Function secrets. Never logged;
/// never returned to a client.
export function spritesConfigFromEnv(): SpritesConfig {
  const baseUrl = Deno.env.get("TREQ_SPRITES_API_URL");
  const apiToken = Deno.env.get("TREQ_SPRITES_API_TOKEN");
  if (!baseUrl || !apiToken) {
    throw new ProviderError(
      "invalid_request",
      "TREQ_SPRITES_API_URL and TREQ_SPRITES_API_TOKEN must be set",
    );
  }
  return { baseUrl, apiToken };
}

export class SpritesProvider implements ManagedComputeProvider {
  constructor(private readonly config: SpritesConfig) {}

  private spritesUrl(): string {
    return `${this.config.baseUrl.replace(/\/+$/, "")}/v1/sprites`;
  }

  private spriteUrl(name: string): string {
    return `${this.spritesUrl()}/${encodeURIComponent(name)}`;
  }

  async execOnMachine(
    providerId: string,
    command: string[],
    timeoutSeconds = 20,
    environment: Readonly<Record<string, string>> = {},
  ): Promise<MachineExecResult> {
    if (command.length === 0) {
      throw new ProviderError("invalid_request", "command is required");
    }

    const url = new URL(`${this.spriteUrl(providerId)}/exec`);
    for (const arg of command) url.searchParams.append("cmd", arg);
    for (const [name, value] of Object.entries(environment)) {
      if (!/^[A-Z_][A-Z0-9_]*$/.test(name)) {
        throw new ProviderError("invalid_request", "invalid environment name");
      }
      url.searchParams.append("env", `${name}=${value}`);
    }
    url.searchParams.set("path", command[0]);

    try {
      const response = await fetch(url, {
        method: "POST",
        headers: this.headers(),
        signal: AbortSignal.timeout(timeoutSeconds * 1_000),
      });
      if (!response.ok) throw await this.mapErrorResponse(response);
      return { exitCode: 0, stdout: await response.text(), stderr: "" };
    } catch (err) {
      if (err instanceof ProviderError) throw err;
      throw new ProviderError(
        err instanceof DOMException && err.name === "TimeoutError"
          ? "timeout"
          : "unavailable",
        `Sprite exec failed: ${(err as Error).message}`,
      );
    }
  }

  async ensureService(
    providerId: string,
    name: string,
    spec: ServiceSpec,
  ): Promise<void> {
    const serviceUrl = `${this.spriteUrl(providerId)}/services/${encodeURIComponent(name)}`;
    try {
      const put = await fetch(serviceUrl, {
        method: "PUT",
        headers: this.headers(),
        body: JSON.stringify({ cmd: spec.cmd, args: spec.args }),
        signal: AbortSignal.timeout(30_000),
      });
      if (!put.ok) throw await this.mapErrorResponse(put);
      await put.body?.cancel();
      // `restart` rather than `start` so a repaired definition or rotated CA
      // file is picked up by an already-running sshd.
      const restart = await fetch(`${serviceUrl}/restart`, {
        method: "POST",
        headers: this.headers(),
        signal: AbortSignal.timeout(60_000),
      });
      if (!restart.ok) throw await this.mapErrorResponse(restart);
      // The response streams NDJSON progress; drain it so the service has
      // finished starting before the caller reads host keys.
      await restart.text();
    } catch (err) {
      if (err instanceof ProviderError) throw err;
      throw new ProviderError(
        err instanceof DOMException && err.name === "TimeoutError"
          ? "timeout"
          : "unavailable",
        `Sprite service setup failed: ${(err as Error).message}`,
      );
    }
  }

  private headers(): HeadersInit {
    return {
      Authorization: `Bearer ${this.config.apiToken}`,
      "Content-Type": "application/json",
    };
  }

  private async mapErrorResponse(response: Response): Promise<ProviderError> {
    const text = await response.text().catch(() => "");
    const truncated = text.length > 500 ? `${text.slice(0, 500)}…` : text;
    switch (response.status) {
      case 404:
        return new ProviderError("not_found", "instance not found");
      case 409:
        return new ProviderError("already_exists", "instance already exists");
      case 429:
        return new ProviderError("quota_exceeded", "provider quota exceeded");
      case 400:
      case 422:
        return new ProviderError("invalid_request", truncated);
      default:
        if (response.status >= 500)
          return new ProviderError("unavailable", truncated);
        return new ProviderError("other", truncated);
    }
  }

  async createInstance(
    params: CreateInstanceParams,
  ): Promise<ProviderInstance> {
    const name = `dev-treq-${params.ownerUserId}`
      .toLowerCase()
      .replace(/[^a-z0-9-]/g, "-")
      .slice(0, 63);
    const body = { name };

    let response: Response;
    try {
      response = await fetch(this.spritesUrl(), {
        method: "POST",
        headers: this.headers(),
        body: JSON.stringify(body),
      });
    } catch (err) {
      throw new ProviderError(
        "unavailable",
        `could not reach Sprites API: ${(err as Error).message}`,
      );
    }

    if (response.status === 409) {
      // A repeated create with the same idempotency key/machine name: treat
      // the vendor's existing-resource response as success rather than an
      // error, so create stays idempotent for the caller.
      return await this.getInstance(name);
    }
    if (!response.ok) throw await this.mapErrorResponse(response);
    return normalizeInstance(await response.json());
  }

  async getInstance(providerId: string): Promise<ProviderInstance> {
    let response: Response;
    try {
      response = await fetch(this.spriteUrl(providerId), {
        headers: this.headers(),
      });
    } catch (err) {
      throw new ProviderError(
        "unavailable",
        `could not reach Sprites API: ${(err as Error).message}`,
      );
    }
    if (!response.ok) throw await this.mapErrorResponse(response);
    return normalizeInstance(await response.json());
  }

  async wakeInstance(providerId: string): Promise<void> {
    // Sprites have no explicit wake endpoint. A paused Sprite resumes when
    // work reaches it: an exec/console command, a TTY session, or a request
    // to its URL (docs.fly.io/sprites/concepts/lifecycle and "Idle
    // Detection" in docs.fly.io/sprites/working-with-sprites). `GET
    // /v1/sprites/{name}` only reads org-level metadata, including the
    // cold/warm status itself, so it is not activity and does not wake the
    // VM. A no-op exec is the cheapest documented operation that does. The
    // Sprite pauses again about 30 seconds after the exec ends unless the
    // SSH session that follows keeps it active.
    await this.execOnMachine(providerId, ["true"]);
  }

  async stopInstance(_providerId: string): Promise<void> {
    // Sprites have no stop endpoint, and need none: a Sprite pauses on its
    // own about 30 seconds after the last exec or connection ends, and its
    // filesystem persists while paused (see wakeInstance). Once the owner
    // has no Pro, nothing reaches it again: remote-instance refuses `wake`
    // and `reprovision`, and remote-ssh-relay refuses connections. A relay
    // connection already open ends at the Edge Function wall-clock limit.
  }

  async replaceInstance(
    params: ReplaceInstanceParams,
  ): Promise<ProviderInstance> {
    // Sprites are durable user environments. Repairing the Treq bootstrap
    // preserves the Sprite and its filesystem; deletion remains an explicit
    // user action handled by deleteInstance.
    return await this.getInstance(params.providerResourceId);
  }

  async deleteInstance(providerId: string): Promise<void> {
    let response: Response;
    try {
      response = await fetch(this.spriteUrl(providerId), {
        method: "DELETE",
        headers: this.headers(),
      });
    } catch (err) {
      throw new ProviderError(
        "unavailable",
        `could not reach Sprites API: ${(err as Error).message}`,
      );
    }
    if (response.ok || response.status === 404) return;
    throw await this.mapErrorResponse(response);
  }
}
