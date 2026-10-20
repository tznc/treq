/**
 * Webhook-drive a 2-stack + independent PR + 3-level stack (with a sibling),
 * then CI/merge until every queue for the repo is drained.
 */
import { expect, it } from "vitest";
import { buildQueueStacks, type QueueEntryStatus } from "../queue-stacks";
import { getServiceClient } from "../clients";
import { withMergeQueueFixture } from "../fixture";
import { recordOutcome } from "../record";
import { TWO_PLUS_INDEPENDENT_PLUS_THREE_WITH_SIBLING } from "../topologies";
import {
  drainMergeQueueWorker,
  drainQueueViaCiAndMerge,
  simulateEnqueueWorkspace,
} from "../webhook";

type StatusRow = {
  branch_name: string;
  pr_number: number;
  status: QueueEntryStatus;
  position: number;
  target_branch: string;
};

it("drains a 2-stack + independent PR + 3-stack with siblings via webhooks", async () => {
  await withMergeQueueFixture(async (fx) => {
    await fx.enableQueue();
    await fx.admin
      .from("merge_queue_configs")
      .update({ batch_size: 1, max_parallel_queues: 1 })
      .eq("repo_id", fx.linked.repoId);

    for (const pr of TWO_PLUS_INDEPENDENT_PLUS_THREE_WITH_SIBLING) {
      await simulateEnqueueWorkspace({
        installationId: fx.linked.installationId,
        repoId: fx.linked.repoId,
        fullName: fx.linked.fullName,
        pr,
      });
    }
    await drainMergeQueueWorker();

    const { data: afterEnqueue, error: listErr } = await fx.client.rpc(
      "get_repo_branch_queue_statuses",
      { p_repo_full_name: fx.linked.fullName },
    );
    expect(listErr).toBeNull();
    const rows = (afterEnqueue ?? []) as StatusRow[];
    expect(rows).toHaveLength(7);

    const stacks = buildQueueStacks(rows);
    const byKey = Object.fromEntries(
      stacks.map((s) => [
        s.entries.map((e) => e.branch_name).join(">"),
        s.entries.map((e) => e.branch_name),
      ]),
    );

    // Stack of 2 into main.
    expect(byKey["feat/two-base>feat/two-top"]).toEqual([
      "feat/two-base",
      "feat/two-top",
    ]);
    // Independent into main.
    expect(byKey["fix/solo"]).toEqual(["fix/solo"]);
    // Three-level chain via mid (lowest-position child wins at each fork).
    expect(byKey["feat/three-base>feat/three-mid>feat/three-top"]).toEqual([
      "feat/three-base",
      "feat/three-mid",
      "feat/three-top",
    ]);
    // Sibling of mid surfaces as its own block (target is still three-base).
    expect(byKey["feat/three-sib"]).toEqual(["feat/three-sib"]);
    expect(stacks).toHaveLength(4);

    await recordOutcome("service-qa-webhook-stacks-01-enqueued", {
      expectations: [
        "Seven labeled webhooks produced a 2-stack, an independent PR, a 3-stack, and a sibling block.",
        "buildQueueStacks groups feat/two-* as Stack of 2 and feat/three-base→mid→top as Stack of 3.",
        "feat/three-sib remains a sibling of mid (own block targeting feat/three-base).",
      ],
      details: { rows, stacks: byKey },
    });

    await drainQueueViaCiAndMerge({
      installationId: fx.linked.installationId,
      repoId: fx.linked.repoId,
      fullName: fx.linked.fullName,
      maxCycles: 40,
    });

    const admin = getServiceClient();
    const { data: active } = await admin
      .from("merge_queue_entries")
      .select("id, status, branch_name, merge_queues!inner(repo_id)")
      .eq("merge_queues.repo_id", fx.linked.repoId)
      .in("status", ["queued", "testing", "merging"]);
    expect(active ?? []).toHaveLength(0);

    const { data: finalRows } = await fx.client.rpc(
      "get_repo_branch_queue_statuses",
      { p_repo_full_name: fx.linked.fullName },
    );
    const finals = (finalRows ?? []) as StatusRow[];
    expect(finals).toHaveLength(7);
    expect(finals.every((r) => r.status === "merged")).toBe(true);

    await recordOutcome("service-qa-webhook-stacks-02-drained", {
      expectations: [
        "CI check_suite webhooks + stub merges drained every queue (main and stacked bases).",
        "All seven branches report merged — including the sibling of the three-stack mid.",
      ],
      details: { finalRows: finals },
    });
  });
}, 240_000);
