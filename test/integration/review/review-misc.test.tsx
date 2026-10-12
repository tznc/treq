// @include-serial
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import path from "node:path";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  findSidebarBranchElement,
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
import { savePendingReview } from "../../../src/lib/api-extra";
import type { LineComment } from "../../../src/lib/api-types";
import { render, screen, waitFor } from "../../test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { findReviewCopyButton, setupReviewMode } from "./comments-helpers";

const REVIEW_FILE = "commit-reload.txt";

async function setupWorkspaceWithChange(branchName: string): Promise<{
  repoPath: string;
  workspace: Workspace;
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

  return { repoPath, workspace };
}

async function setupWorkspaceWithReviewFileChange(branchName: string): Promise<{
  repoPath: string;
  workspace: Workspace;
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

  return { repoPath, workspace };
}

async function setupWorkspaceWithFiles(
  branchName: string,
  files: { name: string; content: string }[],
): Promise<{
  repoPath: string;
  workspace: Workspace;
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

  const wsPath = resolveWorkspacePath(repoPath, workspace.workspace_path);
  for (const file of files) {
    writeWorkspaceFile(wsPath, file.name, file.content);
  }

  return { repoPath, workspace };
}

async function openReviewTab(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  render(<Dashboard />);

  await user.click(await findSidebarBranchElement(branchName));

  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });
}

async function addSingleReviewComment(
  user: ReturnType<typeof userEvent.setup>,
  comment: string,
) {
  await user.click(
    (await screen.findAllByRole("button", { name: /add comment/i }))[0],
  );
  await user.type(
    await screen.findByPlaceholderText(/add a comment/i),
    comment,
  );

  const submit = screen
    .getAllByRole("button", { name: /add comment/i })
    .find((button) => button.textContent === "Add Comment");
  expect(submit).toBeTruthy();
  await user.click(submit!);
  await screen.findByText(comment);
}

describe("Review - deleted file collapsible", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("marks deleted files and does not render their former contents", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    const workspaceId = await createWorkspace(repoPath, "feat/deleted-file");
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    );
    if (!workspace) throw new Error("Workspace not found");
    const workspacePath = resolveWorkspacePath(
      repoPath,
      workspace.workspace_path,
    );

    writeWorkspaceFile(
      workspacePath,
      "doomed.txt",
      "secret line one\nsecret line two\n",
    );
    await createCommit(repoPath, workspaceId, "add doomed file");
    fs.unlinkSync(path.join(workspacePath, "doomed.txt"));

    render(<Dashboard />);
    await user.click(await findSidebarBranchElement("feat/deleted-file"));
    await user.click(await screen.findByRole("tab", { name: /^Changes/ }));
    await screen.findByRole("tab", { name: /^Changes/, selected: true });

    await waitFor(() => {
      expect(screen.getAllByText("doomed.txt").length).toBeGreaterThan(0);
    });
    expect(await screen.findByText("Deleted")).toBeInTheDocument();
    expect(
      await screen.findByTestId("deleted-file-placeholder"),
    ).toHaveTextContent("File deleted");
    expect(screen.queryByText("secret line one")).not.toBeInTheDocument();
    expect(screen.queryByText("secret line two")).not.toBeInTheDocument();
  });
});

describe("Outdated comments - persisted comment with non-existent hunk", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("should enter review mode when persisted comment references a non-existent hunk", async () => {
    const branchName = "feat/outdated-comments";
    const { repoPath, workspace } = await setupWorkspaceWithChange(branchName);
    const wsPath = resolveWorkspacePath(repoPath, workspace.workspace_path);

    writeWorkspaceFile(wsPath, "test.txt", "first line\nsecond line\n");

    const outdatedComment: LineComment = {
      id: "outdated-comment-1",
      file_path: "test.txt",
      hunk_id: "nonexistent-hunk-id",
      start_line: 102,
      end_line: 102,
      line_content: ["+old line that was removed"],
      text: "This line needs work",
      created_at: new Date().toISOString(),
    };
    await savePendingReview(repoPath, workspace.id, [outdatedComment]);

    await openReviewTab(user, branchName);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: /finish review/i }),
      ).toBeTruthy();
    });

    await screen.findAllByText(/test\.txt/);

    await screen.findAllByRole("button", { name: /^discard$/i });
  });
});

describe("Review tab - file loading and display", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("should load and display multiple files when review tab is opened", async () => {
    const branchName = "feat/refresh-multi-files";
    await setupWorkspaceWithFiles(branchName, [
      { name: "file1.txt", content: "content one\n" },
      { name: "file2.txt", content: "content two\n" },
      { name: "file3.txt", content: "content three\n" },
    ]);
    await openReviewTab(user, branchName);

    await waitFor(() => {
      expect(screen.getAllByText(/file1\.txt/).length).toBeGreaterThan(0);
      expect(screen.getAllByText(/file2\.txt/).length).toBeGreaterThan(0);
      expect(screen.getAllByText(/file3\.txt/).length).toBeGreaterThan(0);
    });
  });

  it("should display file content when expanded", async () => {
    const branchName = "feat/refresh-expand";
    await setupWorkspaceWithFiles(branchName, [
      { name: "test.txt", content: "hello world\n" },
    ]);
    await openReviewTab(user, branchName);

    await waitFor(() => {
      expect(screen.getAllByText(/test\.txt/).length).toBeGreaterThan(0);
    });

    const collapseBtn = await screen.findByRole("button", {
      name: "Collapse file diff",
    });
    await user.click(collapseBtn);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Expand file diff" }),
      ).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: "Expand file diff" }));

    await waitFor(() => {
      expect(screen.getAllByText(/hello world/).length).toBeGreaterThan(0);
    });
  });
});

describe("Review tab - auto-reload after commit", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("reloads the review diff after commit instead of showing the stale-files banner", async () => {
    const branchName = "feat/commit-auto-reload";
    const commitMessage = "Commit while reviewing";
    const { repoPath, workspace } =
      await setupWorkspaceWithReviewFileChange(branchName);
    await openReviewTab(user, branchName);

    await waitFor(() => {
      expect(screen.getAllByText(REVIEW_FILE).length).toBeGreaterThan(0);
    });
    await addSingleReviewComment(user, "Please tighten this change");

    await user.type(
      await screen.findByPlaceholderText("Message"),
      commitMessage,
    );
    await user.click(screen.getByRole("button", { name: /^commit$/i }));

    await waitFor(
      async () => {
        const log = await listCommits(
          repoPath,
          workspace.id,
          false,
          undefined,
          30,
        );
        expect(
          log.commits.some((commit) => commit.description === commitMessage),
        ).toBe(true);
      },
      { timeout: 15_000 },
    );

    await waitFor(() => {
      expect(
        screen.queryByText(/changed since you started reviewing/i),
      ).toBeNull();
      expect(screen.queryByRole("button", { name: /^reload$/i })).toBeNull();
      expect(screen.getByTestId("committed-file-label")).toBeInTheDocument();
      expect(
        screen.getByTestId("file-row-commit-reload.txt"),
      ).toBeInTheDocument();
    });
  }, 30_000);
});

describe("Copy button in Finish Review popover", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("should NOT show copy button in header bar", async () => {
    await setupReviewMode(user, "feat/no-copy-header");

    await waitFor(() => {
      const discardButtons = screen.getAllByRole("button", {
        name: /discard/i,
      });
      const reviewDiscardButton = discardButtons.find(
        (btn) => btn.textContent === "Discard",
      );
      expect(reviewDiscardButton).toBeInTheDocument();
    });

    const copyButtons = screen.queryAllByRole("button", { name: /copy/i });
    const discardButtons = screen.getAllByRole("button", { name: /discard/i });
    const reviewDiscardButton = discardButtons.find(
      (btn) => btn.textContent === "Discard",
    )!;
    const headerContainer = reviewDiscardButton.parentElement;

    const copyButtonsInHeader = copyButtons.filter((btn) =>
      headerContainer?.contains(btn),
    );
    expect(copyButtonsInHeader.length).toBe(0);
  });

  it("copies the review from the Finish Review popover", async () => {
    const clipboardSpy = vi.spyOn(navigator.clipboard, "writeText");

    await setupReviewMode(user, "feat/copy-review-popover");

    const finishButton = await screen.findByRole("button", {
      name: /finish review/i,
    });
    await user.click(finishButton);

    await waitFor(() => {
      expect(findReviewCopyButton()).toBeInTheDocument();
    });

    const copyButton = findReviewCopyButton();
    const planButton = screen.getByRole("button", { name: /^plan$/i });
    const editButton = screen.getByRole("button", { name: /^edit$/i });
    const allButtons = screen.getAllByRole("button");
    expect(allButtons.indexOf(copyButton)).toBeLessThan(
      allButtons.indexOf(planButton),
    );
    expect(allButtons.indexOf(copyButton)).toBeLessThan(
      allButtons.indexOf(editButton),
    );
    await screen.findByRole("button", { name: /^close$/i });

    await user.click(copyButton);

    await waitFor(() => {
      expect(clipboardSpy).toHaveBeenCalledWith(
        expect.stringContaining("Test comment"),
      );
    });
    await screen.findByRole("button", { name: /^copied review$/i });
    expect(screen.queryByText("Copied to clipboard")).not.toBeInTheDocument();
  });
});
