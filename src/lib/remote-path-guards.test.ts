import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setActiveRepositorySingleton } from "./active-repository";
import * as api from "./api";
import type { SshEndpoint } from "./api-types-remote";
import { dispatch, dispatchMutationOverSsh } from "./remote-dispatch";
import { RemoteOperationUnsupportedError } from "./repository-adapter";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./remote-dispatch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./remote-dispatch")>()),
  dispatch: vi.fn(() => Promise.resolve(null)),
  dispatchMutationOverSsh: vi.fn(() => Promise.resolve({ status: "applied" })),
}));
vi.mock("./swr-cache", () => ({
  invalidateQueries: vi.fn(() => Promise.resolve()),
  setQueryData: vi.fn(() => Promise.resolve()),
}));

const ROOT = "/srv/guarded";
const PATH_FIRST = /^(?:async\s*)?\(\s*(repoPath|workspacePath|path|paths)\b/;
// Takes temporary agent CLI files, not repository paths.
const NOT_REPOSITORY_PATHS = new Set(["cleanupAgentCliFiles"]);

// Operations with a typed remote command: each must send that command and
// never fall back to a local guard.
const ROUTED: [string, () => Promise<unknown>, string][] = [
  [
    "stashWorkspaceChanges",
    () => api.stashWorkspaceChanges(ROOT, 1),
    "StashWorkspaceChanges",
  ],
  [
    "pullWorkspaceFromRemote",
    () => api.pullWorkspaceFromRemote(ROOT, 1),
    "PullWorkspace",
  ],
  ["archiveWorkspace", () => api.archiveWorkspace(ROOT, 1), "ArchiveWorkspace"],
  [
    "undoRepoOperation",
    () => api.undoRepoOperation(ROOT, 1, "op"),
    "UndoOperation",
  ],
  ["undoCommit", () => api.undoCommit(ROOT, 1, "c"), "UndoCommit"],
  ["revertCommit", () => api.revertCommit(ROOT, 1, "c"), "RevertCommit"],
  [
    "switchRepoBranch",
    () => api.switchRepoBranch(ROOT, "main"),
    "SwitchRepoBranch",
  ],
  ["jjRestoreAll", () => api.jjRestoreAll(ROOT), "RestoreAll"],
  [
    "jjSnapshotWorkingCopy",
    () => api.jjSnapshotWorkingCopy(ROOT),
    "SnapshotWorkingCopy",
  ],
  [
    "jjRestoreSnapshot",
    () => api.jjRestoreSnapshot(ROOT, "s"),
    "RestoreSnapshot",
  ],
  ["getGitRemoteUrl", () => api.getGitRemoteUrl(ROOT), "GitRemoteInfo"],
];

// Operations with no typed remote command yet: each must refuse instead of
// running locally.
const GUARDED: [string, () => Promise<unknown>][] = [
  ["stashCommit", () => api.stashCommit(ROOT, 1, "c")],
  ["listStashes", () => api.listStashes(ROOT)],
  ["applyStash", () => api.applyStash(ROOT, 1, "main")],
  ["deleteStash", () => api.deleteStash(ROOT, 1)],
  ["getStashDiff", () => api.getStashDiff(ROOT, 1)],
  ["exportStashGitPatch", () => api.exportStashGitPatch(ROOT, 1)],
  ["mergeWorkspace", () => api.mergeWorkspace(ROOT, 1, "m", "squash")],
  ["rebaseHomeRepoBranch", () => api.rebaseHomeRepoBranch(ROOT, "a", "b")],
  ["listDirectory", () => api.listDirectory(ROOT)],
  ["lsWorkspaceWithStatus", () => api.lsWorkspaceWithStatus(ROOT, 1)],
];

// Local commands that still receive a remote path. Each entry is a gap to
// close: guard it, route it, or scope it, then remove it from this list.
const KNOWN_UNGUARDED: string[] = [];

function mentionsRemotePath(value: unknown): boolean {
  if (typeof value === "string") {
    return value === ROOT || value.startsWith(`${ROOT}/`);
  }
  if (Array.isArray(value)) return value.some(mentionsRemotePath);
  if (value && typeof value === "object") {
    return Object.values(value).some(mentionsRemotePath);
  }
  return false;
}

describe("path-taking api wrappers for a remote repository", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(null);
    const endpoint = { id: "ep" } as unknown as SshEndpoint;
    setActiveRepositorySingleton({
      id: "remote",
      location: { type: "ssh", host: "box", path: ROOT },
      endpoint,
      endpointId: "ep",
      endpointGeneration: 0,
      canonicalPath: ROOT,
      displayName: "guarded",
      transport: { type: "ssh", endpoint },
    });
  });

  it("never hand a remote path to a local command", async () => {
    const wrappers = Object.entries(api).filter(
      ([name, value]) =>
        typeof value === "function" &&
        !NOT_REPOSITORY_PATHS.has(name) &&
        PATH_FIRST.test(value.toString()),
    ) as [string, (...args: unknown[]) => unknown][];
    expect(wrappers.length).toBeGreaterThan(50);

    await Promise.allSettled(
      wrappers.map(async ([, wrapper]) => {
        const first = PATH_FIRST.exec(wrapper.toString())?.[1];
        return wrapper(first === "paths" ? [ROOT] : ROOT, 1, "x", "y", "z");
      }),
    );

    const leaked = vi
      .mocked(invoke)
      .mock.calls.filter(([, args]) => mentionsRemotePath(args))
      .map(([command]) => command);
    expect([...new Set(leaked)].sort()).toEqual(KNOWN_UNGUARDED);
  });

  it.each(
    ROUTED,
  )("routes %s through its typed command", async (_, call, kind) => {
    vi.mocked(dispatch).mockClear();
    vi.mocked(dispatchMutationOverSsh).mockClear();
    await Promise.allSettled([call()]);
    const sent = [
      ...vi.mocked(dispatch).mock.calls,
      ...vi.mocked(dispatchMutationOverSsh).mock.calls,
    ].map(([, request]) => request.kind);
    expect(sent).toContain(kind);
    expect(invoke).not.toHaveBeenCalled();
  });

  it.each(GUARDED)("refuses %s for a remote repository", async (_, call) => {
    await expect(call()).rejects.toBeInstanceOf(
      RemoteOperationUnsupportedError,
    );
    expect(invoke).not.toHaveBeenCalled();
  });
});
