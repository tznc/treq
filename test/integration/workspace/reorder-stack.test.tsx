// @include-parallel
import { clickGpuiRow } from "../../gpui-sidebar";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getWorkspaces,
  setWorkspaceTargetBranch,
  updateWorkspace,
} from "../../../src/lib/api";
import type { WorkspaceSidebarStatus } from "../../../src/lib/api-types";
import { getFullWorkspacePath } from "../../../src/lib/utils";
import {
  buildWorkspaceTree,
  flattenWorkspaceTree,
} from "../../../src/lib/workspace-tree";
import { render, screen, waitFor } from "../../test-utils";
import { createTestRepo, openRepo } from "../../utils";

function sidebarOrder(workspaces: Awaited<ReturnType<typeof getWorkspaces>>) {
  const statuses: WorkspaceSidebarStatus[] = workspaces.map((current) => ({
    current,
    has_conflicts: false,
  }));
  return flattenWorkspaceTree(buildWorkspaceTree(statuses)).map((node) => ({
    branch: node.branchName,
    depth: node.depth,
  }));
}

describe("workspace sidebar stack reorder", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("moves a parent below its child without creating a cycle", async () => {
    await createWorkspace(repoPath, "feat/parent");
    await createWorkspace(repoPath, "feat/child");

    const initial = await getWorkspaces(repoPath);
    const parent = initial.find((w) => w.branch_name === "feat/parent")!;
    const child = initial.find((w) => w.branch_name === "feat/child")!;

    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(child),
      child.id,
      "feat/parent",
    );

    expect(sidebarOrder(await getWorkspaces(repoPath))).toEqual([
      { branch: "feat/parent", depth: 0 },
      { branch: "feat/child", depth: 1 },
    ]);

    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(parent),
      parent.id,
      "feat/child",
    );

    const after = await getWorkspaces(repoPath);
    expect(
      after.find((w) => w.branch_name === "feat/child")?.target_branch,
    ).toBe(parent.target_branch ?? "main");
    expect(
      after.find((w) => w.branch_name === "feat/parent")?.target_branch,
    ).toBe("feat/child");
    expect(sidebarOrder(after)).toEqual([
      { branch: "feat/child", depth: 0 },
      { branch: "feat/parent", depth: 1 },
    ]);
  });

  it("moves a three-level root below the tip by lifting the bridge", async () => {
    await createWorkspace(repoPath, "feat/a");
    await createWorkspace(repoPath, "feat/b");
    await createWorkspace(repoPath, "feat/c");

    const initial = await getWorkspaces(repoPath);
    const a = initial.find((w) => w.branch_name === "feat/a")!;
    const b = initial.find((w) => w.branch_name === "feat/b")!;
    const c = initial.find((w) => w.branch_name === "feat/c")!;
    const rootTarget = a.target_branch ?? "main";

    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(b),
      b.id,
      "feat/a",
    );
    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(c),
      c.id,
      "feat/b",
    );

    expect(sidebarOrder(await getWorkspaces(repoPath))).toEqual([
      { branch: "feat/a", depth: 0 },
      { branch: "feat/b", depth: 1 },
      { branch: "feat/c", depth: 2 },
    ]);

    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(a),
      a.id,
      "feat/c",
    );

    const after = await getWorkspaces(repoPath);
    expect(after.find((w) => w.branch_name === "feat/b")?.target_branch).toBe(
      rootTarget,
    );
    expect(after.find((w) => w.branch_name === "feat/c")?.target_branch).toBe(
      "feat/b",
    );
    expect(after.find((w) => w.branch_name === "feat/a")?.target_branch).toBe(
      "feat/c",
    );
    expect(sidebarOrder(after)).toEqual([
      { branch: "feat/b", depth: 0 },
      { branch: "feat/c", depth: 1 },
      { branch: "feat/a", depth: 2 },
    ]);
  });

  it("reorders parent below child via the workspace target selector", async () => {
    await createWorkspace(repoPath, "feat/parent");
    await createWorkspace(repoPath, "feat/child");

    const initial = await getWorkspaces(repoPath);
    const child = initial.find((w) => w.branch_name === "feat/child")!;
    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(child),
      child.id,
      "feat/parent",
    );

    render(<Dashboard />);

    await clickGpuiRow("feat/parent");

    const targetBtn = await screen.findByRole("button", {
      name: "Workspace target",
    });
    await waitFor(() => expect(targetBtn).not.toBeDisabled());
    await user.click(targetBtn);

    await user.click(
      await screen.findByText("feat/child", {
        selector: ".branch-list-item *",
      }),
    );

    await waitFor(async () => {
      const after = await getWorkspaces(repoPath);
      expect(
        after.find((w) => w.branch_name === "feat/parent")?.target_branch,
      ).toBe("feat/child");
      expect(sidebarOrder(after)).toEqual([
        { branch: "feat/child", depth: 0 },
        { branch: "feat/parent", depth: 1 },
      ]);
    });
  });

  it("retargets through updateWorkspace (CLI path) without cycles", async () => {
    await createWorkspace(repoPath, "feat/parent");
    await createWorkspace(repoPath, "feat/child");

    const initial = await getWorkspaces(repoPath);
    const parent = initial.find((w) => w.branch_name === "feat/parent")!;
    const child = initial.find((w) => w.branch_name === "feat/child")!;

    await updateWorkspace(repoPath, child.id, "feat/parent");
    await updateWorkspace(repoPath, parent.id, "feat/child");

    const after = await getWorkspaces(repoPath);
    expect(
      after.find((w) => w.branch_name === "feat/parent")?.target_branch,
    ).toBe("feat/child");
    expect(sidebarOrder(after)).toEqual([
      { branch: "feat/child", depth: 0 },
      { branch: "feat/parent", depth: 1 },
    ]);
  });
});
