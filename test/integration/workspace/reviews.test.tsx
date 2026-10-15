// @include-serial
import { clickGpuiRow } from "../../gpui-sidebar";
import * as React from "react";
import fs from "node:fs";
import path from "node:path";
import * as api from "../../../src/lib/api";
import { Dashboard } from "../../../src/components/Dashboard";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
  writeWorkspaceFile,
} from "../../utils";
import {
  type Workspace,
  createWorkspace,
  getWorkspaces,
  listCommits,
} from "../../../src/lib/api";
import { render, screen, waitFor } from "../../test-utils";
import userEvent from "@testing-library/user-event";
import {
  addSingleReviewComment as addReviewComment,
  clickChangedFile,
  openReviewTab,
  waitForChangedFile,
} from "../review/comments-helpers";

const REVIEW_FILE = "reviews-flow.txt";

async function setupWorkspaceWithChange(branchName: string): Promise<{
  repoPath: string;
  workspace: Workspace;
  fileName: string;
}> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (item) => item.id === workspaceId,
  );
  if (!workspace) {
    throw new Error(`Workspace not found for id ${workspaceId}`);
  }

  writeWorkspaceFile(
    resolveWorkspacePath(repoPath, workspace.workspace_path),
    REVIEW_FILE,
    "first review line\nsecond review line\n",
  );

  return { fileName: REVIEW_FILE, repoPath, workspace };
}

async function addSingleReviewComment(
  user: ReturnType<typeof userEvent.setup>,
  fileName: string,
  comment: string,
) {
  await clickChangedFile(fileName);
  await addReviewComment(user, comment);
}

async function openReviewWithChange(branchName: string) {
  const fixture = await setupWorkspaceWithChange(branchName);
  return fixture;
}

async function expectCommitCreated(
  repoPath: string,
  workspaceId: number,
  commitMessage: string,
) {
  await waitFor(async () => {
    const log = await listCommits(repoPath, workspaceId, false, undefined, 30);
    expect(
      log.commits.some((commit) => commit.description === commitMessage),
    ).toBe(true);
  });
}

describe("ShowWorkspace - Reviews integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("is able to create a commit from Review tab", async () => {
    const branchName = "feat/reviews-commit";
    const commitMessage = "Add integration reviews test commit";
    const { repoPath, workspace } = await openReviewWithChange(branchName);
    await openReviewTab(user, branchName);
    await waitForChangedFile(REVIEW_FILE);

    await user.type(
      await screen.findByPlaceholderText("Message"),
      commitMessage,
    );
    await user.click(await screen.findByRole("button", { name: /^commit$/i }));
    await expectCommitCreated(repoPath, workspace.id, commitMessage);
  });

  it("is able to review a change and start an agent", async () => {
    const branchName = "feat/reviews-agent";
    const { repoPath, workspace } = await openReviewWithChange(branchName);
    await openReviewTab(user, branchName);
    const createSessionSpy = vi.spyOn(api, "createSession");

    await addSingleReviewComment(
      user,
      REVIEW_FILE,
      "Please tighten this change",
    );
    await user.click(
      await screen.findByRole("button", { name: /finish review/i }),
    );
    await user.click(await screen.findByRole("button", { name: /^plan$/i }));

    await waitFor(() => {
      expect(createSessionSpy).toHaveBeenCalledWith(
        repoPath,
        workspace.id,
        "Code Review",
      );
    });
  });

  it("is able to mark a file as viewed and back to unviewed", async () => {
    const branchName = "feat/reviews-viewed";
    await openReviewWithChange(branchName);
    await openReviewTab(user, branchName);

    const viewedCheckbox = await screen.findByRole("checkbox", {
      name: "Viewed",
    });
    expect(viewedCheckbox).toHaveAttribute("aria-checked", "false");

    await user.click(viewedCheckbox);
    await waitFor(() => {
      expect(viewedCheckbox).toHaveAttribute("aria-checked", "true");
      expect(
        screen.getByRole("button", { name: "Expand file diff" }),
      ).toBeInTheDocument();
    });

    await user.click(viewedCheckbox);
    await waitFor(() => {
      expect(viewedCheckbox).toHaveAttribute("aria-checked", "false");
      expect(
        screen.getByRole("button", { name: "Collapse file diff" }),
      ).toBeInTheDocument();
    });
  }, 15_000);

  it("is able to expand and collapse a file diff", async () => {
    const branchName = "feat/reviews-expand-collapse";
    await openReviewWithChange(branchName);
    await openReviewTab(user, branchName);
    await waitForChangedFile(REVIEW_FILE);

    await user.click(
      screen.getByRole("button", { name: "Collapse file diff" }),
    );
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Expand file diff" }),
      ).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: "Expand file diff" }));
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Collapse file diff" }),
      ).toBeInTheDocument();
    });
  });
});

async function setupStagingWorkspaceWithChange(branchName: string): Promise<{
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
    "stage-test.txt",
    "line one\nline two\nline three\n",
  );

  return { repoPath, workspace };
}

describe("ShowWorkspace - File staging integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("stages a file and shows it in the Selected section", async () => {
    const branchName = "feat/staging-test";
    await setupStagingWorkspaceWithChange(branchName);

    render(<Dashboard />);
    await clickGpuiRow(branchName);

    const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
    await user.click(reviewTab);
    await screen.findByRole("tab", { name: /^Changes/, selected: true });

    await waitFor(() =>
      expect(screen.getAllByText("stage-test.txt").length).toBeGreaterThan(0),
    );

    const fileRows = screen.getAllByText("stage-test.txt");
    await user.click(fileRows[0]);

    const stageButton = await screen.findByTitle("Stage file(s) for commit");
    await user.click(stageButton);

    await screen.findByText("Selected");
  });
});

async function createDirtyWorkspace(branchName: string, fileName: string) {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await api.createWorkspace(repoPath, branchName);
  const workspace = (await api.getWorkspaces(repoPath)).find(
    (candidate) => candidate.id === workspaceId,
  );
  expect(workspace).toBeTruthy();

  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace!.workspace_path,
  );
  writeWorkspaceFile(workspacePath, fileName, "dirty working copy content\n");

  return { repoPath, workspace: workspace!, workspacePath };
}

describe("Review tab - discard changes", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("discards all uncommitted changes from the Changes tab", async () => {
    const fileName = "discard-all.txt";
    const { workspacePath } = await createDirtyWorkspace(
      "feat/discard-all",
      fileName,
    );
    await openReviewTab(user, "feat/discard-all");
    await waitForChangedFile(fileName);

    await user.click(
      await screen.findByRole("button", { name: /discard all changes/i }),
    );
    await screen.findByText("Discard all changes?");
    const confirmButtons = screen.getAllByRole("button", {
      name: /discard all changes/i,
    });
    await user.click(confirmButtons[confirmButtons.length - 1]);

    await waitFor(() => {
      expect(screen.queryByTestId(`file-row-${fileName}`)).toBeNull();
    });

    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(false);
  });

  it("discards a selected file from the Review sidebar", async () => {
    const fileName = "discard-one.txt";
    const { workspacePath } = await createDirtyWorkspace(
      "feat/discard-one",
      fileName,
    );
    await openReviewTab(user, "feat/discard-one");
    await waitForChangedFile(fileName);

    const [sidebarFile] = screen.getAllByText(fileName);
    await user.click(sidebarFile);

    await user.click(await screen.findByTitle("Discard selected files"));

    await waitFor(() => {
      expect(screen.queryByTestId(`file-row-${fileName}`)).toBeNull();
    });

    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(false);
  });

  it("keeps changes when canceling the discard-all confirmation", async () => {
    const fileName = "discard-cancel.txt";
    const { workspacePath } = await createDirtyWorkspace(
      "feat/discard-cancel",
      fileName,
    );
    await openReviewTab(user, "feat/discard-cancel");
    await waitForChangedFile(fileName);

    await user.click(
      await screen.findByRole("button", { name: /discard all changes/i }),
    );
    await screen.findByText("Discard all changes?");
    await screen.clickByText("Cancel");

    expect(screen.getAllByText(fileName).length).toBeGreaterThan(0);
    expect(screen.queryByText("Discard all changes?")).toBeNull();
    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(true);
  });

  it("undoes discard-all and restores the previous working-copy changes", async () => {
    const fileName = "discard-undo.txt";
    const { workspacePath } = await createDirtyWorkspace(
      "feat/discard-undo",
      fileName,
    );
    await openReviewTab(user, "feat/discard-undo");
    await waitForChangedFile(fileName);

    await user.click(
      await screen.findByRole("button", { name: /discard all changes/i }),
    );
    await screen.findByText("Discard all changes?");
    const confirmButtons = screen.getAllByRole("button", {
      name: /discard all changes/i,
    });
    await user.click(confirmButtons[confirmButtons.length - 1]);

    await waitFor(() => {
      expect(screen.queryByTestId(`file-row-${fileName}`)).toBeNull();
    });
    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(false);

    await user.click(await screen.findByRole("button", { name: "Undo" }));

    await waitFor(() => {
      expect(screen.getByTestId(`file-row-${fileName}`)).toBeTruthy();
    });
    expect(fs.existsSync(path.join(workspacePath, fileName))).toBe(true);
  });
});
