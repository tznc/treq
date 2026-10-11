import { describe, expect, it, vi } from "vitest";
import {
  type EntitlementGateDeps,
  type EntitlementPauseStore,
  holdQueueWithoutPro,
  type PauseGitHub,
  proPausedMarker,
} from "../../supabase/functions/_shared/merge-queue/entitlement-pause.ts";

const QUEUE = { id: "queue-1" };

type Lane = { id: string; test_branch: string; entryIds: string[] };
type Entry = { id: string; pr_number: number; status: string };

// An in-memory queue: lanes hold entries in 'testing'; cancelling a lane
// puts them back to 'queued', like the Supabase store does.
function fakeStore(lanes: Lane[], entries: Entry[]) {
  const notices = new Set<string>();
  const store: EntitlementPauseStore = {
    activeLanes: vi.fn(async () =>
      lanes.map(({ id, test_branch }) => ({ id, test_branch })),
    ),
    cancelLane: vi.fn(async (laneId: string) => {
      const lane = lanes.find((l) => l.id === laneId)!;
      for (const entry of entries) {
        if (lane.entryIds.includes(entry.id)) entry.status = "queued";
      }
      lanes.splice(lanes.indexOf(lane), 1);
    }),
    waitingEntries: vi.fn(async () =>
      entries
        .filter((e) => e.status === "queued" || e.status === "testing")
        .map(({ id, pr_number }) => ({ id, pr_number })),
    ),
    noticeRecorded: vi.fn(async (key: string) => notices.has(key)),
    recordNotice: vi.fn(async (key: string) => {
      notices.add(key);
    }),
  };
  return { store, notices };
}

// GitHub keeps its comments across calls, like the real repository would.
function fakeGitHub() {
  const comments = new Map<number, string[]>();
  const deleted: string[] = [];
  const gh: PauseGitHub = {
    deleteBranch: vi.fn(async (name: string) => {
      deleted.push(name);
    }),
    hasCommentWithMarker: vi.fn(async (pr: number, marker: string) =>
      (comments.get(pr) ?? []).some((c) =>
        c.includes(`<!-- treq:${marker} -->`),
      ),
    ),
    createComment: vi.fn(async (pr: number, body: string, marker?: string) => {
      const list = comments.get(pr) ?? [];
      list.push(marker ? `${body}\n\n<!-- treq:${marker} -->` : body);
      comments.set(pr, list);
    }),
  };
  return { gh, comments, deleted };
}

function deps(
  hasPro: () => boolean,
  store: EntitlementPauseStore,
  gh: PauseGitHub,
): EntitlementGateDeps & { github: ReturnType<typeof vi.fn> } {
  return {
    hasPro: vi.fn(async () => hasPro()),
    store,
    github: vi.fn(async () => gh),
    log: vi.fn(),
  };
}

function queueWithOneLane() {
  const entries: Entry[] = [
    { id: "e1", pr_number: 11, status: "testing" },
    { id: "e2", pr_number: 12, status: "testing" },
    { id: "e3", pr_number: 13, status: "queued" },
    { id: "e4", pr_number: 14, status: "merged" },
  ];
  const lanes: Lane[] = [
    {
      id: "run-1",
      test_branch: "treq/merge-queue/queue-1/run-1",
      entryIds: ["e1", "e2"],
    },
  ];
  return { entries, lanes };
}

describe("holdQueueWithoutPro", () => {
  it("lets the queue run without touching anything when the owner has Pro", async () => {
    const { entries, lanes } = queueWithOneLane();
    const { store } = fakeStore(lanes, entries);
    const { gh } = fakeGitHub();
    const d = deps(() => true, store, gh);

    await expect(holdQueueWithoutPro(QUEUE, d)).resolves.toBe(false);

    expect(store.activeLanes).not.toHaveBeenCalled();
    expect(store.waitingEntries).not.toHaveBeenCalled();
    expect(d.github).not.toHaveBeenCalled();
  });

  it("cancels lanes, keeps their PRs queued, and comments once on each waiting PR", async () => {
    const { entries, lanes } = queueWithOneLane();
    const { store, notices } = fakeStore(lanes, entries);
    const { gh, comments, deleted } = fakeGitHub();

    await expect(
      holdQueueWithoutPro(
        QUEUE,
        deps(() => false, store, gh),
      ),
    ).resolves.toBe(true);

    expect(store.cancelLane).toHaveBeenCalledWith("run-1");
    expect(deleted).toEqual(["treq/merge-queue/queue-1/run-1"]);
    expect(entries.map((e) => e.status)).toEqual([
      "queued",
      "queued",
      "queued",
      "merged",
    ]);
    expect([...comments.keys()].sort()).toEqual([11, 12, 13]);
    for (const pr of [11, 12, 13]) {
      expect(comments.get(pr)).toHaveLength(1);
      expect(comments.get(pr)![0]).toContain("paused");
      expect(comments.get(pr)![0]).toContain("Pro");
    }
    expect(comments.get(11)![0]).toContain(
      `<!-- treq:${proPausedMarker("e1")} -->`,
    );
    expect([...notices].sort()).toEqual(
      ["e1", "e2", "e3"].map(proPausedMarker),
    );
  });

  it("does not comment again or call GitHub on later drives while paused", async () => {
    const { entries, lanes } = queueWithOneLane();
    const { store } = fakeStore(lanes, entries);
    const { gh, comments } = fakeGitHub();
    await holdQueueWithoutPro(
      QUEUE,
      deps(() => false, store, gh),
    );

    const again = deps(() => false, store, gh);
    await expect(holdQueueWithoutPro(QUEUE, again)).resolves.toBe(true);

    expect(again.github).not.toHaveBeenCalled();
    for (const pr of [11, 12, 13]) expect(comments.get(pr)).toHaveLength(1);
  });

  it("finds a comment posted before a crash and only records it", async () => {
    const entries: Entry[] = [{ id: "e1", pr_number: 11, status: "queued" }];
    const { store, notices } = fakeStore([], entries);
    const { gh, comments } = fakeGitHub();
    await gh.createComment(11, "earlier notice", proPausedMarker("e1"));

    await holdQueueWithoutPro(
      QUEUE,
      deps(() => false, store, gh),
    );

    expect(comments.get(11)).toHaveLength(1);
    expect(notices.has(proPausedMarker("e1"))).toBe(true);
  });

  it("still pauses when a test branch cannot be deleted", async () => {
    const { entries, lanes } = queueWithOneLane();
    const { store } = fakeStore(lanes, entries);
    const { gh, comments } = fakeGitHub();
    vi.mocked(gh.deleteBranch).mockRejectedValueOnce(new Error("GitHub 500"));
    const d = deps(() => false, store, gh);

    await expect(holdQueueWithoutPro(QUEUE, d)).resolves.toBe(true);

    expect(d.log).toHaveBeenCalledWith(
      expect.objectContaining({ operation: "branch_cleanup_failed" }),
    );
    expect(comments.get(13)).toHaveLength(1);
  });

  it("resumes when Pro returns: the queued PRs are left for the drive to test", async () => {
    const { entries, lanes } = queueWithOneLane();
    const { store } = fakeStore(lanes, entries);
    const { gh } = fakeGitHub();
    let pro = false;
    await holdQueueWithoutPro(
      QUEUE,
      deps(() => pro, store, gh),
    );

    pro = true;
    const resumed = deps(() => pro, store, gh);
    await expect(holdQueueWithoutPro(QUEUE, resumed)).resolves.toBe(false);

    expect(resumed.github).not.toHaveBeenCalled();
    expect(entries.filter((e) => e.status === "queued")).toHaveLength(3);
  });
});
