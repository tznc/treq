import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  applyAgentReviewSuggestion,
  archiveWorkspace,
  checkBranchExists,
  getGitRemoteUrl,
  getRepoSetting,
  listAgentReviewComments,
  pullWorkspaceFromRemote,
  revertCommit,
  setRepoSetting,
  stashWorkspaceChanges,
  undoCommit,
} from "./api";
import { repoStateScope } from "./repo-state-scope";
import { setActiveRepositorySingleton } from "./active-repository";
import type { SshEndpoint } from "./api-types-remote";
import {
  dispatch,
  dispatchMutationOverSsh,
  type TreqCommandRequest,
} from "./remote-dispatch";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

vi.mock("./remote-dispatch", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./remote-dispatch")>();
  return { ...actual, dispatch: vi.fn(), dispatchMutationOverSsh: vi.fn() };
});

vi.mock("./swr-cache", () => ({
  invalidateQueries: vi.fn(() => Promise.resolve()),
}));

const ROOT = "/srv/project";
const endpoint = { id: "endpoint-1" } as unknown as SshEndpoint;

function sentReads(): TreqCommandRequest[] {
  return vi.mocked(dispatch).mock.calls.map(([, request]) => request);
}

function sentMutations(): TreqCommandRequest[] {
  return vi
    .mocked(dispatchMutationOverSsh)
    .mock.calls.map(([, request]) => request);
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(dispatch).mockReset();
  vi.mocked(dispatchMutationOverSsh).mockReset();
  vi.mocked(dispatchMutationOverSsh).mockResolvedValue({
    status: "applied",
    value: "ok",
  });
  setActiveRepositorySingleton({
    id: "remote-1",
    location: { type: "ssh", host: "box", path: ROOT },
    endpoint,
    endpointId: "endpoint-1",
    endpointGeneration: 1,
    canonicalPath: ROOT,
    displayName: "project",
    transport: { type: "ssh", endpoint },
  });
});

afterEach(() => {
  setActiveRepositorySingleton(null);
});

describe("remote commit undo", () => {
  it("sends UndoCommit for the workspace tip", async () => {
    await undoCommit(ROOT, 7, "abc");
    expect(sentMutations()).toEqual([
      { kind: "UndoCommit", repo: ROOT, workspace: "7", commit: "abc" },
    ]);
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("remote stash", () => {
  it("stashes the working copy with an idempotency key", async () => {
    await stashWorkspaceChanges(ROOT, 7);
    const [stash] = sentMutations();
    expect(stash).toMatchObject({
      kind: "StashWorkspaceChanges",
      repo: ROOT,
      workspace: "7",
    });
    expect((stash as { idempotency_key: string }).idempotency_key).toBeTruthy();
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("remote routes", () => {
  it("reads the GitHub remote of a remote repository on its host", async () => {
    const info = { owner: "acme", repo: "widgets", full_name: "acme/widgets" };
    vi.mocked(dispatch).mockResolvedValueOnce(info);
    await expect(getGitRemoteUrl(ROOT)).resolves.toEqual(info);
    expect(sentReads()).toEqual([{ kind: "GitRemoteInfo", repo: ROOT }]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("checks a remote branch through the typed ListBranches read", async () => {
    vi.mocked(dispatch).mockResolvedValue([{ name: "feat/a" }]);
    await expect(checkBranchExists(ROOT, "feat/a")).resolves.toEqual({
      local_exists: true,
      remote_exists: false,
    });
    expect(sentReads()).toEqual([{ kind: "ListBranches", repo: ROOT }]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("reads default repo settings and scopes agent review comments", async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    await expect(getRepoSetting(ROOT, "default_agent")).resolves.toBeNull();
    await expect(setRepoSetting(ROOT, "default_agent", "x")).rejects.toThrow(
      /^unsupported:/,
    );
    await expect(applyAgentReviewSuggestion(ROOT, "c1")).rejects.toThrow(
      /^unsupported:/,
    );
    await listAgentReviewComments(ROOT, "workspace", "7");
    expect(vi.mocked(invoke).mock.calls).toEqual([
      [
        "list_agent_review_comments",
        {
          repoPath: repoStateScope(ROOT),
          targetType: "workspace",
          targetId: "7",
        },
      ],
    ]);
  });

  it("archives a remote workspace through the typed command", async () => {
    await archiveWorkspace(ROOT, 7);
    expect(sentMutations()).toEqual([
      { kind: "ArchiveWorkspace", repo: ROOT, workspace: "7" },
    ]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("pulls a remote workspace through the typed command", async () => {
    await pullWorkspaceFromRemote(ROOT, 7);
    expect(sentMutations()).toEqual([
      { kind: "PullWorkspace", repo: ROOT, workspace: "7" },
    ]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("reverts a remote commit with an idempotency key", async () => {
    await revertCommit(ROOT, 7, "abc");
    const [revert] = sentMutations();
    expect(revert).toMatchObject({
      kind: "RevertCommit",
      repo: ROOT,
      workspace: "7",
      commit: "abc",
    });
    expect(
      (revert as { idempotency_key: string }).idempotency_key,
    ).toBeTruthy();
    expect(invoke).not.toHaveBeenCalled();
  });
});
