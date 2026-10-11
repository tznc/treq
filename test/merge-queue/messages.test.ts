import { describe, expect, it } from "vitest";
import {
  commandOperationKey,
  deleteTestBranchOperationKey,
  enqueueOperationKey,
  type MergeQueueCommand,
  mergePrOperationKey,
  queueIdOf,
  startLaneOperationKey,
} from "../../supabase/functions/_shared/merge-queue/messages.ts";
import { parseCommand } from "../../supabase/functions/_shared/merge-queue/schemas.ts";

const base = {
  version: 1 as const,
  commandId: "cmd-1",
  occurredAt: "2026-07-16T00:00:00Z",
};

const enqueue: MergeQueueCommand = {
  ...base,
  type: "entry.enqueue",
  deliveryId: "d-1",
  repositoryId: "123",
  queueId: "q-1",
  pullRequestNumber: 42,
  headSha: "abc123",
  baseBranch: "main",
};

describe("operation keys", () => {
  it("builds stable keys from identities", () => {
    expect(enqueueOperationKey("q-1", 42, "abc")).toBe("enqueue:q-1:42:abc");
    expect(startLaneOperationKey("q-1", "run-7", 2, "base", "hash")).toBe(
      "start-lane:q-1:run-7:2:base:hash",
    );
    expect(mergePrOperationKey("123", 42, "abc")).toBe("merge-pr:123:42:abc");
    expect(deleteTestBranchOperationKey("123", "treq/x")).toBe(
      "delete-test-branch:123:treq/x",
    );
  });

  it("gives a lane reserved again after a cancel its own start key", () => {
    // Pausing the queue, or a push to a PR under test, cancels a lane and
    // reserves the same PRs again on the same base in a new CI run. That run
    // must be built, not skipped as already started.
    expect(startLaneOperationKey("q-1", "run-1", 1, "tip", "hash")).not.toBe(
      startLaneOperationKey("q-1", "run-2", 1, "tip", "hash"),
    );
  });

  it("derives command keys per type", () => {
    expect(commandOperationKey(enqueue)).toBe("enqueue:q-1:42:abc123");

    const drive: MergeQueueCommand = {
      ...base,
      type: "queue.drive",
      queueId: "q-1",
      reason: "manual",
    };
    // queue.drive is intentionally keyless: repeated drives are safe.
    expect(commandOperationKey(drive)).toBeNull();

    const ci: MergeQueueCommand = {
      ...base,
      type: "ci.completed",
      deliveryId: "d-2",
      queueId: "q-1",
      ciRunId: "run-1",
      testedSha: "sha-1",
      conclusion: "success",
    };
    expect(commandOperationKey(ci)).toBe("ci-completed:run-1:sha-1:success");
  });

  it("exposes the queue id for queue-scoped commands only", () => {
    expect(queueIdOf(enqueue)).toBe("q-1");
    const sync: MergeQueueCommand = {
      ...base,
      type: "installation.sync",
      deliveryId: "d-3",
      installationId: 7,
      action: "created",
    };
    expect(queueIdOf(sync)).toBeNull();
  });
});

describe("parseCommand", () => {
  it("accepts every valid command type", () => {
    const commands: MergeQueueCommand[] = [
      enqueue,
      {
        ...base,
        type: "entry.dequeue",
        deliveryId: "d-1",
        repositoryId: "123",
        queueId: "q-1",
        pullRequestNumber: 42,
        reason: "closed",
        merged: true,
      },
      { ...base, type: "queue.drive", queueId: "q-1", reason: "entry_added" },
      {
        ...base,
        type: "ci.completed",
        deliveryId: "d-1",
        queueId: "q-1",
        ciRunId: "r-1",
        testedSha: "sha",
        conclusion: "failure",
      },
      {
        ...base,
        type: "installation.sync",
        deliveryId: "d-1",
        installationId: 9,
        action: "deleted",
      },
    ];
    for (const command of commands) {
      const result = parseCommand(command);
      expect(result.ok, JSON.stringify(result)).toBe(true);
    }
  });

  it("rejects unknown versions", () => {
    const result = parseCommand({ ...enqueue, version: 2 });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toContain("unknown message version");
  });

  it("rejects unknown command types", () => {
    const result = parseCommand({ ...base, type: "entry.explode" });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toContain("unknown command type");
  });

  it("rejects non-object messages and missing fields", () => {
    expect(parseCommand(null).ok).toBe(false);
    expect(parseCommand("hi").ok).toBe(false);
    expect(parseCommand({ ...enqueue, headSha: "" }).ok).toBe(false);
    expect(parseCommand({ ...enqueue, pullRequestNumber: "42" }).ok).toBe(
      false,
    );
    expect(parseCommand({ ...enqueue, commandId: undefined }).ok).toBe(false);
  });

  it("requires headSha for synchronized dequeues", () => {
    const dequeue = {
      ...base,
      type: "entry.dequeue",
      deliveryId: "d-1",
      repositoryId: "123",
      queueId: "q-1",
      pullRequestNumber: 42,
      reason: "synchronized",
    };
    expect(parseCommand(dequeue).ok).toBe(false);
    expect(parseCommand({ ...dequeue, headSha: "new-sha" }).ok).toBe(true);
  });

  it("rejects invalid enum values", () => {
    const dequeue = {
      ...base,
      type: "entry.dequeue",
      deliveryId: "d-1",
      repositoryId: "123",
      queueId: "q-1",
      pullRequestNumber: 42,
      reason: "because",
    };
    expect(parseCommand(dequeue).ok).toBe(false);

    const drive = { ...base, type: "queue.drive", queueId: "q-1", reason: "?" };
    expect(parseCommand(drive).ok).toBe(false);
  });
});
