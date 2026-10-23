// Pure scheduling decisions: which lanes may start, how batches are
// composed, which base each lane builds on, and when merges may proceed.
//
// This module never persists anything — the transactional
// reserve_next_merge_queue_lane RPC applies the plan atomically; these
// functions exist so the policy is inspectable and unit-testable.

export interface SchedulerConfig {
  batchSize: number;
  maxParallelLanes: number;
}

export interface ActiveLane {
  laneNumber: number;
  headSha: string;
  status: "pending" | "running" | "passed";
}

export interface QueuedEntry {
  entryId: string;
  pullRequestNumber: number;
  headSha: string;
  position: number;
}

export function effectiveBatchSize(
  config: SchedulerConfig,
  bisectMode: boolean,
): number {
  return bisectMode ? 1 : Math.max(1, config.batchSize);
}

export function availableLaneCapacity(
  config: SchedulerConfig,
  activeLaneCount: number,
): number {
  return Math.max(0, config.maxParallelLanes - activeLaneCount);
}

export interface PlannedLane {
  laneNumber: number;
  entries: QueuedEntry[];
  // null → build on the real tip of the target branch; otherwise the head of
  // the lane below (optimistic stacking).
  expectedBaseSha: string | null;
}

// Compose the set of lanes that may start now, in deterministic queue order.
export function planLaneStarts(
  config: SchedulerConfig,
  bisectMode: boolean,
  activeLanes: readonly ActiveLane[],
  queuedEntries: readonly QueuedEntry[],
): PlannedLane[] {
  const capacity = availableLaneCapacity(config, activeLanes.length);
  if (capacity === 0 || queuedEntries.length === 0) return [];

  const batchSize = effectiveBatchSize(config, bisectMode);
  const ordered = [...queuedEntries].sort(
    (a, b) => a.position - b.position || a.pullRequestNumber - b.pullRequestNumber,
  );

  const highestActive = activeLanes.reduce(
    (max, lane) => Math.max(max, lane.laneNumber),
    0,
  );
  const topOfStack =
    [...activeLanes].sort((a, b) => a.laneNumber - b.laneNumber).at(-1) ?? null;

  const lanes: PlannedLane[] = [];
  for (let i = 0; i < capacity; i++) {
    const batch = ordered.slice(i * batchSize, (i + 1) * batchSize);
    if (batch.length === 0) break;
    const previous = lanes.at(-1);
    lanes.push({
      laneNumber: highestActive + 1 + i,
      entries: batch,
      expectedBaseSha: previous
        ? // Stacked on a lane planned in the same round: the base is only
          // known once that lane's test branch is built.
          null
        : (topOfStack?.headSha ?? null),
    });
  }
  return lanes;
}

// A passed lane may merge only when no lower lane is still pending, running,
// or waiting to merge — lower lanes must land first because this lane's test
// branch was built on top of them.
export function canMergeLane(
  laneNumber: number,
  activeLanes: readonly ActiveLane[],
): boolean {
  return !activeLanes.some(
    (lane) =>
      lane.laneNumber < laneNumber &&
      (lane.status === "pending" || lane.status === "running" || lane.status === "passed"),
  );
}

// After lane N merges, the next lane that can chain-merge is exactly N+1 if
// it already passed.
export function nextMergeableLane(
  mergedLaneNumber: number,
  lanes: readonly ActiveLane[],
): ActiveLane | null {
  return (
    lanes.find(
      (lane) => lane.laneNumber === mergedLaneNumber + 1 && lane.status === "passed",
    ) ?? null
  );
}

// Deterministic test branch name derived from queue + run identity.
export function testBranchName(queueId: string, ciRunId: string): string {
  return `treq/merge-queue/${queueId}/${ciRunId}`;
}

export function isQueueTestBranch(branchName: string): boolean {
  // Current scheme plus the legacy prefix used before the PGMQ refactor.
  return (
    branchName.startsWith("treq/merge-queue/") ||
    branchName.startsWith("treq-queue/")
  );
}

// Stable hash of a lane's entry set, used in start-lane operation keys.
export function entrySetHash(entryIds: readonly string[]): string {
  const joined = [...entryIds].sort().join(",");
  let hash = 5381;
  for (let i = 0; i < joined.length; i++) {
    hash = ((hash << 5) + hash + joined.charCodeAt(i)) | 0;
  }
  return (hash >>> 0).toString(16);
}
