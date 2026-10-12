import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../../test/test-utils";
import userEvent from "@testing-library/user-event";
import type { ParsedFileChange } from "../lib/git-utils";
import type { Workspace, WorkspaceStatus } from "../lib/api";
import * as api from "../lib/api";
import { ShowWorkspace } from "./ShowWorkspace";

vi.mock("./FileBrowser", () => ({
  FileBrowser: () => <div data-testid="file-browser" />,
}));

vi.mock("./LinearCommitHistory", () => ({
  LinearCommitHistory: () => <div data-testid="linear-commit-history" />,
}));

let onChangedFilesChange: ((files: ParsedFileChange[]) => void) | undefined;

vi.mock("./changes-diff-viewer/ChangesDiffViewerMain", () => ({
  ChangesDiffViewer: (props: {
    onChangedFilesChange?: (files: ParsedFileChange[]) => void;
  }) => {
    const { onChangedFilesChange: changedFilesCb } = props;
    onChangedFilesChange = changedFilesCb;
    return <div data-testid="changes-viewer" />;
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
          commit_id: "wc",
          short_id: "wc",
          change_id: "wc",
          description: "",
          author_name: "Test",
          timestamp: new Date().toISOString(),
          parent_ids: [],
          is_working_copy: true,
          bookmarks: [],
          is_immutable: false,
          insertions: 0,
          deletions: 0,
          on_target_only: false,
        },
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

function jjFile(path: string, statusCode = "M") {
  return {
    path,
    status: statusCode,
    changed_line_count: 1,
    diff_deferred: false,
  };
}

async function openReviewTab() {
  const user = userEvent.setup();
  const tab = await screen.findByRole("tab", { name: /changes/i });
  await user.click(tab);
  await screen.findByTestId("changes-viewer");
  return user;
}

function reviewPill() {
  const tab = screen.getByRole("tab", { name: /changes/i });
  return tab.querySelector('[data-testid="review-change-count"]');
}

describe("ShowWorkspace Changes tab pill colors", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    onChangedFilesChange = undefined;
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

  it("shows a yellow pill for uncommitted changes", async () => {
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(
      status({ has_changes: true }),
    );
    vi.mocked(api.getWorkspaceDiff).mockResolvedValue({
      uncommitted_files: [jjFile("a.txt")],
      committed_files: [],
      hunks_by_file: [],
      too_large_to_render: false,
      render_block_reason: null,
    });

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await openReviewTab();
    await waitFor(() => expect(onChangedFilesChange).toBeDefined());

    onChangedFilesChange?.([
      {
        path: "a.txt",
        stagedStatus: " ",
        workspaceStatus: "M",
      },
    ]);

    await waitFor(() => {
      const pill = reviewPill();
      expect(pill).toBeTruthy();
      expect(pill).toHaveTextContent("1");
      expect(pill?.className).toContain("bg-yellow-500");
      expect(pill?.className).not.toContain("bg-destructive");
      expect(pill?.className).not.toContain("bg-muted");
    });
  });

  it("shows a grey pill for committed changes only", async () => {
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(
      status({
        has_changes: false,
        commits_ahead_of_target: [
          {
            hash: "abc",
            timestamp: new Date().toISOString(),
            message: "feat",
          },
        ],
      }),
    );
    vi.mocked(api.getWorkspaceDiff).mockResolvedValue({
      uncommitted_files: [],
      committed_files: [jjFile("committed.txt")],
      hunks_by_file: [],
      too_large_to_render: false,
      render_block_reason: null,
    });

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await waitFor(() => {
      const pill = reviewPill();
      expect(pill).toBeTruthy();
      expect(pill).toHaveTextContent("1");
      expect(pill?.className).toContain("bg-muted");
      expect(pill?.className).not.toContain("bg-yellow-500");
      expect(pill?.className).not.toContain("bg-destructive");
    });
  });

  it("shows a red pill when there are conflicts", async () => {
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(
      status({
        has_conflicts: true,
        has_changes: true,
        conflicted_files: ["shared.txt"],
        commits_ahead_of_target: [
          {
            hash: "abc",
            timestamp: new Date().toISOString(),
            message: "feat",
          },
        ],
      }),
    );
    vi.mocked(api.getWorkspaceDiff).mockResolvedValue({
      uncommitted_files: [jjFile("shared.txt")],
      committed_files: [],
      hunks_by_file: [],
      too_large_to_render: false,
      render_block_reason: null,
    });

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await openReviewTab();
    await waitFor(() => expect(onChangedFilesChange).toBeDefined());

    onChangedFilesChange?.([
      {
        path: "shared.txt",
        stagedStatus: " ",
        workspaceStatus: "M",
      },
    ]);

    await waitFor(() => {
      const pill = reviewPill();
      expect(pill).toBeTruthy();
      expect(pill?.className).toContain("bg-destructive/20");
      expect(pill?.className).toContain("text-destructive");
      expect(pill?.className).not.toContain("bg-yellow-500");
    });
  });

  it("uses the stable unique review change count for the number", async () => {
    vi.mocked(api.getWorkspaceStatus).mockResolvedValue(
      status({
        has_changes: false,
        commits_ahead_of_target: [
          {
            hash: "abc",
            timestamp: new Date().toISOString(),
            message: "feat",
          },
        ],
      }),
    );
    vi.mocked(api.getWorkspaceDiff).mockResolvedValue({
      uncommitted_files: [],
      committed_files: [jjFile("a.ts"), jjFile("b.ts", "A"), jjFile("c.ts")],
      hunks_by_file: [],
      too_large_to_render: false,
      render_block_reason: null,
    });

    render(
      <ShowWorkspace
        repositoryPath={workspace.repo_path}
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );

    await waitFor(() => {
      const pill = reviewPill();
      expect(pill).toHaveTextContent("3");
      expect(pill?.className).toContain("bg-muted");
    });
  });
});
