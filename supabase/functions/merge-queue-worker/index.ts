// PGMQ consumer for merge queue commands.
//
// Invoked by Supabase Cron (recovery mechanism) and nudged fire-and-forget by
// the webhook after publishing. Each invocation processes one bounded batch:
//
//   read (visibility timeout) → claim execution → acquire queue lease →
//   re-read domain state → apply transition + GitHub side effects →
//   archive on success / reschedule with backoff / dead-letter
//
// Two workers can safely run concurrently: PGMQ's visibility timeout keeps a
// message exclusive while in flight, and the per-queue execution lease
// serializes all mutations of a given merge queue.

import { createClient, type SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import {
  archiveCommand,
  publishDeadLetter,
  publishMergeQueueCommand,
  type QueueMessage,
  readCommandBatch,
  rescheduleCommand,
} from "../_shared/merge-queue/commands-queue.ts";
import {
  backoffSeconds,
  classifyError,
  isExhausted,
  LeaseContentionError,
  RetryableError,
} from "../_shared/merge-queue/errors.ts";
import { GitHubRestAdapter } from "../_shared/merge-queue/github-adapter.ts";
import {
  isGithubStubEnabled,
  StubGitHubAdapter,
} from "../_shared/merge-queue/stub-github-adapter.ts";
import {
  claimCommandExecution,
  markExecution,
  operationAlreadySucceeded,
} from "../_shared/merge-queue/idempotency.ts";
import { acquireLease, releaseLease } from "../_shared/merge-queue/leases.ts";
import {
  commandOperationKey,
  type DeadLetter,
  type MergeQueueCommand,
  queueIdOf,
} from "../_shared/merge-queue/messages.ts";
import { parseCommand } from "../_shared/merge-queue/schemas.ts";
import { handleCommand } from "../_shared/merge-queue/command-handler.ts";
import { installationHasPro } from "../_shared/billing/entitlement.ts";
import type { RepoMeta } from "../_shared/merge-queue/queue-repository.ts";

const BATCH_SIZE = 5;
const VISIBILITY_TIMEOUT_SECONDS = 120;
const LEASE_TTL_SECONDS = 120;

interface BatchStats {
  succeeded: number;
  skipped: number;
  retried: number;
  dead_lettered: number;
}

function log(fields: Record<string, unknown>): void {
  console.log(JSON.stringify(fields));
}

async function deadLetter(
  supabase: SupabaseClient,
  message: QueueMessage,
  errorCategory: DeadLetter["errorCategory"],
  error: string,
): Promise<void> {
  await publishDeadLetter(supabase, {
    version: 1,
    originalMessageId: message.msg_id,
    command: message.message,
    attempts: message.read_ct,
    errorCategory,
    error: error.slice(0, 2000),
    firstFailedAt: message.enqueued_at,
    lastFailedAt: new Date().toISOString(),
  });
  await archiveCommand(supabase, message.msg_id);
}

async function processMessage(
  supabase: SupabaseClient,
  workerId: string,
  message: QueueMessage,
  stats: BatchStats,
): Promise<void> {
  const startedAt = Date.now();

  const parsed = parseCommand(message.message);
  if (!parsed.ok) {
    log({
      operation: "command_malformed",
      pgmq_message_id: message.msg_id,
      worker_id: workerId,
      error: parsed.error,
    });
    await deadLetter(supabase, message, "malformed", parsed.error);
    stats.dead_lettered++;
    return;
  }

  const command: MergeQueueCommand = parsed.command;
  const queueId = queueIdOf(command);
  const operationKey = commandOperationKey(command);

  const baseLog = {
    command_id: command.commandId,
    command_type: command.type,
    pgmq_message_id: message.msg_id,
    github_delivery_id: "deliveryId" in command ? command.deliveryId : null,
    queue_id: queueId,
    worker_id: workerId,
    attempt: message.read_ct,
  };

  // Command-level idempotency: a command that already succeeded (or was
  // ruled terminal) is archived without re-execution.
  const claim = await claimCommandExecution(supabase, {
    commandId: command.commandId,
    messageId: message.msg_id,
    commandType: command.type,
    queueId,
    operationKey,
    workerId,
  });
  if (claim.kind !== "claimed") {
    await archiveCommand(supabase, message.msg_id);
    log({ ...baseLog, operation: "command_skipped", outcome: claim.kind });
    stats.skipped++;
    return;
  }

  // Operation-level idempotency: the logical action may have been performed
  // by a different command (e.g. webhook reprocessing minted a new
  // commandId for the same enqueue).
  if (operationKey && (await operationAlreadySucceeded(supabase, operationKey))) {
    await markExecution(supabase, command.commandId, "succeeded", {
      result: { outcome: "already_satisfied", via: "operation_key" },
    });
    await archiveCommand(supabase, message.msg_id);
    log({ ...baseLog, operation: "command_skipped", outcome: "operation_satisfied" });
    stats.skipped++;
    return;
  }

  const lease = queueId
    ? await acquireLeaseOrThrow(supabase, queueId, workerId)
    : null;

  try {
    const result = await handleCommand(
      {
        supabase,
        workerId,
        leaseToken: lease?.token,
        adapterFor: async (repo: RepoMeta) => {
          if (isGithubStubEnabled()) {
            return new StubGitHubAdapter(
              supabase,
              repo.id,
              repo.defaultBranch,
            );
          }
          return GitHubRestAdapter.forInstallation(
            repo.installationId,
            repo.owner,
            repo.name,
          );
        },
        installationHasPro: (installationId) =>
          installationHasPro(supabase, installationId),
        publish: async (cmd, delaySeconds = 0) => {
          await publishMergeQueueCommand(supabase, cmd, delaySeconds);
        },
        log: (fields) => log({ ...baseLog, ...fields }),
      },
      command,
    );

    await markExecution(supabase, command.commandId, "succeeded", { result });
    await archiveCommand(supabase, message.msg_id);
    log({
      ...baseLog,
      operation: "command_processed",
      outcome: result.outcome,
      detail: result.detail ?? null,
      duration_ms: Date.now() - startedAt,
    });
    stats.succeeded++;
  } catch (err) {
    const errorMessage = err instanceof Error ? err.message : String(err);
    const classification = classifyError(err);

    if (classification === "terminal") {
      await markExecution(supabase, command.commandId, "terminal_failed", {
        lastError: errorMessage,
      });
      await deadLetter(supabase, message, "terminal", errorMessage);
      log({
        ...baseLog,
        operation: "command_terminal_failed",
        error: errorMessage,
        duration_ms: Date.now() - startedAt,
      });
      stats.dead_lettered++;
      return;
    }

    // Lease contention retries do not consume real attempts beyond the
    // message's read count budget, but they follow the same path.
    if (isExhausted(message.read_ct) && !(err instanceof LeaseContentionError)) {
      await markExecution(supabase, command.commandId, "terminal_failed", {
        lastError: `retries exhausted: ${errorMessage}`,
      });
      await deadLetter(supabase, message, "retryable_exhausted", errorMessage);
      log({
        ...baseLog,
        operation: "command_exhausted",
        error: errorMessage,
        duration_ms: Date.now() - startedAt,
      });
      stats.dead_lettered++;
      return;
    }

    const delay = err instanceof RetryableError && err.delaySeconds !== undefined
      ? err.delaySeconds
      : backoffSeconds(message.read_ct);
    await markExecution(supabase, command.commandId, "retryable_failed", {
      lastError: errorMessage,
    });
    await rescheduleCommand(supabase, message.msg_id, delay);
    log({
      ...baseLog,
      operation: "command_retry_scheduled",
      error: errorMessage,
      retry_delay_seconds: delay,
      duration_ms: Date.now() - startedAt,
    });
    stats.retried++;
  } finally {
    if (lease) await releaseLease(supabase, lease);
  }
}

async function acquireLeaseOrThrow(
  supabase: SupabaseClient,
  queueId: string,
  workerId: string,
) {
  return await acquireLease(supabase, queueId, workerId, LEASE_TTL_SECONDS);
}

Deno.serve(async (req) => {
  if (req.method !== "POST") {
    return new Response("Method not allowed", { status: 405 });
  }

  const supabase = createClient(
    Deno.env.get("SUPABASE_URL") ?? "",
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );
  const workerId = crypto.randomUUID();

  const stats: BatchStats = {
    succeeded: 0,
    skipped: 0,
    retried: 0,
    dead_lettered: 0,
  };

  let messages: QueueMessage[];
  try {
    messages = await readCommandBatch(supabase, VISIBILITY_TIMEOUT_SECONDS, BATCH_SIZE);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.error("[merge-queue-worker] failed to read queue:", message);
    return new Response(JSON.stringify({ error: message }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }

  for (const message of messages) {
    try {
      await processMessage(supabase, workerId, message, stats);
    } catch (err) {
      // processMessage handles its own failures; reaching here means even
      // the failure handling failed (e.g. database unavailable). Leave the
      // message in flight — the visibility timeout retries it.
      console.error(
        "[merge-queue-worker] failed to process message",
        message.msg_id,
        err instanceof Error ? err.message : String(err),
      );
    }
  }

  log({
    operation: "worker_batch_complete",
    worker_id: workerId,
    read: messages.length,
    ...stats,
  });

  return new Response(JSON.stringify({ read: messages.length, ...stats }), {
    headers: { "Content-Type": "application/json" },
  });
});
