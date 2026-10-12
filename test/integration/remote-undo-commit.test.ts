// @include-parallel
import fs from "node:fs";
import path from "node:path";
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

describe("remote commit undo command", () => {
  it("abandons the workspace's tip commit", async () => {
    const { repoPath } = createTestRepo(false);
    const id = await createWorkspace(repoPath, "feat/undo-commit");
    const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id);
    if (!ws) throw new Error("workspace missing");
    const wsPath = resolveWorkspacePath(repoPath, ws.workspace_path);
    writeWorkspaceFile(wsPath, "a.txt", "a\n");
    await createCommit(repoPath, id, "add a");

    await dispatchLocal({
      kind: "UndoCommit",
      repo: repoPath,
      workspace: String(id),
      commit: resolveChangeId(wsPath, "@-"),
    });

    const log = await listCommits(repoPath, id);
    expect(log.commits.map((c) => c.description)).not.toContain("add a");
    expect(fs.existsSync(path.join(wsPath, "a.txt"))).toBe(false);
  });
});
