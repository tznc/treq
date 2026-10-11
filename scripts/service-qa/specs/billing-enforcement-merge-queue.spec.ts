/**
 * Pro enforcement on the merge queue against the local database:
 *
 * - `set_merge_queue_enabled` and direct writes to merge_queue_configs,
 *   through real PostgREST with the user's session (PT402 → HTTP 402).
 * - The worker's pause and resume: `handleCommand` from
 *   `_shared/merge-queue/command-handler.ts` runs in this process with the
 *   service-role client and the real `installation_has_pro` RPC. GitHub is
 *   the in-memory StubGitHubAdapter (as with MERGE_QUEUE_GITHUB_STUB=1),
 *   kept across commands so its comments persist. Published follow-up
 *   commands are collected rather than sent to PGMQ.
 *
 * Pro comes from the billing write functions (../billing.ts).
 */
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import {
  installationHasPro,
  PRO_REQUIRED_MESSAGES,
} from "../../../supabase/functions/_shared/billing/entitlement";
import {
  authorizeQueueAction,
  queueAccessStore,
} from "../../../supabase/functions/enqueue-workspace/lib";
import { handleCommand } from "../../../supabase/functions/_shared/merge-queue/command-handler";
import { proPausedMarker } from "../../../supabase/functions/_shared/merge-queue/entitlement-pause";
import type { MergeQueueCommand } from "../../../supabase/functions/_shared/merge-queue/messages";
import { StubGitHubAdapter } from "../../../supabase/functions/_shared/merge-queue/stub-github-adapter";
import { endPro, grantPro } from "../billing";
import { withMergeQueueFixture } from "../fixture";
import { recordOutcome } from "../record";

it("refuses to turn the merge queue on without Pro, through the RPC and the table", async () => {
  await withMergeQueueFixture(
    async (fx) => {
      const refused = await fx.client.rpc("set_merge_queue_enabled", {
        p_repo_full_name: fx.linked.fullName,
        p_enabled: true,
      });
      expect(refused.status).toBe(402);
      expect(refused.error).toMatchObject({
        code: "PT402",
        message: "The merge queue needs Pro",
        hint: "pro_required",
      });

      const off = await fx.client.rpc("set_merge_queue_enabled", {
        p_repo_full_name: fx.linked.fullName,
        p_enabled: false,
      });
      expect(off.error).toBeNull();
      expect(off.data).toBe(false);

      const direct = await fx.client
        .from("merge_queue_configs")
        .update({ enabled: true })
        .eq("repo_id", fx.linked.repoId)
        .select("enabled");
      expect(direct.status).toBe(402);
      expect(direct.error?.code).toBe("PT402");

      const enabled = await fx.client.rpc("get_merge_queue_enabled", {
        p_repo_full_name: fx.linked.fullName,
      });
      expect(enabled.data).toBe(false);

      // The same user with Pro.
      await grantPro(fx.admin, fx.user.id);
      const allowed = await fx.client.rpc("set_merge_queue_enabled", {
        p_repo_full_name: fx.linked.fullName,
        p_enabled: true,
      });
      expect(allowed.error).toBeNull();
      expect(allowed.data).toBe(true);

      await recordOutcome("billing-enforcement-merge-queue-01-opt-in", {
        expectations: [
          "set_merge_queue_enabled(true) from a Free user's session answers HTTP 402 with code PT402 and hint pro_required; turning it off still works.",
          "A Free user's direct UPDATE of merge_queue_configs.enabled through PostgREST is also refused with 402, and the queue reads as off.",
          "After a Pro subscription is recorded, the same RPC returns true.",
        ],
        details: {
          refused: { status: refused.status, error: refused.error },
          direct: { status: direct.status, code: direct.error?.code },
        },
      });
    },
    { pro: false },
  );
}, 120_000);

it("lets enqueue-workspace label a PR only for a Pro installation owner", async () => {
  await withMergeQueueFixture(
    async (fx) => {
      const store = queueAccessStore(fx.admin);
      const ask = (action: "enqueue" | "dequeue", userId = fx.user.id) =>
        authorizeQueueAction(
          { userId, repoFullName: fx.linked.fullName, action },
          store,
        );

      const freeEnqueue = await ask("enqueue");
      expect(freeEnqueue).toEqual({
        ok: false,
        status: 402,
        body: { error: PRO_REQUIRED_MESSAGES.mergeQueue, code: "pro_required" },
      });
      const freeDequeue = await ask("dequeue");
      expect(freeDequeue).toMatchObject({ ok: true, repo: { id: fx.linked.repoId } });
      expect(await ask("enqueue", randomUUID())).toMatchObject({ ok: false, status: 403 });

      await grantPro(fx.admin, fx.user.id);
      const proEnqueue = await ask("enqueue");
      expect(proEnqueue).toMatchObject({ ok: true, repo: { id: fx.linked.repoId } });

      await recordOutcome("billing-enforcement-merge-queue-04-enqueue-workspace", {
        expectations: [
          "enqueue-workspace refuses to enqueue for a Free installation owner with 402 pro_required, before any GitHub call.",
          "Dequeue still passes for the Free owner, and another user is refused with 403.",
          "Once the owner has Pro, enqueue passes the access check.",
        ],
        details: { freeEnqueue, freeDequeue: freeDequeue.ok, proEnqueue: proEnqueue.ok },
      });
    },
    { pro: false },
  );
}, 120_000);

it("pauses a running queue when Pro ends, comments once, and resumes when Pro returns", async () => {
  await withMergeQueueFixture(async (fx) => {
    await fx.enableQueue();
    await fx.admin
      .from("merge_queue_configs")
      .update({ batch_size: 2, max_parallel_queues: 1 })
      .eq("repo_id", fx.linked.repoId);
    const a = await fx.seedQueuedEntry({ branchName: "feat/a", prNumber: 21, prSha: "a".repeat(40), position: 1 });
    const b = await fx.seedQueuedEntry({ branchName: "feat/b", prNumber: 22, prSha: "b".repeat(40), position: 2 });
    const queueId = a.queueId;

    const workerId = randomUUID();
    const github = new StubGitHubAdapter(fx.admin, fx.linked.repoId, fx.linked.defaultBranch);
    const published: MergeQueueCommand[] = [];
    const run = async (command: MergeQueueCommand) => {
      const { data: lease, error } = await fx.admin.rpc("acquire_merge_queue_lease", {
        p_queue_id: queueId,
        p_worker_id: workerId,
        p_ttl_seconds: 120,
      });
      if (error || !lease) throw new Error(`lease: ${error?.message ?? "held"}`);
      try {
        return await handleCommand(
          {
            supabase: fx.admin,
            workerId,
            leaseToken: lease as string,
            adapterFor: async () => github,
            installationHasPro: (id) => installationHasPro(fx.admin, id),
            publish: async (cmd) => {
              published.push(cmd);
            },
            log: () => {},
          },
          command,
        );
      } finally {
        await fx.admin.rpc("release_merge_queue_lease", {
          p_queue_id: queueId,
          p_lease_token: lease,
        });
      }
    };
    const drive = (): MergeQueueCommand => ({
      version: 1,
      type: "queue.drive",
      commandId: randomUUID(),
      queueId,
      reason: "reconciliation",
      occurredAt: new Date().toISOString(),
    });
    const entryStatuses = async () => {
      const { data } = await fx.admin
        .from("merge_queue_entries")
        .select("pr_number, status")
        .eq("queue_id", queueId)
        .order("pr_number");
      return data;
    };
    const lanes = async () => {
      const { data } = await fx.admin
        .from("ci_runs")
        .select("id, status, head_sha")
        .eq("queue_id", queueId)
        .order("created_at");
      return data ?? [];
    };
    const pauseNotices = async (pr: number, entryId: string) =>
      (await github.hasCommentWithMarker(pr, proPausedMarker(entryId))) ? 1 : 0;

    // ── With Pro: a lane starts ──────────────────────────────────────────
    const started = await run(drive());
    expect(started).toEqual({ outcome: "applied", detail: "1 lane(s) started" });
    const [lane] = await lanes();
    expect(lane.status).toBe("running");
    expect(await entryStatuses()).toEqual([
      { pr_number: 21, status: "testing" },
      { pr_number: 22, status: "testing" },
    ]);

    // ── Pro ends: the next drive pauses the queue ────────────────────────
    await endPro(fx.admin, fx.pro!);
    const paused = await run(drive());
    expect(paused).toEqual({
      outcome: "already_satisfied",
      detail: "paused: the installation owner has no Pro",
    });
    expect((await lanes()).map((l) => l.status)).toEqual(["cancelled"]);
    expect(await entryStatuses()).toEqual([
      { pr_number: 21, status: "queued" },
      { pr_number: 22, status: "queued" },
    ]);
    expect(await pauseNotices(21, a.entryId)).toBe(1);
    expect(await pauseNotices(22, b.entryId)).toBe(1);

    // CI finishing for the cancelled lane changes nothing.
    const lateCi = await run({
      version: 1,
      type: "ci.completed",
      commandId: randomUUID(),
      deliveryId: `sqa-${randomUUID()}`,
      queueId,
      ciRunId: lane.id,
      testedSha: lane.head_sha,
      conclusion: "success",
      occurredAt: new Date().toISOString(),
    });
    expect(lateCi.outcome).toBe("obsolete");

    // Later drives while paused: no new comments.
    // Every comment the stub holds, pause notice or not.
    const stubComments = Reflect.get(github, "comments") as Map<number, string[]>;
    const commentCount = (pr: number) => (stubComments.get(pr) ?? []).length;
    const before = [commentCount(21), commentCount(22)];
    await run(drive());
    await run(drive());
    expect([commentCount(21), commentCount(22)]).toEqual(before);
    const { data: notices } = await fx.admin
      .from("merge_queue_command_executions")
      .select("operation_key")
      .eq("queue_id", queueId)
      .eq("command_type", "github.pro_paused_notice");
    expect((notices ?? []).map((n) => n.operation_key).sort()).toEqual(
      [proPausedMarker(a.entryId), proPausedMarker(b.entryId)].sort(),
    );

    await recordOutcome("billing-enforcement-merge-queue-02-pause", {
      expectations: [
        "After the owner's subscription is deleted, the next queue.drive cancels the running lane and puts both PRs back to queued.",
        "Each queued PR gets exactly one pause comment, recorded under a pro-paused operation key; later drives add none.",
        "A ci.completed for the cancelled lane is obsolete, so nothing merges while paused.",
      ],
      details: { paused, lateCi, notices },
    });

    // ── Pro returns: the next drive resumes ──────────────────────────────
    await grantPro(fx.admin, fx.user.id, { customerId: fx.pro!.customerId });
    const resumed = await run(drive());
    expect(resumed).toEqual({ outcome: "applied", detail: "1 lane(s) started" });
    const after = await lanes();
    expect(after.map((l) => l.status)).toEqual(["cancelled", "running"]);
    expect(await entryStatuses()).toEqual([
      { pr_number: 21, status: "testing" },
      { pr_number: 22, status: "testing" },
    ]);

    await recordOutcome("billing-enforcement-merge-queue-03-resume", {
      expectations: [
        "Once a new Pro subscription is recorded, the next queue.drive starts a fresh lane with both PRs in testing.",
      ],
      details: { resumed, lanes: after },
    });
  });
}, 120_000);
