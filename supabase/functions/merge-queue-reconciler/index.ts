// Periodic reconciliation for the merge queue. Run every 5-15 minutes via
// Supabase Cron.
//
// The reconciler repairs drift by publishing corrective commands — it never
// performs GitHub side effects itself. Detected conditions:
//
//   - entries stuck in 'testing' with no active CI run → reset to queued
//   - CI runs stuck beyond CI_RUN_TIMEOUT → fail them via ci.completed
//   - expired execution leases → removed
//   - queues with queued entries, free capacity and no recent drive → drive
//   - entries stuck in 'merging' beyond the lease TTL → drive (the drive
//     handler re-runs merge with operation-key idempotency)

import { createClient, type SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { publishMergeQueueCommand } from "../_shared/merge-queue/commands-queue.ts";
import { isServiceRoleRequest } from "../_shared/auth/service-role.ts";
import type { MergeQueueCommand } from "../_shared/merge-queue/messages.ts";

const CI_RUN_TIMEOUT_MINUTES = 120;

function log(fields: Record<string, unknown>): void {
  console.log(JSON.stringify(fields));
}

function drive(queueId: string): MergeQueueCommand {
  return {
    version: 1,
    type: "queue.drive",
    commandId: crypto.randomUUID(),
    queueId,
    reason: "reconciliation",
    occurredAt: new Date().toISOString(),
  };
}

async function resetOrphanedTestingEntries(
  supabase: SupabaseClient,
  touchedQueues: Set<string>,
): Promise<number> {
  // Entries in 'testing' must belong to a pending/running CI run.
  const { data, error } = await supabase
    .from("merge_queue_entries")
    .select("id, queue_id, ci_run_entries(ci_run_id, ci_runs(status))")
    .eq("status", "testing");
  if (error) throw new Error(`failed to load testing entries: ${error.message}`);

  let repaired = 0;
  for (const entry of data ?? []) {
    // PostgREST returns the to-one ci_runs as an object; the generated type
    // says array.
    const memberships = (entry.ci_run_entries ?? []) as unknown as Array<{
      ci_runs: { status: string } | null;
    }>;
    const hasActiveRun = memberships.some(
      (m) => m.ci_runs && ["pending", "running"].includes(m.ci_runs.status),
    );
    if (hasActiveRun) continue;

    await supabase
      .from("merge_queue_entries")
      .update({ status: "queued", updated_at: new Date().toISOString() })
      .eq("id", entry.id)
      .eq("status", "testing");
    touchedQueues.add(entry.queue_id as string);
    repaired++;
    log({
      operation: "reconcile_orphaned_testing_entry",
      entry_id: entry.id,
      queue_id: entry.queue_id,
    });
  }
  return repaired;
}

async function failStuckCiRuns(
  supabase: SupabaseClient,
  touchedQueues: Set<string>,
): Promise<number> {
  const cutoff = new Date(
    Date.now() - CI_RUN_TIMEOUT_MINUTES * 60 * 1000,
  ).toISOString();
  const { data, error } = await supabase
    .from("ci_runs")
    .select("id, queue_id, head_sha, started_at")
    .in("status", ["pending", "running"])
    .lt("started_at", cutoff);
  if (error) throw new Error(`failed to load stuck runs: ${error.message}`);

  for (const run of data ?? []) {
    await publishMergeQueueCommand(supabase, {
      version: 1,
      type: "ci.completed",
      commandId: crypto.randomUUID(),
      deliveryId: `reconciler:${run.id}:${Date.now()}`,
      queueId: run.queue_id,
      ciRunId: run.id,
      testedSha: run.head_sha,
      conclusion: "timed_out",
      occurredAt: new Date().toISOString(),
    });
    touchedQueues.add(run.queue_id as string);
    log({
      operation: "reconcile_stuck_ci_run",
      ci_run_id: run.id,
      queue_id: run.queue_id,
      started_at: run.started_at,
    });
  }
  return (data ?? []).length;
}

async function clearExpiredLeases(supabase: SupabaseClient): Promise<number> {
  const { data, error } = await supabase
    .from("merge_queue_execution_leases")
    .delete()
    .lt("expires_at", new Date().toISOString())
    .select("queue_id");
  if (error) throw new Error(`failed to clear leases: ${error.message}`);
  for (const lease of data ?? []) {
    log({ operation: "reconcile_expired_lease", queue_id: lease.queue_id });
  }
  return (data ?? []).length;
}

async function driveIdleQueues(supabase: SupabaseClient): Promise<number> {
  // Queues with queued/merging entries deserve a drive; the drive handler
  // itself no-ops when capacity is exhausted, the queue is paused, or
  // nothing is runnable, so over-publishing here is harmless.
  const { data, error } = await supabase
    .from("merge_queue_entries")
    .select("queue_id")
    .in("status", ["queued", "merging"]);
  if (error) throw new Error(`failed to load queued entries: ${error.message}`);

  const queueIds = new Set((data ?? []).map((row) => row.queue_id as string));
  for (const queueId of queueIds) {
    await publishMergeQueueCommand(supabase, drive(queueId));
  }
  return queueIds.size;
}

Deno.serve(async (req) => {
  if (req.method !== "POST") {
    return new Response("Method not allowed", { status: 405 });
  }
  if (!isServiceRoleRequest(req, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "")) {
    return new Response("Forbidden", { status: 403 });
  }

  const supabase = createClient(
    Deno.env.get("SUPABASE_URL") ?? "",
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );

  try {
    const touchedQueues = new Set<string>();
    const expiredLeases = await clearExpiredLeases(supabase);
    const orphanedEntries = await resetOrphanedTestingEntries(supabase, touchedQueues);
    const stuckRuns = await failStuckCiRuns(supabase, touchedQueues);
    // Runs after the repairs above so newly re-queued entries are included.
    const driven = await driveIdleQueues(supabase);

    const summary = {
      operation: "reconciler_complete",
      expired_leases: expiredLeases,
      orphaned_testing_entries: orphanedEntries,
      stuck_ci_runs: stuckRuns,
      idle_queues_driven: driven,
    };
    log(summary);
    return new Response(JSON.stringify(summary), {
      headers: { "Content-Type": "application/json" },
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.error("[merge-queue-reconciler] failed:", message);
    return new Response(JSON.stringify({ error: message }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }
});
