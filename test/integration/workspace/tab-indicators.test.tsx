// @include-serial
import { clickGpuiRow, clickGpuiHome } from "../../gpui-sidebar";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import {
  type Workspace,
  createCommit,
  createWorkspace,
  getWorkspaces,
  listCommits,
} from "../../../src/lib/api";
import { sumWorkspaceLocFromLog } from "../../../src/lib/workspace-stack";
import { render, screen, waitFor, within } from "../../test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { openReviewTab, waitForChangedFile } from "../review/comments-helpers";

async function setupWorkspaceWithCommittedAndUncommitted(branchName: string) {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (item) => item.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);

  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );

  writeWorkspaceFile(workspacePath, "committed-only.txt", "committed\n");
  await createCommit(repoPath, workspaceId, "commit committed-only file");

  writeWorkspaceFile(workspacePath, "uncommitted-only.txt", "working only\n");

  return { branchName, repoPath, workspaceId };
}

function getReviewBadgeCount(): number | null {
  const tab = screen.getByRole("tab", { name: /^Changes/ });
  const badge = within(tab).queryByTestId("review-change-count");
  if (!badge?.textContent) return null;
  return Number(badge.textContent);
}

async function setupOverlappingWorkspace(branchName: string) {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (item) => item.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);

  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );

  writeWorkspaceFile(workspacePath, "shared.txt", "shared v1\n");
  await createCommit(repoPath, workspaceId, "commit shared file");

  writeWorkspaceFile(workspacePath, "shared.txt", "shared v2\n");
  writeWorkspaceFile(workspacePath, "committed-only.txt", "committed only\n");
  await createCommit(repoPath, workspaceId, "commit committed-only file");

  writeWorkspaceFile(workspacePath, "shared.txt", "shared v3\n");
  writeWorkspaceFile(workspacePath, "uncommitted-only.txt", "working only\n");

  return {
    branchName,
    repoPath,
    workspaceId,
  };
}

async function setupWorkspaceWithChange(branchName: string): Promise<{
  repoPath: string;
  workspace: Workspace;
}> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error(`Workspace not found for id ${workspaceId}`);

  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace.workspace_path),
    "search-test.txt",
    "hello world line one\nhello world line two\nsome other content\n",
  );

  return { repoPath, workspace };
}

describe("ShowWorkspace tab-row LOC marker", () => {
  it("shows committed + working-copy LOC pulled right of the tab buttons", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    const branchName = "feat/loc-marker";
    const workspaceId = await createWorkspace(repoPath, branchName);
    const workspace = (await getWorkspaces(repoPath)).find(
      (item) => item.id === workspaceId,
    );
    if (!workspace)
      throw new Error(`Workspace not found for id ${workspaceId}`);

    const workspacePath = resolveWorkspacePath(
      repoPath,
      workspace.workspace_path,
    );

    writeWorkspaceFile(
      workspacePath,
      "committed.txt",
      "one\ntwo\nthree\nfour\nfive\n",
    );
    await createCommit(repoPath, workspaceId, "add committed file");

    writeWorkspaceFile(workspacePath, "working.txt", "a\nb\nc\n");

    const expected = sumWorkspaceLocFromLog(
      await listCommits(repoPath, workspaceId),
    );
    expect(expected.insertions).toBeGreaterThan(0);

    render(<Dashboard />);
    await clickGpuiRow(branchName);
    await screen.findByTestId("show-workspace-header");

    const marker = await screen.findByTestId("loc-diff-marker");
    await waitFor(() => {
      expect(marker.textContent).toContain(`+${expected.insertions}`);
      expect(marker.textContent).toContain(`-${expected.deletions}`);
    });

    const codeTab = screen.getByRole("tab", { name: /^Code/ });
    const row = screen.getByTestId("workspace-tab-row");
    expect(row.contains(codeTab)).toBe(true);
    expect(row.contains(marker)).toBe(true);
  });

  it("hides the marker when the workspace has no LOC changes", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    const branchName = "feat/loc-empty";
    await createWorkspace(repoPath, branchName);

    render(<Dashboard />);
    await clickGpuiRow(branchName);
    await screen.findByTestId("show-workspace-header");
    await screen.findByRole("tab", { name: /^Code/ });

    await waitFor(() => {
      expect(screen.queryByTestId("loc-diff-marker")).toBeNull();
    });
  });
});

describe("Review tab change count", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("shows total unique committed + uncommitted files before and after selecting Review", async () => {
    const fixture =
      await setupWorkspaceWithCommittedAndUncommitted("feat/review-count");

    render(<Dashboard />);
    await clickGpuiRow(fixture.branchName);
    await screen.findByTestId("show-workspace-header");

    await waitFor(() => {
      expect(getReviewBadgeCount()).toBe(2);
    });

    const before = getReviewBadgeCount();
    await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
    await screen.findByRole("tab", { name: /^Changes/, selected: true });
    await screen.findByTitle("uncommitted-only.txt");
    await screen.findByTitle("committed-only.txt");

    await waitFor(() => {
      expect(getReviewBadgeCount()).toBe(2);
    });
    expect(getReviewBadgeCount()).toBe(before);
  });

  it("updates the Changes tab badge when switching to a workspace with no changes", async () => {
    const fixture =
      await setupWorkspaceWithCommittedAndUncommitted("feat/review-count");
    await createWorkspace(fixture.repoPath, "feat/clean");

    render(<Dashboard />);
    await clickGpuiRow(fixture.branchName);
    await screen.findByTestId("show-workspace-header");
    await waitFor(() => {
      expect(getReviewBadgeCount()).toBe(2);
    });

    await clickGpuiRow("feat/clean");
    const header = await screen.findByTestId("show-workspace-header");
    await waitFor(() => {
      expect(header).toHaveTextContent("feat/clean");
    });
    await waitFor(() => {
      expect(getReviewBadgeCount()).toBeNull();
    });
  });

  it("updates the Changes tab badge when switching back to the home repo", async () => {
    const fixture =
      await setupWorkspaceWithCommittedAndUncommitted("feat/review-count");

    render(<Dashboard />);
    await clickGpuiRow(fixture.branchName);
    await screen.findByTestId("show-workspace-header");
    await waitFor(() => {
      expect(getReviewBadgeCount()).toBe(2);
    });

    await clickGpuiHome();
    await waitFor(() => {
      expect(getReviewBadgeCount()).not.toBe(2);
    });
  });
});

describe("ShowWorkspace - committed diff dedupe integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  function getCommittedShowButton() {
    return screen.getByRole("button", { name: /^Show$/ });
  }

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("renders committed and uncommitted hunks from the live backend and toggles committed visibility", async () => {
    const fixture = await setupOverlappingWorkspace("feat/committed-dedupe");

    render(<Dashboard />);
    await clickGpuiRow(fixture.branchName);

    const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
    await user.click(reviewTab);
    await screen.findByRole("tab", { name: /^Changes/, selected: true });

    await waitFor(() => {
      expect(screen.getAllByText("shared.txt").length).toBeGreaterThanOrEqual(
        2,
      );
      expect(screen.getByTitle("committed-only.txt")).toBeInTheDocument();
      expect(screen.getByTitle("uncommitted-only.txt")).toBeInTheDocument();
    });

    expect(await screen.findByText("committed only")).toBeInTheDocument();
    expect(
      (await screen.findAllByText("shared v2")).length,
    ).toBeGreaterThanOrEqual(1);
    expect(
      (await screen.findAllByText("shared v3")).length,
    ).toBeGreaterThanOrEqual(1);
    expect(await screen.findByText("working only")).toBeInTheDocument();

    await user.click(getCommittedShowButton());
    await waitFor(() => {
      expect(screen.queryByTitle("committed-only.txt")).not.toBeInTheDocument();
      expect(screen.queryByText("committed only")).not.toBeInTheDocument();
      expect(screen.getByTitle("uncommitted-only.txt")).toBeInTheDocument();
      expect(screen.getByTitle("shared.txt")).toBeInTheDocument();
    });

    expect(screen.getAllByText("shared v2").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("shared v3").length).toBeGreaterThanOrEqual(1);
    expect(await screen.findByText("working only")).toBeInTheDocument();

    await user.click(getCommittedShowButton());
    await waitFor(() => {
      expect(screen.getAllByText("shared.txt").length).toBeGreaterThanOrEqual(
        2,
      );
      expect(screen.getByTitle("committed-only.txt")).toBeInTheDocument();
    });

    expect(await screen.findByText("committed only")).toBeInTheDocument();
    expect(screen.getAllByText("shared v2").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("shared v3").length).toBeGreaterThanOrEqual(1);
    expect(await screen.findByText("working only")).toBeInTheDocument();
  });
});

describe("ShowWorkspace - Search integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("opens search overlay with Ctrl+F and shows matches", async () => {
    const branchName = "feat/search-test";
    await setupWorkspaceWithChange(branchName);

    await openReviewTab(user, branchName);
    await waitForChangedFile("search-test.txt");

    await user.keyboard("{Control>}f{/Control}");

    const searchInput = await screen.findByPlaceholderText("Find");
    expect(searchInput).toBeInTheDocument();

    await user.type(searchInput, "hello");

    await waitFor(() => {
      expect(screen.queryByText(/of \d+/)).toBeTruthy();
    });
  });

  it("closes search overlay with Escape", async () => {
    const branchName = "feat/search-close-test";
    await setupWorkspaceWithChange(branchName);

    await openReviewTab(user, branchName);
    await waitForChangedFile("search-test.txt");

    await user.keyboard("{Control>}f{/Control}");
    const searchInput = await screen.findByPlaceholderText("Find");
    expect(searchInput).toBeInTheDocument();

    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByPlaceholderText("Find")).toBeNull();
    });
  });
});
