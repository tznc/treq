import { describe, expect, it, vi } from "vitest";
import { ActionIdempotencyKeys } from "./remote-idempotency";

function counter() {
  let next = 0;
  return () => {
    next += 1;
    return `id${next}`;
  };
}

describe("ActionIdempotencyKeys", () => {
  it("reuses the key when the same action is retried after an ambiguous result", () => {
    const keys = new ActionIdempotencyKeys(counter());
    const first = keys.keyFor("commit", ["/r", "ws", "msg"]);
    keys.settle(first, { status: "ambiguous", reason: "connection reset" });
    expect(keys.keyFor("commit", ["/r", "ws", "msg"])).toBe(first);
  });

  it("reuses the key when the dispatch threw before any outcome", () => {
    const keys = new ActionIdempotencyKeys(counter());
    const first = keys.keyFor("push", ["/r", "ws"]);
    // No `settle` call: the dispatch rejected.
    expect(keys.keyFor("push", ["/r", "ws"])).toBe(first);
  });

  it("starts a new action after a confirmed outcome", () => {
    const keys = new ActionIdempotencyKeys(counter());
    const first = keys.keyFor("commit", ["/r", "ws", "msg"]);
    keys.settle(first, { status: "applied", value: null });
    const second = keys.keyFor("commit", ["/r", "ws", "msg"]);
    expect(second).not.toBe(first);
    keys.settle(second, { status: "already_applied" });
    expect(keys.keyFor("commit", ["/r", "ws", "msg"])).not.toBe(second);
  });

  it("gives different inputs different keys and keeps each pending one", () => {
    const keys = new ActionIdempotencyKeys(counter());
    const a = keys.keyFor("rebase", ["/r", "ws", "main"]);
    const b = keys.keyFor("rebase", ["/r", "ws", "dev"]);
    expect(a).not.toBe(b);
    expect(keys.keyFor("rebase", ["/r", "ws", "main"])).toBe(a);
  });

  it("fingerprints object inputs independently of property order", () => {
    const keys = new ActionIdempotencyKeys(counter());
    const a = keys.keyFor("GitPush", [{ repo: "/r", workspace: "7" }]);
    expect(keys.keyFor("GitPush", [{ workspace: "7", repo: "/r" }])).toBe(a);
  });

  it("keeps an ambiguous attempt's key across a long reconnect", () => {
    vi.useFakeTimers();
    try {
      const keys = new ActionIdempotencyKeys(counter());
      const first = keys.keyFor("SplitCommit", ["/r", "abc"]);
      keys.settle(first, { status: "ambiguous", reason: "reset" });
      vi.advanceTimersByTime(6 * 60 * 60 * 1000);
      // Elapsed time does not show the first attempt never landed.
      expect(keys.keyFor("SplitCommit", ["/r", "abc"])).toBe(first);
      keys.settle(first, { status: "already_applied" });
      expect(keys.keyFor("SplitCommit", ["/r", "abc"])).not.toBe(first);
    } finally {
      vi.useRealTimers();
    }
  });

  it("releases pending keys by scope, or all of them", () => {
    const keys = new ActionIdempotencyKeys(counter());
    const a = keys.keyFor("GitPush", ["/r"], "repo-a");
    const b = keys.keyFor("GitPush", ["/r"], "repo-b");
    expect(a).not.toBe(b);
    keys.release("repo-a");
    expect(keys.keyFor("GitPush", ["/r"], "repo-a")).not.toBe(a);
    expect(keys.keyFor("GitPush", ["/r"], "repo-b")).toBe(b);
    keys.release();
    expect(keys.keyFor("GitPush", ["/r"], "repo-b")).not.toBe(b);
  });

  it("does not build keys from the clock", () => {
    const keys = new ActionIdempotencyKeys(() => "fixed");
    expect(keys.keyFor("agent-start", ["/r", "ws"])).toBe("agent-start:fixed");
  });
});
