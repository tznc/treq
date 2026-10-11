// Versioned message contracts for the merge queue command queue.
//
// PGMQ messages represent work that must be *attempted*; the domain tables
// remain the source of truth. Every command is reconstructable from domain
// state, carries no credentials and no full webhook payloads.

export const COMMAND_QUEUE = "merge_queue_commands";
export const DEAD_LETTER_QUEUE = "merge_queue_dead_letters";

export const MESSAGE_VERSION = 1;

export type DequeueReason =
  | "label_removed"
  | "closed"
  | "synchronized"
  | "manual";

export type DriveReason =
  | "entry_added"
  | "entry_removed"
  | "ci_completed"
  | "lane_completed"
  | "manual"
  | "reconciliation";

interface CommandBase {
  version: 1;
  commandId: string;
  occurredAt: string;
}

export interface EntryEnqueueCommand extends CommandBase {
  type: "entry.enqueue";
  deliveryId: string;
  repositoryId: string;
  queueId: string;
  pullRequestNumber: number;
  headSha: string;
  baseBranch: string;
  prTitle?: string | null;
  prAuthor?: string | null;
  headRefName?: string | null;
}

export interface EntryDequeueCommand extends CommandBase {
  type: "entry.dequeue";
  deliveryId: string;
  repositoryId: string;
  queueId: string;
  pullRequestNumber: number;
  reason: DequeueReason;
  // Present when reason === "synchronized": the PR's new head SHA. The worker
  // re-queues the entry at this SHA instead of removing it.
  headSha?: string;
  // Present when reason === "closed" and the PR was merged outside the queue.
  merged?: boolean;
}

export interface QueueDriveCommand extends CommandBase {
  type: "queue.drive";
  queueId: string;
  reason: DriveReason;
}

export interface CiCompletedCommand extends CommandBase {
  type: "ci.completed";
  deliveryId: string;
  queueId: string;
  ciRunId: string;
  testedSha: string;
  conclusion: string;
}

export interface InstallationSyncCommand extends CommandBase {
  type: "installation.sync";
  deliveryId: string;
  installationId: number;
  action: "created" | "repositories_changed" | "deleted";
}

export type MergeQueueCommand =
  | EntryEnqueueCommand
  | EntryDequeueCommand
  | QueueDriveCommand
  | CiCompletedCommand
  | InstallationSyncCommand;

export type CommandType = MergeQueueCommand["type"];

// Dead-letter envelope wrapping an exhausted or terminally failed command.
export interface DeadLetter {
  version: 1;
  originalMessageId: number | null;
  command: unknown;
  attempts: number;
  errorCategory: "retryable_exhausted" | "terminal" | "malformed";
  error: string;
  firstFailedAt: string;
  lastFailedAt: string;
}

// ── Operation keys (logical action deduplication) ────────────────────────────

export function enqueueOperationKey(
  queueId: string,
  prNumber: number,
  headSha: string,
): string {
  return `enqueue:${queueId}:${prNumber}:${headSha}`;
}

// Keyed by the CI run as well: a lane that is cancelled (the queue paused,
// or a PR under test was pushed to) is reserved again in a new CI run with
// the same number, base and PRs, and that run still has to be built.
export function startLaneOperationKey(
  queueId: string,
  ciRunId: string,
  laneNumber: number,
  baseSha: string,
  entrySetHash: string,
): string {
  return `start-lane:${queueId}:${ciRunId}:${laneNumber}:${baseSha}:${entrySetHash}`;
}

export function mergePrOperationKey(
  repositoryId: string,
  prNumber: number,
  testedHeadSha: string,
): string {
  return `merge-pr:${repositoryId}:${prNumber}:${testedHeadSha}`;
}

export function deleteTestBranchOperationKey(
  repositoryId: string,
  branchName: string,
): string {
  return `delete-test-branch:${repositoryId}:${branchName}`;
}

// Operation key for the command itself, used to skip already-satisfied work
// when a message is redelivered. queue.drive is intentionally keyless: it is
// safe and desirable to run repeatedly.
export function commandOperationKey(command: MergeQueueCommand): string | null {
  switch (command.type) {
    case "entry.enqueue":
      return enqueueOperationKey(
        command.queueId,
        command.pullRequestNumber,
        command.headSha,
      );
    case "entry.dequeue":
      return `dequeue:${command.queueId}:${command.pullRequestNumber}:${command.reason}:${command.deliveryId}`;
    case "ci.completed":
      return `ci-completed:${command.ciRunId}:${command.testedSha}:${command.conclusion}`;
    case "installation.sync":
      return `installation-sync:${command.installationId}:${command.action}:${command.deliveryId}`;
    case "queue.drive":
      return null;
  }
}

export function queueIdOf(command: MergeQueueCommand): string | null {
  return "queueId" in command ? command.queueId : null;
}
