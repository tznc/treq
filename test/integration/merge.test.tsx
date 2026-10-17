// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import * as api from "../../src/lib/api";
import { commitWorkspaceFile, createTestRepo, openRepo } from "../utils";
import { createWorkspace, getWorkspaces } from "../../src/lib/api";
import { fireEvent, render, screen, waitFor, within } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import { clickGpuiRow } from "../gpui-sidebar";

vi.mock("../../src/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../src/lib/api")>();
  return {
    ...actual,
    jjGetCommitsAhead: vi.fn(),
    getCachedPrInfo: vi.fn(),
    getGitRemoteUrl: vi.fn(),
    ghRepoMergeSettings: vi.fn(),
    mergeWorkspaceOnGithub: vi.fn(),
    syncWorkspaceAfterRemoteMerge: vi.fn(),
    mergeWorkspace: vi.fn(actual.mergeWorkspace),
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
    vi.mocked(api.getCachedPrInfo).mockResolvedValue(null);
    vi.mocked(api.getGitRemoteUrl).mockResolvedValue(null);
    vi.mocked(api.mergeWorkspaceOnGithub).mockReset();
    vi.mocked(api.syncWorkspaceAfterRemoteMerge).mockReset();
    vi.mocked(api.mergeWorkspace).mockClear();
    user = userEvent.setup();
  });

  async function openMergePreview(branchName: string) {
    render(<Dashboard />);
    await clickGpuiRow(branchName);
    let mergeButton: HTMLButtonElement;
    await waitFor(() => {
      const header = screen.getByTestId("show-workspace-header");
      mergeButton = within(header).getByRole("button", {
        name: /^merge\.\.\.$/i,
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

  const escapeRegExp = (text: string) =>
    text.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");

  const targetOf = async (branchName: string) => {
    const ws = (await getWorkspaces(repoPath)).find(
      (w) => w.branch_name === branchName,
    );
    if (!ws?.target_branch) throw new Error(`No target for ${branchName}`);
    return ws.target_branch;
  };

  describe("landing plan", () => {
    it("offers only a local merge when the workspace has no open PR", async () => {
      mockCommitsAhead(1);
      await createWorkspace(repoPath, "feat/mp-local-only");
      await openMergePreview("feat/mp-local-only");

      await screen.findByRole("heading", { name: /what lands where/i });
      expect(
        screen.queryByRole("radio", { name: /on github/i }),
      ).not.toBeInTheDocument();
    });

    it("lists child workspaces that will be retargeted", async () => {
      mockCommitsAhead(1);
      await createWorkspace(repoPath, "feat/mp-parent");
      await createWorkspace(repoPath, "feat/mp-child", "feat/mp-parent");
      const main = await targetOf("feat/mp-parent");
      await openMergePreview("feat/mp-parent");

      expect(
        await screen.findByText(
          new RegExp(`feat/mp-child is retargeted onto ${escapeRegExp(main)}`),
        ),
      ).toBeInTheDocument();
    });

    it("rolls a stacked workspace up into the stack root", async () => {
      const parentId = await createWorkspace(repoPath, "feat/mp-roll-a");
      const childId = await createWorkspace(
        repoPath,
        "feat/mp-roll-b",
        "feat/mp-roll-a",
      );
      const workspaces = await getWorkspaces(repoPath);
      const commitIn = (id: number, file: string) => {
        const ws = workspaces.find((w) => w.id === id)!;
        return commitWorkspaceFile(
          repoPath,
          { id: ws.id, path: ws.workspace_path },
          file,
          file,
          `Add ${file}`,
        );
      };
      await commitIn(parentId, "roll-a.txt");
      await commitIn(childId, "roll-b.txt");
      mockCommitsAhead(2);
      const main = await targetOf("feat/mp-roll-a");
      await openMergePreview("feat/mp-roll-b");

      await user.click(
        await screen.findByRole("checkbox", {
          name: `Merge the whole stack into ${main}`,
        }),
      );
      await screen.findByText(
        `feat/mp-roll-a and feat/mp-roll-b land in ${main}.`,
      );
      await screen.findByText(/Changed Files \(2\)/i);

      const mergeButton = screen.getByRole("button", {
        name: /^confirm merge$/i,
      });
      await waitFor(() => expect(mergeButton).not.toBeDisabled());
      await user.click(mergeButton);

      await waitFor(async () => {
        const remaining = await getWorkspaces(repoPath);
        expect(remaining.map((w) => w.branch_name)).not.toContain(
          "feat/mp-roll-a",
        );
      });
    });
  });

  describe("pushing after a local merge", () => {
    it("pushes the target by default when the repo has a GitHub remote", async () => {
      vi.mocked(api.getGitRemoteUrl).mockResolvedValue({
        owner: "o",
        repo: "r",
        full_name: "o/r",
      });
      mockCommitsAhead(1);
      vi.mocked(api.mergeWorkspace).mockResolvedValueOnce({
        plan: {
          target_branch: "main",
          landed_branches: ["feat/mp-push"],
          retargeted: [],
          can_rollup: false,
        },
        notes: [],
      });
      const workspaceId = await createWorkspace(repoPath, "feat/mp-push");
      const main = await targetOf("feat/mp-push");
      await openMergePreview("feat/mp-push");

      const push = await screen.findByRole("checkbox", {
        name: `Push ${main} to origin after merging`,
      });
      expect(push).toBeChecked();
      const mergeButton = screen.getByRole("button", {
        name: /^confirm merge$/i,
      });
      await waitFor(() => expect(mergeButton).not.toBeDisabled());
      await user.click(mergeButton);

      await waitFor(() =>
        expect(api.mergeWorkspace).toHaveBeenCalledWith(
          repoPath,
          workspaceId,
          expect.any(String),
          "merge",
          { rollup: false, push: true },
        ),
      );
    });

    it("hides the push option without a remote", async () => {
      mockCommitsAhead(1);
      await createWorkspace(repoPath, "feat/mp-no-remote");
      await openMergePreview("feat/mp-no-remote");

      await screen.findByRole("heading", { name: /what lands where/i });
      expect(
        screen.queryByRole("checkbox", { name: /to origin after merging/i }),
      ).not.toBeInTheDocument();
    });
  });

  describe("merging on GitHub", () => {
    const openPr = async (overrides: Partial<api.PrInfo> = {}) => {
      await createWorkspace(repoPath, "feat/mp-gh");
      const base = await targetOf("feat/mp-gh");
      vi.mocked(api.getGitRemoteUrl).mockResolvedValue({
        owner: "o",
        repo: "r",
        full_name: "o/r",
      });
      vi.mocked(api.getCachedPrInfo).mockResolvedValue({
        number: 7,
        title: "My PR",
        state: "OPEN",
        url: "https://github.com/o/r/pull/7",
        head_ref_name: "feat/mp-gh",
        base_ref_name: base,
        merge_state_status: "CLEAN",
        is_draft: false,
        ...overrides,
      });
      vi.mocked(api.ghRepoMergeSettings).mockResolvedValue({
        default_method: "squash",
        allowed_methods: ["merge", "squash"],
      });
    };

    it("defaults to the repository's merge method and merges the PR", async () => {
      await openPr();
      mockCommitsAhead(1);
      vi.mocked(api.mergeWorkspaceOnGithub).mockResolvedValue({
        plan: {
          target_branch: "main",
          landed_branches: ["feat/mp-gh"],
          retargeted: [],
          can_rollup: false,
        },
        notes: ["Moved PR #8 (feat/mp-gh-child) onto main"],
      });
      const workspaceId = (await getWorkspaces(repoPath)).find(
        (w) => w.branch_name === "feat/mp-gh",
      )!.id;
      await openMergePreview("feat/mp-gh");

      await user.click(
        await screen.findByRole("radio", { name: /on github \(#7\)/i }),
      );
      const mergeButton = await screen.findByRole("button", {
        name: /^squash and merge on github$/i,
      });
      await waitFor(() => expect(mergeButton).not.toBeDisabled());
      await user.click(mergeButton);

      await waitFor(() =>
        expect(api.mergeWorkspaceOnGithub).toHaveBeenCalledWith(
          expect.objectContaining({
            repoPath,
            workspaceId,
            repoFullName: "o/r",
            prNumber: 7,
            method: "squash",
          }),
        ),
      );
      await waitFor(() => {
        expect(screen.queryByText("Merge Preview")).not.toBeInTheDocument();
      });
      await screen.findByText(/moved pr #8 \(feat\/mp-gh-child\) onto main/i);
    });

    it("syncs a PR that was already merged on GitHub", async () => {
      await openPr({ state: "MERGED" });
      mockCommitsAhead(1);
      vi.mocked(api.syncWorkspaceAfterRemoteMerge).mockResolvedValue({
        plan: {
          target_branch: "main",
          landed_branches: ["feat/mp-gh"],
          retargeted: [],
          can_rollup: false,
        },
        notes: [],
      });
      const workspaceId = (await getWorkspaces(repoPath)).find(
        (w) => w.branch_name === "feat/mp-gh",
      )!.id;
      await openMergePreview("feat/mp-gh");

      await screen.findByText(/pr #7 was already merged on github/i);
      await user.click(
        await screen.findByRole("button", { name: /^sync merged pr$/i }),
      );

      await waitFor(() =>
        expect(api.syncWorkspaceAfterRemoteMerge).toHaveBeenCalledWith(
          repoPath,
          workspaceId,
        ),
      );
      await waitFor(() => {
        expect(screen.queryByText("Merge Preview")).not.toBeInTheDocument();
      });
    });

    it("warns about conflicts GitHub reports", async () => {
      await openPr({ merge_state_status: "DIRTY" });
      mockCommitsAhead(1);
      await openMergePreview("feat/mp-gh");

      await user.click(
        await screen.findByRole("radio", { name: /on github \(#7\)/i }),
      );
      expect(
        await screen.findByText(/merge conflicts on github/i),
      ).toBeInTheDocument();
    });

    it("surfaces the GitHub error when the merge is refused", async () => {
      await openPr();
      mockCommitsAhead(1);
      vi.mocked(api.mergeWorkspaceOnGithub).mockRejectedValue(
        new Error("Required checks are failing: ci"),
      );
      await openMergePreview("feat/mp-gh");

      await user.click(
        await screen.findByRole("radio", { name: /on github \(#7\)/i }),
      );
      const mergeButton = await screen.findByRole("button", {
        name: /^squash and merge on github$/i,
      });
      await waitFor(() => expect(mergeButton).not.toBeDisabled());
      await user.click(mergeButton);

      expect(
        await screen.findByText(/required checks are failing: ci/i),
      ).toBeInTheDocument();
      await screen.findByText("Merge Preview");
    });
  });
});
