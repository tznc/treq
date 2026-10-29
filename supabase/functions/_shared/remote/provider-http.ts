// Helpers shared by the vendor adapters: HTTP calls mapped onto
// ProviderError, the exec-based service supervisor, and shell quoting.

import { fetchWithTimeout } from "../http/fetch-with-timeout.ts";
import type {
  MachineExecResult,
  ManagedInstanceState,
  ProviderInstance,
  ServiceSpec,
} from "./provider.ts";
import { ProviderError } from "./provider.ts";

/** fetch with a timeout; a transport failure becomes `timeout` or `unavailable`. */
export async function providerFetch(
  fetchImpl: typeof fetch,
  vendor: string,
  url: string,
  init: RequestInit,
  timeoutMs?: number,
): Promise<Response> {
  try {
    return await fetchWithTimeout(url, init, timeoutMs, fetchImpl);
  } catch (err) {
    throw new ProviderError(
      err instanceof DOMException && err.name === "TimeoutError" ? "timeout" : "unavailable",
      `could not reach ${vendor}: ${(err as Error).message}`,
    );
  }
}

/**
 * A vendor-specific status rule, tried before the common mapping. `text` is
 * the response body, `detail` it truncated to 500 characters.
 */
export type StatusOverride = (
  status: number,
  text: string,
  detail: string,
) => ProviderError | undefined;

/** Maps an error response onto ProviderError; `override` handles vendor exceptions. */
export async function providerErrorFrom(
  response: Response,
  override?: StatusOverride,
): Promise<ProviderError> {
  const text = await response.text().catch(() => "");
  const detail = text.length > 500 ? `${text.slice(0, 500)}…` : text;
  const special = override?.(response.status, text, detail);
  if (special) return special;
  switch (response.status) {
    case 404:
      return new ProviderError("not_found", "instance not found");
    case 409:
      return new ProviderError("already_exists", "instance already exists");
    case 401:
    case 403:
      return new ProviderError("unauthorized", "provider rejected the API key");
    // 402: plan or org limits; 429: rate limit.
    case 402:
    case 429:
      return new ProviderError("quota_exceeded", "provider quota exceeded");
    case 400:
    case 422:
      return new ProviderError("invalid_request", detail);
    case 408:
      return new ProviderError("timeout", detail);
    default:
      return new ProviderError(response.status >= 500 ? "unavailable" : "other", detail);
  }
}

/** Instance fields the adapters don't report: region and size are machine types now. */
export function providerInstance(
  providerResourceId: string,
  state: ManagedInstanceState,
  stateChangedAt: string | null = null,
  address: string | null = null,
): ProviderInstance {
  return { providerResourceId, state, region: "us_east", sizePreset: "small", address, stateChangedAt };
}

/** A label or metadata value: lowercase, `[a-z0-9._-]`, at most 63 characters. */
export function labelValue(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9._-]/g, "-").slice(0, 63);
}

/** POSIX single-quote each argument so a shell string runs argv as given. */
export function shellQuote(args: readonly string[]): string {
  return args.map((a) => `'${a.replaceAll("'", `'\\''`)}'`).join(" ");
}

export function assertExecArgs(
  command: readonly string[],
  environment: Readonly<Record<string, string>>,
  namePattern = /^[A-Za-z_][A-Za-z0-9_]*$/,
): void {
  if (command.length === 0) throw new ProviderError("invalid_request", "command is required");
  for (const name of Object.keys(environment)) {
    if (!namePattern.test(name)) throw new ProviderError("invalid_request", "invalid environment name");
  }
}

export interface PidfileServiceOptions {
  vendor: string;
  dir: string;
  /** Create `dir` with sudo and chown it to the exec user. */
  sudo?: boolean;
}

// No vendor here has a service API that restarts on a definition change, so
// a pidfile supervisor through exec stands in: stop the old process if its
// pid is alive, then start the new definition under nohup/setsid so it
// outlives the exec. Callers re-run it after a wake.
export async function ensurePidfileService(
  exec: (command: string[], timeoutSeconds: number) => Promise<MachineExecResult>,
  name: string,
  spec: ServiceSpec,
  { vendor, dir, sudo = false }: PidfileServiceOptions,
): Promise<void> {
  if (!/^[A-Za-z0-9_-]+$/.test(name)) throw new ProviderError("invalid_request", "invalid service name");
  const pid = `${dir}/${name}.pid`;
  const log = `${dir}/${name}.log`;
  const script = [
    sudo ? `sudo mkdir -p ${dir} && sudo chown "$(id -u)" ${dir}` : `mkdir -p ${dir}`,
    `if [ -f ${pid} ] && kill -0 "$(cat ${pid})" 2>/dev/null; then kill "$(cat ${pid})"; sleep 1; fi`,
    `nohup setsid ${shellQuote([spec.cmd, ...spec.args])} >${log} 2>&1 </dev/null & echo $! >${pid}`,
    `sleep 1; kill -0 "$(cat ${pid})"`,
  ].join("\n");
  const result = await exec(["sh", "-c", script], 60);
  if (result.exitCode !== 0) {
    throw new ProviderError(
      "other",
      `${vendor} service ${name} failed to start: ${(result.stderr || result.stdout).slice(0, 500)}`,
    );
  }
}

/** Throws the mapped error unless the response is ok or its status is tolerated; drains the body. */
export async function expectOk(
  response: Response,
  mapError: (response: Response) => Promise<ProviderError>,
  tolerate: readonly number[] = [],
): Promise<void> {
  if (!response.ok && !tolerate.includes(response.status)) throw await mapError(response);
  await response.body?.cancel();
}
