import * as React from "react";
import { expect } from "vitest";
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
} from "../../../src/lib/api";
import { fireEvent, render, screen, waitFor } from "../../test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";
import { clickGpuiRow } from "../../gpui-sidebar";

export async function setupWorkspaceWithDiff(
  branchName: string,
  fileContent = "context line 1\nadded line 2\nadded line 3\nadded line 4\ncontext line 5\ncontext line 6\n",
): Promise<{ repoPath: string; workspace: Workspace }> {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const workspaceId = await createWorkspace(repoPath, branchName);
  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  );
  if (!workspace) throw new Error("Workspace not found");

  const wsPath = resolveWorkspacePath(repoPath, workspace.workspace_path);
  writeWorkspaceFile(wsPath, "test.txt", fileContent);

  return { repoPath, workspace };
}

export async function openReviewTab(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  render(<Dashboard />);
  await clickGpuiRow(branchName);
  const reviewTab = await screen.findByRole("tab", { name: /^Changes/ });
  await user.click(reviewTab);
  await screen.findByRole("tab", { name: /^Changes/, selected: true });
  await waitFor(() => {
    expect(screen.queryByText(/Loading diffs\.\.\./)).not.toBeInTheDocument();
  });
}

async function findChangedFileElement(fileName: string): Promise<HTMLElement> {
  return waitFor(() => {
    const row = document.querySelector(`[data-testid="file-row-${fileName}"]`);
    if (row) return row as HTMLElement;
    const [sidebarMatch] = screen.queryAllByText(fileName);
    if (sidebarMatch) return sidebarMatch;
    throw new Error(`waiting for changed file ${fileName}`);
  });
}

export async function waitForChangedFile(fileName: string) {
  return findChangedFileElement(fileName);
}

export async function clickChangedFile(fileName: string) {
  try {
    fireEvent.click(await findChangedFileElement(fileName));
  } catch (firstError) {
    const changesTab = screen.queryByRole("tab", {
      name: /^Changes/,
      selected: true,
    });
    if (!changesTab) {
      throw firstError;
    }
    fireEvent.click(changesTab);
    await waitFor(() => {
      expect(screen.queryByText(/Loading diffs\.\.\./)).not.toBeInTheDocument();
    });
    fireEvent.click(await findChangedFileElement(fileName));
  }
}

export async function waitForFileAndLines() {
  await clickChangedFile("test.txt");
  if (document.querySelectorAll("[data-diff-line]").length === 0) {
    const [fileLabel] = screen.getAllByText(/test\.txt/);
    fireEvent.click(fileLabel);
  }
  await screen.findByText(/added line 2/);
}

export function getDiffLines() {
  return document.querySelectorAll("[data-diff-line]");
}

export function getClickableArea(line: Element) {
  const el = line.querySelector("[data-testid='line-gutter']");
  expect(el).not.toBeNull();
  return el!;
}

export async function addSingleReviewComment(
  user: ReturnType<typeof userEvent.setup>,
  comment: string,
) {
  await waitFor(() => {
    if (document.querySelectorAll("[data-diff-line]").length === 0) {
      throw new Error("waiting for diff lines");
    }
  });
  const gutterButton = await waitFor(() => {
    const button = document.querySelector(
      "[data-comment-button]",
    ) as HTMLElement | null;
    if (!button) {
      throw new Error("waiting for comment gutter button");
    }
    return button;
  });
  fireEvent.click(gutterButton);
  const input = await screen.findByPlaceholderText(/add a comment/i);
  await user.type(input, comment);
  const submit = await waitFor(() => {
    const button = screen
      .getAllByRole("button", { name: /add comment/i, hidden: true })
      .find((el) => el.textContent?.trim() === "Add Comment");
    if (!button) {
      throw new Error("waiting for Add Comment submit");
    }
    expect(button).not.toBeDisabled();
    return button;
  });
  await user.click(submit);
  await screen.findByText(comment);
}

export async function startEditingComment(comment: string) {
  const commentText = await screen.findByText(comment);
  const commentCard = commentText.closest(
    "[data-testid='inline-comment-card']",
  );
  expect(commentCard).toBeTruthy();
  fireEvent.click(commentCard!);
  return screen.findByDisplayValue(comment);
}

export async function setupEditableComment(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
  comment: string,
) {
  await setupWorkspaceWithDiff(branchName);
  await openReviewTab(user, branchName);
  await waitForFileAndLines();
  await addSingleReviewComment(user, comment);
}

export async function setupReviewMode(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  await setupWorkspaceWithDiff(branchName);
  await openReviewTab(user, branchName);
  await waitForFileAndLines();
  await addSingleReviewComment(user, "Test comment");
}

export function findReviewCopyButton() {
  return screen.getByRole("button", { name: /^copy review$/i });
}
