// @include-parallel
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { createWorkspace, getWorkspaces } from "../../src/lib/api";
import { dispatchLocal } from "../../src/lib/remote-dispatch";
import {
  createTestRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../utils";

describe("remote discard commands", () => {
  it("snapshots a workspace, discards all of its changes, then restores them", async () => {
    const { repoPath } = createTestRepo(false);
    const id = await createWorkspace(repoPath, "feat/discard");
    const ws = (await getWorkspaces(repoPath)).find((w) => w.id === id);
    if (!ws) throw new Error("workspace missing");
    const wsPath = resolveWorkspacePath(repoPath, ws.workspace_path);
    writeWorkspaceFile(wsPath, "draft.txt", "keep me\n");
    const target = { repo: repoPath, workspace: String(id) };
    expect(
      await dispatchLocal({ kind: "ListChanges", ...target }),
    ).toHaveLength(1);

    const snapshotId = await dispatchLocal<string>({
      kind: "SnapshotWorkingCopy",
      ...target,
    });
    await dispatchLocal({ kind: "RestoreAll", ...target });
    expect(fs.existsSync(path.join(wsPath, "draft.txt"))).toBe(false);
    expect(await dispatchLocal({ kind: "ListChanges", ...target })).toEqual([]);

    await dispatchLocal({
      kind: "RestoreSnapshot",
      ...target,
      snapshot_id: snapshotId,
    });
    expect(fs.readFileSync(path.join(wsPath, "draft.txt"), "utf8")).toBe(
      "keep me\n",
    );
  });
});
