import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stashWorkspaceChanges, undoCommit } from "./api";
import { setActiveRepositorySingleton } from "./active-repository";
import type { SshEndpoint } from "./api-types-remote";
import {
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

function sentMutations(): TreqCommandRequest[] {
  return vi
    .mocked(dispatchMutationOverSsh)
    .mock.calls.map(([, request]) => request);
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
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
