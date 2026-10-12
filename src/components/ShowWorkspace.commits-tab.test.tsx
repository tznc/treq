import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import useSWR from "swr";
import { render, screen, waitFor } from "../../test/test-utils";
import type { Workspace, WorkspaceStatus } from "../lib/api";
import * as api from "../lib/api";
import { ShowWorkspace } from "./ShowWorkspace";
import { dispatchRefreshWorkspaceChanges } from "../lib/change-file-drag";

vi.mock("./FileBrowser", () => ({
  FileBrowser: () => <div data-testid="file-browser" />,
}));

vi.mock("./LinearCommitHistory", () => ({
  LinearCommitHistory: () => <div data-testid="linear-commit-history" />,
}));

vi.mock("./changes-diff-viewer/ChangesDiffViewerMain", () => ({
  ChangesDiffViewer: () => <div data-testid="changes-viewer" />,
}));

// Reads commits through the same query key as the real viewer, so a refresh
// that invalidates it shows up as another listCommits call.
vi.mock("./CommitDiffViewer", () => ({
  CommitDiffViewer: ({
    repoPath,
    workspaceId,
  }: {
    repoPath: string;
    workspaceId: number | null;
  }) => {
    useSWR(["commit-diff-viewer-commits", repoPath, workspaceId], () =>
      api.listCommits(repoPath, workspaceId),
    );
    return <div data-testid="commit-diff-viewer" />;
  },
}));

vi.mock("./TargetBranchSelector", () => ({
  TargetBranchSelector: () => <div data-testid="target-branch-selector" />,
}));

vi.mock("../lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    getSetting: vi.fn().mockResolvedValue(null),
    getRepoSetting: vi.fn().mockResolvedValue(null),
    lsWorkspaceWithStatus: vi.fn().mockResolvedValue([]),
    getWorkspaceReadme: vi.fn().mockResolvedValue(null),
    jjGetDefaultBranch: vi.fn().mockResolvedValue("main"),
    listConflictedFiles: vi.fn().mockResolvedValue([]),
    jjGetBranches: vi.fn().mockResolvedValue([]),
    setWorkspaceTargetBranch: vi.fn().mockResolvedValue(undefined),
    jjGetChangedFiles: vi.fn().mockResolvedValue([]),
    getWorkspaceChangedFiles: vi.fn().mockResolvedValue([]),
    getWorkspaceDiff: vi.fn().mockResolvedValue({
      uncommitted_files: [],
      committed_files: [],
      hunks_by_file: [],
      too_large_to_render: false,
      render_block_reason: null,
    }),
    createSession: vi.fn().mockResolvedValue(42),
    checkAndRebaseWorkspaces: vi.fn().mockResolvedValue({
      rebased: false,
      success: true,
      has_conflicts: false,
      conflicted_files: [],
      message: "No rebase needed",
      bookmark_conflicts: [],
    }),
    listCommits: vi.fn().mockResolvedValue({
      commits: [
        {
          commit_id: "c1",
          short_id: "c1",
          change_id: "c1",
          description: "feat",
          author_name: "Test",
          timestamp: new Date().toISOString(),
          parent_ids: [],
          is_working_copy: false,
          bookmarks: [],
          is_immutable: false,
          insertions: 1,
          deletions: 0,
          on_target_only: false,
          has_conflicts: false,
        },
      ],
      target_branch: "main",
      workspace_branch: "feature-one",
    }),
    getWorkspaceStatus: vi.fn(),
  };
});

const workspace: Workspace = {
  id: 1,
  repo_path: "/Users/test/repo",
  workspace_name: "feature-one",
  workspace_path: "/Users/test/repo/.treq/workspaces/feature-one",
  branch_name: "feature-one",
  title: "feature-one",
  created_at: new Date().toISOString(),
  not_on_remote: false,
};

function status(overrides: Partial<WorkspaceStatus> = {}): WorkspaceStatus {
  return {
    current: workspace,
    has_conflicts: false,
    has_changes: false,
    conflicted_files: [],
    remote_sync: { type: "InSync" },
    target: null,
    children: [],
    dag_nodes: [],
    conflicted_workspace_ids: [],
    commits_ahead_of_target: [],
    ...overrides,
  };
}

function commitsTab() {
  return screen.getByRole("tab", { name: /commits/i });
}

function commitsCountPill() {
  return commitsTab().querySelector('[data-testid="commits-tab-count"]');
}

function commitsConflictIcon() {
  return commitsTab().querySelector(
    '[data-testid="commits-tab-conflict-icon"]',
  );
}

describe("ShowWorkspace Commits tab label", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(status());
    vi.mocked(api.getWorkspaceDiff).mockResolvedValue({
      uncommitted_files: [],
      committed_files: [],
      hunks_by_file: [],
      too_large_to_render: false,
      render_block_reason: null,
    });
    vi.mocked(api.getWorkspaceChangedFiles).mockResolvedValue([]);
  });

  it("shows the commit-count pill on a workspace with commits", async () => {
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(
      status({
        commits_ahead_of_target: [
          {
            hash: "abc",
            timestamp: new Date().toISOString(),
            message: "feat",
          },
          {
            hash: "def",
            timestamp: new Date().toISOString(),
            message: "more",
          },
        ],
      }),
    );

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await waitFor(() => {
      const pill = commitsCountPill();
      expect(pill).toBeTruthy();
      expect(pill).toHaveTextContent("2");
    });
    expect(commitsConflictIcon()).toBeNull();
  });

  it("shows the commit-count pill when the workspace has conflicts", async () => {
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(
      status({
        has_conflicts: true,
        conflicted_files: ["conflict.txt"],
        commits_ahead_of_target: [
          {
            hash: "abc",
            timestamp: new Date().toISOString(),
            message: "feat",
          },
        ],
      }),
    );

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await waitFor(() => {
      const pill = commitsCountPill();
      expect(pill).toBeTruthy();
      expect(pill).toHaveTextContent("1");
    });
  });

  it("hides the count pill on the home repo", async () => {
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(
      status({
        commits_ahead_of_target: [],
      }),
    );
    vi.mocked(api.listCommits).mockResolvedValue({
      commits: [],
      target_branch: "main",
      workspace_branch: "main",
    });

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={null}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await screen.findByRole("tab", { name: /commits/i });
    expect(commitsCountPill()).toBeNull();
    expect(commitsConflictIcon()).toBeNull();
  });

  it("shows a conflict warning icon on the home Commits tab when home has conflicts", async () => {
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(
      status({
        has_conflicts: true,
        conflicted_files: ["home.txt"],
        commits_ahead_of_target: [],
      }),
    );
    vi.mocked(api.listCommits).mockResolvedValue({
      commits: [],
      target_branch: "main",
      workspace_branch: "main",
    });

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={null}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await waitFor(() => {
      expect(commitsConflictIcon()).toBeTruthy();
    });
    expect(commitsCountPill()).toBeNull();
  });

  it("refetches the commits list when a filesystem refresh fires on the Commits tab", async () => {
    const user = userEvent.setup();
    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await waitFor(() => {
      expect(api.listCommits).toHaveBeenCalled();
    });
    const callsBefore = vi.mocked(api.listCommits).mock.calls.length;

    await user.click(commitsTab());
    await screen.findByTestId("commit-diff-viewer");
    dispatchRefreshWorkspaceChanges({ workspaceId: workspace.id });

    await waitFor(() => {
      expect(vi.mocked(api.listCommits).mock.calls.length).toBeGreaterThan(
        callsBefore,
      );
    });
  });

  it("shows the Diff/Browser switcher only on the Changes tab", async () => {
    const user = userEvent.setup();
    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await screen.findByRole("tab", { name: /changes/i });
    expect(
      screen.queryByRole("button", { name: "Switch review view" }),
    ).toBeNull();

    await user.click(screen.getByRole("tab", { name: /changes/i }));
    expect(
      screen.getByRole("button", { name: "Switch review view" }),
    ).toBeTruthy();

    await user.click(commitsTab());
    expect(
      screen.queryByRole("button", { name: "Switch review view" }),
    ).toBeNull();
  });
});
