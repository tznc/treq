import { describe, expect, it, vi } from "vitest";
import {
  findWorkspaceByBranch,
  isProcessedAgentRequest,
  markProcessedAgentRequest,
  parseAgentDeepLinkUrl,
  popPendingAgentRequests,
  processAgentDeepLinkRequests,
  queuePendingAgentRequest,
  tryClaimAgentRequest,
} from "./agentDeepLink";
import type { Workspace } from "./api";

describe("agent deep-link helpers", () => {
  it("parses valid agent deep link", () => {
    const parsed = parseAgentDeepLinkUrl(
      "treq://agent/start?repo=%2Ftmp%2Frepo&branch=feat%2Fone&prompt=hello&mode=edit&agent=codex&request_id=req-1",
    );
    expect(parsed).toEqual({
      repo: "/tmp/repo",
      branch: "feat/one",
      prompt: "hello",
      mode: "acceptEdits",
      agent: "codex",
      requestId: "req-1",
    });
  });

  it("rejects malformed deep link payload", () => {
    expect(parseAgentDeepLinkUrl("treq://auth/callback?token=abc")).toBeNull();
    expect(
      parseAgentDeepLinkUrl(
        "treq://agent/start?repo=a&branch=b&prompt=c&mode=bad&agent=codex&request_id=1",
      ),
    ).toBeNull();
  });

  it("tracks processed request ids", () => {
    expect(isProcessedAgentRequest("req-processed")).toBe(false);
    markProcessedAgentRequest("req-processed");
    expect(isProcessedAgentRequest("req-processed")).toBe(true);
  });

  it("allows only the first claim for a request id", () => {
    expect(tryClaimAgentRequest("req-claim")).toBe(true);
    expect(tryClaimAgentRequest("req-claim")).toBe(false);
  });

  it("queues and pops pending requests per repo without duplication", () => {
    const request = {
      repo: "/repo",
      branch: "feat/a",
      prompt: "do thing",
      mode: "plan" as const,
      agent: "claude" as const,
      requestId: "req-queue",
    };
    queuePendingAgentRequest(request);
    queuePendingAgentRequest(request);
    const popped = popPendingAgentRequests("/repo");
    expect(popped).toEqual([request]);
    expect(popPendingAgentRequests("/repo")).toEqual([]);
  });

  it("finds workspace by branch", () => {
    const workspaces = [
      { id: 1, branch_name: "main" },
      { id: 2, branch_name: "feat/x" },
    ] as Workspace[];
    expect(findWorkspaceByBranch(workspaces, "feat/x")?.id).toBe(2);
    expect(findWorkspaceByBranch(workspaces, "missing")).toBeNull();
  });
});

describe("processAgentDeepLinkRequests with supporting repositories", () => {
  const request = (repo: string, requestId: string) => ({
    repo,
    branch: "feat/x",
    prompt: "do it",
    mode: "acceptEdits" as const,
    agent: "claude" as const,
    requestId,
  });

  it("handles a supporting-repository request in this window without deferring", async () => {
    const onSameRepoRequest = vi.fn(async () => {});
    const deferRequest = vi.fn();
    const openOtherRepoWindow = vi.fn();

    await processAgentDeepLinkRequests([request("/repos/api", "sup-1")], {
      repoPath: "/repos/app",
      supportingRepoPaths: ["/repos/api"],
      workspacesLength: 0,
      onSameRepoRequest,
      deferRequest,
      openOtherRepoWindow,
    });

    expect(onSameRepoRequest).toHaveBeenCalledTimes(1);
    expect(deferRequest).not.toHaveBeenCalled();
    expect(openOtherRepoWindow).not.toHaveBeenCalled();
  });

  it("opens another window for a repository that is not linked", async () => {
    const openOtherRepoWindow = vi.fn();

    await processAgentDeepLinkRequests([request("/repos/other", "sup-2")], {
      repoPath: "/repos/app",
      supportingRepoPaths: ["/repos/api"],
      workspacesLength: 3,
      onSameRepoRequest: vi.fn(async () => {}),
      deferRequest: vi.fn(),
      openOtherRepoWindow,
    });

    expect(openOtherRepoWindow).toHaveBeenCalledTimes(1);
  });
});
