// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import * as api from "../../src/lib/api";
import {
  commitWorkspaceFile,
  createTestRepo,
  openRepo,
} from "../utils";
import { createWorkspace, getWorkspaces } from "../../src/lib/api";
import { fireEvent, render, screen, waitFor, within } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import { clickGpuiRow } from "../gpui-sidebar";

vi.mock("../../src/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../src/lib/api")>();
  return {
    ...actual,
    jjGetCommitsAhead: vi.fn(),
  };
});

const mockCommitsAhead = (count: number) => {
  const commits = Array.from({ length: count }, (_, i) => ({
    commit_id: `commit${i}`,
    short_id: `commit${i}`,
    change_id: `change${i}`,
    description: `Merge preview commit ${i}`,
    author_name: "Test Author",
    timestamp: "2024-01-01 10:00:00",
    parent_ids: [],
    is_working_copy: false,
    bookmarks: [],
    is_immutable: false,
    insertions: 1,
    deletions: 0,
  }));
  vi.mocked(api.jjGetCommitsAhead).mockResolvedValue({
    commits,
    total_count: count,
  });
};

describe("MergePreviewPage integration", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    vi.mocked(api.jjGetCommitsAhead).mockResolvedValue({
      commits: [],
      total_count: 0,
    });
    user = userEvent.setup();
  });

  async function openMergePreview(branchName: string) {
    render(<Dashboard />);
    await clickGpuiRow(branchName);
    let mergeButton: HTMLButtonElement;
    await waitFor(() => {
      const header = screen.getByTestId("show-workspace-header");
      mergeButton = within(header).getByRole("button", {
        name: /merge/i,
      }) as HTMLButtonElement;
      expect(mergeButton).not.toBeDisabled();
    });
    fireEvent.click(mergeButton!);
    await screen.findByText("Merge Preview");
  }

  it("shows commits ahead with count", async () => {
    vi.mocked(api.jjGetCommitsAhead).mockResolvedValue({
      commits: [
        {
          commit_id: "abc123",
          short_id: "abc123",
          change_id: "xyz789",
          description: "Add merge preview feature",
          author_name: "Test Author",
          timestamp: "2024-01-01 10:00:00",
          parent_ids: [],
          is_working_copy: false,
          bookmarks: [],
          is_immutable: false,
          insertions: 1,
          deletions: 0,
        },
      ],
      total_count: 1,
    });

    await createWorkspace(repoPath, "feat/mp-commits");
    await openMergePreview("feat/mp-commits");

    await screen.findByText("Add merge preview feature");
    await screen.findByText(/Commits to be merged \(1\)/i);
  });

  it("shows changed files section after loading", async () => {
    mockCommitsAhead(1);
    const workspaceId = await createWorkspace(repoPath, "feat/mp-diff");
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    );
    if (!workspace) throw new Error("Workspace not found");
    await commitWorkspaceFile(
      repoPath,
      { id: workspace.id, path: workspace.workspace_path },
      "diff-test.ts",
      "export const x = 1;",
      "Add diff test file",
    );

    await openMergePreview("feat/mp-diff");

    await screen.findByText(/Changed Files \(1\)/i);
    expect(await screen.findAllByText("diff-test.ts")).not.toHaveLength(0);
  });

  it("allows editing the commit message", async () => {
    mockCommitsAhead(1);
    await createWorkspace(repoPath, "feat/mp-msg");
    await openMergePreview("feat/mp-msg");

    const textarea = await screen.findByPlaceholderText(
      /enter merge commit message/i,
    );
    await user.clear(textarea);
    await user.type(textarea, "Custom merge message");

    expect(textarea).toHaveValue("Custom merge message");
  });

  it("disables confirm merge when no commits ahead", async () => {
    await createWorkspace(repoPath, "feat/mp-no-commits");
    await openMergePreview("feat/mp-no-commits");

    const mergeButton = await screen.findByRole("button", {
      name: /^confirm merge$/i,
    });
    expect(mergeButton).toBeDisabled();
  });

  it("disables confirm merge when commit message is cleared", async () => {
    mockCommitsAhead(1);
    await createWorkspace(repoPath, "feat/mp-empty-msg");
    await openMergePreview("feat/mp-empty-msg");

    const textarea = await screen.findByPlaceholderText(
      /enter merge commit message/i,
    );
    await user.clear(textarea);

    const mergeButton = screen.getByRole("button", {
      name: /^confirm merge$/i,
    });
    expect(mergeButton).toBeDisabled();
  });

  it("returns to home view after successful merge", async () => {
    mockCommitsAhead(1);
    const workspaceId = await createWorkspace(repoPath, "feat/mp-success");
    const workspace = (await getWorkspaces(repoPath)).find(
      (w) => w.id === workspaceId,
    );
    if (!workspace) throw new Error("Workspace not found");
    await commitWorkspaceFile(
      repoPath,
      { id: workspace.id, path: workspace.workspace_path },
      "success-file.txt",
      "merged content",
      "Commit to merge",
    );

    await openMergePreview("feat/mp-success");

    const mergeButton = await screen.findByRole("button", {
      name: /^confirm merge$/i,
    });
    await waitFor(() => expect(mergeButton).not.toBeDisabled());
    await user.click(mergeButton);

    await waitFor(() => {
      expect(screen.queryByText("Merge Preview")).not.toBeInTheDocument();
    });
  });

  it("returns to workspace view when go back is clicked", async () => {
    mockCommitsAhead(1);
    await createWorkspace(repoPath, "feat/mp-cancel");
    await openMergePreview("feat/mp-cancel");

    await user.click(await screen.findByRole("button", { name: /go back/i }));

    await waitFor(() => {
      expect(screen.queryByText("Merge Preview")).not.toBeInTheDocument();
    });
  });
});
