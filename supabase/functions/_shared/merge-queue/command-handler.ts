// Coordinates the execution of one merge queue command. The caller (worker)
// owns message claiming, lease acquisition, retry/dead-letter policy and
// archival; this module owns the domain logic of each command type.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import type {
  CiCompletedCommand,
  EntryDequeueCommand,
  EntryEnqueueCommand,
  InstallationSyncCommand,
  MergeQueueCommand,
  QueueDriveCommand,
} from "./messages.ts";
import { mergePrOperationKey, startLaneOperationKey } from "./messages.ts";
import { RetryableError, TerminalError } from "./errors.ts";
import type { GitHubAdapter } from "./github-adapter.ts";
import {
  operationAlreadySucceeded,
  recordOperationSuccess,
} from "./idempotency.ts";
import {
  decideLaneFailure,
  evaluateRequiredChecks,
  evaluateSuiteConclusion,
  isCiCompletionObsolete,
  isEntryFinished,
  mergePreconditionViolation,
} from "./state-machine.ts";
import { canMergeLane, entrySetHash } from "./scheduler.ts";
import {
  holdQueueWithoutPro,
  supabasePauseStore,
} from "./entitlement-pause.ts";
import type { QueueConfigRow, QueueRow, RepoMeta } from "./queue-repository.ts";
import {
  countQueuedEntries,
  enqueueEntry,
  getEntriesByIds,
  getEntry,
  getQueue,
  getQueueConfig,
  getRepoMeta,
  resetEntriesToQueued,
  setBisectMode,
  updateEntryStatus,
} from "./queue-repository.ts";
import type { CiRunRow } from "./ci-run-repository.ts";
import {
  findActiveRunsContainingEntry,
  getActiveLanes,
  getCiRun,
  getCiRunEntries,
  removeEntryFromRun,
  updateCiRun,
} from "./ci-run-repository.ts";

export interface HandlerContext {
  supabase: SupabaseClient;
  workerId: string;
  // Present while the worker holds the queue's execution lease; required by
  // the transactional lane reservation RPC.
  leaseToken?: string;
  adapterFor: (repo: RepoMeta) => Promise<GitHubAdapter>;
  // installation_has_pro for the repo's GitHub App installation. A queue
  // whose owner has no Pro is paused rather than tested or merged.
  installationHasPro: (installationId: number) => Promise<boolean>;
  publish: (command: MergeQueueCommand, delaySeconds?: number) => Promise<void>;
  log: (fields: Record<string, unknown>) => void;
}

export interface HandlerResult {
  outcome: "applied" | "already_satisfied" | "obsolete";
  detail?: string;
}

export async function handleCommand(
  ctx: HandlerContext,
  command: MergeQueueCommand,
): Promise<HandlerResult> {
  switch (command.type) {
    case "entry.enqueue":
      return handleEnqueue(ctx, command);
    case "entry.dequeue":
      return handleDequeue(ctx, command);
    case "queue.drive":
      return handleDrive(ctx, command);
    case "ci.completed":
      return handleCiCompleted(ctx, command);
    case "installation.sync":
      return handleInstallationSync(ctx, command);
  }
}

function newCommandId(): string {
  return crypto.randomUUID();
}

function driveCommand(
  queueId: string,
  reason: QueueDriveCommand["reason"],
): QueueDriveCommand {
  return {
    version: 1,
    type: "queue.drive",
    commandId: newCommandId(),
    queueId,
    reason,
    occurredAt: new Date().toISOString(),
  };
}

async function loadQueueContext(
  ctx: HandlerContext,
  queueId: string,
): Promise<{ queue: QueueRow; config: QueueConfigRow; repo: RepoMeta } | null> {
  const queue = await getQueue(ctx.supabase, queueId);
  if (!queue) return null;
  const repo = await getRepoMeta(ctx.supabase, queue.repo_id);
  if (!repo) {
    throw new TerminalError(`repository ${queue.repo_id} is no longer tracked`);
  }
  const config = await getQueueConfig(ctx.supabase, queue.repo_id, queue.target_branch);
  return { queue, config, repo };
}

// True when the installation owner has no Pro. The queue is then paused:
// lanes cancelled, PRs kept queued, one comment each.
async function heldWithoutPro(
  ctx: HandlerContext,
  queue: QueueRow,
  repo: RepoMeta,
): Promise<boolean> {
  return await holdQueueWithoutPro(
    { id: queue.id },
    {
      hasPro: () => ctx.installationHasPro(repo.installationId),
      store: supabasePauseStore(ctx.supabase, ctx.workerId),
      github: () => ctx.adapterFor(repo),
      log: ctx.log,
    },
  );
}

const PAUSED_WITHOUT_PRO = "paused: the installation owner has no Pro";

// ── entry.enqueue ─────────────────────────────────────────────────────────────

async function handleEnqueue(
  ctx: HandlerContext,
  command: EntryEnqueueCommand,
): Promise<HandlerResult> {
  const loaded = await loadQueueContext(ctx, command.queueId);
  if (!loaded) {
    return { outcome: "obsolete", detail: "queue no longer exists" };
  }

  await enqueueEntry(ctx.supabase, {
    queueId: command.queueId,
    prNumber: command.pullRequestNumber,
    prSha: command.headSha,
    prTitle: command.prTitle ?? null,
    prAuthor: command.prAuthor ?? null,
    branchName: command.headRefName ?? null,
  });

  await ctx.publish(driveCommand(command.queueId, "entry_added"));
  return { outcome: "applied" };
}

// ── entry.dequeue ─────────────────────────────────────────────────────────────

async function handleDequeue(
  ctx: HandlerContext,
  command: EntryDequeueCommand,
): Promise<HandlerResult> {
  const loaded = await loadQueueContext(ctx, command.queueId);
  if (!loaded) {
    return { outcome: "obsolete", detail: "queue no longer exists" };
  }
  const { queue, repo } = loaded;

  const entry = await getEntry(ctx.supabase, command.queueId, command.pullRequestNumber);
  if (!entry) {
    return { outcome: "obsolete", detail: "entry not found" };
  }
  if (isEntryFinished(entry.status)) {
    return { outcome: "already_satisfied", detail: `entry is ${entry.status}` };
  }

  const wasTesting = entry.status === "testing";

  if (command.reason === "synchronized") {
    // The PR's head moved: keep it queued at the new SHA, but any lane that
    // tested the old SHA (and lanes stacked above it) is invalid.
    await updateEntryStatus(ctx.supabase, entry.id, "queued", {
      prSha: command.headSha!,
    });
  } else {
    const reasonText = command.reason === "closed"
      ? command.merged
        ? "PR merged outside the queue"
        : "PR closed"
      : command.reason === "label_removed"
        ? "Label removed"
        : "Manually dequeued";
    await updateEntryStatus(ctx.supabase, entry.id, "dequeued", {
      failureReason: reasonText,
    });
  }

  if (wasTesting) {
    const runs = await findActiveRunsContainingEntry(ctx.supabase, queue.id, entry.id);
    if (runs.length > 0) {
      const gh = await ctx.adapterFor(repo);
      const fromLane = Math.min(...runs.map((r) => r.lane_number));
      await cancelLanesFrom(ctx, gh, queue.id, fromLane);
      // cancelLanesFrom resets testing entries back to queued; re-apply the
      // dequeue for the entry this command targets.
      if (command.reason !== "synchronized") {
        await updateEntryStatus(ctx.supabase, entry.id, "dequeued", {
          failureReason: "Removed from queue",
        });
      }
    }
  }

  await ctx.publish(driveCommand(queue.id, "entry_removed"));
  return { outcome: "applied" };
}

// ── queue.drive ───────────────────────────────────────────────────────────────

interface LaneReservation {
  ciRunId: string;
  queueId: string;
  laneNumber: number;
  targetBranch: string;
  expectedBaseSha: string | null;
  testBranch: string;
  entries: Array<{
    entryId: string;
    pullRequestNumber: number;
    headSha: string;
    title: string | null;
    order: number;
  }>;
}

async function handleDrive(
  ctx: HandlerContext,
  command: QueueDriveCommand,
): Promise<HandlerResult> {
  const loaded = await loadQueueContext(ctx, command.queueId);
  if (!loaded) {
    return { outcome: "obsolete", detail: "queue no longer exists" };
  }
  const { queue, config, repo } = loaded;
  if (queue.paused || !config.enabled) {
    return { outcome: "already_satisfied", detail: "queue paused or disabled" };
  }
  if (await heldWithoutPro(ctx, queue, repo)) {
    return { outcome: "already_satisfied", detail: PAUSED_WITHOUT_PRO };
  }

  const gh = await ctx.adapterFor(repo);

  // Recover any lane that passed CI but has not merged yet (e.g. a worker
  // crashed between "passed" and the merge).
  await mergeReadyLanes(ctx, gh, queue, config, repo);

  // Start as many new lanes as reservation allows.
  let started = 0;
  for (;;) {
    const reservation = await reserveLane(ctx, queue.id);
    if (!reservation) break;
    await buildLane(ctx, gh, queue, config, repo, reservation);
    started++;
    if (started > 20) {
      // Defensive bound; capacity checks should stop the loop well before.
      break;
    }
  }

  // If everything is drained, leave bisect mode.
  if (queue.bisect_mode) {
    const active = await getActiveLanes(ctx.supabase, queue.id);
    if (active.length === 0) {
      const queued = await countQueuedEntries(ctx.supabase, queue.id);
      if (queued === 0) {
        await setBisectMode(ctx.supabase, queue.id, false);
      }
    }
  }

  return { outcome: "applied", detail: `${started} lane(s) started` };
}

async function reserveLane(
  ctx: HandlerContext & { leaseToken?: string },
  queueId: string,
): Promise<LaneReservation | null> {
  const { data, error } = await ctx.supabase.rpc("reserve_next_merge_queue_lane", {
    p_queue_id: queueId,
    p_worker_id: ctx.workerId,
    p_lease_token: ctx.leaseToken,
    p_batch_size_override: null,
  });
  if (error) {
    if (error.code === "55006") {
      throw new RetryableError(`lease lost while reserving lane for ${queueId}`);
    }
    throw new Error(`failed to reserve lane: ${error.message}`);
  }
  return (data as LaneReservation | null) ?? null;
}

// Build the reserved lane's test branch and flip the run to running.
async function buildLane(
  ctx: HandlerContext,
  gh: GitHubAdapter,
  queue: QueueRow,
  config: QueueConfigRow,
  repo: RepoMeta,
  reservation: LaneReservation,
): Promise<void> {
  const startKey = startLaneOperationKey(
    queue.id,
    reservation.ciRunId,
    reservation.laneNumber,
    reservation.expectedBaseSha ?? "tip",
    entrySetHash(reservation.entries.map((e) => e.entryId)),
  );
  if (await operationAlreadySucceeded(ctx.supabase, startKey)) {
    return;
  }

  const baseSha = reservation.expectedBaseSha ??
    (await gh.getBranchSha(queue.target_branch));

  await gh.createOrResetBranch(reservation.testBranch, baseSha);

  let headSha = baseSha;
  const surviving: LaneReservation["entries"] = [];

  for (const entry of reservation.entries) {
    try {
      const sha = await gh.mergeInto(
        reservation.testBranch,
        entry.headSha,
        `[treq] Merge PR #${entry.pullRequestNumber} into test branch`,
      );
      headSha = sha ?? headSha;
      surviving.push(entry);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (!/→ 409:/.test(message)) throw err;
      // Merge conflict building the batch: the entry is definitively out.
      ctx.log({
        operation: "lane_build_conflict",
        queue_id: queue.id,
        ci_run_id: reservation.ciRunId,
        pull_request_number: entry.pullRequestNumber,
      });
      await updateEntryStatus(ctx.supabase, entry.entryId, "failed", {
        failureReason: "Merge conflict when building test batch",
      });
      await removeEntryFromRun(
        ctx.supabase,
        reservation.ciRunId,
        entry.entryId,
        surviving.map((e) => e.entryId),
      );
      await gh.createComment(
        entry.pullRequestNumber,
        `❌ **Treq Merge Queue**: This PR has a merge conflict with the target branch or another queued PR and has been removed. Rebase and re-add the \`${config.queue_trigger_label}\` label to retry.`,
        `conflict:${entry.entryId}:${entry.headSha}`,
      );
    }
  }

  if (surviving.length === 0) {
    await updateCiRun(ctx.supabase, reservation.ciRunId, {
      status: "cancelled",
      completedAt: new Date().toISOString(),
    });
    await gh.deleteBranch(reservation.testBranch);
    return;
  }

  await ctx.supabase
    .from("ci_runs")
    .update({ entry_ids: surviving.map((e) => e.entryId) })
    .eq("id", reservation.ciRunId);
  await updateCiRun(ctx.supabase, reservation.ciRunId, {
    status: "running",
    headSha,
  });

  await recordOperationSuccess(ctx.supabase, {
    operationKey: startKey,
    commandType: "github.start_lane",
    queueId: queue.id,
    workerId: ctx.workerId,
    result: { ciRunId: reservation.ciRunId, headSha },
  });

  const prList = surviving.map((e) => `#${e.pullRequestNumber}`).join(", ");
  for (const entry of surviving) {
    const marker = `lane:${reservation.ciRunId}:${entry.entryId}`;
    if (await gh.hasCommentWithMarker(entry.pullRequestNumber, marker)) continue;
    await gh.createComment(
      entry.pullRequestNumber,
      `🚦 **Treq Merge Queue**: Testing in lane ${reservation.laneNumber} (batch: ${prList}). CI is running on \`${reservation.testBranch}\` @ \`${headSha.slice(0, 7)}\`.`,
      marker,
    );
  }

  ctx.log({
    operation: "lane_started",
    queue_id: queue.id,
    ci_run_id: reservation.ciRunId,
    lane_number: reservation.laneNumber,
    entry_count: surviving.length,
    head_sha: headSha,
  });
}

// ── ci.completed ──────────────────────────────────────────────────────────────

async function handleCiCompleted(
  ctx: HandlerContext,
  command: CiCompletedCommand,
): Promise<HandlerResult> {
  const run = await getCiRun(ctx.supabase, command.ciRunId);
  if (!run) {
    return { outcome: "obsolete", detail: "ci run no longer exists" };
  }
  if (isCiCompletionObsolete(run.status, run.head_sha, command.testedSha)) {
    return {
      outcome: "obsolete",
      detail: `run is ${run.status} at ${run.head_sha}, event was for ${command.testedSha}`,
    };
  }

  const loaded = await loadQueueContext(ctx, command.queueId);
  if (!loaded) {
    return { outcome: "obsolete", detail: "queue no longer exists" };
  }
  const { queue, config, repo } = loaded;
  // Pausing cancels this lane too, so its result no longer applies.
  if (await heldWithoutPro(ctx, queue, repo)) {
    return { outcome: "obsolete", detail: PAUSED_WITHOUT_PRO };
  }
  const gh = await ctx.adapterFor(repo);

  // Required checks are evaluated against the exact tested SHA; the
  // check-suite conclusion alone is only trusted when none are configured.
  let evaluation: "pending" | "passed" | "failed";
  if (config.required_checks.length > 0) {
    const checkRuns = await gh.listCheckRuns(run.head_sha);
    evaluation = evaluateRequiredChecks(config.required_checks, checkRuns);
  } else {
    evaluation = evaluateSuiteConclusion(command.conclusion);
  }

  if (evaluation === "pending") {
    throw new RetryableError(
      `required checks still pending for ${run.test_branch} @ ${run.head_sha}`,
      { delaySeconds: 60 },
    );
  }

  if (evaluation === "failed") {
    await onLaneFailed(ctx, gh, queue, config, run, command.conclusion);
  } else {
    await updateCiRun(ctx.supabase, run.id, {
      status: "passed",
      conclusion: command.conclusion,
      completedAt: new Date().toISOString(),
    });
    await mergeReadyLanes(ctx, gh, queue, config, repo);
  }

  await ctx.publish(driveCommand(queue.id, "ci_completed"));
  return { outcome: "applied", detail: `evaluation: ${evaluation}` };
}

async function onLaneFailed(
  ctx: HandlerContext,
  gh: GitHubAdapter,
  queue: QueueRow,
  config: QueueConfigRow,
  run: CiRunRow,
  conclusion: string,
): Promise<void> {
  await updateCiRun(ctx.supabase, run.id, {
    status: "failed",
    conclusion,
    completedAt: new Date().toISOString(),
  });

  const runEntries = await getCiRunEntries(ctx.supabase, run.id);
  const entries = await getEntriesByIds(
    ctx.supabase,
    runEntries.map((e) => e.entry_id),
  );

  // Lanes above this one were built on top of it and are now invalid.
  await cancelLanesFrom(ctx, gh, queue.id, run.lane_number + 1);
  await gh.deleteBranch(run.test_branch);

  const decision = decideLaneFailure(entries.length);
  if (decision.action === "fail_entry") {
    for (const entry of entries) {
      await updateEntryStatus(ctx.supabase, entry.id, "failed", {
        failureReason: "CI failed",
      });
      await gh.createComment(
        entry.pr_number,
        `❌ **Treq Merge Queue**: CI failed for this PR and it has been removed from the queue. Fix the issue and re-add the \`${config.queue_trigger_label}\` label to re-queue.`,
        `ci-failed:${run.id}:${entry.id}`,
      );
      await gh.removeLabel(entry.pr_number, config.queue_trigger_label);
    }
  } else {
    // Batch failure: isolate the culprit by re-testing entries individually.
    await setBisectMode(ctx.supabase, queue.id, true);
    await resetEntriesToQueued(ctx.supabase, entries.map((e) => e.id));
    for (const entry of entries) {
      const marker = `bisect:${run.id}:${entry.id}`;
      if (await gh.hasCommentWithMarker(entry.pr_number, marker)) continue;
      await gh.createComment(
        entry.pr_number,
        `⚠️ **Treq Merge Queue**: CI failed for the batch. Re-queuing this PR individually to find the culprit.`,
        marker,
      );
    }
  }

  ctx.log({
    operation: "lane_failed",
    queue_id: queue.id,
    ci_run_id: run.id,
    lane_number: run.lane_number,
    entry_count: entries.length,
    decision: decision.action,
  });
}

// Merge every lane that has passed CI and has no lower lane still active.
// Chains: merging lane N unblocks lane N+1 if it already passed.
async function mergeReadyLanes(
  ctx: HandlerContext,
  gh: GitHubAdapter,
  queue: QueueRow,
  config: QueueConfigRow,
  repo: RepoMeta,
): Promise<void> {
  for (;;) {
    const lanes = await getActiveLanes(ctx.supabase, queue.id, [
      "pending",
      "running",
      "passed",
    ]);
    const ready = lanes
      .filter((lane) => lane.status === "passed")
      .find((lane) =>
        canMergeLane(
          lane.lane_number,
          lanes.map((l) => ({
            laneNumber: l.lane_number,
            headSha: l.head_sha,
            status: l.status as "pending" | "running" | "passed",
          })),
        )
      );
    if (!ready) return;

    await mergeLane(ctx, gh, queue, config, repo, ready);
  }
}

async function mergeLane(
  ctx: HandlerContext,
  gh: GitHubAdapter,
  queue: QueueRow,
  config: QueueConfigRow,
  repo: RepoMeta,
  run: CiRunRow,
): Promise<void> {
  const runEntries = await getCiRunEntries(ctx.supabase, run.id);
  const entries = await getEntriesByIds(
    ctx.supabase,
    runEntries.map((e) => e.entry_id),
  );
  const testedShaByEntry = new Map(
    runEntries.map((e) => [e.entry_id, e.tested_head_sha]),
  );

  for (const entry of entries) {
    if (entry.status === "merged") continue;
    const testedSha = testedShaByEntry.get(entry.id) ?? entry.pr_sha;
    const opKey = mergePrOperationKey(String(repo.id), entry.pr_number, testedSha);

    if (await operationAlreadySucceeded(ctx.supabase, opKey)) {
      // A previous attempt merged this PR but crashed before updating the
      // entry row.
      await updateEntryStatus(ctx.supabase, entry.id, "merged", {
        mergedAt: new Date().toISOString(),
      });
      continue;
    }

    await updateEntryStatus(ctx.supabase, entry.id, "merging");

    const pr = await gh.getPR(entry.pr_number);
    if (!pr) {
      await updateEntryStatus(ctx.supabase, entry.id, "failed", {
        failureReason: "PR no longer exists",
      });
      continue;
    }
    if (pr.state !== "open" && pr.merged) {
      await updateEntryStatus(ctx.supabase, entry.id, "merged", {
        mergedAt: new Date().toISOString(),
      });
      continue;
    }

    const violation = mergePreconditionViolation(
      { state: pr.state, baseRef: pr.baseRef, headSha: pr.headSha },
      { targetBranch: queue.target_branch, testedHeadSha: testedSha },
    );
    if (violation) {
      if (pr.state === "open" && pr.headSha !== testedSha) {
        // Head moved after CI: requeue at the new SHA rather than failing.
        await updateEntryStatus(ctx.supabase, entry.id, "queued", {
          prSha: pr.headSha,
        });
        await gh.createComment(
          entry.pr_number,
          `🔁 **Treq Merge Queue**: This PR changed after CI passed (${violation}). It has been re-queued for a fresh test.`,
          `requeue:${run.id}:${entry.id}:${pr.headSha}`,
        );
      } else {
        await updateEntryStatus(ctx.supabase, entry.id, "dequeued", {
          failureReason: violation,
        });
      }
      continue;
    }

    try {
      await gh.mergePR(
        entry.pr_number,
        config.merge_method,
        `${entry.pr_title ?? `PR #${entry.pr_number}`} (#${entry.pr_number})`,
        testedSha,
      );
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (/→ 409:/.test(message)) {
        // Head moved between our precondition check and the merge call.
        const fresh = await gh.getPR(entry.pr_number);
        await updateEntryStatus(ctx.supabase, entry.id, "queued", {
          prSha: fresh?.headSha ?? entry.pr_sha,
        });
        continue;
      }
      if (/→ 40[35]:/.test(message)) {
        await updateEntryStatus(ctx.supabase, entry.id, "failed", {
          failureReason: `Merge rejected by GitHub: ${message.slice(0, 300)}`,
        });
        await gh.createComment(
          entry.pr_number,
          `❌ **Treq Merge Queue**: CI passed but GitHub rejected the merge. Check branch protection settings and re-queue.`,
          `merge-rejected:${run.id}:${entry.id}`,
        );
        continue;
      }
      throw err;
    }

    await recordOperationSuccess(ctx.supabase, {
      operationKey: opKey,
      commandType: "github.merge_pr",
      queueId: queue.id,
      workerId: ctx.workerId,
      result: { prNumber: entry.pr_number, testedSha },
    });
    await updateEntryStatus(ctx.supabase, entry.id, "merged", {
      mergedAt: new Date().toISOString(),
    });
    ctx.log({
      operation: "pr_merged",
      queue_id: queue.id,
      ci_run_id: run.id,
      pull_request_number: entry.pr_number,
      tested_sha: testedSha,
    });
  }

  await updateCiRun(ctx.supabase, run.id, { status: "merged" });
  await gh.deleteBranch(run.test_branch);
}

// ── installation.sync ─────────────────────────────────────────────────────────

async function handleInstallationSync(
  ctx: HandlerContext,
  command: InstallationSyncCommand,
): Promise<HandlerResult> {
  // Installation and repository rows are directly observable GitHub facts and
  // are upserted by the webhook itself (fast, idempotent DB writes). Deletion
  // cascades clean up all dependent queue state. This command exists for
  // auditability and to verify that ingestion and domain state agree.
  const { data, error } = await ctx.supabase
    .from("github_app_installations")
    .select("id")
    .eq("id", command.installationId)
    .maybeSingle();
  if (error) throw new Error(`failed to check installation: ${error.message}`);

  if (command.action === "deleted" && data) {
    throw new RetryableError(
      `installation ${command.installationId} still present after delete event`,
    );
  }
  return { outcome: "applied" };
}

// ── shared helpers ────────────────────────────────────────────────────────────

// Cancel every pending/running lane at or above `fromLaneNumber`, resetting
// their testing entries back to queued and removing their test branches.
async function cancelLanesFrom(
  ctx: HandlerContext,
  gh: GitHubAdapter,
  queueId: string,
  fromLaneNumber: number,
): Promise<void> {
  const lanes = await getActiveLanes(ctx.supabase, queueId);
  for (const lane of lanes) {
    if (lane.lane_number < fromLaneNumber) continue;
    await updateCiRun(ctx.supabase, lane.id, {
      status: "cancelled",
      completedAt: new Date().toISOString(),
    });
    const runEntries = await getCiRunEntries(ctx.supabase, lane.id);
    await resetEntriesToQueued(
      ctx.supabase,
      runEntries.map((e) => e.entry_id),
    );
    try {
      await gh.deleteBranch(lane.test_branch);
    } catch (err) {
      // Branch cleanup is recoverable by the reconciler.
      ctx.log({
        operation: "branch_cleanup_failed",
        queue_id: queueId,
        ci_run_id: lane.id,
        error: err instanceof Error ? err.message : String(err),
      });
    }
    ctx.log({
      operation: "lane_cancelled",
      queue_id: queueId,
      ci_run_id: lane.id,
      lane_number: lane.lane_number,
    });
  }
}
