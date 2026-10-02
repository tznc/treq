// @include-parallel
import fs from "node:fs";
import { describe, expect, it } from "vitest";
import { createWorkspace, getWorkspaces } from "../../src/lib/api";
import { dispatchLocal } from "../../src/lib/remote-dispatch";
import { createTestRepo, resolveWorkspacePath } from "../utils";

describe("remote archive command", () => {
  it("archives a workspace so it leaves the list and its directory", async () => {
    const { repoPath } = createTestRepo(false);
    const id = await createWorkspace(repoPath, "feat/archive-me");
    const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id);
    if (!ws) throw new Error("workspace missing");
    const wsPath = resolveWorkspacePath(repoPath, ws.workspace_path);

    await dispatchLocal({
      kind: "ArchiveWorkspace",
      repo: repoPath,
      workspace: String(id),
    });

    const listed = await dispatchLocal<{ id: number }[]>({
      kind: "ListWorkspaces",
      repo: repoPath,
    });
    expect(listed.map((w) => w.id)).not.toContain(id);
    expect(fs.existsSync(wsPath)).toBe(false);
  });
});
