// @include-parallel
import { describe, expect, it } from "vitest";
import {
  createCommit,
  createWorkspace,
  getWorkspaces,
} from "../../src/lib/api";
import { dispatchLocal } from "../../src/lib/remote-dispatch";
import {
  createTestRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../utils";

describe("remote branch switch command", () => {
  it("moves the repository working copy onto another branch", async () => {
    const { repoPath } = createTestRepo(false);
    const id = await createWorkspace(repoPath, "feat/switch-to");
    const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id);
    if (!ws) throw new Error("workspace missing");
    writeWorkspaceFile(
      resolveWorkspacePath(repoPath, ws.workspace_path),
      "b.txt",
      "b\n",
    );
    await createCommit(repoPath, id, "branch commit");

    await dispatchLocal({
      kind: "SwitchRepoBranch",
      repo: repoPath,
      bookmark: "feat/switch-to",
    });

    const inspected = await dispatchLocal<{ current_branch: string | null }>({
      kind: "InspectRepository",
      repo: repoPath,
    });
    expect(inspected.current_branch).toBe("feat/switch-to");
  });
});
