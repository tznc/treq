// Runs sandbox migrations under a lease, so the hourly admin job and an
// inline resize request make progress on the same migration without ever
// running it twice at once. Each run is bounded in time and steps; whatever
// is left is picked up by the next run.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent } from "./audit.ts";
import { supabaseMachineSetup } from "./machine-setup.ts";
import { type EngineDeps, runMigration, TERMINAL_STATES } from "./migration-engine.ts";
import { spendStatusFor } from "../billing/spend-cap.ts";
import {
  claimMigrations,
  releaseMigration,
  renewMigrationLease,
  supabaseMigrationStore,
} from "./placement-store.ts";
import type { ProviderKind, SandboxProvider } from "./provider.ts";

// The resize request runs the engine inline for about this long.
export const INLINE_BUDGET_MS = 55_000;
// A lease outlives one run's budget, so a crashed worker's lease expires
// soon after and the next run takes over. Before each step the engine
// extends it to cover that step's worst case (bootstrap and re-clone execs
// run for minutes), and every state change requires the lease
// (advance_migration_leased), so two workers never run one step.
const LEASE_SEC = 120;

export interface RunnerOptions {
  supabase: SupabaseClient;
  providers: (kind: ProviderKind, placementId?: string | null) => SandboxProvider;
  stub: boolean;
  correlationId: string;
  leaseOwner: string;
  budgetMs: number;
  limit: number;
  migrationId?: string;
  now?: () => number;
}

export interface RunnerResult {
  migrations: Array<{ id: string; state: string; steps: number }>;
}

export function engineDeps(
  o: Pick<RunnerOptions, "supabase" | "providers" | "stub" | "correlationId" | "now"> & { leaseOwner?: string },
): EngineDeps {
  const owner = o.leaseOwner;
  return {
    store: supabaseMigrationStore(o.supabase, { leaseOwner: owner }),
    providers: o.providers,
    setup: supabaseMachineSetup(o.supabase, { stub: o.stub, correlationId: o.correlationId, providers: o.providers }),
    now: o.now ?? Date.now,
    log: (fields) => console.log(JSON.stringify({ correlation_id: o.correlationId, ...fields })),
    renewLease: owner ? (id, seconds) => renewMigrationLease(o.supabase, id, owner, seconds) : undefined,
    spendCapped: async (userId) => (await spendStatusFor(o.supabase, userId)).state === "capped",
  };
}

export async function runLeasedMigrations(o: RunnerOptions): Promise<RunnerResult> {
  const now = o.now ?? Date.now;
  const deadline = now() + o.budgetMs;
  const ids = await claimMigrations(o.supabase, {
    owner: o.leaseOwner,
    leaseSec: LEASE_SEC,
    limit: o.limit,
    migrationId: o.migrationId,
  });
  const deps = engineDeps(o);
  const out: RunnerResult = { migrations: [] };
  for (const id of ids) {
    if (now() >= deadline) {
      await releaseMigration(o.supabase, id, o.leaseOwner);
      continue;
    }
    try {
      const r = await runMigration(deps, id, { deadlineMs: deadline, maxSteps: 20 });
      out.migrations.push({ id, ...r });
      if (TERMINAL_STATES.has(r.state)) {
        const m = await deps.store.getMigration(id);
        if (m) {
          await recordAuditEvent(o.supabase, {
            ownerUserId: m.owner_user_id,
            instanceId: m.instance_id,
            eventType: r.state === "completed"
              ? "migration_completed"
              : r.state === "failed"
              ? "migration_failed"
              : "migration_rolled_back",
            detail: { migration_id: id, reason: m.reason, target_machine_type_id: m.target_machine_type_id },
            correlationId: o.correlationId,
            severity: r.state === "completed" ? "info" : r.state === "failed" ? "error" : "warning",
          });
        }
      }
    } catch (err) {
      out.migrations.push({ id, state: "error", steps: 0 });
      console.error(`migration ${id} step failed: ${(err as Error).message}`);
    } finally {
      await releaseMigration(o.supabase, id, o.leaseOwner);
    }
  }
  return out;
}
