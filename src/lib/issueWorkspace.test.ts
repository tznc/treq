import { beforeEach, describe, expect, it, vi } from "vitest";

const linearApi = vi.hoisted(() => ({
  linearOpenOrCreateWorkspaceFromIssue: vi.fn(),
}));

vi.mock("./api-linear", () => linearApi);
vi.mock("./api", () => ({ githubOpenOrCreateWorkspaceFromIssue: vi.fn() }));

import { openOrCreateIssueWorkspace } from "./issueWorkspace";
import type { IssueAttachment } from "./promptAttachments";

function linearIssue(
  overrides: Partial<IssueAttachment> = {},
): IssueAttachment {
  return {
    source: "linear",
    id: "parent",
    key: "ENG-1",
    url: "https://linear.app/t/issue/ENG-1",
    title: "Parent",
    includeSubItems: true,
    subItemIds: ["child-a", "child-b"],
    ...overrides,
  };
}

// Resolves each backend call only when the test says so, and records which
// calls were in flight at the same time.
function controlledBackend() {
  const pending: {
    id: string;
    resolve: () => void;
    reject: (e: Error) => void;
  }[] = [];
  let inFlight = 0;
  let maxInFlight = 0;
  const calls: unknown[][] = [];
  const call = (...args: unknown[]) => {
    calls.push(args);
    inFlight += 1;
    maxInFlight = Math.max(maxInFlight, inFlight);
    const id = String(
      typeof args[1] === "string" ? args[1] : (args[1] as { id: string }).id,
    );
    return new Promise((resolve, reject) => {
      pending.push({
        id,
        resolve: () => {
          inFlight -= 1;
          resolve({
            issue_id: id,
            item_id: id,
            workspace_id: calls.length,
            created: true,
          });
        },
        reject: (e) => {
          inFlight -= 1;
          reject(e);
        },
      });
    });
  };
  const settle = async (outcomes: Record<string, "ok" | Error> = {}) => {
    // Let queued calls start, then settle one at a time until idle.
    for (let i = 0; i < 20; i++) {
      // eslint-disable-next-line no-await-in-loop -- each step must finish first
      await new Promise((r) => setTimeout(r, 0));
      const next = pending.shift();
      if (!next) continue;
      const outcome = outcomes[next.id] ?? "ok";
      if (outcome === "ok") next.resolve();
      else next.reject(outcome);
    }
  };
  return { call, calls, settle, maxInFlight: () => maxInFlight };
}

describe("openOrCreateIssueWorkspace", () => {
  beforeEach(() => {
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockReset();
  });

  it("creates one workspace per backend call, parent first, one at a time", async () => {
    const backend = controlledBackend();
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockImplementation(
      backend.call,
    );

    const done = openOrCreateIssueWorkspace("/repo", linearIssue());
    await backend.settle();
    const result = await done;

    expect(backend.calls).toEqual([
      ["/repo", "parent"],
      ["/repo", "child-a"],
      ["/repo", "child-b"],
    ]);
    expect(backend.maxInFlight()).toBe(1);
    expect(result.workspaceId).toBe(1);
    expect(result.subItemResults).toHaveLength(2);
    expect(result.subItemFailures).toEqual([]);
  });

  it("keeps going after a sub-issue fails and reports it", async () => {
    const backend = controlledBackend();
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockImplementation(
      backend.call,
    );

    const done = openOrCreateIssueWorkspace("/repo", linearIssue());
    await backend.settle({ "child-a": new Error("jj failed") });
    const result = await done;

    expect(backend.calls.map((c) => c[1])).toEqual([
      "parent",
      "child-a",
      "child-b",
    ]);
    expect(result.subItemResults).toHaveLength(1);
    expect(result.subItemFailures).toEqual([
      { id: "child-a", error: "jj failed" },
    ]);
  });

  it("skips sub-issues unless asked", async () => {
    const backend = controlledBackend();
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockImplementation(
      backend.call,
    );

    const done = openOrCreateIssueWorkspace(
      "/repo",
      linearIssue({ includeSubItems: false }),
    );
    await backend.settle();
    await done;

    expect(backend.calls).toEqual([["/repo", "parent"]]);
  });

  it("never runs two kickoffs at once", async () => {
    const backend = controlledBackend();
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockImplementation(
      backend.call,
    );

    const first = openOrCreateIssueWorkspace(
      "/repo",
      linearIssue({ id: "one", subItemIds: ["one-child"] }),
    );
    const second = openOrCreateIssueWorkspace(
      "/repo",
      linearIssue({ id: "two", subItemIds: [] }),
    );
    await backend.settle();
    await Promise.all([first, second]);

    expect(backend.calls.map((c) => c[1])).toEqual(["one", "one-child", "two"]);
    expect(backend.maxInFlight()).toBe(1);
  });
});
