// @include-parallel
import { describe, expect, it } from "vitest";
import {
  createCommit,
  createWorkspace,
  getWorkspaces,
  listCommits,
} from "../../src/lib/api";
import { dispatchLocal } from "../../src/lib/remote-dispatch";
import {
  createTestRepo,
  resolveChangeId,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../utils";

describe("remote revert command", () => {
  it("adds a commit reversing the chosen commit", async () => {
    const { repoPath } = createTestRepo(false);
    const id = await createWorkspace(repoPath, "feat/revert");
    const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id);
    if (!ws) throw new Error("workspace missing");
    const wsPath = resolveWorkspacePath(repoPath, ws.workspace_path);
    writeWorkspaceFile(wsPath, "a.txt", "a\n");
    await createCommit(repoPath, id, "add a");
    const before = (await listCommits(repoPath, id)).commits.length;

    await dispatchLocal({
      kind: "RevertCommit",
      repo: repoPath,
      workspace: String(id),
      commit: resolveChangeId(wsPath, "@-"),
      idempotency_key: "revert-1",
    });

    expect((await listCommits(repoPath, id)).commits.length).toBe(before + 1);
  });
});
