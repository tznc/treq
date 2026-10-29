// Stops idle cloud workspaces, and keeps placement state in step with what
// the provider reports. Run by remote-admin's `enforce_idle` action every
// 5 minutes (.github/workflows/remote-maintenance.yml).
//
// Billing is wall-clock while a machine runs, so a workspace nobody uses
// must stop. Only human or agent activity keeps one awake (owner decision,
// option B): an open ssh session, an agent turn, a terminal session, or a
// process the user pinned. See activity-probe.ts for how each is detected.
//
// For each active placement:
//
// 1. Reconcile: read the provider's state. Providers now stop machines on
//    their own (the auto-stop backstop), so a machine they stopped is
//    recorded `suspended` and one running while we think it stopped is
//    recorded `ready`, both through set_placement_state so billing follows.
// 2. Probe a running machine. Activity moves last_active_at forward and
//    pushes the provider's auto-stop out (`keepAlive`).
// 3. Stop it once idle for TREQ_IDLE_TIMEOUT_MIN. A failed probe is not
//    activity, but a machine is stopped on failed probes only after
//    `maxProbeFailures` in a row, so a flaky exec does not kill a session.
//    Nothing is touched while a migration builds or cuts over to a
//    target (MIGRATION_EXEMPT_STATES); a parked or finishing migration
//    does not keep a workspace awake.
// 4. Record the probe in remote_instance_activity, which the app reads
//    through get_workspace_activity().

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import {
  type ActivityPin,
  parseProbeOutput,
  probeCommand,
  type ProbeSummary,
  summarizeProbe,
} from "./activity-probe.ts";
import { setPlacementState } from "./placement-store.ts";
import type { ProviderKind, SandboxProvider } from "./provider.ts";
import { must } from "./db.ts";

export const DEFAULT_IDLE_TIMEOUT_MIN = 15;
export const DEFAULT_MAX_PROBE_FAILURES = 3;
const PROBE_TIMEOUT_SEC = 30;

export function idleTimeoutMinutes(env: (key: string) => string | undefined): number {
  const value = Number(env("TREQ_IDLE_TIMEOUT_MIN"));
  return Number.isInteger(value) && value > 0 ? value : DEFAULT_IDLE_TIMEOUT_MIN;
}

// Migration states that hold the workspace as it is: the target is being
// built from it or cut over to. Not `requested`, `preflight` or
// `awaiting_ack` (nothing runs, or the user must answer) and not
// `source_retiring` (the source is stopped anyway).
export const MIGRATION_EXEMPT_STATES: ReadonlySet<string> = new Set([
  "target_provisioning",
  "target_bootstrapped",
  "verifying",
  "cutover",
]);

function migrationExempts(c: IdleCandidate): boolean {
  if (!c.migration_open) return false;
  // An older listing without the state exempts every open migration.
  return c.migration_state == null || MIGRATION_EXEMPT_STATES.has(c.migration_state);
}

// Our states in which the machine bills as running (set_placement_state).
const RUNNING = new Set(["ready", "waking", "degraded"]);

/** A row of public.remote_idle_candidates(). */
export interface IdleCandidate {
  instance_id: string;
  owner_user_id: string;
  placement_id: string;
  provider_kind: ProviderKind;
  provider_resource_id: string;
  state: string;
  /** What the controller saw last time: a machine that comes back restarts its idle clock. */
  last_state: "running" | "stopped" | null;
  last_active_at: string | null;
  probe_failures: number;
  migration_open: boolean;
  /** The open migration's state, when one is open. */
  migration_state?: string | null;
  /** When the controller last saw the machine (remote_instance_activity.probed_at). */
  probed_at?: string | null;
}

/** One remote_instance_activity row. `summary` is left unchanged when absent. */
export interface ActivityRecord {
  instance_id: string;
  owner_user_id: string;
  placement_id: string;
  last_state: "running" | "stopped";
  probed_at: string;
  probe_ok: boolean | null;
  probe_failures: number;
  last_active_at: string | null;
  idle_timeout_min: number;
  stopped_at?: string;
  summary?: ProbeSummary;
}

export interface IdleStore {
  candidates(): Promise<IdleCandidate[]>;
  /** Unexpired pins of the instance. */
  pins(instanceId: string): Promise<ActivityPin[]>;
  /** `at` backdates the billing transition (set_placement_state p_at); absent means now. */
  setPlacementState(placementId: string, state: string, at?: string): Promise<void>;
  recordActivity(record: ActivityRecord): Promise<void>;
}

export interface IdleDeps {
  store: IdleStore;
  providers: (
    kind: ProviderKind,
    placementId?: string | null,
  ) => Pick<SandboxProvider, "getInstance" | "execOnMachine" | "stopInstance"> & {
    keepAlive?: (providerId: string) => Promise<void>;
  };
  now: () => Date;
  log: (fields: Record<string, unknown>) => void;
  idleTimeoutMin: number;
  maxProbeFailures?: number;
}

export interface IdleResult {
  probed: string[];
  stopped: string[];
  skipped: string[];
  reconciled: Array<{ instance_id: string; from: string; to: string }>;
  failed: Array<{ instanceId: string; error: string }>;
}

export async function enforceIdle(deps: IdleDeps): Promise<IdleResult> {
  const result: IdleResult = { probed: [], stopped: [], skipped: [], reconciled: [], failed: [] };
  const maxFailures = deps.maxProbeFailures ?? DEFAULT_MAX_PROBE_FAILURES;
  const timeoutMs = deps.idleTimeoutMin * 60_000;

  for (const c of await deps.store.candidates()) {
    if (migrationExempts(c)) {
      result.skipped.push(c.instance_id);
      continue;
    }
    try {
      const provider = deps.providers(c.provider_kind, c.placement_id);
      const now = deps.now();
      const nowIso = now.toISOString();
      const base = {
        instance_id: c.instance_id,
        owner_user_id: c.owner_user_id,
        placement_id: c.placement_id,
        probed_at: nowIso,
        idle_timeout_min: deps.idleTimeoutMin,
      };

      // 1. Reconcile.
      // Billing follows the provider's transition time when the adapter
      // reports one. A stop without one is closed at the last probe, the
      // last time we saw it running; a start without one opens now.
      const seen = await provider.getInstance(c.provider_resource_id);
      const observed = seen.state;
      let ours = c.state;
      if (observed === "suspended" && RUNNING.has(ours)) {
        await deps.store.setPlacementState(c.placement_id, "suspended", seen.stateChangedAt ?? c.probed_at ?? undefined);
        result.reconciled.push({ instance_id: c.instance_id, from: ours, to: "suspended" });
        ours = "suspended";
      } else if (observed === "ready" && ours === "suspended") {
        await deps.store.setPlacementState(c.placement_id, "ready", seen.stateChangedAt ?? undefined);
        result.reconciled.push({ instance_id: c.instance_id, from: ours, to: "ready" });
        ours = "ready";
      } else if (observed !== "ready" && observed !== "suspended") {
        deps.log({ event: "idle_drift_unhandled", instance_id: c.instance_id, ours, observed });
        result.skipped.push(c.instance_id);
        continue;
      }
      if (ours === "suspended") {
        if (c.last_state !== "stopped") {
          await deps.store.recordActivity({
            ...base,
            last_state: "stopped",
            probe_ok: null,
            probe_failures: 0,
            last_active_at: c.last_active_at,
          });
        }
        continue;
      }

      // 2. Probe.
      const comingBack = c.last_state !== "running" || c.last_active_at === null;
      let lastActive = comingBack ? now.getTime() : Date.parse(c.last_active_at!);
      let summary: ProbeSummary | undefined;
      let failures = c.probe_failures;
      try {
        const exec = await provider.execOnMachine(c.provider_resource_id, probeCommand(), PROBE_TIMEOUT_SEC);
        if (exec.exitCode !== 0) throw new Error(`probe exited ${exec.exitCode}`);
        const pins = await deps.store.pins(c.instance_id);
        summary = summarizeProbe(parseProbeOutput(exec.stdout), pins, {
          now,
          recentSec: deps.idleTimeoutMin * 60,
        });
        failures = 0;
        if (summary.idle_sec !== null) {
          lastActive = Math.max(lastActive, now.getTime() - summary.idle_sec * 1000);
        }
        if (summary.active && provider.keepAlive) {
          await provider.keepAlive(c.provider_resource_id).catch((err) =>
            deps.log({ event: "idle_keepalive_failed", instance_id: c.instance_id, error: (err as Error).message })
          );
        }
      } catch (err) {
        failures += 1;
        deps.log({ event: "idle_probe_failed", instance_id: c.instance_id, failures, error: (err as Error).message });
      }
      result.probed.push(c.instance_id);

      // 3. Stop when idle.
      const idleFor = now.getTime() - lastActive;
      const stop = idleFor >= timeoutMs && (summary !== undefined || failures >= maxFailures);
      if (stop) {
        await provider.stopInstance(c.provider_resource_id);
        await deps.store.setPlacementState(c.placement_id, "suspended");
        result.stopped.push(c.instance_id);
        deps.log({ event: "idle_stopped", instance_id: c.instance_id, idle_min: Math.round(idleFor / 60_000), failures });
      }

      // 4. Heartbeat.
      await deps.store.recordActivity({
        ...base,
        last_state: stop ? "stopped" : "running",
        probe_ok: summary !== undefined,
        probe_failures: failures,
        last_active_at: new Date(lastActive).toISOString(),
        ...(stop ? { stopped_at: nowIso } : {}),
        ...(summary ? { summary } : {}),
      });
    } catch (err) {
      result.failed.push({ instanceId: c.instance_id, error: (err as Error).message });
    }
  }
  return result;
}

export function supabaseIdleStore(supabase: SupabaseClient): IdleStore {
  return {
    async candidates() {
      const data = must(await supabase.rpc("remote_idle_candidates"), "list idle candidates");
      const rows = (data ?? []) as IdleCandidate[];
      const probedAt = new Map<string, string>();
      if (rows.length > 0) {
        const activity = must(await supabase
          .from("remote_instance_activity")
          .select("instance_id, probed_at")
          .in("instance_id", rows.map((c) => c.instance_id)), "read last probe times");
        for (const a of activity ?? []) probedAt.set(a.instance_id as string, a.probed_at as string);
      }
      return rows.map((c) => ({
        ...c,
        probe_failures: Number(c.probe_failures ?? 0),
        probed_at: probedAt.get(c.instance_id) ?? null,
      }));
    },
    async pins(instanceId) {
      return (must(await supabase
        .from("remote_process_pins")
        .select("id, pid, command, pattern, expires_at")
        .eq("instance_id", instanceId)
        .gt("expires_at", new Date().toISOString()), "list pins") ?? []) as ActivityPin[];
    },
    setPlacementState: (placementId, state, at) => setPlacementState(supabase, placementId, state, undefined, at),
    async recordActivity(record) {
      must(await supabase
        .from("remote_instance_activity")
        .upsert({ ...record, updated_at: record.probed_at }, { onConflict: "instance_id" }), "record activity");
    },
  };
}
