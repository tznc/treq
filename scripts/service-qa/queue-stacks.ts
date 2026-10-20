/**
 * How the desktop app groups merge queue entries into stacks
 * (`screen/merge_queue_data.rs` in src-tauri), for specs that check the
 * queue the webhooks build.
 */

export type QueueEntryStatus =
  | "queued"
  | "testing"
  | "merging"
  | "merged"
  | "failed"
  | "dequeued";

export interface QueueEntry {
  branch_name: string;
  pr_number: number | null;
  status: QueueEntryStatus;
  position: number;
  target_branch: string;
}

export interface QueueStack {
  /** Branch the whole stack ultimately lands on (never itself queued). */
  targetBranch: string;
  /** Entries in the order the queue will merge them, bottom of stack first. */
  entries: QueueEntry[];
}

/**
 * Group flat queue entries into stacks.
 *
 * A branch whose target is another queued branch is stacked on top of it, and
 * the queue must land them bottom-up. Everything else is a stack of one. Within
 * a stack entries are ordered by the chain itself, and stacks are ordered by
 * their lowest queue position so the block that merges first comes first.
 */
export function buildQueueStacks(entries: readonly QueueEntry[]): QueueStack[] {
  const byBranch = new Map(entries.map((entry) => [entry.branch_name, entry]));
  const childrenOf = new Map<string, QueueEntry[]>();
  const roots: QueueEntry[] = [];

  for (const entry of entries) {
    if (byBranch.has(entry.target_branch)) {
      const siblings = childrenOf.get(entry.target_branch) ?? [];
      siblings.push(entry);
      childrenOf.set(entry.target_branch, siblings);
    } else {
      roots.push(entry);
    }
  }

  const claimed = new Set<string>();
  const stacks: QueueStack[] = [];

  for (const root of roots.sort((a, b) => a.position - b.position)) {
    const chain: QueueEntry[] = [];
    let current: QueueEntry | undefined = root;
    while (current && !claimed.has(current.branch_name)) {
      claimed.add(current.branch_name);
      chain.push(current);
      // Lowest position wins if a branch is unexpectedly forked.
      [current] = (childrenOf.get(current.branch_name) ?? []).sort(
        (a, b) => a.position - b.position,
      );
    }
    stacks.push({ targetBranch: root.target_branch, entries: chain });
  }

  // A leftover sits in a cycle with no root; surface it rather than drop it.
  for (const entry of entries) {
    if (claimed.has(entry.branch_name)) continue;
    claimed.add(entry.branch_name);
    stacks.push({ targetBranch: entry.target_branch, entries: [entry] });
  }

  return stacks.sort(
    (a, b) =>
      Math.min(...a.entries.map((e) => e.position)) -
      Math.min(...b.entries.map((e) => e.position)),
  );
}
