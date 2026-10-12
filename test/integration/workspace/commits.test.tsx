// @include-serial
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen, waitFor } from "../../test-utils";
import {
  commitRepoFile,
  commitWorkspaceFile,
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import * as api from "../../../src/lib/api";
import { Dashboard } from "../../../src/components/Dashboard";

type WorkspaceRef = { id: number; path: string };

async function createWorkspaceRef(
  repoPath: string,
  branchName: string,
): Promise<WorkspaceRef> {
  await api.createWorkspace(repoPath, branchName);
  const workspace = (await api.getWorkspaces(repoPath)).find(
    (candidate) => candidate.branch_name === branchName,
  );
  expect(workspace).toBeTruthy();
  return { id: workspace!.id, path: workspace!.workspace_path };
}

async function openWorkspaceCommitsTab(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  render(<Dashboard />);
  await user.click(await findSidebarBranchElement(branchName));
  await user.click(await screen.findByRole("tab", { name: /^Commits/ }));
}

describe("ShowWorkspace - Commits tab", () => {
  let repoPath: string;

  let testWorkspace: WorkspaceRef;
  let user: ReturnType<typeof userEvent.setup>;
  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    console.log("repoPath", repoPath);
    openRepo(repoPath);
    user = userEvent.setup();
    await Array.from({ length: 13 }, (_, index) => index).reduce(
      (chain, idx) =>
        chain.then(() =>
          commitRepoFile(
            repoPath,
            `target-pagination-${idx}.txt`,
            `target pagination content ${idx}`,
            `Target pagination commit ${idx}`,
          ),
        ),
      Promise.resolve(),
    );

    testWorkspace = await createWorkspaceRef(repoPath, "feat/commits-it");
    await commitWorkspaceFile(
      repoPath,
      testWorkspace,
      "README.md",
      "commits-tab-diff-line-one",
      "Commits diff one",
    );
    await commitWorkspaceFile(
      repoPath,
      testWorkspace,
      "commit-page.txt",
      "commits-tab-diff-line-two",
      "Commits diff two",
    );
  });

  it("renders commit contents and shows per-commit diff", async () => {
    await openWorkspaceCommitsTab(user, "feat/commits-it");

    await screen.findByText("Commits diff two");
    expect((await screen.findAllByText("Today")).length).toBeGreaterThan(0);
    const commit = await screen.findByText("Commits diff two");
    await user.click(commit);
    await screen.findByText("commit-page.txt");
    await screen.findByText("commits-tab-diff-line-two");
  });

  it("defers rendering file diffs larger than 500 changed lines until requested", async () => {
    const largeDiffContent = Array.from(
      { length: 501 },
      (_, index) => `large diff line ${index + 1}`,
    ).join("\n");

    await commitWorkspaceFile(
      repoPath,
      testWorkspace,
      "huge-diff.txt",
      largeDiffContent,
      "Commits huge diff",
    );

    await openWorkspaceCommitsTab(user, "feat/commits-it");

    const commit = await screen.findByText("Commits huge diff");
    await user.click(commit);

    await screen.findByText("huge-diff.txt");
    expect(screen.queryByText("large diff line 501")).toBeNull();

    const loadDiffButton = screen.getByRole("button", {
      name: "Load diff",
    });
    await user.click(loadDiffButton);

    await screen.findByText("large diff line 501");
  });

  it("shows a blocked message for commit diffs over 10k changed lines", async () => {
    const hugeDiffContent = Array.from(
      { length: 10001 },
      (_, index) => `huge diff line ${index + 1}`,
    ).join("\n");

    await commitWorkspaceFile(
      repoPath,
      testWorkspace,
      "huge-commit.txt",
      hugeDiffContent,
      "Commits huge cutoff",
    );

    await openWorkspaceCommitsTab(user, "feat/commits-it");

    const commit = await screen.findByText("Commits huge cutoff");
    await user.click(commit);

    await screen.findByText(
      "This commit changes more than 10,000 lines and is too large to render.",
    );
    expect(screen.queryByText("huge-commit.txt")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Load diff" }),
    ).not.toBeInTheDocument();
  });

  it("shows workspace and target sections with divider for non-default workspace branch", async () => {
    await openWorkspaceCommitsTab(user, "feat/commits-it");

    await screen.findByText("Commits diff two");
    await screen.findByText(/^Recent on /);
  });

  it("shows the target section for a non-default workspace with no extra commits", async () => {
    const noCommitWorkspace = await createWorkspaceRef(
      repoPath,
      "feat/no-workspace-commits",
    );
    expect(noCommitWorkspace.id).toBeGreaterThan(0);

    await openWorkspaceCommitsTab(user, "feat/no-workspace-commits");

    await screen.findByText(/^Recent on /);
    await screen.findByText("Target pagination commit 12");
    await screen.findByText(
      "There are no commits within this workspace branch yet.",
    );
  });

  it("renders overlapping workspace and target commits only once each", async () => {
    await commitRepoFile(
      repoPath,
      "target-overlap.txt",
      "target overlap content",
      "Target overlap commit",
    );

    const overlapWorkspace = await createWorkspaceRef(
      repoPath,
      "feat/disjoint-commits",
    );

    await commitWorkspaceFile(
      repoPath,
      overlapWorkspace,
      "workspace-overlap.txt",
      "workspace overlap content",
      "Workspace overlap commit",
    );

    await openWorkspaceCommitsTab(user, "feat/disjoint-commits");

    await screen.findByText("Workspace overlap commit");
    await screen.findByText("Target overlap commit");
    await screen.findByText(/^Recent on /);
    expect(screen.getAllByText("Workspace overlap commit")).toHaveLength(1);
    expect(screen.getAllByText("Target overlap commit")).toHaveLength(1);
  });

  it("does not render target divider or workspace-empty message for default-branch workspace", async () => {
    const mainWorkspace = await createWorkspaceRef(repoPath, "main");
    await commitWorkspaceFile(
      repoPath,
      mainWorkspace,
      "default-branch-workspace.txt",
      "default branch workspace content",
      "Default branch workspace commit",
    );

    await openWorkspaceCommitsTab(user, "main");

    await screen.findByText("Initial commit");
    expect(screen.queryByText(/^Recent on /)).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "There are no commits within this workspace branch yet.",
      ),
    ).not.toBeInTheDocument();
  });
});

describe("ShowWorkspace - Commits tab single-commit regression", () => {
  let repoPath: string;
  let testWorkspace: WorkspaceRef;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();

    await commitRepoFile(
      repoPath,
      "target-baseline.txt",
      "target baseline content",
      "Target baseline commit",
    );

    testWorkspace = await createWorkspaceRef(
      repoPath,
      "feat/one-commit-visible",
    );
    await commitWorkspaceFile(
      repoPath,
      testWorkspace,
      "README.md",
      "single-workspace-commit-content",
      "Single workspace commit should render",
    );
  });

  it("shows a single workspace commit instead of the empty state", async () => {
    await openWorkspaceCommitsTab(user, "feat/one-commit-visible");

    await screen.findByText("Single workspace commit should render");
    expect(screen.queryByText("No commits yet.")).not.toBeInTheDocument();
  });
});

describe("ShowWorkspace - Commits tab tentative working copy", () => {
  let repoPath: string;
  let dirtyWorkspace: WorkspaceRef;

  beforeEach(async () => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
  });

  it("navigates to the Commits tab and expands the tentative diff when clicked from the Code sidebar", async () => {
    await commitRepoFile(
      repoPath,
      "base.txt",
      "base content",
      "Base repo commit",
    );

    dirtyWorkspace = await createWorkspaceRef(
      repoPath,
      "feat/tentative-sidebar-click",
    );

    const workspacePath = resolveWorkspacePath(repoPath, dirtyWorkspace.path);
    writeWorkspaceFile(
      workspacePath,
      "sidebar-tentative.txt",
      "tentative sidebar click content",
    );

    const dirtyUser = userEvent.setup();
    render(<Dashboard />);
    await dirtyUser.click(
      await findSidebarBranchElement("feat/tentative-sidebar-click"),
    );

    await dirtyUser.click(await screen.findByText("Working copy"));

    expect(
      await screen.findByRole("tab", { name: /^Commits/, selected: true }),
    ).toBeInTheDocument();
    await screen.findByText("sidebar-tentative.txt");
    await screen.findByText("tentative sidebar click content");
  });
});

describe("ShowWorkspace - Commits tab timestamp edit", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("shifts a mutable commit forward by days from the Commits tab", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    const workspace = await createWorkspaceRef(repoPath, "feat/edit-timestamp");
    await commitWorkspaceFile(
      repoPath,
      workspace,
      "ts-a.txt",
      "a",
      "Timestamp commit A",
    );
    await commitWorkspaceFile(
      repoPath,
      workspace,
      "ts-b.txt",
      "b",
      "Timestamp commit B",
    );

    await openWorkspaceCommitsTab(user, "feat/edit-timestamp");
    const commitA = await screen.findByTitle("Timestamp commit A");
    await user.click(commitA.closest("button") ?? commitA);
    await screen.findByRole("button", { name: "Edit description" });
    await user.click(await screen.findByTestId(/edit-timestamp-/));

    const daysInput = await screen.findByLabelText("Days");
    await user.clear(daysInput);
    await user.type(daysInput, "2");
    await user.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => {
      expect(screen.queryByTestId("modal")).not.toBeInTheDocument();
    });
    await screen.findByText("Timestamp updated");
    const logs = await api.listCommits(repoPath, workspace.id);
    const older = logs.commits.find(
      (commit) => commit.description === "Timestamp commit A",
    );
    const newer = logs.commits.find(
      (commit) => commit.description === "Timestamp commit B",
    );
    expect(older).toBeTruthy();
    expect(newer).toBeTruthy();
    const olderMs = Date.parse(older!.timestamp);
    const newerMs = Date.parse(newer!.timestamp);
    expect(newerMs).toBeGreaterThan(olderMs);
    expect(olderMs).toBeGreaterThan(Date.now() + 24 * 60 * 60 * 1000);
  });

  it("shifts the newest mutable commits to now", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    const workspace = await createWorkspaceRef(repoPath, "feat/shift-to-now");
    await commitWorkspaceFile(
      repoPath,
      workspace,
      "now-a.txt",
      "a",
      "Now commit A",
    );
    await commitWorkspaceFile(
      repoPath,
      workspace,
      "now-b.txt",
      "b",
      "Now commit B",
    );

    const before = await api.listCommits(repoPath, workspace.id);
    const older = before.commits.find(
      (commit) => commit.description === "Now commit A",
    );
    expect(older).toBeTruthy();
    await api.shiftCommitTimestamp(repoPath, workspace.id, older!.change_id, {
      days: 2,
    });

    await openWorkspaceCommitsTab(user, "feat/shift-to-now");
    await user.click(await screen.findByTestId("shift-commits-to-now"));
    await screen.findByText("Timestamps updated");

    const after = await api.listCommits(repoPath, workspace.id);
    const newer = after.commits.find(
      (commit) => commit.description === "Now commit B",
    );
    expect(newer).toBeTruthy();
    expect(Math.abs(Date.parse(newer!.timestamp) - Date.now())).toBeLessThan(
      10_000,
    );
  });
});
