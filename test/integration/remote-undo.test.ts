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

describe("remote undo command", () => {
  it("undoes an abandon by its operation id", async () => {
    const { repoPath } = createTestRepo(false);
    const id = await createWorkspace(repoPath, "feat/undo");
    const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id);
    if (!ws) throw new Error("workspace missing");
    const wsPath = resolveWorkspacePath(repoPath, ws.workspace_path);
    writeWorkspaceFile(wsPath, "a.txt", "a\n");
    await createCommit(repoPath, id, "keep this commit");
    const changeId = resolveChangeId(wsPath, "@-");
    const descriptions = async () =>
      (await listCommits(repoPath, id)).commits.map((c) => c.description);

    const operationId = await dispatchLocal<string>({
      kind: "AbandonCommit",
      repo: repoPath,
      workspace: String(id),
      commit: changeId,
      idempotency_key: "abandon-1",
    });
    expect(await descriptions()).not.toContain("keep this commit");

    await dispatchLocal({
      kind: "UndoOperation",
      repo: repoPath,
      workspace: String(id),
      operation_id: operationId,
    });
    expect(await descriptions()).toContain("keep this commit");
  });
});
