// @include-parallel
import { describe, expect, it } from "vitest";
import {
  createWorkspace,
  getWorkspaces,
  listStashes,
  type StashEntry,
} from "../../src/lib/api";
import { dispatchLocal } from "../../src/lib/remote-dispatch";
import {
  createTestRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../utils";

describe("remote stash command", () => {
  it("stashes a workspace's changes into the repository's stash list", async () => {
    const { repoPath } = createTestRepo(false);
    const id = await createWorkspace(repoPath, "feat/stash-me");
    const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id);
    if (!ws) throw new Error("workspace missing");
    writeWorkspaceFile(
      resolveWorkspacePath(repoPath, ws.workspace_path),
      "wip.txt",
      "wip\n",
    );
    const target = { repo: repoPath, workspace: String(id) };

    const entry = await dispatchLocal<StashEntry>({
      kind: "StashWorkspaceChanges",
      ...target,
      idempotency_key: "stash-1",
    });

    expect((await listStashes(repoPath)).map((s) => s.id)).toEqual([entry.id]);
    expect(await dispatchLocal({ kind: "ListChanges", ...target })).toEqual([]);
  });
});
