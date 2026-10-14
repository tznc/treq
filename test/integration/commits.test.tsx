// @include-parallel
import * as React from "react";
import { describe, expect, it } from "vitest";
import {
  commitRepoFile,
  commitWorkspaceFile,
  createTestRepo,
  openRepo,
} from "../utils";
import { createWorkspace, getWorkspaces } from "../../src/lib/api";
import { render, screen, waitFor } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import { clickGpuiRow } from "../gpui-sidebar";

describe("LinearCommitHistory integration", () => {
  it("shows workspace commits in the Code tab sidebar skipping the working copy", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    const workspaceId = await createWorkspace(repoPath, "feat/lch-ws");
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    );
    if (!workspace) throw new Error("Workspace not found");

    await commitWorkspaceFile(
      repoPath,
      { id: workspace.id, path: workspace.workspace_path },
      "lch-alpha.txt",
      "alpha content",
      "Alpha commit",
    );
    await commitWorkspaceFile(
      repoPath,
      { id: workspace.id, path: workspace.workspace_path },
      "lch-beta.txt",
      "beta content",
      "Beta commit",
    );

    render(<Dashboard />);
    await clickGpuiRow("feat/lch-ws");

    await screen.findByText("Beta commit");
    await screen.findByText("Alpha commit");
    await screen.findByText(/^Recent on /);
  });

  it("shows Load more commits button for home repo at the commit limit", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    await Array.from({ length: 13 }, (_, index) => index).reduce(
      (chain, idx) =>
        chain.then(() =>
          commitRepoFile(
            repoPath,
            `lch-paginate-${idx}.txt`,
            `paginate content ${idx}`,
            `LCH paginate commit ${idx}`,
          ),
        ),
      Promise.resolve(),
    );

    render(<Dashboard />);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Load more commits" }),
      ).toBeInTheDocument();
    });
  });

  it("does not show Load more commits button for workspace view", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    const workspaceId = await createWorkspace(repoPath, "feat/lch-no-more");
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    );
    if (!workspace) throw new Error("Workspace not found");

    await commitWorkspaceFile(
      repoPath,
      { id: workspace.id, path: workspace.workspace_path },
      "ws-commit.txt",
      "workspace content",
      "Workspace single commit",
    );

    render(<Dashboard />);
    await clickGpuiRow("feat/lch-no-more");
    await screen.findByText("Workspace single commit");

    expect(
      screen.queryByRole("button", { name: "Load more commits" }),
    ).not.toBeInTheDocument();
  });
});
