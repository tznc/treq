// @include-parallel
import { describe, expect, it } from "vitest";
import {
  createCommit,
  createWorkspace,
  getWorkspaces,
  pushWorkspaceToRemote,
} from "../../src/lib/api";
import type { PullWorkspaceResult } from "../../src/lib/api-types";
import { dispatchLocal } from "../../src/lib/remote-dispatch";
import {
  createTestRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../utils";

describe("remote pull command", () => {
  it("pulls a pushed workspace from its remote branch", async () => {
    const { repoPath } = createTestRepo(true);
    const id = await createWorkspace(repoPath, "feat/pull-me");
    const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id);
    if (!ws) throw new Error("workspace missing");
    writeWorkspaceFile(
      resolveWorkspacePath(repoPath, ws.workspace_path),
      "pulled.txt",
      "content\n",
    );
    await createCommit(repoPath, id, "pushed commit");
    await pushWorkspaceToRemote(repoPath, id);

    const result = await dispatchLocal<PullWorkspaceResult>({
      kind: "PullWorkspace",
      repo: repoPath,
      workspace: String(id),
    });
    expect(result).toMatchObject({ success: true, has_conflicts: false });
  });
});
