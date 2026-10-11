// Pauses a merge queue whose GitHub App installation owner has no Pro
// (prds/billing-and-teams.md, "Enforcement": "The worker pauses the queue
// and comments once on each queued pull request").
//
// The worker checks entitlement before it tests or merges anything. When the
// owner has no Pro, it cancels every lane, so nothing tested before the
// pause merges after it, and puts those pull requests back in the queue.
// Each waiting pull request gets one comment. Entries stay queued, so the
// first drive after Pro returns tests them again; the reconciler drives
// every queue with queued entries, which is what resumes a paused queue.
//
// The notice is recorded under an operation key in
// merge_queue_command_executions, so later drives of a paused queue read
// only the database and never call GitHub. The same key is the comment
// marker, so a comment posted just before a crash is found, not repeated.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import type { CiRunStatus } from "./state-machine.ts";
import type { GitHubAdapter } from "./github-adapter.ts";
import {
  getActiveLanes,
  getCiRunEntries,
  updateCiRun,
} from "./ci-run-repository.ts";
import {
  operationAlreadySucceeded,
  recordOperationSuccess,
} from "./idempotency.ts";

/** Operation key and comment marker for one queued entry's pause notice. */
export function proPausedMarker(entryId: string): string {
  return `pro-paused:${entryId}`;
}

const PAUSE_NOTICE =
  "⏸️ **Treq Merge Queue**: The merge queue for this repository is paused because the owner of its Treq GitHub App installation no longer has Pro. This PR stays in the queue and is tested again once Pro is renewed.";

export interface EntitlementPauseStore {
  /** Lanes holding pull requests: pending, running, or passed but not merged. */
  activeLanes(queueId: string): Promise<Array<{ id: string; test_branch: string }>>;
  /** Marks the lane cancelled and puts its pull requests back in the queue. */
  cancelLane(laneId: string): Promise<void>;
  /** Entries still waiting to merge. */
  waitingEntries(queueId: string): Promise<Array<{ id: string; pr_number: number }>>;
  noticeRecorded(key: string): Promise<boolean>;
  recordNotice(key: string, queueId: string): Promise<void>;
}

export type PauseGitHub = Pick<
  GitHubAdapter,
  "deleteBranch" | "hasCommentWithMarker" | "createComment"
>;

export interface EntitlementGateDeps {
  /** installation_has_pro for the queue's installation. */
  hasPro(): Promise<boolean>;
  store: EntitlementPauseStore;
  /** Called only when there is GitHub work to do. */
  github(): Promise<PauseGitHub>;
  log(fields: Record<string, unknown>): void;
}

/**
 * Returns false when the owner has Pro and the queue may run. Otherwise
 * pauses the queue and returns true.
 */
export async function holdQueueWithoutPro(
  queue: { id: string },
  deps: EntitlementGateDeps,
): Promise<boolean> {
  if (await deps.hasPro()) return false;

  let gh: PauseGitHub | null = null;
  const github = async () => (gh ??= await deps.github());

  const lanes = await deps.store.activeLanes(queue.id);
  for (const lane of lanes) {
    await deps.store.cancelLane(lane.id);
    try {
      await (await github()).deleteBranch(lane.test_branch);
    } catch (err) {
      // The reconciler can remove a leftover test branch later.
      deps.log({
        operation: "branch_cleanup_failed",
        queue_id: queue.id,
        ci_run_id: lane.id,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  }

  let notified = 0;
  for (const entry of await deps.store.waitingEntries(queue.id)) {
    const marker = proPausedMarker(entry.id);
    if (await deps.store.noticeRecorded(marker)) continue;
    const client = await github();
    if (!(await client.hasCommentWithMarker(entry.pr_number, marker))) {
      await client.createComment(entry.pr_number, PAUSE_NOTICE, marker);
      notified++;
    }
    await deps.store.recordNotice(marker, queue.id);
  }

  deps.log({
    operation: "queue_paused_without_pro",
    queue_id: queue.id,
    lanes_cancelled: lanes.length,
    entries_notified: notified,
  });
  return true;
}

const HELD_LANE_STATUSES: CiRunStatus[] = ["pending", "running", "passed"];

/** The store above, on the merge queue tables. */
export function supabasePauseStore(
  supabase: SupabaseClient,
  workerId: string,
): EntitlementPauseStore {
  return {
    async activeLanes(queueId) {
      return await getActiveLanes(supabase, queueId, HELD_LANE_STATUSES);
    },
    async cancelLane(laneId) {
      await updateCiRun(supabase, laneId, {
        status: "cancelled",
        completedAt: new Date().toISOString(),
      });
      const runEntries = await getCiRunEntries(supabase, laneId);
      if (runEntries.length === 0) return;
      // A passed lane may hold entries already marked 'merging'. Merges are
      // recorded under operation keys, so re-testing one that did merge
      // only finds it merged.
      const { error } = await supabase
        .from("merge_queue_entries")
        .update({ status: "queued", updated_at: new Date().toISOString() })
        .in("id", runEntries.map((e) => e.entry_id))
        .in("status", ["testing", "merging"]);
      if (error) throw new Error(`failed to requeue entries: ${error.message}`);
    },
    async waitingEntries(queueId) {
      const { data, error } = await supabase
        .from("merge_queue_entries")
        .select("id, pr_number")
        .eq("queue_id", queueId)
        .in("status", ["queued", "testing", "merging"])
        .order("position", { ascending: true });
      if (error) throw new Error(`failed to load waiting entries: ${error.message}`);
      return (data ?? []) as Array<{ id: string; pr_number: number }>;
    },
    async noticeRecorded(key) {
      return await operationAlreadySucceeded(supabase, key);
    },
    async recordNotice(key, queueId) {
      await recordOperationSuccess(supabase, {
        operationKey: key,
        commandType: "github.pro_paused_notice",
        queueId,
        workerId,
      });
    },
  };
}
