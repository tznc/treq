// The remote-instance process actions behind the app's background-process
// list (get_workspace_activity):
//
// * `pin_process {instance_id, pid | pattern, ttl_hours?}`: keeps the
//   workspace awake while a matching process runs, until the pin expires
//   (default 8 h, at most 72 h). Pins live in remote_process_pins and the
//   idle controller matches them against each probe. A pid pin records the
//   command seen in the last probe, so a recycled pid does not match.
// * `unpin_process {instance_id, pin_id}`.
// * `kill_process {instance_id, pid}`: SIGTERM to a process listed by the
//   last probe. Managed services (sshd) are refused. On the machine the
//   pid is signalled only if its /proc cmdline still matches the probe's
//   command and no /var/run/treq/*.pid names it, so a recycled pid is safe.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import type { ProbeSummary } from "./activity-probe.ts";
import { getInstanceById } from "./instance-store.ts";
import type { ProviderKind, SandboxProvider } from "./provider.ts";
import { must } from "./db.ts";

export const PIN_BILLING_NOTE =
  "A pinned process keeps your cloud workspace running while it runs, and running time is billed.";
export const DEFAULT_PIN_TTL_HOURS = 8;
export const MAX_PIN_TTL_HOURS = 72;
const MAX_PATTERN = 200;

type Parsed<T> = { ok: true; value: T } | { ok: false; error: string };

function pidOf(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) && value > 1 ? value : null;
}

function instanceIdOf(body: Record<string, unknown>): string | null {
  return typeof body.instance_id === "string" && body.instance_id.length > 0 ? body.instance_id : null;
}

export function parsePinRequest(
  body: Record<string, unknown>,
): Parsed<{ instanceId: string; pid: number | null; pattern: string | null; ttlHours: number }> {
  const instanceId = instanceIdOf(body);
  if (!instanceId) return { ok: false, error: "instance_id is required" };
  if ((body.pid === undefined) === (body.pattern === undefined)) {
    return { ok: false, error: "exactly one of pid or pattern is required" };
  }
  let pid: number | null = null;
  let pattern: string | null = null;
  if (body.pid !== undefined) {
    pid = pidOf(body.pid);
    if (pid === null) return { ok: false, error: "pid must be an integer above 1" };
  } else {
    if (typeof body.pattern !== "string" || body.pattern.length === 0 || body.pattern.length > MAX_PATTERN) {
      return { ok: false, error: `pattern must be 1 to ${MAX_PATTERN} characters` };
    }
    pattern = body.pattern;
  }
  const ttl = body.ttl_hours ?? DEFAULT_PIN_TTL_HOURS;
  if (typeof ttl !== "number" || !Number.isInteger(ttl) || ttl < 1 || ttl > MAX_PIN_TTL_HOURS) {
    return { ok: false, error: `ttl_hours must be an integer from 1 to ${MAX_PIN_TTL_HOURS}` };
  }
  return { ok: true, value: { instanceId, pid, pattern, ttlHours: ttl } };
}

export function parseUnpinRequest(body: Record<string, unknown>): Parsed<{ instanceId: string; pinId: string }> {
  const instanceId = instanceIdOf(body);
  if (!instanceId) return { ok: false, error: "instance_id is required" };
  if (typeof body.pin_id !== "string" || body.pin_id.length === 0) return { ok: false, error: "pin_id is required" };
  return { ok: true, value: { instanceId, pinId: body.pin_id } };
}

export function parseKillRequest(body: Record<string, unknown>): Parsed<{ instanceId: string; pid: number }> {
  const instanceId = instanceIdOf(body);
  if (!instanceId) return { ok: false, error: "instance_id is required" };
  const pid = pidOf(body.pid);
  if (pid === null) return { ok: false, error: "pid must be an integer above 1" };
  return { ok: true, value: { instanceId, pid } };
}

export function checkKill(
  summary: ProbeSummary | null,
  pid: number,
): { ok: true; command: string } | { ok: false; error: string } {
  const process = summary?.processes.find((p) => p.pid === pid);
  if (!process) return { ok: false, error: "process not found in the last probe" };
  if (process.origin === "service") return { ok: false, error: "managed services cannot be killed" };
  return { ok: true, command: process.command };
}

export const KILL_EXPECTED_ENV = "TREQ_KILL_EXPECTED";

// Exit 3: a managed service's pidfile names the pid. Exit 2: the pid now
// runs something else. Exit 1: no such process. The expected command comes
// in the environment so it is never parsed as shell.
const KILL_SCRIPT = `pid="$1"
for f in /var/run/treq/*.pid; do
  [ -f "$f" ] && [ "$(cat "$f" 2>/dev/null)" = "$pid" ] && exit 3
done
[ -r "/proc/$pid/cmdline" ] || exit 1
actual="$(tr '\\000' ' ' < "/proc/$pid/cmdline" | sed 's/ $//')"
[ "$actual" = "$${KILL_EXPECTED_ENV}" ] || exit 2
kill -TERM "$pid" || exit 1
`;

export function killCommand(pid: number, expected: string): { cmd: string[]; env: Record<string, string> } {
  return { cmd: ["/bin/sh", "-c", KILL_SCRIPT, "sh", String(pid)], env: { [KILL_EXPECTED_ENV]: expected } };
}

export function killExitError(exitCode: number): string {
  if (exitCode === 3) return "managed services cannot be killed";
  if (exitCode === 2) return "process changed since the last probe";
  return "process is no longer running";
}

export interface ProcessActionDeps {
  supabase: SupabaseClient;
  ownerUserId: string;
  provider: (kind: ProviderKind, placementId?: string | null) => SandboxProvider;
  stub: boolean;
}

type Reply = { status: number; body: Record<string, unknown> };

const invalid = (error: string): Reply => ({ status: 400, body: { error, code: "validation_error" } });
const notFound: Reply = { status: 404, body: { error: "Instance not found", code: "not_found" } };

async function lastSummary(supabase: SupabaseClient, instanceId: string): Promise<ProbeSummary | null> {
  const data = must(await supabase
    .from("remote_instance_activity")
    .select("summary")
    .eq("instance_id", instanceId)
    .maybeSingle(), "read activity");
  return (data?.summary ?? null) as ProbeSummary | null;
}

export async function handleProcessAction(
  action: "pin_process" | "unpin_process" | "kill_process",
  body: Record<string, unknown>,
  deps: ProcessActionDeps,
): Promise<Reply> {
  const { supabase, ownerUserId } = deps;
  const ownInstance = async (id: string) => {
    const instance = await getInstanceById(supabase, ownerUserId, id);
    return instance && instance.status !== "deleted" ? instance : null;
  };

  if (action === "pin_process") {
    const req = parsePinRequest(body);
    if (!req.ok) return invalid(req.error);
    const instance = await ownInstance(req.value.instanceId);
    if (!instance) return notFound;
    const { pid, pattern, ttlHours } = req.value;
    const command = pid === null
      ? null
      : (await lastSummary(supabase, instance.id))?.processes.find((p) => p.pid === pid)?.command ?? null;
    const expiresAt = new Date(Date.now() + ttlHours * 3_600_000).toISOString();
    const data = must(await supabase
      .from("remote_process_pins")
      .insert({ instance_id: instance.id, owner_user_id: ownerUserId, pid, command, pattern, expires_at: expiresAt })
      .select("id, pid, command, pattern, expires_at")
      .single(), "pin");
    return { status: 200, body: { pin: data, note: PIN_BILLING_NOTE } };
  }

  if (action === "unpin_process") {
    const req = parseUnpinRequest(body);
    if (!req.ok) return invalid(req.error);
    const instance = await ownInstance(req.value.instanceId);
    if (!instance) return notFound;
    const data = must(await supabase
      .from("remote_process_pins")
      .delete()
      .eq("id", req.value.pinId)
      .eq("instance_id", instance.id)
      .select("id"), "unpin");
    return { status: 200, body: { unpinned: (data ?? []).length > 0 } };
  }

  const req = parseKillRequest(body);
  if (!req.ok) return invalid(req.error);
  const instance = await ownInstance(req.value.instanceId);
  if (!instance || !instance.provider_resource_id) return notFound;
  if (instance.status !== "ready") {
    return { status: 409, body: { error: "workspace is not running", code: "not_running" } };
  }
  const check = checkKill(await lastSummary(supabase, instance.id), req.value.pid);
  if (!check.ok) return { status: 409, body: { error: check.error, code: "not_killable" } };
  if (deps.stub) return { status: 200, body: { killed: req.value.pid } };
  const kill = killCommand(req.value.pid, check.command);
  const result = await deps.provider(instance.provider_kind as ProviderKind, instance.active_placement_id).execOnMachine(
    instance.provider_resource_id,
    kill.cmd,
    15,
    kill.env,
  );
  if (result.exitCode !== 0) {
    const code = result.exitCode === 3 ? "not_killable" : "not_running";
    return { status: 409, body: { error: killExitError(result.exitCode), code } };
  }
  return { status: 200, body: { killed: req.value.pid } };
}
